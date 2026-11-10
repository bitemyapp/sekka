//! Apps holding the screen on, in the shell, headless: the compositor's
//! `idle-inhibitors-changed` and the session bus's holds given by hand.
//! The cup shows for them in the bar's own colour once they have held a
//! while, and stays a moment after the last lets go; Keep Awake's stays
//! amber. Its panel lists each app once, with its reason, and says what
//! such a hold does and doesn't do.

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aizome_awake::Status;
use aizome_ipc::{Event, IdleHolder, IdleInhibitorInfo, WindowInfo};
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_shell::holders::{LINGERS, SHOWS_AFTER};
use aizome_shell::ipc::IpcEvent;
use aizome_shell::menubar::{BarMsg, Cup, Slot, TrayWidget};
use aizome_shell::shell::{Msg, Shell};
use aizome_shell::systray::panels::{Panel, PanelMsg};
use aizome_sys::apps::{AppIndex, Locale, XdgDirs};
use aizome_sys::screensaver::{Hold, Holds, Peer};
use common::*;

const MS: Duration = Duration::from_millis(1);

/// xwayland-satellite's process: an X11 app's windows are its.
const SATELLITE: i32 = 900;
const MPV: i32 = 2600;
const VLC: u32 = 2100;
const CHROME: i32 = 2400;
/// Keep Awake's service's process.
const SERVICE: u32 = 1500;
/// A script of the person's own that asks the compositor to hold as Keep
/// Awake does.
const SCRIPT: u32 = 2800;

/// The players' desktop entries, as Nixpkgs ships them.
fn players() -> AppIndex {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("aizome-shell-holders-test-{}-{n}", std::process::id()));
    let dirs = XdgDirs::rooted(&root);
    let dir = dirs.data_home.join("applications");
    std::fs::create_dir_all(&dir).unwrap();
    for (id, name, exec) in [
        ("mpv", "mpv Media Player", "mpv --player-operation-mode=pseudo-gui -- %U"),
        ("vlc", "VLC media player", "/nix/store/abc-vlc-3.0.21/bin/vlc --started-from-file %U"),
        ("google-chrome", "Google Chrome", "/nix/store/abc-google-chrome-157/bin/google-chrome-stable %U"),
    ] {
        let entry = format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n");
        std::fs::write(dir.join(format!("{id}.desktop")), entry).unwrap();
    }
    let index = AppIndex::load_with(dirs, Locale::default());
    std::fs::remove_dir_all(root).unwrap();
    index
}

/// The boards' scene with three players' windows open, their desktop
/// entries installed and their programs known, and Keep Awake's service
/// in its unit.
fn theatre(day: bool) -> Harness<Shell> {
    let mut h = scene(day);
    {
        let s = h.program_mut();
        s.apps = Some(Arc::new(players()));
        s.programs_fixture = Some(HashMap::from([
            (VLC, "/nix/store/abc-vlc-3.0.21/bin/.vlc-wrapped".to_owned()),
            (CHROME as u32, "/nix/store/abc-google-chrome-157/share/google/chrome/chrome".to_owned()),
            (SERVICE, "/nix/store/abc-aizome-0.1.0/bin/aizome-awake".to_owned()),
            (SCRIPT, "/home/ada/bin/stay-up".to_owned()),
        ]));
        s.units_fixture =
            Some(HashMap::from([(SERVICE, aizome_awake::UNIT.to_owned()), (SCRIPT, "session-3.scope".to_owned())]));
    }
    for (id, app, title, pid) in [
        (20, "mpv", "Test Pattern - mpv", MPV),
        (21, "vlc", "Test Pattern - VLC media player", SATELLITE),
        (22, "google-chrome", "A Film - Google Chrome", CHROME),
    ] {
        let window = WindowInfo {
            id,
            app_id: app.into(),
            title: title.into(),
            pid: Some(pid),
            desk: Some(3),
            output: Some("eDP-1".into()),
            ..Default::default()
        };
        h.send(Msg::Ipc(IpcEvent::Event(Event::WindowOpened { window })));
    }
    h
}

