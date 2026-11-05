//! HAKO-08: Put Back of a trashed folder merged into a folder now at its
//! place, with a conflicting child skipped, leaves that child in the
//! Trash. Its entry must keep knowing where it came from (its
//! `.trashinfo`), so it can still be put back; it must not become an
//! orphan "of unknown origin".

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

use hako::fsops::transfer::{restore, trash_items};
use hako::fsops::trash::Trash;
use hako::fsops::{Asker, Choice, Conflict, Control, Env, ErrorChoice, Failure, Resolution};

struct Tmp(PathBuf);

impl Tmp {
    fn new(name: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!("hako-repro-08-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(fs::canonicalize(&p).unwrap())
    }
    fn file(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Answers from a script.
struct Script(VecDeque<Resolution>, Vec<String>);

impl Asker for Script {
    fn conflict(&mut self, c: &Conflict) -> Choice {
        self.1.push(c.target.file_name().unwrap().to_string_lossy().into_owned());
        self.0.pop_front().expect("an answer").into()
    }
    fn error(&mut self, _: &Failure) -> ErrorChoice {
        ErrorChoice::Skip
    }
}

#[test]
fn a_merged_put_back_that_skipped_a_child_keeps_its_origin() {
    let t = Tmp::new("merge");
    let trash = Trash {
        home: t.0.join("data/Trash"),
        home_dir: Some(t.0.join("home")),
        uid: unsafe { libc::getuid() },
        topdirs: Some(Vec::new()),
    };
    t.file("home/f/keep.txt", "keep");
    t.file("home/f/clash.txt", "old clash");
    let f = t.0.join("home/f");
    let control = Control::new();
    let mut quiet = Script(VecDeque::new(), Vec::new());
    let (r, _) = trash_items(std::slice::from_ref(&f), &trash, &mut Env::new(&control, &mut quiet, &mut ()));
    assert!(r.is_clean(), "{r:?}");
    // A new f with a clash.txt of its own.
    t.file("home/f/clash.txt", "new clash");
    // Put Back: merge the folders, skip the conflicting child.
    let mut s = Script([Resolution::Merge, Resolution::Skip].into(), Vec::new());
    let entries = trash.list();
    let r = restore(&entries, &trash, &mut Env::new(&control, &mut s, &mut ()));
    eprintln!("asked about {:?}; report {r:?}", s.1);
    assert_eq!(fs::read_to_string(f.join("keep.txt")).unwrap(), "keep");
    assert_eq!(fs::read_to_string(f.join("clash.txt")).unwrap(), "new clash");
    // The skipped child is still in the Trash...
    let left = trash.list();
    eprintln!(
        "the Trash now: {:?}",
        left.iter().map(|e| (e.file.clone(), e.info.clone(), e.original.clone())).collect::<Vec<_>>()
    );
    let entry = left.iter().find(|e| e.file.join("clash.txt").exists()).expect("the skipped child stays in the Trash");
    // ...and must still know where it came from.
    assert_eq!(
        entry.original.as_deref(),
        Some(f.as_path()),
        "the folder left in the Trash lost its .trashinfo: it lists as of unknown origin"
    );
    // So it can be put back later (here, by keeping both).
    let mut later = Script([Resolution::Merge, Resolution::KeepBoth].into(), Vec::new());
    let r = restore(std::slice::from_ref(entry), &trash, &mut Env::new(&control, &mut later, &mut ()));
    assert!(r.failed.is_empty(), "a later Put Back works: {r:?}");
}
