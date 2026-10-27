//! The window, headless: pages, the sunburst's clicks and drags, the
//! Collector and the Trash, and golden images of the main views.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_kit::{Point, Size};
use kura::mounts::Mount;
use kura::nix::StoreSize;
use kura::scan::{ProgressSnapshot, Scan, ScanOptions};
use kura::tree::{Entry, Kind, NodeId, Tree};
use kura::ui::rings::{self, Geometry, RINGS};
use kura::ui::{Kura, Msg, Page};

const GB: u64 = 1_000_000_000;
const MB: u64 = 1_000_000;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kura-ui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(&d).unwrap()
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name)
}

/// The window against its golden picture, once the rings' smooth drawing
/// (made on a worker; plain until it comes) is there.
fn golden_settled(h: &mut Harness<Kura>, w: aizome_kit::WindowKey, path: PathBuf, scale: f64) {
    let _ = h.render(w, scale);
    for _ in 0..1000 {
        if h.program().rings.borrow().settled() {
            return assert_golden(h, w, path, scale);
        }
        std::thread::sleep(Duration::from_millis(2));
        h.send(Msg::Poll);
    }
    panic!("the rings were never drawn");
}

/// A made-up home: what a developer's often holds.
enum Fake {
    F(&'static str, u64),
    D(&'static str, Vec<Fake>),
}
use Fake::{D, F};

fn many(prefix: &'static str, n: usize, each: u64) -> Vec<Fake> {
    (0..n)
        .map(|i| {
            F(Box::leak(format!("{prefix}-{i:03}.jpg").into_boxed_str()), each + (i as u64 * 7919) % (each / 3 + 1))
        })
        .collect()
}

fn home() -> Fake {
    D(
        "/home/sekka",
        vec![
            D(
                "work",
                vec![
                    D(
                        "aizome",
                        vec![
                            D(
                                "target",
                                vec![D(
                                    "debug",
                                    vec![
                                        D(
                                            "deps",
                                            vec![
                                                F("libsidestep_ui.rlib", 9 * GB),
                                                F("libsmithay.rlib", 6 * GB),
                                                F("aizome-comp", 4 * GB),
                                            ],
                                        ),
                                        D("build", vec![F("out.o", 5 * GB)]),
                                        D("incremental", vec![F("state.bin", 3 * GB)]),
                                    ],
                                )],
                            ),
                            D("crates", vec![F("lib.rs", 2 * MB), F("main.rs", MB)]),
                            F("Cargo.toml", 4096),
                        ],
                    ),
                    D(
                        "sekka",
                        vec![
                            D("result", vec![F("system.img", 3 * GB)]),
                            D("desktop", vec![F("wallpapers.tar", 2 * GB)]),
                            F("flake.lock", 8192),
                        ],
                    ),
                    D("notes", vec![F("plan.md", 40_000)]),
                ],
            ),
            D(
                "Videos",
                vec![
                    F("holiday-2025.mkv", 8 * GB),
                    F("conference-talk.mp4", 3 * GB),
                    D("clips", vec![F("a.mp4", GB), F("b.mp4", 700 * MB)]),
                ],
            ),
            D("Pictures", vec![D("2025", many("IMG", 60, 90 * MB)), D("wallpapers", many("wall", 12, 40 * MB))]),
            D(
                "Downloads",
                vec![
                    F("sekka-nightly.iso", 3 * GB + 400 * MB),
                    F("old-backup.zip", 1200 * MB),
                    F("installer.AppImage", 400 * MB),
                ],
            ),
            D(
                ".cache",
                vec![
                    D("google-chrome", vec![F("data_1", 1900 * MB)]),
                    D("pip", vec![F("wheels", 800 * MB)]),
                    D("nix", vec![F("eval.sqlite", 600 * MB)]),
                ],
            ),
            D(
                ".local",
                vec![D(
                    "share",
                    vec![
                        D("containers", vec![F("layers.tar", 4 * GB)]),
                        D("Trash", vec![F("old-vm.qcow2", 1100 * MB)]),
                    ],
                )],
            ),
            D(
                "Documents",
                vec![F("thesis.pdf", 30 * MB), F("taxes-2025.pdf", 4 * MB), D("scans", many("scan", 20, 15 * MB))],
            ),
            D("Music", vec![D("albums", many("track", 40, 60 * MB))]),
            F(".bash_history", 200_000),
        ],
    )
}

fn push(t: &mut Tree, at: NodeId, kids: &[Fake]) {
    let entries: Vec<Entry> = kids
        .iter()
        .map(|k| match k {
            F(n, s) => Entry {
                name: n.as_bytes().to_vec(),
                kind: Kind::File,
                flags: 0,
                alloc: *s,
                apparent: *s,
                mtime: 1_760_000_000,
            },
            D(n, _) => Entry {
                name: n.as_bytes().to_vec(),
                kind: Kind::Dir,
                flags: 0,
                alloc: 4096,
                apparent: 4096,
                mtime: 1_760_000_000,
            },
        })
        .collect();
    let first = t.push_children(at, &entries);
    for (i, k) in kids.iter().enumerate() {
        if let D(_, sub) = k
            && !sub.is_empty()
        {
            push(t, NodeId(first + i as u32), sub);
        }
    }
}

fn fake_scan() -> Arc<Scan> {
    let D(root, kids) = home() else { unreachable!() };
    let mut t = Tree::new(PathBuf::from(root), Kind::Dir, 4096, 4096, 1_760_000_000);
    push(&mut t, NodeId::ROOT, &kids);
    t.finish();
    Arc::new(Scan {
        tree: t,
        options: ScanOptions::new(root),
        started: 1_760_000_000,
        duration: Duration::from_millis(840),
        error_count: 0,
        errors: Vec::new(),
        other_filesystems: Vec::new(),
        hard_links: 0,
        threads: 8,
    })
}

fn fake_mounts() -> Vec<Mount> {
    let m = |p: &str, s: &str, t: &str, total: u64, free: u64| Mount {
        mount_point: PathBuf::from(p),
        source: s.into(),
        fs_type: t.into(),
        total_bytes: total,
        free_bytes: free,
        available_bytes: free,
        read_only: false,
    };
    vec![
        m("/", "/dev/nvme0n1p2", "ext4", 469 * GB, 17 * GB),
        m("/boot", "/dev/nvme0n1p1", "vfat", GB, 800 * MB),
        m("/run/media/sekka/Archive", "/dev/sda1", "btrfs", 2000 * GB, 1240 * GB),
    ]
}

/// Run the program's background work until `done` says so.
fn settle(h: &mut Harness<Kura>, done: impl Fn(&Kura) -> bool) {
    for _ in 0..600 {
        if done(h.program()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.advance(Duration::from_millis(100));
        h.send(Msg::Poll);
    }
    panic!("the program didn't settle");
}

/// Kura in `look`, writing only into `out`, with fixed disks and store.
fn kura(out: &Path, look: Appearance) -> Harness<Kura> {
    let mut k = Kura::sandboxed(out);
    k.analyse = false;
    k.has_store = true;
    k.index_dir = None;
    let mut h = Harness::with(k, look, false);
    // The real disks arrive first; then the made-up ones replace them.
    settle(&mut h, |k| !k.mounts.is_empty());
    h.send(Msg::Disks(fake_mounts()));
    h.send(Msg::StoreMeasured(Some(StoreSize { bytes: 54_400_000_000, paths: 37_879, ..StoreSize::default() })));
    let w = h.window();
    h.resize(w, Size::new(960.0, 640.0));
    h
}

fn night() -> Appearance {
    Appearance::default()
}

fn day() -> Appearance {
    Appearance { theme: ThemeChoice::Day, ..Appearance::default() }
}

fn with_results(h: &mut Harness<Kura>) {
    h.send(Msg::Scanned(Ok(fake_scan())));
    h.send(Msg::Indexed(Ok(fake_analysis())));
    assert_eq!(h.program().page, Page::Results);
}

/// The middle of segment `name` (a child of the centre) in window points.
fn point_on(h: &Harness<Kura>, name: &str) -> Point {
    let w = h.window();
    let rect = h.find_id(w, "sunburst").expect("the sunburst");
    let k = h.program();
    let tree = k.tree().unwrap();
    let segs = rings::segments(tree, k.center, RINGS);
    let seg = segs.iter().find(|s| s.depth == 1 && s.node.is_some_and(|n| tree.name(n) == name)).expect("its segment");
    let g = Geometry::new(rect.size(), RINGS);
    let a = Geometry::angle((seg.x0 + seg.x1) / 2.0);
    let r = g.hole + g.ring * 0.5;
    Point::new(rect.x0 + g.center.x + r * a.cos(), rect.y0 + g.center.y + r * a.sin())
}

fn center_point(h: &Harness<Kura>) -> Point {
    let rect = h.find_id(h.window(), "sunburst").unwrap();
    let g = Geometry::new(rect.size(), RINGS);
    Point::new(rect.x0 + g.center.x, rect.y0 + g.center.y)
}

#[test]
fn disks_page() {
    let out = scratch("disks");
    let mut h = kura(&out, night());
    let w = h.window();
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "System"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Archive"));
    assert!(texts.iter().any(|t| t == "Nix store"));
    assert!(texts.iter().any(|t| t.contains("54.4 GB in 37,879 store paths")));
    assert!(texts.iter().any(|t| t == "Open Yukimi's Storage"));
    assert_golden(&mut h, w, golden("disks-night.png"), 2.0);
    h.set_appearance(day());
    assert_golden(&mut h, w, golden("disks-day.png"), 2.0);
}

#[test]
fn scanning_page() {
    let out = scratch("scanning");
    let mut h = kura(&out, night());
    let w = h.window();
    h.program_mut().show_progress(
        PathBuf::from("/home/sekka"),
        ProgressSnapshot {
            files: 412_331,
            dirs: 38_212,
            bytes: 182_400_000_000,
            errors: 0,
            current: PathBuf::from("/home/sekka/work/aizome/target/debug/deps"),
        },
    );
    h.send(Msg::Dismiss);
    assert!(h.texts(w).iter().any(|t| t == "412,331 files · 38,212 folders"));
    assert_golden(&mut h, w, golden("scanning-night.png"), 2.0);
}

#[test]
fn results_draw_and_drill() {
    let out = scratch("results");
    let mut h = kura(&out, night());
    let w = h.window();
    with_results(&mut h);
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "work"), "the list shows the top level: {texts:?}");
    golden_settled(&mut h, w, golden("results-night.png"), 2.0);
    h.set_appearance(day());
    golden_settled(&mut h, w, golden("results-day.png"), 2.0);
    h.set_appearance(night());

    // A click on a folder's segment goes into it; the middle goes back up.
    let p = point_on(&h, "work");
    h.click(w, p);
    let k = h.program();
    assert_eq!(k.tree().unwrap().name(k.center), "work");
    assert!(k.transition.is_some_and(|t| t.zoom_in), "going down a level moves the rings");
    h.click(w, center_point(&h));
    let k = h.program();
    assert_eq!(k.center, NodeId::ROOT);
    assert!(k.transition.is_some_and(|t| !t.zoom_in));
    assert_eq!(k.selected.map(|s| k.tree().unwrap().name(s).into_owned()).as_deref(), Some("work"));
}

#[test]
fn reduced_motion_cuts() {
    let out = scratch("reduced");
    let mut h = kura(&out, Appearance { reduced_motion: true, ..night() });
    with_results(&mut h);
    let w = h.window();
    let p = point_on(&h, "Videos");
    h.click(w, p);
    let k = h.program();
    assert_eq!(k.tree().unwrap().name(k.center), "Videos");
    assert!(k.transition.is_none());
}

#[test]
fn hovering_and_collecting() {
    let out = scratch("collect");
    let mut h = kura(&out, night());
    with_results(&mut h);
    let w = h.window();
    let tree = h.program().tree().unwrap().clone();
    let id = |p: &str| tree.find(Path::new(p)).unwrap();

    h.send(Msg::Collect(id("/home/sekka/work/sekka/result")));
    assert_eq!(h.program().collector.len(), 1);
    // Something inside what's collected is already collected; collecting
    // its folder takes the inner one's place.
    h.send(Msg::Collect(id("/home/sekka/work/sekka/result/system.img")));
    assert_eq!(h.program().collector.len(), 1);
    h.send(Msg::Collect(id("/home/sekka/work/sekka")));
    assert_eq!(h.program().collector, vec![id("/home/sekka/work/sekka")]);
    h.send(Msg::Collect(id("/home/sekka/Downloads/sekka-nightly.iso")));
    assert_eq!(h.program().collected_bytes(), tree.node(id("/home/sekka/work/sekka")).alloc + 3_400_000_000);

    h.move_to(w, point_on(&h, "Pictures"));
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Move to Trash…"));
    golden_settled(&mut h, w, golden("results-hover-collector-night.png"), 2.0);
}

#[test]
fn dragging_a_segment_into_the_collector() {
    let out = scratch("drag");
    let mut h = kura(&out, night());
    with_results(&mut h);
    let w = h.window();
    // The Collector says where it is when it's drawn.
    let _ = h.render(w, 1.0);
    let zone = h.find_id(w, "collector").expect("the collector");
    let from = point_on(&h, "Videos");
    h.press(w, from, PointerButton::Left, 1);
    h.move_to(w, Point::new(from.x + 10.0, from.y + 10.0));
    assert!(h.program().dragging.is_some(), "moving past the slop starts a drag");
    let to = zone.center();
    h.move_to(w, to);
    assert!(h.program().drag_over);
    golden_settled(&mut h, w, golden("results-drag-night.png"), 2.0);
    h.release(w, to, PointerButton::Left);
    let k = h.program();
    assert!(k.dragging.is_none());
    assert_eq!(k.collector.len(), 1);
    assert_eq!(k.tree().unwrap().name(k.collector[0]), "Videos");
    assert_eq!(k.center, NodeId::ROOT, "a drag isn't a click");
}

#[test]
fn moving_to_the_trash_and_writing_the_index() {
    let out = scratch("trash");
    // A real folder this time: the scanner, the Trash and the index.
    let home = out.join("home");
    std::fs::create_dir_all(home.join("big")).unwrap();
    std::fs::write(home.join("big/blob.bin"), vec![7u8; 3_000_000]).unwrap();
    std::fs::write(home.join("keep.txt"), b"keep").unwrap();
    let mut k = Kura::sandboxed(&out);
    k.has_store = false;
    k.duplicates = false;
    let mut h = Harness::new(k);
    h.send(Msg::Scan(home.clone()));
    settle(&mut h, |k| {
        k.page == Page::Results && k.busy == 0 && matches!(k.index, kura::ui::IndexState::Written { .. })
    });
    let index = out.join("index/index.json");
    assert!(index.exists() && out.join("index/index.md").exists());
    let before = h.program().tree().unwrap().node(NodeId::ROOT).alloc;

    let big = h.program().tree().unwrap().find(&home.join("big")).unwrap();
    h.send(Msg::Collect(big));
    h.send(Msg::Trash);
    settle(&mut h, |k| {
        k.collector.is_empty() && k.busy == 0 && matches!(k.index, kura::ui::IndexState::Written { .. })
    });
    assert!(!home.join("big").exists());
    assert!(out.join("Trash/files/big/blob.bin").exists());
    assert!(out.join("Trash/info/big.trashinfo").exists());
    let k = h.program();
    let after = k.tree().unwrap().node(NodeId::ROOT).alloc;
    assert!(before - after >= 3_000_000, "the tree no longer counts it");
    assert!(k.notice.as_deref().is_some_and(|n| n.starts_with("Moved 1 item to the Trash")));
    assert!(!k.rows.contains(&big));
    let ix = kura::index::read(&index).unwrap();
    assert!(ix.totals.allocated_bytes < before);
    assert!(!ix.largest_files.iter().any(|f| f.path.ends_with("blob.bin")));
}

#[test]
fn the_nix_store_is_never_collected() {
    let out = scratch("store");
    let mut h = kura(&out, night());
    let e = |n: &str, k: Kind, a: u64| Entry {
        name: n.as_bytes().to_vec(),
        kind: k,
        flags: 0,
        alloc: a,
        apparent: a,
        mtime: 0,
    };
    let mut t = Tree::new(PathBuf::from("/nix"), Kind::Dir, 4096, 4096, 0);
    let s = t.push_children(NodeId::ROOT, &[e("store", Kind::Dir, 4096), e("var", Kind::Dir, 4096)]);
    t.push_children(NodeId(s), &[e("abc123-firefox-140", Kind::Dir, 300 * MB)]);
    t.finish();
    let scan = Scan {
        tree: t,
        options: ScanOptions::new("/nix"),
        started: 0,
        duration: Duration::ZERO,
        error_count: 0,
        errors: Vec::new(),
        other_filesystems: Vec::new(),
        hard_links: 0,
        threads: 1,
    };
    h.send(Msg::Scanned(Ok(Arc::new(scan))));
    let tree = h.program().tree().unwrap().clone();
    for p in ["/nix/store", "/nix/store/abc123-firefox-140"] {
        h.send(Msg::Collect(tree.find(Path::new(p)).unwrap()));
        assert!(h.program().collector.is_empty(), "{p} isn't collected");
        assert!(h.program().notice.as_deref().is_some_and(|n| n.contains("Yukimi")));
    }
    // Inside the store, the list says how it's cleaned.
    h.send(Msg::Open(tree.find(Path::new("/nix/store")).unwrap()));
    assert!(h.texts(h.window()).iter().any(|t| t == "Open Yukimi's Storage"));
}

/// A home of 29,000 files in four levels, sizes spread unevenly.
fn big_scan() -> Arc<Scan> {
    let mut t = Tree::new(PathBuf::from("/home/big"), Kind::Dir, 4096, 4096, 0);
    let mut seed = 0x9e37_79b9_u64;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let dir =
        |n: String| Entry { name: n.into_bytes(), kind: Kind::Dir, flags: 0, alloc: 4096, apparent: 4096, mtime: 0 };
    let top = t.push_children(NodeId::ROOT, &(0..12).map(|i| dir(format!("top-{i}"))).collect::<Vec<_>>());
    for a in 0..12 {
        let mid = t.push_children(NodeId(top + a), &(0..8).map(|i| dir(format!("mid-{i}"))).collect::<Vec<_>>());
        for b in 0..8 {
            let low = t.push_children(NodeId(mid + b), &(0..10).map(|i| dir(format!("low-{i}"))).collect::<Vec<_>>());
            for c in 0..10 {
                let files: Vec<Entry> = (0..30)
                    .map(|i| {
                        let s = (rand() % 50_000_000) * (1 + (a as u64 % 4)) * (1 + (i as u64 % 3));
                        Entry {
                            name: format!("file-{i}.bin").into_bytes(),
                            kind: Kind::File,
                            flags: 0,
                            alloc: s,
                            apparent: s,
                            mtime: 0,
                        }
                    })
                    .collect();
                t.push_children(NodeId(low + c), &files);
            }
        }
    }
    t.finish();
    Arc::new(Scan {
        tree: t,
        options: ScanOptions::new("/home/big"),
        started: 0,
        duration: Duration::ZERO,
        error_count: 0,
        errors: Vec::new(),
        other_filesystems: Vec::new(),
        hard_links: 0,
        threads: 8,
    })
}

fn frame(h: &mut Harness<Kura>, act: impl FnOnce(&mut Harness<Kura>)) -> Option<Duration> {
    let w = h.window();
    h.take_damage(w);
    let start = std::time::Instant::now();
    act(h);
    // The damage's rectangles, as the window's renderer draws them.
    let damage = h.take_damage(w);
    h.render_damage(w, &damage, 2.0)?;
    Some(start.elapsed())
}

/// Print the percentiles: the median.
fn report(what: &str, mut frames: Vec<Duration>) -> Duration {
    frames.sort();
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
    eprintln!(
        "kura {what}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
        p(0.5),
        p(0.95),
        frames.last().unwrap(),
        frames.len()
    );
    p(0.5)
}

#[test]
fn hovering_and_going_down_fit_the_frame_budget() {
    let out = scratch("perf");
    let mut h = kura(&out, night());
    h.send(Msg::Scanned(Ok(big_scan())));
    let w = h.window();
    let _ = h.render(w, 1.0);
    let rect = h.find_id(w, "sunburst").unwrap();
    let g = Geometry::new(rect.size(), RINGS);
    eprintln!("kura: {} segments in the rings", h.program().segs.len());

    // The pointer moving around the rings: each new segment it's over is a
    // frame (the middle and the lit segment change).
    let mut hover = Vec::new();
    for i in 0..600 {
        let a = i as f64 / 600.0 * std::f64::consts::TAU * 3.0;
        let r = g.hole + g.ring * (0.5 + (i % 4) as f64);
        let p = Point::new(rect.x0 + g.center.x + r * a.cos(), rect.y0 + g.center.y + r * a.sin());
        if let Some(d) = frame(&mut h, |h| h.move_to(w, p)) {
            hover.push(d);
        }
    }
    let hover50 = report("hover", hover);

    // Going down a level and back up: the click, then the rings' frames.
    let mut moving = Vec::new();
    for _ in 0..6 {
        let p = point_on(&h, "top-0");
        h.move_to(w, p);
        if let Some(d) = frame(&mut h, |h| h.click(w, p)) {
            moving.push(d);
        }
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(8));
            if let Some(d) = frame(&mut h, |h| h.advance(Duration::from_millis(8))) {
                moving.push(d);
            }
        }
        std::thread::sleep(Duration::from_millis(90));
        h.advance(Duration::from_millis(100));
        let c = center_point(&h);
        h.move_to(w, c);
        if let Some(d) = frame(&mut h, |h| h.click(w, c)) {
            moving.push(d);
        }
        std::thread::sleep(Duration::from_millis(90));
        h.advance(Duration::from_millis(100));
    }
    let moving50 = report("going down and up", moving);

    // The list: arrow keys through the rows.
    let row = h.find(w, "top-0").unwrap();
    h.click(w, row.center());
    let mut keys = Vec::new();
    for i in 0..40 {
        let k = if i % 20 < 10 {
            aizome_kit::sidestep_ui::NamedKey::ArrowDown
        } else {
            aizome_kit::sidestep_ui::NamedKey::ArrowUp
        };
        if let Some(d) = frame(&mut h, |h| h.named(w, k, aizome_kit::sidestep_ui::Modifiers::NONE)) {
            keys.push(d);
        }
    }
    let keys50 = report("list arrows", keys);
    // The medians, which a busy machine (other builds, other tests in this
    // process) moves less than the tails; docs/performance.md has both.
    if !cfg!(debug_assertions) {
        assert!(hover50 < Duration::from_millis(4), "hover p50 {hover50:?}");
        assert!(keys50 < Duration::from_millis(4), "list p50 {keys50:?}");
        assert!(moving50 < Duration::from_millis(8), "moving p50 {moving50:?}");
    }
}