/// A window's idle inhibitor, as the compositor reports it.
fn inhibitor(window: u64, pid: i32) -> IdleInhibitorInfo {
    IdleInhibitorInfo { kind: IdleHolder::App, window: Some(window), pid: Some(pid), ..Default::default() }
}

/// The compositor says who has an idle inhibitor.
fn inhibitors(h: &mut Harness<Shell>, inhibitors: Vec<IdleInhibitorInfo>) {
    h.send(Msg::Ipc(IpcEvent::Event(Event::IdleInhibitorsChanged { inhibitors })));
}

/// The session bus's holds, as the shell's screen saver would have them:
/// (connection, process, the app's name for itself, its reason).
fn bus_holds(h: &mut Harness<Shell>, holds: &[(&str, u32, &str, &str)]) {
    let mut table = Holds::default();
    for (peer, pid, app, reason) in holds {
        table.take(Peer::new(peer).unwrap(), Some(*pid), app, reason).unwrap();
    }
    let holds: Vec<Hold> = table.state().holds;
    h.program_mut().screen_holds_changed(holds);
    h.send(Msg::Nothing);
}

fn cup(h: &Harness<Shell>) -> Option<Rect> {
    let bar = window_titled(h, "Menu bar");
    h.find_id(bar, &aizome_shell::menubar::tray_id(TrayWidget::Awake))
}

fn open_panel(h: &mut Harness<Shell>) -> WindowKey {
    let bar = window_titled(h, "Menu bar");
    let rect = cup(h).expect("the cup");
    h.send(Msg::Bar(bar, BarMsg::Press(Slot::Tray(TrayWidget::Awake), rect)));
    window_titled(h, TrayWidget::Awake.name())
}

fn says(h: &Harness<Shell>, window: WindowKey, text: &str) -> bool {
    h.texts(window).iter().any(|t| t == text)
}

/// A hold of the compositor's `keep-awake` kind, by the process that
/// asked for it.
fn keep_awake_hold(pid: u32) -> IdleInhibitorInfo {
    IdleInhibitorInfo { kind: IdleHolder::KeepAwake, pid: Some(pid as i32), ..Default::default() }
}

/// What the compositor lists while the three play: mpv's and Chrome's
/// windows' inhibitors, and the shell's own for the bus's holds.
fn three_playing() -> Vec<IdleInhibitorInfo> {
    let shells_own = IdleInhibitorInfo { pid: Some(std::process::id() as i32), ..Default::default() };
    vec![inhibitor(20, MPV), inhibitor(22, CHROME), shells_own]
}

/// mpv plays (a Wayland inhibitor on its window), VLC plays (the bus; the
/// shell holds its own inhibitor for it), and Chrome plays with the keys
/// (both ways).
fn three_play(h: &mut Harness<Shell>) {
    let chrome = "/nix/store/abc-google-chrome-157/share/google/chrome/chrome";
    bus_holds(h, &[(":1.40", VLC, "vlc", "Playing some media."), (":1.61", CHROME as u32, chrome, "Video Wake Lock")]);
    inhibitors(h, three_playing());
}

/// Keep Awake goes on beside what the compositor `listed`: logind lists
/// its locks, and its service holds at the compositor as Keep Awake.
fn keep_awake_goes_on(h: &mut Harness<Shell>, listed: Vec<IdleInhibitorInfo>) {
    h.send(Msg::Awake(Status::On));
    inhibitors(h, listed.into_iter().chain([keep_awake_hold(SERVICE)]).collect());
}

/// The rows the shell has, as `aizome-shell msg state` gives them.
fn rows(h: &Harness<Shell>) -> Vec<(String, String)> {
    let state = h.program().awake_json();
    let rows = state["holders"].as_array().expect("holders");
    rows.iter().map(|r| (r["name"].as_str().unwrap().to_owned(), r["detail"].as_str().unwrap().to_owned())).collect()
}

