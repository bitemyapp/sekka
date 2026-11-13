// SPDX-License-Identifier: GPL-3.0-or-later
//! GEN-34: the person confirms before preparation finishes (allowed by
//! design: "Finishing preparation…"), then preparation fails before any disk
//! write, e.g. the in-memory build's download fails ("the target disk has not
//! been erased"). That is a failed preparation, not a failed installation:
//! no step may be marked failed, and nothing may say the disk was modified.
//! It used to mark step 1 "Partition the disk" failed, head the page
//! "Installation Failed", say "The disk may have been modified", and leave
//! no way back to the review.
//!
//! Headless, on the preview's review page (preparation still working); the
//! helper's events go through the real `Msg::Session` handler, with the
//! review's ticket, in the order the helper sends them for this case:
//! `Failed`, then the exit (status 1).
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use calamares_nixos::install::Event;
use genkan::Step;
use genkan::app::{Genkan, Helper, Msg, PrepState, StepState};
use genkan::flow::HelperAnswer;

#[test]
fn a_failure_before_any_disk_write_is_not_reported_as_a_failed_partitioning() {
    let appearance = Appearance {
        theme: ThemeChoice::Night,
        reduced_motion: true,
        ..Appearance::default()
    };
    let mut h = Harness::with(Genkan::new(Some("review".into())), appearance, false);
    h.allow_agreements();
    let w = h.window();
    assert_eq!(h.program().step, Step::Review);
    assert!(!h.program().prepared(), "preparation is still running");
    let ticket = h
        .program()
        .review_ticket()
        .expect("the sample review's ticket");
    let phrase = h.program().phrase.clone();
    h.send(Msg::Erase(phrase));
    h.send(Msg::Consent(true));
    assert!(h.program().next_enabled(), "confirming early is allowed");
    press_confirm(&mut h);
    assert_eq!(h.program().step, Step::Install);
    println!("after confirming: {:?}", h.program().install_message);

    // The helper's in-memory build fails (install.rs's build stage), before
    // step 1.
    let message = "The installed system could not be built; the target disk has not been erased: \
                   nix failed (exit status: 1): error: unable to download 'https://cache.nixos.org/nar/…': HTTP error 503";
    h.send(Msg::Session(HelperAnswer::new(
        ticket,
        Helper::Event(Event::Failed {
            message: message.into(),
        }),
    )));
    h.send(Msg::Session(HelperAnswer::new(
        ticket,
        Helper::Finished(Err("The installation helper stopped (authorization canceled or helper failed, exit status: 1). Do not assume the disk is unchanged if installation had started.".into())),
    )));

    let app = h.program();
    let texts = h.texts(w);
    let says = |what: &str| texts.iter().any(|t| t.contains(what));
    let failed_steps: Vec<usize> = app
        .steps()
        .iter()
        .enumerate()
        .filter(|(_, s)| **s == StepState::Failed)
        .map(|(i, _)| i + 1)
        .collect();
    let heading_failed = says("Installation Failed");
    let disk_may_be_modified = says("The disk may have been modified");
    let back = texts.iter().any(|t| t == "Back");
    println!(
        "step={:?} install_step={} failed_steps={failed_steps:?} heading_failed={heading_failed} \
         disk_may_be_modified={disk_may_be_modified} back_offered={back} next_label={:?} \
         preparation={:?} password_kept={}",
        app.step,
        app.install_step(),
        app.next_label(),
        app.prep_state(),
        !app.password.is_empty()
    );
    assert!(
        failed_steps.is_empty() && !disk_may_be_modified && !heading_failed,
        "a failure before any disk write was shown as a failed installation: failed steps \
         {failed_steps:?} (1 = \"Partition the disk\"), \"The disk may have been modified\": \
         {disk_may_be_modified}, \"Installation Failed\": {heading_failed}"
    );
    // It's the review's failed preparation, which says so and can be left
    // to change the choices, with the passwords still there.
    assert_eq!(app.step, Step::Review);
    assert_eq!(app.prep_state(), PrepState::Failed);
    assert!(says("Preparation failed") && says("could not be built"));
    assert!(back, "Back is offered");
    assert!(!app.password.is_empty(), "the passwords were kept");
    assert!(
        !app.next_enabled(),
        "a failed preparation can't be confirmed"
    );
    h.send(Msg::Back);
    assert_eq!(h.program().step, Step::Keyboard);
}

