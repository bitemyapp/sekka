// P6 owner-mismatch: enter `Pressed` with no owner. A transition into a
// state with data takes the data.
use impl_fsmentry::pointer::{Press, PressEntry};

fn main() {
    let mut press = Press::Hovering;
    if let PressEntry::Hovering(hovering) = press.entry() {
        hovering.press();
    }
}
