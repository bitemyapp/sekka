//! The keypads: which keys each mode has, where, and the key itself.
//!
//! Keys are square, edged in 1pt with the top highlight, as the kit's
//! buttons are. Digits sit on `raised`, functions on `chrome`; the
//! arithmetic operators step down the dye ramp, from kamenozoki at ÷ to ai
//! at +, as cloth darkens with each dip; `=` is the focus colour.

use std::rc::Rc;

use aizome_kit::draw;
use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Key as KeyName, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Role, Theme};
use aizome_kit::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Color, Element, Layoutable};

use crate::entry::{Family, Key, Mode};

/// A key's side, in points.
pub const KEY: f64 = 52.0;
/// Between keys.
pub const GAP: f64 = 4.0;
/// Between the scientific keys and the basic ones.
pub const GROUP_GAP: f64 = 8.0;

/// A grid of keys, row by row.
pub struct Grid {
    pub cols: usize,
    pub keys: Vec<Key>,
}

impl Grid {
    pub fn rows(&self) -> usize {
        self.keys.len().div_ceil(self.cols)
    }

    pub fn width(&self) -> f64 {
        self.cols as f64 * KEY + (self.cols as f64 - 1.0) * GAP
    }

    pub fn height(&self) -> f64 {
        let r = self.rows() as f64;
        r * KEY + (r - 1.0) * GAP
    }
}

pub fn basic() -> Grid {
    use Key::*;
    Grid {
        cols: 4,
        keys: vec![
            Clear,
            Back,
            Percent,
            Div, //
            Digit(7),
            Digit(8),
            Digit(9),
            Mul, //
            Digit(4),
            Digit(5),
            Digit(6),
            Sub, //
            Digit(1),
            Digit(2),
            Digit(3),
            Add, //
            Negate,
            Digit(0),
            Point,
            Equals,
        ],
    }
}

pub fn scientific() -> Grid {
    use Key::*;
    Grid {
        cols: 6,
        keys: vec![
            Open, Close, MemClear, MemAdd, MemSub, MemRecall, //
            Second, Square, Cube, Pow, Exp, Pow10, //
            Recip, Sqrt, Cbrt, Ln, Log, Fact, //
            Sin, Cos, Tan, Pi, Euler, EE, //
            Angle, Sinh, Cosh, Tanh, Abs, Rand,
        ],
    }
}

/// The hex keypad (C D E F over 8 9 A B, as the KIM-1's), with the
/// bitwise operators beside it.
pub fn programmer() -> Grid {
    use Key::*;
    Grid {
        cols: 7,
        keys: vec![
            And,
            Or,
            Clear,
            Back,
            Open,
            Close,
            Div, //
            Xor,
            Not,
            Digit(12),
            Digit(13),
            Digit(14),
            Digit(15),
            Mul, //
            Shl,
            Shr,
            Digit(8),
            Digit(9),
            Digit(10),
            Digit(11),
            Sub, //
            Rol,
            Ror,
            Digit(4),
            Digit(5),
            Digit(6),
            Digit(7),
            Add, //
            Mod,
            Negate,
            Digit(0),
            Digit(1),
            Digit(2),
            Digit(3),
            Equals,
        ],
    }
}

/// The grids a mode shows, left to right.
pub fn grids(mode: Mode) -> Vec<Grid> {
    match mode {
        Mode::Basic => vec![basic()],
        Mode::Scientific => vec![scientific(), basic()],
        Mode::Programmer => vec![programmer()],
    }
}

/// The keypads' size for a mode.
pub fn size(mode: Mode) -> Size {
    let gs = grids(mode);
    let w = gs.iter().map(Grid::width).sum::<f64>() + GROUP_GAP * (gs.len() as f64 - 1.0);
    let h = gs.iter().map(Grid::height).fold(0.0, f64::max);
    Size::new(w, h)
}

/// How a key looks now.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub key: Key,
    pub label: String,
    pub sup: Option<&'static str>,
    /// Two words, one above the other, the first or second lit (Deg/Rad).
    pub pair: Option<(&'static str, &'static str, bool)>,
    pub family: Family,
    pub enabled: bool,
    /// Latched on (2nd).
    pub on: bool,
    /// Shown pressed because its key was typed.
    pub flashed: bool,
}

/// A key on the keypad.
pub fn key<Msg: Clone + Send + 'static>(face: Face, on_press: Msg, spoken: String) -> Element<Msg> {
    let w = KeyWidget { face, on_press: MsgFn::new(on_press), spoken };
    let key = w.face.key;
    Element::new(w, Vec::new()).key(format!("{key:?}")).id(format!("key-{key:?}")).width(KEY).height(KEY)
}

struct KeyWidget {
    face: Face,
    on_press: MsgFn,
    spoken: String,
}

