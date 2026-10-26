//! Battery & Power: the battery's charge, time left and health with the
//! last day's charge drawn; the power mode; when the screen turns off; and
//! what the power key and the lid do, which logind decides (NixOS pins
//! logind.conf, so they show as the system configuration's).

use std::sync::Arc;
use std::time::Duration;

use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::kurbo::BezPath;
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};
use aizome_kit::{FontSpec, column, draw, row};
use aizome_sys::power::{
    BatteryState, HistoryPoint, PowerDevice, PowerKind, PowerProfiles, Profile, ProfilesState, Upower, UpowerState,
};

use super::lock::{minutes_at, minutes_choice};
use crate::extra;
use crate::form;
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::system::Cell;

pub enum Batteries {
    Live(Upower),
    Sample(Cell<UpowerState>),
}

pub enum Modes {
    Live(PowerProfiles),
    Sample(Cell<ProfilesState>),
}

/// What logind does with the power key and the lid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Logind {
    pub power_key: String,
    pub lid: String,
    pub lid_on_power: String,
    pub lid_docked: String,
    /// logind.conf comes from the system configuration.
    pub managed: bool,
}

#[derive(Default)]
pub struct Battery {
    batteries: Option<Batteries>,
    modes: Option<Modes>,
    history: Option<Arc<Vec<HistoryPoint>>>,
    logind: Option<Logind>,
    /// Keep Awake is on (`None` until read).
    awake: Option<bool>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    History(Arc<Vec<HistoryPoint>>),
    Logind(Logind),
    Mode(usize),
    ScreenOff(usize),
    LockAfter(usize),
    PowerKey(usize),
    Percent(bool),
    /// Keep Awake turned on or off here.
    KeepAwake(bool),
    /// Keep Awake as it is now (read, or after `aizome-awake on`/`off`).
    Awake(bool),
    Done(&'static str, Result<(), String>),
}

pub const POWER_KEY: [(&str, &str); 5] = [
    ("ask", "Ask what to do"),
    ("sleep", "Sleep"),
    ("lock", "Lock the screen"),
    ("off", "Shut down"),
    ("nothing", "Nothing"),
];

/// logind.conf's settings, from the file and its drop-ins (later wins).
pub fn read_logind() -> Logind {
    let mut l = Logind {
        power_key: "poweroff".into(),
        lid: "suspend".into(),
        lid_on_power: String::new(),
        lid_docked: "ignore".into(),
        managed: false,
    };
    let main = std::path::Path::new("/etc/systemd/logind.conf");
    let mut files = vec![main.to_path_buf()];
    for dir in ["/etc/systemd/logind.conf.d", "/run/systemd/logind.conf.d"] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut v: Vec<_> =
                rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "conf")).collect();
            v.sort();
            files.extend(v);
        }
    }
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        if std::fs::read_link(f).is_ok_and(|t| t.starts_with("/etc/static") || t.starts_with("/nix/store"))
            || f.starts_with("/etc/static")
        {
            l.managed = true;
        }
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim().to_owned();
            match k.trim() {
                "HandlePowerKey" => l.power_key = v,
                "HandleLidSwitch" => l.lid = v,
                "HandleLidSwitchExternalPower" => l.lid_on_power = v,
                "HandleLidSwitchDocked" => l.lid_docked = v,
                _ => {}
            }
        }
    }
    l
}

/// What a logind action is called.
pub fn logind_words(a: &str) -> &str {
    match a {
        "suspend" => "Sleep",
        "hibernate" => "Hibernate",
        "hybrid-sleep" => "Sleep, ready to hibernate",
        "suspend-then-hibernate" => "Sleep, then hibernate",
        "poweroff" => "Shut down",
        "reboot" => "Restart",
        "lock" => "Lock the screen",
        "ignore" => "Nothing (the desktop decides)",
        "halt" => "Halt",
        "" => "As on battery",
        other => other,
    }
}

