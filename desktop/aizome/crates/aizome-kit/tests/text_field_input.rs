//! What a text field tells input methods. A password field asks for a
//! password (so input methods that honour it type directly, without
//! composing or predicting); "Show password" toggled while it's focused
//! keeps the focus and changes what's asked; when the program sets the
//! text under a composition, the input method is told to start over and
//! what it sends for that composition isn't typed.

#[path = "support/password.rs"]
mod support;

use aizome_kit::sidestep_ui::TextPurpose;
use aizome_kit::testing::Harness;
use support::{BLINK, Form, Msg, click, focused};

fn purpose(h: &Harness<Form>) -> Option<TextPurpose> {
    let mut all = h.platform.text_purposes.values().copied();
    let p = all.next();
    assert!(all.next().is_none(), "one window");
    p
}

#[test]
fn a_password_field_asks_input_methods_for_a_password() {
    let (mut h, w) = focused("", false);
    assert!(h.platform.text_input.values().any(|(on, _)| *on));
    assert_eq!(purpose(&h), Some(TextPurpose::Password));
    click(&mut h, w, "name");
    assert_eq!(purpose(&h), Some(TextPurpose::Normal), "a plain field asks for text");
    click(&mut h, w, "secret");
    assert_eq!(purpose(&h), Some(TextPurpose::Password));
}

#[test]
fn showing_the_password_keeps_the_field_and_changes_what_it_asks_for() {
    let (mut h, w) = focused("", false);
    h.type_text(w, "ab");
    h.send(Msg::Show(true));
    assert_eq!(h.focused_name(w).as_deref(), Some("secret"), "it keeps the focus");
    h.advance(BLINK);
    assert_eq!(purpose(&h), Some(TextPurpose::Normal), "told at the field's next event");
    h.type_text(w, "c");
    assert_eq!(h.program().secret, "abc", "the caret stayed at the end");
    h.send(Msg::Show(false));
    h.type_text(w, "d");
    assert_eq!(purpose(&h), Some(TextPurpose::Password));
    assert_eq!(h.program().secret, "abcd");
}

#[test]
fn a_composition_the_program_dropped_starts_the_input_method_over() {
    let (mut h, w) = focused("pa", false);
    h.ime(w, None, "WW");
    h.send(Msg::Set(String::new()));
    assert!(h.platform.text_input_resets.is_empty(), "told at the field's next event");
    // The input method commits the composition it still has: it was
    // composed into text that's gone, so it isn't typed.
    h.ime(w, Some("WW"), "");
    assert_eq!(h.program().secret, "");
    assert_eq!(h.platform.text_input_resets.len(), 1, "the input method starts over");
    h.ime(w, Some("x"), "");
    assert_eq!(h.program().secret, "x", "then what it commits is typed");
}

#[test]
fn losing_the_focus_drops_what_was_being_composed() {
    let (mut h, w) = focused("pa", false);
    h.ime(w, None, "WW");
    click(&mut h, w, "name");
    click(&mut h, w, "secret");
    let back = h.render_rgba(w, 2.0);
    let (mut fresh, fw) = focused("pa", false);
    assert_eq!(support::differ(&back, &fresh.render_rgba(fw, 2.0)), 0);
}

#[test]
fn after_showing_the_password_the_field_arrives_before_the_next_blink() {
    let (mut h, w) = focused("", false);
    h.send(Msg::Show(true));
    h.advance(std::time::Duration::ZERO);
    assert_eq!(purpose(&h), Some(TextPurpose::Normal), "told at once, not at the caret's next blink");
    h.ime(w, Some("k"), "");
    assert_eq!(h.program().secret, "k", "what the input method sends after that is typed");
}
