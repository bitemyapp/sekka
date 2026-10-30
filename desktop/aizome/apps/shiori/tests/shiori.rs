//! Shiori run headlessly over generated documents, with real hayro
//! workers: opening, drawing exactly, moving about, zooming, finding,
//! selecting and copying, links, passwords, a worker crashing, your
//! place kept and a file changing.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::ThemeChoice;
use aizome_kit::{Appearance, WindowState};
use aizome_shiori::app::{Load, Msg, Sheet, Shiori};
use aizome_shiori::engine::{Event, Failure};
use aizome_shiori::layout::Zoom;
use aizome_shiori::places::Places;
use aizome_shiori::protocol::{Job, Reply, TileKey};
use aizome_shiori::text::{Caret, Selection};

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
fn a_zoom_shows_at_once_and_sharpens() {
    use aizome_shiori::pages::{Drawing, pages_of};
    let mut r = rig("standin", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let shown = |r: &Rig| {
        let p = r.h.program();
        pages_of(p, &p.docs[0]).shown.iter().flat_map(|pd| pd.tiles.clone()).collect::<Vec<_>>()
    };
    assert!(shown(&r).iter().all(|(_, d)| matches!(d, Drawing::Exact(_))));
    // At once, before the worker's drawn anything at the new zoom, the
    // last zoom's drawings stand in: no paper where the page was.
    r.h.send(Msg::ZoomIn);
    let standing = shown(&r);
    assert!(!standing.is_empty());
    assert!(standing.iter().all(|(_, d)| !matches!(d, Drawing::Blank)), "paper stood in");
    settle(&mut r);
    let sharp = shown(&r);
    assert!(sharp.iter().all(|(_, d)| matches!(d, Drawing::Exact(_))));
    assert!(doc(&r).standins.is_empty(), "stand-ins kept once drawn");
    // And what stood in has its ink where the drawings have theirs: the
    // box around the dark pixels of the part made, in the page's pixels.
    let ink = |img: &Image, at: (u32, u32), part: &TileKey| {
        let (w, _) = img.pixel_size();
        let px = img.premultiplied_pixels().unwrap();
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in part.at.1..part.at.1 + part.h {
            for x in part.at.0..part.at.0 + part.w {
                let o = (((y - at.1) * w + (x - at.0)) * 4) as usize;
                if px[o] < 128 {
                    b = Some(b.map_or((x, y, x, y), |(x0, y0, x1, y1)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y))));
                }
            }
        }
        b
    };
    let mut compared = 0;
    for (k, d) in &standing {
        let Drawing::StandIn { part, image } = d else { continue };
        let Some((_, Drawing::Exact(exact))) = sharp.iter().find(|(s, _)| s == k) else { continue };
        let (Some(a), Some(b)) = (ink(image, part.at, part), ink(exact, k.at, part)) else { continue };
        let near = |p: u32, q: u32| p.abs_diff(q) <= 3;
        assert!(near(a.0, b.0) && near(a.1, b.1) && near(a.2, b.2) && near(a.3, b.3), "{k:?}: {a:?} {b:?}");
        compared += 1;
    }
    assert!(compared > 0, "no ink to compare");
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
    assert!(matches!(doc(&r).hover, aizome_shiori::app::Hover::Link { page: 0, .. }));
    // The toolbar says where it goes.
    assert!(r.h.texts(w).iter().any(|t| t == "Go to page 3"), "{:?}", r.h.texts(w));
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
fn the_last_version_is_still_drawn_while_the_file_cant_be_read() {
    let mut r = rig("unreadable", &[("doc.pdf", common::sample())], &["doc.pdf"]);
    let w = r.h.window();
    let pid = doc(&r).engine.as_ref().and_then(|e| e.pid());
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(r.dir.join("doc.pdf"), b"%PDF-1.7 half written").unwrap();
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    assert!(doc(&r).trouble.as_deref().is_some_and(|t| t.contains("can't be read yet")));
    // The worker that read it is the one still drawing it.
    assert_eq!(doc(&r).engine.as_ref().and_then(|e| e.pid()), pid);
    assert!(doc(&r).reload.is_none());
    // A zoom is drawn sharp, and find finds, in the last version.
    r.h.send(Msg::ZoomIn);
    settle(&mut r);
    let d = doc(&r);
    assert!(matches!(d.zoom, Zoom::Fixed(_)));
    let view = Rect::new(d.scroll.0, d.scroll.1, d.scroll.0 + d.view.width, d.scroll.1 + d.view.height);
    for i in d.layout.visible(view.y0, view.y1) {
        for k in d.layout.tiles(i, d.scale, view, view.center()) {
            assert!(d.tiles.has(&k), "{k:?} wasn't drawn");
        }
    }
    r.h.send(Msg::Find);
    r.h.send(Msg::FindQuery(w, "needle".into()));
    r.h.advance(Duration::from_millis(250));
    settle(&mut r);
    assert_eq!(doc(&r).find.as_ref().map(|f| f.count()), Some(4));
    assert_eq!(doc(&r).pages(), 4);
}