#[test]
#[ignore = "a measurement, not a check"]
fn where_a_frame_goes() {
    let out = scratch("perf-split");
    let mut h = kura(&out, night());
    h.send(Msg::Scanned(Ok(big_scan())));
    let w = h.window();
    let _ = h.render(w, 1.0);
    let rect = h.find_id(w, "sunburst").unwrap();
    let mut update = Vec::new();
    let mut paint = Vec::new();
    let mut whole = Vec::new();
    for i in 0..200 {
        let a = i as f64 / 200.0 * std::f64::consts::TAU;
        let g = Geometry::new(rect.size(), RINGS);
        let r = g.hole + g.ring * 1.5;
        let p = Point::new(rect.x0 + g.center.x + r * a.cos(), rect.y0 + g.center.y + r * a.sin());
        h.take_damage(w);
        let t = std::time::Instant::now();
        h.move_to(w, p);
        update.push(t.elapsed());
        let damage = h.take_damage(w);
        if let Some(b) = damage.into_iter().reduce(|a, b| a.union(b)) {
            let t = std::time::Instant::now();
            let _ = h.render_rect(w, b, 2.0);
            paint.push(t.elapsed());
            eprintln!("damage {b:?}");
        }
        let t = std::time::Instant::now();
        let _ = h.render_rect(w, rect, 2.0);
        whole.push(t.elapsed());
    }
    report("event, update, view, layout", update);
    report("paint the damage", paint);
    report("paint the whole sunburst", whole);
}

