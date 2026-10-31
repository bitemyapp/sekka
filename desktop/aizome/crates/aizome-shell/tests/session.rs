//! The session's dialogs and the notifications' cards, headless.

mod common;

use std::time::Duration;

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_shell::actions::{Action, SessionVerb};
use aizome_shell::confirm::ConfirmMsg;
use aizome_shell::notifications::cards::CardMsg;
use aizome_shell::notifications::server::NoteEvent;
use aizome_shell::notifications::store::{Incoming, Urgency};
use aizome_shell::shell::Msg;
use common::*;

#[test]
fn shutting_down_counts_down_a_minute() {
    let mut h = scene(false);
    h.send(Msg::Run(Action::Session(SessionVerb::ShutDown)));
    let c = window_titled(&h, "Confirm");
    assert!(h.texts(c).iter().any(|t| t.contains("in 60 seconds")));
    h.advance(Duration::from_secs(30));
    assert!(h.texts(c).iter().any(|t| t.contains("in 30 seconds")), "{:?}", h.texts(c));
    assert_eq!(h.program().session_done, None);
    h.advance(Duration::from_secs(31));
    assert_eq!(h.program().session_done, Some(SessionVerb::ShutDown));
    assert!(h.windows().iter().all(|k| h.title(*k) != "Confirm"));
}

#[test]
fn cancelling_asks_nothing_more() {
    let mut h = scene(false);
    h.send(Msg::Run(Action::Session(SessionVerb::LogOut)));
    h.send(Msg::Confirm(ConfirmMsg::Cancel));
    h.advance(Duration::from_secs(90));
    assert_eq!(h.program().session_done, None);
    h.send(Msg::Run(Action::Session(SessionVerb::Restart)));
    let c = window_titled(&h, "Confirm");
    h.named(c, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().session_done, Some(SessionVerb::Restart), "Return does it now");
}

#[test]
fn desks_rename_in_place() {
    let mut h = scene(false);
    h.send(Msg::Run(Action::RenameDesk { index: 2 }));
    let bar = window_titled(&h, "Menu bar");
    h.named(bar, NamedKey::Backspace, Modifiers::SUPER);
    h.type_text(bar, "Ship");
    assert_eq!(h.program().renaming.as_ref().map(|r| r.1.as_str()), Some("Ship"));
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().renaming.is_none());
}

#[test]
fn critical_notifications_stay_as_cards() {
    let mut h = scene(false);
    let n = Incoming {
        app_name: "Power".into(),
        summary: "Battery low".into(),
        body: "Plug in soon: 6% left".into(),
        urgency: Urgency::Critical,
        actions: vec![("settings".into(), "Battery Settings".into())],
        ..Default::default()
    };
    h.send(Msg::Note(NoteEvent::Notify(40, Box::new(n))));
    let c = window_titled(&h, "Notifications");
    assert!(h.texts(c).iter().any(|t| t.contains("Battery low")));
    h.advance(Duration::from_secs(10));
    assert!(h.windows().iter().any(|k| h.title(*k) == "Notifications"), "it stays");
    h.save_png(c, shots().join("cards-night.png"), 2.0);
    h.send(Msg::Card(CardMsg::Dismiss(40)));
    assert!(h.windows().iter().all(|k| h.title(*k) != "Notifications"));
}

#[test]
fn screenshots_say_where_they_went() {
    let mut h = scene(false);
    h.send(Msg::Ipc(aizome_shell::ipc::IpcEvent::Event(aizome_ipc::Event::ScreenshotTaken {
        path: "/home/u/Pictures/Screenshots/a.png".into(),
    })));
    let id = h.program().banner.expect("shown in the bar");
    let n = h.program().notes.get(id).unwrap().clone();
    assert_eq!(n.summary, "Screenshot saved");
    assert!(n.buttons().any(|(_, l)| l == "Show in Hako"));
}

