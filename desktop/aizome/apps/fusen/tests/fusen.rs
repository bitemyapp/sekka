//! Fusen headlessly: notes from a folder, writing and renaming them,
//! folders, search, pins and colours, task boxes ticked with a click,
//! stickies that come back, new notes from the command line and a second
//! launch, the Trash; golden images of the window and a sticky.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use aizome_fusen::app::{Fusen, Msg, Places, Request};
use aizome_fusen::notes::NoteColor;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_kit::{Point, WindowKey};

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fusen").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

fn places(dir: &Path) -> Places {
    Places {
        notes: dir.join("Notes"),
        state: Some(dir.join("state")),
        trash: Some(dir.join("Trash")),
        autostart: Some(dir.join("autostart")),
        compositor: false,
    }
}

fn app(dir: &Path, request: Request) -> Fusen {
    let mut f = Fusen::new(places(dir), request);
    f.open_links = false;
    f
}

/// Notes as on a desk: a few, a folder, one with tasks.
fn fill(dir: &Path) {
    let n = dir.join("Notes");
    std::fs::create_dir_all(n.join("Work")).unwrap();
    let write = |p: &str, t: &str, age: u64| {
        let path = n.join(p);
        std::fs::write(&path, t).unwrap();
        // Fixed dates (a year back), so the pictures stay the same.
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_700_000) - Duration::from_secs(age);
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(when).unwrap();
    };
    write("Groceries.md", "# Groceries\n- [ ] miso\n- [x] tofu\n- [ ] negi\n", 60);
    write(
        "Tiles at fractional scale.md",
        "Tiles at fractional scale\nHalf a pixel off rather than blurred: snap the tile grid\nto device pixels and let **damage** round outward.\n",
        3600,
    );
    write(
        "Work/Release notes.md",
        "Release notes\n\n- Sumi: the board's editor\n- Fusen: stickies that float\n\nSee [the board](https://aizome.org/board).\n",
        86400 * 2,
    );
    write("Work/Desks.md", "Desks\nMail, Build, Write: one folder each.\n", 86400 * 40);
}

fn open(h: &mut Harness<Fusen>) -> WindowKey {
    let w = h.program().main.expect("the main window");
    h.focus_window(w, true);
    w
}

fn id_of(h: &Harness<Fusen>, title: &str) -> u64 {
    h.program().notes.iter().find(|n| n.title == title).unwrap_or_else(|| panic!("{title}")).id
}

fn text_of(h: &Harness<Fusen>, id: u64) -> String {
    h.program().open[&id].content.text()
}

#[test]
fn notes_come_from_the_folder_newest_first() {
    let dir = scratch("list");
    fill(&dir);
    let mut h = Harness::new(app(&dir, Request::default()));
    let w = open(&mut h);
    let titles: Vec<&str> = h.program().notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, ["Groceries", "Tiles at fractional scale", "Release notes", "Desks"]);
    assert_eq!(h.program().folders, ["Work"]);
    assert_eq!(h.program().selected, Some(id_of(&h, "Groceries")), "the newest is shown");
    assert!(h.texts(w).iter().any(|t| t == "miso"), "the preview line");
    h.send(Msg::Folder(Some("Work".into())));
    assert_eq!(h.program().shown().len(), 2);
    h.send(Msg::Folder(None));
    h.send(Msg::Search("pixel".into()));
    let shown: Vec<&str> = h.program().shown().iter().map(|n| n.title.as_str()).collect();
    assert_eq!(shown, ["Tiles at fractional scale"], "search looks in the text too");
}

#[test]
fn writing_saves_and_renames_with_the_title() {
    let dir = scratch("write");
    let mut h = Harness::new(app(&dir, Request::default()));
    let w = open(&mut h);
    assert!(!dir.join("Notes").exists(), "made on first use, not before");
    h.command(w, 'n');
    assert_eq!(h.focused_name(w).as_deref(), Some("editor"));
    h.type_text(w, "Shopping");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    h.type_text(w, "- [ ] rice");
    h.advance(Duration::from_secs(1));
    let file = dir.join("Notes/Shopping.md");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Shopping\n- [ ] rice");
    // A new title, a new name (Fusen named it).
    h.named(w, NamedKey::ArrowUp, Modifiers::SUPER);
    h.type_text(w, "Weekly ");
    h.advance(Duration::from_secs(1));
    assert!(!file.exists());
    assert_eq!(std::fs::read_to_string(dir.join("Notes/Weekly Shopping.md")).unwrap(), "Weekly Shopping\n- [ ] rice");
    // An empty new note left behind goes.
    h.command(w, 'n');
    let n = h.program().notes.len();
    h.send(Msg::Select(Some(id_of(&h, "Weekly Shopping"))));
    assert_eq!(h.program().notes.len(), n - 1);
}