/// Kill `r`'s worker, and wait for the one started in its place to go
/// without a document (as it does when it can't open the file).
fn kill_and_lose_the_worker(r: &mut Rig) {
    let pid = doc(r).engine.as_ref().and_then(|e| e.pid()).expect("a worker");
    // SAFETY: a signal to our own child.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    // The clock stands still, so `watch` doesn't look at the file.
    let gone = r.h.settle(Duration::from_secs(30), |p| p.docs[0].engine.is_none());
    assert!(gone, "the worker started again is still there: {:?}", doc(r).trouble);
}

/// Every tile of the pages in view is drawn at the view's zoom.
fn all_drawn(r: &Rig) {
    let d = doc(r);
    let view = Rect::new(d.scroll.0, d.scroll.1, d.scroll.0 + d.view.width, d.scroll.1 + d.view.height);
    for i in d.layout.visible(view.y0, view.y1) {
        for k in d.layout.tiles(i, d.scale, view, view.center()) {
            assert!(d.tiles.has(&k), "{k:?} wasn't drawn");
        }
    }
}

#[test]
fn a_worker_started_again_on_a_half_written_file_leaves_the_last_version() {
    let mut r = rig("restart-unreadable", &[("doc.pdf", common::sample())], &["doc.pdf"]);
    let drawn = doc(&r).tiles.len();
    // The file's half written when its worker dies, before Shiori sees it
    // change: the worker started again can't open it.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(r.dir.join("doc.pdf"), b"%PDF-1.7 half written").unwrap();
    kill_and_lose_the_worker(&mut r);
    settle(&mut r);
    let d = doc(&r);
    assert!(matches!(d.load, Load::Open(_)));
    assert_eq!(d.pages(), 4);
    assert_eq!(d.tiles.len(), drawn, "what showed went");
    assert_eq!(d.trouble.as_deref(), Some("The file changed and can't be read yet; this is how it was."));
    // Seen to change, it's tried again (and can't be read yet), and then
    // written whole, it's read.
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    assert!(doc(&r).trouble.as_deref().is_some_and(|t| t.contains("can't be read yet")));
    std::fs::write(r.dir.join("doc.pdf"), common::long(3)).unwrap();
    r.h.advance(Duration::from_millis(1600));
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    assert_eq!(doc(&r).pages(), 3);
    assert!(doc(&r).engine.is_some());
    assert!(doc(&r).trouble.is_none(), "{:?}", doc(&r).trouble);
    all_drawn(&r);
}

