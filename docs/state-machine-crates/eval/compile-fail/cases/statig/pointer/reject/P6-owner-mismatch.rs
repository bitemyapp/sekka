// P6 owner-mismatch: `State::Pressed` can't be built without its owner.
use impl_statig::pointer::{Hover, State};

fn main() {
    let _ = State::Pressed { hover: Hover::default() };
}
