//! What a window writes for tests that drive a running app from outside
//! (`AIZOME_KIT_INSPECT`): its elements, where they are, and their state.

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Form {
    on: bool,
    renderer: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    Toggle(bool),
    Renderer(usize),
}

impl Program for Form {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Form", "Form", "Form")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Form").size(320.0, 200.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Toggle(on) => self.on = on,
            Msg::Renderer(i) => self.renderer = i,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        column![
            text("Wi-Fi"),
            toggle(self.on).on_toggle(Msg::Toggle).id("wifi-switch").label("Wi-Fi"),
            button("Join Other Network…"),
            segmented(["Automatic", "GPU", "Software"], self.renderer).on_select(Msg::Renderer).id("renderer"),
        ]
        .spacing(8.0)
        .padding(16.0)
        .into()
    }
}

fn find<'a>(elements: &'a [serde_json::Value], key: &str, value: &str) -> &'a serde_json::Value {
    elements.iter().find(|e| e[key] == value).unwrap_or_else(|| panic!("no element with {key} {value}: {elements:#?}"))
}

#[test]
fn elements_carry_their_names_texts_places_and_states() {
    let mut h = Harness::new(Form::default());
    let w = h.window();
    let elements = h.inspected(w);

    let label = find(&elements, "text", "Wi-Fi");
    assert_eq!(label["visible"], true);
    let switch = find(&elements, "name", "wifi-switch");
    assert_eq!(switch["role"], "switch");
    assert_eq!(switch["label"], "Wi-Fi");
    assert_eq!(switch["checked"], false);
    // The rectangle is in the window's points, and a click at its middle
    // lands on it.
    let r: Vec<f64> = switch["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(r[0] >= 16.0 && r[1] > 16.0 && r[2] > 0.0 && r[3] > 0.0, "{r:?}");
    h.click(w, Point::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0));
    assert!(h.program().on);
    assert_eq!(find(&h.inspected(w), "name", "wifi-switch")["checked"], true);

    find(&elements, "text", "Join Other Network…");
}

#[test]
fn a_segmented_control_names_its_segments_where_they_are() {
    let mut h = Harness::new(Form::default());
    let w = h.window();
    let elements = h.inspected(w);
    let seg = find(&elements, "name", "renderer");
    assert_eq!(seg["value"], "Automatic");
    let parts = seg["parts"].as_array().expect("parts");
    let names: Vec<&str> = parts.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Automatic", "GPU", "Software"]);
    // Side by side across the control, the whole height.
    let r = |v: &serde_json::Value| -> Vec<f64> { v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
    let (whole, soft) = (r(&seg["rect"]), r(&parts[2]["rect"]));
    assert!(soft[0] > whole[0] && soft[0] + soft[2] <= whole[0] + whole[2] + 0.5, "{soft:?} in {whole:?}");
    assert_eq!((soft[1], soft[3]), (whole[1], whole[3]));
    h.click(w, Point::new(soft[0] + soft[2] / 2.0, soft[1] + soft[3] / 2.0));
    assert_eq!(h.program().renderer, 2);
}

struct Tools;

impl Program for Tools {
    type Msg = ();
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Tools", "Tools", "Tools")
    }
    fn launched(&mut self, cx: &mut Cx<()>) {
        cx.open_window(WindowSpec::new("Tools").size(200.0, 80.0));
    }
    fn update(&mut self, _msg: (), _cx: &mut Cx<()>) {}
    fn view(&self, _w: WindowKey) -> Element<()> {
        aizome_kit::row![
            icon_button(Icon::ArrowUp).on_press(()).tooltip("Move up"),
            button("Remove").on_press(()).tooltip("Remove this layout"),
        ]
        .padding(16.0)
        .into()
    }
}

#[test]
fn an_icon_button_is_named_by_its_tooltip_and_a_text_button_by_its_text() {
    let h = Harness::new(Tools);
    let w = h.window();
    let labels: Vec<String> = h.semantics(w).into_iter().filter_map(|(s, _)| s.label).collect();
    assert!(labels.iter().any(|l| l == "Move up"), "{labels:?}");
    assert!(labels.iter().any(|l| l == "Remove"), "{labels:?}");
    assert!(!labels.iter().any(|l| l == "arrow-up"), "{labels:?}");
    let elements = h.inspected(w);
    find(&elements, "label", "Move up");
}
