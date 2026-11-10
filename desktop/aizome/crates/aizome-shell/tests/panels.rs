//! The systray's panels, headless: each opens under its widget with its
//! controls, closes at a press outside it, and draws as the boards' panels
//! do.

mod common;

use std::time::Duration;

use aizome_ipc::Event;
use aizome_kit::prelude::*;
use aizome_shell::agenda::{EventRow, ZoneRow};
use aizome_shell::clock::Local;
use aizome_shell::ipc::IpcEvent;
use aizome_shell::menubar::{BarMsg, Slot, TrayWidget};
use aizome_shell::shell::Msg;
use aizome_shell::systray::panels::*;
use common::*;

fn fixture() -> aizome_shell::systray::glue::Fixture {
    let mut f = aizome_shell::systray::glue::Fixture::new();
    let net = |ssid: &str, bars: u8, secure: bool, connected: bool| NetRow {
        ssid: ssid.into(),
        bars,
        secure,
        connected,
        connecting: false,
        known: connected,
    };
    f.insert(
        TrayWidget::Network,
        Panel::Network(NetPanel {
            wifi: Some(true),
            networks: vec![
                net("kestrel", 4, true, true),
                net("Ramen Bar Guest", 3, false, false),
                net("NETGEAR-5G", 2, true, false),
                net("printer-setup", 1, true, false),
            ],
            vpns: vec![("u1".into(), "Office WireGuard".into(), false)],
            airplane: false,
            asking: None,
            error: None,
        }),
    );
    f.insert(
        TrayWidget::Sound,
        Panel::Sound(SoundPanel {
            output: Some((0.62, false)),
            outputs: vec![("speakers".into(), "Speakers".into(), true), ("hp".into(), "WH-1000XM4".into(), false)],
            input: Some((0.4, false)),
            media: Some(Media { title: "Clair de lune".into(), artist: "Isao Tomita".into(), playing: true }),
        }),
    );
    f.insert(
        TrayWidget::Bluetooth,
        Panel::Bluetooth(BtPanel {
            powered: true,
            devices: vec![
                BtDevice { path: "/a".into(), name: "WH-1000XM4".into(), connected: true, battery: Some(80) },
                BtDevice { path: "/b".into(), name: "MX Master 3".into(), connected: false, battery: None },
            ],
        }),
    );
    f.insert(
        TrayWidget::Display,
        Panel::Display(DisplayPanel {
            backlights: vec![("intel_backlight".into(), 0.7)],
            theme: 0,
            size: 1,
            night_light: Some(false),
        }),
    );
    f.insert(
        TrayWidget::Battery,
        Panel::Battery(BatteryPanel {
            percent: 84.0,
            state: "On battery".into(),
            time: Some("5:12 left".into()),
            modes: vec!["power-saver".into(), "balanced".into(), "performance".into()],
            mode: Some(1),
            keep_awake: Default::default(),
        }),
    );
    f.insert(
        TrayWidget::Clock,
        Panel::Clock(ClockPanel {
            now: Local { year: 2026, month: 10, day: 17, hour: 21, minute: 42, second: 0, weekday: 6 },
            dnd: false,
            events: Some(vec![
                EventRow {
                    when: "Now".into(),
                    title: "Release review".into(),
                    color: Some(0x7aa2f7),
                    now: true,
                    date: "2026-10-17".into(),
                },
                EventRow {
                    when: "Sun 09:30".into(),
                    title: "Dentist".into(),
                    color: Some(0xe0af68),
                    now: false,
                    date: "2026-10-18".into(),
                },
                EventRow {
                    when: "Sun".into(),
                    title: "Mum's birthday".into(),
                    color: Some(0xe0af68),
                    now: false,
                    date: "2026-10-18".into(),
                },
            ]),
            zones: vec![
                ZoneRow { name: "Tokyo".into(), time: "11:42".into(), note: "+14h, tomorrow".into() },
                ZoneRow { name: "Lisbon".into(), time: "03:42".into(), note: "+6h, tomorrow".into() },
            ],
        }),
    );
    f
}

