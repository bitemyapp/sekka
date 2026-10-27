//! Keys a surface answers itself (the palette's ↑ ↓ Tab Escape, the lock
//! screen's Escape): a wrapper that hears the keys its focused child
//! didn't use, and turns the ones it wants into messages.

use std::rc::Rc;

use aizome_kit::element::{AnyMsg, Element, MsgFn};
use aizome_kit::sidestep_ui::KeyEvent;
use aizome_kit::widget::{Event, EventCx, FocusRing, Focusable, Status, Widget};

type KeyMap = Rc<dyn Fn(&KeyEvent) -> Option<AnyMsg>>;

/// `child`, with keys it leaves turned into messages by `f`.
pub fn keys<Msg: Send + 'static>(
    child: impl Into<Element<Msg>>,
    f: impl Fn(&KeyEvent) -> Option<Msg> + 'static,
) -> Element<Msg> {
    let map: KeyMap = Rc::new(move |k| f(k).map(|m| Box::new(m) as AnyMsg));
    Element::new(Keys { map, focusable: false }, vec![child.into()])
}

/// The same, and it takes the focus itself (a panel with nothing to type
/// in, so Escape still reaches it).
pub fn focus_keys<Msg: Send + 'static>(
    child: impl Into<Element<Msg>>,
    f: impl Fn(&KeyEvent) -> Option<Msg> + 'static,
) -> Element<Msg> {
    let map: KeyMap = Rc::new(move |k| f(k).map(|m| Box::new(m) as AnyMsg));
    Element::new(Keys { map, focusable: true }, vec![child.into()])
}

struct Keys {
    map: KeyMap,
    focusable: bool,
}

impl Widget for Keys {
    fn name(&self) -> &'static str {
        "keys"
    }

    fn focusable(&self) -> Focusable {
        if self.focusable { Focusable::Click } else { Focusable::No }
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Key(k) if k.pressed => match (self.map)(k) {
                Some(msg) => {
                    let cell = std::cell::RefCell::new(Some(msg));
                    cx.emit(&MsgFn::from_fn(move || cell.borrow_mut().take().expect("once")));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            _ => Status::Ignored,
        }
    }
}
