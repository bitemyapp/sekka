use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::theme::ThemeChoice;
mod common;
use common::*;

#[test]
fn close_alert_escape_cancels_without_saving() {
    let mut r = rig("modal-escape", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    let p = r.dir.join("IMG_2.jpg");
    let before = std::fs::read(&p).unwrap();
    r.h.focus_window(w, true);
    r.h.command(w, 'r');
    r.h.command(w, 'q');
    assert!(r.h.texts(w).iter().any(|t| t == "Save a turned picture?"));
    r.h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(!r.h.texts(w).iter().any(|t| t == "Save a turned picture?"));
    assert!(!r.h.platform.quit);
    assert_eq!(std::fs::read(&p).unwrap(), before);
    assert!(!r.h.program().turns.is_empty());
}

#[test]
fn close_alert_blocks_background_save() {
    let mut r = rig("modal-command", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    let p = r.dir.join("IMG_2.jpg");
    let before = std::fs::read(&p).unwrap();
    r.h.focus_window(w, true);
    r.h.command(w, 'r');
    r.h.command(w, 'q');
    r.h.command(w, 's');
    settle(&mut r, 1.0);
    assert!(r.h.texts(w).iter().any(|t| t == "Save a turned picture?"));
    assert_eq!(std::fs::read(&p).unwrap(), before, "Save behind the modal changed the JPEG");
    assert!(!r.h.program().turns.is_empty());
}
