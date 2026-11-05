//! SYC-07, in the kit's text field: a password field must not show what
//! an input method is composing in it.
//!
//! `text_field(..).password()` (Settei's Wi-Fi and user passwords, the
//! network agent's secrets, Genkan's account) drew its value as dots but
//! inserted the composition at the caret in clear. The test renders the
//! password field holding "pa" while an input method composes "WW", and a
//! plain field holding the dots "••" with the same composition: before,
//! they were pixel for pixel the same. A password field now keeps only a
//! count of what's composed, drawn as dots. Text the program sets drops
//! the composition, as losing the focus did already, and so does ticking
//! "Show password" while composing.
//!
//! These build on the field before and after it was made a typestate, so
//! they run against both.

#[path = "support/password.rs"]
mod support;

use support::{Msg, differ, focused};

#[test]
fn a_password_text_field_hides_the_text_being_composed() {
    let (mut h, w) = focused("pa", false);
    h.ime(w, None, "WW");
    let composing = h.render_rgba(w, 2.0);
    let (mut plain, pw) = focused("••", true);
    plain.ime(pw, None, "WW");
    let in_clear = plain.render_rgba(pw, 2.0);
    let (mut idle, iw) = focused("pa", false);
    let dots_only = idle.render_rgba(iw, 2.0);
    let vs_clear = differ(&composing, &in_clear);
    let vs_dots = differ(&composing, &dots_only);
    eprintln!(
        "pixels differing from the dots composing in clear: {vs_clear}; from the field with no composition: {vs_dots}"
    );
    assert!(vs_dots > 0, "the composition shows somehow");
    assert!(
        vs_clear > 0,
        "the password field composing \"WW\" draws what a plain field showing \"••\" and composing \"WW\" draws: the composed text is in clear"
    );
}

#[test]
fn the_programs_text_drops_what_a_password_field_was_composing() {
    let (mut h, w) = focused("pa", false);
    h.ime(w, None, "WW");
    // A wrong password: the sheet clears the field.
    h.send(Msg::Set(String::new()));
    let cleared = h.render_rgba(w, 2.0);
    let (mut fresh, fw) = focused("", false);
    let clean = fresh.render_rgba(fw, 2.0);
    eprintln!("cleared vs clean: {}", differ(&cleared, &clean));
    assert_eq!(differ(&cleared, &clean), 0, "the cleared field shows nothing of the composition");
}

#[test]
fn showing_the_password_shows_nothing_composed_while_hidden() {
    let (mut h, w) = focused("", false);
    h.ime(w, None, "WW");
    h.send(Msg::Show(true));
    // The input method's next event, sent for the composition it started
    // while the password was hidden.
    h.ime(w, None, "WWX");
    let after = h.render_rgba(w, 2.0);
    let (mut fresh, fw) = focused("", true);
    let empty = fresh.render_rgba(fw, 2.0);
    eprintln!("after Show Password vs an empty shown field: {}", differ(&after, &empty));
    assert_eq!(differ(&after, &empty), 0, "nothing composed while hidden shows in clear");
}

#[test]
fn showing_the_password_types_nothing_composed_while_hidden() {
    let (mut h, w) = focused("", false);
    h.ime(w, None, "WW");
    h.send(Msg::Show(true));
    h.ime(w, Some("WW"), "");
    eprintln!("the shown field's text: {:?}", h.program().secret);
    assert_eq!(h.program().secret, "", "the hidden composition isn't typed once shown");
    h.ime(w, Some("x"), "");
    assert_eq!(h.program().secret, "x", "then what the input method commits is typed");
}
