//! Displays with Desks of their own, as XMonad's screens show workspaces:
//! switching Desks changes only the display with the focus; a Desk another
//! display shows swaps places with it; windows going to a display of
//! another size scale to it and come back exactly where they were; a
//! window dragged to another display joins its Desk; and a display that
//! goes and comes back gets its Desk back, windows and all.

mod common;

use aizome_ipc::{DeskInfo, DeskSel, Event, Request, Response, Synthetic, WindowInfo};
use common::*;

const DESKTOP: u32 = 0x16161e;

fn desks(c: &Compositor) -> Vec<DeskInfo> {
    match c.request(Request::Desks) {
        Response::Desks { desks, .. } => desks,
        r => panic!("{r:?}"),
    }
}

/// The Desk (1-based) a display shows.
fn shows(c: &Compositor, output: &str) -> Option<u32> {
    desks(c).into_iter().find(|d| d.output.as_deref() == Some(output)).map(|d| d.index)
}

fn current(c: &Compositor) -> u32 {
    desks(c).into_iter().find(|d| d.current).map(|d| d.index).unwrap()
}

fn set_desk(c: &Compositor, index: u32, output: Option<&str>) {
    c.ok(Request::SetDesk { desk: DeskSel::Index(index), output: output.map(str::to_owned) });
}

fn click(c: &Compositor, x: f64, y: f64) {
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x, y }, Synthetic::Click { button: "left".into() }],
    });
}

fn config(name: &str, enabled: bool) -> Request {
    Request::OutputConfig {
        name: name.into(),
        mode: None,
        scale: None,
        position: None,
        transform: None,
        enabled: Some(enabled),
    }
}

fn centre(w: &WindowInfo) -> (i32, i32) {
    (w.geometry.x + w.geometry.w / 2, w.geometry.y + w.geometry.h / 2)
}

fn two(second: &'static str) -> Options<'static> {
    let outputs: &'static [&'static str] = match second {
        "1920x1200@1" => &["1280x800@1", "1920x1200@1"],
        _ => &["1280x800@1", "1280x800@1"],
    };
    Options { outputs, ..Default::default() }
}

#[test]
fn switching_desks_changes_only_the_display_with_the_focus() {
    let c = Compositor::start(two("1280x800@1"));
    // The main display (the first) shows Desk 1, the other Desk 2.
    assert_eq!((shows(&c, "HEADLESS-1"), shows(&c, "HEADLESS-2")), (Some(1), Some(2)));
    let _a = c.client(Spec::new("test.a", 0xcc2222));
    let a = c.wait_window("test.a");
    assert_eq!((a.output.as_deref(), a.desk), (Some("HEADLESS-1"), Some(1)));

    // A click on the second display's desktop gives it the focus; a new
    // window opens there, on its Desk.
    click(&c, 1280.0 + 640.0, 700.0);
    assert_eq!(current(&c), 2);
    let _b = c.client(Spec::new("test.b", 0x2222cc));
    let b = c.wait_window("test.b");
    assert_eq!((b.output.as_deref(), b.desk), (Some("HEADLESS-2"), Some(2)));

    // ⌃❖3 there: the second display shows Desk 3; the first still shows A.
    c.ok(Request::Action { action: "go-to-desk-3".into() });
    assert_eq!((shows(&c, "HEADLESS-1"), shows(&c, "HEADLESS-2")), (Some(1), Some(3)));
    c.settle();
    let img = c.screenshot();
    keep(&img, "screens-desk-per-display");
    let (ax, ay) = centre(&a);
    let (bx, by) = centre(&b);
    assert_px(&img, ax, ay, rgb(0xcc2222), "A, on the first display's Desk");
    assert_px(&img, bx, by, rgb(DESKTOP), "B's Desk went from the second display");
    // Stepping through Desks from there passes Desk 1, which the first
    // display shows.
    c.ok(Request::Action { action: "next-desk".into() });
    assert_eq!(shows(&c, "HEADLESS-2"), Some(4));
    c.ok(Request::Action { action: "next-desk".into() });
    assert_eq!(shows(&c, "HEADLESS-2"), Some(2), "past Desk 1");
    assert_eq!(shows(&c, "HEADLESS-1"), Some(1));
}