fn open(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, w: TrayWidget) -> WindowKey {
    let bar = window_titled(h, "Menu bar");
    h.send(Msg::Bar(bar, BarMsg::Press(Slot::Tray(w), Rect::new(1300.0, 0.0, 1315.0, 24.0))));
    window_titled(h, w.name())
}

#[test]
fn a_panel_opens_under_its_widget_and_toggles() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let p = open(&mut h, TrayWidget::Network);
    assert!(h.texts(p).iter().any(|t| t == "kestrel"));
    assert!(h.texts(p).iter().any(|t| t == "Network Settings…"));
    let spec = &h.platform.windows.values().last().unwrap().spec;
    assert!(matches!(spec.kind, aizome_kit::platform::OpenKind::Popup { grab: true, .. }));
    // Pressing the widget again closes it.
    open_again(&mut h, TrayWidget::Network);
    assert!(h.program().panel.is_none());
}

fn open_again(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, w: TrayWidget) {
    let bar = window_titled(h, "Menu bar");
    h.send(Msg::Bar(bar, BarMsg::Press(Slot::Tray(w), Rect::new(1300.0, 0.0, 1315.0, 24.0))));
}

/// A click on a systray widget, where the bar shows it.
fn press_widget(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, w: TrayWidget) {
    let bar = window_titled(h, "Menu bar");
    let at = h.find_id(bar, &aizome_shell::menubar::tray_id(w)).expect("the widget in the bar").center();
    h.click(bar, at);
}

/// The widget whose panel the shell has open.
fn showing(h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>) -> Option<TrayWidget> {
    h.program().panel.as_ref().map(|p| p.widget)
}

/// The panels' windows that are open, by title (a panel's is its widget's
/// name).
fn panel_windows(h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>) -> Vec<String> {
    h.windows().into_iter().map(|k| h.title(k)).filter(|t| TrayWidget::named(t).is_some()).collect()
}

/// A press outside a panel closes it, in whichever of the shell's surfaces
/// it lands: the compositor closes it for a press in another app only, and
/// leaves one on the shell's own surfaces to the shell (#207).
#[test]
fn a_press_outside_a_panel_closes_it() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let bar = window_titled(&h, "Menu bar");
    let desktop = window_titled(&h, "Desktop");
    let deskbar = window_titled(&h, "Deskbar");

    // On the desktop.
    press_widget(&mut h, TrayWidget::Network);
    assert_eq!(showing(&h), Some(TrayWidget::Network));
    h.click(desktop, Point::new(900.0, 500.0));
    assert_eq!(showing(&h), None, "a press on the desktop closes it");
    assert!(panel_windows(&h).is_empty(), "{:?}", panel_windows(&h));
    let screen = h.program().screens[0].clone();
    assert_eq!(h.program().bar_model(&screen).open, None, "and its widget isn't lit");

    // On the Deskbar.
    press_widget(&mut h, TrayWidget::Sound);
    assert_eq!(showing(&h), Some(TrayWidget::Sound));
    let row = h.find(deskbar, "Editor").expect("Editor's row").center();
    h.click(deskbar, row);
    assert_eq!(showing(&h), None, "a press on the Deskbar closes it");
    assert!(panel_windows(&h).is_empty(), "{:?}", panel_windows(&h));

    // Elsewhere on the bar: a title's press closes it, and opens the
    // title's menu.
    press_widget(&mut h, TrayWidget::Clock);
    assert_eq!(showing(&h), Some(TrayWidget::Clock));
    let file = h.find(bar, "File").expect("the File title").center();
    h.click(bar, file);
    assert_eq!(showing(&h), None, "a press elsewhere on the bar closes it");
    assert!(panel_windows(&h).is_empty(), "{:?}", panel_windows(&h));
    assert_eq!(h.popups(bar).len(), 1, "and the title's menu opens");
}

