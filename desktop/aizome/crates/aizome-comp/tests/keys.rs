//! Keys: the command key (❖) translated to Ctrl (Ctrl+Shift in
//! terminals) for apps that don't use Aizome's kit, the system's chords
//! kept for the compositor, and shell bindings sent to subscribers.

mod common;

use std::time::Duration;

use aizome_ipc::{Event, Request, Synthetic};
use common::*;

const SHIFT: u32 = 1;
const CTRL: u32 = 4;
const LOGO: u32 = 64;
const KEY_A: u32 = 30;
const KEY_C: u32 = 46;

fn chord(c: &Compositor, keys: &str) {
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: keys.into() }] });
}

#[test]
fn super_becomes_ctrl_for_other_apps() {
    for app_id in ["org.example.editor", "org.calamares.Other", "org.calamares.NixOSRust.Other"] {
        let c = Compositor::start(Options::default());
        let a = c.client(Spec::new(app_id, 0x2222cc));
        c.wait_window(app_id);
        a.wait_seen("keyboard focus", |s| s.keyboard_focus);
        chord(&c, "super+c");
        let s = a.wait_seen("❖C as Ctrl+C", |s| s.keys.iter().any(|(k, _)| *k == KEY_C));
        let (_, mods) = s.keys.iter().find(|(k, _)| *k == KEY_C).unwrap();
        assert_eq!(*mods & (CTRL | LOGO | SHIFT), CTRL, "{app_id}: the app saw Ctrl, not Super: {mods:#x}");
        // After the chord, nothing stays held.
        a.wait_seen("modifiers released", |s| s.mods & (CTRL | LOGO) == 0);
    }
}

#[test]
fn terminals_get_ctrl_shift() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("foot", 0x2222cc));
    c.wait_window("foot");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    chord(&c, "super+c");
    let s = a.wait_seen("❖C as Ctrl+Shift+C", |s| s.keys.iter().any(|(k, _)| *k == KEY_C));
    let (_, mods) = s.keys.iter().find(|(k, _)| *k == KEY_C).unwrap();
    assert_eq!(*mods & (CTRL | LOGO | SHIFT), CTRL | SHIFT, "{mods:#x}");
}

#[test]
fn kit_apps_keep_super() {
    // Genkan uses the kit while preserving its pre-kit desktop identity.
    for app_id in ["org.aizome.Soroban", "org.calamares.NixOSRust"] {
        let c = Compositor::start(Options::default());
        let a = c.client(Spec::new(app_id, 0x2222cc));
        c.wait_window(app_id);
        a.wait_seen("keyboard focus", |s| s.keyboard_focus);
        for (keys, code) in [("super+a", KEY_A), ("super+c", KEY_C)] {
            chord(&c, keys);
            let s = a.wait_seen("command key unchanged", |s| s.keys.iter().any(|(k, _)| *k == code));
            let (_, mods) = s.keys.iter().find(|(k, _)| *k == code).unwrap();
            assert_eq!(*mods & (CTRL | LOGO | SHIFT), LOGO, "{app_id}: {keys}: {mods:#x}");
            a.wait_seen("modifiers released", |s| s.mods & (CTRL | LOGO) == 0);
        }
    }
}

#[test]
fn the_setting_turns_translation_off() {
    let c = Compositor::start(Options {
        settings: "[appearance]\neffects = \"austere\"\n[keys]\ncommand_key_for_all_apps = false\n",
        ..Default::default()
    });
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    chord(&c, "super+c");
    let s = a.wait_seen("❖C as is", |s| s.keys.iter().any(|(k, _)| *k == KEY_C));
    let (_, mods) = s.keys.iter().find(|(k, _)| *k == KEY_C).unwrap();
    assert_eq!(*mods & (CTRL | LOGO), LOGO);
}

#[test]
fn system_chords_stay_with_the_system_and_shell_ones_go_to_subscribers() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    // ❖Space: the shell's palette.
    chord(&c, "super+space");
    let got = events.by_ref().find_map(|e| match e {
        Ok(Event::Binding { action, .. }) => Some(action),
        _ => None,
    });
    assert_eq!(got.as_deref(), Some("palette"));
    // ⌥❖M: the menu bar, and the power key: the shell's. (F10 stays with
    // this app: it isn't one of Aizome's.)
    // ⌥Print, recording: the shell's too.
    for (keys, want) in
        [("f10", None), ("super+alt+m", Some("menu-bar")), ("power", Some("power-key")), ("alt+print", Some("record"))]
    {
        chord(&c, keys);
        if let Some(want) = want {
            let got = events.by_ref().find_map(|e| match e {
                Ok(Event::Binding { action, .. }) => Some(action),
                _ => None,
            });
            assert_eq!(got.as_deref(), Some(want), "{keys}");
        }
    }
    a.wait_seen("F10 in the app", |s| s.keys.iter().any(|(k, _)| *k == 68));
    // ⌥❖↑ fills: the compositor's, and the app sees no key.
    chord(&c, "alt+super+up");
    c.wait_windows("filled", |ws| ws[0].fill);
    // ⌃❖2 goes to Desk 2.
    chord(&c, "ctrl+super+2");
    let aizome_ipc::Response::Desks { current, .. } = c.request(Request::Desks) else { panic!() };
    assert_eq!(current, 2);
    let s = a.seen();
    assert!(
        !s.keys.iter().any(|(k, _)| *k == 57 || *k == 103 || *k == 3 || *k == 50 || *k == 116),
        "no system chord reached the app: {:?}",
        s.keys
    );
    let _ = w;
}

