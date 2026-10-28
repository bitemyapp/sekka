//! The lock screen, headless, with a stand-in for PAM.

mod common;

use std::sync::Arc;
use std::time::Duration;

use aizome_kit::Output;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_shell::actions::{Action, SessionVerb};
use aizome_shell::lock::Phase;
use aizome_shell::shell::{Msg, Shell};
use common::*;

fn locked(day: bool) -> (aizome_kit::testing::Harness<Shell>, WindowKey) {
    let mut h = scene(day);
    {
        let s = h.program_mut();
        s.authenticator = Arc::new(aizome_shell::pam::Fixed("hunter2".into()));
        s.user = ("callen".into(), "Chris Allen".into());
        s.settings.appearance.wallpaper_night = Some(wallpaper(false).to_string_lossy().into_owned());
        s.settings.appearance.wallpaper_day = Some(wallpaper(true).to_string_lossy().into_owned());
        s.theme_day = day;
        s.load_wallpapers();
    }
    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    assert!(h.platform.lock_asked);
    h.confirm_lock();
    assert_eq!(h.program().lock.phase, Phase::Locked);
    // The lock surface is the window without a title.
    let lock = h.windows().into_iter().find(|k| h.title(*k).is_empty()).expect("a lock surface");
    h.resize(lock, Size::new(W, H));
    (h, lock)
}

/// Let the authenticator's thread answer.
fn settle(h: &mut aizome_kit::testing::Harness<Shell>) {
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(10));
        h.send(Msg::Nothing);
        if !h.program().lock.busy {
            return;
        }
    }
}

