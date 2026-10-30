//! The worker, through the engine the app uses: a real process
//! reading generated documents.

mod common;

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use aizome_shiori::engine::{Engine, Event, Failure};
use aizome_shiori::protocol::{DocInfo, Job, OpenError, Reply, Request, Target, TileKey};
use common::{LinkTo, Page};

fn start(path: std::path::PathBuf, password: Option<&str>) -> (Engine, Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let e = Engine::start(common::worker(), path, password.map(str::to_owned), move |ev| {
        let _ = tx.lock().unwrap().send(ev);
    });
    (e, rx)
}

/// The next event `pick` takes, within ten seconds.
fn wait<T>(rx: &Receiver<Event>, mut pick: impl FnMut(Event) -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let ev = rx.recv_timeout(left).expect("the worker answered in time");
        if let Some(t) = pick(ev) {
            return t;
        }
    }
}

fn opened(rx: &Receiver<Event>) -> DocInfo {
    wait(rx, |e| match e {
        Event::Reply(Reply::Opened(info)) => Some(*info),
        Event::Reply(Reply::OpenFailed(e)) => panic!("didn't open: {e:?}"),
        _ => None,
    })
}

#[test]
fn opens_and_describes_a_document() {
    let dir = common::scratch("worker-open");
    let path = common::write(&dir, "sample.pdf", &common::sample());
    let (_e, rx) = start(path, None);
    let info = opened(&rx);
    // The turned page shows landscape.
    assert_eq!(info.pages, vec![(612.0, 792.0), (612.0, 792.0), (792.0, 612.0), (792.0, 612.0)]);
    assert_eq!(info.outline.len(), 2);
    assert_eq!(info.outline[0].title, "Introduction");
    assert!(matches!(info.outline[0].target, Some(Target::Page { page: 0, .. })));
    assert_eq!(info.outline[1].children[0].title, "Turned");
    assert!(matches!(info.outline[1].children[0].target, Some(Target::Page { page: 3, .. })));
    assert!(!info.encrypted);
    assert!(info.can_copy);
}

#[test]
fn draws_tiles_where_the_text_is() {
    let dir = common::scratch("worker-draw");
    let path = common::write(&dir, "sample.pdf", &common::sample());
    let (e, rx) = start(path, None);
    opened(&rx);
    // Page 1 at 1×, its top half; and the turned page whole.
    let top = TileKey { page: 0, size: (612, 792), at: (0, 0), w: 612, h: 396 };
    let turned = TileKey { page: 3, size: (792, 612), at: (0, 0), w: 792, h: 612 };
    e.send(Request::Draw(vec![top, turned]));
    e.send(Request::Text(0));
    e.send(Request::Text(3));
    let mut images = Vec::new();
    let mut texts = Vec::new();
    while images.len() < 2 || texts.len() < 2 {
        wait(&rx, |ev| {
            match ev {
                Event::Drawn(k, img) => images.push((k, img)),
                Event::Reply(Reply::Text { page, text }) => texts.push((page, text)),
                Event::Reply(Reply::DrawFailed(k)) => panic!("{k:?} wasn't drawn"),
                _ => return None,
            }
            Some(())
        });
    }
    // Each character the worker found is over ink in the drawing.
    for (page, text) in &texts {
        let (key, img) = images.iter().find(|(k, _)| k.page == *page).unwrap();
        let rgba = img.to_rgba8().expect("pixels");
        let dark = |x: f32, y: f32, x1: f32, y1: f32| {
            let mut n = 0;
            for yy in y as u32..(y1 as u32).min(key.h) {
                for xx in x as u32..(x1 as u32).min(key.w) {
                    let p = &rgba[(yy * key.w + xx) as usize * 4..][..3];
                    n += usize::from(p.iter().all(|c| *c < 128));
                }
            }
            n
        };
        let s: String = text.chars.iter().collect();
        let mut letters = 0;
        for (c, b) in text.chars.iter().zip(&text.boxes) {
            if c.is_alphanumeric() && b[3] < key.h as f32 {
                letters += 1;
                assert!(dark(b[0], b[1], b[2], b[3]) > 0, "page {page}: no ink under {c:?} at {b:?} in {s:?}");
            }
        }
        assert!(letters > 10, "page {page}: {s:?}");
    }
    let (_, first) = texts.iter().find(|(p, _)| *p == 0).unwrap();
    let s: String = first.chars.iter().collect();
    assert!(s.contains("Shiori reads documents."), "{s:?}");
    // The turned page's line runs down its right side.
    let (_, turned) = texts.iter().find(|(p, _)| *p == 3).unwrap();
    let b = turned.boxes[turned.chars.iter().position(|c| *c == 'A').unwrap()];
    assert!(b[0] > 700.0 && b[2] < 745.0 && b[1] > 60.0 && b[1] < 90.0, "{b:?}");
}