#[test]
fn a_worker_started_again_that_finds_no_pages_goes() {
    let mut r = rig("restart-no-pages", &[("doc.pdf", common::sample())], &["doc.pdf"]);
    // A file with no pages, as one being written again can be: the worker
    // started again finds none, and would fail every drawing.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(r.dir.join("doc.pdf"), common::pdf(&[], &[])).unwrap();
    kill_and_lose_the_worker(&mut r);
    settle(&mut r);
    let d = doc(&r);
    assert!(matches!(d.load, Load::Open(_)));
    assert_eq!(d.pages(), 4);
    assert!(d.broken.is_empty(), "pages were taken for broken: {:?}", d.broken);
    assert!(d.trouble.as_deref().is_some_and(|t| t.contains("can't be read yet")), "{:?}", d.trouble);
    // When it has pages again, they show.
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    std::fs::write(r.dir.join("doc.pdf"), common::long(3)).unwrap();
    r.h.advance(Duration::from_millis(1600));
    r.h.advance(Duration::from_millis(1600));
    settle(&mut r);
    assert_eq!(doc(&r).pages(), 3);
    assert!(doc(&r).trouble.is_none(), "{:?}", doc(&r).trouble);
    all_drawn(&r);
}

#[test]
fn a_file_that_hasnt_changed_isnt_said_to_have() {
    // SAFETY: no memory is involved.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root reads a file whatever its mode");
        return;
    }
    let mut r = rig("restart-unchanged", &[("doc.pdf", common::sample())], &["doc.pdf"]);
    let w = r.h.window();
    let path = r.dir.join("doc.pdf");
    let mode = |m: u32| std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(m)).unwrap();
    // The file can't be read for a while, as it is (its time and size
    // are), when its worker dies: the worker started again can't open it,
    // and the file isn't blamed.
    mode(0o000);
    kill_and_lose_the_worker(&mut r);
    mode(0o644);
    settle(&mut r);
    let d = doc(&r);
    assert!(matches!(d.load, Load::Open(_)));
    assert_eq!(d.pages(), 4);
    assert_eq!(d.trouble.as_deref(), Some("The page reader couldn't open the file again; Reload (❖R) tries again."));
    // Nothing tries again by itself, as nothing changed.
    r.h.advance(Duration::from_millis(3200));
    settle(&mut r);
    assert!(doc(&r).engine.is_none());
    // Reload does, and what shows is drawn again.
    r.h.send(Msg::Reload);
    settle(&mut r);
    assert!(doc(&r).engine.is_some());
    assert!(doc(&r).trouble.is_none(), "{:?}", doc(&r).trouble);
    r.h.send(Msg::ZoomIn);
    settle(&mut r);
    all_drawn(&r);
    // A worker that takes too long opening the same file says so.
    let serial = doc(&r).serial;
    let late = Event::Failed { job: Some(Job::Open), why: Failure::TooLong, gave_up: true };
    r.h.send(Msg::Worker(serial, late));
    assert!(doc(&r).engine.is_none());
    assert_eq!(
        doc(&r).trouble.as_deref(),
        Some("The page reader took too long opening the file again; Reload (❖R) tries again.")
    );
    let _ = r.h.render(w, 1.0);
    assert!(r.h.texts(w).iter().any(|t| t.starts_with("The page reader took too long")), "{:?}", r.h.texts(w));
}

