use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;

/// A tree to index, each folder's time set an hour back, as a tree left
/// alone is (a folder changed this instant is listed again: racy).
struct Tree {
    root: PathBuf,
    db: PathBuf,
}

impl Tree {
    fn new(name: &str) -> Tree {
        let base = std::env::temp_dir().join(format!("aizome-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("games");
        for d in ["snes", "psx/Disc Game", ".hidden", "node_modules", "cache"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for (f, body) in [
            ("snes/Kirby (USA).sfc", "kirby"),
            ("snes/Mario (USA).sfc", "mario"),
            ("psx/Disc Game/Disc Game (USA).cue", "cue"),
            ("psx/Disc Game/Disc Game (USA).bin", "bin"),
            ("readme.txt", "hello"),
            (".hidden/secret.sfc", "no"),
            ("node_modules/x.js", "no"),
            ("cache/CACHEDIR.TAG", "Signature: 8a477f597d28d172789f06886806bc55"),
            ("cache/junk.bin", "no"),
        ] {
            std::fs::write(root.join(f), body).unwrap();
        }
        let t = Tree { root, db: base.join("index.sqlite") };
        t.settle();
        t
    }

    /// Every folder's time an hour back.
    fn settle(&self) {
        fn walk(d: &Path, when: SystemTime) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    walk(&e.path(), when);
                }
            }
            std::fs::File::open(d).unwrap().set_modified(when).unwrap();
        }
        walk(&self.root, SystemTime::now() - Duration::from_secs(3600));
    }

    fn names(&self, index: &Index) -> Vec<String> {
        let mut v: Vec<String> = index
            .entries(&self.root)
            .unwrap()
            .into_iter()
            .map(|e| e.path.strip_prefix(self.root.canonicalize().unwrap()).unwrap().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.parent().unwrap());
    }
}

#[test]
fn a_second_look_lists_only_what_changed() {
    let t = Tree::new("second");
    let mut index = Index::open_at(&t.db).unwrap();
    let first = index.refresh(&t.root, &Refresh::default()).unwrap();
    // The root, snes, psx, Disc Game and cache (listed, its contents not
    // kept); not .hidden or node_modules.
    assert_eq!((first.folders, first.listed), (5, 5), "{first:?}");
    assert_eq!(
        t.names(&index),
        [
            "cache",
            "psx",
            "psx/Disc Game",
            "psx/Disc Game/Disc Game (USA).bin",
            "psx/Disc Game/Disc Game (USA).cue",
            "readme.txt",
            "snes",
            "snes/Kirby (USA).sfc",
            "snes/Mario (USA).sfc",
        ]
    );
    let g = index.generation(&t.root).unwrap();
    assert!(g > 0);

    // Nothing changed: every folder looked at, none listed.
    let again = index.refresh(&t.root, &Refresh::default()).unwrap();
    assert_eq!((again.folders, again.listed), (5, 0), "{again:?}");
    assert!(!again.any());
    assert_eq!(index.generation(&t.root).unwrap(), g);

    // A game added deep down: only its folder is listed, though the
    // folders above it didn't change.
    std::fs::write(t.root.join("psx/Disc Game/Disc Game (USA) (Track 2).bin"), "t2").unwrap();
    // (Its time put ahead, so it's surely within a tick of when it's listed.)
    let ahead = SystemTime::now() + Duration::from_secs(10);
    std::fs::File::open(t.root.join("psx/Disc Game")).unwrap().set_modified(ahead).unwrap();
    let added = index.refresh(&t.root, &Refresh::default()).unwrap();
    assert_eq!((added.listed, added.added, added.removed), (1, 1, 0), "{added:?}");
    assert!(index.generation(&t.root).unwrap() > g);
    // Listed in the same tick as its change: listed again next time, as
    // a change hidden behind an unchanged time could be.
    let racy = index.refresh(&t.root, &Refresh::default()).unwrap();
    assert_eq!((racy.listed, racy.added), (1, 0), "{racy:?}");

    // One gone, and a folder renamed: what was in it moves with it.
    std::fs::remove_file(t.root.join("snes/Mario (USA).sfc")).unwrap();
    std::fs::rename(t.root.join("psx/Disc Game"), t.root.join("psx/Disc Game!")).unwrap();
    let moved = index.refresh(&t.root, &Refresh::default()).unwrap();
    assert_eq!((moved.added, moved.removed), (4, 2), "{moved:?}");
    let names = t.names(&index);
    assert!(names.contains(&"psx/Disc Game!/Disc Game (USA).cue".to_owned()), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("psx/Disc Game/") || n.contains("Mario")), "{names:?}");
}

