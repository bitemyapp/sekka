//! Tokei's own elements: clock digits that redraw one by one, the timer's
//! ring, and the window's keys.

use std::rc::Rc;

use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Size};
use aizome_kit::sidestep_ui::kurbo::{Arc, Circle};
use aizome_kit::sidestep_ui::{KeyEvent, StrokeStyle};
use aizome_kit::theme::{FontSpec, Role};
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Element, Layoutable, Length, text};

/// IBM Plex Mono with tabular digits, at `size` and `weight`.
pub fn mono(size: f64, weight: f32) -> FontSpec {
    let mut f = FontSpec::mono(size);
    f.weight = weight;
    f.tabular = true;
    f
}

/// A time, a character to a cell: every cell is its own element keyed by
/// its place from the right, so a tick that changes the last digit
/// redraws that digit alone (the font's digits are all one width, so
/// nothing moves).
pub fn digits<Msg: 'static>(s: &str, font: FontSpec, role: Role) -> Element<Msg> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let cells: Vec<Element<Msg>> =
        chars.iter().enumerate().map(|(i, c)| text(c.to_string()).font(font).role(role).key(n - i).into()).collect();
    aizome_kit::row(cells).label(s.to_owned()).into()
}

/// The timer's ring: a track, and the part of it gone in the dye's
/// hanada, from the top clockwise.
pub fn ring<Msg: 'static>(fraction: f64, size: f64) -> Element<Msg> {
    // A thousand steps: the ring moves when a step does, not every tick.
    let step = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u32;
    Element::new(RingWidget { step }, Vec::new()).key("ring").width(size).height(size)
}

struct RingWidget {
    step: u32,
}

impl Widget for RingWidget {
    fn name(&self) -> &'static str {
        "tokei-ring"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(bc.max)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let w = 8.0;
        let radius = r.width().min(r.height()) / 2.0 - w / 2.0 - 2.0;
        let c = r.center();
        cx.canvas.stroke(&Circle::new(c, radius), &StrokeStyle::new(w), t.sunk());
        cx.canvas.stroke(&Circle::new(c, radius + w / 2.0), &StrokeStyle::new(1.0), t.line());
        cx.canvas.stroke(&Circle::new(c, radius - w / 2.0), &StrokeStyle::new(1.0), t.line());
        let left = 1.0 - f64::from(self.step) / 1000.0;
        if left > 0.0 {
            let sweep = left * std::f64::consts::TAU;
            let start = -std::f64::consts::FRAC_PI_2;
            let arc = Arc::new(c, (radius, radius), start, sweep, 0.0);
            cx.canvas.stroke(&arc, &StrokeStyle::new(w - 2.0), t.role(Role::Accent));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.step);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::ProgressIndicator,
            range: Some((0.0, 1000.0, f64::from(self.step))),
            ..Default::default()
        })
    }
}

/// Wraps the window's content and turns keys nothing else used into
/// messages: `on_key(key, whether the window's own focus is here)`.
pub fn keys<Msg: Clone + Send + 'static>(
    child: impl Into<Element<Msg>>,
    on_key: impl Fn(&KeyEvent, bool) -> Option<Msg> + 'static,
) -> Element<Msg> {
    Element::new(Keys { on_key: Rc::new(on_key) }, vec![child.into()])
        .id("tokei-keys")
        .width(Length::Fill)
        .height(Length::Fill)
}

/// What turns a key into a message, if it means one: the key, and whether
/// the window's own focus is on the wrapper.
type OnKey<Msg> = Rc<dyn Fn(&KeyEvent, bool) -> Option<Msg>>;

struct Keys<Msg> {
    on_key: OnKey<Msg>,
}

impl<Msg: Clone + Send + 'static> Widget for Keys<Msg> {
    fn name(&self) -> &'static str {
        "tokei-keys"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if let Event::Key(k) = event
            && k.pressed
            && let Some(msg) = (self.on_key)(k, cx.is_focused())
        {
            cx.emit(&MsgFn::new(msg));
            return Status::Handled;
        }
        Status::Ignored
    }
}
