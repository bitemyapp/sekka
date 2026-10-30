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

fn poll(what: &str, f: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !f() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_keyboard_marking_menu_does_nothing_behind_the_lock() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let _w = c.wait_window("test.a");
    let chord = |keys: &str| c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: keys.into() }] });
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let desk = c.windows()[0].desk;
    // The menu is up when the lid closes or the idle lock fires.
    chord("ctrl+super+space");
    c.settle();
    let lock = LockClient::spawn(&c, 0x7e2553);
    poll("locked", || *lock.locked.lock().unwrap() == Some(true));
    // A password's first letters: C would close the window, ↑ fill it, S
    // stack it, D then 2 move it to Desk 2.
    for k in ["c", "up", "s", "d", "2"] {
        chord(k);
    }
    c.settle();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!a.seen().close_requested, "a key typed at the lock screen closed the window behind it");
    let w = &c.windows()[0];
    assert!(!w.fill && w.desk == desk, "a key typed at the lock screen moved the window: {w:?}");
    // The lock closed the menu rather than hiding it: once unlocked, keys
    // go to the app again.
    a.wait_seen("the lock has the keyboard", |s| !s.keyboard_focus);
    lock.unlock();
    a.wait_seen("keyboard focus again", |s| s.keyboard_focus);
    assert_eq!(type_x(&c, &a), 0, "x after the unlock: {:?}", a.seen().keys);
}

/// The overview up when the session locks closes: after the unlock the
/// window has the keyboard again, where the overview used to keep it.
#[test]
fn locking_closes_the_overview() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let _w = c.wait_window("test.a");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: "ctrl+super+up".into() }] });
    a.wait_seen("the overview has the keyboard", |s| !s.keyboard_focus);
    let lock = LockClient::spawn(&c, 0x7e2553);
    poll("locked", || *lock.locked.lock().unwrap() == Some(true));
    lock.unlock();
    a.wait_seen("keyboard focus again", |s| s.keyboard_focus);
    assert_eq!(type_x(&c, &a), 0, "x after the unlock: {:?}", a.seen().keys);
}

/// ❖Tab held when the session locks: the switcher closes without
/// switching, so letting go of ❖ at the prompt activates nothing, and the
/// window has the keyboard after the unlock.
#[test]
fn locking_closes_a_held_switcher_without_switching() {
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("org.example.alpha", 0x2222cc).title("Alpha one"));
    let _wa = c.wait_window("org.example.alpha");
    let b = c.client(Spec::new("org.example.beta", 0x22cc22).title("Beta one"));
    let wb = c.wait_window("org.example.beta");
    c.wait_windows("beta focused", |ws| ws.iter().any(|w| w.id == wb.id && w.focused));
    c.ok(Request::Synthesize { events: vec![key("super", true), key("tab", true), key("tab", false)] });
    c.settle();
    let lock = LockClient::spawn(&c, 0x7e2553);
    poll("locked", || *lock.locked.lock().unwrap() == Some(true));
    c.ok(Request::Synthesize { events: vec![key("super", false)] });
    c.settle();
    lock.unlock();
    b.wait_seen("keyboard focus again", |s| s.keyboard_focus);
    assert!(c.windows().iter().any(|w| w.id == wb.id && w.focused), "beta keeps the focus: {:?}", c.windows());
    assert_eq!(type_x(&c, &b), 0, "x after the unlock: {:?}", b.seen().keys);
}

/// Subscribes to the compositor's events; a read waits at most 10 s.
fn subscribe(c: &Compositor) -> aizome_ipc::Events {
    let events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    events
}

/// The next pick reported, its tag and area, and whether a pick started
/// on the way; `None` if 10 s pass with no event.
fn next_pick(events: &mut aizome_ipc::Events) -> (Option<(String, Option<aizome_ipc::Rect>)>, bool) {
    let mut started = false;
    let picked = events.by_ref().map_while(Result::ok).find_map(|e| match e {
        aizome_ipc::Event::PickStarted { .. } => {
            started = true;
            None
        }
        aizome_ipc::Event::AreaPicked { tag, area, .. } => Some((tag, area)),
        _ => None,
    });
    (picked, started)
}

/// A pick the shell or the share picker borrowed when the session locks:
/// whoever asked hears nothing was picked, as with Esc, and after the
/// unlock a click is the window's.
#[test]
fn locking_gives_up_a_borrowed_area_pick() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let w = c.wait_window("test.a");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let mut events = subscribe(&c);
    c.ok(Request::PickArea { tag: "share".into(), prompt: None, targets: Vec::new() });
    let lock = LockClient::spawn(&c, 0x7e2553);
    poll("locked", || *lock.locked.lock().unwrap() == Some(true));
    assert_eq!(next_pick(&mut events).0, Some(("share".to_string(), None)), "the borrowed pick was given up");
    lock.unlock();
    a.wait_seen("keyboard focus again", |s| s.keyboard_focus);
    let (cx, cy) = (w.geometry.x as f64 + 40.0, w.geometry.y as f64 + 40.0);
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x: cx, y: cy }, Synthetic::Click { button: "left".into() }],
    });
    a.wait_seen("the click reached the window", |s| s.buttons.contains(&(0x110, true)));
}