#[test]
fn sizes_are_read_when_asked_and_a_file_changed_in_place_is_seen() {
    let t = Tree::new("stat");
    let mut index = Index::open_at(&t.db).unwrap();
    index.refresh(&t.root, &Refresh::default()).unwrap();
    assert!(index.entries(&t.root).unwrap().iter().all(|e| e.size.is_none()), "names only");
    let stat = Refresh { stat: true, ..Refresh::default() };
    let first = index.refresh(&t.root, &stat).unwrap();
    assert_eq!((first.listed, first.changed), (0, 0), "the listing kept; sizes read for the first time: {first:?}");
    let kirby = t.root.canonicalize().unwrap().join("snes/Kirby (USA).sfc");
    let size = |index: &Index| index.entries(&t.root).unwrap().into_iter().find(|e| e.path == kirby).unwrap().size;
    assert_eq!(size(&index), Some(5));
    // A ROM rewritten: its folder's time stays, its own changes.
    std::fs::write(&kirby, "kirby, patched").unwrap();
    let changed = index.refresh(&t.root, &stat).unwrap();
    assert_eq!((changed.listed, changed.changed), (0, 1), "{changed:?}");
    assert_eq!(size(&index), Some(14));
}

#[test]
fn facts_hold_while_their_file_does() {
    let t = Tree::new("facts");
    let mut index = Index::open_at(&t.db).unwrap();
    index.refresh(&t.root, &Refresh::default()).unwrap();
    let rom = t.root.join("snes/Kirby (USA).sfc");
    assert_eq!(index.fact(&rom, "crc32").unwrap(), None);
    index.set_fact(&rom, "crc32", b"1234abcd").unwrap();
    assert_eq!(index.fact(&rom, "crc32").unwrap().as_deref(), Some(&b"1234abcd"[..]));
    // Rewritten (another size): forgotten.
    std::fs::write(&rom, "kirby, patched").unwrap();
    assert_eq!(index.fact(&rom, "crc32").unwrap(), None);
    // A file not yet indexed is indexed to keep one.
    let other = t.root.join("snes/new.sfc");
    std::fs::write(&other, "n").unwrap();
    index.set_fact(&other, "crc32", b"0").unwrap();
    assert_eq!(index.fact(&other, "crc32").unwrap().as_deref(), Some(&b"0"[..]));
}

#[test]
fn apps_share_the_index() {
    let t = Tree::new("shared");
    // Two connections, as two apps would have: what one read, the other
    // knows, and its look is a second look.
    let mut a = Index::open_at(&t.db).unwrap();
    let mut b = Index::open_at(&t.db).unwrap();
    a.refresh(&t.root, &Refresh::default()).unwrap();
    assert_eq!(t.names(&a), t.names(&b));
    let second = b.refresh(&t.root, &Refresh::default()).unwrap();
    assert_eq!(second.listed, 0, "{second:?}");
    // A part of the tree refreshed by itself is the same rows.
    let part = a.refresh(&t.root.join("snes"), &Refresh::default()).unwrap();
    assert_eq!((part.folders, part.listed), (1, 0), "{part:?}");
    assert_eq!(a.entries(&t.root.join("snes")).unwrap().len(), 2);
}

#[test]
fn a_refresh_stops_when_asked_and_keeps_what_it_read() {
    let t = Tree::new("stop");
    let mut index = Index::open_at(&t.db).unwrap();
    let stop = AtomicBool::new(true);
    let r = index.refresh(&t.root, &Refresh { stop: Some(&stop), ..Refresh::default() }).unwrap();
    assert!(r.stopped && r.folders == 0, "{r:?}");
    // A part only: kept, and found next time.
    let part = index.refresh(&t.root.join("snes"), &Refresh::default()).unwrap();
    assert_eq!(part.added, 2);
    assert_eq!(index.entries(&t.root).unwrap().len(), 3, "only what was read: snes and its two");
}
