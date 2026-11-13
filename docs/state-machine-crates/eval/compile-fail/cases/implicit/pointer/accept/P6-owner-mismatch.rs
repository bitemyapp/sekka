// P6 owner-mismatch. A shape probe: a drags-only owner flag with no owner,
// and a click under naming another node than `active`, are values of the
// state's type: accepted.
use impl_implicit::pointer::Fields;
use spec::pointer::{Config, Node, Point};

fn main() {
    let mismatched = Fields {
        config: Config::default(),
        pointer: Some(Point::new(0, 0)),
        hot: None,
        hot_under: None,
        active: Some(Node(1)),
        active_drags_only: true,
        press_under: Some((Node(2), Point::new(0, 0), 1, None)),
        native_drag: false,
    };
    assert_ne!(mismatched.active.map(|n| n.0), mismatched.press_under.map(|u| u.0 .0));
}