#[test]
fn a_drawing_of_a_page_that_isnt_there_is_let_go() {
    let mut r = rig("bad-drawing", &[("long.pdf", common::long(3))], &["long.pdf"]);
    let w = r.h.window();
    let serial = doc(&r).serial;
    let image = Image::from_premultiplied_rgba(10, 10, vec![255u8; 400]).unwrap();
    // Of a page past the last, and of a part past a page's edge (and past
    // the end of a `u32`).
    let far = TileKey { page: 9999, size: (10, 10), at: (0, 0), w: 10, h: 10 };
    let past = TileKey { page: 0, at: (u32::MAX - 5, 0), ..far };
    for key in [far, past] {
        r.h.send(Msg::Worker(serial, Event::Drawn(key, image.clone())));
        settle(&mut r);
        assert!(!doc(&r).tiles.has(&key), "{key:?} was kept");
    }
    // Hits on a page past the last are let go too.
    r.h.send(Msg::Find);
    r.h.send(Msg::FindQuery(w, "page".into()));
    r.h.advance(Duration::from_millis(250));
    settle(&mut r);
    let find = doc(&r).find.as_ref().map(|f| f.serial).expect("a search");
    let hits = vec![vec![[0.0, 0.0, 10.0, 10.0]]];
    r.h.send(Msg::Worker(serial, Event::Reply(Reply::Found { serial: find, page: 9999, hits })));
    assert_eq!(doc(&r).find.as_ref().map(|f| f.count()), Some(3));
    r.h.send(Msg::FindNext);
    r.h.send(Msg::FindNext);
    r.h.send(Msg::FindNext);
    settle(&mut r);
    assert!(matches!(doc(&r).load, Load::Open(_)));
    assert_eq!(doc(&r).find.as_ref().and_then(|f| f.current).map(|(p, _)| p), Some(0));
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

#[test]
fn a_drag_from_a_link_chooses_its_text() {
    let mut r = rig("link-drag", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    let (a, b) = text_span(&r, 0, "Visit sekka.computer");
    // Pointed at, it says where it goes; dragged over, it's text.
    r.h.move_to(w, view_point(&r, 0, a));
    assert!(r.h.texts(w).iter().any(|t| t == "https://sekka.computer/"), "{:?}", r.h.texts(w));
    r.h.drag(w, view_point(&r, 0, (a.0 - 3.0, a.1)), view_point(&r, 0, (b.0 + 3.0, b.1)), 6);
    settle(&mut r);
    assert!(r.h.program().opened.is_empty(), "the link was followed");
    r.h.command(w, 'c');
    assert_eq!(r.h.platform.clipboard.as_deref(), Some("Visit sekka.computer"));
    // A press that barely moves is still a click.
    let at = view_point(&r, 0, a);
    r.h.press(w, at, PointerButton::Left, 1);
    r.h.move_to(w, at + Vec2::new(2.0, 1.0));
    r.h.release(w, at + Vec2::new(2.0, 1.0), PointerButton::Left);
    assert_eq!(r.h.program().opened, vec!["https://sekka.computer/".to_owned()]);
}

#[test]
fn escape_empties_find_closes_it_and_leaves_full_screen() {
    let mut r = rig("escape", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    r.h.window_state(w, WindowState { fullscreen: true, maximized: false, suspended: false });
    settle(&mut r);
    assert!(r.h.find_id(w, "page-number").is_none(), "the toolbar showed in full screen");
    // Find shows in full screen too, and has the keys.
    r.h.command(w, 'f');
    settle(&mut r);
    assert!(r.h.find_id(w, "find").is_some(), "no find field in full screen");
    r.h.type_text(w, "needle");
    r.h.advance(Duration::from_millis(250));
    settle(&mut r);
    assert_eq!(doc(&r).find.as_ref().map(|f| f.count()), Some(4));
    r.h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(doc(&r).find.as_ref().map(|f| f.query.as_str()), Some(""));
    r.h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(doc(&r).find.is_none());
    r.h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(r.h.fullscreen_asked(w), Some(false));
}

#[test]
fn copies_pages_whose_text_hadnt_come() {
    let mut r = rig("copy-far", &[("long.pdf", common::long(80))], &["long.pdf"]);
    let w = r.h.window();
    // From the first page to the seventieth, gone to straight away: the
    // pages between were never read.
    r.h.send(Msg::GoPage(w, 69));
    settle(&mut r);
    let end = doc(&r).texts.get(&69).expect("the page's text").chars.len();
    assert!(!doc(&r).texts.contains_key(&30));
    r.h.program_mut().docs[0].selection =
        Some(Selection { anchor: Caret { page: 0, index: 0 }, focus: Caret { page: 69, index: end } });
    r.h.send(Msg::Copy);
    assert!(doc(&r).copying.is_some(), "it copied what it had");
    assert!(r.h.platform.clipboard.is_none());
    assert!(r.h.texts(w).iter().any(|t| t.starts_with("Reading ") && t.ends_with(" to copy…")), "{:?}", r.h.texts(w));
    settle(&mut r);
    let copied = r.h.platform.clipboard.clone().expect("a copy");
    let lines: Vec<&str> = copied.lines().collect();
    assert_eq!(lines.len(), 70, "{copied}");
    assert_eq!(lines[30], "This is page 31 of 80.");
    // What it read to copy isn't all kept.
    assert!(doc(&r).texts.len() <= aizome_shiori::app::KEPT_TEXTS);
    assert!(doc(&r).texts.contains_key(&69));
}

#[test]
fn a_page_that_fails_at_one_size_is_drawn_at_another() {
    let mut r = rig("fail-size", &[("long.pdf", common::long(3))], &["long.pdf"]);
    let fail = |r: &mut Rig| {
        let d = doc(r);
        let size = d.layout.device_size(0, d.scale);
        let key = TileKey { page: 0, size, at: (0, 0), w: size.0.min(1024), h: size.1.min(1024) };
        let serial = d.serial;
        r.h.send(Msg::Worker(serial, Event::Reply(Reply::DrawFailed(key))));
        settle(r);
        size
    };
    let first = fail(&mut r);
    assert!(doc(&r).broken_at(0, first));
    // Zoomed in, it's drawn.
    r.h.send(Msg::ZoomIn);
    settle(&mut r);
    let d = doc(&r);
    let second = d.layout.device_size(0, d.scale);
    assert_ne!(second, first);
    assert!(!d.broken_at(0, second));
    assert!(d.tiles.of_page(0).any(|(k, _)| k.size == second));
    // Failing there too, it isn't tried at a third size.
    fail(&mut r);
    r.h.send(Msg::ZoomIn);
    settle(&mut r);
    let d = doc(&r);
    let third = d.layout.device_size(0, d.scale);
    assert!(d.broken_at(0, third));
    assert!(!d.tiles.of_page(0).any(|(k, _)| k.size == third));
    assert!(!d.broken_at(1, third), "only that page");
}

#[test]
fn find_says_when_it_cant_search() {
    let mut r = rig("find-stopped", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    let serial = doc(&r).serial;
    r.h.send(Msg::Worker(serial, Event::Failed { job: None, why: Failure::Crashed, gave_up: true }));
    r.h.send(Msg::Find);
    r.h.send(Msg::FindQuery(w, "needle".into()));
    r.h.advance(Duration::from_millis(250));
    settle(&mut r);
    assert!(r.h.texts(w).iter().any(|t| t == "Search stopped"), "{:?}", r.h.texts(w));
}

#[test]
fn the_contents_show_where_you_are() {
    let mut r = rig("contents-here", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    // Introduction (1), Wide (2) and in it Turned (3).
    assert_eq!(doc(&r).current_entry(), Some(1));
    r.h.send(Msg::ZoomPick(w, 8));
    r.h.send(Msg::GoPage(w, 3));
    settle(&mut r);
    // Wide is closed, and stands for Turned.
    assert_eq!(doc(&r).current_entry(), Some(2));
    r.h.send(Msg::OutlineToggle(w, 2, true));
    assert_eq!(doc(&r).current_entry(), Some(3));
    r.h.send(Msg::GoPage(w, 2));
    settle(&mut r);
    assert_eq!(doc(&r).current_entry(), Some(2));
}

#[test]
fn keeps_the_sidebar_with_your_place() {
    let mut r = rig("sidebar-kept", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    let w = r.h.window();
    assert_eq!((doc(&r).sidebar, doc(&r).sidebar_tab), (true, 1));
    r.h.send(Msg::SidebarTab(w, 0));
    settle(&mut r);
    r.h.request_close(w);
    let dir = r.dir.clone();
    drop(r);
    let mut r = rig_in(dir, ThemeChoice::Night, &["sample.pdf"]);
    assert_eq!((doc(&r).sidebar, doc(&r).sidebar_tab), (true, 0));
    r.h.send(Msg::ToggleSidebar);
    settle(&mut r);
    let w = r.h.window();
    r.h.request_close(w);
    let dir = r.dir.clone();
    drop(r);
    let r = rig_in(dir, ThemeChoice::Night, &["sample.pdf"]);
    assert!(!doc(&r).sidebar);
}

#[test]
fn the_middle_button_moves_the_pages() {
    let mut r = rig("pan", &[("long.pdf", common::long(3))], &["long.pdf"]);
    let w = r.h.window();
    r.h.send(Msg::ZoomPick(w, 8));
    settle(&mut r);
    let before = doc(&r).scroll;
    let mid = r.h.find_id(w, "pages").unwrap().center();
    let to = mid - Vec2::new(100.0, 50.0);
    r.h.press(w, mid, PointerButton::Middle, 1);
    r.h.move_to(w, to);
    r.h.release(w, to, PointerButton::Middle);
    let after = doc(&r).scroll;
    assert_eq!((after.0 - before.0, after.1 - before.1), (100.0, 50.0));
    assert!(doc(&r).drag.is_none());
}

#[test]
fn a_selection_held_past_the_edge_scrolls_on() {
    let mut r = rig("autoscroll", &[("long.pdf", common::long(10))], &["long.pdf"]);
    let w = r.h.window();
    let (a, _) = text_span(&r, 0, "This");
    let from = view_point(&r, 0, a);
    let below = Point::new(from.x, r.h.find_id(w, "pages").unwrap().y1 + 30.0);
    r.h.press(w, from, PointerButton::Left, 1);
    r.h.move_to(w, below);
    let first = doc(&r).scroll.1;
    for _ in 0..10 {
        r.h.advance(Duration::from_millis(60));
    }
    let later = doc(&r).scroll.1;
    assert!(later > first + 100.0, "{first} then {later}");
    r.h.release(w, below, PointerButton::Left);
    let stopped = doc(&r).scroll.1;
    r.h.advance(Duration::from_millis(300));
    assert_eq!(doc(&r).scroll.1, stopped);
    assert!(doc(&r).selection.is_some_and(|s| s.focus.page > 0), "{:?}", doc(&r).selection);
}

#[test]
fn the_page_number_asks_where_to_go() {
    let mut r = rig("page-number", &[("long.pdf", common::long(12))], &["long.pdf"]);
    let w = r.h.window();
    let at = r.h.find_id(w, "page-number").expect("the page number").center();
    r.h.click(w, at);
    let sheet = *r.h.program().sheets.keys().next().expect("Go to Page");
    assert!(matches!(r.h.program().sheets.get(&sheet), Some(Sheet::GoTo { .. })));
    r.h.type_text(sheet, "7");
    r.h.named(sheet, NamedKey::Enter, Modifiers::NONE);
    settle(&mut r);
    assert_eq!(doc(&r).current_page(), 6);
}

#[test]
fn get_info_says_when_it_was_made() {
    let mut r = rig("info", &[("sample.pdf", common::sample())], &["sample.pdf"]);
    r.h.send(Msg::Info);
    settle(&mut r);
    let sheet = *r.h.program().sheets.keys().next().expect("Get Info");
    let texts = r.h.texts(sheet);
    for t in ["Created", "2026-10-28 09:30", "Modified", "2026-10-29 10:15"] {
        assert!(texts.iter().any(|s| s == t), "no {t} in {texts:?}");
    }
}
