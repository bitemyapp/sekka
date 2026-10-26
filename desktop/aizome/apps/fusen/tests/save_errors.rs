//! Fault-injected filesystem failures preserve drafts and block normal exit.
use aizome_fusen::app::{Fusen, Msg, Places, Request};
use aizome_kit::WindowKey;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fixture(name: &str) -> (PathBuf, Harness<Fusen>, WindowKey) {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fusen-save-errors").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut h = Harness::new(Fusen::new(Places { notes: d.join("Notes"), ..Places::default() }, Request::default()));
    let w = h.program().main.unwrap();
    h.focus_window(w, true);
    (d, h, w)
}
fn draft(h: &mut Harness<Fusen>, w: WindowKey, text: &str) -> u64 {
    h.command(w, 'n');
    h.type_text(w, text);
    h.program().selected.unwrap()
}
fn failed(h: &Harness<Fusen>, w: WindowKey) -> bool {
    h.texts(w).iter().any(|s| s.contains("couldn't be saved") || s == "Not saved")
}

#[test]
fn failed_directory_creation_keeps_dirty_and_shows_retry() {
    let (d, mut h, w) = fixture("mkdir");
    std::fs::write(d.join("Notes"), "file blocks directory").unwrap();
    let id = draft(&mut h, w, "Draft\nKeep this text");
    h.advance(Duration::from_secs(1));
    h.save_png(w, d.join("failed.png"), 1.0);
    assert!(h.program().open[&id].dirty, "failed mkdir cannot mark a draft saved");
    assert!(failed(&h, w), "save error must be visible");
    std::fs::remove_file(d.join("Notes")).unwrap();
    assert!(h.click_text(w, "Retry"));
    assert!(!h.program().open[&id].dirty);
    assert!(!failed(&h, w));
    assert_eq!(std::fs::read_to_string(d.join("Notes/Draft.md")).unwrap(), "Draft\nKeep this text");
    h.save_png(w, d.join("recovered.png"), 1.0);
}

#[test]
fn existing_note_failure_survives_rescan_and_blocks_close_and_quit() {
    let (d, mut h, w) = fixture("existing");
    let id = draft(&mut h, w, "Existing\nOriginal");
    h.advance(Duration::from_secs(1));
    std::fs::rename(d.join("Notes"), d.join("Stored")).unwrap();
    std::fs::write(d.join("Notes"), "blocked").unwrap();
    h.type_text(w, " plus unsaved");
    h.advance(Duration::from_secs(4));
    assert!(h.program().open.contains_key(&id), "rescan must keep an unreadable dirty note");
    assert!(h.program().open[&id].dirty);
    h.request_close(w);
    assert!(h.windows().contains(&w), "native Close must not lose draft");
    assert!(h.click_text(w, "Keep Open"));
    h.command(w, 'w');
    assert!(h.windows().contains(&w));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    h.command(w, 'q');
    assert!(!h.platform.quit, "Quit must not lose draft");
    h.save_png(w, d.join("quit-blocked.png"), 1.0);
    std::fs::remove_file(d.join("Notes")).unwrap();
    std::fs::rename(d.join("Stored"), d.join("Notes")).unwrap();
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.platform.quit);
    assert_eq!(std::fs::read_to_string(d.join("Notes/Existing.md")).unwrap(), "Existing\nOriginal plus unsaved");
}

