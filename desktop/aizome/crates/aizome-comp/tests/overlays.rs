//! The switcher, the Desks overview and the marking menu, driven by
//! synthetic input through the compositor's own paths.

mod common;

use aizome_ipc::{Request, Synthetic};
use common::*;

fn key(k: &str, pressed: bool) -> Synthetic {
    Synthetic::Key { key: k.into(), pressed }
}

#[test]
fn the_switcher_picks_the_previous_app_on_release() {
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("org.example.alpha", 0x2222cc).title("Alpha one"));
    let wa = c.wait_window("org.example.alpha");
    let _b = c.client(Spec::new("org.example.beta", 0x22cc22).title("Beta one"));
    c.wait_windows("b focused", |ws| ws.iter().any(|w| w.app_id == "org.example.beta" && w.focused));
    c.ok(Request::Synthesize { events: vec![key("super", true), key("tab", true), key("tab", false)] });
    c.settle();
    let img = c.screenshot();
    keep(&img, "switcher");
    // The panel is up in the middle: raised, not the desktop.
    let mid = img.px(640, 400);
    assert!(!near(mid, rgb(0x16161e)), "the switcher shows");
    c.ok(Request::Synthesize { events: vec![key("super", false)] });
    c.wait_windows("alpha focused", |ws| ws.iter().any(|w| w.id == wa.id && w.focused));
}

#[test]
fn the_switcher_goes_down_to_windows_and_escape_cancels() {
    let c = Compositor::start(Options::default());
    let _a1 = c.client(Spec::new("org.example.alpha", 0x2222cc).title("Alpha one"));
    let a1 = c.wait_window("org.example.alpha");
    let _a2 = c.client(Spec::new("org.example.alpha", 0x3333dd).title("Alpha two"));
    c.wait_windows("two alphas", |ws| ws.len() == 2);
    let _b = c.client(Spec::new("org.example.beta", 0x22cc22));
    c.wait_windows("b focused", |ws| ws.iter().any(|w| w.app_id == "org.example.beta" && w.focused));
    // ❖Tab to Alpha, ` into its windows (the second, A1), release.
    c.ok(Request::Synthesize {
        events: vec![key("super", true), key("tab", true), key("tab", false), key("grave", true), key("grave", false)],
    });
    c.settle();
    keep(&c.screenshot(), "switcher-windows");
    c.ok(Request::Synthesize { events: vec![key("super", false)] });
    c.wait_windows("alpha one focused", |ws| ws.iter().any(|w| w.id == a1.id && w.focused));
    // Esc cancels.
    let before = c.windows().into_iter().find(|w| w.focused).unwrap().id;
    c.ok(Request::Synthesize {
        events: vec![
            key("super", true),
            key("tab", true),
            key("tab", false),
            key("escape", true),
            key("escape", false),
            key("super", false),
        ],
    });
    c.settle();
    assert_eq!(c.windows().into_iter().find(|w| w.focused).unwrap().id, before);
}

#[test]
fn the_overview_shows_desks_and_moves_windows() {
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("test.a", 0xcc2222).title("Red"));
    let wa = c.wait_window("test.a");
    c.ok(Request::Overview { state: Some(true) });
    c.settle();
    let img = c.screenshot();
    keep(&img, "overview");
    // The backdrop replaces the desktop; the window's spot shows cards.
    assert!(!near(img.px(wa.geometry.x + 50, wa.geometry.y + 50), rgb(0xcc2222)), "the window itself is hidden");
    // Typing filters; 2 goes to Desk 2.
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: "2".into() }] });
    c.settle();
    let aizome_ipc::Response::Desks { current, .. } = c.request(Request::Desks) else { panic!() };
    assert_eq!(current, 2);
    // Back, and the overview closes with Esc.
    c.ok(Request::Overview { state: Some(true) });
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: "escape".into() }] });
    c.settle();
    let img = c.screenshot();
    assert!(near(img.px(5, 5), rgb(0x16161e)), "the desktop again");
}

#[test]
fn a_parked_desk_suspends_its_windows_until_it_is_visited() {
    let c = Compositor::start(Options::default());
    let red = c.client(Spec::new("test.a", 0xcc2222).title("Red"));
    let wa = c.wait_window("test.a");
    c.ok(Request::MoveToDesk { id: Some(wa.id), desk: aizome_ipc::DeskSel::Index(2), follow: false });
    c.ok(Request::Overview { state: Some(true) });
    c.settle();
    // Desk 2's card holds the red window's thumbnail: point at it.
    let img = c.screenshot();
    let mut at = None;
    'find: for y in (0..img.height as i32).step_by(4) {
        for x in (0..img.width as i32).step_by(4) {
            if near(img.px(x, y), rgb(0xcc2222)) {
                at = Some((x as f64 + 2.0, y as f64 + 2.0));
                break 'find;
            }
        }
    }
    let (x, y) = at.expect("the red window's thumbnail on Desk 2's card");
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x, y }] });
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: "p".into() }] });
    red.wait_seen("suspended once its Desk is parked", |s| s.suspended);
    keep(&c.screenshot(), "overview-parked");
    // The overview stayed, unfiltered: p parked rather than typed.
    let img = c.screenshot();
    assert!(!near(img.px(x as i32, y as i32), rgb(0xcc2222)), "a parked Desk's windows show faintly");
    // Going there wakes it.
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: "2".into() }] });
    red.wait_seen("woken on arriving at its Desk", |s| !s.suspended);
}