#[test]
#[ignore = "a measurement, not a check"]
fn what_painting_nothing_costs() {
    let out = scratch("perf-empty");
    let mut h = kura(&out, night());
    let w = h.window();
    // The disks page's empty lower half, the sunburst's size.
    let r = aizome_kit::Rect::new(0.0, 0.0, 620.0, 588.0);
    let mut v = Vec::new();
    for _ in 0..100 {
        let t = std::time::Instant::now();
        let _ = h.render_rect(w, r, 2.0);
        v.push(t.elapsed());
    }
    report("painting the page behind, the sunburst's size", v);
}

#[test]
#[ignore = "a measurement, not a check: KURA_BENCH_DIR names a real folder to draw"]
fn what_drawing_the_rings_costs() {
    use aizome_kit::sidestep_ui::{Appearance as Look, offscreen};
    use kura::ui::sunburst::Burst;
    let scan = match std::env::var_os("KURA_BENCH_DIR") {
        Some(d) => Arc::new(kura::scan::scan(ScanOptions::new(PathBuf::from(d)), kura::scan::Progress::new()).unwrap()),
        None => big_scan(),
    };
    eprintln!(
        "kura: {} nodes in {} bytes of arena ({:.1} bytes a node, names included)",
        scan.tree.len(),
        scan.tree.heap_bytes(),
        scan.tree.heap_bytes() as f64 / scan.tree.len() as f64
    );
    let segs = Rc::new(rings::segments(&scan.tree, NodeId::ROOT, RINGS));
    eprintln!("kura: {} segments", segs.len());
    let theme = aizome_kit::Theme::night();
    let burst = Burst {
        scan: scan.clone(),
        generation: 1,
        center: NodeId::ROOT,
        segs,
        outer: None,
        transition: None,
        progress: None,
        hover: None,
        selected: None,
        press: None,
        dragging: false,
        drag_over: false,
        can_up: false,
        origin: Rc::default(),
        zone: Rc::default(),
        labels: Rc::default(),
        marks: Rc::default(),
        highlight_gen: 0,
        hint: None,
        rings: Rc::default(),
        rings_gen: 0,
        ask: None,
    };
    let size = Size::new(730.0, 565.0);
    let bounds = aizome_kit::Rect::from_origin_size(Point::ZERO, size);
    let time = |what: &str, f: &dyn Fn(&mut aizome_kit::sidestep_ui::Canvas)| {
        let mut v = Vec::new();
        for _ in 0..30 {
            let t = std::time::Instant::now();
            let _ = offscreen::render(size, 2.0, Look::DARK, |c| f(c));
            v.push(t.elapsed());
        }
        report(what, v);
    };
    let surface = aizome_kit::theme::color(theme.palette.surface);
    time("nothing", &|_c| {});
    time("the surface", &|c| c.fill_rect(bounds, surface));
    time("surface and rings", &|c| {
        c.fill_rect(bounds, surface);
        burst.draw(c, bounds, &theme)
    });
    let cached = offscreen::render(size, 2.0, Look::DARK, |c| {
        c.fill_rect(bounds, surface);
        burst.draw(c, bounds, &theme)
    });
    time("a cached picture of them", &|c| c.draw_image(&cached, bounds));
    let g = Geometry::new(size, RINGS);
    let fills = |c: &mut aizome_kit::sidestep_ui::Canvas, aa: bool, pieces: bool| {
        c.set_antialias(aa);
        for s in burst.segs.iter() {
            let (ri, ro) = g.radii(f64::from(s.depth));
            let a0 = Geometry::angle(s.x0);
            let sweep = s.span() * std::f64::consts::TAU;
            let n = if pieces { (sweep / (64.0 / ro).min(0.35)).ceil().max(1.0) as usize } else { 1 };
            for k in 0..n {
                let seg = aizome_kit::sidestep_ui::kurbo::CircleSegment::new(
                    g.center,
                    ro,
                    ri,
                    a0 + sweep * k as f64 / n as f64,
                    sweep / n as f64,
                );
                c.fill(&seg, surface);
            }
        }
    };
    time("sectors whole", &|c| fills(c, true, false));
    time("sectors in pieces", &|c| fills(c, true, true));
    time("sectors whole, no antialiasing", &|c| fills(c, false, false));
    time("rects of the same count", &|c| {
        for (i, _) in burst.segs.iter().enumerate() {
            c.fill_rect(aizome_kit::Rect::new(0.0, (i % 50) as f64, 700.0, 500.0), surface);
        }
    });
}

