//! Layout elements: `column!`, `row!`, `stack!`, `container`, `spacer`,
//! `divider`.

use std::marker::PhantomData;

use sidestep_ui::Color;

use crate::draw;
use crate::element::{Common, Element, RawElement};
use crate::hash::PaintHasher;
use crate::layout::{Align, Constraints, Length, Padding, Point, Rect, Size};
use crate::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};

/// Children side by side (`row!`) or one above another (`column!`).
pub struct Flex<Msg> {
    pub(crate) common: Common,
    axis: Axis,
    spacing: f64,
    padding: Padding,
    align: Align,
    children: Vec<RawElement>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Flex);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Children one above another.
pub fn column<Msg>(children: Vec<Element<Msg>>) -> Flex<Msg> {
    Flex::new(Axis::Vertical, children)
}

/// Children side by side, centred across.
pub fn row<Msg>(children: Vec<Element<Msg>>) -> Flex<Msg> {
    Flex::new(Axis::Horizontal, children).align(Align::Center)
}

impl<Msg> Flex<Msg> {
    fn new(axis: Axis, children: Vec<Element<Msg>>) -> Flex<Msg> {
        Flex {
            common: Common::default(),
            axis,
            spacing: 0.0,
            padding: Padding::ZERO,
            align: Align::Start,
            children: children.into_iter().map(|c| c.raw).collect(),
            _m: PhantomData,
        }
    }

    /// Space between children.
    pub fn spacing(mut self, points: f64) -> Self {
        self.spacing = points;
        self
    }

    pub fn padding(mut self, p: impl Into<Padding>) -> Self {
        self.padding = p.into();
        self
    }

    /// Where children sit across the axis.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn push(mut self, child: impl Into<Element<Msg>>) -> Self {
        self.children.push(child.into().raw);
        self
    }

    pub fn extend(mut self, children: impl IntoIterator<Item = Element<Msg>>) -> Self {
        self.children.extend(children.into_iter().map(|c| c.raw));
        self
    }
}

impl<Msg> From<Flex<Msg>> for Element<Msg> {
    fn from(f: Flex<Msg>) -> Element<Msg> {
        let w = FlexWidget { axis: f.axis, spacing: f.spacing, padding: f.padding, align: f.align };
        Element::from_raw(RawElement { widget: Box::new(w), common: f.common, children: f.children })
    }
}

pub(crate) struct FlexWidget {
    pub axis: Axis,
    pub spacing: f64,
    pub padding: Padding,
    pub align: Align,
}

impl FlexWidget {
    pub fn column(spacing: f64) -> FlexWidget {
        FlexWidget { axis: Axis::Vertical, spacing, padding: Padding::ZERO, align: Align::Start }
    }
}

/// (main, cross) of a size along an axis.
fn split(axis: Axis, s: Size) -> (f64, f64) {
    match axis {
        Axis::Horizontal => (s.width, s.height),
        Axis::Vertical => (s.height, s.width),
    }
}

fn join(axis: Axis, main: f64, cross: f64) -> Size {
    match axis {
        Axis::Horizontal => Size::new(main, cross),
        Axis::Vertical => Size::new(cross, main),
    }
}

fn constraints(axis: Axis, main: (f64, f64), cross: (f64, f64)) -> Constraints {
    Constraints { min: join(axis, main.0, cross.0), max: join(axis, main.1, cross.1) }
}

