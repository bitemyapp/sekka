//! Sumi headlessly: the board's editor (golden images in Night and Day),
//! typing Markdown, the Format menu's commands and shortcuts, find and
//! replace, encodings and line endings through open and save, autosave and
//! restoring after a crash, recent files, closing with unsaved changes,
//! one instance, large files, and what typing and scrolling cost.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_kit::{EditorAction, Point, Selection, Vec2, WindowKey};
use aizome_sumi::app::{Command, Msg, Places, Sumi};
use aizome_sumi::files::{Encoding, LineEnding};
use aizome_sumi::format::Inline;
use aizome_sumi::syntax::Language;

const NONE: Modifiers = Modifiers::NONE;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

/// A folder of the test's own.
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sumi").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app(places: Places) -> Sumi {
    let mut s = Sumi::new(places);
    s.open_links = false;
    // Tests wait for line colours rather than seeing guesses.
    s.highlight_budget = Duration::from_secs(10);
    s
}

fn sumi() -> Harness<Sumi> {
    Harness::new(app(Places::default()))
}

fn sumi_in(theme: ThemeChoice) -> Harness<Sumi> {
    Harness::with(app(Places::default()), Appearance { theme, ..Appearance::default() }, false)
}

fn doc(h: &Harness<Sumi>, w: WindowKey) -> &aizome_sumi::doc::Doc {
    &h.program().docs[&w]
}

fn text(h: &Harness<Sumi>, w: WindowKey) -> String {
    doc(h, w).content.text()
}

fn focus_editor(h: &mut Harness<Sumi>, w: WindowKey) {
    h.focus_window(w, true);
    let e = h.find_id(w, "editor").expect("the editor");
    h.click(w, Point::new(e.x1 - 30.0, e.y1 - 10.0));
}

fn select(h: &mut Harness<Sumi>, w: WindowKey, anchor: usize, head: usize) {
    h.send(Msg::Edit(w, EditorAction::Select(Selection { anchor, head })));
}

fn char_at(h: &Harness<Sumi>, w: WindowKey, line: usize, col: usize) -> usize {
    doc(h, w).content.rope().line_to_char(line) + col
}

/// Wait for work on other threads (a big file read, a search).
fn settle(h: &mut Harness<Sumi>, done: impl Fn(&Harness<Sumi>) -> bool) {
    let until = Instant::now() + Duration::from_secs(30);
    while !done(h) {
        assert!(Instant::now() < until, "timed out");
        std::thread::sleep(Duration::from_millis(5));
        h.send(Msg::Nothing);
    }
}

/// The board's window: architecture.md at line 38, a selection from
/// "fills" on line 48 into line 49.
fn the_board(h: &mut Harness<Sumi>) -> WindowKey {
    let w = h.window();
    h.send(Msg::Opened(Some(vec![fixture("architecture.md")])));
    assert_eq!(h.title(w), "architecture.md");
    focus_editor(h, w);
    let e = h.find_id(w, "editor").unwrap();
    h.scroll(w, Point::new(e.x0 + 200.0, e.y0 + 100.0), Vec2::new(0.0, 37.0 * 21.0));
    // The position bar shows while it scrolls, and goes after.
    std::thread::sleep(Duration::from_millis(950));
    h.advance(Duration::from_secs(1));
    let (a, b) = (char_at(h, w, 48, 15), char_at(h, w, 47, 9));
    select(h, w, a, b);
    w
}

#[test]
fn the_boards_editor() {
    let mut h = sumi();
    let w = the_board(&mut h);
    assert_eq!(doc(&h, w).language, Language::Markdown);
    assert!(h.texts(w).iter().any(|t| t == "Ln 48, Col 10 · UTF-8 · LF"), "{:?}", h.texts(w));
    assert!(h.texts(w).iter().any(|t| t == "Markdown"));
    assert_eq!(h.selection(w).as_deref(), Some("fills only the pixels whose centres its\ndamage contains"));
    let at = h.find_id(w, "editor").unwrap();
    // The first line showing is 38.
    h.click(w, Point::new(at.x0 + 60.0, at.y0 + 14.0));
    assert_eq!(doc(&h, w).content.cursor_position().0, 37);
    let (a, b) = (char_at(&h, w, 48, 15), char_at(&h, w, 47, 9));
    select(&mut h, w, a, b);
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join("sumi-board-night@2x.png"), 2.0);
    assert_golden(&mut h, w, golden("board-night"), 1.0);
}

