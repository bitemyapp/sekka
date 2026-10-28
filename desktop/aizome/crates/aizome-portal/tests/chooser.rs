//! The chooser driven as a person would, on the kit's stand-in platform,
//! in a home of its own: what it lists, what each mode answers, and what
//! it remembers.

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_portal::chooser::model::Loc;
use aizome_portal::chooser::{Chooser, Msg};
use aizome_portal::memory::Memory;
use aizome_portal::request::{Choice, Filter, Mode, Rule, bytes_of_path};
use hako::model::uri::file_uri;

mod common;
use common::*;

fn names(h: &Harness<Chooser>) -> Vec<String> {
    h.program().browser.shown.iter().map(|e| e.display.clone()).collect()
}

fn row_of(h: &Harness<Chooser>, name: &str) -> usize {
    names(h).iter().position(|n| n == name).unwrap_or_else(|| panic!("no “{name}” in {:?}", names(h)))
}

fn uris(h: &Harness<Chooser>) -> Vec<String> {
    h.program().answer.as_ref().expect("an answer").uris.clone()
}

#[test]
fn opens_over_the_app_that_asked() {
    let home = Home::new("parent");
    let h = chooser(&home, request(Mode::Open, &home.path("Documents")));
    let w = h.window();
    let opened = h.platform.windows.values().next().unwrap();
    assert_eq!(opened.spec.foreign_parent.as_deref(), Some("exported-1"));
    assert_eq!(h.title(w), "Open File");
    assert!(h.texts(w).iter().any(|t| t == "Open"));
}

#[test]
fn lists_folders_first_without_hidden_files() {
    let home = Home::new("list");
    let mut h = chooser(&home, request(Mode::Open, &home.path("Documents")));
    assert_eq!(names(&h), ["Taxes", "notes.md", "report.pdf"]);
    h.send(Msg::Hidden(true));
    assert_eq!(names(&h), ["Taxes", ".secret", "notes.md", "report.pdf"]);
}

#[test]
fn opens_a_file_and_navigates_into_folders() {
    let home = Home::new("open");
    let mut h = chooser(&home, request(Mode::Open, &home.path("")));
    // A folder activated is entered, not chosen.
    let docs = row_of(&h, "Documents");
    h.send(Msg::Activate(docs));
    settle(&mut h);
    assert_eq!(h.program().browser.loc, Loc::Folder(home.path("Documents")));
    assert!(h.program().answer.is_none());
    h.send(Msg::Select(vec![row_of(&h, "notes.md")]));
    h.send(Msg::Accept);
    assert_eq!(uris(&h), [file_uri(&home.path("Documents/notes.md"))]);
    assert!(h.program().answer.as_ref().unwrap().writable);
    assert!(h.platform.quit);
    // The next chooser starts where this one chose.
    let memory = Memory::load(&home.path(".state/file-chooser.json"));
    assert_eq!(memory.folder(""), Some(home.path("Documents")));
}

#[test]
fn back_forward_and_up() {
    let home = Home::new("history");
    let mut h = chooser(&home, request(Mode::Open, &home.path("Documents")));
    h.send(Msg::Up);
    settle(&mut h);
    assert_eq!(h.program().browser.loc, Loc::Folder(home.path("")));
    // Up selects the folder it came from.
    assert_eq!(h.program().browser.selected, vec![row_of(&h, "Documents")]);
    h.send(Msg::Back);
    settle(&mut h);
    assert_eq!(h.program().browser.loc, Loc::Folder(home.path("Documents")));
    h.send(Msg::Forward);
    settle(&mut h);
    assert_eq!(h.program().browser.loc, Loc::Folder(home.path("")));
}

#[test]
fn filters_by_pattern_and_type_and_remembers_the_choice() {
    let home = Home::new("filters");
    let images =
        Filter { name: "Images".into(), rules: vec![Rule::Glob("*.jpg".into()), Rule::Mime("image/png".into())] };
    let text = Filter { name: "Text".into(), rules: vec![Rule::Mime("text/plain".into())] };
    let mut r = request(Mode::Open, &home.path("Pictures"));
    r.filters = vec![images.clone(), text.clone()];
    r.current_filter = Some(text.clone());
    let mut h = chooser(&home, r);
    assert_eq!(names(&h), ["readme.txt"]);
    h.send(Msg::Filter(0));
    assert_eq!(names(&h), ["cat.png", "DOG.JPG"]);
    h.send(Msg::Select(vec![0]));
    h.send(Msg::Accept);
    assert_eq!(h.program().answer.as_ref().unwrap().current_filter, Some(images));
}

