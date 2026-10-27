//! A window's furniture: `split` panes with a divider to drag, the
//! `sidebar_layout` Settings and Files use (with `sidebar_item` rows),
//! the `toolbar` under a window's top edge, the `status_bar`, and
//! settings forms (`form_section` of `form_row`s).

use std::marker::PhantomData;
use std::sync::Arc;

use sidestep_ui::{Cursor, PointerButton};

use crate::draw;
use crate::element::Layoutable;
use crate::element::{Common, Element, MsgFn, MsgMap, RawElement};
use crate::hash::PaintHasher;
use crate::icons::Icon;
use crate::layout::{Align, Constraints, Length, Padding, Point, Rect, Size};
use crate::text::{TextSpec, text};
use crate::theme::{FontSpec, Role, Theme};
use crate::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use crate::widgets::layout::{Axis, Tone, container, flex_layout};

// Splits.

/// Where a split's divider starts.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Start {
    /// This share of the room to the first pane.
    Ratio(f64),
    /// The first pane this wide (or tall).
    First(f64),
}

/// Two panes with a divider between them that the pointer drags:
/// `split(list, preview).ratio(0.4)`.
pub struct Split<Msg> {
    pub(crate) common: Common,
    first: RawElement,
    second: RawElement,
    axis: Axis,
    start: Start,
    min: (f64, f64),
    on_resize: Option<MsgMap<f64>>,
    first_tone: Option<Tone>,
    resizable: bool,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Split);

pub fn split<Msg>(first: impl Into<Element<Msg>>, second: impl Into<Element<Msg>>) -> Split<Msg> {
    Split {
        common: Common::sized(Length::Fill, Length::Fill),
        first: first.into().raw,
        second: second.into().raw,
        axis: Axis::Horizontal,
        start: Start::Ratio(0.5),
        min: (80.0, 80.0),
        on_resize: None,
        first_tone: None,
        resizable: true,
        _m: PhantomData,
    }
}

/// A sidebar and the content beside it, as Settings and Files have:
/// the sidebar on `sunk`, 200pt wide (draggable), edged in `line`.
pub fn sidebar_layout<Msg>(sidebar: impl Into<Element<Msg>>, content: impl Into<Element<Msg>>) -> Split<Msg> {
    Split { start: Start::First(-1.0), min: (140.0, 240.0), first_tone: Some(Tone::Sunk), ..split(sidebar, content) }
}

impl<Msg: Send + 'static> Split<Msg> {
    /// Sent with the first pane's new size when the divider is dragged.
    pub fn on_resize(mut self, f: impl Fn(f64) -> Msg + 'static) -> Self {
        self.on_resize = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> Split<Msg> {
    /// One above the other.
    pub fn vertical(mut self) -> Self {
        self.axis = Axis::Vertical;
        self
    }

    /// The first pane's share to start with.
    pub fn ratio(mut self, r: f64) -> Self {
        self.start = Start::Ratio(r.clamp(0.0, 1.0));
        self
    }

    /// The first pane's size to start with (or to keep, if the program
    /// keeps it through `on_resize`).
    pub fn first_size(mut self, points: f64) -> Self {
        self.start = Start::First(points);
        self
    }

    /// The least each pane may have.
    pub fn min_sizes(mut self, first: f64, second: f64) -> Self {
        self.min = (first, second);
        self
    }

    /// Whether the divider drags (it does by default).
    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }
}

impl<Msg> From<Split<Msg>> for Element<Msg> {
    fn from(s: Split<Msg>) -> Element<Msg> {
        let w = SplitWidget {
            axis: s.axis,
            start: s.start,
            min: s.min,
            on_resize: s.on_resize,
            first_tone: s.first_tone,
            resizable: s.resizable,
            at: 0.0,
        };
        Element::from_raw(RawElement { widget: Box::new(w), common: s.common, children: vec![s.first, s.second] })
    }
}

