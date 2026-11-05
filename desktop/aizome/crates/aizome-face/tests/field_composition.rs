//! SYC-07: what an input method was composing in a field goes when the
//! field loses the focus, the program sets its text, or the prompt starts
//! echoing. Before, it was kept and drawn again (in clear, in a password
//! field) when the field next had the focus, beside the text the program
//! set, or in the echoing prompt.
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

#[test]
fn a_prompt_that_starts_echoing_shows_nothing_composed_in_the_password() {
    // A PAM conversation: a password, then a prompt that echoes, in the
    // same field.
    let (mut h, w) = focused("", true);
    h.ime(w, None, "WW");
    h.send(M::Secure(false));
    // The input method's next event, sent for the composition it started
    // in the password prompt.
    h.ime(w, None, "WWX");
    let after = h.render_rgba(w, 2.0);
    let (mut fresh, fw) = focused("", false);
    let empty = fresh.render_rgba(fw, 2.0);
    eprintln!("after the prompt starts echoing vs an empty prompt: {}", differ(&after, &empty));
    assert_eq!(differ(&after, &empty), 0, "nothing composed in the password prompt shows in clear");
}

#[test]
fn a_prompt_that_starts_echoing_types_nothing_composed_in_the_password() {
    let (mut h, w) = focused("", true);
    h.ime(w, None, "WW");
    h.send(M::Secure(false));
    // An input method that commits what it was composing when it's started
    // over (the purpose changed).
    h.ime(w, Some("WW"), "");
    eprintln!("the echoing prompt's text: {:?}", h.program().password);
    assert_eq!(h.program().password, "", "the password's composition isn't typed into the echoing prompt");
    h.ime(w, Some("x"), "");
    assert_eq!(h.program().password, "x", "then what the input method commits is typed");
}