#[test]
fn task_boxes_tick_with_a_click() {
    let dir = scratch("tasks");
    fill(&dir);
    let mut h = Harness::new(app(&dir, Request::default()));
    let w = open(&mut h);
    let g = id_of(&h, "Groceries");
    h.send(Msg::Select(Some(g)));
    let e = h.find_id(w, "editor").expect("the editor");
    // Find line 2's box ("- [ ] miso") with the pointer: clicks elsewhere
    // only place the caret.
    let before = text_of(&h, g);
    let mut ticked = false;
    'search: for dy in (36..80).step_by(3) {
        for dx in (16..64).step_by(3) {
            h.click(w, Point::new(e.x0 + dx as f64, e.y0 + dy as f64));
            let now = text_of(&h, g);
            if now != before {
                assert_eq!(now, before.replace("- [ ] miso", "- [x] miso"), "only miso's box");
                ticked = true;
                break 'search;
            }
        }
    }
    assert!(ticked, "{}", text_of(&h, g));
    h.advance(Duration::from_secs(1));
    assert!(std::fs::read_to_string(dir.join("Notes/Groceries.md")).unwrap().contains("- [x] miso"));
    // Ticking is one step to undo.
    h.command(w, 'z');
    assert!(text_of(&h, g).contains("- [ ] miso"));
}

#[test]
fn pins_and_colours_are_kept_beside_the_notes() {
    let dir = scratch("pins");
    fill(&dir);
    {
        let mut h = Harness::new(app(&dir, Request::default()));
        let _w = open(&mut h);
        let desks = id_of(&h, "Desks");
        h.send(Msg::Select(Some(desks)));
        h.send(Msg::Pin);
        h.send(Msg::Color(NoteColor::Hanada));
        assert_eq!(h.program().notes[0].title, "Desks", "pinned first");
    }
    let index = std::fs::read_to_string(dir.join("Notes/.fusen.json")).unwrap();
    assert!(index.contains("Work/Desks.md") && index.contains("hanada"));
    let h = Harness::new(app(&dir, Request::default()));
    let d = h.program().notes.iter().find(|n| n.title == "Desks").unwrap();
    assert!(d.meta.pinned && d.meta.color == NoteColor::Hanada);
    assert_eq!(h.program().notes[0].title, "Desks");
}

#[test]
fn stickies_float_and_come_back() {
    let dir = scratch("stickies");
    fill(&dir);
    {
        let mut h = Harness::new(app(&dir, Request::default()));
        let _w = open(&mut h);
        let g = id_of(&h, "Groceries");
        h.send(Msg::Sticky(Some(g)));
        assert_eq!(h.windows().len(), 2);
        let s = *h.program().stickies.keys().next().unwrap();
        assert_eq!(h.title(s), "Groceries");
        // Typing in the sticky edits the note.
        h.focus_window(s, true);
        let e = h.find_id(s, "sticky-editor").unwrap();
        h.click(s, Point::new(e.x1 - 10.0, e.y1 - 10.0));
        h.named(s, NamedKey::ArrowDown, Modifiers::SUPER);
        h.type_text(s, "\n- [ ] mirin");
        h.advance(Duration::from_secs(1));
        assert!(std::fs::read_to_string(dir.join("Notes/Groceries.md")).unwrap().ends_with("- [ ] mirin"));
        assert!(dir.join("autostart/org.aizome.Fusen.Stickies.desktop").exists(), "a login item brings it back");
        h.send(Msg::Quit);
    }
    // At login: only the stickies.
    let h = Harness::new(app(&dir, Request { stickies_only: true, ..Request::default() }));
    assert_eq!(h.windows().len(), 1);
    assert!(h.program().main.is_none());
    let s = *h.program().stickies.keys().next().unwrap();
    assert_eq!(h.title(s), "Groceries");
    // Closing it forgets it, and the login item.
    let mut h = h;
    h.focus_window(s, true);
    h.command(s, 'w');
    assert!(h.program().stickies.is_empty());
    assert!(!dir.join("autostart/org.aizome.Fusen.Stickies.desktop").exists());
}

#[test]
fn new_notes_from_the_command_line_and_a_second_launch() {
    let dir = scratch("cli");
    let r = Request::parse(&["--new".into(), "From the shell\nselected text".into(), "--stickies".into()]);
    assert_eq!(r.new, ["From the shell\nselected text"]);
    assert!(r.stickies_only);
    let sock = dir.join("fusen.sock");
    let aizome_sumi::instance::Claim::First(listener) = aizome_sumi::instance::claim_at(sock.clone(), &[]) else {
        panic!("first")
    };
    let mut a = app(&dir, Request { new: vec!["Hello\nfrom --new".into()], ..Request::default() });
    a.instance = Some(listener);
    let mut h = Harness::new(a);
    assert_eq!(std::fs::read_to_string(dir.join("Notes/Hello.md")).unwrap(), "Hello\nfrom --new");
    // A second launch's --new-from-stdin, as main() hands it over.
    let handed = ["--new".to_owned(), "Second\nlaunch".to_owned()];
    assert!(matches!(aizome_sumi::instance::claim_at(sock, &handed), aizome_sumi::instance::Claim::Handed));
    let until = Instant::now() + Duration::from_secs(10);
    while !dir.join("Notes/Second.md").exists() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
        h.send(Msg::Nothing);
    }
    assert_eq!(h.program().selected_note().map(|n| n.title.as_str()), Some("Second"));
}