/// The dye ramp for the four operators, pale to deep, and the text on
/// each.
fn dye(t: &Theme, step: u8) -> (Color, Color) {
    let ramp = [0xa9c4ff, 0x7aa2f7, 0x5a7fd6, 0x3d59a1];
    let c = ramp[usize::from(step.min(3))];
    let fill = Color::rgba8((c >> 16) as u8, (c >> 8) as u8, c as u8, 255);
    let ink = if step < 2 { Color::rgba8(0x1a, 0x1b, 0x26, 255) } else { Color::rgba8(0xee, 0xf1, 0xff, 255) };
    let _ = t;
    (fill, ink)
}

/// A key's face colour, edge colour and label colour.
fn colors(t: &Theme, f: &Face) -> (Option<Color>, Color, Color) {
    if !f.enabled {
        return (None, t.line(), t.faint());
    }
    if f.on {
        return (Some(t.selection()), t.bevel_lo(), t.on_selection());
    }
    match f.family {
        Family::Digit => (Some(t.raised()), t.line2(), t.text()),
        Family::Function => (Some(t.chrome()), t.line2(), t.text2()),
        Family::Clear => (Some(t.chrome()), t.line2(), t.role(Role::Danger)),
        Family::Bitwise => (
            Some(t.chrome().mix(t.role(Role::Accent), if t.is_dark() { 0.12 } else { 0.10 })),
            t.line2(),
            t.role(Role::Accent),
        ),
        Family::Operator(step) => {
            let (fill, ink) = dye(t, step);
            (Some(fill), t.bevel_lo(), ink)
        }
        Family::Equals => (Some(t.focus()), t.bevel_lo(), t.on_focus()),
    }
}

fn glyph(label: &str) -> bool {
    matches!(label, "+" | "−" | "×" | "÷" | "=" | "±" | "." | "(" | ")" | "<<" | ">>" | "%")
}

impl KeyWidget {
    /// The label's font: digits and operators in IBM Plex Mono, words in
    /// Archivo (condensed when long).
    fn font(&self, size_hint: f64) -> FontSpec {
        let f = &self.face;
        let mut font = if f.family == Family::Digit || glyph(&f.label) {
            let mut m = FontSpec::mono(if f.family == Family::Digit { 20.0 } else { 22.0 });
            m.weight = 500.0;
            m.tabular = true;
            m
        } else {
            let mut u = FontSpec::ui(size_hint);
            u.weight = 600.0;
            u
        };
        if f.label == "⌫" {
            font.size = 20.0;
        }
        font
    }
}