#[test]
fn the_boards_editor_in_day() {
    let mut h = sumi_in(ThemeChoice::Day);
    let w = the_board(&mut h);
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join("sumi-board-day@2x.png"), 2.0);
    assert_golden(&mut h, w, golden("board-day"), 1.0);
}

#[test]
fn typing_markdown_and_the_title() {
    let mut h = sumi();
    let w = h.window();
    assert_eq!(h.title(w), "Untitled");
    focus_editor(&mut h, w);
    h.type_text(w, "## Notes");
    h.named(w, NamedKey::Enter, NONE);
    h.type_text(w, "- one");
    h.named(w, NamedKey::Enter, NONE);
    h.type_text(w, "two");
    h.named(w, NamedKey::Enter, NONE);
    h.named(w, NamedKey::Enter, NONE);
    h.type_text(w, "1. first");
    h.named(w, NamedKey::Enter, NONE);
    // Return on an empty item ends the list, on that line.
    assert_eq!(text(&h, w), "## Notes\n- one\n- two\n1. first\n2. ");
    assert_eq!(h.title(w), "Untitled — Edited");
    h.command(w, 'z');
    assert_eq!(text(&h, w), "## Notes\n- one\n- two\n1. first");
}

#[test]
fn format_commands_wrap_and_unwrap() {
    let mut h = sumi();
    let w = h.window();
    focus_editor(&mut h, w);
    h.type_text(w, "make this bold");
    select(&mut h, w, 10, 14);
    h.command(w, 'b');
    assert_eq!(text(&h, w), "make this **bold**");
    assert_eq!(h.selection(w).as_deref(), Some("bold"));
    h.command(w, 'b');
    assert_eq!(text(&h, w), "make this bold");
    h.command(w, 'i');
    assert_eq!(text(&h, w), "make this *bold*");
    h.command(w, 'e');
    assert_eq!(text(&h, w), "make this *`bold`*");
    h.command(w, 'z');
    h.command(w, 'z');
    assert_eq!(text(&h, w), "make this bold");
    // Heading 2 (⌥❖2), Quote (⇧❖', typed as ⇧❖"), and the toolbar's B.
    h.key(w, Key::Character("2".into()), Modifiers::SUPER | Modifiers::ALT);
    assert_eq!(text(&h, w), "## make this bold");
    h.key(w, Key::Character("\"".into()), Modifiers::SUPER | Modifiers::SHIFT);
    assert_eq!(text(&h, w), "> ## make this bold");
    h.key(w, Key::Character("\"".into()), Modifiers::SUPER | Modifiers::SHIFT);
    assert_eq!(text(&h, w), "## make this bold");
    select(&mut h, w, 3, 7);
    assert!(h.find(w, "B").is_some(), "the toolbar's B");
    h.send(Msg::On(w, Command::Inline(Inline::Bold)));
    assert_eq!(text(&h, w), "## **make** this bold");
    // Copy Style from it, Paste Style onto "bold".
    h.key(w, Key::Character("c".into()), Modifiers::SUPER | Modifiers::ALT);
    select(&mut h, w, 17, 21);
    h.key(w, Key::Character("v".into()), Modifiers::SUPER | Modifiers::ALT);
    assert_eq!(text(&h, w), "## **make** this **bold**");
    // Outside Markdown the Format items do nothing.
    h.send(Msg::On(w, Command::SetLanguage(Language::Rust)));
    h.command(w, 'b');
    assert_eq!(text(&h, w), "## **make** this **bold**");
}

#[test]
fn links_ask_for_their_address() {
    let mut h = sumi();
    let w = h.window();
    focus_editor(&mut h, w);
    h.type_text(w, "read the docs");
    select(&mut h, w, 9, 13);
    h.platform.clipboard = Some("https://aizome.org/docs".into());
    h.command(w, 'k');
    assert_eq!(h.program().sheets.len(), 1, "the Link sheet");
    h.send(Msg::LinkDone(true));
    assert_eq!(text(&h, w), "read the [docs](https://aizome.org/docs)");
    assert!(h.program().sheets.is_empty());
}

