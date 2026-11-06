//! A first look: a window of controls drawn offscreen.

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::Modifiers;
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Demo {
    on: bool,
    checked: bool,
    seg: usize,
    value: f64,
    clicks: u32,
    shift_toggles: u32,
}

#[derive(Clone, Debug)]
enum Msg {
    Toggle(bool),
    Check(bool),
    ShiftCheck(bool),
    Seg(usize),
    Slide(f64),
    Click,
}

impl Program for Demo {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Demo", "Demo", "Demo")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Demo").size(420.0, 260.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Toggle(b) => self.on = b,
            Msg::Check(b) => self.checked = b,
            Msg::ShiftCheck(b) => {
                self.checked = b;
                self.shift_toggles += 1;
            }
            Msg::Seg(i) => self.seg = i,
            Msg::Slide(v) => self.value = v,
            Msg::Click => self.clicks += 1,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        column![
            text("Controls").size(15.0).semibold(),
            row![
                button("Cancel").on_press(Msg::Click),
                button("Save").primary().on_press(Msg::Click),
                button("Delete").destructive().on_press(Msg::Click),
                icon_button(Icon::Settings).on_press(Msg::Click),
            ]
            .spacing(8.0),
            row![text("Wi-Fi"), spacer(), toggle(self.on).on_toggle(Msg::Toggle)].width(Fill),
            checkbox("Show hidden files", self.checked).on_toggle(Msg::Check).on_shift_toggle(Msg::ShiftCheck),
            segmented(["Night", "Day", "Auto"], self.seg).on_select(Msg::Seg),
            slider(self.value, ValueRange::UNIT).on_change(Msg::Slide),
            text(format!("{} clicks", self.clicks)).role(Role::Text3).mono(),
        ]
        .spacing(10.0)
        .padding(16.0)
        .into()
    }
}

#[test]
fn controls_draw_and_answer() {
    let mut h = Harness::new(Demo::default());
    let w = h.window();
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("smoke.png");
    h.save_png(w, &out, 2.0);
    eprintln!("wrote {}", out.display());
    assert!(h.click_text(w, "Save"));
    assert_eq!(h.program().clicks, 1);
    assert!(h.texts(w).iter().any(|t| t == "1 clicks"));
}

#[test]
fn a_shift_click_on_a_checkbox_says_so() {
    let mut h = Harness::new(Demo::default());
    let w = h.window();
    let at = h.find(w, "Show hidden files").unwrap().center();
    h.click(w, at);
    assert!(h.program().checked);
    assert_eq!(h.program().shift_toggles, 0);
    h.click_with(w, at, Modifiers::SHIFT);
    assert!(!h.program().checked, "it still says what the box becomes");
    assert_eq!(h.program().shift_toggles, 1);
    // Shift is let go after the click.
    h.click(w, at);
    assert_eq!(h.program().shift_toggles, 1);
}
