// P3 click-under-after-drag: `Press::EdgeDrag` keeps no `beneath`.
use impl_enum::pointer::{Pointer, Press, State};
use spec::pointer::{Config, Machine};

fn main() {
    let m = Pointer::new(Config::default());
    if let State::Inside { press: Press::EdgeDrag { edge, beneath }, .. } = m.typed() {
        let _ = (edge, beneath);
    }
}