#[test]
fn a_desk_another_display_shows_swaps_and_scales_there_and_back_exactly() {
    let c = Compositor::start(two("1920x1200@1"));
    let a_client = c.client(Spec::new("test.a", 0xcc2222));
    let a = c.wait_window("test.a");
    let r0 = aizome_ipc::Rect::new(101, 150, 600, 400);
    c.ok(Request::Resize { id: Some(a.id), rect: r0 });
    a_client.wait_seen("its size", |s| s.size == (600, 400));
    c.wait_windows("placed", |ws| ws[0].geometry == r0);

    // Desk 1 to the larger display, from the smaller one's place: Desk 2
    // comes to the smaller.
    set_desk(&c, 1, Some("HEADLESS-2"));
    assert_eq!((shows(&c, "HEADLESS-1"), shows(&c, "HEADLESS-2")), (Some(2), Some(1)));
    let w = c.wait_windows("A on the larger display", |ws| ws[0].output.as_deref() == Some("HEADLESS-2"));
    let g = w[0].geometry;
    // Half as big again, frame and all, at the same place in proportion.
    assert!(g.x >= 1280 + 150 && g.x <= 1280 + 160, "{g:?}");
    assert!((g.w - 900).abs() <= 8 && (g.h - 600).abs() <= 16, "{g:?}");
    a_client.wait_seen("asked to be that big", |s| s.size == (g.w, g.h));
    c.settle();
    let img = c.screenshot();
    keep(&img, "screens-scaled");
    assert_px(&img, g.x + g.w - 5, g.y + g.h - 5, rgb(0xcc2222), "drawn at its new size");

    // And back: exactly where and as big as it was.
    set_desk(&c, 1, Some("HEADLESS-1"));
    c.wait_windows("A back", |ws| ws[0].geometry == r0 && ws[0].output.as_deref() == Some("HEADLESS-1"));
    a_client.wait_seen("its size again", |s| s.size == (600, 400));
}

#[test]
fn a_window_dragged_to_another_display_joins_its_desk() {
    let c = Compositor::start(two("1280x800@1"));
    let _a = c.client(Spec::new("test.a", 0xcc2222));
    let a = c.wait_window("test.a");
    assert_eq!(a.desk, Some(1));
    // By its tab, to the second display.
    let tab = (a.geometry.x as f64 + 30.0, a.geometry.y as f64 - 12.0);
    let mut events = vec![
        Synthetic::PointerMotion { x: tab.0, y: tab.1 },
        Synthetic::PointerButton { button: "left".into(), pressed: true },
    ];
    for i in 1..=10 {
        events.push(Synthetic::PointerMotion { x: tab.0 + 140.0 * i as f64, y: tab.1 });
    }
    events.push(Synthetic::PointerButton { button: "left".into(), pressed: false });
    c.ok(Request::Synthesize { events });
    let w = c.wait_windows("on the second display", |ws| ws[0].output.as_deref() == Some("HEADLESS-2"));
    assert_eq!(w[0].desk, Some(2), "it joined the Desk there");
    assert_eq!(current(&c), 2, "the focus went with it");
    // Desk 1, on the first display, is empty now; switching the second
    // display's Desk takes it along.
    c.ok(Request::Action { action: "go-to-desk-3".into() });
    c.wait_windows("hidden with its Desk", |ws| ws[0].desk == Some(2) && !ws[0].focused);
}

