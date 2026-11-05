//! HAKO-05: Put Back (and Undo of Move to Trash) answered "Replace" at a
//! conflict must move the item that was there to the Trash, as the
//! conflict sheet promises ("Replacing moves the one there to the
//! Trash"), not delete it for good.

use std::fs;
use std::path::{Path, PathBuf};

use hako::fsops::transfer::{restore, trash_items};
use hako::fsops::trash::Trash;
use hako::fsops::undo::{History, Record, Step, invert};
use hako::fsops::{Asker, Choice, Conflict, Control, Env, ErrorChoice, Failure, ReplaceAll, Replacing};

/// A folder of the test's own under the private TMPDIR, removed when dropped.
struct Tmp(PathBuf);

impl Tmp {
    fn new(name: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!("hako-repro-05-{name}-{}", std::process::id()));
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

/// Replace at every conflict, as a person who pressed Replace with "Apply
/// to all" (the kit's test agreement stands for their press).
struct Replacer(ReplaceAll);

impl Asker for Replacer {
    fn conflict(&mut self, c: &Conflict) -> Choice {
        Choice::Replace(self.0.consent(c))
    }
    fn error(&mut self, _: &Failure) -> ErrorChoice {
        ErrorChoice::Skip
    }
}

fn replace_all() -> Replacer {
    let agreed = aizome_kit::agree::testing::agreed(Replacing { job: 1, target: PathBuf::new(), all: true });
    Replacer(ReplaceAll::new(agreed, 1).expect("an \"Apply to all\" Replace for job 1"))
}

/// A Trash entirely inside the test folder (no mount points looked at).
fn trash_in(t: &Tmp) -> Trash {
    Trash {
        home: t.0.join("data/Trash"),
        home_dir: Some(t.0.join("home")),
        uid: unsafe { libc::getuid() },
        topdirs: Some(Vec::new()),
    }
}

/// Whether some item in the Trash came from `original` and reads `text`.
fn in_trash(trash: &Trash, original: &Path, text: &str) -> bool {
    trash
        .list()
        .iter()
        .any(|e| e.original.as_deref() == Some(original) && fs::read_to_string(&e.file).ok().as_deref() == Some(text))
}

#[test]
fn put_back_replace_moves_the_newer_item_to_the_trash() {
    let t = Tmp::new("putback");
    let trash = trash_in(&t);
    let a = t.file("home/docs/a.txt", "old a");
    let control = Control::new();
    let mut asker = replace_all();
    let (r, trashed) = trash_items(std::slice::from_ref(&a), &trash, &mut Env::new(&control, &mut asker, &mut ()));
    assert!(r.is_clean() && trashed.len() == 1, "{r:?}");
    // Something new takes the name meanwhile.
    t.file("home/docs/a.txt", "newer a");
    // Put Back, answering the conflict with Replace.
    let entries = trash.list();
    let r = restore(&entries, &trash, &mut Env::new(&control, &mut asker, &mut ()));
    eprintln!("restore report: {r:?}");
    assert_eq!(fs::read_to_string(&a).unwrap(), "old a", "the trashed item is back");
    assert!(
        in_trash(&trash, &a, "newer a"),
        "the item it replaced should be in the Trash (the sheet says so); the Trash holds {:?}",
        trash.list().iter().map(|e| (e.file.clone(), e.original.clone())).collect::<Vec<_>>()
    );
}

#[test]
fn undo_of_move_to_trash_replace_moves_the_newer_item_to_the_trash() {
    let t = Tmp::new("undo");
    let trash = trash_in(&t);
    let a = t.file("home/docs/a.txt", "old a");
    let control = Control::new();
    let mut asker = replace_all();
    let (_, items) = trash_items(std::slice::from_ref(&a), &trash, &mut Env::new(&control, &mut asker, &mut ()));
    t.file("home/docs/a.txt", "newer a");
    // ⌘Z of the Move to Trash: the inverse is a restore.
    let mut history = History::default();
    history.push(Step { record: Record::Trash { items }, label: "Move to Trash of “a.txt”".into() });
    let inversion = history.begin_undo().expect("the Move to Trash to undo");
    let (r, inverted) = invert(inversion, &trash, &mut Env::new(&control, &mut asker, &mut ()));
    history.finished(inverted, |note| panic!("nothing is left out: {note}"));
    eprintln!("undo report: {r:?}");
    assert_eq!(fs::read_to_string(&a).unwrap(), "old a");
    assert!(in_trash(&trash, &a, "newer a"), "the item undo replaced should be in the Trash, not deleted");
}
