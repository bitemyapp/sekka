//! Apps keeping the screen awake over a private bus: one calling KDE's
//! path and leaving without a word, as VLC does when it's killed; one
//! calling the specification's path and giving its cookie back, as
//! Chromium does; a cookie that isn't the caller's; and who may take the
//! name.

mod common;

use aizome_sys::screensaver::{KDE_PATH, NAME, PATH, ScreenSaver};
use aizome_sys::{Takeover, Wake};
use common::eventually;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use zbus::blocking::Connection;

fn app(bus: &common::TestBus) -> Connection {
    bus.builder().build().expect("an app's connection")
}

fn inhibit(app: &Connection, path: &str, name: &str, reason: &str) -> u32 {
    let reply = app.call_method(Some(NAME), path, Some(NAME), "Inhibit", &(name, reason)).expect("Inhibit");
    reply.body().deserialize().expect("a cookie")
}

fn uninhibit(app: &Connection, path: &str, cookie: u32) {
    app.call_method(Some(NAME), path, Some(NAME), "UnInhibit", &(cookie,)).expect("UnInhibit");
}

fn apps(saver: &ScreenSaver) -> Vec<String> {
    saver.snapshot().holds.iter().map(|h| h.app.clone()).collect()
}

#[test]
fn a_hold_lasts_until_its_app_gives_the_cookie_back() {
    let bus = test_bus!();
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = woken.clone();
    let wake = Wake::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let saver = ScreenSaver::start(&bus.bus(), wake, Takeover::Any).expect("serve the screen saver");
    assert!(saver.available());
    assert!(!saver.snapshot().held());

    let chromium = app(&bus);
    let cookie = inhibit(&chromium, PATH, "chromium", "Video playback");
    assert_ne!(cookie, 0);
    assert!(eventually(|| saver.snapshot().held()), "no hold after Inhibit");
    let state = saver.snapshot();
    assert_eq!(state.holds.len(), 1);
    assert_eq!(state.holds[0].cookie.get(), cookie);
    assert_eq!(state.holds[0].peer.as_str(), chromium.unique_name().unwrap().as_str());
    assert_eq!((state.holds[0].app.as_str(), state.holds[0].reason.as_str()), ("chromium", "Video playback"));
    assert!(woken.load(Ordering::SeqCst) >= 1, "the shell wasn't woken for the hold");

    // A second video in the same app: its own hold.
    let second = inhibit(&chromium, PATH, "chromium", "Video playback");
    assert_ne!(second, cookie);
    assert!(eventually(|| saver.snapshot().holds.len() == 2));
    uninhibit(&chromium, PATH, cookie);
    assert!(eventually(|| saver.snapshot().holds.len() == 1));
    assert!(saver.snapshot().held(), "the second video still plays");
    // Given back twice, and a cookie nobody has: nothing, and no error.
    uninhibit(&chromium, PATH, cookie);
    uninhibit(&chromium, PATH, 0);
    uninhibit(&chromium, PATH, 4242);
    assert_eq!(saver.snapshot().holds.len(), 1);
    let before = woken.load(Ordering::SeqCst);
    uninhibit(&chromium, PATH, second);
    assert!(eventually(|| !saver.snapshot().held()), "held after the last cookie came back");
    assert!(eventually(|| woken.load(Ordering::SeqCst) > before), "the shell wasn't woken when the hold went");
}

#[test]
fn an_app_that_leaves_the_bus_takes_its_holds_with_it() {
    let bus = test_bus!();
    let saver = ScreenSaver::start(&bus.bus(), Wake::none(), Takeover::Any).expect("serve the screen saver");

    // VLC's way: KDE's path, on a connection of its own for this alone.
    let vlc = app(&bus);
    let cookie = inhibit(&vlc, KDE_PATH, "vlc", "Playing some media.");
    let mpv_like = app(&bus);
    inhibit(&mpv_like, PATH, "another player", "playing");
    assert!(eventually(|| saver.snapshot().holds.len() == 2));
    assert_eq!(saver.snapshot().holds[0].cookie.get(), cookie);

    // Killed, it says nothing: its connection just goes.
    drop(vlc);
    assert!(eventually(|| apps(&saver) == ["another player"]), "a hold outlived its app: {:?}", apps(&saver));
    drop(mpv_like);
    assert!(eventually(|| !saver.snapshot().held()), "a hold outlived its app: {:?}", apps(&saver));
}

