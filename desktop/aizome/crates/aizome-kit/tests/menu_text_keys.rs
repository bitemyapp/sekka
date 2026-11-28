//! A menu item whose shortcut is also a key text is edited with leaves
//! the key to a text field or editor that has the keyboard, if it says so
//! (`MenuItem::yields_to_text`).

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::{EditorAction, EditorContent};

struct Notes {
    main: Option<WindowKey>,
    sheet: Option<WindowKey>,
    name: String,
    body: EditorContent,
    draft: String,
    did: Vec<&'static str>,
}

#[derive(Clone, Debug)]
enum Msg {
    Name(String),
    Body(EditorAction),
    Draft(String),
    Sheet,
    Did(&'static str),
}

impl Program for Notes {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.TextKeys", "Notes", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.main = Some(cx.open_window(WindowSpec::new("Notes").size(360.0, 280.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Name(s) => self.name = s,
            Msg::Body(a) => {
                self.body.perform(a);
            }
            Msg::Draft(s) => self.draft = s,
            Msg::Sheet => {
                self.sheet = Some(cx.show_sheet(self.main.unwrap()));
                cx.focus("draft");
            }
            Msg::Did(what) => self.did.push(what),
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if Some(w) == self.sheet {
            return column![text_field(&self.draft).on_change(Msg::Draft).id("draft")]
                .padding(16.0)
                .width(320.0)
                .into();
        }
        column![
            text_field(&self.name).on_change(Msg::Name).id("name"),
            button("Done").on_press(Msg::Did("done")).id("done"),
            text_editor(&self.body).on_action(Msg::Body).id("body"),
        ]
        .spacing(8.0)
        .padding(12.0)
        .into()
    }
    fn menus(&self) -> MenuBar<Msg> {
        MenuBar::standard(&self.info()).with_file_menu(
            Menu::new("File")
                .item(
                    MenuItem::new("Delete Note", Msg::Did("delete"))
                        .shortcut(Shortcut::cmd_key(ShortcutKey::Backspace))
                        .yields_to_text(),
                )
                // The same kind of key, on an item that keeps it.
                .item(MenuItem::new("Open Note", Msg::Did("open")).shortcut(Shortcut::cmd_key(ShortcutKey::Down))),
        )
    }
}

fn notes() -> (Harness<Notes>, WindowKey) {
    let notes = Notes {
        main: None,
        sheet: None,
        name: String::new(),
        body: EditorContent::new(""),
        draft: String::new(),
        did: Vec::new(),
    };
    let mut h = Harness::new(notes);
    let w = h.window();
    h.focus_window(w, true);
    (h, w)
}

const NONE: Modifiers = Modifiers::NONE;

#[test]
fn a_field_with_the_keyboard_keeps_the_key() {
    let (mut h, w) = notes();
    // Nothing has the keyboard: the key is the menu's.
    h.named(w, NamedKey::Backspace, Modifiers::SUPER);
    assert_eq!(h.program().did, ["delete"]);
    // The field has it: ❖⌫ deletes back to the start of the line, as it
    // does where no menu has the key.
    let name = h.find_id(w, "name").unwrap();
    h.click(w, name.center());
    h.type_text(w, "groceries");
    h.named(w, NamedKey::Backspace, Modifiers::SUPER);
    assert_eq!(h.program().name, "");
    assert_eq!(h.program().did, ["delete"], "the item wasn't chosen");
    // An item that doesn't yield takes its key from the field as before.
    h.type_text(w, "bread");
    h.named(w, NamedKey::ArrowDown, Modifiers::SUPER);
    assert_eq!(h.program().did, ["delete", "open"]);
    // The keyboard on a button: the menu's again, and the text stays.
    h.named(w, NamedKey::Tab, NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("done"));
    h.named(w, NamedKey::Backspace, Modifiers::SUPER);
    assert_eq!(h.program().did, ["delete", "open", "delete"]);
    assert_eq!(h.program().name, "bread");
}

#[test]
fn an_editor_with_the_keyboard_keeps_the_key() {
    let (mut h, w) = notes();
    let body = h.find_id(w, "body").unwrap();
    h.click(w, body.center());
    assert_eq!(h.focused_name(w).as_deref(), Some("body"));
    h.type_text(w, "milk");
    h.named(w, NamedKey::Enter, NONE);
    h.type_text(w, "eggs and flour");
    h.named(w, NamedKey::Backspace, Modifiers::SUPER);
    assert_eq!(h.program().body.text(), "milk\n", "the line back to its start");
    assert!(h.program().did.is_empty());
}

#[test]
fn a_field_in_a_sheet_keeps_the_key() {
    let (mut h, w) = notes();
    h.send(Msg::Sheet);
    assert_eq!(h.focused_name(w).as_deref(), Some("draft"));
    h.type_text(w, "a draft");
    // The window's commands can't act under a sheet; a key that yields to
    // text still edits the sheet's field.
    h.named(w, NamedKey::Backspace, Modifiers::SUPER);
    assert_eq!(h.program().draft, "");
    h.type_text(w, "another");
    h.named(w, NamedKey::ArrowDown, Modifiers::SUPER);
    assert!(h.program().did.is_empty());
    assert_eq!(h.program().draft, "another");
}
