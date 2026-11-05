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
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use calamares_nixos::install::Event;
use genkan::Step;
use genkan::app::{Genkan, Helper, Msg, PrepState, StepState};

#[test]
fn a_failure_before_any_disk_write_is_not_reported_as_a_failed_partitioning() {
    let appearance = Appearance {
        theme: ThemeChoice::Night,
        reduced_motion: true,
        ..Appearance::default()
    };
    let mut h = Harness::with(Genkan::new(Some("review".into())), appearance, false);
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
    h.send(Msg::Next);
    assert_eq!(h.program().step, Step::Install);
    println!("after confirming: {:?}", h.program().install_message);

    // The helper's in-memory build fails (install.rs's build stage), before
    // step 1.
    let message = "The installed system could not be built; the target disk has not been erased: \
                   nix failed (exit status: 1): error: unable to download 'https://cache.nixos.org/nar/…': HTTP error 503";
    h.send(Msg::Session(
        ticket,
        Helper::Event(Event::Failed {
            message: message.into(),
        }),
    ));
    h.send(Msg::Session(
        ticket,
        Helper::Finished(Err("The installation helper stopped (authorization canceled or helper failed, exit status: 1). Do not assume the disk is unchanged if installation had started.".into())),
    ));

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
    let ticket = h.program().review_ticket().unwrap();
    let phrase = h.program().phrase.clone();
    h.send(Msg::Erase(phrase));
    h.send(Msg::Consent(true));
    h.send(Msg::Next);
    assert!(h.program().installing() && !h.program().prepared());
    assert!(!h.program().password.is_empty(), "nothing was sent yet");
    h.send(Msg::Session(
        ticket,
        Helper::Event(Event::Prepared {
            summary: Default::default(),
        }),
    ));
    assert_eq!(
        h.program().install_message,
        "Preparation complete. Starting the installation…"
    );
    assert!(h.program().password.is_empty(), "sent, so forgotten");
    h.send(Msg::Session(
        ticket,
        Helper::Event(Event::Progress {
            step: 1,
            message: "Erasing the selected disk and creating a GPT partition table".into(),
        }),
    ));
    h.send(Msg::Session(
        ticket,
        Helper::Event(Event::Failed {
            message: "wipefs failed".into(),
        }),
    ));
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