/// A panel's own widget pressed again closes it, and it stays closed: the
/// press closes the panel as any press outside it does, and then reaches
/// the widget. Another widget's press opens that widget's panel instead.
#[test]
fn a_panels_widget_closes_it_and_another_widget_opens_its_own() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    press_widget(&mut h, TrayWidget::Network);
    assert_eq!(showing(&h), Some(TrayWidget::Network));
    press_widget(&mut h, TrayWidget::Network);
    assert_eq!(showing(&h), None, "pressed again, it closes and stays closed");
    assert!(panel_windows(&h).is_empty(), "{:?}", panel_windows(&h));
    // Pressed once more, it opens.
    press_widget(&mut h, TrayWidget::Network);
    assert_eq!(showing(&h), Some(TrayWidget::Network));

    press_widget(&mut h, TrayWidget::Sound);
    assert_eq!(showing(&h), Some(TrayWidget::Sound));
    assert_eq!(panel_windows(&h), ["sound"], "one panel at a time");
    press_widget(&mut h, TrayWidget::Clock);
    assert_eq!(showing(&h), Some(TrayWidget::Clock));
    assert_eq!(panel_windows(&h), ["clock"]);
}

#[test]
fn a_press_inside_a_panel_leaves_it_open() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    press_widget(&mut h, TrayWidget::Network);
    let p = window_titled(&h, "network");
    // The connected network's row: nothing to do.
    assert!(h.click_text(p, "kestrel"));
    assert_eq!(showing(&h), Some(TrayWidget::Network));
    // A secured one's: its password is asked for, in the panel.
    assert!(h.click_text(p, "NETGEAR-5G"));
    assert_eq!(showing(&h), Some(TrayWidget::Network));
    assert_eq!(h.program().panel_ui.asking.as_ref().map(|a| a.0.as_str()), Some("NETGEAR-5G"));
    assert_eq!(panel_windows(&h), ["network"]);
}

/// One at a time: a menu opening closes the panel wherever it opens, and a
/// widget pressed while a menu is open closes the menu and opens its panel.
#[test]
fn a_panel_makes_way_for_a_menu_and_a_menu_for_a_panel() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let bar = window_titled(&h, "Menu bar");
    press_widget(&mut h, TrayWidget::Network);
    assert_eq!(showing(&h), Some(TrayWidget::Network));
    // The menu at the pointer comes by the compositor's binding: no press
    // reaches the shell to close the panel.
    h.send(Msg::Ipc(IpcEvent::Event(Event::Binding {
        action: "app-menu".into(),
        pointer: Some((600.0, 300.0)),
        output: Some("eDP-1".into()),
    })));
    // Services wait for the selected text, at most 150 ms.
    h.advance(Duration::from_millis(200));
    assert_eq!(showing(&h), None, "the menu at the pointer closes it");
    assert!(panel_windows(&h).is_empty(), "{:?}", panel_windows(&h));
    assert_eq!(h.popups(bar).len(), 1, "and is open");

    press_widget(&mut h, TrayWidget::Sound);
    assert!(h.popups(bar).is_empty(), "a widget's press closes the menu");
    assert_eq!(showing(&h), Some(TrayWidget::Sound));
}

/// Dismissed by the compositor (a press in another app), a panel is
/// forgotten, and its widget opens it again.
#[test]
fn a_panel_the_compositor_dismissed_opens_again_from_its_widget() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    press_widget(&mut h, TrayWidget::Network);
    let p = window_titled(&h, "network");
    // The shell hears of the compositor's dismissal as a close request.
    h.request_close(p);
    assert_eq!(showing(&h), None);
    assert!(panel_windows(&h).is_empty(), "{:?}", panel_windows(&h));
    // A moment later (a press right as a panel closes is taken for the
    // press that closed it), its widget opens it again.
    std::thread::sleep(Duration::from_millis(60));
    press_widget(&mut h, TrayWidget::Network);
    assert_eq!(showing(&h), Some(TrayWidget::Network));
}

#[test]
fn dismissal_on_blank_bar_clears_the_widget_highlight() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let bar = window_titled(&h, "Menu bar");
    let widget = h.find_id(bar, &aizome_shell::menubar::tray_id(TrayWidget::Network)).unwrap();
    let blank = Point::new(900.0, 12.0);
    h.move_to(bar, blank);
    let closed = h.render_rect(bar, widget, 1.0).to_rgba8().unwrap();
    press_widget(&mut h, TrayWidget::Network);
    assert_eq!(showing(&h), Some(TrayWidget::Network));
    assert!(h.render_rect(bar, widget, 1.0).to_rgba8().unwrap() != closed, "its widget is highlighted");
    // Empty bar space sends no application message after the close request.
    h.click(bar, blank);
    assert_eq!(showing(&h), None);
    let actual = h.render_rect(bar, widget, 1.0).to_rgba8().unwrap();
    assert!(actual == closed, "the widget loses its highlight without an unrelated update");
}

