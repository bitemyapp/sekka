//! Keys: the command key (❖) translated to Ctrl (Ctrl+Shift in
//! terminals) for apps that don't use Aizome's kit, the system's chords
//! kept for the compositor, shell bindings sent to subscribers, and
//! synthesized chords pressed as the keyboard that typed last has them.

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
fn third_party_kit_apps_receive_command_shortcuts_without_reserving_their_app_id() {
    const APP_ID: &str = "org.example.Test";
    let c = Compositor::start(Options::default());
    let marker = c.dir.join("command-performed");
    let exe = std::env::current_exe().unwrap();
    let mut app = desktop::Program(
        desktop::command(&c, exe.to_str().unwrap())
            .args(["--exact", "third_party_kit_client", "--ignored", "--nocapture", "--test-threads=1"])
            .env("AIZOME_KEYS_TEST_MARKER", &marker)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(c.dir.join("kit-client.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    c.wait_windows("the third-party kit window has focus", |ws| ws.iter().any(|w| w.app_id == APP_ID && w.focused));
    // Exercise the real kit's startup and menu matching, not a client that
    // announces itself only because the test told it to. There is no shell:
    // command shortcuts must also work before it starts or while it restarts.
    chord(&c, "super+z");
    desktop::wait("the third-party kit's command menu action", || {
        assert!(app.0.try_wait().unwrap().is_none(), "the kit client exited; see {}", c.dir.display());
        marker.exists()
    });
    assert!(c.window(APP_ID).unwrap().kit_app, "the window snapshot recognizes the kit connection");

    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    chord(&c, "f10");
    let binding = events.find_map(|e| match e.expect("the compositor's F10 binding") {
        Event::Binding { action, .. } => Some(action),
        _ => None,
    });
    assert_eq!(binding.as_deref(), Some("menu-bar"), "F10 recognizes third-party kit apps too");

    drop(app);
    c.wait_windows("the kit process's window closed", |ws| ws.is_empty());
    // A different process using the same ID is still an ordinary client.
    // Neither its name nor a previous connection grants the kit's policy.
    let other = c.client(Spec::new(APP_ID, 0x2222cc));
    let w = c.wait_window(APP_ID);
    assert!(!w.kit_app, "a disconnected kit app must not reserve its app ID");
    other.wait_seen("ordinary client keyboard focus", |s| s.keyboard_focus);
    chord(&c, "super+c");
    let seen = other.wait_seen("ordinary client's Ctrl+C", |s| s.keys.iter().any(|(key, _)| *key == KEY_C));
    let (_, mods) = seen.keys.iter().find(|(key, _)| *key == KEY_C).unwrap();
    assert_eq!(*mods & (CTRL | LOGO | SHIFT), CTRL);
    other.wait_seen("modifiers released", |s| s.mods & (CTRL | LOGO | SHIFT) == 0);
}

#[test]
fn native_command_keys_follow_live_connections_and_override_terminal_fallbacks() {
    const APP_ID: &str = "org.example.Terminal";
    const KEY_W: u32 = 17;
    let c = Compositor::start(Options {
        settings: "[appearance]\neffects = \"austere\"\n[keys]\nterminals = [\"org.example.Terminal\"]\n",
        ..Default::default()
    });
    let app = c.client(Spec::new(APP_ID, 0x2222cc));
    let window = c.wait_window(APP_ID);
    app.wait_seen("terminal keyboard focus", |s| s.keyboard_focus);
    assert!(!window.kit_app);
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let wait_changed = |events: &mut aizome_ipc::Events, native| {
        events
            .find_map(|event| match event.expect("native command key metadata changed") {
                Event::WindowChanged { window: changed } if changed.id == window.id && changed.kit_app == native => {
                    Some(changed)
                }
                _ => None,
            })
            .expect("the mapped window's native command key policy changed")
    };

    // These sockets and the Wayland client belong to this test process.
    // An app can opt in after mapping and hold more than one connection.
    let mut first = aizome_ipc::Client::connect_to(&c.ipc).unwrap();
    first.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    assert_eq!(first.request(&Request::NativeCommandKeys).unwrap(), aizome_ipc::Response::Ok);
    wait_changed(&mut events, true);
    let mut second = aizome_ipc::Client::connect_to(&c.ipc).unwrap();
    second.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    assert_eq!(second.request(&Request::NativeCommandKeys).unwrap(), aizome_ipc::Response::Ok);
    drop(first);
    c.settle();
    assert!(c.window(APP_ID).unwrap().kit_app, "the remaining connection keeps native commands enabled");

    chord(&c, "super+w");
    let seen = app.wait_seen("the terminal app's own Super+W", |s| s.keys.iter().any(|(key, _)| *key == KEY_W));
    let (_, mods) = seen.keys.iter().find(|(key, _)| *key == KEY_W).unwrap();
    assert_eq!(*mods & (CTRL | LOGO | SHIFT), LOGO);
    assert!(!seen.close_requested, "the compositor's terminal-close fallback must leave native apps alone");
    app.wait_seen("native command modifiers released", |s| s.mods & (CTRL | LOGO | SHIFT) == 0);

    drop(second);
    wait_changed(&mut events, false);
    assert!(!c.window(APP_ID).unwrap().kit_app, "the last connection's closure revokes native command keys");
    chord(&c, "super+c");
    let seen = app.wait_seen("the terminal's Ctrl+Shift+C fallback", |s| s.keys.iter().any(|(key, _)| *key == KEY_C));
    let (_, mods) = seen.keys.iter().find(|(key, _)| *key == KEY_C).unwrap();
    assert_eq!(*mods & (CTRL | LOGO | SHIFT), CTRL | SHIFT);
    app.wait_seen("fallback modifiers released", |s| s.mods & (CTRL | LOGO | SHIFT) == 0);
}

/// A chord means what it says, and the keymap of the keyboard that typed
/// last reads it. Koe pastes into Aizome's apps with ❖V, which must reach
/// them as ❖V whichever key holds ❖ on that keyboard and wherever its
/// layout has v: with left Alt and Super swapped (as Genkan suggests for a
/// PC keyboard), Sumi got Alt+V and typed a v.
#[test]
fn a_chord_means_what_it_says_on_the_keyboard_that_typed_last() {
    const ALT: u32 = 8;
    const KEY_V: u32 = 47;
    // Where Dvorak has v: the full stop's key on a US keyboard.
    const KEY_DOT: u32 = 52;
    const KEY_SPACE: u32 = 57;
    const PC: &str = "Logitech K120";
    // The layout's variant, the keyboard's own keys, and the key that types
    // v there.
    let keyboards = [
        ("", "altwin:swap_lalt_lwin", KEY_V),
        ("", "ctrl:swap_lwin_lctl", KEY_V),
        ("dvorak", "", KEY_DOT),
        ("dvorak", "altwin:swap_lalt_lwin", KEY_DOT),
    ];
    // What the app saw of ❖V on each: the key and the modifiers held.
    let saw = keyboards.map(|(variant, options, _)| {
        // The keyboard's own keys, as Settei's keyboard setup saves them.
        let settings = format!(
            "[appearance]\neffects = \"austere\"\n\
             [input.keyboard]\nlayouts = [\"us\"]\nvariant = \"{variant}\"\noptions = \"\"\n\
             [[input.keyboards]]\nname = \"{PC}\"\nid = \"046d:c31c\"\noptions = \"{options}\"\n"
        );
        let c = Compositor::start(Options { settings: &settings, ..Default::default() });
        let a = c.client(Spec::new("org.aizome.Sumi", 0x2222cc));
        c.wait_window("org.aizome.Sumi");
        a.wait_seen("keyboard focus", |s| s.keyboard_focus);
        // It types, so the keymap is its own.
        let space = |pressed| Synthetic::KeyOn { keyboard: PC.into(), key: "space".into(), pressed };
        c.ok(Request::Synthesize {
            events: vec![
                Synthetic::PlugKeyboard { name: PC.into(), id: "046d:c31c".into(), bus: aizome_ipc::KeyboardBus::Usb },
                space(true),
                space(false),
            ],
        });
        a.wait_seen("the keyboard's space bar", |s| s.keys.iter().any(|(k, _)| *k == KEY_SPACE));
        chord(&c, "super+v");
        // The chord's letter, on whichever of the two keys it came.
        let letter = |s: &Seen| s.keys.iter().find(|(k, _)| [KEY_V, KEY_DOT].contains(k)).copied();
        let (key, mods) = letter(&a.wait_seen("the chord's letter", |s| letter(s).is_some())).unwrap();
        a.wait_seen("modifiers released", |s| s.mods & (SHIFT | CTRL | ALT | LOGO) == 0);
        (variant, options, key, mods & (SHIFT | CTRL | ALT | LOGO))
    });
    assert_eq!(saw, keyboards.map(|(variant, options, v)| (variant, options, v, LOGO)));
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
fn holding_ctrl_super_d_toggles_the_deskbar_once() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let key = |key: &str, pressed| Synthetic::Key { key: key.into(), pressed };
    c.ok(Request::Synthesize { events: vec![key("ctrl", true), key("super", true), key("d", true)] });
    // Held well past the default 600 ms repeat delay.
    std::thread::sleep(Duration::from_millis(1500));
    c.ok(Request::Synthesize { events: vec![key("d", false), key("super", false), key("ctrl", false)] });
    // ❖Space after it: every binding before the palette's came from the hold.
    chord(&c, "super+space");
    let mut got = Vec::new();
    for e in events.by_ref() {
        match e {
            Ok(Event::Binding { action, .. }) => {
                let last = action == "palette";
                got.push(action);
                if last {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert_eq!(got, ["deskbar", "palette"], "one toggle for one press, however long it's held");
    let s = a.seen();
    assert!(!s.keys.iter().any(|(k, _)| *k == 32), "D stayed with the system: {:?}", s.keys);
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

/// Run by the shortcut regression in a separate process, so the Wayland
/// connection and its kit announcement belong to a real third-party app.
#[test]
#[ignore = "a helper: the third-party shortcut regression runs it in its own process"]
fn third_party_kit_client() {
    use aizome_kit::prelude::*;

    let Some(marker) = std::env::var_os("AIZOME_KEYS_TEST_MARKER") else { return };
    struct ShortcutApp {
        marker: std::path::PathBuf,
    }
    impl Program for ShortcutApp {
        type Msg = ();

        fn info(&self) -> AppInfo {
            AppInfo::new("org.example.Test", "Third-party test", "Shortcut regression")
        }

        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("Third-party kit shortcuts").size(360.0, 160.0));
        }

        fn update(&mut self, (): (), _cx: &mut Cx<()>) {
            std::fs::write(&self.marker, "command performed\n").unwrap();
        }

        fn view(&self, _window: WindowKey) -> Element<()> {
            text("Press the command shortcut").into()
        }

        fn menus(&self) -> MenuBar<()> {
            MenuBar::new().with_menu(Menu::new("Edit").item(MenuItem::new("Perform", ()).shortcut(Shortcut::cmd('Z'))))
        }
    }
    aizome_kit::try_run(ShortcutApp { marker: marker.into() }).unwrap();
}