#[derive(Default)]
struct SplitState {
    /// Where the user put the divider.
    at: Option<f64>,
    dragging: Option<f64>,
    near: bool,
}

struct SplitWidget {
    axis: Axis,
    start: Start,
    min: (f64, f64),
    on_resize: Option<MsgMap<f64>>,
    first_tone: Option<Tone>,
    resizable: bool,
    /// The divider's place, as laid out.
    at: f64,
}

const GRAB: f64 = 4.0;

impl SplitWidget {
    fn along(&self, p: Point) -> f64 {
        if self.axis == Axis::Horizontal { p.x } else { p.y }
    }

    fn near(&self, p: Point) -> bool {
        self.resizable && (self.along(p) - self.at - 0.5).abs() <= GRAB
    }

    fn divider(&self, size: Size) -> Rect {
        match self.axis {
            Axis::Horizontal => Rect::new(self.at, 0.0, self.at + 1.0, size.height),
            Axis::Vertical => Rect::new(0.0, self.at, size.width, self.at + 1.0),
        }
    }

    fn cursor_for(&self) -> Cursor {
        if self.axis == Axis::Horizontal { Cursor::ColResize } else { Cursor::RowResize }
    }
}

impl Widget for SplitWidget {
    fn name(&self) -> &'static str {
        "split"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 600.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 400.0 },
        );
        let total = if self.axis == Axis::Horizontal { size.width } else { size.height };
        let sidebar = cx.theme().metrics.sidebar;
        let wanted = cx.state::<SplitState>().at.unwrap_or(match self.start {
            Start::Ratio(r) => (total * r).round(),
            Start::First(p) if p < 0.0 => sidebar,
            Start::First(p) => p,
        });
        let hi = (total - 1.0 - self.min.1).max(self.min.0.min(total));
        let at = wanted.clamp(self.min.0.min(hi), hi).round();
        self.at = at;
        let rest = (total - at - 1.0).max(0.0);
        let (a, b) = match self.axis {
            Axis::Horizontal => (Size::new(at, size.height), Size::new(rest, size.height)),
            Axis::Vertical => (Size::new(size.width, at), Size::new(size.width, rest)),
        };
        cx.layout_child(0, Constraints::tight(a));
        cx.layout_child(1, Constraints::tight(b));
        cx.place_child(0, Point::ZERO);
        cx.place_child(
            1,
            match self.axis {
                Axis::Horizontal => Point::new(at + 1.0, 0.0),
                Axis::Vertical => Point::new(0.0, at + 1.0),
            },
        );
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let size = cx.size();
        if let Some(tone) = self.first_tone {
            let r = match self.axis {
                Axis::Horizontal => Rect::new(0.0, 0.0, self.at, size.height),
                Axis::Vertical => Rect::new(0.0, 0.0, size.width, self.at),
            };
            cx.canvas.fill_rect(r, tone.color(t));
        }
        let dragging = cx.state::<SplitState>().is_some_and(|s| s.dragging.is_some() || s.near);
        cx.canvas.fill_rect(self.divider(size), if dragging { t.focus() } else { t.line() });
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.at).add(&(self.first_tone.is_some(), self.axis));
    }

    fn hit(&self) -> bool {
        self.resizable
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.resizable {
            return Status::Ignored;
        }
        match event {
            Event::PointerMove { pos, .. } => {
                if let Some(grab) = cx.state::<SplitState>().dragging {
                    let at = self.along(*pos) - grab;
                    cx.state::<SplitState>().at = Some(at);
                    cx.relayout();
                    cx.invalidate();
                    if let Some(f) = &self.on_resize {
                        cx.emit_any(f.make(at));
                    }
                    return Status::Handled;
                }
                let near = self.near(*pos);
                if near != cx.state::<SplitState>().near {
                    cx.state::<SplitState>().near = near;
                    let d = self.divider(cx.size());
                    cx.invalidate_rect(d);
                    cx.set_cursor(if near { self.cursor_for() } else { Cursor::Default });
                }
                Status::Ignored
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } if self.near(*pos) => {
                let grab = self.along(*pos) - self.at;
                cx.state::<SplitState>().dragging = Some(grab);
                cx.capture_pointer();
                Status::Handled
            }
            Event::PointerUp { .. } if cx.state::<SplitState>().dragging.is_some() => {
                cx.state::<SplitState>().dragging = None;
                cx.release_pointer();
                cx.invalidate();
                Status::Handled
            }
            Event::PointerLeave => {
                if std::mem::take(&mut cx.state::<SplitState>().near) {
                    let d = self.divider(cx.size());
                    cx.invalidate_rect(d);
                    cx.set_cursor(Cursor::Default);
                }
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Splitter, range: Some((0.0, 1.0, self.at)), ..Default::default() })
    }
}