/// The confirmation queued before preparation finished goes to the helper
/// when it reports `Prepared`, and only then does the installation start.
#[test]
fn a_queued_confirmation_starts_the_installation_once_prepared() {
    let mut h = Harness::with(
        Genkan::new(Some("review".into())),
        Appearance::default(),
        false,
    );
    h.allow_agreements();
    let ticket = h.program().review_ticket().unwrap();
    let phrase = h.program().phrase.clone();
    h.send(Msg::Erase(phrase));
    h.send(Msg::Consent(true));
    press_confirm(&mut h);
    assert!(h.program().installing() && !h.program().prepared());
    assert!(!h.program().password.is_empty(), "nothing was sent yet");
    h.send(Msg::Session(HelperAnswer::new(
        ticket,
        Helper::Event(Event::Prepared {
            summary: Default::default(),
        }),
    )));
    assert_eq!(
        h.program().install_message,
        "Preparation complete. Starting the installation…"
    );
    assert!(h.program().password.is_empty(), "sent, so forgotten");
    h.send(Msg::Session(HelperAnswer::new(
        ticket,
        Helper::Event(Event::Progress {
            step: 1,
            message: "Erasing the selected disk and creating a GPT partition table".into(),
        }),
    )));
    h.send(Msg::Session(HelperAnswer::new(
        ticket,
        Helper::Event(Event::Failed {
            message: "wipefs failed".into(),
        }),
    )));
    // Past the first write, a failure is the installation's.
    let app = h.program();
    assert_eq!(app.step, Step::Install);
    assert!(app.failed() && app.disk_written());
    assert_eq!(app.steps()[0], StepState::Failed);
    assert!(
        h.texts(h.window())
            .iter()
            .any(|t| t.contains("The disk may have been modified"))
    );
}

/// A press of the Review page's Erase Disk and Install button, as a person's:
/// the agreement only such a press makes is what confirms.
fn press_confirm(h: &mut Harness<Genkan>) {
    let w = h.window();
    let at = h
        .find_id(w, "next")
        .expect("the Review page's button")
        .center();
    h.click(w, at);
}

/// Nothing but a person's press of the button confirms: not Next sent for
/// them (as the Review page's Next once confirmed, and the preview's sample
/// still confirms by its own path), not a press the harness makes without
/// letting it agree, and not a press before the phrase is typed.
#[test]
fn only_a_press_of_the_button_confirms() {
    let mut h = Harness::with(
        Genkan::new(Some("review".into())),
        Appearance::default(),
        false,
    );
    let phrase = h.program().phrase.clone();
    h.send(Msg::Consent(true));
    h.allow_agreements();
    press_confirm(&mut h);
    assert_eq!(
        h.program().step,
        Step::Review,
        "a press before the phrase is typed"
    );
    h.send(Msg::Erase(phrase));
    h.send(Msg::Next);
    assert_eq!(
        h.program().step,
        Step::Review,
        "Next sent by code confirms nothing"
    );
    assert!(!h.program().installing());

    let mut h = Harness::with(
        Genkan::new(Some("review".into())),
        Appearance::default(),
        false,
    );
    let phrase = h.program().phrase.clone();
    h.send(Msg::Erase(phrase));
    h.send(Msg::Consent(true));
    press_confirm(&mut h);
    assert_eq!(
        h.program().step,
        Step::Review,
        "a press that makes no agreement"
    );
    h.allow_agreements();
    press_confirm(&mut h);
    assert_eq!(h.program().step, Step::Install, "the person's press");
}

