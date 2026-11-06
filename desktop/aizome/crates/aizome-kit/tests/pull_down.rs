//! A pull-down: a dropdown that is a menu of actions under a fixed title.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;

const ACTIONS: [&str; 2] = ["Tick the first twelve, which frees a lot", "Tick them all"];

#[derive(Default)]
struct Ticks {
    done: Vec<usize>,
}

#[derive(Clone, Debug)]
enum Msg {
    Act(usize),
}

impl Program for Ticks {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.PullDown", "Ticks", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Ticks").size(520.0, 200.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        let Msg::Act(i) = msg;
        self.done.push(i);
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        // The last action passed as chosen, which a pull-down never shows.
        let last = self.done.last().copied();
        aizome_kit::column![
            dropdown(ACTIONS, last).placeholder("Select").pull_down().id("select").on_select(Msg::Act),
            dropdown(ACTIONS, last).placeholder("Select").id("pop-up").on_select(Msg::Act),
        ]
        .spacing(8.0)
        .padding(16.0)
        .into()
    }
}

#[test]
fn a_pull_down_keeps_its_title_and_acts() {
    let mut h = Harness::new(Ticks::default());
    let w = h.window();
    let select = h.find_id(w, "select").unwrap();
    let popup = h.find_id(w, "pop-up").unwrap();
    assert!(select.width() < popup.width() / 2.0, "as wide as its title: {select:?} beside {popup:?}");
    h.click(w, select.center());
    let menu = h.popups(w).last().map(|(p, _)| *p).expect("its menu");
    h.popup_keyboard(menu, Key::Named(NamedKey::Home), Modifiers::default());
    h.popup_keyboard(menu, Key::Named(NamedKey::ArrowDown), Modifiers::default());
    h.popup_keyboard(menu, Key::Named(NamedKey::Enter), Modifiers::default());
    assert_eq!(h.program().done, [1]);
    // The pop-up shows the choice; the pull-down still says "Select".
    let texts = h.texts(w);
    assert_eq!(texts.iter().filter(|t| *t == "Select").count(), 1, "{texts:?}");
    assert!(texts.iter().any(|t| t == ACTIONS[1]));
    assert_eq!(h.find_id(w, "select").unwrap().width(), select.width());
}
