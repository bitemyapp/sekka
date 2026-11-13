// P5 hover-outside. A shape probe: the state struct holds `hot` whatever
// `pointer` says, so a hovered node with the pointer outside is a value of
// the state's type: accepted.
use impl_implicit::pointer::Fields;
use spec::pointer::{Config, Node};

fn main() {
    let outside = Fields {
        config: Config::default(),
        pointer: None,
        hot: Some(Node(3)),
        hot_under: None,
        active: None,
        active_drags_only: false,
        press_under: None,
        native_drag: false,
    };
    assert!(outside.pointer.is_none() && outside.hot.is_some());
}