#[test]
#[ignore = "a picture to look at: KURA_PICTURES names the folder"]
fn pictures_of_the_rings_moving() {
    let Some(dir) = std::env::var_os("KURA_PICTURES").map(PathBuf::from) else { return };
    let out = scratch("moving");
    let mut h = kura(&out, night());
    with_results(&mut h);
    let w = h.window();
    let p = point_on(&h, "work");
    h.click(w, p);
    for (i, at) in [20u64, 40, 60].into_iter().enumerate() {
        let t = h.program().transition.unwrap();
        h.program_mut().transition = Some(kura::ui::sunburst::Transition {
            started: std::time::Instant::now() - Duration::from_millis(at),
            duration: Duration::from_millis(80),
            ..t
        });
        h.send(Msg::Dismiss);
        h.save_png(w, dir.join(format!("moving-{i}.png")), 1.0);
    }
}

#[test]
fn the_keyboard_reaches_everything_in_the_list() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let out = scratch("keys");
    let mut h = kura(&out, night());
    with_results(&mut h);
    let w = h.window();
    assert_eq!(h.focused_name(w).as_deref(), Some("folder-list"), "the list has the keyboard after a scan");
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    let name = |h: &Harness<Kura>, n: Option<NodeId>| n.map(|n| h.program().tree().unwrap().name(n).into_owned());
    assert_eq!(name(&h, h.program().selected).as_deref(), Some("work"));
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    assert_eq!(name(&h, h.program().selected).as_deref(), Some("Videos"));
    if let Some(d) = std::env::var_os("KURA_PICTURES") {
        h.save_png(w, PathBuf::from(d).join("selected.png"), 1.0);
    }
    // ❖K collects it; the list keeps the keyboard.
    h.command(w, 'k');
    assert_eq!(h.program().collector.len(), 1);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    assert_eq!(name(&h, h.program().selected).as_deref(), Some("Pictures"));
    // Return goes in; ❖↑ comes back out, with the folder selected.
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(name(&h, Some(h.program().center)).as_deref(), Some("Pictures"));
    h.named(w, NamedKey::ArrowUp, Modifiers::SUPER);
    assert_eq!(h.program().center, NodeId::ROOT);
    assert_eq!(name(&h, h.program().selected).as_deref(), Some("Pictures"));
}