#[test]
fn find_and_replace() {
    let mut h = sumi();
    let w = h.window();
    focus_editor(&mut h, w);
    h.type_text(w, "Damage is per layer. damage contains damaged tiles");
    h.named(w, NamedKey::ArrowUp, Modifiers::SUPER);
    h.command(w, 'f');
    assert!(doc(&h, w).find.shown);
    assert_eq!(h.focused_name(w).as_deref(), Some("find"));
    h.type_text(w, "damage");
    assert_eq!(doc(&h, w).find.matches.len(), 3);
    assert!(h.texts(w).iter().any(|t| t == "1 of 3"), "{:?}", h.texts(w));
    h.command(w, 'g');
    assert_eq!(doc(&h, w).content.selection().range(), 21..27);
    h.key(w, Key::Character("G".into()), Modifiers::SUPER | Modifiers::SHIFT);
    assert_eq!(doc(&h, w).content.selection().range(), 0..6);
    // Whole words, then case.
    h.send(Msg::On(w, Command::FindWord));
    assert_eq!(doc(&h, w).find.matches.len(), 2);
    h.send(Msg::On(w, Command::FindCase));
    assert_eq!(doc(&h, w).find.matches.len(), 1);
    h.send(Msg::On(w, Command::FindCase));
    h.send(Msg::On(w, Command::FindWord));
    // A regular expression with a group, replaced everywhere at once.
    h.send(Msg::On(w, Command::FindRegex));
    h.send(Msg::On(w, Command::FindQuery(r"(d|D)amage(d?)".into())));
    assert_eq!(doc(&h, w).find.matches.len(), 3);
    h.send(Msg::On(w, Command::FindQuery("(".into())));
    assert!(doc(&h, w).find.error.is_some());
    h.send(Msg::On(w, Command::FindQuery(r"(d|D)amage(d?)".into())));
    h.key(w, Key::Character("f".into()), Modifiers::SUPER | Modifiers::ALT);
    assert!(doc(&h, w).find.replace);
    h.send(Msg::On(w, Command::FindReplacement("${1}ent$2".into())));
    h.send(Msg::On(w, Command::ReplaceAll));
    assert_eq!(text(&h, w), "Dent is per layer. dent contains dentd tiles");
    // Undone in the document (in the find field, Cmd-Z undoes its typing).
    focus_editor(&mut h, w);
    h.command(w, 'z');
    assert_eq!(text(&h, w), "Damage is per layer. damage contains damaged tiles", "one step");
    // Replace one at a time.
    h.send(Msg::On(w, Command::FindQuery("layer".into())));
    h.send(Msg::On(w, Command::FindReplacement("tile".into())));
    h.send(Msg::On(w, Command::ReplaceOne));
    assert_eq!(text(&h, w), "Damage is per tile. damage contains damaged tiles");
    // Escape puts the bar away.
    h.named(w, NamedKey::Escape, NONE);
    assert!(!doc(&h, w).find.shown);
    assert_eq!(h.focused_name(w).as_deref(), Some("editor"));
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join("sumi-find@2x.png"), 2.0);
}

#[test]
fn the_find_bar_looks_finished() {
    let mut h = sumi();
    let w = the_board(&mut h);
    h.send(Msg::On(w, Command::ReplaceShow));
    h.send(Msg::On(w, Command::FindRegex));
    h.send(Msg::On(w, Command::FindQuery(r"pixels?".into())));
    h.send(Msg::On(w, Command::FindReplacement("px".into())));
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join("sumi-find-replace-night@2x.png"), 2.0);
    assert_golden(&mut h, w, golden("find-replace-night"), 1.0);
}