#[test]
fn deleting_puts_a_note_in_the_trash() {
    let dir = scratch("trash");
    fill(&dir);
    let mut h = Harness::new(app(&dir, Request::default()));
    let w = open(&mut h);
    h.send(Msg::Select(Some(id_of(&h, "Desks"))));
    h.key(w, aizome_kit::sidestep_ui::Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    assert!(!dir.join("Notes/Work/Desks.md").exists());
    assert!(dir.join("Trash/files/Desks.md").exists());
    assert!(dir.join("Trash/info/Desks.md.trashinfo").exists());
    assert!(!h.program().notes.iter().any(|n| n.title == "Desks"));
}

#[test]
fn other_programs_changes_show() {
    let dir = scratch("external");
    fill(&dir);
    let mut h = Harness::new(app(&dir, Request::default()));
    let _w = open(&mut h);
    let g = id_of(&h, "Groceries");
    std::fs::write(dir.join("Notes/Groceries.md"), "# Groceries\n- [ ] an agent added this\n").unwrap();
    std::fs::write(dir.join("Notes/From an agent.md"), "From an agent\nhello\n").unwrap();
    h.advance(Duration::from_secs(4));
    assert!(text_of(&h, g).contains("an agent added this"));
    assert!(h.program().notes.iter().any(|n| n.title == "From an agent"));
}

#[test]
fn goldens() {
    for theme in [ThemeChoice::Night, ThemeChoice::Day] {
        let name = if theme == ThemeChoice::Night { "night" } else { "day" };
        let dir = scratch(&format!("golden-{name}"));
        fill(&dir);
        let mut h = Harness::with(app(&dir, Request::default()), Appearance { theme, ..Appearance::default() }, false);
        let w = open(&mut h);
        let t = id_of(&h, "Tiles at fractional scale");
        h.send(Msg::Select(Some(t)));
        h.send(Msg::Pin);
        h.send(Msg::Color(NoteColor::Hanada));
        let g = id_of(&h, "Groceries");
        h.send(Msg::Select(Some(g)));
        h.send(Msg::Color(NoteColor::Green));
        h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fusen-main-{name}@2x.png")), 2.0);
        assert_golden(&mut h, w, golden(&format!("main-{name}")), 1.0);
        if theme == ThemeChoice::Night {
            for (title, color, file) in
                [("Groceries", NoteColor::Green, "sticky-green"), ("Release notes", NoteColor::Amber, "sticky-amber")]
            {
                let id = id_of(&h, title);
                h.send(Msg::Select(Some(id)));
                h.send(Msg::Color(color));
                h.send(Msg::Sticky(Some(id)));
                let s = *h.program().stickies.iter().find(|(_, s)| s.note == id).unwrap().0;
                h.focus_window(s, true);
                let e = h.find_id(s, "sticky-editor").unwrap();
                h.click(s, Point::new(e.x1 - 10.0, e.y1 - 10.0));
                h.save_png(s, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fusen-{file}@2x.png")), 2.0);
                assert_golden(&mut h, s, golden(file), 1.0);
            }
        }
    }
}

/// Typing in a note and moving through the list, measured as Sumi's
/// typing is: update, view, layout, diff and rasterising the damage at 2×.
#[test]
fn typing_fits_the_frame_budget() {
    let dir = scratch("budget");
    fill(&dir);
    let mut h = Harness::new(app(&dir, Request::default()));
    let w = open(&mut h);
    let e = h.find_id(w, "editor").unwrap();
    h.click(w, Point::new(e.x1 - 20.0, e.y1 - 20.0));
    let _ = h.render(w, 2.0);
    let mut frames = Vec::new();
    for round in 0..4 {
        for c in "a few words in a note ".chars() {
            h.take_damage(w);
            let t = Instant::now();
            h.type_text(w, &c.to_string());
            if let Some(b) = h.take_damage(w).into_iter().reduce(|a, b| a.union(b)) {
                let _ = h.render_rect(w, b, 2.0);
            }
            if round > 0 {
                frames.push(t.elapsed());
            }
        }
        h.advance(Duration::from_secs(1));
    }
    frames.sort();
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
    eprintln!(
        "fusen typing: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
        p(0.5),
        p(0.95),
        frames.last().unwrap(),
        frames.len()
    );
    if !cfg!(debug_assertions) {
        assert!(p(0.95) < Duration::from_millis(4), "p95 {:?}", p(0.95));
    }
}