// Sidebars.

/// A row of a sidebar: an icon and a label, `selection`-filled when it's
/// the one showing: `sidebar_item(Icon::Wifi, "Network", self.pane == Pane::Network).on_press(Msg::Pane(Pane::Network))`.
pub struct SidebarItem<Msg> {
    pub(crate) common: Common,
    icon: Option<Icon>,
    label: Arc<str>,
    selected: bool,
    detail: Option<Arc<str>>,
    on_press: Option<MsgFn>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(SidebarItem);

pub fn sidebar_item<Msg>(
    icon: impl Into<Option<Icon>>,
    label: impl Into<Arc<str>>,
    selected: bool,
) -> SidebarItem<Msg> {
    SidebarItem {
        common: Common::sized(Length::Fill, Length::Shrink),
        icon: icon.into(),
        label: label.into(),
        selected,
        detail: None,
        on_press: None,
        _m: PhantomData,
    }
}

impl<Msg: Clone + Send + 'static> SidebarItem<Msg> {
    pub fn on_press(mut self, msg: Msg) -> Self {
        self.on_press = Some(MsgFn::new(msg));
        self
    }
}

impl<Msg> SidebarItem<Msg> {
    /// Quiet text at the right end (a count, a state).
    pub fn detail(mut self, d: impl Into<Arc<str>>) -> Self {
        self.detail = Some(d.into());
        self
    }
}

impl<Msg> From<SidebarItem<Msg>> for Element<Msg> {
    fn from(s: SidebarItem<Msg>) -> Element<Msg> {
        let w = SidebarItemWidget {
            icon: s.icon,
            label: s.label,
            selected: s.selected,
            detail: s.detail,
            on_press: s.on_press,
        };
        Element::from_raw(RawElement::new(w, s.common, Vec::new()))
    }
}

struct SidebarItemWidget {
    icon: Option<Icon>,
    label: Arc<str>,
    selected: bool,
    detail: Option<Arc<str>>,
    on_press: Option<MsgFn>,
}

impl Widget for SidebarItemWidget {
    fn name(&self) -> &'static str {
        "sidebar-item"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let w = cx.measure(&self.label, &TextSpec::new(FontSpec::ui(t.metrics.body), t.text())).width + 48.0;
        bc.constrain(Size::new(if bc.max.width.is_finite() { bc.max.width } else { w }, t.metrics.row + 4.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let fg = if self.selected {
            cx.canvas.fill_rect(r, t.selection());
            t.on_selection()
        } else {
            if cx.is_hovered() {
                cx.canvas.fill_rect(r, t.hover(t.sunk()));
            }
            t.text()
        };
        let mut x = 10.0;
        if let Some(i) = self.icon {
            let c = if self.selected { fg } else { t.text2() };
            i.draw(cx.canvas, Point::new(x, ((r.height() - 16.0) / 2.0).round()), 16.0, c);
            x += 24.0;
        }
        let mut right = r.x1 - 10.0;
        if let Some(d) = &self.detail {
            let spec = TextSpec::new(
                FontSpec { tabular: true, ..FontSpec::mono(t.metrics.small) },
                if self.selected { fg } else { t.text3() },
            );
            let w = cx.draw_text_in(d, &spec, Rect::new(x, 0.0, right, r.y1), Align::End);
            right -= w + 8.0;
        }
        let spec = TextSpec::new(
            FontSpec { weight: if self.selected { 600.0 } else { 400.0 }, ..FontSpec::ui(t.metrics.body) },
            fg,
        );
        cx.draw_text_in(&self.label, &spec, Rect::new(x, 0.0, right, r.y1), Align::Start);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.label).add(&(self.icon, self.selected, self.detail.as_deref()));
    }