/// Lay children out along `axis`: fixed and shrinking ones first, then
/// the fills share what's left by their portions.
pub(crate) fn flex_layout(
    cx: &mut LayoutCx,
    bc: Constraints,
    axis: Axis,
    spacing: f64,
    padding: Padding,
    align: Align,
) -> Size {
    let inner = bc.shrink(padding);
    let n = cx.child_count();
    let (max_main, max_cross) = split(axis, inner.max);
    let (min_main, min_cross) = split(axis, inner.min);
    let gaps = spacing * n.saturating_sub(1) as f64;
    let portions: Vec<Option<u16>> = (0..n)
        .map(|i| {
            let flex = cx.tree.nodes[cx.tree.nodes[cx.node].children[i]].widget.flex();
            let c = cx.child_common(i);
            let len = match axis {
                Axis::Horizontal => c.width,
                Axis::Vertical => c.height,
            };
            flex.or(len.portion()).filter(|_| max_main.is_finite())
        })
        .collect();
    let cross_fill: Vec<bool> = (0..n)
        .map(|i| {
            let c = cx.child_common(i);
            match axis {
                Axis::Horizontal => c.height.is_fill(),
                Axis::Vertical => c.width.is_fill(),
            }
        })
        .collect();
    let mut sizes = vec![Size::ZERO; n];
    let mut used = gaps;
    let mut cross = 0.0f64;
    for i in 0..n {
        if portions[i].is_some() {
            continue;
        }
        let room = (max_main - used).max(0.0);
        let s = cx.layout_child(i, constraints(axis, (0.0, room), (0.0, max_cross)));
        let (m, c) = split(axis, s);
        used += m;
        cross = cross.max(c);
        sizes[i] = s;
    }
    let total: u32 = portions.iter().flatten().map(|p| u32::from(*p)).sum();
    if total > 0 {
        let left = (max_main - used).max(0.0);
        let mut given = 0.0;
        let mut seen = 0u32;
        for i in 0..n {
            let Some(p) = portions[i] else { continue };
            seen += u32::from(p);
            // Whole points, the last taking what rounding left.
            let end = (left * f64::from(seen) / f64::from(total)).round();
            let share = (end - given).max(0.0);
            given = end;
            let s = cx.layout_child(i, constraints(axis, (share, share), (0.0, max_cross)));
            let (m, c) = split(axis, s);
            used += m;
            cross = cross.max(c);
            sizes[i] = s;
        }
    }
    let cross_tight = min_cross >= max_cross && max_cross.is_finite();
    let cross_size = if cross_tight { max_cross } else { cross.max(min_cross) };
    // Children that fill across take the row's height (the column's width).
    for i in 0..n {
        let (m, c) = split(axis, sizes[i]);
        if cross_fill[i] && c != cross_size {
            sizes[i] = cx.layout_child(i, constraints(axis, (m, m), (cross_size, cross_size)));
        }
    }
    let main_size = used.max(min_main);
    let mut at = 0.0;
    for (i, s) in sizes.iter().enumerate() {
        let (m, c) = split(axis, *s);
        let off = align.offset(c, cross_size);
        let p = match axis {
            Axis::Horizontal => Point::new(padding.left + at, padding.top + off),
            Axis::Vertical => Point::new(padding.left + off, padding.top + at),
        };
        cx.place_child(i, p);
        at += m + spacing;
    }
    let content = join(axis, main_size, cross_size);
    bc.constrain(Size::new(content.width + padding.horizontal(), content.height + padding.vertical()))
}

impl Widget for FlexWidget {
    fn name(&self) -> &'static str {
        match self.axis {
            Axis::Horizontal => "row",
            Axis::Vertical => "column",
        }
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        flex_layout(cx, bc, self.axis, self.spacing, self.padding, self.align)
    }
}

/// Children over one another, the last on top: `stack![content, badge]`.
pub struct Stack<Msg> {
    pub(crate) common: Common,
    align_x: Align,
    align_y: Align,
    children: Vec<RawElement>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Stack);

pub fn stack<Msg>(children: Vec<Element<Msg>>) -> Stack<Msg> {
    Stack {
        common: Common::default(),
        align_x: Align::Start,
        align_y: Align::Start,
        children: children.into_iter().map(|c| c.raw).collect(),
        _m: PhantomData,
    }
}

impl<Msg> Stack<Msg> {
    pub fn align_x(mut self, a: Align) -> Self {
        self.align_x = a;
        self
    }

    pub fn align_y(mut self, a: Align) -> Self {
        self.align_y = a;
        self
    }
}

impl<Msg> From<Stack<Msg>> for Element<Msg> {
    fn from(s: Stack<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement {
            widget: Box::new(StackWidget { align_x: s.align_x, align_y: s.align_y }),
            common: s.common,
            children: s.children,
        })
    }
}

pub(crate) struct StackWidget {
    pub align_x: Align,
    pub align_y: Align,
}

impl Widget for StackWidget {
    fn name(&self) -> &'static str {
        "stack"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        stack_layout(cx, bc, self.align_x, self.align_y)
    }
}