#[test]
fn code_is_coloured() {
    let dir = scratch("code");
    let file = dir.join("tile.rs");
    std::fs::write(
        &file,
        "//! Tiles: a surface cut into squares.\n\nuse std::ops::Range;\n\n/// A tile and whether it needs drawing.\n#[derive(Clone, Debug)]\npub struct Tile<'a> {\n    pub rect: Rect,\n    pub name: &'a str,\n    dirty: bool, /* since the\n    last present */\n}\n\nimpl Tile<'_> {\n    pub const SIZE: u32 = 256;\n\n    pub fn new(x: f64) -> Self {\n        let label = format!(\"tile {x:.1}\");\n        println!(\"{label}\");\n        Tile { rect: Rect::new(x, 0.0, x + 256.0, 256.0), name: \"t\", dirty: true }\n    }\n}\n",
    )
    .unwrap();
    let mut h = sumi();
    let w = h.window();
    h.send(Msg::Opened(Some(vec![file])));
    assert_eq!(doc(&h, w).language, Language::Rust);
    assert!(h.texts(w).iter().any(|t| t == "Rust"));
    focus_editor(&mut h, w);
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join("sumi-code-night@2x.png"), 2.0);
    assert_golden(&mut h, w, golden("code-night"), 1.0);
}

#[test]
fn encodings_and_line_endings_survive_open_and_save() {
    let dir = scratch("encodings");
    let file = dir.join("latin.txt");
    std::fs::write(&file, b"caf\xe9\r\nna\xefve\r\n").unwrap();
    let mut h = sumi();
    let w = h.window();
    h.send(Msg::Opened(Some(vec![file.clone()])));
    assert_eq!(text(&h, w), "café\nnaïve\n");
    assert_eq!((doc(&h, w).encoding, doc(&h, w).line_ending), (Encoding::Latin1, LineEnding::Crlf));
    assert!(h.texts(w).iter().any(|t| t.ends_with("· Latin-1 · CRLF")), "{:?}", h.texts(w));
    focus_editor(&mut h, w);
    h.type_text(w, "ok");
    h.command(w, 's');
    assert_eq!(std::fs::read(&file).unwrap(), b"caf\xe9\r\nna\xefve\r\nok");
    assert_eq!(h.title(w), "latin.txt");
    // A character Latin-1 hasn't: asked, then saved in UTF-8.
    h.type_text(w, " ✓");
    h.command(w, 's');
    assert_eq!(std::fs::read(&file).unwrap(), b"caf\xe9\r\nna\xefve\r\nok", "not saved yet");
    h.send(Msg::SaveUtf8(w));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "café\r\nnaïve\r\nok ✓");
    // Line endings switched.
    h.send(Msg::On(w, Command::SetLineEnding(LineEnding::Lf)));
    assert_eq!(h.title(w), "latin.txt — Edited");
    h.command(w, 's');
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "café\nnaïve\nok ✓");
    // UTF-16 with its mark.
    h.send(Msg::On(w, Command::SetEncoding(Encoding::Utf16Le)));
    h.command(w, 's');
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..4], &[0xff, 0xfe, b'c', 0]);
    // Changed on disk behind its back: saving asks.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&file, "someone else").unwrap();
    h.type_text(w, "!");
    h.command(w, 's');
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "someone else");
    h.send(Msg::Overwrite(w));
    assert!(std::fs::read(&file).unwrap().len() > 20);
}

#[test]
fn autosave_brings_everything_back_after_a_crash() {
    let dir = scratch("autosave");
    let places = Places {
        state: Some(dir.join("state")),
        recent: Some(dir.join("recently-used.xbel")),
        settings: Some(dir.join("sumi.toml")),
        documents: None,
    };
    let file = dir.join("notes.md");
    std::fs::write(&file, "# Notes\n").unwrap();
    {
        let mut h = Harness::new(app(places.clone()));
        let w = h.window();
        focus_editor(&mut h, w);
        h.type_text(w, "an untitled draft");
        h.send(Msg::Opened(Some(vec![file.clone()])));
        let w2 = *h.program().docs.keys().find(|k| **k != w).expect("a second window");
        h.focus_window(w2, true);
        focus_editor(&mut h, w2);
        h.type_text(w2, "more");
        h.advance(Duration::from_secs(2));
        // The process dies here: nothing saved, nothing closed.
        std::mem::forget(h);
    }
    std::thread::sleep(Duration::from_millis(300));
    let h = Harness::new(app(places.clone()));
    let docs: Vec<_> = h.program().docs.values().collect();
    assert_eq!(docs.len(), 2, "both documents back");
    let untitled = docs.iter().find(|d| d.path.is_none()).expect("the untitled one");
    assert_eq!(untitled.content.text(), "an untitled draft");
    assert!(untitled.dirty());
    let notes = docs.iter().find(|d| d.path.is_some()).expect("notes.md");
    assert_eq!(notes.content.text(), "# Notes\nmore");
    assert!(notes.dirty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "# Notes\n", "the file itself untouched");
    // Recent files went to the shared list.
    let xbel = std::fs::read_to_string(dir.join("recently-used.xbel")).unwrap();
    assert!(xbel.contains("notes.md") && xbel.contains("name=\"Sumi\""));
    assert_eq!(h.program().recent.first(), Some(&file));
}