#[test]
fn the_cup_shows_once_a_hold_has_lasted_and_lingers_after_the_last() {
    let mut h = theatre(false);
    let bar = window_titled(&h, "Menu bar");
    assert!(cup(&h).is_none(), "a cup with nothing holding");

    inhibitors(&mut h, vec![inhibitor(20, MPV)]);
    h.advance(SHOWS_AFTER - 100 * MS);
    assert!(cup(&h).is_none(), "the cup before the hold had lasted {SHOWS_AFTER:?}");
    h.advance(200 * MS);
    assert!(cup(&h).is_some(), "no cup for a hold that lasted");
    assert!(h.find(bar, "An app is keeping the screen on").is_some(), "{:?}", h.texts(bar));
    assert_eq!(h.program().cup(), Some(Cup::Apps(1)));

    // mpv pauses: the cup stays a moment, then goes.
    inhibitors(&mut h, Vec::new());
    h.advance(LINGERS - 100 * MS);
    assert!(cup(&h).is_some(), "the cup went with the hold");
    h.advance(200 * MS);
    assert!(cup(&h).is_none(), "the cup outstayed {LINGERS:?}");
    assert_eq!(h.program().cup(), None);
}

#[test]
fn a_hold_let_go_at_once_shows_nothing_and_a_gap_blinks_nothing() {
    let mut h = theatre(false);
    // Played and paused within a second.
    inhibitors(&mut h, vec![inhibitor(20, MPV)]);
    h.advance(1000 * MS);
    inhibitors(&mut h, Vec::new());
    for _ in 0..80 {
        h.advance(100 * MS);
        assert!(cup(&h).is_none(), "a cup for a hold that lasted a second");
    }

    // VLC looping a two-second clip lets go and holds again 60 ms later
    // at each loop: the cup comes all the same, and then never blinks.
    let vlc: &[(&str, u32, &str, &str)] = &[(":1.40", VLC, "vlc", "Playing some media.")];
    bus_holds(&mut h, vlc);
    for looped in 0..6 {
        h.advance(2000 * MS);
        bus_holds(&mut h, &[]);
        let shown = cup(&h).is_some();
        h.advance(60 * MS);
        assert_eq!(cup(&h).is_some(), shown, "the cup changed in a gap");
        bus_holds(&mut h, vlc);
        if looped >= 1 {
            assert!(shown && cup(&h).is_some(), "no cup {looped} loops in");
        }
    }
    let state = h.program().awake_json();
    assert_eq!(state["cup"], "apps", "{state}");
    assert_eq!(state["holders"].as_array().map(Vec::len), Some(1), "a gap left a second row: {state}");
}

