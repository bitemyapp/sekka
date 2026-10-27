//! Programmer mode's own controls: the value in each radix (a row each,
//! the one being typed marked as the Deskbar marks the focused window),
//! and the bit toggles, two bytes to a row, each byte in its own well.

use std::rc::Rc;

use aizome_kit::draw;
use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Key as KeyName, NamedKey, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Role};
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Align, Element, Layoutable, Length};

use crate::grouped::{self, Cut};

/// A bit's cell.
pub const CELL: f64 = 24.0;
/// Between the two bytes of a row.
pub const BYTE_GAP: f64 = 4.0;
/// The bit numbers under a row.
pub const CAPTION: f64 = 13.0;
/// The grid's size.
pub const WIDTH: f64 = 16.0 * CELL + BYTE_GAP;
pub const HEIGHT: f64 = 4.0 * (CELL + CAPTION);

/// A radix's row: `HEX  DEAD BEEF`.
pub const ROW: f64 = 24.0;

pub fn radix_row<Msg: Clone + Send + 'static>(
    label: &'static str,
    value: String,
    selected: bool,
    on_press: Msg,
) -> Element<Msg> {
    Element::new(RadixRow { label, value, selected, on_press: MsgFn::new(on_press) }, Vec::new())
        .key(label)
        .width(Length::Fill)
        .height(ROW)
}

struct RadixRow {
    label: &'static str,
    value: String,
    selected: bool,
    on_press: MsgFn,
}

fn mono(size: f64, weight: f32) -> FontSpec {
    let mut f = FontSpec::mono(size);
    f.weight = weight;
    f.tabular = true;
    f
}

impl Widget for RadixRow {
    fn name(&self) -> &'static str {
        "soroban-radix"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width.min(4000.0), ROW))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        if self.selected {
            cx.canvas.fill_rect(r, t.focus().with_alpha(if t.is_dark() { 0.14 } else { 0.22 }));
            cx.canvas.fill_rect(Rect::new(r.x0 + 2.0, r.y0 + 5.0, r.x0 + 5.0, r.y1 - 5.0), t.focus());
        } else if cx.is_hovered() {
            cx.canvas.fill_rect(r, t.hover(t.surface()));
        }
        let mut lf = mono(10.5, if self.selected { 650.0 } else { 500.0 });
        lf.tabular = false;
        let mut ls = TextSpec::new(lf, if self.selected { t.text() } else { t.text3() });
        ls.letter_spacing = 0.8;
        cx.draw_text_in(self.label, &ls, Rect::new(r.x0 + 12.0, r.y0, r.x0 + 48.0, r.y1), Align::Start);
        let vs = TextSpec::new(
            mono(12.5, if self.selected { 600.0 } else { 400.0 }),
            if self.selected { t.text() } else { t.text2() },
        );
        let room = Rect::new(r.x0 + 48.0, r.y0, r.x1 - 8.0, r.y1);
        // The low digits matter most: cut from the left.
        let text = grouped::fit(cx, &self.value, &vs, room.width(), Cut::Start);
        grouped::draw_in(cx, &text, &vs, room, Align::Start);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.label, &self.value, self.selected));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Tab
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                if cx.bounds().contains(*pos) {
                    cx.emit(&self.on_press);
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed && cx.is_focused() && k.modifiers.is_empty() => {
                let works = matches!(&k.key, KeyName::Named(NamedKey::Enter))
                    || matches!(&k.key, KeyName::Character(s) if s == " ");
                if works {
                    cx.emit(&self.on_press);
                    return Status::Handled;
                }
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        cx.emit(&self.on_press);
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::RadioButton,
            label: Some(self.label.into()),
            value: Some(self.value.replace('\u{2009}', "")),
            selected: Some(self.selected),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(self.label)
    }
}

/// The 64 bit toggles: `value`'s bits, those past `bits` faint.
pub fn bit_grid<Msg: Clone + Send + 'static>(
    value: u64,
    bits: u32,
    on_toggle: impl Fn(u32) -> Msg + 'static,
) -> Element<Msg> {
    Element::new(BitGrid { value, bits, on_toggle: Rc::new(on_toggle) }, Vec::new())
        .key("bits")
        .id("bits")
        .width(WIDTH)
        .height(HEIGHT)
}

struct BitGrid<Msg> {
    value: u64,
    bits: u32,
    on_toggle: Rc<dyn Fn(u32) -> Msg>,
}

#[derive(Default)]
struct GridState {
    hover: Option<u32>,
    /// The bit ←→↑↓ move to and Space flips.
    cursor: u32,
}

/// Where bit `i` is drawn.
pub fn cell(i: u32) -> Rect {
    let row = 3 - (i / 16);
    let col = 15 - (i % 16);
    let x = f64::from(col) * CELL + if col >= 8 { BYTE_GAP } else { 0.0 };
    let y = f64::from(row) * (CELL + CAPTION);
    Rect::new(x, y, x + CELL, y + CELL)
}

fn bit_at(p: Point) -> Option<u32> {
    (0..64).find(|&i| cell(i).contains(p))
}