/// Lay every child out in `bc`; the size is the largest (or `bc`'s if
/// tight), fills take it all.
pub(crate) fn stack_layout(cx: &mut LayoutCx, bc: Constraints, ax: Align, ay: Align) -> Size {
    let n = cx.child_count();
    let mut size = bc.min;
    for i in 0..n {
        let c = cx.child_common(i);
        if c.width.is_fill() && c.height.is_fill() && bc.max.width.is_finite() && bc.max.height.is_finite() {
            continue;
        }
        let s = cx.layout_child(i, bc.loosen());
        size = Size::new(size.width.max(s.width), size.height.max(s.height));
    }
    let size = bc.constrain(size);
    for i in 0..n {
        let s = cx.layout_child(i, Constraints::loose(size));
        cx.place_child(i, Point::new(ax.offset(s.width, size.width), ay.offset(s.height, size.height)));
    }
    size
}

/// A fill for a container: a theme token or a colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Surface,
    Chrome,
    Raised,
    Sunk,
    Bar,
    Selection,
    Focus,
    MenuTitle,
    Color(Color),
}

impl Tone {
    pub(crate) fn color(self, t: &crate::theme::Theme) -> Color {
        match self {
            Tone::Surface => t.surface(),
            Tone::Chrome => t.chrome(),
            Tone::Raised => t.raised(),
            Tone::Sunk => t.sunk(),
            Tone::Bar => t.bar(),
            Tone::Selection => t.selection(),
            Tone::Focus => t.focus(),
            Tone::MenuTitle => t.menu_title(),
            Tone::Color(c) => c,
        }
    }
}

/// An edge's colour: a theme token or a colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Line,
    Line2,
    BevelLo,
    Focus,
    Color(Color),
}

impl Edge {
    pub(crate) fn color(self, t: &crate::theme::Theme) -> Color {
        match self {
            Edge::Line => t.line(),
            Edge::Line2 => t.line2(),
            Edge::BevelLo => t.bevel_lo(),
            Edge::Focus => t.focus(),
            Edge::Color(c) => c,
        }
    }
}

/// Which sides an edge runs along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sides {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

impl Sides {
    pub const ALL: Sides = Sides { top: true, right: true, bottom: true, left: true };
    pub const TOP: Sides = Sides { top: true, right: false, bottom: false, left: false };
    pub const BOTTOM: Sides = Sides { top: false, right: false, bottom: true, left: false };
    pub const LEFT: Sides = Sides { top: false, right: false, bottom: false, left: true };
    pub const RIGHT: Sides = Sides { top: false, right: true, bottom: false, left: false };
}

/// One child with padding, a fill, edges, a bevel, a hard shadow, and
/// alignment: `container(text("Hi")).padding(8.0).fill(Tone::Chrome)`.
pub struct Container<Msg> {
    pub(crate) common: Common,
    style: ContainerStyle,
    child: Option<RawElement>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Container);

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct ContainerStyle {
    pub padding: Padding,
    pub fill: Option<Tone>,
    pub edge: Option<(Edge, Sides)>,
    pub bevel: bool,
    pub floating: bool,
    pub align_x: Align,
    pub align_y: Align,
    pub clip: bool,
    pub role: Option<SemanticRole>,
}

pub fn container<Msg>(child: impl Into<Element<Msg>>) -> Container<Msg> {
    Container {
        common: Common::default(),
        style: ContainerStyle::default(),
        child: Some(child.into().raw),
        _m: PhantomData,
    }
}

impl<Msg> Container<Msg> {
    pub fn padding(mut self, p: impl Into<Padding>) -> Self {
        self.style.padding = p.into();
        self
    }

    pub fn fill(mut self, fill: Tone) -> Self {
        self.style.fill = Some(fill);
        self
    }

    /// A 1pt edge all round.
    pub fn border(mut self, edge: Edge) -> Self {
        self.style.edge = Some((edge, Sides::ALL));
        self
    }

    /// A 1pt edge along some sides.
    pub fn edges(mut self, edge: Edge, sides: Sides) -> Self {
        self.style.edge = Some((edge, sides));
        self
    }

    /// The 1pt `bevel_hi` highlight along the top inside.
    pub fn bevel(mut self) -> Self {
        self.style.bevel = true;
        self
    }

    /// A floating panel: raised, a `bevel_lo` edge, the highlight and the
    /// hard shadow.
    pub fn floating(mut self) -> Self {
        self.style.floating = true;
        self
    }

    pub fn align_x(mut self, a: Align) -> Self {
        self.style.align_x = a;
        self
    }

