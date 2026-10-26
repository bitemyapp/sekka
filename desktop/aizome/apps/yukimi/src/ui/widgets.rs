//! Yukimi's own widgets, in Aizome's look, for what the kit doesn't have
//! (yet): a pressable row, bars of dye
//! colours, a wrapping grid, monograms and falling snow.
use std::cell::Cell;
use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::draw;
use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Align, Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::kurbo::{BezPath, Circle};
use aizome_kit::sidestep_ui::{Cursor, PointerButton, StrokeStyle};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Role, Theme};
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Color, Element};

// Pressable rows and cards.

/// How a [`tap`] looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TapStyle {
    /// A row in a list: hover shades it, selection fills it with `selection`.
    #[default]
    Row,
    /// A card: raised, edged, the top highlight; hover lifts it.
    Card,
}

/// Something pressable made of other elements: a sidebar place, a list
/// row, a card. Its content is laid out inside `padding`.
pub struct Tap<Msg> {
    content: Element<Msg>,
    on_press: Option<MsgFn>,
    selected: bool,
    style: TapStyle,
    padding: [f64; 4],
    label: Option<Arc<str>>,
}

pub fn tap<Msg>(content: impl Into<Element<Msg>>) -> Tap<Msg> {
    Tap {
        content: content.into(),
        on_press: None,
        selected: false,
        style: TapStyle::Row,
        padding: [0.0; 4],
        label: None,
    }
}

impl<Msg: Clone + Send + 'static> Tap<Msg> {
    pub fn on_press(mut self, msg: Msg) -> Self {
        self.on_press = Some(MsgFn::new(msg));
        self
    }
}

impl<Msg> Tap<Msg> {
    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }

    pub fn style(mut self, style: TapStyle) -> Self {
        self.style = style;
        self
    }

    /// `[vertical, horizontal]`.
    pub fn padding(mut self, [v, h]: [f64; 2]) -> Self {
        self.padding = [v, h, v, h];
        self
    }

    /// What assistive technology calls it.
    pub fn named(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl<Msg> From<Tap<Msg>> for Element<Msg> {
    fn from(t: Tap<Msg>) -> Element<Msg> {
        let w = TapWidget {
            on_press: t.on_press,
            selected: t.selected,
            style: t.style,
            padding: t.padding,
            label: t.label,
        };
        Element::new(w, vec![t.content])
    }
}

struct TapWidget {
    on_press: Option<MsgFn>,
    selected: bool,
    style: TapStyle,
    padding: [f64; 4],
    label: Option<Arc<str>>,
}

impl Widget for TapWidget {
    fn name(&self) -> &'static str {
        "tap"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let [top, right, bottom, left] = self.padding;
        let inner = bc.shrink(aizome_kit::Padding { top, right, bottom, left });
        let s = if cx.child_count() > 0 { cx.layout_child(0, inner) } else { Size::ZERO };
        if cx.child_count() > 0 {
            cx.place_child(0, Point::new(left, top));
        }
        bc.constrain(Size::new(s.width + left + right, s.height + top + bottom))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let live = self.on_press.is_some();
        match self.style {
            TapStyle::Row => {
                if self.selected {
                    cx.canvas.fill_rect(r, t.selection());
                } else if live && cx.is_pressed() {
                    cx.canvas.fill_rect(r, t.sunk());
                } else if live && cx.is_hovered() {
                    cx.canvas.fill_rect(r, t.hover(t.surface()).with_alpha(0.9));
                }
            }
            TapStyle::Card => {
                let fill = if live && cx.is_pressed() {
                    t.sunk()
                } else if live && cx.is_hovered() {
                    t.hover(t.raised())
                } else {
                    t.raised()
                };
                draw::raised(cx.canvas, t, r, Some(fill), if self.selected { t.focus() } else { t.line2() });
            }
        }
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        if cx.focus_visible() {
            let c = cx.theme().focus();
            draw::border(cx.canvas, cx.bounds(), c, 2.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.selected, self.style, self.on_press.is_some()));
    }

    fn hit(&self) -> bool {
        self.on_press.is_some()
    }

    fn solid(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.on_press.is_some() { Focusable::Tab } else { Focusable::No }
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn cursor(&self) -> Option<Cursor> {
        None
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let Some(msg) = &self.on_press else { return Status::Ignored };
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => {
                cx.invalidate();
                Status::Handled
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                cx.invalidate();
                if cx.bounds().contains(*pos) {
                    cx.emit(msg);
                }
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
            role: if self.on_press.is_some() { SemanticRole::Button } else { SemanticRole::Group },
            label: self.label.as_deref().map(str::to_owned),
            selected: self.selected.then_some(true),
            actions: if self.on_press.is_some() {
                vec![SemanticAction::Click, SemanticAction::Focus]
            } else {
                Vec::new()
            },
            ..Default::default()
        })
    }
}