impl<Msg: Clone + Send + 'static> Widget for BitGrid<Msg> {
    fn name(&self) -> &'static str {
        "soroban-bits"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(WIDTH, HEIGHT))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let (hover, cursor) = cx.state::<GridState>().map_or((None, None), |s| (s.hover, Some(s.cursor)));
        let hover = hover.filter(|_| cx.is_hovered());
        let show_cursor = cx.focus_visible();
        let set_fill = t.role(Role::Accent).with_alpha(if t.is_dark() { 0.20 } else { 0.16 });
        // Each byte in its well.
        for row in 0..4u32 {
            for byte in 0..2u32 {
                let hi = (3 - row) * 16 + (1 - byte) * 8 + 7;
                let lo = hi - 7;
                let well = cell(hi).union(cell(lo));
                let active = lo < self.bits;
                if active {
                    draw::sunken(cx.canvas, t, well, t.sunk(), t.line());
                } else {
                    // Past the word: an empty outline.
                    draw::border(cx.canvas, well, t.line(), 1.0);
                }
                // The nibbles apart.
                let mid = cell(lo + 3).x1;
                cx.canvas.fill_rect(Rect::new(mid, well.y0 + 5.0, mid + 1.0, well.y1 - 5.0), t.line());
                let cap = TextSpec::new(mono(9.5, 400.0), if active { t.text3() } else { t.faint() });
                let caps = Rect::new(well.x0 + 1.0, well.y1 + 1.0, well.x1 - 1.0, well.y1 + CAPTION);
                // Each byte's top bit, and the lowest bit at the row's end.
                cx.draw_text_in(&hi.to_string(), &cap, caps, Align::Start);
                if byte == 1 {
                    cx.draw_text_in(&lo.to_string(), &cap, caps, Align::End);
                }
            }
        }
        for i in 0..64u32 {
            let c = cell(i);
            let active = i < self.bits;
            let on = active && self.value >> i & 1 == 1;
            let inner = c.inset(-2.0);
            if on {
                cx.canvas.fill_rect(inner, set_fill);
            }
            if active && hover == Some(i) {
                cx.canvas.fill_rect(inner, t.hover(t.sunk()).with_alpha(0.9));
                if on {
                    cx.canvas.fill_rect(inner, set_fill);
                }
            }
            let color = if !active {
                t.faint()
            } else if on {
                t.text()
            } else {
                t.text3()
            };
            let spec = TextSpec::new(mono(13.0, if on { 650.0 } else { 400.0 }), color);
            let digit = if !active {
                "·"
            } else if on {
                "1"
            } else {
                "0"
            };
            cx.draw_text_in(digit, &spec, c, Align::Center);
            if show_cursor && cursor == Some(i) {
                draw::border(cx.canvas, c.inset(-1.0), t.focus(), 2.0);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.value, self.bits));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Tab
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerMove { pos, .. } => {
                let at = bit_at(*pos).filter(|i| *i < self.bits);
                let s = cx.state::<GridState>();
                if s.hover != at {
                    s.hover = at;
                    cx.invalidate();
                }
                Status::Ignored
            }
            Event::PointerLeave => {
                cx.state::<GridState>().hover = None;
                cx.invalidate();
                Status::Ignored
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                if let Some(i) = bit_at(*pos).filter(|i| *i < self.bits) {
                    cx.state::<GridState>().cursor = i;
                    cx.emit(&MsgFn::new((self.on_toggle)(i)));
                }
                Status::Handled
            }
            Event::PointerUp { .. } => Status::Handled,
            Event::Key(k) if k.pressed && k.modifiers.is_empty() => {
                let bits = self.bits;
                let s = cx.state::<GridState>();
                let c = s.cursor.min(bits - 1);
                let moved = match &k.key {
                    KeyName::Named(NamedKey::ArrowLeft) => Some((c + 1).min(bits - 1)),
                    KeyName::Named(NamedKey::ArrowRight) => Some(c.saturating_sub(1)),
                    KeyName::Named(NamedKey::ArrowUp) => Some(if c + 16 < bits { c + 16 } else { c }),
                    KeyName::Named(NamedKey::ArrowDown) => Some(c.saturating_sub(16).min(c)),
                    KeyName::Named(NamedKey::Home) => Some(bits - 1),
                    KeyName::Named(NamedKey::End) => Some(0),
                    _ => None,
                };
                if let Some(m) = moved {
                    let m = if matches!(&k.key, KeyName::Named(NamedKey::ArrowDown)) && c < 16 { c } else { m };
                    s.cursor = m;
                    cx.invalidate();
                    return Status::Handled;
                }
                let flip = matches!(&k.key, KeyName::Named(NamedKey::Enter))
                    || matches!(&k.key, KeyName::Character(s) if s == " ");
                if flip {
                    cx.emit(&MsgFn::new((self.on_toggle)(c)));
                    return Status::Handled;
                }
                Status::Ignored
            }
            Event::Focus(_) => {
                cx.invalidate();
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Group,
            label: Some("Bits".into()),
            value: Some(format!("{:0width$b}", self.value, width = self.bits as usize)),
            ..Default::default()
        })
    }
}
