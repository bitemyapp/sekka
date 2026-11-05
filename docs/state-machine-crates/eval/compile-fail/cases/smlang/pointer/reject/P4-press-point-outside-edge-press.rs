// P4 press-point-outside-edge-press: `States::Pressed` carries `Owned`
// (hover and owner), no press point.
use impl_smlang::pointer::{Pointer, States};
use spec::pointer::{Config, Machine};

fn main() {
    let mut m = Pointer::new(Config::default());
    if let States::Pressed(owner) = m.state_enum() {
        let _ = owner.at;
    }
}
