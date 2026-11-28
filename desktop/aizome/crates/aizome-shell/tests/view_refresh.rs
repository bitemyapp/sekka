mod common;

use aizome_shell::shell::{Msg, Surface};
use common::*;

fn fixed_night(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>) {
    h.program_mut().settings.appearance.theme = aizome_config::ThemeChoice::Night;
    h.program_mut().theme_day = false;
}

#[test]
fn a_meter_reading_rebuilds_only_the_bars_on_each_output() {
    let mut h = scene(false);
    let mut second = output();
    second.id = 2;
    second.name = "DP-1".into();
    second.connector = Some("DP-1".into());
    h.set_outputs(vec![output(), second]);
    let bars = h.program().surfaces.values().filter(|s| matches!(s, Surface::MenuBar { .. })).count();
    assert_eq!(bars, 2);
    assert!(h.windows().len() > bars);
    h.reset_stats();

    let mut reading = meters();
    reading.cores.fill(95.0);
    h.send(Msg::Meters(reading.clone()));
    assert_eq!(h.stats().view_layout.len(), bars);
    assert_eq!(h.program().meters, reading);
}

#[test]
fn a_clock_update_does_not_rebuild_the_desktop_and_deskbar() {
    let mut h = scene(false);
    fixed_night(&mut h);
    let bars = h.program().surfaces.values().filter(|s| matches!(s, Surface::MenuBar { .. })).count();
    assert!(h.windows().len() > bars);
    h.reset_stats();
    h.send(Msg::TimeChanged);
    assert_eq!(h.stats().view_layout.len(), bars);
    let label = h.program().now.bar_label(
        h.program().bar_cfg.clock,
        h.program().bar_cfg.clock_seconds,
        h.program().bar_cfg.clock_date,
    );
    let bar = window_titled(&h, "Menu bar");
    assert!(h.texts(bar).contains(&label));
}

#[test]
fn clock_and_agenda_updates_keep_the_open_panel_current() {
    use aizome_kit::prelude::Rect;
    use aizome_shell::agenda::{Agenda, Place};
    use aizome_shell::menubar::{BarMsg, Slot, TrayWidget};
    use aizome_shell::systray::panels::PanelMsg;

    let mut h = scene(false);
    fixed_night(&mut h);
    let bar = window_titled(&h, "Menu bar");
    h.send(Msg::Bar(bar, BarMsg::Press(Slot::Tray(TrayWidget::Clock), Rect::new(1300.0, 0.0, 1315.0, 24.0))));
    let panel = h.program().panel.as_ref().unwrap().window;
    h.reset_stats();
    h.send(Msg::TimeChanged);
    assert_eq!(h.stats().view_layout.len(), 2, "bar and clock panel");

    h.reset_stats();
    h.send(Msg::Agenda(Agenda {
        places: vec![Place { name: "Tokyo".into(), zone: "Asia/Tokyo".into() }],
        ..Default::default()
    }));
    assert_eq!(h.stats().view_layout.len(), 1, "only the clock panel");
    assert!(h.texts(panel).iter().any(|t| t.starts_with("Tokyo ")));

    h.send(Msg::Panel(PanelMsg::Close));
    h.reset_stats();
    h.send(Msg::Agenda(Agenda::default()));
    assert!(h.stats().view_layout.is_empty(), "a late agenda result has no visible consumer");
}

#[test]
fn clock_updates_reach_lock_screens_and_theme_transitions_refresh_the_shell() {
    use aizome_shell::actions::{Action, SessionVerb};

    let mut h = scene(false);
    fixed_night(&mut h);
    h.program_mut().theme_day = true;
    h.reset_stats();
    h.send(Msg::TimeChanged);
    assert_eq!(h.stats().view_layout.len(), h.windows().len(), "theme transition refreshes every surface");

    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    h.confirm_lock();
    let lock = h.program().lock.surfaces[0].0;
    h.reset_stats();
    h.send(Msg::TimeChanged);
    let time = h.program().now.time_label(h.program().bar_cfg.clock, false);
    assert!(h.texts(lock).contains(&time));
    assert_eq!(h.stats().view_layout.len(), 2, "bar and lock screen");
}
