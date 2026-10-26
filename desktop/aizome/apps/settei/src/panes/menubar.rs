//! Menu Bar: the clock (24-hour, seconds, the date) and which of the
//! systray's widgets and meters show, with the bar drawn as it will look.
//! `[menubar]`, which the shell follows.

use aizome_config::{ClockFormat, MenubarSettings, SYSTRAY_WIDGETS};
use aizome_kit::prelude::*;
use aizome_kit::row;

use crate::form;
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::widgets::{Segment, mark};

#[derive(Default)]
pub struct MenuBar;

#[derive(Clone, Debug)]
pub enum Msg {
    Widget(String, bool),
    BatteryPercent(bool),
    H24(bool),
    Seconds(bool),
    Date(bool),
}

/// What each widget is called and drawn with.
pub fn widget_info(w: &str) -> (&'static str, &'static str, Icon) {
    match w {
        "cpu" => ("Processor meter", "A bar for each core, amber when it's busy", Icon::Cpu),
        "memory" => ("Memory meter", "How full the memory is", Icon::MemoryStick),
        "network" => ("Network", "Wi-Fi and wired connections, VPNs", Icon::Wifi),
        "sound" => ("Sound", "Volume, devices and what's playing", Icon::Volume2),
        "bluetooth" => ("Bluetooth", "Paired devices", Icon::Bluetooth),
        "display" => ("Display", "Brightness, Night and Day, size", Icon::SunMoon),
        "battery" => ("Battery", "Charge and power mode", Icon::BatteryMedium),
        _ => ("Widget", "", Icon::Circle),
    }
}

/// The menu bar as it will look, in this theme: the mark, Toolchest,
/// this app's name, the Desks strip, the widgets chosen, the clock.
fn preview(m: &MenubarSettings) -> Element<Msg> {
    let mut bar = row![
        mark(2.0),
        text("Toolchest").size(12.0).weight(650.0).condensed(),
        space(6.0),
        text("Settei").size(12.0).weight(750.0),
        text("Edit").size(12.0),
        text("View").size(12.0),
        spacer(),
        container(text("1 Mail").size(11.0)).padding([1.0, 6.0]),
        container(text("2 Build").size(11.0).role(Role::OnSelection)).padding([1.0, 6.0]).fill(Tone::Selection),
        space(8.0),
    ]
    .spacing(8.0)
    .align(Align::Center)
    .padding([0.0, 8.0])
    .height(24.0)
    .width(Fill);
    for w in SYSTRAY_WIDGETS {
        if !m.shows(w) {
            continue;
        }
        bar = match w {
            "cpu" => bar.push(
                meter(vec![0.3, 0.55, 0.2, 0.8, 0.4, 0.25, 0.6, 0.35])
                    .role(Role::Accent)
                    .bar_width(3.0)
                    .gap(1.0)
                    .height(14.0),
            ),
            "memory" => bar.push(meter(vec![0.62]).role(Role::Accent).bar_width(5.0).height(14.0)),
            "battery" => {
                let b = bar.push(icon(Icon::BatteryMedium).size(15.0).role(Role::Text2));
                if m.battery_percent { b.push(text("84%").mono().size(11.0).role(Role::Text2)) } else { b }
            }
            other => bar.push(icon(widget_info(other).2).size(15.0).role(Role::Text2)),
        };
    }
    let (h, mi, s) = (21, 42, 7);
    let clock = super::general::clock_text(h, mi, s, m.clock == ClockFormat::H24, m.clock_seconds);
    let clock = if m.clock_date { format!("Sat 17  {clock}") } else { clock };
    bar = bar.push(space(4.0)).push(text(clock).mono().size(12.0).weight(600.0).tabular());
    let _ = Segment::Ai;
    container(bar).fill(Tone::Bar).border(Edge::BevelLo).width(Fill).into()
}

impl Pane for MenuBar {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "menubar"
    }

    fn title(&self) -> &'static str {
        "Menu Bar"
    }

    fn icon(&self) -> Icon {
        Icon::Menu
    }

    fn tile(&self) -> Tile {
        Tile::Kon
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["systray", "status bar", "top bar", "clock", "tray"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        let mut v = vec![
            SettingEntry::new("clock-24h", "24-hour clock").keywords(&["time format", "am pm"]),
            SettingEntry::new("clock-seconds", "Show seconds").keywords(&["clock"]),
            SettingEntry::new("clock-date", "Show the date").keywords(&["clock", "weekday"]),
            SettingEntry::new("battery-percent", "Battery percentage").keywords(&["charge"]),
        ];
        for w in SYSTRAY_WIDGETS {
            let (title, _, _) = widget_info(w);
            v.push(SettingEntry::new(format!("widget-{w}"), title).keywords(&["systray", "show", "hide", "menu bar"]));
        }
        v
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        cx.change(|s| {
            let mut m = s.menubar_settings();
            match msg {
                Msg::Widget(w, on) => {
                    m.widgets.retain(|x| *x != w);
                    if on {
                        // Keep the systray's order.
                        let mut all: Vec<String> = SYSTRAY_WIDGETS.iter().map(|s| s.to_string()).collect();
                        all.retain(|x| m.widgets.contains(x) || *x == w);
                        m.widgets = all;
                    }
                }
                Msg::BatteryPercent(on) => m.battery_percent = on,
                Msg::H24(on) => m.clock = if on { ClockFormat::H24 } else { ClockFormat::H12 },
                Msg::Seconds(on) => m.clock_seconds = on,
                Msg::Date(on) => m.clock_date = on,
            }
            s.set_shell_section("menubar", &m);
        });
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let m = vcx.settings().menubar_settings();
        let widgets: Vec<form::Row<Msg>> = SYSTRAY_WIDGETS
            .iter()
            .map(|w| {
                let (title, about, glyph) = widget_info(w);
                let id = w.to_string();
                form::row(title, toggle(m.shows(w)).on_toggle(move |on| Msg::Widget(id.clone(), on)))
                    .leading(icon(glyph).size(16.0).role(Role::Text2))
                    .detail(about)
                    .entry(format!("widget-{w}"))
                    .compact()
            })
            .collect();
        form::page(vec![
            vcx.section_with(
                "Preview",
                vec![form::whole(container(preview(&m)).padding([14.0, form::INSET]).width(Fill))],
                Some("A widget whose hardware is missing hides by itself: no battery on a desktop, no Bluetooth without an adapter.".into()),
            ),
            vcx.section(
                "Clock",
                vec![
                    form::row("24-hour clock", toggle(m.clock == ClockFormat::H24).on_toggle(Msg::H24)).entry("clock-24h"),
                    form::row("Show seconds", toggle(m.clock_seconds).on_toggle(Msg::Seconds)).entry("clock-seconds"),
                    form::row("Show the date", toggle(m.clock_date).on_toggle(Msg::Date)).entry("clock-date"),
                ],
            ),
            vcx.section("Widgets", widgets),
            vcx.section(
                "Battery",
                vec![form::row("Show the percentage", toggle(m.battery_percent).on_toggle(Msg::BatteryPercent)).entry("battery-percent")],
            ),
        ])
    }
}