#[test]
fn the_panel_lists_each_app_once_with_its_reason() {
    let mut h = theatre(false);
    three_play(&mut h);
    h.advance(SHOWS_AFTER);
    let bar = window_titled(&h, "Menu bar");
    assert!(h.find(bar, "3 apps are keeping the screen on").is_some(), "{:?}", h.texts(bar));

    let p = open_panel(&mut h);
    let texts = h.texts(p);
    // By name, never a program's path; Chrome once, with the reason it
    // gave the bus; mpv, which gave none, with its window's title.
    for row in [
        "Google Chrome: Video Wake Lock",
        "mpv Media Player: Test Pattern - mpv",
        "VLC media player: Playing some media.",
    ] {
        assert_eq!(texts.iter().filter(|t| *t == row).count(), 1, "{row} in {texts:?}");
    }
    assert!(!texts.iter().any(|t| t.contains("/nix/store") || t.contains("An app")), "{texts:?}");
    // What these holds do, and what they don't: Keep Awake is off.
    assert!(says(&h, p, "Off: the computer can sleep as usual. The apps below keep its screen on."), "{texts:?}");
    assert!(says(&h, p, "Keeping the screen on"), "{texts:?}");
    assert!(
        says(&h, p, "These apps keep the screen on and unlocked. They don't hold off sleep or the lid."),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t.starts_with("Won't sleep") || t.contains("locks and turns")), "{texts:?}");
    // No switch but Keep Awake's own: an app's hold isn't the shell's to
    // let go.
    assert!(!texts.iter().any(|t| t.contains("Stop") || t.contains("Release") || t.contains("Ignore")), "{texts:?}");
    let data = h.program().panel_data(TrayWidget::Awake);
    assert_eq!(h.size(p).height, aizome_shell::systray::panels::size(&data).height);

    // One stops: its row stays a moment, then the panel is that much
    // shorter, and says "apps" still.
    inhibitors(&mut h, vec![inhibitor(22, CHROME)]);
    assert!(says(&h, p, "mpv Media Player: Test Pattern - mpv"), "the row went with the hold");
    h.advance(LINGERS);
    assert!(!says(&h, p, "mpv Media Player: Test Pattern - mpv"), "{:?}", h.texts(p));
    let shorter = aizome_shell::systray::panels::size(&h.program().panel_data(TrayWidget::Awake));
    assert_eq!(h.program().panel.as_ref().map(|open| open.size), Some(shorter), "the panel kept its old size");
    assert!(shorter.height < aizome_shell::systray::panels::size(&data).height);

    // Down to one, it's "This app".
    bus_holds(&mut h, &[(":1.40", VLC, "vlc", "Playing some media.")]);
    inhibitors(&mut h, Vec::new());
    h.advance(LINGERS);
    assert!(says(&h, p, "This app keeps the screen on and unlocked. It doesn't hold off sleep or the lid."));
    assert!(says(&h, p, "Off: the computer can sleep as usual. The app below keeps its screen on."));

    // The last lets go: the cup lingers, then it and its panel go.
    bus_holds(&mut h, &[]);
    assert!(h.program().panel.is_some());
    h.advance(LINGERS);
    assert!(h.program().panel.is_none(), "the panel outlived its cup");
    assert!(cup(&h).is_none());
}

#[test]
fn keep_awake_keeps_its_amber_cup_and_its_panel_shows_the_apps_as_well() {
    let mut h = theatre(false);
    let bar = window_titled(&h, "Menu bar");
    three_play(&mut h);
    h.advance(SHOWS_AFTER);
    assert_eq!(h.program().cup(), Some(Cup::Apps(3)));

    // The user's own switch: its cup, whatever apps hold. Its service's
    // hold is in the compositor's list now, and is nobody's row.
    keep_awake_goes_on(&mut h, three_playing());
    assert_eq!(h.program().cup(), Some(Cup::KeepAwake));
    assert!(h.find(bar, "Keep Awake is on").is_some(), "{:?}", h.texts(bar));
    let p = open_panel(&mut h);
    let texts = h.texts(p);
    assert!(texts.iter().any(|t| t.starts_with("Won't sleep, lock or turn the displays off")), "{texts:?}");
    assert!(says(&h, p, "VLC media player: Playing some media."), "{texts:?}");
    assert_eq!(rows(&h).len(), 3, "{:?}", rows(&h));
    assert!(
        says(&h, p, "These apps keep the screen on and unlocked. They don't hold off sleep or the lid."),
        "{texts:?}"
    );
    let data = h.program().panel_data(TrayWidget::Awake);
    assert_eq!(h.size(p).height, aizome_shell::systray::panels::size(&data).height);
    // On without its locks: red, and it says so, over the same list.
    h.send(Msg::Awake(Status::Unprotected));
    assert_eq!(h.program().cup(), Some(Cup::Unprotected));
    assert!(h.texts(p).iter().any(|t| t.starts_with("Not working")), "{:?}", h.texts(p));

    // Turned off from the panel while apps hold (its service stops, and
    // its hold at the compositor goes with it): the cup changes back to
    // theirs and the panel stays, saying what's still held.
    inhibitors(&mut h, three_playing());
    h.send(Msg::Panel(PanelMsg::KeepAwake(false)));
    assert_eq!(h.program().awake, Status::Off);
    assert_eq!(h.program().cup(), Some(Cup::Apps(3)));
    assert!(h.program().panel.is_some(), "the panel went though apps still hold");
    assert!(says(&h, p, "Off: the computer can sleep as usual. The apps below keep its screen on."));

    // With Keep Awake on, a hold that has only just begun is listed in
    // the panel at once: the wait is the cup's.
    let mut fresh = theatre(false);
    keep_awake_goes_on(&mut fresh, vec![inhibitor(20, MPV)]);
    let p = open_panel(&mut fresh);
    assert!(says(&fresh, p, "mpv Media Player: Test Pattern - mpv"), "{:?}", fresh.texts(p));
    // And off with nothing held long enough for a cup of its own, the
    // panel goes with Keep Awake's cup, as before.
    inhibitors(&mut fresh, vec![inhibitor(20, MPV)]);
    fresh.send(Msg::Panel(PanelMsg::KeepAwake(false)));
    assert!(fresh.program().panel.is_none());
    assert!(cup(&fresh).is_none());
}

