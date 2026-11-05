//! A modal owns editing; app commands must not reach its covered document.
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{FakeShell, Harness, menu_item};

#[derive(Default)]
struct Editor {
    main: Option<WindowKey>,
    sheet: Option<WindowKey>,
    text: String,
    log: Vec<&'static str>,
    selected: usize,
    close_requests: usize,
}
#[derive(Clone)]
enum Msg {
    Sheet,
    Alert,
    Close,
    Text(String),
    Action(&'static str),
    Choose(usize),
    Other,
}
impl Program for Editor {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.ModalCommands", "Editor", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.main = Some(cx.open_window(WindowSpec::new("Document").size(720.0, 520.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Sheet => {
                self.sheet = Some(cx.show_sheet(self.main.unwrap()));
                cx.focus("draft");
            }
            Msg::Alert => cx.alert(
                self.main.unwrap(),
                Alert::new("Confirm change?", "Only this alert may decide.")
                    .default_button("Apply", Msg::Action("apply"))
                    .cancel_button("Cancel"),
            ),
            Msg::Close => cx.close_window(self.sheet.take().unwrap()),
            Msg::Text(t) => self.text = t,
            Msg::Action(a) => self.log.push(a),
            Msg::Choose(i) => self.selected = i,
            Msg::Other => {
                cx.open_window(WindowSpec::new("Other").size(400.0, 300.0));
            }
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if Some(w) == self.sheet {
            return aizome_kit::column![
                text_field(&self.text).id("draft").on_change(Msg::Text),
                dropdown(["First", "Second"], Some(self.selected)).id("choice").on_select(Msg::Choose),
                button("Close Editor").cancel().on_press(Msg::Close),
            ]
            .spacing(12.0)
            .padding(16.0)
            .width(440.0)
            .into();
        }
        button("Open Editor").on_press(Msg::Sheet).into()
    }
    fn menus(&self) -> MenuBar<Msg> {
        MenuBar::standard(&self.info())
            .with_file_menu(
                Menu::new("File")
                    .item(MenuItem::new("Save", Msg::Action("save")).shortcut(Shortcut::cmd('s')))
                    .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('w'))),
            )
            .replace(
                "Edit",
                Menu::new("Edit")
                    .item(
                        MenuItem::new("Undo", Msg::Action("app undo"))
                            .role(MenuRole::Undo)
                            .shortcut(Shortcut::cmd('z')),
                    )
                    .item(MenuItem::role_item("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('a')))
                    .item(MenuItem::role_item("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('c')))
                    .item(MenuItem::role_item("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('v'))),
            )
    }
    fn close_requested(&mut self, _w: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        self.close_requests += 1;
        true
    }
}
fn opened() -> (Harness<Editor>, WindowKey) {
    let mut h = Harness::new(Editor::default());
    let w = h.window();
    h.focus_window(w, true);
    h.send(Msg::Sheet);
    (h, w)
}
#[test]
fn alert_blocks_app_shortcuts_native_close_and_undo_fallback() {
    let (mut h, w) = opened();
    h.type_text(w, "draft");
    h.send(Msg::Alert);
    for key in ['s', 'z', 'w', 'q'] {
        h.command(w, key);
    }
    h.request_close(w);
    assert!(h.program().log.is_empty());
    assert_eq!(h.program().text, "draft");
    assert_eq!(h.program().close_requests, 0);
    assert!(!h.platform.quit);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().log, ["apply"]);
    assert_eq!(h.focused_name(w).as_deref(), Some("draft"));
    h.send(Msg::Alert);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    h.command(w, 's');
    assert_eq!(h.program().log, ["apply", "save"]);
    h.request_close(w);
    assert_eq!(h.program().close_requests, 1);
}
#[test]
fn modal_text_editing_and_its_dropdown_still_work() {
    let (mut h, w) = opened();
    h.type_text(w, "draft");
    h.command(w, 'a');
    h.command(w, 'c');
    assert_eq!(h.platform.clipboard.as_deref(), Some("draft"));
    h.platform.clipboard = Some("replacement".into());
    h.command(w, 'v');
    assert_eq!(h.program().text, "replacement");
    h.command(w, 'z');
    assert_eq!(h.program().text, "draft");
    assert!(h.program().log.is_empty(), "Undo must stay in the draft field");
    let dropdown = h.find_id(w, "choice").unwrap();
    h.click(w, dropdown.center());
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().selected, 1);
    assert!(h.popups(w).is_empty());
    h.send(Msg::Alert);
    h.command(w, 'v');
    assert_eq!(h.program().text, "draft");
}
#[test]
fn commands_are_scoped_to_the_target_window() {
    let (mut h, w) = opened();
    h.send(Msg::Other);
    let other = h.windows()[1];
    h.focus_window(other, true);
    h.command(other, 's');
    assert_eq!(h.program().log, ["save"]);
    h.command(other, 'q');
    assert!(!h.platform.quit);
    assert_eq!(h.program().close_requests, 0, "Quit must not close another window's dialog");
    h.focus_window(w, true);
    h.command(w, 's');
    assert_eq!(h.program().log, ["save"]);
    h.focus_window(other, true);
    h.request_close(other);
    assert_eq!(h.program().close_requests, 1);
    assert!(h.program().sheet.is_some());
}
#[test]
fn opening_a_modal_dismisses_armed_background_menus() {
    let mut h = Harness::with(Editor::default(), aizome_kit::Appearance::default(), true);
    let w = h.window();
    h.click(w, Point::new(82.0, 12.0));
    assert_eq!(h.popups(w).len(), 1);
    h.send(Msg::Alert);
    assert!(h.popups(w).is_empty());
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().log, ["apply"]);
}
#[test]
fn shell_activation_rechecks_scope_and_publishes_dismissal() {
    let mut shell = FakeShell::new();
    let mut h = shell.launch(Editor::default());
    let w = h.window();
    h.focus_window(w, true);
    h.advance(Duration::from_millis(50));
    let menus = shell.menus();
    let save = menu_item(&menus, "Save").id;
    let undo = menu_item(&menus, "Undo").id;
    assert!(menu_item(&menus, "Save").enabled);
    h.send(Msg::Alert);
    h.advance(Duration::from_millis(50));
    let menus = shell.menus();
    assert!(!menu_item(&menus, "Save").enabled);
    assert!(!menu_item(&menus, "Undo").enabled);
    assert!(!menu_item(&menus, "Close Window").enabled);
    assert!(menu_item(&menus, "Minimise").enabled);
    for id in [save, undo] {
        shell.activate(&mut h, id);
    }
    assert!(h.program().log.is_empty(), "stale shell IDs must not bypass the modal");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    h.advance(Duration::from_millis(50));
    let menus = shell.menus();
    assert!(menu_item(&menus, "Save").enabled);
    assert!(menu_item(&menus, "Undo").enabled);
    shell.activate(&mut h, save);
    assert_eq!(h.program().log, ["save"]);
}
