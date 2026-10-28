//! Shiori run headlessly over generated documents, with real PDFium
//! workers: opening, drawing exactly, moving about, zooming, finding,
//! selecting and copying, links, passwords, a worker crashing, your
//! place kept and a file changing.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_kit::Appearance;
use aizome_kit::prelude::*;
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::ThemeChoice;
use aizome_shiori::app::{Load, Msg, Shiori};
use aizome_shiori::layout::Zoom;
use aizome_shiori::places::Places;

struct Rig {
    h: Harness<Shiori>,
    dir: PathBuf,
}

fn app(dir: &Path, open: &[&str]) -> Shiori {
    let mut app = Shiori::new();
    app.worker = common::worker();
    app.places = Places::load(dir.join("places.json"));
    app.dry_run = true;
    app.initial = open.iter().map(|n| dir.join(n)).collect();
    app
}

/// Shiori over `files` in a fresh folder, `open` opened, settled.
fn rig(name: &str, files: &[(&str, Vec<u8>)], open: &[&str]) -> Rig {
    let dir = common::scratch(name);
    for (n, bytes) in files {
        common::write(&dir, n, bytes);
    }
    rig_in(dir, ThemeChoice::Night, open)
}

fn rig_in(dir: PathBuf, theme: ThemeChoice, open: &[&str]) -> Rig {
    let h = Harness::with(app(&dir, open), Appearance { theme, ..Appearance::default() }, false);
    let mut r = Rig { h, dir };
    settle(&mut r);
    r
}

