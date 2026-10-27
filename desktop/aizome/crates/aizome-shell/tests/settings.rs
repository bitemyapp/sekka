//! What Settings (Settei) says, honoured live: the desktop's icons, the
//! Shelf, notifications blocked, silent or previewed on the lock screen,
//! the lock screen's clock and locking with the monitors off, the power
//! key, the palette's scopes, the bell's flash and Night Light.

mod common;

use std::time::Duration;

use aizome_config::PowerKey;
use aizome_ipc::Event;
use aizome_kit::prelude::*;
use aizome_shell::actions::{Action, SessionVerb};
use aizome_shell::ipc::IpcEvent;
use aizome_shell::lock::Phase;
use aizome_shell::notifications::server::NoteEvent;
use aizome_shell::notifications::store::{Incoming, NoteSound};
use aizome_shell::shell::{Msg, Shell};
use aizome_shell::systray::panels::{Panel, PanelMsg};
use common::*;

type H = aizome_kit::testing::Harness<Shell>;

/// Change the settings as Settei would, and have the shell read them.
fn settings(h: &mut H, f: impl FnOnce(&mut aizome_config::Settings)) {
    f(&mut h.program_mut().settings);
    h.send(Msg::SettingsChanged);
}

fn note(h: &mut H, id: u32, app: &str, summary: &str, sound: Option<NoteSound>) {
    let n = Incoming {
        app_name: app.into(),
        summary: summary.into(),
        body: "Half past twelve".into(),
        sound,
        ..Default::default()
    };
    h.send(Msg::Note(NoteEvent::Notify(id, Box::new(n))));
}

fn binding(h: &mut H, action: &str) {
    h.send(Msg::Ipc(IpcEvent::Event(Event::Binding { action: action.into(), pointer: None, output: None })));
}

#[test]
fn the_desktop_shows_the_icons_it_is_told_to() {
    let mut h = scene(false);
    h.program_mut().volumes_fixture =
        vec![aizome_sys::disks::Volume { block: "/b/nvme0n1p3".into(), name: "nvme0".into(), ..Default::default() }];
    h.program_mut().user = ("callen".into(), "Chris Allen".into());
    let keys = |h: &H| h.program().desktop_icons().into_iter().map(|i| i.key).collect::<Vec<_>>();
    assert_eq!(keys(&h), ["home", "vol:/b/nvme0n1p3", "trash"]);
    settings(&mut h, |s| {
        let mut d = s.desktop_settings();
        d.show_trash = false;
        d.show_disks = false;
        s.set_shell_section("desktop", &d);
    });
    assert_eq!(keys(&h), ["home"]);
}

#[test]
fn the_shelf_can_go() {
    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    let with = h.program().deskbar_model();
    assert!(!with.hide_shelf);
    settings(&mut h, |s| {
        let mut d = s.deskbar_settings();
        d.show_shelf = false;
        s.set_shell_section("deskbar", &d);
    });
    let without = h.program().deskbar_model();
    assert!(without.hide_shelf);
    assert!(without.height() < with.height());
    let db = window_titled(&h, "Deskbar");
    assert!(!h.texts(db).iter().any(|t| t == "wallpaper-draft.png"), "no Shelf rows");
}

#[test]
fn blocked_apps_are_dropped_and_silent_ones_make_no_sound() {
    let mut h = scene(false);
    settings(&mut h, |s| {
        let mut n = s.notification_settings();
        n.blocked_apps = vec!["spam.desktop".into()];
        n.silent_apps = vec!["Mail".into()];
        s.set_shell_section("notifications", &n);
    });
    let ding = || Some(NoteSound::Named("message-new-instant".into()));
    note(&mut h, 90, "Spam", "Win a prize", ding());
    assert!(h.program().notes.get(90).is_none(), "dropped");
    assert_eq!(h.program().banner, None);
    assert_eq!(h.program().last_sound, None);
    note(&mut h, 91, "Mail", "Lunch?", ding());
    assert!(h.program().notes.get(91).is_some(), "shown");
    assert_eq!(h.program().last_sound, None, "but silent");
    note(&mut h, 92, "Chat", "Hi", ding());
    assert_eq!(h.program().last_sound.as_deref(), Some("Named(\"message-new-instant\") at 0.70"));
    // At the alert volume Sound sets.
    settings(&mut h, |s| {
        aizome_config::Settings::set_shell_section(
            s,
            "sound",
            &aizome_config::SoundSettings { alert_volume: 0.25, ..Default::default() },
        )
    });
    note(&mut h, 93, "Chat", "Again", ding());
    assert_eq!(h.program().last_sound.as_deref(), Some("Named(\"message-new-instant\") at 0.25"));
}

fn lock(h: &mut H) -> WindowKey {
    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    h.confirm_lock();
    let lock = h.windows().into_iter().find(|k| h.title(*k).is_empty()).expect("a lock surface");
    h.resize(lock, Size::new(W, H));
    lock
}

