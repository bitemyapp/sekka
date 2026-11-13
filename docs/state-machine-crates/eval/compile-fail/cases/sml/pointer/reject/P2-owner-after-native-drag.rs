// P2 owner-after-native-drag: the root state `NativeDrag` has no payload; a
// press owner is the payload of `Inside`'s child states only. (The inactive
// child keeps its last state: after a drag from `Pressed(n)`,
// `is(&PointerInsideStates::Pressed(_))` still answers true, so `state()`
// tests the root first. Its owner can't be read from outside, and the child
// resets on re-entry, so nothing routes to it.)
use impl_sml::pointer::{Pointer, PointerStates};
use spec::pointer::{Config, Machine};

fn main() {
    let m = Pointer::new(Config::default());
    m.machine.visit_current_states(|state| {
        if let PointerStates::NativeDrag(owner) = state {
            let _ = owner;
        }
    });
}