#[test]
fn a_row_collects_by_its_menu_or_by_dragging() {
    use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
    let out = scratch("rows");
    let mut h = kura(&out, night());
    with_results(&mut h);
    let w = h.window();
    let _ = h.render(w, 1.0);

    // Right click on a row: its menu, whose second item is Collect.
    let row = h.find(w, "Downloads").unwrap();
    h.press(w, row.center(), PointerButton::Right, 1);
    h.release(w, row.center(), PointerButton::Right);
    let popups = h.popups(w);
    assert_eq!(popups.len(), 1, "the row's menu");
    if let Some(d) = std::env::var_os("KURA_PICTURES") {
        h.save_png(w, PathBuf::from(d).join("row-menu.png"), 1.0);
    }
    let p = popups[0].0;
    for k in [NamedKey::ArrowDown, NamedKey::ArrowDown, NamedKey::Enter] {
        h.popup_keyboard(p, Key::Named(k), Modifiers::NONE);
    }
    let k = h.program();
    assert_eq!(k.collector.iter().map(|&n| k.tree().unwrap().name(n).into_owned()).collect::<Vec<_>>(), ["Downloads"]);

    // A row dragged onto the Collector.
    let zone = h.find_id(w, "collector").unwrap();
    let row = h.find(w, "Music").unwrap();
    h.press(w, row.center(), PointerButton::Left, 1);
    h.move_to(w, Point::new(row.center().x, row.center().y + 12.0));
    assert!(h.program().dragging.is_some());
    h.move_to(w, zone.center());
    assert!(h.program().drag_over);
    h.release(w, zone.center(), PointerButton::Left);
    let k = h.program();
    assert_eq!(k.collector.len(), 2);
    assert!(k.dragging.is_none());

    // Let go elsewhere: nothing collected.
    let row = h.find(w, "Documents").unwrap();
    h.press(w, row.center(), PointerButton::Left, 1);
    h.move_to(w, Point::new(row.center().x - 200.0, row.center().y));
    h.release(w, Point::new(row.center().x - 200.0, row.center().y), PointerButton::Left);
    assert_eq!(h.program().collector.len(), 2);
}