#[test]
fn failed_atomic_write_keeps_bytes_and_sticky_close_can_retry() {
    let (d, mut h, w) = fixture("sticky");
    let id = draft(&mut h, w, "Sticky\nOriginal");
    h.advance(Duration::from_secs(1));
    h.send(Msg::Sticky(Some(id)));
    let s = *h.program().stickies.keys().next().unwrap();
    h.focus_window(s, true);
    // A directory at the target is a deterministic atomic rename failure,
    // including when the tests run as root. Preserve original bytes elsewhere.
    std::fs::rename(d.join("Notes/Sticky.md"), d.join("original.md")).unwrap();
    std::fs::create_dir(d.join("Notes/Sticky.md")).unwrap();
    h.send(Msg::Edit(id, aizome_kit::EditorAction::Insert(" updated".into())));
    h.advance(Duration::from_secs(1));
    assert!(h.program().open[&id].dirty);
    assert!(failed(&h, s));
    h.request_close(s);
    assert!(h.program().stickies.contains_key(&s));
    h.save_png(s, d.join("sticky-blocked.png"), 1.0);
    std::fs::remove_dir(d.join("Notes/Sticky.md")).unwrap();
    h.named(s, NamedKey::Enter, Modifiers::NONE);
    assert!(!h.program().stickies.contains_key(&s));
    assert!(h.windows().contains(&w));
    assert_eq!(std::fs::read_to_string(d.join("Notes/Sticky.md")).unwrap(), "Sticky\nOriginal updated");
}

#[test]
fn offscreen_failed_note_blocks_exit_but_saved_notes_are_unchanged() {
    let (d, mut h, w) = fixture("multiple");
    let good = draft(&mut h, w, "Saved\nUnchanged");
    h.advance(Duration::from_secs(1));
    std::fs::write(d.join("Notes/Blocked"), "not a folder").unwrap();
    h.send(Msg::Folder(Some("Blocked".into())));
    let bad = draft(&mut h, w, "Unwritten\nRecover me");
    h.advance(Duration::from_secs(1));
    h.send(Msg::Folder(None));
    h.send(Msg::Select(Some(good)));
    assert!(failed(&h, w), "an error must stay visible with another note selected");
    h.command(w, 'w');
    assert!(h.windows().contains(&w));
    assert!(h.program().open[&bad].dirty);
    assert!(h.click_text(w, "Keep Open"));
    assert_eq!(std::fs::read_to_string(d.join("Notes/Saved.md")).unwrap(), "Saved\nUnchanged");
    std::fs::remove_file(d.join("Notes/Blocked")).unwrap();
    assert!(h.click_text(w, "Retry"));
    h.command(w, 'w');
    assert!(!h.windows().contains(&w));
    assert_eq!(std::fs::read_to_string(d.join("Notes/Blocked/Unwritten.md")).unwrap(), "Unwritten\nRecover me");
}

#[test]
fn minimum_sticky_keeps_retry_and_editor_reachable() {
    let (d, mut h, w) = fixture("minimum-sticky");
    let id = draft(&mut h, w, "Small\nOriginal");
    h.advance(Duration::from_secs(1));
    h.send(Msg::Sticky(Some(id)));
    let sticky = *h.program().stickies.keys().next().unwrap();
    h.focus_window(sticky, true);
    h.resize(sticky, aizome_kit::Size::new(160.0, 100.0));
    std::fs::rename(d.join("Notes/Small.md"), d.join("original.md")).unwrap();
    std::fs::create_dir(d.join("Notes/Small.md")).unwrap();
    h.send(Msg::Edit(id, aizome_kit::EditorAction::Insert(" edited".into())));
    h.advance(Duration::from_secs(1));
    h.save_png(sticky, d.join("minimum-error.png"), 1.0);
    let retry = h.find(sticky, "Retry").expect("Retry remains visible at the minimum size");
    assert!(retry.y0 >= 0.0 && retry.y1 <= 100.0);
    let editor = h.find_id(sticky, "sticky-editor").unwrap();
    assert!(editor.height() >= 24.0 && editor.y1 <= 100.0, "editor remains usable: {editor:?}");
    std::fs::remove_dir(d.join("Notes/Small.md")).unwrap();
    assert!(h.click_text(sticky, "Retry"));
    assert!(!h.program().open[&id].dirty);
    assert_eq!(std::fs::read_to_string(d.join("Notes/Small.md")).unwrap(), "Small\nOriginal edited");
}