/// Sends requests in one write, which the compositor reads and answers in
/// one go, before its loop turns and so before the overlays close behind
/// the lock. Input after a request that opens an overlay then comes while
/// it's up, as input that arrives in the same turn as the lock does.
fn at_once(c: &Compositor, reqs: &[Request]) {
    use std::io::{BufRead, Write};
    let mut s = std::os::unix::net::UnixStream::connect(&c.ipc).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
    let mut lines = String::new();
    for r in reqs {
        lines += &serde_json::to_string(r).unwrap();
        lines.push('\n');
    }
    s.write_all(lines.as_bytes()).unwrap();
    let mut answers = std::io::BufReader::new(s);
    for r in reqs {
        let mut line = String::new();
        answers.read_line(&mut line).unwrap();
        let answer: aizome_ipc::Response = serde_json::from_str(line.trim_end()).unwrap();
        assert!(!matches!(answer, aizome_ipc::Response::Error { .. }), "{r:?}: {answer:?}");
    }
}

/// With an overlay opened behind the lock, the keys go to the prompt:
/// C doesn't close the window from its menu, Q doesn't quit it from the
/// switcher, and 2 doesn't go to Desk 2 from the overview.
#[test]
fn behind_the_lock_no_overlay_takes_a_key() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let _w = c.wait_window("test.a");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let lock = LockClient::spawn(&c, 0x7e2553);
    poll("locked", || *lock.locked.lock().unwrap() == Some(true));
    let chord = |keys: &str| Request::Synthesize { events: vec![Synthetic::Chord { keys: keys.into() }] };
    at_once(&c, &[Request::Action { action: "window-menu".into() }, chord("c")]);
    lock.wait_heard("C at the prompt, not the window's menu", |h| h.keys.contains(&46));
    at_once(&c, &[Request::Switcher { reverse: false, state: None }, chord("q")]);
    lock.wait_heard("Q at the prompt, not the switcher", |h| h.keys.contains(&16));
    at_once(&c, &[Request::Overview { state: Some(true) }, chord("2")]);
    lock.wait_heard("2 at the prompt, not the overview", |h| h.keys.contains(&3));
    let aizome_ipc::Response::Desks { current, .. } = c.request(Request::Desks) else { panic!() };
    assert_eq!(current, 1, "a key at the prompt went to another Desk");
    // Each overlay closed once the loop turned: after the unlock, keys are
    // the window's, and none of them closed it.
    lock.unlock();
    a.wait_seen("keyboard focus again", |s| s.keyboard_focus);
    assert_eq!(type_x(&c, &a), 0, "x after the unlock: {:?}", a.seen().keys);
    assert!(!a.seen().close_requested, "a key at the prompt closed the window");
}

/// With an overlay opened behind the lock, the pointer is the prompt's:
/// a drag doesn't pick an area for a borrowed picker, the wheel doesn't
/// zoom the overview, and a click doesn't land on the window's menu.
#[test]
fn behind_the_lock_no_overlay_takes_the_pointer() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.a", 0xcc2222));
    let _w = c.wait_window("test.a");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let lock = LockClient::spawn(&c, 0x7e2553);
    poll("locked", || *lock.locked.lock().unwrap() == Some(true));
    let mut events = subscribe(&c);
    let motion = |x: f64, y: f64| Synthetic::PointerMotion { x, y };
    let button = |pressed| Synthetic::PointerButton { button: "left".into(), pressed };
    let pick = Request::PickArea { tag: "record".into(), prompt: None, targets: Vec::new() };
    let drag = vec![motion(100.0, 120.0), button(true), motion(500.0, 420.0), button(false)];
    at_once(&c, &[pick, Request::Synthesize { events: drag }]);
    let (picked, started) = next_pick(&mut events);
    assert!(started, "the picker came up behind the lock");
    assert_eq!(picked, Some(("record".to_string(), None)), "the drag picked nothing; the lock gave the pick up");
    lock.wait_heard("the drag's press and release at the prompt", |h| h.buttons == [(0x110, true), (0x110, false)]);
    let wheel = Synthetic::PointerScroll { dx: 0.0, dy: 10.0 };
    at_once(&c, &[Request::Overview { state: Some(true) }, Request::Synthesize { events: vec![wheel] }]);
    lock.wait_heard("the wheel at the prompt, not the overview", |h| h.scrolled != 0.0);
    let click = Synthetic::Click { button: "left".into() };
    at_once(&c, &[Request::Action { action: "window-menu".into() }, Request::Synthesize { events: vec![click] }]);
    lock.wait_heard("the click at the prompt, not the window's menu", |h| h.buttons.len() == 4);
    lock.unlock();
    a.wait_seen("keyboard focus again", |s| s.keyboard_focus);
    assert_eq!(type_x(&c, &a), 0, "x after the unlock: {:?}", a.seen().keys);
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