/// "5 h 3 min".
pub fn duration_words(d: Duration) -> String {
    let m = d.as_secs() / 60;
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

fn profile_title(name: &str) -> &str {
    match name {
        "power-saver" => "Power Saver",
        "balanced" => "Balanced",
        "performance" => "Performance",
        other => other,
    }
}

fn sample_battery() -> UpowerState {
    let battery = PowerDevice {
        path: "/org/freedesktop/UPower/devices/battery_BAT0".into(),
        native_path: "BAT0".into(),
        kind: PowerKind::Battery,
        vendor: "Sunwoda".into(),
        model: "5B11M90061".into(),
        power_supply: true,
        is_present: true,
        percentage: 84.0,
        state: BatteryState::Discharging,
        time_to_empty: Some(Duration::from_secs(5 * 3600 + 3 * 60)),
        energy_rate: 8.7,
        energy: 44.0,
        energy_full: 52.4,
        energy_full_design: 57.0,
        capacity: 91.9,
        charge_cycles: Some(212),
        temperature: Some(31.0),
        ..PowerDevice::default()
    };
    let mouse = PowerDevice {
        path: "/org/freedesktop/UPower/devices/mouse_hidpp".into(),
        kind: PowerKind::Mouse,
        vendor: "Logitech".into(),
        model: "MX Master 3S".into(),
        is_present: true,
        percentage: 62.0,
        state: BatteryState::Discharging,
        ..PowerDevice::default()
    };
    UpowerState {
        available: true,
        on_battery: true,
        lid_present: true,
        display: Some(battery.clone()),
        devices: vec![battery, mouse],
        ..UpowerState::default()
    }
}

fn sample_profiles() -> ProfilesState {
    let p = |n: &str| Profile { name: n.into(), driver: "platform_profile".into() };
    ProfilesState {
        available: true,
        active: "balanced".into(),
        profiles: vec![p("power-saver"), p("balanced"), p("performance")],
        ..ProfilesState::default()
    }
}

/// The sample's last day: charged overnight, unplugged at nine, a top-up
/// at lunch, at 84% now.
fn sample_history(now: u32) -> Vec<HistoryPoint> {
    let mut v = Vec::new();
    let h = 3600u32;
    for i in 0..=96u32 {
        let t = now - 24 * h + i * 900;
        let hours = f64::from(i) / 4.0; // 0 = 24 h ago
        let (value, state) = if hours < 6.0 {
            (38.0 - hours * 3.0, BatteryState::Discharging)
        } else if hours < 9.0 {
            ((20.0 + (hours - 6.0) * 27.0).min(100.0), BatteryState::Charging)
        } else if hours < 13.0 {
            (100.0, BatteryState::FullyCharged)
        } else if hours < 19.0 {
            (100.0 - (hours - 13.0) * 7.5, BatteryState::Discharging)
        } else if hours < 20.5 {
            (55.0 + (hours - 19.0) * 22.0, BatteryState::Charging)
        } else {
            (88.0 - (hours - 20.5) * 1.15, BatteryState::Discharging)
        };
        v.push(HistoryPoint { time: t, value, state });
    }
    v
}

impl Battery {
    fn state(&self) -> Arc<UpowerState> {
        match &self.batteries {
            Some(Batteries::Live(u)) => u.snapshot(),
            Some(Batteries::Sample(c)) => c.get(),
            None => Arc::new(UpowerState::default()),
        }
    }

    fn profiles(&self) -> Arc<ProfilesState> {
        match &self.modes {
            Some(Modes::Live(p)) => p.snapshot(),
            Some(Modes::Sample(c)) => c.get(),
            None => Arc::new(ProfilesState::default()),
        }
    }

    fn fetch_history(&mut self, cx: &mut PaneCx<Msg>) {
        let Some(Batteries::Live(u)) = &self.batteries else { return };
        let Some(b) = u.snapshot().batteries().next().cloned() else { return };
        let task = u.history(&b.path, "charge", 86_400, 200);
        cx.wait(task, |r| Msg::History(Arc::new(r.unwrap_or_default())));
    }
}

impl Pane for Battery {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "battery"
    }

    fn title(&self) -> &'static str {
        "Battery & Power"
    }

    fn icon(&self) -> Icon {
        Icon::BatteryFull
    }

    fn tile(&self) -> Tile {
        Tile::Green
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["energy", "power", "charge", "sleep", "suspend"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("charge", "Battery charge").keywords(&["level", "time left", "percentage"]),
            SettingEntry::new("history", "Charge over the last day").keywords(&["usage", "graph"]),
            SettingEntry::new("health", "Battery health").keywords(&["capacity", "cycles", "condition", "wear"]),
            SettingEntry::new("mode", "Power mode").keywords(&[
                "power saver",
                "balanced",
                "performance",
                "profile",
                "low power",
            ]),
            SettingEntry::new("screen-off", "Turn the screen off after").keywords(&["display sleep", "blank", "idle"]),
            SettingEntry::new("lock-after", "Lock after").keywords(&["idle", "screen lock"]),
            SettingEntry::new("power-key", "Power key").keywords(&["power button", "shutdown"]),
            SettingEntry::new("lid", "When the lid closes").keywords(&["lid switch", "clamshell", "suspend"]),
            SettingEntry::new("percent", "Show the percentage in the menu bar").keywords(&["battery percent"]),
            SettingEntry::new("devices", "Devices' batteries").keywords(&["mouse", "keyboard", "headphones"]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        let sample = cx.sample();
        if self.batteries.is_none() {
            self.batteries = Some(if sample {
                Batteries::Sample(Cell::new(sample_battery(), cx.wake()))
            } else {
                Batteries::Live(Upower::new(cx.wake()))
            });
            self.modes = Some(if sample {
                Modes::Sample(Cell::new(sample_profiles(), cx.wake()))
            } else {
                Modes::Live(PowerProfiles::new(cx.wake()))
            });
        }
        if sample {
            self.history = Some(Arc::new(sample_history(1_792_273_320)));
            self.logind = Some(Logind {
                power_key: "poweroff".into(),
                lid: "suspend".into(),
                lid_on_power: String::new(),
                lid_docked: "ignore".into(),
                managed: true,
            });
        } else {
            self.fetch_history(cx);
            cx.spawn(|| Msg::Logind(read_logind()));
        }
        // The setting, not whether logind holds the locks: the menu bar's
        // cup says that (docs/keep-awake.md).
        self.awake = Some(!sample && aizome_awake::requested());
    }

    fn system_changed(&mut self, cx: &mut PaneCx<Msg>) {
        // UPower appears after a moment: ask for the history once it has.
        if self.history.is_none() && !cx.sample() && self.state().batteries().next().is_some() {
            self.history = Some(Arc::new(Vec::new()));
            self.fetch_history(cx);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::History(h) => self.history = Some(h),
            Msg::Logind(l) => self.logind = Some(l),
            Msg::Mode(i) => {
                let p = self.profiles();
                let Some(name) = p.profiles.get(i).map(|p| p.name.clone()) else { return };
                let task = match &self.modes {
                    Some(Modes::Live(m)) => m.set_active(&name),
                    Some(Modes::Sample(c)) => c.update(move |s| s.active = name),
                    None => return,
                };
                cx.wait(task, |r| Msg::Done("Power mode", r.map_err(|e| e.to_string())));
            }
            Msg::ScreenOff(i) => {
                cx.change(|s| s.idle.monitors_off_after_minutes = minutes_at(s.idle.monitors_off_after_minutes, i))
            }
            Msg::LockAfter(i) => cx.change(|s| s.idle.lock_after_minutes = minutes_at(s.idle.lock_after_minutes, i)),
            Msg::PowerKey(i) => {
                cx.change(|s| extra::set(&mut s.power, "power_key", POWER_KEY[i.min(POWER_KEY.len() - 1)].0))
            }
            Msg::Percent(on) => cx.change(|s| {
                let mut m = s.menubar_settings();
                m.battery_percent = on;
                s.set_shell_section("menubar", &m);
            }),
            Msg::KeepAwake(on) => {
                self.awake = Some(on);
                if cx.sample() {
                    return;
                }
                cx.spawn(move || {
                    let out = std::process::Command::new("aizome-awake").arg(if on { "on" } else { "off" }).output();
                    match out {
                        Ok(o) if o.status.success() => Msg::Awake(on),
                        Ok(o) => Msg::Done(
                            "Keep Awake",
                            Err(String::from_utf8_lossy(&o.stderr)
                                .trim()
                                .trim_start_matches("aizome-awake: ")
                                .to_owned()),
                        ),
                        Err(e) => Msg::Done("Keep Awake", Err(format!("couldn't run aizome-awake: {e}"))),
                    }
                });
            }
            Msg::Awake(on) => self.awake = Some(on),
            Msg::Done(_, Ok(())) => {}
            Msg::Done(what, Err(e)) => {
                if what == "Keep Awake" {
                    self.awake = Some(aizome_awake::requested());
                }
                cx.failed(what, e)
            }
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let st = self.state();
        let s = vcx.settings();
        let mut sections = Vec::new();
        if let Some(b) = st.batteries().next() {
            let (line, role) = match b.state {
                BatteryState::Charging => (
                    b.time_to_full
                        .map_or_else(|| "Charging".to_owned(), |d| format!("Charging, full in {}", duration_words(d))),
                    Role::Success,
                ),
                BatteryState::FullyCharged => ("Charged".to_owned(), Role::Success),
                BatteryState::PendingCharge => ("Plugged in, not charging".to_owned(), Role::Text2),
                _ if st.plugged_in() => ("Plugged in".to_owned(), Role::Text2),
                _ => (
                    b.time_to_empty
                        .map_or_else(|| "On battery".to_owned(), |d| format!("On battery, {} left", duration_words(d))),
                    Role::Text2,
                ),
            };
            let hero = row![
                gauge(b.percentage, b.state == BatteryState::Charging),
                column![
                    text(format!("{:.0}%", b.percentage)).mono().size(30.0).weight(600.0).tabular(),
                    text(line).role(role),
                    text(if b.energy_rate > 0.0 { format!("{:.1} W", b.energy_rate) } else { String::new() })
                        .mono()
                        .size(11.0)
                        .role(Role::Text3),
                ]
                .spacing(3.0),
            ]
            .spacing(18.0)
            .align(Align::Center);
            let history: Element<Msg> = match &self.history {
                Some(h) if h.len() > 1 => chart(h.clone(), if vcx.sample() { 1_792_273_320 } else { now() }),
                _ => container(
                    text("No history yet: UPower records it as the battery is used.").size(11.0).role(Role::Text3),
                )
                .padding([8.0, 0.0])
                .into(),
            };
            sections.push(vcx.section(
                "Battery",
                vec![
                        form::whole(container(hero).padding([14.0, form::INSET]).width(Fill)).entry("charge"),
                        form::whole(
                            container(column![form::caption::<Msg>("Last 24 hours"), history].spacing(6.0))
                                .padding([4.0, form::INSET, 12.0, form::INSET])
                                .width(Fill),
                        )
                        .entry("history"),
                    ],
            ));
            let mut health = vec![
                form::value("Maximum capacity", format!("{:.0}%", b.capacity))
                    .entry("health")
                    .help("How much it holds now, against what it held new. Under 80% it's worth replacing."),
            ];
            if let Some(c) = b.charge_cycles {
                health.push(form::value_mono("Charge cycles", c.to_string()));
            }
            if b.energy_full_design > 0.0 {
                health.push(form::value_mono(
                    "Holds",
                    format!("{:.1} Wh of {:.1} Wh", b.energy_full, b.energy_full_design),
                ));
            }
            if !b.model.is_empty() {
                health.push(form::value_mono("Battery", format!("{} {}", b.vendor, b.model).trim().to_owned()));
            }
            sections.push(vcx.section("Health", health));
        }

        let p = self.profiles();
        if p.available && !p.profiles.is_empty() {
            let names: Vec<&str> = p.profiles.iter().map(|x| profile_title(&x.name)).collect();
            let i = p.profiles.iter().position(|x| x.name == p.active).unwrap_or(0);
            let mut help = match p.active.as_str() {
                "power-saver" => "Slower and cooler, for the longest time on battery.".to_owned(),
                "performance" => "Everything at full speed, louder and warmer.".to_owned(),
                _ => "Speed when it's needed, quiet when it isn't.".to_owned(),
            };
            if !p.performance_degraded.is_empty() {
                help.push_str(&format!(" Performance is held back: {}.", p.performance_degraded.replace('-', " ")));
            }
            for h in &p.holds {
                help.push_str(&format!(" {} holds {} ({}).", h.application_id, profile_title(&h.profile), h.reason));
            }
            sections.push(vcx.section(
                "Power Mode",
                vec![form::row("Power mode", form::segments(&names, i, Msg::Mode)).entry("mode").help(help)],
            ));
        }

        if let Some(on) = self.awake {
            sections.push(vcx.section(
                "Keep Awake",
                vec![
                    form::row("Keep the computer awake", toggle(on).on_toggle(Msg::KeepAwake))
                        .entry("keep-awake")
                        .help(
                            "It won't sleep, lock or turn the displays off until this is off again: closing the lid \
                             just turns its screen off. A cup in the menu bar shows meanwhile. Sleep chosen from the \
                             menu still sleeps.",
                        ),
                ],
            ));
        }

        let (off, off_i) = minutes_choice(s.idle.monitors_off_after_minutes);
        let (lock, lock_i) = minutes_choice(s.idle.lock_after_minutes);
        sections.push(vcx.section(
            "Screen",
            vec![
                    form::row(
                        "Turn the screen off after",
                        dropdown(off, off_i).on_select(Msg::ScreenOff).min_width(150.0),
                    )
                    .entry("screen-off"),
                    form::row("Lock after", dropdown(lock, lock_i).on_select(Msg::LockAfter).min_width(150.0))
                        .entry("lock-after"),
                ],
        ));

        if let Some(l) = &self.logind {
            let mut rows = Vec::new();
            if l.power_key == "ignore" {
                let cur = extra::get_str(&s.power, "power_key").unwrap_or("ask");
                let i = POWER_KEY.iter().position(|(k, _)| *k == cur).unwrap_or(0);
                rows.push(
                    form::row("The power key", form::choice(&POWER_KEY.map(|k| k.1), i, Msg::PowerKey))
                        .entry("power-key"),
                );
            } else if l.managed {
                rows.push(
                    form::managed(
                        "The power key",
                        logind_words(&l.power_key),
                        "Set services.logind.settings.Login.HandlePowerKey = \"ignore\" to choose here.",
                    )
                    .entry("power-key"),
                );
            } else {
                rows.push(form::value("The power key", logind_words(&l.power_key)).entry("power-key"));
            }
            // A computer without a lid (a desktop, a VM) has nothing to close.
            if st.lid_present {
                let lid = if l.managed {
                    form::managed(
                        "When the lid closes",
                        logind_words(&l.lid),
                        "services.logind.settings.Login.HandleLidSwitch",
                    )
                } else {
                    form::value("When the lid closes", logind_words(&l.lid))
                };
                rows.push(lid.entry("lid"));
                if !l.lid_on_power.is_empty() {
                    rows.push(form::value("…when plugged in", logind_words(&l.lid_on_power)));
                }
                rows.push(form::value("…with a display connected", logind_words(&l.lid_docked)));
            }
            sections.push(vcx.section(if st.lid_present { "Power Key and Lid" } else { "Power Key" }, rows));
        }

        let menubar = s.menubar_settings();
        let mut more = vec![
            form::row("Show the percentage in the menu bar", toggle(menubar.battery_percent).on_toggle(Msg::Percent))
                .entry("percent"),
        ];
        let peripherals: Vec<&PowerDevice> = st.peripherals().collect();
        for (i, d) in peripherals.iter().enumerate() {
            let glyph = match d.kind {
                PowerKind::Mouse => Icon::Mouse,
                PowerKind::Keyboard => Icon::Keyboard,
                PowerKind::Headset | PowerKind::Headphones => Icon::Headphones,
                PowerKind::Phone => Icon::Smartphone,
                PowerKind::Tablet => Icon::Tablet,
                PowerKind::Gamepad => Icon::Gamepad2,
                _ => Icon::Battery,
            };
            let mut r = form::row(
                format!("{} {}", d.vendor, d.model).trim().to_owned(),
                row![
                    progress(d.percentage / 100.0).width(80.0),
                    text(format!("{:.0}%", d.percentage)).mono().size(12.0).role(Role::Text2),
                ]
                .spacing(8.0)
                .align(Align::Center),
            )
            .leading(icon(glyph).size(16.0).role(Role::Text2))
            .compact();
            if i == 0 {
                r = r.entry("devices");
            }
            more.push(r);
        }
        sections.push(vcx.section("Also", more));
        if st.batteries().next().is_none() {
            sections.insert(0, form::note("This computer runs on mains power: there's no battery to show."));
        }
        form::page(sections)
    }
}

fn now() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32)
}