// Suggestions.

fn hp(path: &str, bytes: u64, note: Option<&str>) -> kura::hints::HintPath {
    kura::hints::HintPath {
        path: path.into(),
        allocated_bytes: bytes,
        apparent_bytes: bytes,
        modified: Some("2025-06-01T10:00:00Z".into()),
        note: note.map(Into::into),
        command: None,
    }
}

fn hint(
    id: &str,
    title: &str,
    summary: &str,
    risk: kura::hints::Risk,
    kind: kura::hints::ActionKind,
    paths: Vec<kura::hints::HintPath>,
) -> kura::hints::Hint {
    kura::hints::Hint {
        id: id.into(),
        title: title.into(),
        summary: summary.into(),
        reclaimable_bytes: paths.iter().map(|p| p.allocated_bytes).sum(),
        risk,
        action: kura::hints::Action { kind, command: None, description: "Run each path's command.".into() },
        paths,
        more_paths: 0,
        count: None,
        count_unit: None,
        groups: Vec::new(),
    }
}

/// Hints for the made-up home, as the analysis would find them.
fn fake_hints() -> Vec<kura::hints::Hint> {
    use kura::hints::{ActionKind as K, DuplicateGroup, Risk as R};
    let mut build = hint(
        "build-outputs",
        "Build outputs and dependencies",
        "Build tools make these again from the project's sources; the next build takes longer.",
        R::Low,
        K::Command,
        vec![hp("/home/sekka/work/aizome/target", 27 * GB, Some("Rust (Cargo) build output of ~/work/aizome"))],
    );
    build.paths[0].command = Some("cargo clean --manifest-path /home/sekka/work/aizome/Cargo.toml".into());
    let caches = hint(
        "package-caches",
        "Package managers' caches",
        "Downloads that pip and Nix keep so the next build needn't fetch them. They're fetched again when needed.",
        R::Safe,
        K::Command,
        vec![
            hp("/home/sekka/.cache/pip", 800 * MB, Some("pip's cache")),
            hp("/home/sekka/.cache/nix", 600 * MB, Some("Nix's evaluation and download caches")),
        ],
    );
    let browser = hint(
        "browser-caches",
        "Browser and app caches",
        "Web browsers keep copies of pages, scripts and media, and download them again as needed. Quit the browser first.",
        R::Low,
        K::Command,
        vec![hp("/home/sekka/.cache/google-chrome", 1900 * MB, Some("Chrome's cache"))],
    );
    let downloads = hint(
        "old-downloads",
        "Old downloads in /home/sekka/Downloads",
        "Files in Downloads not changed for 90 days. Installers and disk images are often done with, but these are yours: choose.",
        R::Medium,
        K::Trash,
        vec![
            hp("/home/sekka/Downloads/sekka-nightly.iso", 3400 * MB, Some("Disk images, untouched 4 months ago")),
            hp("/home/sekka/Downloads/old-backup.zip", 1200 * MB, Some("Archives, untouched 1 year ago")),
        ],
    );
    let mut dupes = hint(
        "duplicates",
        "Duplicate files",
        "1 set of files with identical contents, each 1 MB or larger. Keeping one copy of each would free the rest.",
        R::Medium,
        K::Trash,
        vec![hp("/home/sekka/Documents/scans/scan-001.jpg", 15 * MB, Some("a copy of ~/Pictures/2025/IMG-001.jpg"))],
    );
    dupes.count = Some(1);
    dupes.count_unit = Some("sets".into());
    dupes.groups = vec![DuplicateGroup {
        bytes_each: 15 * MB,
        hash: "6c1f0e0a9b3d4e2f8a7c5b1d0e9f8a7c".into(),
        verified: "full".into(),
        reclaimable_bytes: 15 * MB,
        paths: vec!["/home/sekka/Pictures/2025/IMG-001.jpg".into(), "/home/sekka/Documents/scans/scan-001.jpg".into()],
    }];
    let trash = hint(
        "trash",
        "Trash",
        "1 item already moved to the Trash still takes space until it's emptied. Emptying it can't be undone.",
        R::Medium,
        K::Command,
        vec![hp("/home/sekka/.local/share/Trash", 1100 * MB, None)],
    );
    let mut containers = hint(
        "containers",
        "Container storage",
        "Container images and layers. `podman system df` shows what's in use.",
        R::Medium,
        K::Command,
        vec![hp("/home/sekka/.local/share/containers", 4 * GB, Some("podman storage"))],
    );
    containers.action.command = Some("podman image prune".into());
    containers.action.description = "Ask before pruning: stopped containers and unused images go.".into();
    let mut gens = hint(
        "nix-generations",
        "Old Nix generations",
        "4 old generations are kept besides the current ones. Never delete store paths by hand: Yukimi's Storage removes old generations safely.",
        R::Low,
        K::OpenApp,
        Vec::new(),
    );
    gens.count = Some(4);
    gens.count_unit = Some("generations".into());
    vec![build, containers, downloads, browser, caches, trash, dupes, gens]
}

