// P3 click-under-after-drag: `State::EdgeDragging` keeps no `beneath`.
use impl_statig::pointer::{Pointer, State};
use spec::pointer::{Config, Machine};

fn main() {
    let m = Pointer::new(Config::default());
    if let State::EdgeDragging { beneath, .. } = m.state_enum() {
        let _ = beneath;
    }
}
