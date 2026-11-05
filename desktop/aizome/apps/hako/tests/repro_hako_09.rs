//! HAKO-09: an undo that fails (the name it would rename back to is
//! taken now) must not lose its step: ⌘Z again must not silently undo
//! the step before it instead.

mod common;

use std::path::PathBuf;

use aizome_kit::theme::ThemeChoice;
use common::*;
use hako::ui::Msg;

fn sandbox(name: &str) -> Sandbox {
    let dir = std::env::temp_dir().join(format!("hako-repro-09-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Sandbox(std::fs::canonicalize(&dir).unwrap())
}

fn names(dir: &PathBuf) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn a_failed_undo_keeps_its_step() {
    let s = sandbox("undo");
    let docs = s.dir("home/docs");
    s.file("home/docs/a.txt", "a");
    let mut h = harness(&s, ThemeChoice::Night);
    let w = h.window();
    h.focus_window(w, true);
    // Step 1: Duplicate a.txt.
    h.send(Msg::GoPath(w, docs.join("a.txt")));
    settle(&mut h);
    h.send(Msg::Duplicate(w));
    settle(&mut h);
    assert_eq!(h.program().history.undo_label(), Some("Duplicate"));
    let copy = names(&docs).into_iter().find(|n| n.contains("copy")).expect("a copy made");
    // Step 2: rename a.txt to b.txt.
    h.send(Msg::GoPath(w, docs.join("a.txt")));
    settle(&mut h);
    h.send(Msg::StartRename(w));
    h.send(Msg::RenameText(w, "b.txt".into()));
    h.send(Msg::RenameCommit(w));
    settle(&mut h);
    assert_eq!(h.program().history.undo_label(), Some("Rename"));
    // Something else takes the old name.
    s.file("home/docs/a.txt", "newer");
    // ⌘Z: renaming back fails ("a.txt is taken now").
    h.send(Msg::Undo);
    settle(&mut h);
    eprintln!(
        "after the failed undo: undo {:?}, redo {:?}, folder {:?}",
        h.program().history.undo_label(),
        h.program().history.redo_label(),
        names(&docs)
    );
    assert!(docs.join("b.txt").exists(), "the rename wasn't undone");
    let label = h.program().history.undo_label().map(str::to_owned);
    // ⌘Z again must not undo the Duplicate behind the person's back.
    h.send(Msg::Undo);
    settle(&mut h);
    let copy_kept = docs.join(&copy).exists();
    eprintln!(
        "after a second ⌘Z: folder {:?}, items in the Trash {}",
        names(&docs),
        h.program().opts.trash.list().len()
    );
    assert_eq!(
        label.as_deref(),
        Some("Rename"),
        "the step that failed to undo is gone from the history; the next ⌘Z then {}",
        if copy_kept { "kept the copy" } else { "trashed the Duplicate's copy" }
    );
    assert!(copy_kept, "a second ⌘Z undid the step before the failed one: {:?}", names(&docs));
}
