// P4 press-point-outside-edge-press: read the press point in `Pressed`,
// whose data is the owner alone.
use impl_fsmentry::pointer::{Press, PressEntry};
use spec::pointer::Node;

fn main() {
    let mut press = Press::Pressed(Node(1));
    if let PressEntry::Pressed(pressed) = press.entry() {
        let _ = pressed.as_ref().at;
    }
}