    pub fn align_y(mut self, a: Align) -> Self {
        self.style.align_y = a;
        self
    }

    /// Centre the child both ways.
    pub fn center(self) -> Self {
        self.align_x(Align::Center).align_y(Align::Center)
    }

    /// Clip the child to the container.
    pub fn clip(mut self) -> Self {
        self.style.clip = true;
        self
    }

    /// What assistive technology calls the group.
    pub fn group_role(mut self, role: SemanticRole) -> Self {
        self.style.role = Some(role);
        self
    }
}

impl<Msg> From<Container<Msg>> for Element<Msg> {
    fn from(c: Container<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement {
            widget: Box::new(ContainerWidget { style: c.style }),
            common: c.common,
            children: c.child.into_iter().collect(),
        })
    }
}

pub(crate) struct ContainerWidget {
    pub style: ContainerStyle,
}

impl Widget for ContainerWidget {
    fn name(&self) -> &'static str {
        "container"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = &self.style;
        let inner = bc.shrink(s.padding);
        if cx.child_count() == 0 {
            return bc.constrain(s.padding.size());
        }
        let child = cx.layout_child(0, inner.loosen());
        let size = bc.constrain(Size::new(child.width + s.padding.horizontal(), child.height + s.padding.vertical()));
        let room = Size::new(size.width - s.padding.horizontal(), size.height - s.padding.vertical());
        cx.place_child(
            0,
            Point::new(
                s.padding.left + s.align_x.offset(child.width, room.width),
                s.padding.top + s.align_y.offset(child.height, room.height),
            ),
        );
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let s = &self.style;
        let t = cx.theme();
        let r = cx.bounds();
        if s.floating {
            draw::floating(cx.canvas, t, r);
            return;
        }
        if let Some(f) = s.fill {
            cx.canvas.fill_rect(r, f.color(t));
        }
        if let Some((edge, sides)) = s.edge {
            let c = edge.color(t);
            if sides == Sides::ALL {
                draw::border(cx.canvas, r, c, 1.0);
            } else {
                if sides.top {
                    draw::top(cx.canvas, r, c, 1.0);
                }
                if sides.bottom {
                    draw::bottom(cx.canvas, r, c, 1.0);
                }
                if sides.left {
                    draw::left(cx.canvas, r, c, 1.0);
                }
                if sides.right {
                    draw::right(cx.canvas, r, c, 1.0);
                }
            }
        }
        if s.bevel {
            let inset = if s.edge.is_some_and(|(_, sd)| sd.top) { 1.0 } else { 0.0 };
            let side = if s.edge.is_some_and(|(_, sd)| sd == Sides::ALL) { 1.0 } else { 0.0 };
            draw::top(cx.canvas, Rect::new(r.x0 + side, r.y0 + inset, r.x1 - side, r.y1), t.bevel_hi(), 1.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        let s = &self.style;
        h.add(&(s.fill.is_some(), s.bevel, s.floating, s.clip, s.edge.map(|(_, sd)| sd)));
        if let Some(f) = s.fill {
            h.add(&format!("{f:?}"));
        }
        if let Some((e, _)) = s.edge {
            h.add(&format!("{e:?}"));
        }
    }

    fn solid(&self) -> bool {
        self.style.fill.is_some() || self.style.floating
    }

    fn clips(&self) -> bool {
        self.style.clip
    }

    fn overflow(&self) -> f64 {
        if self.style.floating { 3.0 } else { 0.0 }
    }

    fn semantics(&self) -> Option<Semantics> {
        self.style.role.map(|role| Semantics { role, ..Default::default() })
    }
}

/// Room that stretches: in a row, it pushes what follows to the right.
pub struct Spacer<Msg> {
    pub(crate) common: Common,
    portion: u16,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Spacer);

pub fn spacer<Msg>() -> Spacer<Msg> {
    Spacer { common: Common::default(), portion: 1, _m: PhantomData }
}

/// Fixed room: `space(8.0)` is 8pt along a row or a column.
pub fn space<Msg>(points: f64) -> Element<Msg> {
    Element::from_raw(RawElement::new(Gap(points), Common::default(), Vec::new()))
}

impl<Msg> Spacer<Msg> {
    pub fn portion(mut self, n: u16) -> Self {
        self.portion = n.max(1);
        self
    }
}

impl<Msg> From<Spacer<Msg>> for Element<Msg> {
    fn from(s: Spacer<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement::new(SpacerWidget(s.portion), s.common, Vec::new()))
    }
}

