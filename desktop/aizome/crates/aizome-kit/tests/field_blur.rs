//! A field that keeps its text only when editing ends hears of it when
//! the focus goes elsewhere, not only on Return.

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Name {
    draft: String,
    saved: Vec<String>,
}

#[derive(Clone, Debug)]
enum Msg {
    Edit(String),
    Save,
}

impl Program for Name {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Name", "Name", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Name").size(320.0, 160.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Edit(s) => self.draft = s,
            Msg::Save => self.saved.push(self.draft.clone()),
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        column![
            text_field(&self.draft).on_change(Msg::Edit).on_submit(Msg::Save).on_blur(Msg::Save).id("name"),
            text_field("").id("other"),
        ]
        .spacing(8.0)
        .padding(16.0)
        .into()
    }
}

#[test]
fn clicking_elsewhere_ends_editing() {
    let mut h = Harness::new(Name::default());
    let w = h.window();
    let name = h.find_id(w, "name").unwrap();
    h.click(w, name.center());
    h.type_text(w, "Mail");
    assert!(h.program().saved.is_empty());
    let other = h.find_id(w, "other").unwrap();
    h.click(w, other.center());
    assert_eq!(h.program().saved, ["Mail"]);
}
