//! Popups of the program's own: a panel under a desktop surface's button,
//! its content the program's view.

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_kit::{Anchors, Layer, LayerOptions, MenuPlacement};

#[derive(Default)]
struct Bar {
    bar: Option<WindowKey>,
    panel: Option<WindowKey>,
    closed: Vec<WindowKey>,
}

#[derive(Clone, Debug)]
enum Msg {
    Open,
}

impl Program for Bar {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Bar", "Bar", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let o = LayerOptions::new(Layer::Top, "bar").anchors(Anchors::TOP | Anchors::LEFT | Anchors::RIGHT);
        self.bar = Some(cx.open_window(WindowSpec::layer(o, 0.0, 24.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Open => {
                let spec = WindowSpec::new("panel").size(320.0, 200.0).kind(WindowKind::Popup {
                    parent: self.bar.unwrap(),
                    anchor: Rect::new(500.0, 0.0, 524.0, 24.0),
                    placement: MenuPlacement::Below,
                    grab: true,
                });
                self.panel = Some(cx.open_window(spec));
            }
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if Some(w) == self.panel { text("Wi-Fi").into() } else { button("Net").on_press(Msg::Open).into() }
    }
    fn close_requested(&mut self, w: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        self.closed.push(w);
        true
    }
    fn quit_when_last_window_closes(&self) -> bool {
        false
    }
}

#[test]
fn a_program_popup_opens_over_its_parent_and_shows_its_view() {
    let mut h = Harness::new(Bar::default());
    h.send(Msg::Open);
    let panel = h.program().panel.expect("opened");
    let spec = &h.platform.windows.values().last().expect("the popup").spec;
    match &spec.kind {
        aizome_kit::platform::OpenKind::Popup { anchor, grab, .. } => {
            assert_eq!(*anchor, Rect::new(500.0, 0.0, 524.0, 24.0));
            assert!(*grab);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(spec.background, None, "it draws its own ground");
    assert!(h.texts(panel).iter().any(|t| t == "Wi-Fi"));
    assert!(!h.program().quit_when_last_window_closes());
}