#[test]
fn quit_keeps_unsaved_work_and_close_asks() {
    let dir = scratch("quit");
    let places = Places { state: Some(dir.join("state")), recent: None, settings: None, documents: None };
    let mut h = Harness::new(app(places.clone()));
    let w = h.window();
    focus_editor(&mut h, w);
    h.type_text(w, "keep me");
    h.command(w, 'q');
    assert!(h.platform.quit);
    drop(h);
    let mut h = Harness::new(app(places.clone()));
    let w = h.window();
    assert_eq!(text(&h, w), "keep me");
    // Closing a dirty window asks; Don't Save forgets it.
    h.focus_window(w, true);
    h.command(w, 'w');
    assert_eq!(h.windows().len(), 1, "still open, asking");
    h.send(Msg::Close(w, aizome_sumi::app::CloseChoice::Discard));
    assert!(h.program().docs.is_empty());
    drop(h);
    let h = Harness::new(app(places));
    let w = h.window();
    assert_eq!(text(&h, w), "", "a fresh Untitled");
}

#[test]
fn one_instance_takes_the_seconds_files() {
    let dir = scratch("instance");
    let sock = dir.join("sumi.sock");
    let file = dir.join("handed.md");
    std::fs::write(&file, "handed over\n").unwrap();
    let aizome_sumi::instance::Claim::First(listener) = aizome_sumi::instance::claim_at(sock.clone(), &[]) else {
        panic!("first")
    };
    let mut a = app(Places::default());
    a.instance = Some(listener);
    let mut h = Harness::new(a);
    assert!(matches!(
        aizome_sumi::instance::claim_at(sock, &[file.display().to_string()]),
        aizome_sumi::instance::Claim::Handed
    ));
    settle(&mut h, |h| h.program().docs.values().any(|d| d.path.as_deref() == Some(file.as_path())));
}

#[test]
fn menus_are_the_boards() {
    let h = sumi();
    let bar = aizome_kit::Program::menus(h.program());
    let titles: Vec<&str> = bar.menus().iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Sumi", "File", "Edit", "Format", "View", "Window", "Help"]);
    let format = &bar.menus()[3];
    let items: Vec<String> = format
        .items
        .iter()
        .map(|i| match i {
            aizome_kit::menu::ItemSpec::Action(a) => {
                format!("{}{}", a.label, a.shortcut.map(|s| format!(" {}", s.label())).unwrap_or_default())
            }
            aizome_kit::menu::ItemSpec::Submenu(m) => format!("{} ▸", m.title),
            aizome_kit::menu::ItemSpec::Separator => "—".into(),
            aizome_kit::menu::ItemSpec::Header(t) => t.clone(),
        })
        .collect();
    assert_eq!(
        items,
        [
            "Font ▸",
            "Paragraph ▸",
            "—",
            "Bold ❖B",
            "Italic ❖I",
            "Code ❖E",
            "Link… ❖K",
            "—",
            "Heading ▸",
            "Quote ⇧❖'",
            "—",
            "Copy Style ⌥❖C",
            "Paste Style ⌥❖V"
        ]
    );
}