#[test]
fn reads_links_and_finds_text() {
    let dir = common::scratch("worker-links");
    let path = common::write(&dir, "sample.pdf", &common::sample());
    let (e, rx) = start(path, None);
    opened(&rx);
    e.send(Request::Links(0));
    let links = wait(&rx, |ev| match ev {
        Event::Reply(Reply::Links { page: 0, links }) => Some(links),
        _ => None,
    });
    assert_eq!(links.len(), 2);
    let inner = links.iter().find(|l| matches!(l.target, Target::Page { page: 2, .. })).expect("a link to page 3");
    // [70, 598, 300, 622] up from the bottom of a 792pt page.
    assert_eq!(inner.rect, [70.0, 170.0, 300.0, 194.0]);
    assert!(links.iter().any(|l| l.target == Target::Uri("https://sekka.computer/".into())));

    e.send(Request::Find { serial: 7, query: "needle".into(), case: false, pages: vec![0, 1, 2, 3] });
    let mut found = Vec::new();
    while found.len() < 4 {
        found.push(wait(&rx, |ev| match ev {
            Event::Reply(Reply::Found { serial: 7, page, hits }) => Some((page, hits.len())),
            _ => None,
        }));
    }
    assert_eq!(found, vec![(0, 1), (1, 2), (2, 0), (3, 1)]);
    e.send(Request::Find { serial: 8, query: "Needle".into(), case: true, pages: vec![1] });
    let hits = wait(&rx, |ev| match ev {
        Event::Reply(Reply::Found { serial: 8, hits, .. }) => Some(hits),
        _ => None,
    });
    assert_eq!(hits.len(), 1);
}

#[test]
fn asks_for_a_password_and_says_what_isnt_a_pdf() {
    let dir = common::scratch("worker-errors");
    let junk = common::write(&dir, "junk.pdf", b"this is not a PDF at all");
    let (_e, rx) = start(junk, None);
    let err = wait(&rx, |e| match e {
        Event::Reply(Reply::OpenFailed(err)) => Some(err),
        _ => None,
    });
    assert!(matches!(err, OpenError::Format(_)), "{err:?}");
    let (_e, rx) = start(dir.join("missing.pdf"), None);
    let err = wait(&rx, |e| match e {
        Event::Reply(Reply::OpenFailed(err)) => Some(err),
        _ => None,
    });
    assert!(matches!(err, OpenError::Io(_)), "{err:?}");
}

#[test]
fn a_worker_that_dies_is_replaced() {
    let dir = common::scratch("worker-crash");
    let path = common::write(&dir, "sample.pdf", &common::sample());
    let (e, rx) = start(path, None);
    opened(&rx);
    let pid = e.pid().expect("a worker");
    // SAFETY: a signal to our own child.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    let (job, why, gave_up) = wait(&rx, |ev| match ev {
        Event::Failed { job, why, gave_up } => Some((job, why, gave_up)),
        _ => None,
    });
    assert_eq!((job, why, gave_up), (None, Failure::Crashed, false));
    // Another opens the document and draws.
    opened(&rx);
    assert_ne!(e.pid(), Some(pid));
    e.send(Request::Draw(vec![TileKey { page: 1, size: (61, 79), at: (0, 0), w: 61, h: 79 }]));
    wait(&rx, |ev| matches!(ev, Event::Drawn(..)).then_some(()));
    let _ = Job::Open;
}