#[test]
fn the_lock_screen_shows_what_came_and_its_clock_as_told() {
    let mut h = scene(false);
    let lock_k = lock(&mut h);
    assert!(h.texts(lock_k).iter().any(|t| t == "21:42"));
    note(&mut h, 93, "Mail", "Lunch?", None);
    note(&mut h, 94, "Mail", "Re: Lunch?", None);
    // Without previews: whose, and how many.
    assert!(h.texts(lock_k).iter().any(|t| t == "Mail 2 notifications"), "{:?}", h.texts(lock_k));
    assert!(!h.texts(lock_k).iter().any(|t| t.contains("Lunch")));
    settings(&mut h, |s| {
        let mut n = s.notification_settings();
        n.previews_when_locked = true;
        s.set_shell_section("notifications", &n);
        let mut l = s.lock_settings();
        l.show_clock = false;
        s.set_shell_section("lock", &l);
    });
    let texts = h.texts(lock_k);
    assert!(texts.iter().any(|t| t == "Mail Re: Lunch?") && texts.iter().any(|t| t == "Mail Lunch?"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "21:42"), "no clock");
    h.save_png(lock_k, shots().join("lock-notes-night.png"), 1.0);
}

#[test]
fn the_monitors_going_off_locks_unless_told_not_to() {
    let idle = |h: &mut H| h.send(Msg::Ipc(IpcEvent::Event(Event::Idle { kind: "monitors-off".into() })));
    let mut h = scene(false);
    settings(&mut h, |s| {
        let mut l = s.lock_settings();
        l.after_monitors_off = false;
        s.set_shell_section("lock", &l);
    });
    idle(&mut h);
    assert_eq!(h.program().lock.phase, Phase::Unlocked);
    settings(&mut h, |s| {
        let mut l = s.lock_settings();
        l.after_monitors_off = true;
        s.set_shell_section("lock", &l);
    });
    idle(&mut h);
    assert_ne!(h.program().lock.phase, Phase::Unlocked);
}

#[test]
fn the_power_key_does_what_it_is_told() {
    let set = |h: &mut H, k: Option<PowerKey>| {
        settings(h, |s| s.set_shell_section("power", &aizome_config::PowerSettings { power_key: k }))
    };
    // Unset, with logind handling the key: the shell leaves it alone.
    let mut h = scene(false);
    h.program_mut().logind_power_key = Some("poweroff".into());
    binding(&mut h, "power-key");
    assert!(h.program().confirm.is_none() && h.program().session_done.is_none());
    // Unset, logind leaving it to the session: ask.
    h.program_mut().logind_power_key = Some("ignore".into());
    binding(&mut h, "power-key");
    let c = window_titled(&h, "Confirm");
    for want in ["Sleep", "Restart", "Cancel", "Shut Down"] {
        assert!(h.texts(c).iter().any(|t| t == want), "{want} in {:?}", h.texts(c));
    }
    h.save_png(c, shots().join("confirm-power-night.png"), 2.0);
    h.send(Msg::Confirm(aizome_shell::confirm::ConfirmMsg::Do(SessionVerb::Sleep)));
    assert_eq!(h.program().session_done, Some(SessionVerb::Sleep));
    assert!(h.program().confirm.is_none());
    // Set: lock, whatever logind says.
    set(&mut h, Some(PowerKey::Lock));
    h.program_mut().logind_power_key = Some("poweroff".into());
    binding(&mut h, "power-key");
    assert_ne!(h.program().lock.phase, Phase::Unlocked);
    let mut h = scene(false);
    set(&mut h, Some(PowerKey::Nothing));
    binding(&mut h, "power-key");
    assert!(h.program().confirm.is_none() && h.program().session_done.is_none());
}

#[test]
fn the_palette_finds_only_its_scopes() {
    let mut h = scene(false);
    settings(&mut h, |s| {
        let mut p = s.palette_settings();
        p.scopes = vec!["commands".into(), "apps".into()];
        s.set_shell_section("palette", &p);
    });
    use aizome_search::Scope;
    assert_eq!(h.program().palette_scopes(), [Scope::Command, Scope::App]);
    let items = h.program().palette_items();
    assert!(items.iter().all(|i| matches!(i.scope, Scope::Command | Scope::App)));
    assert!(items.iter().any(|i| i.scope == Scope::Command));
    // A scope it doesn't find opens All.
    h.send(Msg::Run(Action::Palette { scope: Some(Scope::Window), query: String::new() }));
    let p = window_titled(&h, "Palette");
    assert_eq!(h.program().palette.as_ref().map(|p| p.scope), Some(None));
    let texts = h.texts(p);
    assert!(texts.iter().any(|t| t == "Apps") && !texts.iter().any(|t| t == "Windows"), "the chips: {texts:?}");
}