#[test]
fn a_wrong_password_says_so_and_the_right_one_unlocks() {
    let (mut h, lock) = locked(false);
    assert!(h.texts(lock).iter().any(|t| t == "21:42"));
    h.type_text(lock, "nope");
    h.named(lock, NamedKey::Enter, Modifiers::NONE);
    settle(&mut h);
    assert!(h.program().lock.error.is_some());
    assert_eq!(h.program().lock.phase, Phase::Locked);
    assert!(h.program().lock.password.is_empty(), "the password isn't kept");
    h.type_text(lock, "hunter2");
    h.named(lock, NamedKey::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(h.program().lock.phase, Phase::Unlocked);
    assert!(h.windows().iter().all(|k| *k != lock), "the lock surfaces closed");
}

#[test]
fn notifications_dont_show_while_locked() {
    let (mut h, _lock) = locked(false);
    use aizome_shell::notifications::server::NoteEvent;
    let n = aizome_shell::notifications::store::Incoming {
        app_name: "Mail".into(),
        summary: "Secret".into(),
        ..Default::default()
    };
    h.send(Msg::Note(NoteEvent::Notify(99, Box::new(n))));
    assert!(h.program().banner.is_none());
    assert_eq!(h.program().notes.unread_count(), 1, "filed for later");
}

fn external() -> Output {
    Output { id: 2, name: "DP-1".into(), connector: Some("DP-1".into()), ..output() }
}

fn prompt(h: &mut aizome_kit::testing::Harness<Shell>, output: u32) -> WindowKey {
    let key = h.program().lock.surfaces.iter().find(|(_, id)| *id == output).expect("lock surface on output").0;
    h.resize(key, Size::new(W, H));
    assert!(h.find_id(key, "lock-password").is_some(), "every display has a password field");
    assert_eq!(h.focused_name(key).as_deref(), Some("lock-password"), "ready to type without clicking");
    key
}

#[test]
fn every_display_can_accept_the_password() {
    let mut h = scene(false);
    h.program_mut().authenticator = Arc::new(aizome_shell::pam::Fixed("hunter2".into()));
    h.set_outputs(vec![output(), external()]);
    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    h.confirm_lock();
    let laptop = prompt(&mut h, 1);
    let monitor = prompt(&mut h, 2);
    h.type_text(laptop, "hunter");
    h.type_text(monitor, "2");
    assert_eq!(h.program().lock.password, "hunter2", "one password shared across displays");
    h.named(monitor, NamedKey::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(h.program().lock.phase, Phase::Unlocked);
}

#[test]
fn returning_laptop_panel_gets_a_prompt_without_restarting_the_lock() {
    let mut h = scene(false);
    h.program_mut().authenticator = Arc::new(aizome_shell::pam::Fixed("hunter2".into()));
    h.set_outputs(vec![external()]);
    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    h.confirm_lock();
    let old = prompt(&mut h, 2);
    // Dock removed while asleep, then the built-in panel comes back with
    // a new Wayland global. There may briefly be no advertised outputs.
    h.set_outputs(vec![]);
    assert_eq!(h.program().lock.phase, Phase::Locked);
    assert!(h.program().lock.surfaces.is_empty());
    assert!(!h.windows().contains(&old));
    let panel = Output { id: 7, ..output() };
    h.set_outputs(vec![panel.clone()]);
    let lock = prompt(&mut h, 7);
    h.set_outputs(vec![panel]);
    assert_eq!(h.program().lock.surfaces, vec![(lock, 7)], "repeated descriptions don't duplicate surfaces");
    h.type_text(lock, "hunter2\n");
    settle(&mut h);
    assert_eq!(h.program().lock.phase, Phase::Unlocked);
}

#[test]
fn displays_can_change_before_lock_confirmation() {
    let mut h = scene(false);
    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    let old = prompt(&mut h, 1);
    h.set_outputs(vec![external()]);
    assert_eq!(h.program().lock.phase, Phase::Locking);
    assert!(!h.windows().contains(&old));
    prompt(&mut h, 2);
    h.confirm_lock();
    assert_eq!(h.program().lock.phase, Phase::Locked);
    h.finish_lock();
    h.set_outputs(vec![output(), external()]);
    assert!(h.program().lock.surfaces.is_empty(), "an ended lock isn't recreated");
}

#[test]
fn removing_one_display_preserves_the_other_prompt_and_password() {
    let (mut h, laptop) = locked(false);
    h.type_text(laptop, "hunter");
    h.set_outputs(vec![output(), external()]);
    let monitor = prompt(&mut h, 2);
    h.set_outputs(vec![external()]);
    assert!(!h.windows().contains(&laptop));
    assert_eq!(h.program().lock.surfaces, vec![(monitor, 2)]);
    assert_eq!(h.program().lock.password, "hunter");
    h.type_text(monitor, "2\n");
    settle(&mut h);
    assert_eq!(h.program().lock.phase, Phase::Unlocked);
}

#[test]
fn a_closed_lock_surface_is_replaced_without_ending_the_lock() {
    let (mut h, old) = locked(false);
    h.request_close(old);
    let new = prompt(&mut h, 1);
    assert_ne!(old, new);
    assert!(!h.windows().contains(&old));
    assert_eq!(h.program().lock.phase, Phase::Locked);
    h.type_text(new, "hunter2\n");
    settle(&mut h);
    assert_eq!(h.program().lock.phase, Phase::Unlocked);
}

#[test]
fn an_output_description_repositions_the_existing_prompt() {
    let (mut h, lock) = locked(false);
    let before = h.find_id(lock, "lock-password").unwrap();
    let mut panel = output();
    panel.frame = Rect::new(0.0, 0.0, W - 200.0, H - 100.0);
    h.set_outputs(vec![panel]);
    // The output callback must rebuild existing views even before another
    // input event or clock tick. The surface identity and typed text stay.
    let after = h.find_id(lock, "lock-password").unwrap();
    assert_eq!(after.x0, before.x0 - 100.0);
    assert_eq!(after.y0, before.y0 - 50.0);
    assert_eq!(h.program().lock.surfaces, vec![(lock, 1)]);
}

#[test]
fn pictures_of_the_lock_screen() {
    for day in [false, true] {
        let (mut h, lock) = locked(day);
        h.type_text(lock, "abc");
        let name = if day { "lock-day.png" } else { "lock-night.png" };
        h.save_png(lock, shots().join(name), 1.0);
        aizome_kit::testing::assert_golden(&mut h, lock, golden(name), 1.0);
    }
}