#[test]
fn a_hold_as_keep_awake_is_hidden_only_when_it_is_keep_awakes_own() {
    // A script asks the compositor to hold as Keep Awake does (any
    // program of the session may), and Keep Awake is off. The screen
    // stays on, locked or not: the cup must show for it, and its panel
    // must not say the displays turn off as usual.
    let mut h = theatre(false);
    let bar = window_titled(&h, "Menu bar");
    inhibitors(&mut h, vec![keep_awake_hold(SCRIPT)]);
    assert_eq!(rows(&h), [("stay-up".to_owned(), "Also while the screen is locked".to_owned())]);
    h.advance(SHOWS_AFTER);
    assert_eq!(h.program().cup(), Some(Cup::Apps(1)));
    assert!(h.find(bar, "An app is keeping the screen on").is_some(), "{:?}", h.texts(bar));
    let p = open_panel(&mut h);
    let texts = h.texts(p);
    assert!(says(&h, p, "stay-up: Also while the screen is locked"), "{texts:?}");
    assert!(says(&h, p, "Off: the computer can sleep as usual. The app below keeps its screen on."), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("turns the displays off as usual")), "{texts:?}");
    // Its row leads to Myaku at its process, as any row does.
    assert!(h.find(p, "Show stay-up in Myaku").is_some(), "{texts:?}");

    // Keep Awake goes on beside it: Keep Awake's cup, its own service's
    // hold nobody's row, and the script's still listed.
    keep_awake_goes_on(&mut h, vec![keep_awake_hold(SCRIPT)]);
    assert_eq!(h.program().cup(), Some(Cup::KeepAwake));
    assert_eq!(rows(&h), [("stay-up".to_owned(), "Also while the screen is locked".to_owned())]);
    // The script ends: only Keep Awake is left, with no rows.
    inhibitors(&mut h, vec![keep_awake_hold(SERVICE)]);
    h.advance(LINGERS);
    assert!(rows(&h).is_empty(), "{:?}", rows(&h));
    assert_eq!(h.program().cup(), Some(Cup::KeepAwake));

    // The service's hold while the menu bar says Keep Awake is off (its
    // setting gone, its service still running) is shown like anyone's.
    let mut stray = theatre(false);
    inhibitors(&mut stray, vec![keep_awake_hold(SERVICE)]);
    assert_eq!(rows(&stray), [("aizome-awake".to_owned(), "Also while the screen is locked".to_owned())]);
    stray.advance(SHOWS_AFTER);
    assert_eq!(stray.program().cup(), Some(Cup::Apps(1)));
    // Told that Keep Awake is on after all, it's Keep Awake's.
    stray.send(Msg::Awake(Status::On));
    stray.advance(LINGERS);
    assert!(rows(&stray).is_empty(), "{:?}", rows(&stray));
}