// The battery, drawn.

fn gauge<Msg>(percent: f64, charging: bool) -> Element<Msg> {
    Element::new(Gauge { percent, charging }, Vec::new())
}

struct Gauge {
    percent: f64,
    charging: bool,
}

impl Widget for Gauge {
    fn name(&self) -> &'static str {
        "battery-gauge"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(84.0, 40.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let body = Rect::new(0.0, 0.0, 78.0, 40.0);
        draw::sunken(cx.canvas, t, body, t.sunk(), t.line2());
        cx.canvas.fill_rect(Rect::new(78.0, 13.0, 84.0, 27.0), t.line2());
        let p = self.percent.clamp(0.0, 100.0) / 100.0;
        let fill = if self.charging || p > 0.2 {
            t.role(Role::Success)
        } else if p > 0.1 {
            t.role(Role::Warning)
        } else {
            t.role(Role::Danger)
        };
        let inner = Rect::new(4.0, 4.0, 74.0, 36.0);
        cx.canvas.fill_rect(Rect::new(inner.x0, inner.y0, (inner.x0 + inner.width() * p).round(), inner.y1), fill);
        if self.charging {
            Icon::Zap.draw(cx.canvas, Point::new(29.0, 10.0), 20.0, t.text());
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.percent).add(&self.charging);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Meter, range: Some((0.0, 100.0, self.percent)), ..Default::default() })
    }
}