impl Widget for KeyWidget {
    fn name(&self) -> &'static str {
        "soroban-key"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(KEY, KEY))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let f = &self.face;
        let r = cx.bounds();
        let pressed = f.enabled && (cx.is_pressed() || f.flashed);
        let hovered = f.enabled && cx.is_hovered() && !pressed;
        let (fill, edge, mut ink) = colors(t, f);
        match fill {
            Some(base) => {
                let colored = matches!(f.family, Family::Operator(_) | Family::Equals) || f.on;
                let fill = if pressed {
                    if colored { base.mix(t.bevel_lo(), 0.22) } else { t.pressed(base) }
                } else if hovered {
                    if colored { base.mix(Color::WHITE, 0.12) } else { t.hover(base) }
                } else {
                    base
                };
                cx.canvas.fill_rect(r, fill);
                draw::border(cx.canvas, r, edge, 1.0);
                let inner = r.inset(-1.0);
                if pressed {
                    let shade = if t.is_dark() { t.bevel_lo().with_alpha(0.55) } else { t.bevel_lo().with_alpha(0.22) };
                    draw::top(cx.canvas, inner, shade, 1.0);
                } else {
                    let hi = if colored { Color::WHITE.with_alpha(0.22) } else { t.bevel_hi() };
                    draw::top(cx.canvas, inner, hi, 1.0);
                }
            }
            None => draw::border(cx.canvas, r, edge, 1.0),
        }
        let dy = if pressed { 1.0 } else { 0.0 };
        if let Some((a, b, first)) = f.pair {
            // Deg over Rad, the one in use lit.
            let lit = |on: bool| {
                let mut font = FontSpec::ui(13.0);
                font.weight = if on { 650.0 } else { 450.0 };
                font
            };
            let (ca, cb) = if first { (ink, t.faint()) } else { (t.faint(), ink) };
            let top = Rect::new(r.x0, r.y0 + 8.0 + dy, r.x1, r.y0 + 26.0 + dy);
            let bottom = Rect::new(r.x0, r.y0 + 26.0 + dy, r.x1, r.y1 - 8.0 + dy);
            cx.draw_text_in(a, &TextSpec::new(lit(first), ca), top, aizome_kit::Align::Center);
            cx.draw_text_in(b, &TextSpec::new(lit(!first), cb), bottom, aizome_kit::Align::Center);
            return;
        }
        let mut size = 15.0;
        let mut font = self.font(size);
        let mut spec = TextSpec::new(font, ink);
        let mut w = cx.layout_text(&f.label, &spec, None).size().width;
        let sup_font = |base: FontSpec| {
            let mut s = base;
            s.size = (base.size * 0.62).round();
            s.weight = 600.0;
            s
        };
        let sup_w =
            f.sup.map_or(0.0, |s| cx.layout_text(s, &TextSpec::new(sup_font(font), ink), None).size().width + 1.0);
        // Long words narrow to fit, then shrink.
        while w + sup_w > r.width() - 10.0 && size > 10.0 {
            if !font.condensed && font.family != aizome_kit::theme::Family::Mono {
                font.condensed = true;
            } else {
                size -= 1.0;
                font.size = size;
            }
            spec = TextSpec::new(font, ink);
            w = cx.layout_text(&f.label, &spec, None).size().width;
        }
        if f.family == Family::Clear && !f.enabled {
            ink = t.faint();
            spec.color = ink;
        }
        let l = cx.layout_text(&f.label, &spec, None);
        let s = l.size();
        let x = ((r.width() - s.width - sup_w) / 2.0).round();
        let y = ((r.height() - s.height) / 2.0).round() + dy;
        cx.canvas.draw_text(&l, Point::new(x, y));
        if let Some(sup) = f.sup {
            let sl = cx.layout_text(sup, &TextSpec::new(sup_font(font), ink), None);
            let sy = (y - font.size * 0.12).round();
            cx.canvas.draw_text(&sl, Point::new(x + s.width + 1.0, sy));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        let f = &self.face;
        h.add(&(f.key, &f.label, f.sup, f.pair, f.family, f.enabled, f.on, f.flashed));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.face.enabled { Focusable::Tab } else { Focusable::No }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.face.enabled {
            return Status::Ignored;
        }
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => {
                cx.invalidate();
                Status::Handled
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                cx.invalidate();
                if cx.bounds().contains(*pos) {
                    cx.emit(&self.on_press);
                }
                Status::Handled
            }
            // Space works the key with the keyboard focus; every other key,
            // Return too, goes on to the calculator (Return is always =).
            Event::Key(k) if k.pressed && cx.is_focused() && k.modifiers.is_empty() => {
                if matches!(&k.key, KeyName::Character(s) if s == " ") {
                    cx.emit(&self.on_press);
                    cx.invalidate();
                    return Status::Handled;
                }
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        if self.face.enabled {
            cx.emit(&self.on_press);
            cx.invalidate();
            return true;
        }
        false
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Button,
            label: Some(self.spoken.clone()),
            disabled: !self.face.enabled,
            checked: (self.face.key == Key::Second).then_some(self.face.on),
            actions: vec![SemanticAction::Click, SemanticAction::Focus],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.face.label)
    }
}

/// A widget around the whole window that turns typed keys into the
/// calculator's messages (keys bubble up to it from whatever has the
/// focus; it takes the focus itself on a click, so typing always works).
pub fn keyboard<Msg: Clone + Send + 'static>(
    child: impl Into<Element<Msg>>,
    on_key: impl Fn(&aizome_kit::sidestep_ui::KeyEvent) -> Option<Msg> + 'static,
) -> Element<Msg> {
    Element::new(Catcher { on_key: Rc::new(on_key) }, vec![child.into()])
        .id("keyboard")
        .width(aizome_kit::Length::Fill)
        .height(aizome_kit::Length::Fill)
}

/// What turns a key into a message, if it means one.
type OnKey<Msg> = Rc<dyn Fn(&aizome_kit::sidestep_ui::KeyEvent) -> Option<Msg>>;

struct Catcher<Msg> {
    on_key: OnKey<Msg>,
}

impl<Msg: Clone + Send + 'static> Widget for Catcher<Msg> {
    fn name(&self) -> &'static str {
        "soroban-keyboard"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let mut size = Size::ZERO;
        for i in 0..cx.child_count() {
            let s = cx.layout_child(i, bc);
            cx.place_child(i, Point::ZERO);
            size = Size::new(size.width.max(s.width), size.height.max(s.height));
        }
        bc.constrain(size)
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> aizome_kit::widget::FocusRing {
        aizome_kit::widget::FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if let Event::Key(k) = event
            && k.pressed
            && let Some(msg) = (self.on_key)(k)
        {
            cx.emit(&MsgFn::new(msg));
            return Status::Handled;
        }
        Status::Ignored
    }
}