#[test]
fn a_row_leads_to_its_app_and_never_lets_go_of_its_hold() {
    let mut h = theatre(false);
    three_play(&mut h);
    h.advance(SHOWS_AFTER);
    let p = open_panel(&mut h);
    let Panel::Awake(panel) = h.program().panel_data(TrayWidget::Awake) else { panic!("the cup's panel") };
    let row = |name: &str| panel.holders.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}"));
    // Each row's window: its own, and for VLC (an X11 app, whose window
    // is satellite's on the Wayland side) the one a switch to it shows.
    assert_eq!(row("mpv Media Player").window, Some(20));
    assert_eq!(row("VLC media player").window, Some(21));
    assert_eq!(row("Google Chrome").window, Some(22));
    // And its own process for Myaku, never satellite's.
    assert_eq!(row("VLC media player").pid.map(|pid| pid.get()), Some(VLC));
    assert_eq!(row("mpv Media Player").pid.map(|pid| pid.get()), Some(MPV as u32));

    // A click on a row goes to its window (the panel makes way), and the
    // hold is as it was.
    assert!(h.click_text(p, "VLC media player: Playing some media."));
    assert!(h.program().panel.is_none(), "the panel stayed in the window's way");
    assert_eq!(h.program().cup(), Some(Cup::Apps(3)));
    // The button at its right goes to Myaku.
    let p = open_panel(&mut h);
    assert!(h.find(p, "Show mpv Media Player in Myaku").is_some(), "{:?}", h.texts(p));
    assert!(h.click_text(p, "Show mpv Media Player in Myaku"));
    assert!(h.program().panel.is_none());
    assert_eq!(h.program().cup(), Some(Cup::Apps(3)));
}

#[test]
fn many_holders_are_counted_past_the_ones_listed() {
    let mut h = theatre(false);
    let holds: Vec<(String, u32)> = (0..9).map(|i| (format!(":1.{}", 100 + i), 5000 + i)).collect();
    let names: Vec<String> = (0..9).map(|i| format!("player-{i}")).collect();
    let rows: Vec<(&str, u32, &str, &str)> =
        holds.iter().zip(&names).map(|((peer, pid), name)| (peer.as_str(), *pid, name.as_str(), "Playing")).collect();
    bus_holds(&mut h, &rows);
    h.advance(SHOWS_AFTER);
    let bar = window_titled(&h, "Menu bar");
    assert!(h.find(bar, "9 apps are keeping the screen on").is_some(), "{:?}", h.texts(bar));
    let p = open_panel(&mut h);
    let texts = h.texts(p);
    assert_eq!(texts.iter().filter(|t| t.starts_with("player-")).count(), aizome_shell::systray::panels::HOLDERS);
    assert!(says(&h, p, "and 3 more apps"), "{texts:?}");
    let data = h.program().panel_data(TrayWidget::Awake);
    assert_eq!(h.size(p).height, aizome_shell::systray::panels::size(&data).height);
}

#[test]
fn pictures_of_the_cup_and_its_panel() {
    for day in [false, true] {
        let tone = if day { "day" } else { "night" };
        let mut h = theatre(day);
        let bar = window_titled(&h, "Menu bar");
        // One app.
        inhibitors(&mut h, vec![inhibitor(20, MPV)]);
        h.advance(SHOWS_AFTER);
        h.save_png(bar, shots().join(format!("bar-cup-apps-{tone}.png")), 2.0);
        let p = open_panel(&mut h);
        h.save_png(p, shots().join(format!("panel-cup-one-app-{tone}.png")), 2.0);
        h.send(Msg::Panel(PanelMsg::Close));
        // Several.
        three_play(&mut h);
        h.advance(LINGERS);
        let p = open_panel(&mut h);
        h.save_png(p, shots().join(format!("panel-cup-apps-{tone}.png")), 2.0);
        // Keep Awake on as well: its amber cup.
        keep_awake_goes_on(&mut h, three_playing());
        h.save_png(p, shots().join(format!("panel-cup-keep-awake-and-apps-{tone}.png")), 2.0);
        h.send(Msg::Panel(PanelMsg::Close));
        h.save_png(bar, shots().join(format!("bar-cup-keep-awake-{tone}.png")), 2.0);
        // A program that holds as Keep Awake does, beside a player, with
        // Keep Awake off.
        let mut h = theatre(day);
        inhibitors(&mut h, vec![inhibitor(20, MPV), keep_awake_hold(SCRIPT)]);
        h.advance(SHOWS_AFTER);
        let p = open_panel(&mut h);
        h.save_png(p, shots().join(format!("panel-cup-locked-too-{tone}.png")), 2.0);
    }
}
