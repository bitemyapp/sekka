// P3 click-under-after-drag: `States::DragsOnlyDragging` carries `Owned`
// (hover and owner); the `beneath` of the click under is gone.
use impl_smlang::pointer::{Pointer, States};
use spec::pointer::{Config, Machine};

fn main() {
    let mut m = Pointer::new(Config::default());
    if let States::DragsOnlyDragging(edge) = m.state_enum() {
        let _ = edge.beneath;
    }
}