#[test]
fn a_secured_network_asks_its_password_inline() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let p = open(&mut h, TrayWidget::Network);
    h.send(Msg::Panel(PanelMsg::Network("NETGEAR-5G".into())));
    assert_eq!(h.program().panel_ui.asking.as_ref().map(|a| a.0.as_str()), Some("NETGEAR-5G"));
    h.type_text(p, "hunter22");
    assert_eq!(h.program().panel_ui.asking.as_ref().map(|a| a.1.as_str()), Some("hunter22"));
    assert!(!h.texts(p).iter().any(|t| t.contains("hunter22")), "the password doesn't show");
}

#[test]
fn the_clock_lists_whats_next_and_the_world_clocks() {
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let p = open(&mut h, TrayWidget::Clock);
    let texts = h.texts(p);
    for want in ["Now Release review", "Sun 09:30 Dentist", "Tokyo 11:42", "Lisbon 03:42"] {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
    // The panel is as tall as its rows.
    let data = h.program().panel_data(TrayWidget::Clock);
    assert_eq!(h.size(p).height, aizome_shell::systray::panels::size(&data).height);
    // An event opens Koyomi at its day (the panel makes way).
    h.click_text(p, "Sun 09:30 Dentist");
    assert!(h.program().panel.is_none());
}

#[test]
fn the_clock_panel_reads_the_agenda() {
    // From the shell's own reading (not a fixture): Koyomi's rows and
    // Tokei's places, as the panel shows them.
    let mut h = scene(false);
    h.program_mut().agenda = aizome_shell::agenda::Agenda {
        events: Some(vec![aizome_shell::agenda::Event {
            title: "Standup".into(),
            start: "2026-10-17T09:30:00-05:00".into(),
            date: "2026-10-17".into(),
            color: "#3a6ea5".into(),
            ..Default::default()
        }]),
        places: vec![aizome_shell::agenda::Place { name: "Tokyo".into(), zone: "Asia/Tokyo".into() }],
    };
    h.program_mut().now = Local { year: 2026, month: 10, day: 17, hour: 8, minute: 0, second: 0, weekday: 6 };
    let Panel::Clock(c) = h.program().panel_data(TrayWidget::Clock) else { panic!("the clock's") };
    let events = c.events.expect("Koyomi's");
    assert_eq!((events[0].when.as_str(), events[0].title.as_str()), ("09:30", "Standup"));
    assert_eq!(c.zones.len(), 1);
    assert_eq!(c.zones[0].name, "Tokyo");
    // No Koyomi: no events section at all.
    h.program_mut().agenda.events = None;
    let Panel::Clock(c) = h.program().panel_data(TrayWidget::Clock) else { panic!("the clock's") };
    assert!(c.events.is_none());
}

#[test]
fn pictures_of_the_panels() {
    for day in [false, true] {
        let mut h = scene(day);
        h.program_mut().panels_fixture = Some(fixture());
        for w in [
            TrayWidget::Network,
            TrayWidget::Sound,
            TrayWidget::Bluetooth,
            TrayWidget::Display,
            TrayWidget::Battery,
            TrayWidget::Clock,
        ] {
            let p = open(&mut h, w);
            let name = format!("panel-{}-{}.png", w.name(), if day { "day" } else { "night" });
            h.save_png(p, shots().join(&name), 2.0);
            if w == TrayWidget::Network || w == TrayWidget::Sound {
                aizome_kit::testing::assert_golden(&mut h, p, golden(&name), 2.0);
            }
            h.send(Msg::Panel(PanelMsg::Close));
        }
    }
}

#[test]
fn escape_closes_a_panel() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    for w in [TrayWidget::Sound, TrayWidget::Network, TrayWidget::Clock] {
        let p = open(&mut h, w);
        // The compositor gives the popup the keyboard as it maps it.
        h.focus_window(p, true);
        h.named(p, NamedKey::Escape, Modifiers::NONE);
        assert!(h.program().panel.is_none(), "Escape closes the {} panel", w.name());
    }
}