#[test]
fn a_marking_menu_flick_fills_without_drawing() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let w = c.wait_window("test.a");
    let tab = (w.geometry.x as f64 + 60.0, w.geometry.y as f64 - 12.0);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: tab.0, y: tab.1 },
            Synthetic::PointerButton { button: "right".into(), pressed: true },
            Synthetic::PointerMotion { x: tab.0, y: tab.1 - 20.0 },
            Synthetic::PointerMotion { x: tab.0, y: tab.1 - 60.0 },
            Synthetic::PointerButton { button: "right".into(), pressed: false },
        ],
    });
    a.wait_seen("filled by a flick up", |s| s.maximized);
}

#[test]
fn ctrl_super_space_opens_the_marking_menu_for_the_keys() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let w = c.wait_window("test.a");
    let chord = |keys: &str| c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: keys.into() }] });
    // The pointer is in the window, well away from its tab.
    let away = (w.geometry.x as f64 + 40.0, w.geometry.y as f64 + 200.0);
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x: away.0, y: away.1 }] });
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    chord("ctrl+super+space");
    c.settle();
    let img = c.screenshot();
    keep(&img, "marking-menu-keys");
    // Around the tab, not the pointer: nothing drawn where it is.
    assert!(near(img.px(away.0 as i32, away.1 as i32), rgb(0xcc2222)), "the menu isn't at the pointer");
    // Esc closes it with nothing done, and keys go to the app again; so
    // does ⌃❖Space again.
    chord("escape");
    chord("up");
    a.wait_seen("↑ reached the app", |s| s.keys.iter().any(|(k, _)| *k == 103));
    chord("ctrl+super+space");
    chord("ctrl+super+space");
    chord("up");
    a.wait_seen("↑ reached the app again", |s| s.keys.iter().filter(|(k, _)| *k == 103).count() == 2);
    // A click away from its verbs closes it too, picking nothing.
    chord("ctrl+super+space");
    c.ok(Request::Synthesize { events: vec![Synthetic::Click { button: "left".into() }] });
    c.settle();
    let now = c.windows();
    assert!(!now[0].fill && !now[0].minimised, "a click away picks nothing");
    // The arrows pick the verb that way.
    chord("ctrl+super+space");
    chord("up");
    a.wait_seen("filled by ⌃❖Space ↑", |s| s.maximized);
    // D opens the Desk ring, and a digit picks the Desk.
    chord("ctrl+super+space");
    chord("d");
    chord("2");
    c.wait_windows("on Desk 2", |ws| ws[0].desk == Some(2));
    // None of it reached the app but the two ↑ while the menu was shut.
    let keys: Vec<u32> = a.seen().keys.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys.iter().filter(|k| **k == 103).count(), 2, "{keys:?}");
    assert!(!keys.iter().any(|k| [1, 3, 32, 57].contains(k)), "{keys:?}");
}

/// Types X; the modifiers the app last heard of when it arrived.
fn type_x(c: &Compositor, a: &TestClient) -> u32 {
    let n = a.seen().keys.iter().filter(|(k, _)| *k == 45).count();
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: "x".into() }] });
    let s = a.wait_seen("x reached the app", |s| s.keys.iter().filter(|(k, _)| *k == 45).count() > n);
    s.keys.iter().rev().find(|(k, _)| *k == 45).unwrap().1
}

#[test]
fn keys_after_the_keyboard_marking_menu_carry_no_stale_modifiers() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let _w = c.wait_window("test.a");
    let chord = |keys: &str| c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: keys.into() }] });
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    // ⌃ and ❖ come up while the menu is up: the app hears them, and X
    // after Esc is a plain X, not ⌃X.
    let x = || type_x(&c, &a);
    chord("ctrl+super+space");
    chord("escape");
    assert_eq!(x(), 0, "x after Esc: {:?}", a.seen().keys);
    // And after a verb picked by its key.
    chord("ctrl+super+space");
    chord("f");
    assert_eq!(x(), 0, "x after F: {:?}", a.seen().keys);
    // ⇧ pressed while ⌃❖ are still down, and let go after the verb: the
    // app hears each come up.
    c.ok(Request::Synthesize {
        events: vec![
            key("ctrl", true),
            key("super", true),
            key("space", true),
            key("space", false),
            key("shift", true),
            key("ctrl", false),
            key("super", false),
            key("up", true),
            key("up", false),
            key("shift", false),
        ],
    });
    a.wait_seen("filled by ⇧↑", |s| s.maximized);
    assert_eq!(x(), 0, "x after ⇧↑: {:?}", a.seen().keys);
    // The menu's own keys never reached it.
    let keys: Vec<u32> = a.seen().keys.iter().map(|(k, _)| *k).collect();
    assert!(!keys.iter().any(|k| [1, 33, 57, 103].contains(k)), "{keys:?}");
}