// Bars of dye colours.

/// A bar split into segments by share, each in its colour, with a 1pt gap
/// of the surface between them: the store's parts, a package's share.
/// Colours are chosen at paint time from the theme by `Dye`.
pub fn bar<Msg>(segments: Vec<(f64, Dye)>, height: f64) -> Element<Msg> {
    Element::new(BarWidget { segments, height }, Vec::new())
}

/// A colour from the theme's palette, chosen when painting so Night and
/// Day each get theirs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dye {
    /// Ai 藍, the selection indigo.
    Ai,
    /// Hanada 縹, the blue.
    Hanada,
    /// Kamenozoki 甕覗, the palest blue.
    Kamenozoki,
    Violet,
    Green,
    Cyan,
    Amber,
    Red,
    Faint,
    Line,
}

impl Dye {
    pub fn color(self, t: &Theme) -> Color {
        let dark = t.is_dark();
        match self {
            Dye::Ai => t.selection(),
            Dye::Hanada => t.role(Role::Accent),
            Dye::Kamenozoki => {
                if dark {
                    Color::hex(0xa9c4ff)
                } else {
                    Color::hex(0x8fb0f0)
                }
            }
            Dye::Violet => t.role(Role::Violet),
            Dye::Green => t.role(Role::Success),
            Dye::Cyan => t.role(Role::Cyan),
            Dye::Amber => t.role(Role::Warning),
            Dye::Red => t.role(Role::Danger),
            Dye::Faint => t.faint(),
            Dye::Line => t.line2(),
        }
    }
}

struct BarWidget {
    segments: Vec<(f64, Dye)>,
    height: f64,
}

impl Widget for BarWidget {
    fn name(&self) -> &'static str {
        "bar"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 160.0 };
        bc.constrain(Size::new(w, self.height))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        // The track.
        cx.canvas.fill_rect(r, t.sunk());
        let total: f64 = self.segments.iter().map(|(s, _)| s.max(0.0)).sum::<f64>().max(1e-9);
        let full = self.segments.iter().map(|(s, _)| s.max(0.0)).sum::<f64>() >= 0.999;
        let scale = if full { r.width() / total } else { r.width() };
        let mut x = r.x0;
        let n = self.segments.len();
        for (i, (share, dye)) in self.segments.iter().enumerate() {
            let w = (share.max(0.0) * scale).round();
            if w <= 0.0 {
                continue;
            }
            let end = if full && i == n - 1 { r.x1 } else { (x + w).min(r.x1) };
            let gap = if end < r.x1 - 0.5 { 1.0 } else { 0.0 };
            cx.canvas.fill_rect(Rect::new(x, r.y0, (end - gap).max(x), r.y1), dye.color(t));
            x = end;
        }
        draw::border(cx.canvas, r, t.bevel_lo().with_alpha(0.35), 1.0);
    }

    fn hash(&self, h: &mut PaintHasher) {
        for (s, d) in &self.segments {
            h.f64(*s).add(d);
        }
        h.f64(self.height);
    }
}