    fn hit(&self) -> bool {
        self.on_press.is_some()
    }

    fn focusable(&self) -> Focusable {
        if self.on_press.is_some() { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match (event, &self.on_press) {
            (Event::PointerDown { button: PointerButton::Left, .. }, Some(m)) => {
                cx.emit(m);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        match &self.on_press {
            Some(m) => {
                cx.emit(m);
                true
            }
            None => false,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Tab,
            label: Some(self.label.to_string()),
            selected: Some(self.selected),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.label)
    }
}

/// A section's name in a sidebar or a form: IBM Plex Mono, upper case,
/// spaced, `text3`.
pub fn caption<Msg>(title: impl Into<Arc<str>>) -> Element<Msg> {
    let t: Arc<str> = title.into();
    container(text(t).mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3))
        .padding([10.0, 10.0, 4.0, 10.0])
        .into()
}

// Toolbars and status bars.

/// The bar under a window's top edge (`chrome`, a `bevel_lo` bottom edge,
/// 30pt): `toolbar(vec![dropdown(...).compact().into(), toolbar_separator(), button("B").toolbar().into(), spacer().into(), status_text("Ln 48, Col 12")])`.
pub fn toolbar<Msg>(items: Vec<Element<Msg>>) -> Element<Msg> {
    let row: Element<Msg> = Element::from_raw(RawElement {
        widget: Box::new(crate::widgets::layout::FlexWidget {
            axis: Axis::Horizontal,
            spacing: 4.0,
            padding: Padding::symmetric(0.0, 8.0),
            align: Align::Center,
        }),
        common: Common::sized(Length::Fill, Length::Fill),
        children: items.into_iter().map(|e| e.raw).collect(),
    });
    let mut common = Common::sized(Length::Fill, Length::Shrink);
    common.key = Some(crate::hash::hash_of("toolbar"));
    Element::from_raw(RawElement { widget: Box::new(Toolbar), common, children: vec![row.raw] })
}

/// The toolbar's frame, sized by the theme.
struct Toolbar;

impl Widget for Toolbar {
    fn name(&self) -> &'static str {
        "toolbar"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let h = cx.theme().metrics.toolbar;
        let size = bc.constrain(Size::new(bc.max.width, h));
        cx.layout_child(0, Constraints::tight(Size::new(size.width, h - 1.0)));
        cx.place_child(0, Point::new(0.0, 0.0));
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        cx.canvas.fill_rect(r, t.chrome());
        draw::bottom(cx.canvas, r, t.bevel_lo(), 1.0);
        draw::top(cx.canvas, r, t.bevel_hi(), 1.0);
    }

    fn solid(&self) -> bool {
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Toolbar, ..Default::default() })
    }
}

/// A short rule between groups of a toolbar's items.
pub fn toolbar_separator<Msg>() -> Element<Msg> {
    Element::from_raw(RawElement::new(Rule, Common::default(), Vec::new()))
}

struct Rule;

impl Widget for Rule {
    fn name(&self) -> &'static str {
        "toolbar-separator"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(13.0, 16.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        cx.canvas.fill_rect(Rect::new(6.0, 0.0, 7.0, 16.0), t.line());
    }
}