fn fake_analysis() -> kura::ui::Analysis {
    let hints = fake_hints();
    let mut by_risk = std::collections::BTreeMap::new();
    for h in &hints {
        *by_risk.entry(h.risk.id().to_owned()).or_insert(0) += h.reclaimable_bytes;
    }
    kura::ui::Analysis {
        json: Some(PathBuf::from("/home/sekka/.cache/aizome/kura/index.json")),
        reclaimable: hints.iter().map(|h| h.reclaimable_bytes).sum(),
        by_risk,
        hints: Arc::new(hints),
    }
}

fn hint_named(h: &Harness<Kura>, id: &str) -> usize {
    h.program().analysis.hints.iter().position(|x| x.id == id).unwrap_or_else(|| panic!("a {id} hint"))
}

#[test]
fn suggestions_list_the_hints_and_light_their_paths() {
    let out = scratch("suggest");
    let mut h = kura(&out, night());
    with_results(&mut h);
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t.ends_with("to free")), "the toolbar says what could be freed");
    h.send(Msg::ShowSide(kura::ui::Side::Suggestions));
    let texts = h.texts(w);
    for t in [
        "Build outputs and dependencies",
        "Old Nix generations",
        "Duplicate files",
        "podman image prune",
        "Empty Trash…",
    ] {
        assert!(texts.iter().any(|x| x == t), "{t} in {texts:?}");
    }

    // Picking build outputs lights the target folder and shows it.
    let b = hint_named(&h, "build-outputs");
    h.send(Msg::FocusHint(Some(b)));
    let k = h.program();
    let tree = k.tree().unwrap();
    assert_eq!(tree.name(k.center), "aizome", "the rings move to the folder holding it");
    assert_eq!(k.highlight.len(), 1);
    assert!(k.marks.contains(&kura::ui::sunburst::Mark::Lit));
    // Caches: pip's and Nix's lit, Chrome's (another hint's) faint.
    let c = hint_named(&h, "package-caches");
    h.send(Msg::FocusHint(Some(c)));
    let k = h.program();
    let tree = k.tree().unwrap();
    assert_eq!(tree.name(k.center), ".cache");
    assert_eq!(k.highlight.len(), 2);
    assert!(k.marks.contains(&kura::ui::sunburst::Mark::Lit));
    assert!(k.marks.contains(&kura::ui::sunburst::Mark::Dim));
    golden_settled(&mut h, w, golden("suggestions-night.png"), 2.0);
    h.set_appearance(day());
    golden_settled(&mut h, w, golden("suggestions-day.png"), 2.0);
    h.set_appearance(night());

    // Old downloads and duplicates opened: ticks, and copies side by side.
    let d = hint_named(&h, "old-downloads");
    let g = hint_named(&h, "duplicates");
    h.send(Msg::FocusHint(None));
    assert!(h.program().marks.is_empty());
    h.send(Msg::ExpandHint(d, true));
    let texts = h.texts(w);
    assert!(texts.iter().any(|x| x == "Tick what to move first"), "old downloads wait for the person's choice");
    assert!(texts.iter().any(|x| x == "sekka-nightly.iso"));
    h.send(Msg::Choose("/home/sekka/Downloads/sekka-nightly.iso".into(), true));
    assert!(h.texts(w).iter().any(|x| x == "Move 1 item to Trash…"));
    golden_settled(&mut h, w, golden("suggestions-downloads-night.png"), 2.0);
    // Duplicates, near the end: the copies side by side, one kept.
    h.send(Msg::ExpandHint(d, false));
    h.send(Msg::ExpandHint(g, true));
    let texts = h.texts(w);
    assert!(texts.iter().any(|x| x == "IMG-001.jpg") && texts.iter().any(|x| x == "scan-001.jpg"));
    let panel = h.find_id(w, "suggestions").unwrap();
    h.scroll(w, panel.center(), aizome_kit::Vec2::new(0.0, 5000.0));
    golden_settled(&mut h, w, golden("suggestions-duplicates-night.png"), 2.0);
    h.set_appearance(day());
    golden_settled(&mut h, w, golden("suggestions-duplicates-day.png"), 2.0);
    h.set_appearance(night());

    // Containers: the command, copied.
    h.send(Msg::CopyText("podman image prune".into()));
    assert_eq!(h.platform.clipboard.as_deref(), Some("podman image prune"));
    // Nothing waits for a yes yet.
    assert!(h.program().pending.is_none());
}

/// A home with one of each kind of hint, in a folder of the test's own.
fn suggest_home(out: &Path) -> (PathBuf, kura::hints::Context) {
    let h = out.join("home");
    let write = |p: &Path, n: usize| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, (0..n).map(|i| (i % 251) as u8).collect::<Vec<u8>>()).unwrap();
    };
    write(&h.join(".cache/someapp/blob"), 2_000_000);
    write(&h.join("proj/Cargo.toml"), 10);
    write(&h.join("proj/target/debug/big.rlib"), 3_000_000);
    write(&h.join("Downloads/old.iso"), 2_500_000);
    let t = std::time::SystemTime::now() - Duration::from_secs(400 * 86_400);
    std::fs::File::options().write(true).open(h.join("Downloads/old.iso")).unwrap().set_modified(t).unwrap();
    write(&h.join("Pictures/a.jpg"), 1_300_000);
    std::fs::create_dir_all(h.join("Documents")).unwrap();
    std::fs::copy(h.join("Pictures/a.jpg"), h.join("Documents/copy-of-a.jpg")).unwrap();
    write(&h.join(".local/share/Trash/files/gone.bin"), 1_400_000);
    write(&h.join(".local/share/Trash/info/gone.bin.trashinfo"), 60);
    let h = std::fs::canonicalize(&h).unwrap();
    let mut cx = kura::hints::Context::from_env();
    cx.home = Some(h.clone());
    cx.cache_home = Some(h.join(".cache"));
    cx.data_home = Some(h.join(".local/share"));
    cx.config_home = Some(h.join(".config"));
    cx.downloads = Some(h.join("Downloads"));
    cx.trash = Some(h.join(".local/share/Trash"));
    cx.nix_profiles = Vec::new();
    cx.nix_store = None;
    (h, cx)
}