// A wrapping grid.

/// Children in as many equal columns as fit (each at least `min` wide),
/// row after row: the catalog's cards, the Overview's tiles.
pub fn grid<Msg>(children: Vec<Element<Msg>>, min: f64, gap: f64) -> Element<Msg> {
    Element::new(GridWidget { min, gap }, children)
}

struct GridWidget {
    min: f64,
    gap: f64,
}

impl Widget for GridWidget {
    fn name(&self) -> &'static str {
        "grid"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let n = cx.child_count();
        let width = if bc.max.width.is_finite() { bc.max.width } else { self.min * 3.0 };
        let columns = (((width + self.gap) / (self.min + self.gap)).floor() as usize).clamp(1, n.max(1));
        let cell = ((width - self.gap * (columns as f64 - 1.0)) / columns as f64).floor();
        let mut y = 0.0;
        let mut i = 0;
        while i < n {
            let row: Vec<usize> = (i..(i + columns).min(n)).collect();
            // Cells in a row share the tallest one's height.
            let mut h = 0.0f64;
            for &k in &row {
                h = h.max(
                    cx.layout_child(k, Constraints::tight(Size::new(cell, 0.0)).with_max_height(f64::INFINITY)).height,
                );
            }
            for (c, &k) in row.iter().enumerate() {
                cx.layout_child(k, Constraints::tight(Size::new(cell, h)));
                cx.place_child(k, Point::new(c as f64 * (cell + self.gap), y));
            }
            y += h + self.gap;
            i += columns;
        }
        bc.constrain(Size::new(width, (y - self.gap).max(0.0)))
    }
}

// Monograms.

/// A square of a dye colour with a name's initials, the same colour every
/// time for a name: what stands for a package without an icon.
pub fn monogram<Msg>(name: &str, size: f64) -> Element<Msg> {
    Element::new(Monogram { name: name.to_owned(), size }, Vec::new())
}

struct Monogram {
    name: String,
    size: f64,
}

impl Monogram {
    fn initials(&self) -> String {
        let words: Vec<&str> = self.name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        let mut out = String::new();
        match words.as_slice() {
            [] => {}
            [one] => out.extend(one.chars().take(2)),
            [a, b, ..] => {
                out.extend(a.chars().next());
                out.extend(b.chars().next());
            }
        }
        let mut chars = out.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => "?".to_owned(),
        }
    }

    fn dye(&self) -> Dye {
        const DYES: [Dye; 6] = [Dye::Ai, Dye::Hanada, Dye::Violet, Dye::Green, Dye::Cyan, Dye::Amber];
        let h = self.name.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
        DYES[(h % DYES.len() as u32) as usize]
    }
}

impl Widget for Monogram {
    fn name(&self) -> &'static str {
        "monogram"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(self.size, self.size))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let dye = self.dye().color(t);
        // A tinted face edged in the dye, as a dyed cloth swatch.
        cx.canvas.fill_rect(r, dye.mix(t.surface(), if t.is_dark() { 0.72 } else { 0.78 }));
        draw::border(cx.canvas, r, dye.with_alpha(0.85), 1.0);
        let mut f = FontSpec::ui((self.size * 0.4).round());
        f.weight = 700.0;
        let text = if t.is_dark() { dye.mix(Color::WHITE, 0.45) } else { dye.mix(t.text(), 0.35) };
        cx.draw_text_in(&self.initials(), &TextSpec::new(f, text), r, Align::Center);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.name).f64(self.size);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, label: Some(self.name.clone()), ..Default::default() })
    }
}

// Falling snow.

/// Snow falling over whatever is under it, thirty times a second while the
/// window has the keyboard; still when the desktop asks for less motion
/// (and drawn where it would be at rest).
pub fn snowfall<Msg>(count: usize, motion: bool) -> Element<Msg> {
    Element::new(Snow { count, motion }, Vec::new())
}

