//! The paper tape: every calculation, newest at the bottom, in a well
//! beside the keypad. A click (or ↑↓ and Return) puts an answer back
//! into the expression.

use std::rc::Rc;

use aizome_kit::draw;
use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Rect, Size};
use aizome_kit::sidestep_ui::{Key as KeyName, NamedKey, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Align, Element, Layoutable, Length};

use crate::grouped;

/// One entry's height.
pub const ROW: f64 = 42.0;
const PAD: f64 = 10.0;

pub fn tape<Msg: Clone + Send + 'static>(
    entries: Vec<(String, String)>,
    on_recall: impl Fn(usize) -> Msg + 'static,
) -> Element<Msg> {
    Element::new(TapeWidget { entries, on_recall: Rc::new(on_recall), view: 0.0 }, Vec::new())
        .key("tape-list")
        .id("tape-list")
        .width(Length::Fill)
        .height(Length::Fill)
}

struct TapeWidget<Msg> {
    entries: Vec<(String, String)>,
    on_recall: Rc<dyn Fn(usize) -> Msg>,
    /// The height laid out at.
    view: f64,
}

/// What the tape remembers between views.
#[derive(Default)]
struct TapeState {
    /// Scrolled up from the end, in points (0 keeps the newest showing).
    from_end: f64,
    hover: Option<usize>,
    /// The entry chosen with the keyboard.
    cursor: Option<usize>,
    /// How many entries there were, to follow new ones.
    seen: usize,
}

impl<Msg> TapeWidget<Msg> {
    fn content(&self) -> f64 {
        self.entries.len() as f64 * ROW
    }

    fn max_scroll(&self) -> f64 {
        (self.content() - self.view).max(0.0)
    }

    /// The content's offset at the top of the view.
    fn top(&self, s: &TapeState) -> f64 {
        (self.max_scroll() - s.from_end).max(0.0)
    }

    fn index_at(&self, s: &TapeState, y: f64) -> Option<usize> {
        let i = ((y + self.top(s)) / ROW).floor();
        (i >= 0.0 && (i as usize) < self.entries.len()).then_some(i as usize)
    }

    /// Scroll so entry `i` shows.
    fn scroll_to(&self, s: &mut TapeState, i: usize) {
        let y0 = i as f64 * ROW;
        let top = self.top(s);
        let new_top = if y0 < top {
            y0
        } else if y0 + ROW > top + self.view {
            y0 + ROW - self.view
        } else {
            top
        };
        s.from_end = (self.max_scroll() - new_top).clamp(0.0, self.max_scroll());
    }
}

