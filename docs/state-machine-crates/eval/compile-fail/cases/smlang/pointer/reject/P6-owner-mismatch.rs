// P6 owner-mismatch: `States::DragsOnlyPressed`'s data can't be built with
// a `beneath` but no drags-only `edge` owning the press.
use impl_smlang::pointer::{EdgePress, Hover, States};
use spec::pointer::{Node, Point};

fn main() {
    let _ = States::DragsOnlyPressed(EdgePress {
        hover: Hover::default(),
        at: Point::new(0, 0),
        clicks: 1,
        beneath: Some(Node(2)),
    });
}