#[test]
fn the_bell_sounds_the_alert_and_flashes_the_edge_when_asked() {
    let mut h = scene(false);
    h.send(Msg::Run(Action::None));
    let bell = |h: &mut H| {
        h.send(Msg::Ipc(IpcEvent::Event(Event::Binding { action: "bell".into(), pointer: None, output: None })))
    };
    bell(&mut h);
    assert_eq!(h.program().last_sound.as_deref(), Some("bell at 0.70"));
    assert!(h.program().flash.is_none());
    // The alert chosen in Sound, at its volume; silent at none.
    settings(&mut h, |s| {
        s.set_shell_section(
            "sound",
            &aizome_config::SoundSettings { alert_sound: "message".into(), alert_volume: 0.4, ..Default::default() },
        )
    });
    bell(&mut h);
    assert_eq!(h.program().last_sound.as_deref(), Some("message at 0.40"));
    h.program_mut().last_sound = None;
    settings(&mut h, |s| {
        s.set_shell_section("sound", &aizome_config::SoundSettings { alert_volume: 0.0, ..Default::default() })
    });
    bell(&mut h);
    assert_eq!(h.program().last_sound, None);
    // Seen as well as heard.
    settings(&mut h, |s| {
        s.set_shell_section("sound", &aizome_config::SoundSettings { visual_alert: true, ..Default::default() })
    });
    bell(&mut h);
    assert_eq!(h.program().last_sound.as_deref(), Some("bell at 0.70"));
    let f = window_titled(&h, "Flash");
    h.resize(f, Size::new(W, H));
    h.save_png(f, shots().join("flash-night.png"), 1.0);
    let (w, hh, px) = h.render_rgba(f, 1.0);
    let at = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize];
    assert_eq!(at(w / 2, hh / 2), 0, "see-through inside");
    assert_eq!(at(2, hh / 2), 255, "the edge");
    h.advance(Duration::from_millis(200));
    assert!(h.windows().iter().all(|k| h.title(*k) != "Flash"), "gone after a moment");
}

#[test]
fn the_volume_keys_play_feedback_when_sound_asks() {
    let mut h = scene(false);
    h.send(Msg::Run(Action::None));
    let key = |h: &mut H, action: &str| {
        h.send(Msg::Ipc(IpcEvent::Event(Event::Binding { action: action.into(), pointer: None, output: None })));
        h.advance(Duration::from_millis(100));
    };
    key(&mut h, "volume-up");
    assert_eq!(h.program().last_sound, None, "no feedback unless asked");
    settings(&mut h, |s| {
        s.set_shell_section("sound", &aizome_config::SoundSettings { volume_feedback: true, ..Default::default() })
    });
    key(&mut h, "volume-down");
    assert_eq!(h.program().last_sound.as_deref(), Some("audio-volume-change at 1.00"));
    // Muting makes none (the mute key is heard by its silence).
    h.program_mut().last_sound = None;
    key(&mut h, "mute");
    assert_eq!(h.program().last_sound, None);
    // Nor do the brightness keys.
    key(&mut h, "brightness-up");
    assert_eq!(h.program().last_sound, None);
}

#[test]
fn night_light_shows_where_the_compositor_can() {
    let mut h = scene(false);
    let display = |h: &H| match h.program().panel_data(aizome_shell::menubar::TrayWidget::Display) {
        Panel::Display(d) => d.night_light,
        _ => unreachable!(),
    };
    assert_eq!(display(&h), None, "nothing said yet: no switch");
    let state = aizome_ipc::NightLightState { enabled: false, outputs: vec!["eDP-1".into()], ..Default::default() };
    h.send(Msg::Ipc(IpcEvent::Event(Event::NightLightChanged { state })));
    assert_eq!(display(&h), Some(false));
    h.send(Msg::Panel(PanelMsg::NightLight(true)));
    assert_eq!(display(&h), Some(true));
    assert!(h.program().settings.night_light.enabled, "kept in the settings");
}

#[test]
fn brightness_keys_reach_only_a_built_in_panel() {
    // On a laptop's panel, the key shows its level.
    let mut h = scene(false);
    assert!(h.program().shows_builtin_panel());
    binding(&mut h, "brightness-up");
    assert!(h.program().osd.is_some(), "the brightness OSD shows on eDP-1");
    // A session on other outputs (headless, nested in another desktop)
    // leaves the machine's backlight alone, and shows nothing.
    let mut h = scene(false);
    let mut o = output();
    o.id = 2;
    o.name = "HEADLESS-1".into();
    o.connector = Some("HEADLESS-1".into());
    h.set_outputs(vec![o]);
    h.send(Msg::Nothing);
    assert!(!h.program().shows_builtin_panel());
    binding(&mut h, "brightness-up");
    assert!(h.program().osd.is_none(), "no OSD, and no backlight changed, on HEADLESS-1");
    assert!(aizome_shell::shell::builtin_panel("LVDS-1") && !aizome_shell::shell::builtin_panel("DP-2"));
}
