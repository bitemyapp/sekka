//! Keys for the whole window that no control takes: ⇧❖. for hidden
//! files (by the key's place, whatever the layout types with Shift).

use std::rc::Rc;

use aizome_kit::element::{Element, MsgFn};
use aizome_kit::sidestep_ui::KeyEvent;
use aizome_kit::widget::{Event, EventCx, LayoutCx, Status, Widget};
use aizome_kit::{Constraints, Point, Size};

/// The evdev code of the full stop key.
pub const KEY_DOT: u32 = 52;

/// `child`, with `on_key` hearing the keys nothing inside took.
pub fn catcher<Msg: Clone + Send + 'static>(
    child: Element<Msg>,
    on_key: impl Fn(&KeyEvent) -> Option<Msg> + 'static,
) -> Element<Msg> {
    Element::new(Catcher { on_key: Rc::new(on_key) }, vec![child])
}

/// What a key means, if anything.
type OnKey<Msg> = Rc<dyn Fn(&KeyEvent) -> Option<Msg>>;

struct Catcher<Msg> {
    on_key: OnKey<Msg>,
}

impl<Msg: Clone + Send + 'static> Widget for Catcher<Msg> {
    fn name(&self) -> &'static str {
        "hako-keys"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if let Event::Key(k) = event
            && k.pressed
            && let Some(m) = (self.on_key)(k)
        {
            cx.emit(&MsgFn::new(m));
            return Status::Handled;
        }
        Status::Ignored
    }
}