#[test]
fn super_shift_brackets_switch_a_groups_tabs_and_ctrl_carries_them() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("org.example.a", 0x2222cc));
    let wa = c.wait_window("org.example.a");
    let b = c.client(Spec::new("org.example.b", 0x22cc22));
    let wb = c.wait_window("org.example.b");
    c.ok(Request::Stack { a: wa.id, b: wb.id });
    let shows = |id| move |ws: &[aizome_ipc::WindowInfo]| ws.iter().any(|w| w.id == id && w.group_active && w.focused);
    c.wait_windows("b shows", shows(wb.id));
    // ❖⇧] from the last tab wraps round to the first; ❖⇧[ goes back.
    chord(&c, "shift+super+]");
    c.wait_windows("❖⇧] shows a", shows(wa.id));
    chord(&c, "shift+super+[");
    c.wait_windows("❖⇧[ shows b", shows(wb.id));
    // ⌃⇧❖[ moves the showing tab left, stopping first; ⌃⇧❖] takes it back.
    let order = |want: Vec<u64>| move |ws: &[aizome_ipc::WindowInfo]| ws.iter().all(|w| w.group_members == want);
    chord(&c, "ctrl+shift+super+[");
    c.wait_windows("b first", order(vec![wb.id, wa.id]));
    c.settle();
    keep(&c.screenshot(), "tab-moved-by-keys");
    chord(&c, "ctrl+shift+super+[");
    chord(&c, "ctrl+shift+super+]");
    c.wait_windows("b last again, still showing", |ws| order(vec![wa.id, wb.id])(ws) && shows(wb.id)(ws));
    // ⌃⇧❖S takes it out of the group; ⌃❖S stacks it with the one behind.
    chord(&c, "ctrl+shift+super+s");
    c.wait_windows("unstacked", |ws| ws.iter().all(|w| w.group.is_none()));
    chord(&c, "ctrl+super+s");
    c.wait_windows("stacked again", |ws| ws.iter().all(|w| w.group.is_some()) && shows(wb.id)(ws));
    // The chords stayed with the system (not Ctrl+[ or Ctrl+S for the apps).
    for s in [a.seen(), b.seen()] {
        assert!(!s.keys.iter().any(|(k, _)| [26, 27, 31].contains(k)), "a chord reached an app: {:?}", s.keys);
    }
}

#[test]
fn f10_opens_the_menu_bar_over_aizomes_apps_and_the_desktop() {
    let c = Compositor::start(Options::default());
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let next_binding = |events: &mut aizome_ipc::Events| {
        events.by_ref().find_map(|e| match e {
            Ok(Event::Binding { action, .. }) => Some(action),
            _ => None,
        })
    };
    // Nothing focused: the desktop.
    chord(&c, "f10");
    assert_eq!(next_binding(&mut events).as_deref(), Some("menu-bar"));
    // Kit apps, including Genkan's preserved desktop identity.
    for app_id in ["org.aizome.Soroban", "org.calamares.NixOSRust"] {
        let a = c.client(Spec::new(app_id, 0x2222cc));
        c.wait_window(app_id);
        a.wait_seen("keyboard focus", |s| s.keyboard_focus);
        chord(&c, "f10");
        assert_eq!(next_binding(&mut events).as_deref(), Some("menu-bar"), "{app_id}");
        assert!(!a.seen().keys.iter().any(|(k, _)| *k == 68), "{app_id}");
    }
}

