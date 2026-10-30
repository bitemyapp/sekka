//! Full screen, asked for and told of, and pinches over a canvas.

use aizome_kit::WindowState;
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_kit::widget::Event;

#[derive(Default)]
struct Viewer {
    window: WindowKey,
    states: Vec<WindowState>,
    zoom: f64,
}

#[derive(Clone, Debug)]
enum Msg {
    FullScreen(bool),
    Pinched(f64),
}

impl Program for Viewer {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Viewer", "Viewer", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.zoom = 1.0;
        self.window = cx.open_window(WindowSpec::new("Viewer").size(400.0, 300.0));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::FullScreen(on) => cx.set_fullscreen(self.window, on),
            Msg::Pinched(s) => self.zoom *= s,
        }
    }
    fn window_state(&mut self, window: WindowKey, state: WindowState, _cx: &mut Cx<Msg>) {
        assert_eq!(window, self.window);
        self.states.push(state);
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        canvas((), |_, _, _, _| {})
            .on_event(|ev, _, _| match ev {
                Event::Pinch { scale, .. } => Some(Msg::Pinched(*scale)),
                _ => None,
            })
            .into()
    }
}

#[test]
fn full_screen_is_asked_for_and_told_once_per_change() {
    let mut h = Harness::new(Viewer::default());
    let w = h.window();
    h.send(Msg::FullScreen(true));
    assert_eq!(h.fullscreen_asked(w), Some(true));
    let full = WindowState { fullscreen: true, maximized: false, suspended: false };
    h.window_state(w, full);
    h.window_state(w, full);
    h.window_state(w, WindowState::default());
    assert_eq!(h.program().states, [full, WindowState::default()]);
}

#[test]
fn pinches_reach_the_widget_under_them() {
    let mut h = Harness::new(Viewer::default());
    let w = h.window();
    h.pinch(w, Point::new(200.0, 150.0), 1.25);
    h.pinch(w, Point::new(200.0, 150.0), 1.25);
    assert!((h.program().zoom - 1.5625).abs() < 1e-9);
}