/// `tests/fixtures/locked.pdf` is `sample()` encrypted with AES-256 by
/// qpdf 12.4 (`qpdf --encrypt --user-password=kagi
/// --owner-password=kagi-owner --bits=256 -- sample.pdf locked.pdf`).
fn locked() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/locked.pdf")
}

#[test]
fn opens_a_locked_document_with_its_password() {
    let failed = |password: Option<&str>| {
        let (_e, rx) = start(locked(), password);
        wait(&rx, |e| match e {
            Event::Reply(Reply::OpenFailed(err)) => Some(err),
            Event::Reply(Reply::Opened(_)) => panic!("opened with {password:?}"),
            _ => None,
        })
    };
    assert_eq!(failed(None), OpenError::Password { tried: false });
    assert_eq!(failed(Some("wrong")), OpenError::Password { tried: true });
    let (_e, rx) = start(locked(), Some("kagi"));
    let info = opened(&rx);
    assert_eq!(info.pages.len(), 4);
    assert!(info.encrypted);
}

/// A page with a link 10^40 points wide and a word 10^39 points along,
/// numbers an `f32` can't hold, besides an ordinary link and line.
fn too_large() -> Vec<u8> {
    let mut page = Page::letter().text(72.0, 720.0, 18.0, "An ordinary line.");
    page.links.push(([70.0, 700.0, 300.0, 740.0], LinkTo::Uri("https://sekka.computer/".into())));
    page.raw_annots.push(format!(
        "<< /Type /Annot /Subtype /Link /Rect [0 0 1{} 20] /A << /S /URI /URI (https://example.com/) >> >>",
        "0".repeat(40)
    ));
    page.raw_content = format!("BT /F1 18 Tf 72 680 Td (far) Tj 1{} 0 Td (away) Tj ET\n", "0".repeat(39));
    common::pdf(&[page], &[])
}

#[test]
fn what_isnt_a_number_is_left_out_not_taken_for_a_crash() {
    let dir = common::scratch("worker-too-large");
    let path = common::write(&dir, "too-large.pdf", &too_large());
    let (e, rx) = start(path, None);
    opened(&rx);
    e.send(Request::Links(0));
    e.send(Request::Text(0));
    e.send(Request::Find { serial: 3, query: "away".into(), case: false, pages: vec![0] });
    let (mut links, mut text, mut hits) = (None, None, None);
    while links.is_none() || text.is_none() || hits.is_none() {
        wait(&rx, |ev| {
            match ev {
                Event::Reply(Reply::Links { links: l, .. }) => links = Some(l),
                Event::Reply(Reply::Text { text: t, .. }) => text = Some(t),
                Event::Reply(Reply::Found { hits: h, .. }) => hits = Some(h),
                Event::Failed { job, why, .. } => panic!("the worker was taken for broken on {job:?} ({why:?})"),
                _ => return None,
            }
            Some(())
        });
    }
    // The link too large to be is left out; the ordinary one stays.
    let links = links.unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].target, Target::Uri("https://sekka.computer/".into()));
    let text = text.unwrap();
    let s: String = text.chars.iter().collect();
    assert!(s.contains("An ordinary line."), "{s:?}");
    assert!(text.boxes.iter().flatten().all(|v| v.is_finite()), "{:?}", text.boxes);
    assert!(hits.unwrap().iter().flatten().flatten().all(|v| v.is_finite()));
    // And it carries on.
    e.send(Request::Draw(vec![TileKey { page: 0, size: (61, 79), at: (0, 0), w: 61, h: 79 }]));
    wait(&rx, |ev| match ev {
        Event::Drawn(..) => Some(()),
        Event::Failed { job, .. } => panic!("the worker was taken for broken on {job:?}"),
        _ => None,
    });
}