struct SpacerWidget(u16);

impl Widget for SpacerWidget {
    fn name(&self) -> &'static str {
        "spacer"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.min
    }

    fn flex(&self) -> Option<u16> {
        Some(self.0)
    }
}

struct Gap(f64);

impl Widget for Gap {
    fn name(&self) -> &'static str {
        "space"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(self.0, self.0))
    }
}

/// A 1pt rule across (or, `.vertical()`, down).
pub struct Divider<Msg> {
    pub(crate) common: Common,
    vertical: bool,
    engraved: bool,
    edge: Edge,
    inset: f64,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Divider);

pub fn divider<Msg>() -> Divider<Msg> {
    Divider {
        common: Common::sized(Length::Fill, Length::Shrink),
        vertical: false,
        engraved: false,
        edge: Edge::Line,
        inset: 0.0,
        _m: PhantomData,
    }
}

impl<Msg> Divider<Msg> {
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self.common.width = Length::Shrink;
        self.common.height = Length::Fill;
        self
    }

    /// With the highlight beside it, as menus' separators are.
    pub fn engraved(mut self) -> Self {
        self.engraved = true;
        self
    }

    pub fn edge(mut self, e: Edge) -> Self {
        self.edge = e;
        self
    }

    /// Room at either end.
    pub fn inset(mut self, points: f64) -> Self {
        self.inset = points;
        self
    }
}

impl<Msg> From<Divider<Msg>> for Element<Msg> {
    fn from(d: Divider<Msg>) -> Element<Msg> {
        let w = DividerWidget { vertical: d.vertical, engraved: d.engraved, edge: d.edge, inset: d.inset };
        Element::from_raw(RawElement::new(w, d.common, Vec::new()))
    }
}

struct DividerWidget {
    vertical: bool,
    engraved: bool,
    edge: Edge,
    inset: f64,
}

impl Widget for DividerWidget {
    fn name(&self) -> &'static str {
        "divider"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let thick = if self.engraved { 2.0 } else { 1.0 };
        if self.vertical { bc.constrain(Size::new(thick, 0.0)) } else { bc.constrain(Size::new(0.0, thick)) }
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let c = self.edge.color(t);
        if self.vertical {
            let (y0, y1) = (r.y0 + self.inset, r.y1 - self.inset);
            let x = (r.x0 + (r.width() - if self.engraved { 2.0 } else { 1.0 }) / 2.0).round();
            cx.canvas.fill_rect(Rect::new(x, y0, x + 1.0, y1), c);
            if self.engraved {
                cx.canvas.fill_rect(Rect::new(x + 1.0, y0, x + 2.0, y1), t.bevel_hi());
            }
        } else {
            let (x0, x1) = (r.x0 + self.inset, r.x1 - self.inset);
            let y = (r.y0 + (r.height() - if self.engraved { 2.0 } else { 1.0 }) / 2.0).round();
            if self.engraved {
                draw::separator(cx.canvas, t, x0, x1, y);
            } else {
                cx.canvas.fill_rect(Rect::new(x0, y, x1, y + 1.0), c);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.vertical, self.engraved, format!("{:?}", self.edge))).f64(self.inset);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Splitter, ..Default::default() })
    }
}

/// Children one above another: `column![text("a"), button("b")]`.
#[macro_export]
macro_rules! column {
    () => { $crate::widgets::layout::column(Vec::new()) };
    ($($x:expr),+ $(,)?) => { $crate::widgets::layout::column(vec![$($crate::Element::from($x)),+]) };
}

/// Children side by side: `row![icon(Icon::Wifi), text("Wi-Fi"), spacer(), toggle(on)]`.
#[macro_export]
macro_rules! row {
    () => { $crate::widgets::layout::row(Vec::new()) };
    ($($x:expr),+ $(,)?) => { $crate::widgets::layout::row(vec![$($crate::Element::from($x)),+]) };
}

/// Children over one another, the last on top.
#[macro_export]
macro_rules! stack {
    () => { $crate::widgets::layout::stack(Vec::new()) };
    ($($x:expr),+ $(,)?) => { $crate::widgets::layout::stack(vec![$($crate::Element::from($x)),+]) };
}