#[test]
fn a_large_file_opens_quickly_and_stays_quick() {
    let dir = scratch("large");
    let file = dir.join("big.rs");
    let mut s = String::with_capacity(10 << 20);
    let mut i = 0;
    while s.len() < 10 << 20 {
        s.push_str(&format!("    let value_{i} = compute(\"tile {i}\", {i}.5); // line {i}\n"));
        i += 1;
    }
    std::fs::write(&file, &s).unwrap();
    let lines = i;
    let mut h = sumi();
    let w = h.window();
    let started = Instant::now();
    h.send(Msg::Opened(Some(vec![file.clone()])));
    settle(&mut h, |h| h.program().docs.values().next().is_some_and(|d| d.loading.is_none() && d.path.is_some()));
    let opened = started.elapsed();
    assert_eq!(doc(&h, w).content.line_count(), lines + 1);
    focus_editor(&mut h, w);
    // To the end and type there.
    h.key(w, Key::Named(NamedKey::ArrowDown), Modifiers::SUPER);
    let started = Instant::now();
    h.type_text(w, "x");
    let first_key = started.elapsed();
    let mut frames = Vec::new();
    for c in "fn main() { let a = 1; }".chars() {
        h.take_damage(w);
        let t = Instant::now();
        h.type_text(w, &c.to_string());
        let damage = h.take_damage(w);
        if let Some(b) = damage.into_iter().reduce(|a, b| a.union(b)) {
            let _ = h.render_rect(w, b, 2.0);
        }
        frames.push(t.elapsed());
    }
    frames.sort();
    eprintln!(
        "sumi: 10 MB ({lines} lines) opened in {opened:?}; first key at the end {first_key:?}; typing p50 {:?} max {:?}",
        frames[frames.len() / 2],
        frames.last().unwrap()
    );
    assert!(text(&h, w).ends_with("xfn main() { let a = 1; }"));

    // With the real budget: a jump to the end costs a frame's worth of
    // colouring, the rest coming in the frames after (guessed meanwhile).
    let mut a = Sumi::new(Places::default());
    a.highlight_budget = Duration::from_millis(3);
    let mut h = Harness::new(a);
    let w = h.window();
    h.send(Msg::Opened(Some(vec![file])));
    settle(&mut h, |h| h.program().docs.values().next().is_some_and(|d| d.loading.is_none() && d.path.is_some()));
    focus_editor(&mut h, w);
    let t = Instant::now();
    h.key(w, Key::Named(NamedKey::ArrowDown), Modifiers::SUPER);
    let jump = t.elapsed();
    let valid = |h: &Harness<Sumi>| doc(h, w).highlight.lock().unwrap().valid();
    let t = Instant::now();
    let mut passes = 0;
    while valid(&h) + 30 < lines {
        // What the highlighter asked for, then the pass it schedules.
        h.send(Msg::Nothing);
        h.advance(Duration::from_millis(5));
        passes += 1;
        assert!(passes < 100_000);
    }
    eprintln!(
        "sumi: jump to the end of 10 MB in {jump:?}; its colours exact after {passes} more frames ({:?})",
        t.elapsed()
    );
}