/// Draw, take in what the workers sent, let timers run, and again, until
/// nothing is being opened or drawn.
fn settle(r: &mut Rig) {
    let mut quiet = 0;
    for _ in 0..3000 {
        for w in r.h.windows() {
            let _ = r.h.render(w, 1.0);
        }
        r.h.send(Msg::Nothing);
        r.h.advance(Duration::from_millis(20));
        if r.h.program().idle() {
            quiet += 1;
            if quiet >= 4 {
                return;
            }
        } else {
            quiet = 0;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    panic!("Shiori didn't settle");
}

fn doc(r: &Rig) -> &aizome_shiori::app::Doc {
    r.h.program().docs.first().expect("a document")
}

/// The point of the view over `at` on page `page` (page points).
fn view_point(r: &Rig, page: usize, at: (f64, f64)) -> Point {
    let d = doc(r);
    let p = d.layout.to_doc(page, at);
    // The toolbar is above the pages, the sidebar beside them.
    let origin = r.h.find_id(r.h.window(), "pages").expect("the pages").origin();
    Point::new(p.x - d.scroll.0 + origin.x, p.y - d.scroll.1 + origin.y)
}

/// Where on page `page` the text `needle` is (the middle of its first
/// character's box, and of its last's).
fn text_span(r: &Rig, page: u32, needle: &str) -> ((f64, f64), (f64, f64)) {
    let t = doc(r).texts.get(&page).expect("the page's text");
    let s: String = t.chars.iter().collect();
    let start = s.chars().collect::<Vec<_>>();
    let n: Vec<char> = needle.chars().collect();
    let i = (0..start.len()).find(|i| start[*i..].starts_with(&n)).expect("the text is there");
    let mid = |b: [f32; 4]| ((f64::from(b[0]) + f64::from(b[2])) / 2.0, (f64::from(b[1]) + f64::from(b[3])) / 2.0);
    (mid(t.boxes[i]), mid(t.boxes[i + n.len() - 1]))
}

#[test]
fn opens_and_draws_pages_exactly() {
    let mut r = rig("open", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    let d = doc(&r);
    assert!(matches!(d.load, Load::Open(_)));
    assert_eq!(d.pages(), 4);
    assert_eq!(r.h.title(w), "sample.pdf");
    // Every tile of what shows is a drawing for this zoom and scale.
    let view = Rect::new(d.scroll.0, d.scroll.1, d.scroll.0 + d.view.width, d.scroll.1 + d.view.height);
    for i in d.layout.visible(view.y0, view.y1) {
        for k in d.layout.tiles(i, d.scale, view, view.center()) {
            assert!(d.tiles.has(&k), "{k:?} wasn't drawn");
        }
    }
    assert!(r.h.texts(w).iter().any(|t| t.starts_with("Page 1 of 4")), "{:?}", r.h.texts(w));
    // The first line's ink is where its text is.
    let (a, b) = text_span(&r, 0, "Shiori");
    let (pa, pb) = (view_point(&r, 0, a), view_point(&r, 0, b));
    let img = r.h.render(w, 1.0);
    let (iw, _) = img.pixel_size();
    let px = img.to_rgba8().unwrap();
    let dark = |p: Point| {
        let mut n = 0;
        for y in (p.y as i64 - 6)..(p.y as i64 + 6) {
            for x in (p.x as i64 - 6)..(p.x as i64 + 6) {
                let o = (y as usize * iw as usize + x as usize) * 4;
                n += usize::from(px[o] < 100 && px[o + 1] < 100 && px[o + 2] < 100);
            }
        }
        n
    };
    assert!(dark(pa) > 4 && dark(pb) > 4, "no ink at {pa:?} or {pb:?}");
}

#[test]
fn moves_and_zooms_keeping_the_place() {
    let mut r = rig("move", &[("long.pdf", common::long(40))], &["long.pdf"]);
    let w = r.h.window();
    r.h.send(Msg::GoPage(w, 19));
    settle(&mut r);
    assert_eq!(doc(&r).current_page(), 19);
    assert!(r.h.texts(w).iter().any(|t| t.starts_with("Page 20 of 40")));
    // Zooming in keeps page 20 in the middle of the view.
    r.h.send(Msg::ZoomIn);
    settle(&mut r);
    let d = doc(&r);
    assert!(matches!(d.zoom, Zoom::Fixed(_)));
    let middle = d.layout.page_at(d.scroll.1 + d.view.height / 2.0);
    assert_eq!(middle, 19);
    // Page Down moves a view's height less a line.
    let before = doc(&r).scroll.1;
    r.h.send(Msg::Nav(w, aizome_shiori::app::Nav::PageDown));
    let d = doc(&r);
    assert!((d.scroll.1 - before - (d.view.height - aizome_kit::LINE)).abs() < 1.0);
    // A pinch zooms about the pointer, and tiles wait until it rests.
    let at = Point::new(200.0, 300.0);
    r.h.send(Msg::ZoomBy { window: w, factor: 1.5, at });
    assert!(doc(&r).settling);
    settle(&mut r);
    assert!(!doc(&r).settling);
    r.h.send(Msg::LastPage);
    settle(&mut r);
    assert_eq!(doc(&r).current_page(), 39);
}

#[test]
fn finds_and_steps_through_hits() {
    let mut r = rig("find", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    r.h.command(w, 'f');
    assert_eq!(r.h.focused_name(w).as_deref(), Some("find"));
    r.h.type_text(w, "needle");
    r.h.advance(Duration::from_millis(250));
    settle(&mut r);
    let f = doc(&r).find.clone().expect("a search");
    assert_eq!(f.count(), 4);
    assert_eq!(f.current, Some((0, 0)));
    assert!(r.h.texts(w).iter().any(|t| t == "1 of 4"), "{:?}", r.h.texts(w));
    r.h.send(Msg::FindNext);
    r.h.send(Msg::FindNext);
    assert_eq!(doc(&r).find.as_ref().unwrap().current, Some((1, 1)));
    r.h.send(Msg::FindNext);
    settle(&mut r);
    // The hit on the turned page: it's in view.
    assert_eq!(doc(&r).find.as_ref().unwrap().current, Some((3, 0)));
    assert_eq!(doc(&r).current_page(), 3);
    r.h.send(Msg::FindNext);
    assert_eq!(doc(&r).find.as_ref().unwrap().current, Some((0, 0)));
    // Matching case leaves one "Needle".
    r.h.send(Msg::FindCase(w, true));
    r.h.send(Msg::FindQuery(w, "Needle".into()));
    r.h.advance(Duration::from_millis(250));
    settle(&mut r);
    assert_eq!(doc(&r).find.as_ref().unwrap().count(), 1);
    r.h.send(Msg::FindClose(w));
    assert!(doc(&r).find.is_none());
}

#[test]
fn selects_and_copies_text() {
    let mut r = rig("select", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    let (a, b) = text_span(&r, 0, "Shiori reads documents.");
    let from = view_point(&r, 0, (a.0 - 3.0, a.1));
    let to = view_point(&r, 0, (b.0 + 3.0, b.1));
    r.h.drag(w, from, to, 6);
    settle(&mut r);
    r.h.command(w, 'c');
    assert_eq!(r.h.platform.clipboard.as_deref(), Some("Shiori reads documents."));
    // A double click takes a word.
    let (m, _) = text_span(&r, 0, "needle");
    r.h.press(w, view_point(&r, 0, m), aizome_kit::sidestep_ui::PointerButton::Left, 2);
    r.h.release(w, view_point(&r, 0, m), aizome_kit::sidestep_ui::PointerButton::Left);
    r.h.command(w, 'c');
    assert_eq!(r.h.platform.clipboard.as_deref(), Some("needle"));
    // A click on nothing clears it.
    let gutter = r.h.find_id(w, "pages").unwrap().origin() + Vec2::new(4.0, 300.0);
    r.h.click(w, gutter);
    assert!(doc(&r).selection.is_none());
}

#[test]
fn follows_links() {
    let mut r = rig("links", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    let (a, _) = text_span(&r, 0, "Go to page three");
    let at = view_point(&r, 0, a);
    r.h.move_to(w, at);
    assert_eq!(doc(&r).hover, aizome_shiori::app::Hover::Link);
    r.h.click(w, at);
    settle(&mut r);
    assert_eq!(doc(&r).current_page(), 2);
    r.h.send(Msg::FirstPage);
    settle(&mut r);
    let (u, _) = text_span(&r, 0, "Visit");
    r.h.click(w, view_point(&r, 0, u));
    assert_eq!(r.h.program().opened, vec!["https://sekka.computer/".to_owned()]);
}

#[test]
fn shows_the_outline_and_pages_in_the_sidebar() {
    let mut r = rig("sidebar", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    // A document with an outline opens with it showing.
    assert!(doc(&r).sidebar);
    assert!(r.h.texts(w).iter().any(|t| t == "Introduction"));
    let wide = r.h.find(w, "Wide").expect("the outline's entry");
    r.h.click(w, wide.center());
    settle(&mut r);
    assert_eq!(doc(&r).current_page(), 2);
    // The pages tab draws each page small.
    r.h.send(Msg::SidebarTab(w, 0));
    settle(&mut r);
    let d = doc(&r);
    let pw = (aizome_shiori::app::PREVIEW * d.scale).round() as u32;
    for i in 0..d.pages() {
        assert!(d.tiles.has(&d.layout.preview(i, pw)), "page {} has no preview", i + 1);
    }
}

#[test]
fn opens_a_locked_document() {
    let dir = common::scratch("locked");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/locked.pdf"), dir.join("locked.pdf"))
        .unwrap();
    let mut r = rig_in(dir, ThemeChoice::Night, &["locked.pdf"]);
    let w = r.h.window();
    assert!(matches!(doc(&r).load, Load::Locked { tried: false }));
    assert_eq!(r.h.title(w), "locked.pdf (Locked)");
    let sheet = *r.h.program().sheets.keys().next().expect("the password sheet");
    r.h.type_text(sheet, "wrong\n");
    settle(&mut r);
    assert!(matches!(doc(&r).load, Load::Locked { tried: true }));
    let sheet = *r.h.program().sheets.keys().next().expect("asked again");
    r.h.type_text(sheet, "kagi\n");
    settle(&mut r);
    assert!(matches!(doc(&r).load, Load::Open(_)), "{:?}", doc(&r).load);
    assert_eq!(doc(&r).pages(), 4);
    assert_eq!(r.h.title(w), "locked.pdf");
}

#[test]
fn carries_on_when_a_worker_dies() {
    let mut r = rig("crash", &[("long.pdf", common::long(12))], &["long.pdf"]);
    let w = r.h.window();
    let pid = doc(&r).engine.as_ref().and_then(|e| e.pid()).expect("a worker");
    // SAFETY: a signal to our own child.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    std::thread::sleep(Duration::from_millis(100));
    r.h.send(Msg::GoPage(w, 8));
    settle(&mut r);
    let d = doc(&r);
    assert!(matches!(d.load, Load::Open(_)));
    assert!(d.trouble.is_some());
    assert!(d.broken.is_empty(), "no page was being drawn");
    assert_eq!(d.current_page(), 8);
    let view = Rect::new(d.scroll.0, d.scroll.1, d.scroll.0 + d.view.width, d.scroll.1 + d.view.height);
    assert!(d.layout.tiles(8, d.scale, view, view.center()).iter().all(|k| d.tiles.has(k)));
    // The notice gives way to the page number again.
    r.h.advance(Duration::from_secs(12));
    assert!(doc(&r).trouble.is_none());
    assert!(r.h.texts(w).iter().any(|t| t.starts_with("Page 9 of 12")), "{:?}", r.h.texts(w));
}

#[test]
fn keeps_your_place() {
    let mut r = rig("place", &[("long.pdf", common::long(30))], &["long.pdf"]);
    let w = r.h.window();
    r.h.send(Msg::GoPage(w, 11));
    r.h.send(Msg::ZoomIn);
    settle(&mut r);
    let zoom = doc(&r).zoom;
    r.h.request_close(w);
    let dir = r.dir.clone();
    drop(r);
    let r = rig_in(dir, ThemeChoice::Night, &["long.pdf"]);
    assert_eq!(doc(&r).zoom, zoom);
    assert_eq!(doc(&r).layout.page_at(doc(&r).scroll.1 + 20.0), 11);
}

#[test]
fn reloads_a_file_that_changes() {
    let mut r = rig("reload", &[("doc.pdf", common::sample())], &["doc.pdf"]);
    assert_eq!(doc(&r).pages(), 4);
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(r.dir.join("doc.pdf"), common::long(6)).unwrap();
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    assert_eq!(doc(&r).pages(), 6);
    // Something that isn't a PDF (yet) leaves the last one showing.
    std::fs::write(r.dir.join("doc.pdf"), b"%PDF-1.7 half written").unwrap();
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    assert!(matches!(doc(&r).load, Load::Open(_)));
    assert_eq!(doc(&r).pages(), 6);
    assert!(doc(&r).trouble.as_deref().is_some_and(|t| t.contains("can't be read yet")));
    // Written whole at last: read again.
    std::fs::write(r.dir.join("doc.pdf"), common::long(3)).unwrap();
    r.h.advance(Duration::from_millis(1600));
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    assert_eq!(doc(&r).pages(), 3);
    assert!(doc(&r).trouble.is_none());
}

#[test]
fn says_what_it_cant_open() {
    let r = rig("bad", &[("notes.pdf", b"not a PDF".to_vec())], &["notes.pdf"]);
    let w = r.h.window();
    assert!(matches!(doc(&r).load, Load::Failed(_)));
    assert!(r.h.texts(w).iter().any(|t| t == "Shiori can't open “notes.pdf”"), "{:?}", r.h.texts(w));
}

#[test]
fn looks() {
    for (theme, name) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let dir = common::scratch(&format!("look-{name}"));
        common::write(&dir, "sample.pdf", &common::sample());
        let mut r = rig_in(dir, theme, &["sample.pdf"]);
        let w = r.h.window();
        r.h.send(Msg::Find);
        r.h.send(Msg::FindQuery(w, "needle".into()));
        r.h.advance(Duration::from_millis(250));
        settle(&mut r);
        assert_golden(&mut r.h, w, format!("tests/golden/{name}.png"), 1.0);
    }
    let mut r = rig("look-empty", &[], &[]);
    let w = r.h.window();
    assert_golden(&mut r.h, w, "tests/golden/empty.png", 1.0);
}

#[test]
fn the_pages_either_side_are_drawn_ahead() {
    let mut r = rig("ahead", &[("long.pdf", common::long(20))], &["long.pdf"]);
    let w = r.h.window();
    r.h.send(Msg::GoPage(w, 10));
    settle(&mut r);
    let d = doc(&r);
    let around = d.neighbours();
    assert!(around.start < 10 && around.end > 11, "{around:?}");
    // The page above and the page below are drawn whole at this zoom,
    // with their text.
    for i in around.clone() {
        let whole = d.layout.page_rect(i);
        for k in d.layout.tiles(i, d.scale, whole, whole.center()) {
            assert!(d.tiles.has(&k), "page {} isn't drawn ahead: {k:?}", i + 1);
        }
        assert!(d.texts.contains_key(&(i as u32)), "page {} has no text", i + 1);
    }
    // Scrolling a little either way, and back, asks the worker for
    // nothing: what comes into view is there.
    let at = Point::new(400.0, 300.0);
    for dy in [120.0, 120.0, -360.0, 120.0] {
        r.h.scroll(w, at, Vec2::new(0.0, dy));
        assert!(doc(&r).sent.is_empty(), "scrolling {dy} asked for {:?}", doc(&r).sent);
        assert!(doc(&r).asked.is_empty());
    }
}

#[test]
fn the_sidebar_draws_rows_ahead_and_no_more() {
    let mut r = rig("sidebar-ahead", &[("long.pdf", common::long(60))], &["long.pdf"]);
    let w = r.h.window();
    // No outline: the sidebar opens on the pages when asked for.
    assert!(!doc(&r).sidebar);
    r.h.send(Msg::ToggleSidebar);
    settle(&mut r);
    let d = doc(&r);
    let pw = (aizome_shiori::app::PREVIEW * d.scale).round() as u32;
    let has = |i: usize| d.tiles.has(&d.layout.preview(i, pw));
    // The rows that show (four or five, 176 points each) and three past
    // them are drawn; the rest of the document isn't.
    let shown = (0..d.pages()).take_while(|i| r.h.texts(w).contains(&format!("{}", i + 1))).count();
    assert!((4..=6).contains(&shown), "{shown} rows show");
    for i in 0..shown + aizome_shiori::app::THUMBS_AHEAD {
        assert!(has(i), "row {} has no preview", i + 1);
    }
    assert!(!has(40), "page 41 was drawn though nothing near it showed");
    assert!(d.thumbs.borrow().asked.is_empty());
}

#[test]
fn says_when_a_document_has_no_pages() {
    // A catalog whose page tree is empty.
    let r = rig("no-pages", &[("empty.pdf", common::pdf(&[], &[]))], &["empty.pdf"]);
    let w = r.h.window();
    assert!(matches!(&doc(&r).load, Load::Failed(why) if why.contains("no pages")), "{:?}", doc(&r).load);
    assert!(r.h.texts(w).iter().any(|t| t == "Shiori can't open “empty.pdf”"));
}