struct Flake {
    x: f64,
    y: f64,
    radius: f64,
    /// Fraction of the height fallen per second.
    speed: f64,
    drift: f64,
    phase: f64,
    /// Drawn with six arms rather than as a dot.
    crystal: bool,
}

/// The same snow every start.
fn flakes(count: usize) -> Vec<Flake> {
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % 10_000) as f64 / 10_000.0
    };
    (0..count)
        .map(|i| {
            let crystal = i % 7 == 0;
            Flake {
                x: next(),
                y: next(),
                radius: if crystal { 3.5 + next() * 3.0 } else { 0.8 + next() * 2.0 },
                speed: 0.025 + next() * 0.05,
                drift: 4.0 + next() * 14.0,
                phase: next() * TAU,
                crystal,
            }
        })
        .collect()
}

struct SnowState {
    flakes: Vec<Flake>,
    time: f64,
    last: Option<Instant>,
    ticking: bool,
    /// The window had the keyboard when last painted.
    active: Cell<bool>,
}

impl Default for SnowState {
    fn default() -> SnowState {
        SnowState { flakes: Vec::new(), time: 0.0, last: None, ticking: false, active: Cell::new(true) }
    }
}

struct Snow {
    count: usize,
    motion: bool,
}

const FRAME: Duration = Duration::from_millis(33);

impl Widget for Snow {
    fn name(&self) -> &'static str {
        "snow"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let motion = self.motion && cx.theme().motion();
        let count = self.count;
        let s = cx.state::<SnowState>();
        if s.flakes.len() != count {
            s.flakes = flakes(count);
        }
        let start = motion && !s.ticking;
        if start {
            s.ticking = true;
            s.last = None;
            cx.request_tick(FRAME);
        }
        bc.constrain(bc.max)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let Some(s) = cx.state::<SnowState>() else { return };
        s.active.set(cx.window_active());
        let size = cx.size();
        let (w, h) = (size.width, size.height);
        for flake in &s.flakes {
            let fall = (flake.y + flake.speed * s.time).rem_euclid(1.1) - 0.05;
            let x = flake.x * w + (s.time * 0.6 + flake.phase).sin() * flake.drift;
            let y = fall * h;
            let alpha = (0.3 + 0.5 * (flake.radius / 6.5).min(1.0)) as f32;
            let c = Color::WHITE.with_alpha(alpha);
            if flake.crystal {
                let mut path = BezPath::new();
                for arm in 0..6 {
                    let angle = arm as f64 * TAU / 6.0 + flake.phase + s.time * 0.15;
                    let (dx, dy) = (angle.cos() * flake.radius, angle.sin() * flake.radius);
                    path.move_to((x, y));
                    path.line_to((x + dx, y + dy));
                    // A small branch near each tip.
                    let (bx, by) = (x + dx * 0.6, y + dy * 0.6);
                    for side in [-1.0, 1.0] {
                        let a = angle + side * 0.6;
                        path.move_to((bx, by));
                        path.line_to((bx + a.cos() * flake.radius * 0.35, by + a.sin() * flake.radius * 0.35));
                    }
                }
                cx.canvas.stroke(&path, &StrokeStyle::new(1.0), c);
            } else {
                cx.canvas.fill(&Circle::new((x, y), flake.radius), c);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.count, self.motion));
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !matches!(event, Event::Tick) {
            return Status::Ignored;
        }
        let now = Instant::now();
        let motion = self.motion && cx.theme().motion();
        let s = cx.state::<SnowState>();
        if !motion {
            s.ticking = false;
            return Status::Handled;
        }
        if !s.active.get() {
            // In the background: look again in a second (painting one
            // point tells whether the window has the keyboard again).
            s.last = None;
            cx.request_tick(Duration::from_secs(1));
            cx.invalidate_rect(Rect::new(0.0, 0.0, 1.0, 1.0));
            return Status::Handled;
        }
        let dt = s.last.map_or(0.0, |t| now.duration_since(t).as_secs_f64().min(0.1));
        s.last = Some(now);
        s.time += dt;
        cx.request_tick(FRAME);
        cx.invalidate();
        Status::Handled
    }
}