/// A keyboard-only person: Return in the phrase field moves to Erase Disk
/// and Install without confirming, and Return there confirms.
#[test]
fn return_in_the_phrase_field_moves_to_the_button_which_confirms() {
    let mut h = Harness::with(
        Genkan::new(Some("review".into())),
        Appearance::default(),
        false,
    );
    h.allow_agreements();
    let w = h.window();
    h.send(Msg::Consent(true));
    let phrase = h.program().phrase.clone();
    let field = h.find_id(w, "erase").expect("the phrase field").center();
    h.click(w, field);
    h.type_text(w, &phrase);
    assert_eq!(h.program().erase, phrase);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(
        h.program().step,
        Step::Review,
        "Return in the field confirms nothing"
    );
    assert_eq!(h.focused_name(w).as_deref(), Some("next"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(
        h.program().step,
        Step::Install,
        "Return on the button is the person's press"
    );
}

/// The prepared sample review ("Ready to install"), with the phrase typed in
/// its field by the keyboard and Return pressed there and not let go: the
/// focus has moved to Erase Disk and Install, and the key is still down.
fn return_down_in_the_phrase_field() -> Harness<Genkan> {
    let mut h = Harness::with(
        Genkan::new(Some("ready".into())),
        Appearance::default(),
        false,
    );
    h.allow_agreements();
    let w = h.window();
    assert!(
        h.program().prepared(),
        "the helper has prepared and waits for the confirmation"
    );
    // The preview fills the phrase in: type it as a person does.
    h.send(Msg::Erase(String::new()));
    assert!(!h.program().next_enabled());
    let phrase = h.program().phrase.clone();
    let field = h.find_id(w, "erase").expect("the phrase field").center();
    h.click(w, field);
    h.type_text(w, &phrase);
    assert_eq!(h.program().erase, phrase);
    assert!(h.program().next_enabled());
    h.key_down(w, Key::Named(NamedKey::Enter), Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("next"));
    assert_eq!(h.program().step, Step::Review);
    h
}

/// Nothing was confirmed: the review is still the page, and nothing went to
/// the helper (which erases the disk once it has the confirmation).
fn assert_nothing_confirmed(h: &Harness<Genkan>, after: &str) {
    let app = h.program();
    assert!(
        app.step == Step::Review && !app.installing() && app.prepared(),
        "{after} confirmed the erase: step {:?}, installing: {}, {:?}",
        app.step,
        app.installing(),
        app.install_message
    );
    assert!(
        !app.password.is_empty(),
        "{after}: nothing was sent to the helper"
    );
}

/// One Return, held: in the phrase field it moves to Erase Disk and Install,
/// and its repeats there are no press of the button. Holding a key down must
/// never start the erase; letting go and pressing Return again is the
/// person's press.
#[test]
fn a_return_held_in_the_phrase_field_confirms_nothing() {
    let mut h = return_down_in_the_phrase_field();
    let w = h.window();
    for _ in 0..50 {
        h.key_repeat(w, Key::Named(NamedKey::Enter), Modifiers::NONE);
    }
    assert_nothing_confirmed(&h, "a held Return's repeats");
    h.key_up(w, Key::Named(NamedKey::Enter), Modifiers::NONE);
    assert_nothing_confirmed(&h, "letting the held Return go");
    assert_eq!(h.focused_name(w).as_deref(), Some("next"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(
        h.program().step,
        Step::Install,
        "Return pressed again on the button is the person's press"
    );
    assert!(h.program().installing());
}

/// The same from a platform that doesn't mark a held key's repeats: Return
/// presses with no release between them confirm nothing either.
#[test]
fn return_presses_with_no_release_between_confirm_nothing() {
    let mut h = return_down_in_the_phrase_field();
    let w = h.window();
    for _ in 0..50 {
        h.key_down(w, Key::Named(NamedKey::Enter), Modifiers::NONE);
    }
    assert_nothing_confirmed(&h, "Return presses with no release between");
    h.key_up(w, Key::Named(NamedKey::Enter), Modifiers::NONE);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().step, Step::Install);
}

/// A held Space on the consent box flips it once, so what it shows when the
/// key is let go is what the person set.
#[test]
fn a_space_held_on_the_consent_box_ticks_it_once() {
    let mut h = Harness::with(
        Genkan::new(Some("review".into())),
        Appearance::default(),
        false,
    );
    let w = h.window();
    assert!(!h.program().consent);
    let field = h.find_id(w, "erase").expect("the phrase field").center();
    h.click(w, field);
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("consent"));
    let space = Key::Character(" ".into());
    h.key_down(w, space.clone(), Modifiers::NONE);
    for _ in 0..7 {
        h.key_repeat(w, space.clone(), Modifiers::NONE);
    }
    h.key_up(w, space, Modifiers::NONE);
    assert!(h.program().consent, "a held Space left the box unticked");
}