#[test]
fn a_marking_menu_shows_after_a_pause_and_picks() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let w = c.wait_window("test.a");
    let tab = (w.geometry.x as f64 + 60.0, w.geometry.y as f64 - 12.0);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: tab.0, y: tab.1 },
            Synthetic::PointerButton { button: "right".into(), pressed: true },
        ],
    });
    std::thread::sleep(std::time::Duration::from_millis(400));
    // Toward "park" (down).
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x: tab.0, y: tab.1 + 90.0 }] });
    c.settle();
    let img = c.screenshot();
    keep(&img, "marking-menu");
    // The highlighted chip is ai (selection) indigo.
    let row: Vec<[u8; 3]> = (0..60).map(|d| img.px(tab.0 as i32 - 30 + d, tab.1 as i32 + 96)).collect();
    assert!(row.iter().any(|p| near(*p, rgb(0x3d59a1))), "park is highlighted");
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerButton { button: "right".into(), pressed: false }] });
    c.wait_windows("parked", |ws| ws[0].minimised);
    let _ = a;
}

/// The shell borrows the screenshot picker to choose what to record: the
/// pick comes back as an `area-picked` event, not a screenshot.
#[test]
fn the_area_picker_reports_to_the_shell() {
    use aizome_ipc::{Event, Rect};
    let c = Compositor::start(Options::default());
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut next_pick = || {
        events.by_ref().find_map(|e| match e {
            Ok(Event::AreaPicked { tag, area, output, window, .. }) => Some((tag, area, output, window)),
            _ => None,
        })
    };
    let motion = |x: f64, y: f64| Synthetic::PointerMotion { x, y };
    let button = |pressed| Synthetic::PointerButton { button: "left".into(), pressed };
    // A drag: that area.
    c.ok(Request::PickArea { tag: "record".into(), prompt: None, targets: Vec::new() });
    c.ok(Request::Synthesize { events: vec![motion(100.0, 120.0), button(true), motion(500.0, 420.0), button(false)] });
    let (tag, area, output, window) = next_pick().unwrap();
    assert_eq!(tag, "record");
    assert_eq!(area, Some(Rect::new(100, 120, 400, 300)));
    assert_eq!((output.as_deref(), window), (Some("HEADLESS-1"), None));
    // A click: the whole display under the pointer.
    c.ok(Request::PickArea { tag: "record".into(), prompt: None, targets: Vec::new() });
    c.ok(Request::Synthesize { events: vec![motion(300.0, 300.0), button(true), button(false)] });
    assert_eq!(next_pick().unwrap().1, Some(Rect::new(0, 0, 1280, 800)));
    // Space, then a click on a window: its frame and tab.
    let _a = c.client(Spec::new("org.example.alpha", 0x2222cc));
    let w = c.wait_window("org.example.alpha");
    c.settle();
    c.ok(Request::PickArea { tag: "record".into(), prompt: None, targets: Vec::new() });
    let (cx, cy) = ((w.geometry.x + 20) as f64, (w.geometry.y + 20) as f64);
    c.ok(Request::Synthesize {
        events: vec![key("space", true), key("space", false), motion(cx, cy), button(true), button(false)],
    });
    let (_, area, _, window) = next_pick().unwrap();
    let area = area.expect("the window's bounds");
    assert_eq!(window, Some(w.id));
    assert!(area.x <= w.geometry.x && area.y < w.geometry.y, "the tab and frame are in: {area:?} {:?}", w.geometry);
    assert!(area.w >= w.geometry.w && area.h > w.geometry.h);
    // Esc: nothing, and the picker is gone (a click then is a click).
    c.ok(Request::PickArea { tag: "record".into(), prompt: None, targets: Vec::new() });
    c.ok(Request::Synthesize { events: vec![key("escape", true), key("escape", false)] });
    assert_eq!(next_pick().unwrap().1, None);
    // No screenshot was taken along the way.
    assert!(!c.dir.join("home/Pictures/Screenshots").exists());
}