// Chips.

/// A small label tinted in a role's colour: `newer version`, `running`,
/// `unfree`. Quieter than the kit's solid badge, for lists of them.
pub fn chip<Msg>(text: &str, role: Role) -> Element<Msg> {
    Element::new(Chip { text: text.to_owned(), role }, Vec::new())
}

struct Chip {
    text: String,
    role: Role,
}

impl Chip {
    fn spec(&self, t: &Theme) -> TextSpec {
        let mut f = FontSpec::mono(t.metrics.caption);
        f.weight = 500.0;
        TextSpec::new(f, t.role(self.role))
    }
}

impl Widget for Chip {
    fn name(&self) -> &'static str {
        "chip"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let spec = self.spec(cx.theme());
        let w = cx.measure(&self.text, &spec).width.ceil();
        bc.constrain(Size::new(w + 12.0, 18.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let c = t.role(self.role);
        cx.canvas.fill_rect(r, c.with_alpha(if t.is_dark() { 0.14 } else { 0.10 }));
        draw::border(cx.canvas, r, c.with_alpha(0.45), 1.0);
        let spec = self.spec(t);
        cx.draw_text_in(&self.text, &spec, r, Align::Center);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.text).add(&self.role);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Label, label: Some(self.text.clone()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }
}

// Columns of a chart.

/// Bars standing side by side, each with its label under it and its value
/// over it, in mono: the generations by size. The `highlight`ed one is in
/// the focus colour.
pub fn columns<Msg>(bars: Vec<(String, String, f64)>, highlight: Option<usize>, height: f64) -> Element<Msg> {
    Element::new(Columns { bars, highlight, height }, Vec::new())
}

struct Columns {
    /// (label, value, share of the tallest).
    bars: Vec<(String, String, f64)>,
    highlight: Option<usize>,
    height: f64,
}

impl Widget for Columns {
    fn name(&self) -> &'static str {
        "columns"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 240.0 };
        bc.constrain(Size::new(w, self.height))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let n = self.bars.len().max(1) as f64;
        let label_h = 16.0;
        let value_h = 16.0;
        let chart = Rect::new(r.x0, r.y0 + value_h, r.x1, r.y1 - label_h);
        let slot = (r.width() / n).floor();
        let bar_w = (slot * 0.56).round().clamp(6.0, 40.0);
        let mut small = FontSpec::mono(t.metrics.caption);
        small.tabular = true;
        // The baseline.
        cx.canvas.fill_rect(Rect::new(r.x0, chart.y1, r.x1, chart.y1 + 1.0), t.line2());
        for (i, (label, value, share)) in self.bars.iter().enumerate() {
            let x = r.x0 + slot * i as f64 + ((slot - bar_w) / 2.0).round();
            let h = (chart.height() * share.clamp(0.0, 1.0)).round().max(2.0);
            let bar = Rect::new(x, chart.y1 - h, x + bar_w, chart.y1);
            let on = self.highlight == Some(i);
            let color = if on { t.focus() } else { Dye::Hanada.color(t).with_alpha(0.75) };
            cx.canvas.fill_rect(bar, color);
            draw::top(cx.canvas, bar, if on { Color::WHITE.with_alpha(0.35) } else { t.bevel_hi() }, 1.0);
            let lr = Rect::new(r.x0 + slot * i as f64, chart.y1 + 2.0, r.x0 + slot * (i + 1) as f64, r.y1);
            let label_color = if on { t.text() } else { t.text3() };
            cx.draw_text_in(label, &TextSpec::new(small, label_color), lr, Align::Center);
            let vr = Rect::new(lr.x0, (bar.y0 - value_h).max(r.y0), lr.x1, bar.y0);
            cx.draw_text_in(value, &TextSpec::new(small, if on { t.text() } else { t.text2() }), vr, Align::Center);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for (l, v, s) in &self.bars {
            h.str(l).str(v).f64(*s);
        }
        h.add(&self.highlight).f64(self.height);
    }
}

