//! SYC-07: what an input method was composing in a field goes when the
//! field loses the focus or the program sets its text. Before, it was kept
//! and drawn again (in clear, in a password field) when the field next had
//! the focus, or beside the text the program set.
//!
//! These build on the field before and after it was made a typestate, so
//! they run against both.

mod support;

use support::{M, click, differ, focused};

#[test]
fn losing_the_focus_drops_what_was_being_composed() {
    let (mut h, w) = focused("pa", true);
    h.ime(w, None, "WW");
    let composing = h.render_rgba(w, 2.0);
    click(&mut h, w, "other");
    click(&mut h, w, "password");
    let back = h.render_rgba(w, 2.0);
    let (mut fresh, fw) = focused("pa", true);
    let clean = fresh.render_rgba(fw, 2.0);
    eprintln!("composing vs clean: {}; back vs clean: {}", differ(&composing, &clean), differ(&back, &clean));
    assert!(differ(&composing, &clean) > 0, "the composition shows while it's being composed");
    assert_eq!(differ(&back, &clean), 0, "back in the field, nothing of the old composition shows");
}

#[test]
fn the_programs_text_drops_what_was_being_composed() {
    let (mut h, w) = focused("pa", true);
    h.ime(w, None, "WW");
    // A wrong password: the lock screen clears the field.
    h.send(M::Set(String::new()));
    let cleared = h.render_rgba(w, 2.0);
    let (mut fresh, fw) = focused("", true);
    let clean = fresh.render_rgba(fw, 2.0);
    eprintln!("cleared vs clean: {}", differ(&cleared, &clean));
    assert_eq!(differ(&cleared, &clean), 0, "the cleared field shows nothing of the composition");
}
