//! Sheets, alerts and file panels.

use std::path::PathBuf;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Doc {
    main: Option<WindowKey>,
    sheet: Option<WindowKey>,
    opened: Vec<Option<Vec<PathBuf>>>,
    saved: Vec<Option<PathBuf>>,
    answer: Vec<&'static str>,
    dir: PathBuf,
}

#[derive(Clone, Debug)]
enum Msg {
    Open,
    OpenMultiple,
    OpenDirectories,
    Save,
    Opened(Option<Vec<PathBuf>>),
    Saved(Option<PathBuf>),
    Ask,
    Answer(&'static str),
    Sheet,
    CloseSheet,
}

impl Program for Doc {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Fusen", "Fusen", "Notes")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.main = Some(cx.open_window(WindowSpec::new("Notes").size(720.0, 520.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        let w = self.main.unwrap();
        match msg {
            Msg::Open => cx.open_panel(
                w,
                FileDialog::new().title("Choose documents").directory(self.dir.clone()).filter("Text", &["txt", "md"]),
                Msg::Opened,
            ),
            Msg::OpenMultiple => cx.open_panel(
                w,
                FileDialog::new()
                    .title("Choose documents")
                    .directory(self.dir.clone())
                    .filter("Text", &["txt", "md"])
                    .multiple(),
                Msg::Opened,
            ),
            Msg::OpenDirectories => cx.open_panel(
                w,
                FileDialog::new().title("Choose folders").directory(self.dir.clone()).directories().multiple(),
                Msg::Opened,
            ),
            Msg::Save => {
                cx.save_panel(w, FileDialog::new().directory(self.dir.clone()).file_name("untitled.md"), Msg::Saved)
            }
            Msg::Opened(p) => self.opened.push(p),
            Msg::Saved(p) => self.saved.push(p),
            Msg::Ask => cx.alert(
                w,
                Alert::new("Delete “notes.md”?", "It goes to the Trash; you can take it back from there.")
                    .destructive_button("Delete", Msg::Answer("delete"))
                    .cancel_button("Cancel")
                    .default_button("Keep", Msg::Answer("keep")),
            ),
            Msg::Answer(a) => self.answer.push(a),
            Msg::Sheet => self.sheet = Some(cx.show_sheet(w)),
            Msg::CloseSheet => {
                if let Some(s) = self.sheet.take() {
                    cx.close_window(s);
                }
            }
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if Some(w) == self.sheet {
            return column![
                text("Export").semibold(),
                text_field("").placeholder("Name").id("sheet-name"),
                aizome_kit::row![spacer(), button("Done").default().on_press(Msg::CloseSheet)]
            ]
            .spacing(10.0)
            .padding(16.0)
            .width(360.0)
            .into();
        }
        column![
            button("Open…").on_press(Msg::Open).id("open"),
            button("Behind").on_press(Msg::Answer("behind")).id("behind")
        ]
        .spacing(8.0)
        .padding(16.0)
        .into()
    }
}

fn fixture(name: &str) -> PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("projects/aizome")).unwrap();
    std::fs::write(d.join("notes.md"), "# Notes").unwrap();
    std::fs::write(d.join("todo.txt"), "milk").unwrap();
    std::fs::write(d.join("photo.png"), [0u8; 16]).unwrap();
    std::fs::write(d.join("projects/plan.md"), "plan").unwrap();
    std::fs::write(d.join(".hidden.md"), "").unwrap();
    d
}

fn harness(dir: PathBuf) -> (Harness<Doc>, WindowKey) {
    let mut h = Harness::new(Doc { dir, ..Doc::default() });
    let w = h.window();
    h.focus_window(w, true);
    (h, w)
}

#[test]
fn open_panels_list_navigate_and_choose() {
    let dir = fixture("panel-open");
    let (mut h, w) = harness(dir.clone());
    h.send(Msg::Open);
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "projects"));
    assert!(texts.iter().any(|t| t == "notes.md") && texts.iter().any(|t| t == "todo.txt"));
    assert!(!texts.iter().any(|t| t == "photo.png"), "filtered out");
    assert!(!texts.iter().any(|t| t == ".hidden.md"), "hidden");
    h.save_png(w, std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("open-panel.png"), 2.0);
    // The list has the keyboard: down to "projects", Return opens it.
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.texts(w).iter().any(|t| t == "plan.md"), "in projects: {:?}", h.texts(w));
    h.click_text(w, "plan.md");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().opened, [Some(vec![dir.join("projects/plan.md")])]);
    assert!(!h.texts(w).iter().any(|t| t == "plan.md"), "the panel closed");
}

#[test]
fn save_panels_take_a_name_and_escape_cancels() {
    let dir = fixture("panel-save");
    let (mut h, w) = harness(dir.clone());
    h.send(Msg::Save);
    h.command(w, 'a');
    h.type_text(w, "ideas.md");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().saved, [Some(dir.join("ideas.md"))]);
    h.send(Msg::Open);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().opened, [None]);
}

