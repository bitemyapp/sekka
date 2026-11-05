// P3 click-under-after-drag: release out of `DragsOnlyDragging` and click
// what was beneath. Its `release` hands back its data, the edge alone; only
// `DragsOnlyPressed::release` returns the `EdgePress` with `beneath`.
use impl_fsmentry::pointer::{EdgePress, Press, PressEntry};
use spec::pointer::Node;

fn main() {
    let mut press = Press::DragsOnlyDragging(Node(9));
    if let PressEntry::DragsOnlyDragging(dragging) = press.entry() {
        let EdgePress { beneath, .. } = dragging.release();
        let _ = beneath;
    }
}
