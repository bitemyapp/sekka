//! SYC-07: what a field tells input methods. A password field asks for a
//! password (so input methods that honour it type directly, without
//! composing or predicting), and when the field drops a composition the
//! input method still has, it's told to start over, and what it sends for
//! that composition is dropped.

mod support;

use aizome_kit::sidestep_ui::TextPurpose;
use aizome_kit::testing::Harness;
use support::{BLINK, M, Probe, click, focused};

fn purpose(h: &Harness<Probe>) -> Option<TextPurpose> {
    let mut all = h.platform.text_purposes.values().copied();
    let p = all.next();
    assert!(all.next().is_none(), "one window");
    p
}

fn wanted(h: &Harness<Probe>) -> bool {
    h.platform.text_input.values().any(|(on, _)| *on)
}

#[test]
fn a_password_field_asks_input_methods_for_a_password() {
    let (mut h, w) = focused("", true);
    assert!(wanted(&h));
    assert_eq!(purpose(&h), Some(TextPurpose::Password));
    click(&mut h, w, "other");
    assert!(wanted(&h));
    assert_eq!(purpose(&h), Some(TextPurpose::Normal), "a plain field asks for text");
    click(&mut h, w, "password");
    assert_eq!(purpose(&h), Some(TextPurpose::Password));
}

#[test]
fn a_prompt_that_stops_echoing_asks_for_a_password() {
    // A PAM conversation: a name, then a password, in the same field.
    let (mut h, w) = focused("", false);
    assert_eq!(purpose(&h), Some(TextPurpose::Normal));
    h.send(M::Secure(true));
    assert_eq!(h.focused_name(w).as_deref(), Some("password"), "it keeps the focus");
    h.advance(BLINK);
    assert_eq!(purpose(&h), Some(TextPurpose::Password), "told at the field's next event");
    h.send(M::Secure(false));
    h.type_text(w, "x");
    assert_eq!(purpose(&h), Some(TextPurpose::Normal));
    assert_eq!(h.program().password, "x");
}

#[test]
fn a_composition_the_program_dropped_starts_the_input_method_over() {
    let (mut h, w) = focused("pa", true);
    h.ime(w, None, "WW");
    h.send(M::Set(String::new()));
    assert!(h.platform.text_input_resets.is_empty(), "told at the field's next event");
    // The input method commits the composition it still has: it was
    // composed into text that's gone, so it isn't typed.
    h.ime(w, Some("WW"), "");
    assert_eq!(h.program().password, "");
    assert_eq!(h.platform.text_input_resets.len(), 1, "the input method starts over");
    h.ime(w, Some("x"), "");
    assert_eq!(h.program().password, "x", "then what it commits is typed");
    assert_eq!(h.platform.text_input_resets.len(), 1);
}

#[test]
fn a_composition_the_program_dropped_is_told_at_the_next_blink() {
    let (mut h, _) = focused("pa", true);
    h.platform.text_input_resets.clear();
    let w = h.window();
    h.ime(w, None, "WW");
    h.send(M::Set(String::new()));
    h.advance(BLINK);
    assert_eq!(h.platform.text_input_resets.len(), 1);
    h.advance(BLINK);
    assert_eq!(h.platform.text_input_resets.len(), 1, "once");
}

#[test]
fn a_committed_composition_is_typed_as_dots() {
    let (mut h, w) = focused("", true);
    h.ime(w, None, "パ");
    assert_eq!(h.program().password, "", "composing isn't typing");
    h.ime(w, Some("パ"), "");
    assert_eq!(h.program().password, "パ");
    assert!(h.platform.text_input_resets.is_empty());
}

#[test]
fn a_prompt_that_stops_echoing_arrives_before_the_next_blink() {
    let (mut h, w) = focused("", false);
    h.send(M::Secure(true));
    h.advance(std::time::Duration::ZERO);
    assert_eq!(purpose(&h), Some(TextPurpose::Password), "told at once, not at the caret's next blink");
    h.ime(w, Some("k"), "");
    assert_eq!(h.program().password, "k", "what the input method sends after that is typed");
}