#[test]
fn a_display_that_goes_and_comes_back_gets_its_desk_back() {
    let c = Compositor::start(two("1280x800@1"));
    let _a = c.client(Spec::new("test.a", 0xcc2222));
    let a = c.wait_window("test.a");
    click(&c, 1280.0 + 640.0, 700.0);
    let _b = c.client(Spec::new("test.b", 0x2222cc));
    let b = c.wait_window("test.b");
    let r1 = aizome_ipc::Rect::new(1280 + 333, 222, 400, 300);
    c.ok(Request::Move { id: Some(b.id), x: r1.x, y: r1.y });
    c.wait_windows("B placed", |ws| ws.iter().any(|w| w.id == b.id && w.geometry == r1));

    // Unplugged with the focus on it: its Desk comes to the first display,
    // whose own Desk hides.
    c.ok(config("HEADLESS-2", false));
    assert_eq!(shows(&c, "HEADLESS-1"), Some(2));
    c.wait_windows("B on the first display", |ws| {
        ws.iter().any(|w| w.id == b.id && w.output.as_deref() == Some("HEADLESS-1"))
    });
    c.settle();
    let img = c.screenshot();
    let (ax, ay) = centre(&a);
    let bb = c.window("test.b").unwrap();
    assert_px(&img, centre(&bb).0, centre(&bb).1, rgb(0x2222cc), "B, brought along");
    let (g, h) = (bb.geometry, a.geometry);
    let apart = g.x >= h.x + h.w || h.x >= g.x + g.w || g.y >= h.y + h.h || h.y >= g.y + g.h;
    if apart {
        assert_px(&img, ax, ay, rgb(DESKTOP), "A's Desk hides meanwhile");
    }
    assert!(!c.window("test.a").unwrap().focused);

    // Plugged back in: both Desks where they were, B exactly so.
    c.ok(config("HEADLESS-2", true));
    assert_eq!((shows(&c, "HEADLESS-1"), shows(&c, "HEADLESS-2")), (Some(1), Some(2)));
    c.wait_windows("B back", |ws| ws.iter().any(|w| w.id == b.id && w.geometry == r1));
    c.settle();
    let img = c.screenshot();
    assert_px(&img, ax, ay, rgb(0xcc2222), "A shows again");
    let (bx, by) = (r1.x + r1.w / 2, r1.y + r1.h / 2);
    assert_px(&img, bx, by, rgb(0x2222cc), "B on its display again");
}

#[test]
fn a_sticky_windows_dialog_opens_on_its_display() {
    let c = Compositor::start(two("1280x800@1"));
    let a_client = c.client(Spec::new("test.a", 0xcc2222));
    let a = c.wait_window("test.a");
    c.ok(Request::Sticky { id: Some(a.id), state: Some(true) });
    // A's Desk goes to the second display; sticky, A stays on the first.
    set_desk(&c, 1, Some("HEADLESS-2"));
    assert_eq!((shows(&c, "HEADLESS-1"), shows(&c, "HEADLESS-2")), (Some(2), Some(1)));
    c.wait_windows("A still on the first display", |ws| {
        ws.iter().any(|w| w.id == a.id && w.output.as_deref() == Some("HEADLESS-1"))
    });
    // Its dialog opens beside it, not on the display showing A's Desk.
    a_client.send(Cmd::Dialog("Save".into(), 0x22cc22));
    let ws = c.wait_windows("the dialog placed", |ws| ws.iter().any(|w| w.title == "Save" && w.geometry.w == 200));
    let d = ws.iter().find(|w| w.title == "Save").unwrap();
    assert_eq!(d.output.as_deref(), Some("HEADLESS-1"), "with its parent: {d:?}");
}

#[test]
fn a_desk_for_a_display_not_in_the_layout_is_refused() {
    let c = Compositor::start(two("1280x800@1"));
    match c.request(Request::SetDesk { desk: DeskSel::Index(2), output: Some("HDMI-9".into()) }) {
        Response::Error { message } => assert!(message.contains("HDMI-9"), "{message}"),
        r => panic!("{r:?}"),
    }
}

#[test]
fn losing_an_unfocused_display_announces_that_its_desk_is_no_longer_visible() {
    // The shell underlines Desks shown on another display from its event
    // subscription. Correct answers to `desks` alone don't refresh it.
    for lid in [true, false] {
        let c = Compositor::start(Options { outputs: &["eDP-1=1280x800@1", "1280x800@1"], ..Default::default() });
        set_desk(&c, 2, Some("HEADLESS-2"));
        for enabled in [false, true, false, true] {
            let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
            events.stream().set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
            if lid {
                c.ok(Request::Synthesize { events: vec![Synthetic::Lid { closed: !enabled }] });
            } else {
                c.ok(config("eDP-1", enabled));
            }
            assert_eq!(current(&c), 2, "focus stays on the external display");
            let expected = desks(&c);
            assert_eq!(expected[0].output.as_deref(), enabled.then_some("eDP-1"));
            let announced = events.find_map(|event| match event.expect("the shell needs updated Desk visibility") {
                Event::DesksChanged { desks } => Some(desks),
                _ => None,
            });
            assert_eq!(announced.as_ref(), Some(&expected), "lid={lid}, enabled={enabled}");
        }
    }
}