fn settled(k: &Kura) -> bool {
    k.page == Page::Results && k.busy == 0 && matches!(k.index, kura::ui::IndexState::Written { .. })
}

#[test]
fn suggested_actions_ask_then_act_and_rescan_what_they_touched() {
    let out = scratch("suggest-act");
    let (home, places) = suggest_home(&out);
    let mut k = Kura::sandboxed(&out);
    k.has_store = false;
    k.index_dir = None;
    k.trash_dir = Some(home.join(".local/share/Trash"));
    k.places = Some(places);
    let mut h = Harness::new(k);
    h.send(Msg::Scan(home.clone()));
    settle(&mut h, settled);
    let ids: Vec<String> = h.program().analysis.hints.iter().map(|x| x.id.clone()).collect();
    for id in ["caches", "build-outputs", "old-downloads", "duplicates", "trash"] {
        assert!(ids.iter().any(|x| x == id), "{id} in {ids:?}");
    }
    let trash_before = {
        let k = h.program();
        let t = k.tree().unwrap();
        t.node(t.find(&home.join(".local/share/Trash")).unwrap()).alloc
    };

    // Build outputs: asked first; nothing moves until the yes.
    let b = hint_named(&h, "build-outputs");
    h.send(Msg::Act(b));
    assert!(
        matches!(&h.program().pending, Some(kura::ui::Pending::Trash { paths, .. }) if paths == &[home.join("proj/target")])
    );
    assert!(home.join("proj/target").exists());
    // "Cancel" leaves it.
    h.send(Msg::CancelPending);
    assert!(h.program().pending.is_none());
    h.send(Msg::Act(b));
    h.send(Msg::Confirm);
    settle(&mut h, settled);
    assert!(!home.join("proj/target").exists());
    assert!(home.join(".local/share/Trash/files/target/debug/big.rlib").exists());
    {
        let k = h.program();
        let t = k.tree().unwrap();
        let gone = t.gone();
        assert!(t.find(&home.join("proj/target")).is_none_or(|n| gone[n.index()]));
        // The Trash, inside the scan, was scanned again: it holds the target now.
        let trash = t.find(&home.join(".local/share/Trash")).unwrap();
        assert!(t.node(trash).alloc >= trash_before + 3_000_000, "{} after {}", t.node(trash).alloc, trash_before);
        assert!(t.find(&home.join(".local/share/Trash/files/target/debug/big.rlib")).is_some());
        assert!(!k.analysis.hints.iter().any(|x| x.id == "build-outputs"), "the hint went with what it named");
        assert!(k.notice.as_deref().is_some_and(|n| n.starts_with("Moved 1 item to the Trash")));
    }

    // Old downloads are the person's: nothing is ticked until they choose.
    let d = hint_named(&h, "old-downloads");
    h.send(Msg::Act(d));
    assert!(h.program().pending.is_none());
    h.send(Msg::Choose(home.join("Downloads/old.iso").display().to_string(), true));
    h.send(Msg::Act(d));
    h.send(Msg::Confirm);
    settle(&mut h, settled);
    assert!(!home.join("Downloads/old.iso").exists());

    // Duplicates: keep the second copy, the first goes.
    let g = hint_named(&h, "duplicates");
    let set = h.program().analysis.hints[g].groups[0].clone();
    h.send(Msg::Keep(set.hash.clone(), 1));
    h.send(Msg::ActGroup(g, 0));
    h.send(Msg::Confirm);
    settle(&mut h, settled);
    assert!(!Path::new(&set.paths[0]).exists());
    assert!(Path::new(&set.paths[1]).exists());

    // Emptying the Trash: for good, after a yes, and only the Trash.
    let t = hint_named(&h, "trash");
    h.send(Msg::Act(t));
    assert_eq!(h.program().pending, Some(kura::ui::Pending::EmptyTrash));
    h.send(Msg::Confirm);
    settle(&mut h, settled);
    assert_eq!(std::fs::read_dir(home.join(".local/share/Trash/files")).unwrap().count(), 0);
    assert!(home.join("Pictures").exists() && home.join("proj/Cargo.toml").exists());
    let k = h.program();
    let t = k.tree().unwrap();
    let trash = t.find(&home.join(".local/share/Trash")).unwrap();
    assert!(t.node(trash).alloc < 100_000, "the Trash's size was taken again: {}", t.node(trash).alloc);
    assert!(!k.analysis.hints.iter().any(|x| x.id == "trash"));
    assert!(k.notice.as_deref().is_some_and(|n| n.starts_with("Emptied the Trash")));
}

#[test]
fn suggestions_fit_the_frame_budget() {
    let out = scratch("perf-suggest");
    let mut h = kura(&out, night());
    with_results(&mut h);
    let w = h.window();
    h.send(Msg::ShowSide(kura::ui::Side::Suggestions));
    let _ = h.render(w, 1.0);
    let n = h.program().analysis.hints.len();
    let mut pick = Vec::new();
    let mut open = Vec::new();
    let mut tick = Vec::new();
    for round in 0..4 {
        for i in 0..n {
            if let Some(d) = frame(&mut h, |h| h.send(Msg::FocusHint(Some(i)))) {
                pick.push(d);
            }
            if let Some(d) = frame(&mut h, |h| h.send(Msg::ExpandHint(i, round % 2 == 0))) {
                open.push(d);
            }
        }
        let d = hint_named(&h, "old-downloads");
        let p = h.program().analysis.hints[d].paths[0].path.clone();
        if let Some(t) = frame(&mut h, |h| h.send(Msg::Choose(p.clone(), round % 2 == 0))) {
            tick.push(t);
        }
    }
    let pick50 = report("picking a hint (the rings move and dim)", pick);
    let open50 = report("opening or closing a hint", open);
    report("ticking a path", tick);
    if !cfg!(debug_assertions) {
        assert!(open50 < Duration::from_millis(4), "opening p50 {open50:?}");
        let _ = pick50;
    }
}