// Notes across a page.

/// A well with a stripe of a role's colour down its left edge, around
/// `content`: a note, a warning, an error.
pub fn noted<Msg>(role: Role, content: impl Into<Element<Msg>>) -> Element<Msg> {
    Element::new(Noted(role), vec![content.into()])
}

struct Noted(Role);

const NOTE_PAD: [f64; 4] = [10.0, 12.0, 10.0, 15.0];

impl Widget for Noted {
    fn name(&self) -> &'static str {
        "noted"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let [top, right, bottom, left] = NOTE_PAD;
        let inner = bc.shrink(aizome_kit::Padding { top, right, bottom, left }).loosen();
        let s = if cx.child_count() > 0 { cx.layout_child(0, inner) } else { Size::ZERO };
        if cx.child_count() > 0 {
            cx.place_child(0, Point::new(left, top));
        }
        let width = if bc.max.width.is_finite() { bc.max.width } else { s.width + left + right };
        bc.constrain(Size::new(width, s.height + top + bottom))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        cx.canvas.fill_rect(r, t.sunk());
        draw::border(cx.canvas, r, t.line(), 1.0);
        cx.canvas.fill_rect(Rect::new(r.x0, r.y0, r.x0 + 3.0, r.y1), t.role(self.0));
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.0);
    }
}

// Rows that make room.

/// A row of `main` and the things at its right (`trailing`), which go to a
/// line of their own under it, at the right, when the window is too
/// narrow for `main` to keep `min_main` points beside them.
pub fn adapt<Msg>(main: Element<Msg>, trailing: Vec<Element<Msg>>, spacing: f64, min_main: f64) -> Element<Msg> {
    let mut children = vec![main];
    children.extend(trailing);
    Element::new(Adapt { spacing, min_main }, children)
}

struct Adapt {
    spacing: f64,
    min_main: f64,
}

impl Widget for Adapt {
    fn name(&self) -> &'static str {
        "adapt"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let n = cx.child_count();
        if n == 0 {
            return bc.min;
        }
        let width = if bc.max.width.is_finite() { bc.max.width } else { 640.0 };
        let loose = Constraints::loose(Size::new(width, f64::INFINITY));
        let mut sizes = Vec::with_capacity(n);
        let mut trailing = 0.0;
        for i in 1..n {
            let s = cx.layout_child(i, loose);
            trailing += s.width + self.spacing;
            sizes.push(s);
        }
        let tall = sizes.iter().map(|s| s.height).fold(0.0, f64::max);
        let beside = width - trailing;
        if n == 1 || beside >= self.min_main {
            let main = cx.layout_child(
                0,
                Constraints { min: Size::new(beside.max(0.0), 0.0), max: Size::new(beside.max(0.0), f64::INFINITY) },
            );
            let h = main.height.max(tall);
            cx.place_child(0, Point::new(0.0, ((h - main.height) / 2.0).round()));
            let mut x = beside + self.spacing;
            for (k, s) in sizes.iter().enumerate() {
                cx.place_child(k + 1, Point::new(x, ((h - s.height) / 2.0).round()));
                x += s.width + self.spacing;
            }
            return bc.constrain(Size::new(width, h));
        }
        // Too narrow: the trailing things under the main part, at the right.
        let main = cx.layout_child(0, Constraints { min: Size::new(width, 0.0), max: Size::new(width, f64::INFINITY) });
        cx.place_child(0, Point::ZERO);
        let y = main.height + 6.0;
        let mut x = (width - (trailing - self.spacing)).max(0.0);
        for (k, s) in sizes.iter().enumerate() {
            cx.place_child(k + 1, Point::new(x, y + ((tall - s.height) / 2.0).round()));
            x += s.width + self.spacing;
        }
        bc.constrain(Size::new(width, y + tall))
    }
}
