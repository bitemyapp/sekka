//! Publishing menus to the shell over its socket, and acting on what it
//! sends back; drops from other programs.

use std::time::Duration;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::testing::{FakeShell, Harness, menu_item};
use aizome_shell_proto as proto;

#[derive(Default)]
struct Notes {
    log: Vec<String>,
    dirty: bool,
    dropped: Vec<String>,
    text: String,
}

#[derive(Clone, Debug)]
enum Msg {
    New,
    Save,
    Dropped(Vec<String>),
    Typed(String),
}

impl Program for Notes {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Fusen", "Fusen", "Notes")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Notes").size(400.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::New => {
                self.log.push("new".into());
                self.dirty = true;
            }
            Msg::Save => self.log.push("save".into()),
            Msg::Dropped(u) => self.dropped = u,
            Msg::Typed(t) => self.text = t,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        column![
            aizome_kit::mouse_area(container(text("Drop here")).padding(40.0).width(Fill))
                .on_drop(|d: aizome_kit::Dropped| Msg::Dropped(d.urls))
                .id("drop"),
            text_field(&self.text).on_change(Msg::Typed).id("note"),
        ]
        .into()
    }
    fn menus(&self) -> MenuBar<Msg> {
        MenuBar::standard(&self.info()).with_file_menu(
            Menu::new("File")
                .item(MenuItem::new("New Note", Msg::New).shortcut(Shortcut::cmd('N')))
                .item(MenuItem::new("Save", Msg::Save).shortcut(Shortcut::cmd('S')).enabled(self.dirty)),
        )
    }
}

#[test]
fn menus_go_to_the_shell_and_its_choices_come_back() {
    let mut shell = FakeShell::new();
    let mut h = shell.launch(Notes::default());
    let w = h.window();
    let hello = shell.read();
    assert!(matches!(hello, proto::AppMessage::Hello { ref app_id, .. } if app_id == "org.aizome.Fusen"));
    // The menus go out once the debounce passes.
    h.advance(Duration::from_millis(50));
    let proto::AppMessage::Menus { menus } = shell.read() else { panic!("menus") };
    let titles: Vec<&str> = menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Fusen", "File", "Edit", "Window", "Help"]);
    let new = menu_item(&menus, "New Note");
    assert_eq!(new.shortcut.as_ref().map(|s| s.label()), Some("❖N".to_owned()));
    assert!(!menu_item(&menus, "Save").enabled);
    // The shell chooses New Note.
    shell.activate(&mut h, new.id);
    assert_eq!(h.program().log, ["new"]);
    // Save is enabled now: the menus go out again.
    h.advance(Duration::from_millis(50));
    let proto::AppMessage::Menus { menus } = shell.read() else { panic!("menus") };
    assert!(menu_item(&menus, "Save").enabled);
    // No in-window bar with a shell.
    assert!(!h.texts(w).iter().any(|t| t == "File"));
    // Help ▸ Search asks the shell for its palette, on the app's commands.
    let search = menu_item(&menus, "Search");
    assert_eq!(search.role.as_deref(), Some("help-search"));
    assert_eq!(menu_item(&menus, "Aizome Tour").role.as_deref(), Some("tour"));
    shell.activate(&mut h, search.id);
    let asked = shell.read();
    assert_eq!(
        asked,
        proto::AppMessage::Request {
            request: proto::ShellRequest::Palette { scope: Some("commands".into()), query: String::new() }
        }
    );
}

#[test]
fn drops_reach_mouse_areas_that_take_them() {
    let mut h = Harness::new(Notes::default());
    let w = h.window();
    let d = h.find_id(w, "drop").unwrap();
    assert!(h.drop(w, d.center(), vec!["file:///home/callen/notes.md".into()], None));
    assert_eq!(h.program().dropped, ["file:///home/callen/notes.md"]);
    assert!(!h.drop(w, Point::new(d.x0 + 5.0, d.y1 + 50.0), vec!["file:///x".into()], None), "nothing takes it there");
}

#[test]
fn without_a_shell_the_menu_bar_is_in_the_window() {
    let mut h = Harness::with(Notes::default(), aizome_kit::Appearance::default(), true);
    let w = h.window();
    let png = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("menubar.png");
    // "Fusen" in bold, then "File".
    h.click(w, Point::new(82.0, 12.0));
    assert_eq!(h.popups(w).len(), 1, "the File menu opened");
    h.save_png(w, &png, 2.0);
    h.named(w, aizome_kit::sidestep_ui::NamedKey::ArrowDown, aizome_kit::sidestep_ui::Modifiers::NONE);
    h.named(w, aizome_kit::sidestep_ui::NamedKey::Enter, aizome_kit::sidestep_ui::Modifiers::NONE);
    assert_eq!(h.program().log, ["new"]);
}

#[test]
fn a_shell_that_comes_late_gets_the_menus_and_one_that_goes_gives_them_back() {
    let mut shell = FakeShell::unheard();
    // Started before the shell, as at login: the menus are the window's.
    let mut h = Harness::waiting_for_shell(Notes::default(), shell.path(), Duration::from_millis(20));
    let w = h.window();
    assert!(!h.has_shell());
    assert!(bar_in(&h, w), "the menu bar is in the window");

    // The shell comes: the app finds it, says hello, and sends its menus.
    shell.listen();
    shell.accept();
    assert!(matches!(shell.read(), proto::AppMessage::Hello { .. }));
    assert!(h.wait_for_shell(true, Duration::from_secs(5)));
    let proto::AppMessage::Menus { menus } = shell.read() else { panic!("menus") };
    assert!(menus.iter().any(|m| m.title == "File"));
    assert!(!bar_in(&h, w), "the window's bar has gone to the shell");
    h.focus_window(w, true);
    let field = h.find_id(w, "note").expect("the field");
    h.click(w, field.center());
    h.type_text(w, "a");

    // The shell goes: the menus come back into the window, and the field
    // keeps the focus (the shell restarting doesn't take it).
    shell.hang_up();
    assert!(h.wait_for_shell(false, Duration::from_secs(5)));
    assert!(bar_in(&h, w), "the menu bar is back in the window");
    assert_eq!(h.focused_name(w).as_deref(), Some("note"));
    h.type_text(w, "b");
    assert_eq!(h.program().text, "ab");

    // And it comes back once more.
    shell.listen();
    shell.accept();
    assert!(matches!(shell.read(), proto::AppMessage::Hello { .. }));
    assert!(h.wait_for_shell(true, Duration::from_secs(5)));
}

/// Whether window `w` draws its own menu bar.
fn bar_in(h: &Harness<Notes>, w: WindowKey) -> bool {
    h.semantics(w).iter().any(|(s, _)| s.role == aizome_kit::widget::SemanticRole::MenuBar)
}
