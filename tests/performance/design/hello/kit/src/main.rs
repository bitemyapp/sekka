//! A label and a button in a 480 × 200 window, in Aizome's kit.
use aizome_kit::column;
use aizome_kit::prelude::*;

#[derive(Default)]
struct Hello {
    presses: u32,
}

#[derive(Clone, Debug)]
enum Msg {
    Press,
}

impl Program for Hello {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.HelloBench", "Hello", "Hello")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Hello").size(480.0, 200.0));
    }

    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Press => self.presses += 1,
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        column![text("Hello, world").size(20.0), button("Press me").on_press(Msg::Press),]
            .spacing(12.0)
            .padding(16.0)
            .into()
    }
}

fn main() {
    aizome_kit::run(Hello::default())
}