#[test]
fn the_system_bell_rings_for_the_shell() {
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_registry;
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols::xdg::system_bell::v1::client::xdg_system_bell_v1;
    struct B;
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for B {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<xdg_system_bell_v1::XdgSystemBellV1, ()> for B {
        fn event(
            _: &mut Self,
            _: &xdg_system_bell_v1::XdgSystemBellV1,
            _: xdg_system_bell_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    let c = Compositor::start(Options::default());
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let conn =
        Connection::from_socket(std::os::unix::net::UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
    let (globals, mut queue) = registry_queue_init::<B>(&conn).unwrap();
    let bell: xdg_system_bell_v1::XdgSystemBellV1 =
        globals.bind(&queue.handle(), 1..=1, ()).expect("xdg_system_bell_v1");
    bell.ring(None);
    queue.roundtrip(&mut B).unwrap();
    let got = events.by_ref().find_map(|e| match e {
        Ok(Event::Binding { action, output, .. }) => Some((action, output)),
        _ => None,
    });
    assert_eq!(got, Some(("bell".to_string(), Some("HEADLESS-1".to_string()))));
}

#[test]
fn an_app_keeps_its_home_key_over_a_window_it_opened() {
    const KEY_HOME: u32 = 102;
    const KEY_F1: u32 = 59;
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("org.example.browser", 0x2222cc));
    let wa = c.wait_window("org.example.browser");
    let b = c.client(Spec::new("org.example.other", 0xcc2222));
    let wb = c.wait_window("org.example.other");
    let focus = |id: u64, who: &TestClient| {
        c.ok(Request::Focus { id });
        who.wait_seen("keyboard focus", |s| s.keyboard_focus);
    };
    let tap = |key: &str| {
        let key = |pressed| Synthetic::Key { key: key.into(), pressed };
        c.ok(Request::Synthesize { events: vec![key(true), key(false)] });
    };
    let caught = |events: &mut aizome_ipc::Events| {
        events.by_ref().find_map(|e| match e {
            Ok(Event::KeyCaught { tag, code, .. }) => Some((tag, code)),
            _ => None,
        })
    };
    let home = |windows: Vec<u64>| Request::CatchKeys {
        tag: "home".into(),
        keys: if windows.is_empty() { Vec::new() } else { vec!["home".into(), "f1".into()] },
        windows,
    };

    // Asked on a connection that doesn't listen, nobody would hear it.
    let mut plain = aizome_ipc::Client::connect_to(&c.ipc).unwrap();
    assert!(matches!(plain.request(&home(vec![wa.id])).unwrap(), aizome_ipc::Response::Error { .. }));
    let mut keeper = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    keeper.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let bad = Request::CatchKeys { tag: "home".into(), keys: vec!["no-such-key".into()], windows: vec![wa.id] };
    assert!(matches!(keeper.request(&bad).unwrap(), aizome_ipc::Response::Error { .. }));
    assert_eq!(keeper.request(&home(vec![wa.id])).unwrap(), aizome_ipc::Response::Ok);

    // Over the window: Home and F1 come to the keeper alone; other keys,
    // and Home with Ctrl, are the window's.
    focus(wa.id, &a);
    tap("home");
    assert_eq!(caught(&mut keeper), Some(("home".into(), KEY_HOME)));
    tap("f1");
    assert_eq!(caught(&mut keeper), Some(("home".into(), KEY_F1)));
    tap("a");
    chord(&c, "ctrl+home");
    let s = a.wait_seen("Ctrl+Home", |s| s.keys.iter().any(|(k, _)| *k == KEY_HOME));
    assert!(s.keys.iter().any(|(k, _)| *k == KEY_A), "{:?}", s.keys);
    assert!(!s.keys.iter().any(|(k, _)| *k == KEY_F1), "F1 was caught: {:?}", s.keys);
    let homes: Vec<u32> = s.keys.iter().filter(|(k, _)| *k == KEY_HOME).map(|(_, m)| *m).collect();
    assert_eq!(homes.len(), 1, "Home alone was caught: {:?}", s.keys);
    assert_ne!(homes[0] & CTRL, 0, "the one that came held Ctrl");

    // Another window with the keys: Home is its own.
    focus(wb.id, &b);
    tap("home");
    b.wait_seen("Home", |s| s.keys.iter().any(|(k, _)| *k == KEY_HOME));

    // Let go: Home is the window's again.
    focus(wa.id, &a);
    assert_eq!(keeper.request(&home(Vec::new())).unwrap(), aizome_ipc::Response::Ok);
    tap("home");
    a.wait_seen("Home let go", |s| s.keys.iter().filter(|(k, _)| *k == KEY_HOME).count() == 2);

    // The keeper gone (Chanoma quit), nothing is caught.
    assert_eq!(keeper.request(&home(vec![wa.id])).unwrap(), aizome_ipc::Response::Ok);
    drop(keeper);
    c.settle();
    tap("f1");
    a.wait_seen("F1", |s| s.keys.iter().any(|(k, _)| *k == KEY_F1));
}