#[test]
fn opens_several_but_not_folders_with_them() {
    let home = Home::new("multiple");
    let mut r = request(Mode::Open, &home.path("Documents"));
    r.multiple = true;
    let mut h = chooser(&home, r);
    h.send(Msg::Select(vec![0, 1]));
    h.send(Msg::Accept);
    assert!(h.program().answer.is_none());
    assert!(h.program().browser.problem.is_some());
    h.send(Msg::Select(vec![row_of(&h, "notes.md"), row_of(&h, "report.pdf")]));
    h.send(Msg::Accept);
    assert_eq!(uris(&h), [file_uri(&home.path("Documents/notes.md")), file_uri(&home.path("Documents/report.pdf"))]);
}

#[test]
fn chooses_a_folder_or_the_one_it_is_in() {
    let home = Home::new("directory");
    let mut r = request(Mode::Open, &home.path("Documents"));
    r.directory = true;
    let mut h = chooser(&home, r);
    assert_eq!(names(&h), ["Taxes"]);
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "Choose"));
    h.send(Msg::Accept);
    assert_eq!(uris(&h), [file_uri(&home.path("Documents"))]);

    let mut r = request(Mode::Open, &home.path("Documents"));
    r.directory = true;
    let mut h = chooser(&home, r);
    h.send(Msg::Select(vec![0]));
    h.send(Msg::Accept);
    assert_eq!(uris(&h), [file_uri(&home.path("Documents/Taxes"))]);
}

#[test]
fn saves_a_new_file_with_the_choices() {
    let home = Home::new("save");
    let mut r = request(Mode::Save, &home.path("Documents"));
    r.current_name = Some("summary.md".into());
    r.title = "Export Summary".into();
    r.accept_label = Some("Export".into());
    r.choices = vec![
        Choice {
            id: "format".into(),
            label: "Format".into(),
            options: vec![("md".into(), "Markdown".into()), ("txt".into(), "Plain text".into())],
            default: "md".into(),
        },
        Choice { id: "toc".into(), label: "Contents".into(), options: vec![], default: "false".into() },
    ];
    let mut h = chooser(&home, r);
    let w = h.window();
    assert_eq!(h.title(w), "Export Summary");
    assert_eq!(h.focused_name(w).as_deref(), Some("chooser-name"));
    h.send(Msg::Choice(0, "txt".into()));
    h.send(Msg::Choice(1, "true".into()));
    h.named(w, NamedKey::Enter, Modifiers::default());
    let a = h.program().answer.as_ref().expect("saved");
    assert_eq!(a.uris, [file_uri(&home.path("Documents/summary.md"))]);
    assert_eq!(a.choices, [("format".into(), "txt".into()), ("toc".into(), "true".into())]);
    assert!(!a.writable);
}

#[test]
fn asks_before_replacing() {
    let home = Home::new("replace");
    let mut r = request(Mode::Save, &home.path("Documents"));
    r.current_file = Some(bytes_of_path(&home.path("Documents/notes.md")));
    let mut h = chooser(&home, r);
    // Save As: the file's own name and folder, the file selected.
    assert_eq!(h.program().browser.name, "notes.md");
    assert_eq!(h.program().browser.selected, vec![row_of(&h, "notes.md")]);
    h.send(Msg::Accept);
    let w = h.window();
    assert!(h.program().answer.is_none());
    assert!(h.texts(w).iter().any(|t| t.contains("already exists")), "{:?}", h.texts(w));
    assert!(h.click_text(w, "Replace"));
    assert_eq!(uris(&h), [file_uri(&home.path("Documents/notes.md"))]);
}