/// Status text, as a toolbar's or a status bar's: IBM Plex Mono 11 in
/// `text3`, digits tabular.
pub fn status_text<Msg>(s: impl Into<Arc<str>>) -> Element<Msg> {
    text(s).mono().tabular().size(11.0).role(Role::Text3).into()
}

/// The bar along a window's bottom: `chrome`, a `line` top edge, 22pt.
pub fn status_bar<Msg>(items: Vec<Element<Msg>>) -> Element<Msg> {
    let row: Element<Msg> = Element::from_raw(RawElement {
        widget: Box::new(crate::widgets::layout::FlexWidget {
            axis: Axis::Horizontal,
            spacing: 12.0,
            padding: Padding::symmetric(0.0, 10.0),
            align: Align::Center,
        }),
        common: Common::sized(Length::Fill, Length::Fill),
        children: items.into_iter().map(|e| e.raw).collect(),
    });
    Element::from_raw(RawElement {
        widget: Box::new(StatusBar),
        common: Common::sized(Length::Fill, Length::Shrink),
        children: vec![row.raw],
    })
}

struct StatusBar;

impl Widget for StatusBar {
    fn name(&self) -> &'static str {
        "status-bar"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let h = cx.theme().metrics.status_bar;
        let size = bc.constrain(Size::new(bc.max.width, h));
        cx.layout_child(0, Constraints::tight(Size::new(size.width, h - 1.0)));
        cx.place_child(0, Point::new(0.0, 1.0));
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        cx.canvas.fill_rect(r, t.chrome());
        draw::top(cx.canvas, r, t.line(), 1.0);
    }

    fn solid(&self) -> bool {
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::StatusBar, ..Default::default() })
    }
}

// Settings forms.

/// A group of settings under a caption, in a box with rules between its
/// rows: `form_section("Wi-Fi", vec![form_row("Wi-Fi", toggle(on)).into(), …])`.
pub fn form_section<Msg>(title: impl Into<Arc<str>>, rows: Vec<Element<Msg>>) -> Element<Msg> {
    let title: Arc<str> = title.into();
    let body = Element::from_raw(RawElement {
        widget: Box::new(FormBox { rules: Vec::new() }),
        common: Common::sized(Length::Fill, Length::Shrink),
        children: rows.into_iter().map(|e| e.raw).collect(),
    });
    let mut items = Vec::new();
    if !title.is_empty() {
        items.push(
            container(text(title).mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3))
                .padding([0.0, 2.0, 6.0, 2.0])
                .into(),
        );
    }
    items.push(body);
    crate::widgets::layout::column(items).width(Length::Fill).into()
}

/// The box of a form section: `chrome`, a `line` edge with the top
/// highlight, rules between rows (inset from the left, as macOS's).
struct FormBox {
    rules: Vec<f64>,
}

impl Widget for FormBox {
    fn name(&self) -> &'static str {
        "form-section"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = flex_layout(cx, bc, Axis::Vertical, 0.0, Padding::all(1.0), Align::Start);
        let mut y = 1.0;
        self.rules.clear();
        for i in 0..cx.child_count() {
            y += cx.child_size(i).height;
            if i + 1 < cx.child_count() {
                self.rules.push(y);
            }
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        cx.canvas.fill_rect(r, t.chrome());
        draw::border(cx.canvas, r, t.line(), 1.0);
        draw::top(cx.canvas, r.inset(-1.0), t.bevel_hi(), 1.0);
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let w = cx.size().width;
        for &y in &self.rules {
            cx.canvas.fill_rect(Rect::new(ROW_PAD, y - 1.0, w - 1.0, y), t.line());
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for r in &self.rules {
            h.f64(*r);
        }
    }

    fn solid(&self) -> bool {
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Group, ..Default::default() })
    }
}