#[test]
fn multiple_open_keeps_the_range_when_confirming_or_pressing_return() {
    for (name, keyboard) in [("panel-multiple-button", false), ("panel-multiple-return", true)] {
        let dir = fixture(name);
        let (mut h, w) = harness(dir.clone());
        h.send(Msg::OpenMultiple);
        assert!(h.click_text(w, "notes.md"));
        h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
        h.save_png(w, std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.png")), 1.0);
        if keyboard {
            h.named(w, NamedKey::Enter, Modifiers::NONE);
        } else {
            assert!(h.click_text(w, "Open"));
        }
        assert_eq!(h.program().opened, [Some(vec![dir.join("notes.md"), dir.join("todo.txt")])]);
    }
}

#[test]
fn multiple_open_navigates_one_folder_and_can_choose_several_folders() {
    let dir = fixture("panel-multiple-folders");
    std::fs::create_dir(dir.join("zebra")).unwrap();
    let (mut h, w) = harness(dir.clone());
    h.send(Msg::OpenMultiple);
    assert!(h.click_text(w, "projects"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().opened.is_empty());
    assert!(h.texts(w).iter().any(|t| t == "plan.md"));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    h.send(Msg::OpenDirectories);
    assert!(!h.texts(w).iter().any(|t| t == "notes.md"));
    assert!(h.click_text(w, "projects"));
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().opened.last(), Some(&Some(vec![dir.join("projects"), dir.join("zebra")])));
}

#[test]
fn single_open_still_selects_one_and_multiple_cancel_returns_nothing() {
    let dir = fixture("panel-selection-modes");
    let (mut h, w) = harness(dir.clone());
    h.send(Msg::Open);
    assert!(h.click_text(w, "notes.md"));
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    h.save_png(w, std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("panel-single.png"), 1.0);
    assert!(h.click_text(w, "Open"));
    assert_eq!(h.program().opened, [Some(vec![dir.join("todo.txt")])]);
    h.send(Msg::OpenMultiple);
    assert!(h.click_text(w, "notes.md"));
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().opened.last(), Some(&None));
}

#[test]
fn multiple_open_rejects_mixed_or_multiple_folders_and_recovers() {
    let dir = fixture("panel-mixed-folders");
    std::fs::create_dir(dir.join("archive")).unwrap();
    let (mut h, w) = harness(dir.clone());
    h.send(Msg::OpenMultiple);
    assert!(h.click_text(w, "archive"));
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    for mixed in [false, true] {
        if mixed {
            h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
        }
        assert!(h.texts(w).iter().any(|t| t == "Select files, or open one folder."));
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert!(h.program().opened.is_empty());
        assert!(h.click_text(w, "Open"));
        assert!(h.program().opened.is_empty(), "disabled Open cannot return folders as files");
        // Clicking the disabled button moves focus; restore it to the list.
        if !mixed {
            assert!(h.click_text(w, "projects"));
        }
    }
    assert!(h.click_text(w, "notes.md"));
    assert!(!h.texts(w).iter().any(|t| t == "Select files, or open one folder."));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().opened, [Some(vec![dir.join("notes.md")])]);
}

#[test]
fn return_uses_selection_after_command_click_toggles_the_cursor_off() {
    let dir = fixture("panel-toggle-return");
    let (mut h, w) = harness(dir.clone());
    h.send(Msg::OpenMultiple);
    assert!(h.click_text(w, "notes.md"));
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    // An unmapped key updates the held modifier flags for the pointer event.
    h.named(w, NamedKey::F(35), Modifiers::SUPER);
    assert!(h.click_text(w, "todo.txt"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().opened, [Some(vec![dir.join("notes.md")])]);

    h.send(Msg::OpenMultiple);
    assert!(h.click_text(w, "notes.md"));
    h.named(w, NamedKey::F(35), Modifiers::SUPER);
    assert!(h.click_text(w, "notes.md"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().opened.len(), 1, "an empty selection must stay empty");
    // A double click on an unselected file still selects and activates it.
    let at = h.find(w, "todo.txt").unwrap().center();
    h.press(w, at, PointerButton::Left, 2);
    h.release(w, at, PointerButton::Left);
    assert_eq!(h.program().opened.last(), Some(&Some(vec![dir.join("todo.txt")])));
}

#[test]
fn alerts_answer_return_and_escape_and_block_what_is_behind() {
    let (mut h, w) = harness(fixture("alert"));
    let behind = h.find_id(w, "behind").unwrap();
    h.send(Msg::Ask);
    h.save_png(w, std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("alert.png"), 2.0);
    h.click(w, behind.center());
    assert!(h.program().answer.is_empty(), "the sheet keeps clicks from what's behind");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().answer, ["keep"]);
    h.send(Msg::Ask);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().answer, ["keep"], "cancel sends nothing");
    h.send(Msg::Ask);
    h.click_text(w, "Delete");
    assert_eq!(h.program().answer, ["keep", "delete"]);
    // Tab stays in the sheet.
    h.send(Msg::Ask);
    for _ in 0..5 {
        h.named(w, NamedKey::Tab, Modifiers::NONE);
        let f = h.focused_rect(w).unwrap();
        assert!(f.y0 < 200.0, "focus within the sheet: {f:?}");
    }
    let _ = (Key::Named(NamedKey::Tab), PointerButton::Left);
}

#[test]
fn sheets_show_the_programs_view_and_close() {
    let (mut h, w) = harness(fixture("sheet"));
    h.send(Msg::Sheet);
    assert!(h.texts(w).iter().any(|t| t == "Export"));
    h.save_png(w, std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("sheet.png"), 2.0);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(!h.texts(w).iter().any(|t| t == "Export"), "Return pressed Done");
}