/// The share picker borrows the picker for displays and windows only,
/// with a banner: a drag is the display under the pointer, a window comes
/// with its ext-foreign-toplevel identifier (how the portal names it), a
/// click on no window keeps picking, and windows alone start at windows.
#[test]
fn the_picker_picks_what_may_be_shared() {
    use aizome_ipc::{Event, PickTarget, Rect};
    let c = Compositor::start(Options::default());
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut next_pick = || {
        events.by_ref().find_map(|e| match e {
            Ok(Event::AreaPicked { tag, area, output, window, toplevel }) => {
                Some((tag, area, output, window, toplevel))
            }
            _ => None,
        })
    };
    let motion = |x: f64, y: f64| Synthetic::PointerMotion { x, y };
    let button = |pressed| Synthetic::PointerButton { button: "left".into(), pressed };
    let share = |targets: Vec<PickTarget>| Request::PickArea {
        tag: "share".into(),
        prompt: Some("Choose a screen or window to share — Esc to cancel".into()),
        targets,
    };
    let both = || share(vec![PickTarget::Display, PickTarget::Window]);
    // A drag isn't an area here: the display under the pointer.
    c.ok(both());
    c.ok(Request::Synthesize { events: vec![motion(100.0, 120.0), button(true), motion(500.0, 420.0), button(false)] });
    let (tag, area, output, window, toplevel) = next_pick().unwrap();
    assert_eq!((tag.as_str(), area), ("share", Some(Rect::new(0, 0, 1280, 800))));
    assert_eq!((output.as_deref(), window, toplevel), (Some("HEADLESS-1"), None, None));
    // Space, a click on nothing (still picking), then on a window.
    let _a = c.client(Spec::new("org.example.alpha", 0x2222cc).size(300, 200));
    let w = c.wait_window("org.example.alpha");
    c.settle();
    let (cx, cy) = ((w.geometry.x + 20) as f64, (w.geometry.y + 20) as f64);
    c.ok(both());
    c.ok(Request::Synthesize {
        events: vec![key("space", true), key("space", false), motion(1270.0, 790.0), button(true), button(false)],
    });
    c.ok(Request::Synthesize { events: vec![motion(cx, cy), button(true), button(false)] });
    let (_, _, _, window, toplevel) = next_pick().unwrap();
    assert_eq!(window, Some(w.id));
    let toplevel = toplevel.expect("the window's identifier");
    assert!(toplevel.len() >= 16, "{toplevel}");
    // Windows alone: a click picks one at once, and Space doesn't leave them.
    c.ok(share(vec![PickTarget::Window]));
    c.ok(Request::Synthesize {
        events: vec![key("space", true), key("space", false), motion(cx, cy), button(true), button(false)],
    });
    assert_eq!(next_pick().unwrap().3, Some(w.id));
    // Esc: refused.
    c.ok(both());
    c.ok(Request::Synthesize { events: vec![key("escape", true), key("escape", false)] });
    assert_eq!(next_pick().unwrap().1, None);
}

/// A window is picked on the press, which closes the picker; the release
/// is the picker's still, not the window's, which would see a release
/// with no press (foot: "stray button release event"). For a share and
/// for ❖⇧4's screenshot of a window alike; a click then is the window's.
#[test]
fn a_window_pick_keeps_its_release_from_the_window() {
    use aizome_ipc::{Event, PickTarget};
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("org.example.alpha", 0x2222cc).size(300, 200));
    let w = c.wait_window("org.example.alpha");
    c.settle();
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let button = |pressed| Synthetic::PointerButton { button: "left".into(), pressed };
    let (cx, cy) = ((w.geometry.x + 20) as f64, (w.geometry.y + 20) as f64);
    let click = vec![Synthetic::PointerMotion { x: cx, y: cy }, button(true), button(false)];
    // Shared: windows alone, so a click picks.
    c.ok(Request::PickArea { tag: "share".into(), prompt: None, targets: vec![PickTarget::Window] });
    c.ok(Request::Synthesize { events: click.clone() });
    let picked = events.by_ref().find_map(|e| match e {
        Ok(Event::AreaPicked { window, .. }) => Some(window),
        _ => None,
    });
    assert_eq!(picked, Some(Some(w.id)));
    // A screenshot of it: ❖⇧4, Space, a click.
    c.ok(Request::Synthesize {
        events: vec![Synthetic::Chord { keys: "super+shift+4".into() }, key("space", true), key("space", false)],
    });
    c.ok(Request::Synthesize { events: click.clone() });
    let shot = events.by_ref().find_map(|e| match e {
        Ok(Event::ScreenshotTaken { path }) => Some(path),
        _ => None,
    });
    assert!(shot.is_some(), "a screenshot of the window");
    // The window saw neither click; this one is its own, press and release.
    c.ok(Request::Synthesize { events: click });
    let s = a.wait_seen("the click", |s| s.buttons.len() >= 2);
    assert_eq!(s.buttons, [(0x110, true), (0x110, false)]);
}
