//! The lock screen, headless, with a stand-in for PAM.

mod common;

use std::sync::Arc;
use std::time::Duration;

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