impl<Msg: Clone + Send + 'static> Widget for TapeWidget<Msg> {
    fn name(&self) -> &'static str {
        "soroban-tape"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = bc.constrain(Size::new(bc.max.width, bc.max.height));
        self.view = size.height;
        let n = self.entries.len();
        let s = cx.state::<TapeState>();
        if n != s.seen {
            // New calculations bring the tape back to its end.
            if n > s.seen {
                s.from_end = 0.0;
            }
            s.seen = n;
            s.cursor = s.cursor.filter(|c| *c < n);
            s.hover = None;
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        draw::sunken(cx.canvas, t, r, t.sunk(), t.line());
        let (top, hover, cursor) = match cx.state::<TapeState>() {
            Some(s) => (self.top(s), s.hover, s.cursor),
            None => (self.max_scroll(), None, None),
        };
        if self.entries.is_empty() {
            let mut f = FontSpec::ui(12.0);
            f.italic = true;
            let spec = TextSpec::new(f, t.text3());
            cx.draw_text_in("Calculations appear here", &spec, r, Align::Center);
            return;
        }
        cx.canvas.save();
        cx.canvas.clip_rect(r.inset(-1.0));
        let mut ef = FontSpec::mono(11.5);
        ef.tabular = true;
        let mut rf = FontSpec::mono(14.0);
        rf.weight = 600.0;
        rf.tabular = true;
        let first = (top / ROW).floor().max(0.0) as usize;
        let last = (((top + r.height()) / ROW).ceil() as usize).min(self.entries.len());
        for i in first..last {
            let y = i as f64 * ROW - top;
            let row = Rect::new(r.x0 + 1.0, y, r.x1 - 1.0, y + ROW);
            let chosen = cursor == Some(i) && cx.is_focused();
            let (ink, ink2) = if chosen {
                cx.canvas.fill_rect(row, t.selection());
                (t.on_selection(), t.on_selection().with_alpha(0.8))
            } else {
                if hover == Some(i) && cx.is_hovered() {
                    cx.canvas.fill_rect(row, t.hover(t.sunk()));
                }
                (t.text(), t.text3())
            };
            let (expr, result) = &self.entries[i];
            let text_box = |y0: f64, y1: f64| Rect::new(row.x0 + PAD, y0, row.x1 - PAD, y1);
            grouped::draw_in(cx, expr, &TextSpec::new(ef, ink2), text_box(y + 4.0, y + 19.0), Align::End);
            let result = format!("= {result}");
            grouped::draw_in(cx, &result, &TextSpec::new(rf, ink), text_box(y + 19.0, y + 38.0), Align::End);
            if i + 1 < self.entries.len() && !chosen && cursor != Some(i + 1) {
                cx.canvas.fill_rect(Rect::new(row.x0 + PAD, y + ROW - 1.0, row.x1 - PAD, y + ROW), t.line());
            }
        }
        // Where the view is, when there's more than shows.
        let max = self.max_scroll();
        if max > 0.0 {
            let track = r.inset(-3.0);
            let h = (track.height() * r.height() / self.content()).max(16.0);
            let y = track.y0 + (track.height() - h) * (top / max);
            cx.canvas.fill_rect(Rect::new(track.x1 - 2.0, y, track.x1, y + h), t.faint());
        }
        cx.canvas.restore();
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.entries);
    }

    fn hit(&self) -> bool {
        true
    }

    fn clips(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.entries.is_empty() { Focusable::No } else { Focusable::Tab }
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Keyboard
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerMove { pos, .. } => {
                let s = cx.state::<TapeState>();
                let at = if pos.x >= 0.0 { self.index_at(s, pos.y) } else { None };
                let s = cx.state::<TapeState>();
                if s.hover != at {
                    s.hover = at;
                    cx.invalidate();
                }
                Status::Ignored
            }
            Event::PointerLeave => {
                cx.state::<TapeState>().hover = None;
                cx.invalidate();
                Status::Ignored
            }
            Event::PointerDown { button: PointerButton::Left, .. } => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                let s = cx.state::<TapeState>();
                if let Some(i) = self.index_at(s, pos.y).filter(|_| cx.bounds().contains(*pos)) {
                    cx.emit(&MsgFn::new((self.on_recall)(i)));
                }
                Status::Handled
            }
            Event::Scroll { delta, .. } => {
                let max = self.max_scroll();
                let s = cx.state::<TapeState>();
                let before = s.from_end;
                s.from_end = (s.from_end - delta.y).clamp(0.0, max);
                if s.from_end != before {
                    cx.invalidate();
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed && k.modifiers.is_empty() => {
                let n = self.entries.len();
                if n == 0 {
                    return Status::Ignored;
                }
                let s = cx.state::<TapeState>();
                let cur = s.cursor;
                let next = match &k.key {
                    KeyName::Named(NamedKey::ArrowUp) => Some(cur.map_or(n - 1, |c| c.saturating_sub(1))),
                    KeyName::Named(NamedKey::ArrowDown) => Some(cur.map_or(n - 1, |c| (c + 1).min(n - 1))),
                    KeyName::Named(NamedKey::Home) => Some(0),
                    KeyName::Named(NamedKey::End) => Some(n - 1),
                    KeyName::Named(NamedKey::Enter) => {
                        if let Some(c) = cur {
                            cx.emit(&MsgFn::new((self.on_recall)(c)));
                            return Status::Handled;
                        }
                        None
                    }
                    _ => None,
                };
                match next {
                    Some(i) => {
                        let mut st = std::mem::take(cx.state::<TapeState>());
                        st.cursor = Some(i);
                        self.scroll_to(&mut st, i);
                        *cx.state::<TapeState>() = st;
                        cx.invalidate();
                        Status::Handled
                    }
                    None => Status::Ignored,
                }
            }
            Event::Focus(_) => {
                let n = self.entries.len();
                let s = cx.state::<TapeState>();
                if s.cursor.is_none() && n > 0 {
                    s.cursor = Some(n - 1);
                }
                cx.invalidate();
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::List,
            label: Some("Paper tape".into()),
            value: self.entries.last().map(|(e, r)| format!("{e} = {r}")),
            ..Default::default()
        })
    }
}

/// The width of the tape's column.
pub const WIDTH: f64 = 216.0;