// The charge history.

fn chart<Msg>(points: Arc<Vec<HistoryPoint>>, now: u32) -> Element<Msg> {
    Element::new(Chart { points, now }, Vec::new())
}

struct Chart {
    points: Arc<Vec<HistoryPoint>>,
    now: u32,
}

impl Widget for Chart {
    fn name(&self) -> &'static str {
        "charge-chart"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width.min(2000.0), 110.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let size = cx.size();
        let plot = Rect::new(0.0, 4.0, size.width - 34.0, size.height - 18.0);
        let start = self.now.saturating_sub(86_400);
        let x = |time: u32| plot.x0 + (f64::from(time.saturating_sub(start)) / 86_400.0) * plot.width();
        let y = |v: f64| plot.y1 - (v.clamp(0.0, 100.0) / 100.0) * plot.height();
        let label = TextSpec::new(FontSpec::mono(9.5), t.text3());
        for v in [0.0, 50.0, 100.0] {
            let yy = y(v).round();
            cx.canvas.fill_rect(Rect::new(plot.x0, yy, plot.x1, yy + 1.0), if v == 0.0 { t.line2() } else { t.line() });
            cx.draw_text_in(
                &format!("{v:.0}%"),
                &label,
                Rect::new(plot.x1 + 4.0, yy - 7.0, size.width, yy + 7.0),
                Align::Start,
            );
        }
        for (i, h) in [24, 18, 12, 6, 0].iter().enumerate() {
            let xx = x(self.now - h * 3600).round();
            cx.canvas.fill_rect(Rect::new(xx, plot.y0, xx + 1.0, plot.y1), t.line().with_alpha(0.5));
            let text = if *h == 0 { "now".to_owned() } else { format!("−{h} h") };
            let align = if i == 0 {
                Align::Start
            } else if *h == 0 {
                Align::End
            } else {
                Align::Center
            };
            let r = match align {
                Align::Start => Rect::new(xx, plot.y1 + 2.0, xx + 40.0, size.height),
                Align::End => Rect::new(xx - 40.0, plot.y1 + 2.0, xx, size.height),
                _ => Rect::new(xx - 20.0, plot.y1 + 2.0, xx + 20.0, size.height),
            };
            cx.draw_text_in(&text, &label, r, align);
        }
        let pts: Vec<&HistoryPoint> = self.points.iter().filter(|p| p.time >= start && p.time <= self.now).collect();
        if pts.len() < 2 {
            return;
        }
        // The area under the line, then the line, coloured by state.
        let mut area = BezPath::new();
        area.move_to((x(pts[0].time), plot.y1));
        for p in &pts {
            area.line_to((x(p.time), y(p.value)));
        }
        area.line_to((x(pts[pts.len() - 1].time), plot.y1));
        area.close_path();
        cx.canvas.fill(&area, t.role(Role::Accent).with_alpha(0.16));
        for w in pts.windows(2) {
            let charging = matches!(w[1].state, BatteryState::Charging | BatteryState::FullyCharged);
            let c = if charging { t.role(Role::Success) } else { t.role(Role::Accent) };
            let mut seg = BezPath::new();
            seg.move_to((x(w[0].time), y(w[0].value)));
            seg.line_to((x(w[1].time), y(w[1].value)));
            cx.canvas.stroke(&seg, &aizome_kit::sidestep_ui::StrokeStyle::new(2.0), c);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.points.len(), self.now));
        if let Some(p) = self.points.last() {
            h.f64(p.value).add(&p.time);
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Image,
            label: Some("Charge over the last day".into()),
            ..Default::default()
        })
    }
}
