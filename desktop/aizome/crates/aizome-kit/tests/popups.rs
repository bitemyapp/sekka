//! Popups placed anew while open: popovers follow their anchors.

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Anchored {
    gap: f64,
}

#[derive(Clone, Debug)]
enum Msg {
    Gap(f64),
}

impl Program for Anchored {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Anchored", "Anchored", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Anchored").size(400.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        let Msg::Gap(g) = msg;
        self.gap = g;
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        column![space(self.gap), popover(button("Details").id("anchor"), text("More"), true)].padding(10.0).into()
    }
}

#[test]
fn a_popover_follows_its_anchor() {
    let mut h = Harness::new(Anchored::default());
    let w = h.window();
    let (p, before) = h.popups(w)[0];
    let anchor = h.find_id(w, "anchor").unwrap();
    assert!((before.y0 - anchor.y1).abs() < 0.5, "under its anchor: {before:?}, {anchor:?}");
    h.send(Msg::Gap(60.0));
    let (q, after) = h.popups(w)[0];
    assert_eq!(p, q, "the same popup, placed anew");
    assert_eq!(after.y0 - before.y0, 60.0, "{after:?} from {before:?}");
}