#[test]
fn an_app_that_takes_a_hold_and_leaves_at_once_leaves_none() {
    let bus = test_bus!();
    let saver = ScreenSaver::start(&bus.bus(), Wake::none(), Takeover::Any).expect("serve the screen saver");
    // The call and the connection's leaving reach the service on separate
    // queues: whichever it sees first, no hold may stay.
    for _ in 0..50 {
        let app = app(&bus);
        inhibit(&app, PATH, "a script", "one call");
        drop(app);
    }
    assert!(eventually(|| !saver.snapshot().held()), "holds without an app: {:?}", apps(&saver));
}

#[test]
fn another_apps_cookie_gives_nothing_back() {
    let bus = test_bus!();
    let saver = ScreenSaver::start(&bus.bus(), Wake::none(), Takeover::Any).expect("serve the screen saver");
    let player = app(&bus);
    let cookie = inhibit(&player, PATH, "a player", "playing");
    assert!(eventually(|| saver.snapshot().held()));
    let other = app(&bus);
    uninhibit(&other, PATH, cookie);
    uninhibit(&other, KDE_PATH, cookie);
    // The player's own word, at either path, does.
    let later = inhibit(&other, PATH, "another app", "to see it's served");
    assert!(eventually(|| saver.snapshot().holds.len() == 2), "the player's hold went with another's word");
    uninhibit(&other, PATH, later);
    uninhibit(&player, KDE_PATH, cookie);
    assert!(eventually(|| !saver.snapshot().held()));
}

#[test]
fn what_it_does_not_serve_is_an_unknown_method() {
    let bus = test_bus!();
    let _saver = ScreenSaver::start(&bus.bus(), Wake::none(), Takeover::Any).expect("serve the screen saver");
    let app = app(&bus);
    let error = app.call_method(Some(NAME), PATH, Some(NAME), "GetActive", &()).expect_err("GetActive isn't served");
    assert!(error.to_string().contains("UnknownMethod"), "{error}");
}

#[test]
fn a_restarted_shell_takes_the_name_over_and_a_headless_one_only_from_the_shell_it_replaces() {
    let bus = test_bus!();
    let owner = || {
        let dbus = zbus::blocking::fdo::DBusProxy::new(&bus.builder().build().unwrap()).unwrap();
        dbus.get_name_owner(NAME.try_into().unwrap()).unwrap().to_string()
    };
    let connect = || aizome_sys::Bus::at(&bus.address).unwrap();
    let desktop = ScreenSaver::start(&bus.bus(), Wake::none(), Takeover::Any).expect("serve the screen saver");
    let held = owner();
    // A test or nested shell on the person's bus leaves their desktop's
    // screen saver alone, whoever else it would replace.
    for takeover in [Takeover::Free, Takeover::From(std::process::id().wrapping_add(1))] {
        assert!(ScreenSaver::start(&connect(), Wake::none(), takeover).is_err(), "{takeover:?}");
        assert_eq!(owner(), held, "{takeover:?}");
    }
    let player = app(&bus);
    let old = inhibit(&player, KDE_PATH, "a player", "playing");
    assert!(eventually(|| desktop.snapshot().held()));
    // The shell that replaces it takes the name, and apps reach the new
    // one. The old one forgets its holds: nobody can give a cookie back to
    // it, so it would keep the screen awake for as long as it lived.
    let next = connect();
    let second = ScreenSaver::start(&next, Wake::none(), Takeover::From(std::process::id())).expect("take over");
    assert_eq!(Some(owner()), next.unique_name());
    assert!(eventually(|| !desktop.snapshot().held()), "the replaced service kept a hold");
    uninhibit(&player, KDE_PATH, old);
    assert!(!second.snapshot().held(), "the new service starts with no holds");
    inhibit(&player, KDE_PATH, "a player", "playing");
    assert!(eventually(|| second.snapshot().held()));
    assert!(!desktop.snapshot().held());
    drop(desktop);
    drop(second);
    // With nobody serving, an app hears so.
    assert!(player.call_method(Some(NAME), PATH, Some(NAME), "Inhibit", &("a player", "playing")).is_err());
}
