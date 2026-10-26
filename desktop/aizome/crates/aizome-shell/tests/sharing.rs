//! Screen and window sharing in the shell, headless (the compositor's
//! `shares-changed` sent by hand, which reports a share once it has
//! lasted a second): the Deskbar's indicator in the accent, its menu with
//! what's shared, and the indicator gone when sharing stops.

mod common;

use std::time::Duration;

use aizome_ipc::{Event, ShareInfo};
use aizome_shell::ipc::IpcEvent;
use aizome_shell::shell::Msg;
use common::*;

fn shares(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, shares: Vec<ShareInfo>) {
    h.send(Msg::Ipc(IpcEvent::Event(Event::SharesChanged { shares })));
}

fn screen() -> ShareInfo {
    ShareInfo { id: 1, output: Some("eDP-1".into()), window: None, cursor: true }
}

/// Lets the bar catch up, on the harness's timer and the wall clock both.
fn settle(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>) {
    std::thread::sleep(aizome_shell::sharing::SETTLE + Duration::from_millis(30));
    h.advance(aizome_shell::sharing::SETTLE + Duration::from_millis(30));
}

#[test]
fn the_indicator_shows_whats_shared() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    shares(&mut h, vec![screen()]);
    settle(&mut h);
    let chip = h.find(bar, "Sharing the screen").expect("the indicator");
    // In the systray, left of the clock.
    let clock = h.find(bar, "21:42").unwrap();
    assert!(chip.x1 < clock.x0, "{chip:?} {clock:?}");
    let state = h.program().sharing_json();
    assert_eq!(state[0]["what"], "Built-in Display", "{state}");
    // A press opens what's shared, with Stop Sharing.
    assert!(h.click_text(bar, "Sharing the screen"));
    assert_eq!(h.popups(bar).len(), 1, "the Sharing menu");
    // Sharing stopped: the indicator goes at once.
    shares(&mut h, Vec::new());
    assert!(h.find(bar, "Sharing the screen").is_none());
}

#[test]
fn nothing_shared_shows_nothing() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    shares(&mut h, Vec::new());
    settle(&mut h);
    assert!(h.find(bar, "Sharing the screen").is_none());
    assert_eq!(h.program().sharing_json(), serde_json::json!([]));
}

#[test]
fn several_shares_are_counted() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let window = ShareInfo { id: 2, output: None, window: Some(5), cursor: false };
    shares(&mut h, vec![screen(), window]);
    settle(&mut h);
    assert!(h.find(bar, "Sharing 2 things").is_some(), "{:?}", h.texts(bar));
}

#[test]
fn pictures_of_the_indicator() {
    for day in [false, true] {
        let mut h = scene(day);
        shares(&mut h, vec![screen()]);
        settle(&mut h);
        let bar = window_titled(&h, "Menu bar");
        let name = if day { "sharing-day.png" } else { "sharing-night.png" };
        h.save_png(bar, shots().join(name), 2.0);
    }
}
