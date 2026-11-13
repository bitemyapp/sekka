// P6 owner-mismatch: `Pressed` can't be built without its owner.
use impl_sml::pointer::PointerInsideStates;

fn main() {
    let _ = PointerInsideStates::Pressed();
}