#[test]
fn a_typed_folder_is_gone_into_and_a_bad_name_is_explained() {
    let home = Home::new("typed");
    let mut h = chooser(&home, request(Mode::Save, &home.path("")));
    h.send(Msg::Name("Documents/Taxes".into()));
    h.send(Msg::Accept);
    settle(&mut h);
    assert_eq!(h.program().browser.loc, Loc::Folder(home.path("Documents/Taxes")));
    assert_eq!(h.program().browser.name, "");
    h.send(Msg::Name("nowhere/x.txt".into()));
    h.send(Msg::Accept);
    assert!(h.program().browser.problem.as_deref().is_some_and(|p| p.contains("no folder")));
    h.send(Msg::Name("~/Downloads/from-home.txt".into()));
    h.send(Msg::Accept);
    assert_eq!(uris(&h), [file_uri(&home.path("Downloads/from-home.txt"))]);
}

#[test]
fn a_new_filter_changes_the_extension() {
    let home = Home::new("extension");
    let mut r = request(Mode::Save, &home.path(""));
    r.current_name = Some("drawing.png".into());
    r.filters = vec![
        Filter { name: "PNG".into(), rules: vec![Rule::Glob("*.png".into())] },
        Filter { name: "JPEG".into(), rules: vec![Rule::Glob("*.jpg".into())] },
    ];
    let mut h = chooser(&home, r);
    h.send(Msg::Filter(1));
    assert_eq!(h.program().browser.name, "drawing.jpg");
}

#[test]
fn saves_several_files_without_overwriting() {
    let home = Home::new("save-files");
    let mut r = request(Mode::SaveFiles, &home.path("Documents"));
    r.files = vec![b"notes.md".to_vec(), b"new.txt".to_vec(), b"new.txt".to_vec()];
    let mut h = chooser(&home, r);
    h.send(Msg::Accept);
    assert_eq!(
        uris(&h),
        [
            file_uri(&home.path("Documents/notes 2.md")),
            file_uri(&home.path("Documents/new.txt")),
            file_uri(&home.path("Documents/new 2.txt")),
        ]
    );
}

#[test]
fn makes_a_folder_and_saves_in_it() {
    let home = Home::new("new-folder");
    let mut h = chooser(&home, request(Mode::Save, &home.path("")));
    h.send(Msg::NewFolder);
    h.send(Msg::SheetText("Drafts".into()));
    h.send(Msg::SheetSubmit);
    settle(&mut h);
    assert!(home.path("Drafts").is_dir());
    assert_eq!(h.program().browser.loc, Loc::Folder(home.path("Drafts")));
    // Making it again explains why not.
    h.send(Msg::Up);
    settle(&mut h);
    h.send(Msg::NewFolder);
    h.send(Msg::SheetText("Drafts".into()));
    h.send(Msg::SheetSubmit);
    assert!(h.program().sheet_problem().is_some_and(|p| p.contains("already")));
}

#[test]
fn goes_to_a_typed_folder_or_file() {
    let home = Home::new("go-to");
    let mut h = chooser(&home, request(Mode::Open, &home.path("Pictures")));
    h.send(Msg::GoTo);
    h.send(Msg::SheetText("~/Documents/report.pdf".into()));
    h.send(Msg::SheetSubmit);
    settle(&mut h);
    assert_eq!(h.program().browser.loc, Loc::Folder(home.path("Documents")));
    assert_eq!(h.program().browser.selected, vec![row_of(&h, "report.pdf")]);
    h.send(Msg::GoTo);
    h.send(Msg::SheetText("../nowhere".into()));
    h.send(Msg::SheetSubmit);
    assert!(h.program().sheet_problem().is_some());
}

#[test]
fn cancelling_answers_nothing() {
    let home = Home::new("cancel");
    let mut h = chooser(&home, request(Mode::Open, &home.path("")));
    let w = h.window();
    h.named(w, NamedKey::Escape, Modifiers::default());
    assert!(h.platform.quit);
    assert!(h.program().answer.is_none());
}

#[test]
fn an_unreadable_folder_says_so() {
    let home = Home::new("unreadable");
    let h = chooser(&home, request(Mode::Open, &home.path("")));
    let mut h = h;
    h.send(Msg::Go(Loc::Folder(home.path("gone"))));
    settle(&mut h);
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t.contains("isn't there")), "{:?}", h.texts(w));
}