#[test]
fn keyboards_and_receivers_use_existing_settings_without_setup_notifications() {
    use aizome_ipc::{KeyboardBus, KeyboardInfo};
    let mut h = scene(false);
    let added = |name: &str, id: &str, bus| {
        let keyboard = KeyboardInfo { name: name.into(), id: id.into(), bus, last: false };
        Msg::Ipc(aizome_shell::ipc::IpcEvent::Event(aizome_ipc::Event::KeyboardAdded { keyboard }))
    };
    h.send(added("AT Translated Set 2 keyboard", "0001:0001", KeyboardBus::BuiltIn));
    h.send(added("QEMU Virtio Keyboard", "0000:0000", KeyboardBus::Other));
    assert_eq!(h.program().banner, None, "the computer's own, or a VM's");

    h.program_mut().settings.input.keyboards.push(aizome_config::KeyboardProfile {
        name: "Topre REALFORCE R4H".into(),
        id: "0853:0321".into(),
        options: Some("altwin:swap_lalt_lwin".into()),
        ..Default::default()
    });
    let settings = h.program().settings.clone();
    let notes = h.program().notes.list.clone();
    for _ in 0..2 {
        h.send(added("Apple Inc. Magic Keyboard", "05ac:029c", KeyboardBus::Bluetooth));
        h.send(added("Topre REALFORCE R4H", "0853:0321", KeyboardBus::Usb));
        h.send(added("Topre REALFORCE R4H Keyboard", "0853:0321", KeyboardBus::Usb));
        h.send(added("Razer DeathAdder V4 Pro", "1532:00bf", KeyboardBus::Usb));
        h.send(added("Razer DeathAdder V4 Pro Keyboard", "1532:00bf", KeyboardBus::Usb));
        h.send(added("Yubico YubiKey OTP+FIDO+CCID", "1050:0407", KeyboardBus::Usb));
        h.send(Msg::Ipc(aizome_shell::ipc::IpcEvent::Connected(Box::new(aizome_shell::ipc::SnapshotBox(snapshot())))));
    }
    assert_eq!(h.program().banner, None);
    assert_eq!(h.program().notes.list, notes);
    assert_eq!(h.program().settings, settings, "connecting a device doesn't write profiles");
}

#[test]
fn a_replaced_shell_cannot_restore_retired_keyboard_offers_in_its_final_handoff() {
    use aizome_shell::carry::Carried;
    use aizome_shell::notifications::store::Notification;
    let mut h = scene(false);
    let old = Carried {
        notes: vec![
            Notification {
                id: 900,
                app_name: "Settei".into(),
                summary: "Set up Topre REALFORCE R4H Keyboard?".into(),
                body: "Press the key beside its space bar, and Settei suggests where ❖ goes on it.".into(),
                actions: vec![
                    ("default".into(), "Set Up…".into()),
                    ("set-up".into(), "Set Up…".into()),
                    ("same".into(), "Same as Others".into()),
                ],
                ..Default::default()
            },
            Notification {
                id: 901,
                app_name: "Settei".into(),
                summary: "Other settings advice".into(),
                ..Default::default()
            },
        ],
        next_id: 901,
        ..Default::default()
    };
    for _ in 0..2 {
        h.send(Msg::Carried(Box::new(old.clone())));
        assert!(h.program().notes.get(900).is_none());
        assert_eq!(h.program().notes.get(901), Some(&old.notes[1]));
    }
}

#[test]
fn the_bar_goes_back_to_the_menus_five_seconds_after_the_last_notification() {
    let mut h = scene(false);
    let note = |s: &str| Box::new(Incoming { app_name: "Mail".into(), summary: s.into(), ..Default::default() });
    h.send(Msg::Note(NoteEvent::Notify(50, note("one"))));
    h.advance(Duration::from_secs(3));
    h.send(Msg::Note(NoteEvent::Notify(51, note("two"))));
    assert_eq!(h.program().banner, Some(51), "the newer one takes the line");
    h.advance(Duration::from_millis(2500));
    assert_eq!(h.program().banner, Some(51), "the first's time running out doesn't end the second's");
    h.advance(Duration::from_millis(2600));
    assert_eq!(h.program().banner, None, "five seconds after the last, the menus are back");
    let bar = window_titled(&h, "Menu bar");
    assert!(h.texts(bar).iter().any(|t| t == "Format"));
}

#[test]
fn palette_commands_wear_their_apps_glyph() {
    let h = scene(false);
    let items = h.program().palette_items();
    let bold = items.iter().find(|i| i.title == "Bold").unwrap();
    assert_eq!(bold.glyph, aizome_kit::Icon::Code, "Sumi's glyph, not ⌘");
}