/// Typing and scrolling in the board's document, measured as Soroban's
/// key presses are: update, view, layout, diff and rasterising the damage
/// at 2×.
#[test]
fn typing_and_scrolling_fit_the_frame_budget() {
    let mut h = sumi();
    let w = the_board(&mut h);
    let at = char_at(&h, w, 50, 10);
    select(&mut h, w, at, at);
    let _ = h.render(w, 2.0);
    let mut typing = Vec::new();
    for round in 0..6 {
        for c in "a few words typed into the paragraph ".chars() {
            h.take_damage(w);
            let t = Instant::now();
            h.type_text(w, &c.to_string());
            let damage = h.take_damage(w);
            if let Some(b) = damage.into_iter().reduce(|a, b| a.union(b)) {
                let _ = h.render_rect(w, b, 2.0);
            }
            if round > 0 {
                typing.push(t.elapsed());
            }
        }
        h.advance(Duration::from_secs(2));
    }
    let e = h.find_id(w, "editor").unwrap();
    let mut scrolling = Vec::new();
    for i in 0..60 {
        h.take_damage(w);
        let t = Instant::now();
        let dy = if (i / 15) % 2 == 0 { 30.0 } else { -30.0 };
        h.scroll(w, Point::new(e.x0 + 200.0, e.y0 + 200.0), Vec2::new(0.0, dy));
        let damage = h.take_damage(w);
        if let Some(b) = damage.into_iter().reduce(|a, b| a.union(b)) {
            let _ = h.render_rect(w, b, 2.0);
        }
        scrolling.push(t.elapsed());
    }
    let stats = |v: &mut Vec<Duration>| {
        v.sort();
        let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
        (p(0.5), p(0.95), *v.last().unwrap())
    };
    let (t50, t95, tmax) = stats(&mut typing);
    let (s50, s95, smax) = stats(&mut scrolling);
    eprintln!(
        "sumi typing: p50 {t50:?} p95 {t95:?} max {tmax:?} (n={}); scrolling: p50 {s50:?} p95 {s95:?} max {smax:?} (n={}) at 2x",
        typing.len(),
        scrolling.len()
    );
    if !cfg!(debug_assertions) {
        assert!(t95 < Duration::from_millis(4), "typing p95 {t95:?}");
    }
}

#[test]
fn markdown_is_styled() {
    for (theme, name) in [(ThemeChoice::Night, "markdown-night"), (ThemeChoice::Day, "markdown-day")] {
        let mut h = sumi_in(theme);
        let w = h.window();
        h.send(Msg::Opened(Some(vec![fixture("styles.md")])));
        focus_editor(&mut h, w);
        let at = char_at(&h, w, 7, 17);
        select(&mut h, w, at, at);
        h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sumi-{name}@2x.png")), 2.0);
        assert_golden(&mut h, w, golden(name), 1.0);
    }
}

#[test]
fn save_and_open_through_the_panels() {
    let dir = scratch("panels");
    let places = Places { documents: Some(dir.clone()), ..Places::default() };
    let mut h = Harness::new(app(places));
    let w = h.window();
    focus_editor(&mut h, w);
    h.type_text(w, "# A draft");
    // ❖S on an untitled document asks where, in the documents folder.
    h.command(w, 's');
    h.command(w, 'a');
    h.type_text(w, "draft.md");
    h.named(w, NamedKey::Enter, NONE);
    assert_eq!(std::fs::read_to_string(dir.join("draft.md")).unwrap(), "# A draft");
    assert_eq!(h.title(w), "draft.md");
    assert_eq!(doc(&h, w).language, Language::Markdown);
    // ❖O lists the folder; choosing a file opens it in a window of its own.
    std::fs::write(dir.join("other.txt"), "other text\n").unwrap();
    h.command(w, 'o');
    assert!(h.texts(w).iter().any(|t| t == "other.txt"), "{:?}", h.texts(w));
    assert!(h.click_text(w, "other.txt"));
    h.named(w, NamedKey::Enter, NONE);
    assert_eq!(h.windows().len(), 2);
    let other = h
        .program()
        .docs
        .values()
        .find(|d| d.path.as_deref() == Some(dir.join("other.txt").as_path()))
        .expect("other.txt open");
    assert_eq!(other.content.text(), "other text\n");
    assert_eq!(other.language, Language::Plain);
}

#[test]
fn command_click_opens_links() {
    let mut h = sumi();
    let w = h.window();
    focus_editor(&mut h, w);
    h.type_text(w, "see [the docs](https://aizome.org/docs) now");
    let e = h.find_id(w, "editor").unwrap();
    let on_docs = Point::new(e.x0 + 44.0 + 9.0 * 7.8, e.y0 + 12.0 + 10.0);
    h.click(w, on_docs);
    assert!(h.program().links.is_empty(), "a plain click places the caret");
    // With the command key held (a key with it leaves it held for the click).
    h.key(w, Key::Named(NamedKey::Insert), Modifiers::SUPER);
    h.press(w, on_docs, aizome_kit::sidestep_ui::PointerButton::Left, 1);
    h.release(w, on_docs, aizome_kit::sidestep_ui::PointerButton::Left);
    assert_eq!(h.program().links, ["https://aizome.org/docs"]);
}