/// Asked for over the shell's socket (Settings, a key, aizomectl), a panel
/// opens under its widget, as a press on it would put it.
#[test]
fn a_panel_asked_for_over_the_socket_opens_under_its_widget() {
    use aizome_shell::apps::server::ServerEvent;
    use aizome_shell_proto as proto;
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let bar = window_titled(&h, "Menu bar");
    let ask = |h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, what: &str| {
        h.send(Msg::Server(ServerEvent::Message(
            1,
            proto::AppMessage::Request { request: proto::ShellRequest::Open { what: what.into() } },
        )));
    };
    for w in [TrayWidget::Sound, TrayWidget::Network, TrayWidget::Clock] {
        let widget = h.find_id(bar, &aizome_shell::menubar::tray_id(w)).expect("the widget in the bar");
        ask(&mut h, w.name());
        let open = h.program().panel.as_ref().expect("a panel");
        assert_eq!(open.widget, w);
        assert_eq!(open.under, widget, "{} not under its widget", w.name());
        let spec = &h.platform.windows.values().last().unwrap().spec;
        match spec.kind {
            aizome_kit::platform::OpenKind::Popup { anchor, .. } => {
                assert!((anchor.x1 - (widget.x1 + 10.0)).abs() < 0.5, "{anchor:?} against {widget:?}");
            }
            ref other => panic!("{other:?}"),
        }
        // Asked again: it closes.
        ask(&mut h, w.name());
        assert!(h.program().panel.is_none());
    }
}

/// Keep Awake: its cup shows in the bar only while it's on, red while it's
/// on without its locks; it opens its panel, and turning it off there
/// takes the panel and the cup away. The Battery panel and the palette's
/// command switch it too. (A test's shell keeps Keep Awake to itself.)
#[test]
fn keep_awake_shows_its_cup_and_switches_from_its_panels() {
    use aizome_awake::Status;
    use aizome_shell::actions::Action;
    let mut h = scene(false);
    h.program_mut().panels_fixture = Some(fixture());
    let bar = window_titled(&h, "Menu bar");
    let cup = aizome_shell::menubar::tray_id(TrayWidget::Awake);
    assert!(h.find_id(bar, &cup).is_none(), "a cup while it's off");

    let p = open(&mut h, TrayWidget::Battery);
    assert!(h.texts(p).iter().any(|t| t == "Keep Awake"), "{:?}", h.texts(p));
    h.send(Msg::Panel(PanelMsg::KeepAwake(true)));
    assert_eq!(h.program().awake, Status::On);
    let data = h.program().panel_data(TrayWidget::Battery);
    assert_eq!(h.size(p).height, aizome_shell::systray::panels::size(&data).height);
    h.send(Msg::Panel(PanelMsg::Close));
    assert!(h.find_id(bar, &cup).is_some(), "no cup while it's on");

    let p = open(&mut h, TrayWidget::Awake);
    assert!(h.texts(p).iter().any(|t| t.starts_with("Won't sleep")), "{:?}", h.texts(p));
    let data = h.program().panel_data(TrayWidget::Awake);
    assert_eq!(h.size(p).height, aizome_shell::systray::panels::size(&data).height);
    h.save_png(p, shots().join("panel-keep-awake-night.png"), 2.0);
    // On, but logind doesn't list the locks: it says so.
    h.send(Msg::Awake(Status::Unprotected));
    assert!(h.texts(p).iter().any(|t| t.starts_with("Not working")), "{:?}", h.texts(p));
    h.save_png(bar, shots().join("bar-keep-awake-unprotected.png"), 2.0);

    // Off from its own panel: the panel goes with the cup.
    h.send(Msg::Panel(PanelMsg::KeepAwake(false)));
    assert!(h.program().panel.is_none());
    assert!(h.find_id(bar, &cup).is_none(), "a cup after it's off");

    // The palette's command.
    h.send(Msg::Run(Action::ToggleKeepAwake));
    assert_eq!(h.program().awake, Status::On);
    h.send(Msg::Run(Action::ToggleKeepAwake));
    assert_eq!(h.program().awake, Status::Off);
}