/// A setting: its label at the left in `text2` (and a description under
/// it), its control at the right.
pub struct FormRow<Msg> {
    pub(crate) common: Common,
    label: Arc<str>,
    description: Option<Arc<str>>,
    icon: Option<Icon>,
    control: RawElement,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(FormRow);

pub fn form_row<Msg>(label: impl Into<Arc<str>>, control: impl Into<Element<Msg>>) -> FormRow<Msg> {
    FormRow {
        common: Common::sized(Length::Fill, Length::Shrink),
        label: label.into(),
        description: None,
        icon: None,
        control: control.into().raw,
        _m: PhantomData,
    }
}

impl<Msg> FormRow<Msg> {
    /// A line or two under the label, in `text3`.
    pub fn description(mut self, d: impl Into<Arc<str>>) -> Self {
        self.description = Some(d.into());
        self
    }

    pub fn icon(mut self, i: Icon) -> Self {
        self.icon = Some(i);
        self
    }
}

impl<Msg> From<FormRow<Msg>> for Element<Msg> {
    fn from(r: FormRow<Msg>) -> Element<Msg> {
        let w = FormRowWidget { label: r.label, description: r.description, icon: r.icon, label_size: Size::ZERO };
        Element::from_raw(RawElement { widget: Box::new(w), common: r.common, children: vec![r.control] })
    }
}

struct FormRowWidget {
    label: Arc<str>,
    description: Option<Arc<str>>,
    icon: Option<Icon>,
    label_size: Size,
}

const ROW_PAD: f64 = 12.0;

impl FormRowWidget {
    fn label_spec(t: &Theme) -> TextSpec {
        TextSpec::new(FontSpec::ui(t.metrics.body), t.text2())
    }

    fn desc_spec(t: &Theme) -> TextSpec {
        TextSpec { wrap: sidestep_ui::Wrap::Words, ..TextSpec::new(FontSpec::ui(t.metrics.small), t.text3()) }
    }

    fn label_x(&self) -> f64 {
        ROW_PAD + if self.icon.is_some() { 26.0 } else { 0.0 }
    }
}

impl Widget for FormRowWidget {
    fn name(&self) -> &'static str {
        "form-row"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let width = if bc.max.width.is_finite() { bc.max.width } else { 480.0 };
        // The control at its own size, the label in what's left.
        let control = cx.layout_child(0, Constraints::loose(Size::new((width - 2.0 * ROW_PAD) * 0.62, f64::INFINITY)));
        let room = (width - control.width - self.label_x() - ROW_PAD - 16.0).max(40.0);
        let label = cx.measure(&self.label, &FormRowWidget::label_spec(t));
        let desc = match &self.description {
            Some(d) => cx.env.text.layout(t, d, &FormRowWidget::desc_spec(t), Some(room)).size(),
            None => Size::ZERO,
        };
        let text_h = label.height + if desc.height > 0.0 { desc.height + 2.0 } else { 0.0 };
        let h = (text_h + 16.0).max(control.height + 12.0).max(t.metrics.form_row).ceil();
        self.label_size = Size::new(room, text_h);
        cx.place_child(0, Point::new((width - ROW_PAD - control.width).round(), ((h - control.height) / 2.0).round()));
        bc.constrain(Size::new(width, h))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let size = cx.size();
        let x = self.label_x();
        let top = ((size.height - self.label_size.height) / 2.0).round();
        if let Some(i) = self.icon {
            i.draw(cx.canvas, Point::new(ROW_PAD, ((size.height - 16.0) / 2.0).round()), 16.0, t.text2());
        }
        let label = cx.layout_text(&self.label, &FormRowWidget::label_spec(t), None);
        let lh = label.size().height;
        cx.canvas.draw_text(&label, Point::new(x, top));
        if let Some(d) = &self.description {
            let desc = cx.layout_text(d, &FormRowWidget::desc_spec(t), Some(self.label_size.width));
            cx.canvas.draw_text(&desc, Point::new(x, top + lh + 2.0));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.label).add(&(self.description.as_deref(), self.icon)).f64(self.label_size.width);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Group, label: Some(self.label.to_string()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.label)
    }
}
