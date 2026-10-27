//! Date & Time: network time, the time zone (by map or search), and how
//! the menu bar's clock reads. Through systemd-timedated; a time zone the
//! system configuration pins (`time.timeZone`) says so.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aizome_config::ClockFormat;
use aizome_kit::element::{MsgFn, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use aizome_kit::{TimerHandle, column, draw, row};
use aizome_sys::datetime::{TimeDate, TimeDateState, Zone};

use super::Msg as G;
use crate::form;
use crate::pane::{PaneCx, SettingEntry, ViewCx};
use crate::system::Cell;
use crate::tz;
use crate::widgets::{PressStyle, press};

pub enum Svc {
    Live(TimeDate),
    Sample(Cell<TimeDateState>),
}

impl Svc {
    fn snapshot(&self) -> Arc<TimeDateState> {
        match self {
            Svc::Live(t) => t.snapshot(),
            Svc::Sample(c) => c.get(),
        }
    }
}

/// The sample's moment: Saturday 17 October 2026, 01:09 UTC.
pub const SAMPLE_NOW: i64 = 1_792_199_340;

#[derive(Default)]
pub struct State {
    svc: Option<Svc>,
    zones: Option<Arc<Vec<Zone>>>,
    dir: Option<PathBuf>,
    query: String,
    now: i64,
    tick: Option<TimerHandle>,
    /// The system configuration enables network time (NixOS links its
    /// units into the store, so timedated can't switch it).
    ntp_managed: bool,
}

/// Whether systemd's units are the system configuration's (NixOS keeps
/// them in the store): timedated then can't turn network time on or off.
pub fn units_managed(units: &std::path::Path) -> bool {
    std::fs::canonicalize(units).is_ok_and(|p| p.starts_with("/nix/store"))
}

#[derive(Clone, Debug)]
pub enum Msg {
    Zones(Arc<Vec<Zone>>, Option<PathBuf>),
    Tick,
    Ntp(bool),
    Query(String),
    Pick(String),
    MapPick(f64, f64),
    Clock24(bool),
    ShowDate(bool),
    Seconds(bool),
    Done(&'static str, Result<(), String>),
}

pub fn entries() -> Vec<SettingEntry> {
    vec![
        SettingEntry::new("auto-time", "Set the time automatically").under("Date & Time").keywords(&[
            "ntp",
            "network time",
            "sync",
        ]),
        SettingEntry::new("timezone", "Time zone")
            .under("Date & Time")
            .keywords(&["timezone", "location", "city", "utc"]),
        SettingEntry::new("clock-24h", "24-hour time").under("Date & Time").keywords(&[
            "clock format",
            "am pm",
            "12-hour",
        ]),
        SettingEntry::new("clock-date", "Show the date in the menu bar")
            .under("Date & Time")
            .keywords(&["clock", "weekday"]),
        SettingEntry::new("clock-seconds", "Show seconds").under("Date & Time").keywords(&["clock"]),
    ]
}

fn sample_state() -> TimeDateState {
    TimeDateState {
        available: true,
        timezone: "Asia/Tokyo".into(),
        timezone_managed: false,
        local_rtc: false,
        can_ntp: true,
        ntp: true,
        ntp_synchronized: true,
    }
}

/// A few zones, for a computer without tzdata.
fn fallback_zones() -> Vec<Zone> {
    let z = |id: &str, country: &str, lat: f64, lon: f64| Zone {
        id: id.into(),
        region: id.split('/').next().unwrap_or("").into(),
        city: aizome_sys::datetime::zones::city_name(id),
        countries: Vec::new(),
        country_names: vec![country.into()],
        latitude: lat,
        longitude: lon,
        comment: String::new(),
    };
    vec![
        z("Asia/Tokyo", "Japan", 35.65, 139.74),
        z("Asia/Shanghai", "China", 31.23, 121.47),
        z("Asia/Kolkata", "India", 22.53, 88.37),
        z("Asia/Singapore", "Singapore", 1.28, 103.85),
        z("Asia/Dubai", "United Arab Emirates", 25.30, 55.30),
        z("Europe/London", "Britain (UK)", 51.51, -0.13),
        z("Europe/Paris", "France", 48.87, 2.33),
        z("Europe/Berlin", "Germany", 52.50, 13.37),
        z("Europe/Moscow", "Russia", 55.76, 37.62),
        z("Africa/Cairo", "Egypt", 30.05, 31.25),
        z("Africa/Lagos", "Nigeria", 6.45, 3.40),
        z("Africa/Johannesburg", "South Africa", -26.25, 28.0),
        z("America/New_York", "United States", 40.71, -74.01),
        z("America/Chicago", "United States", 41.85, -87.65),
        z("America/Denver", "United States", 39.74, -104.98),
        z("America/Los_Angeles", "United States", 34.05, -118.24),
        z("America/Sao_Paulo", "Brazil", -23.53, -46.62),
        z("America/Mexico_City", "Mexico", 19.40, -99.15),
        z("Australia/Sydney", "Australia", -33.87, 151.22),
        z("Pacific/Auckland", "New Zealand", -36.87, 174.77),
        z("UTC", "", 0.0, 0.0),
    ]
}

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "Saturday 17 October 2026" and the hours, minutes and seconds of a
/// local time (`now` plus the offset).
pub fn broken(local: i64) -> (String, u32, u32, u32) {
    let days = local.div_euclid(86400);
    let secs = local.rem_euclid(86400) as u32;
    let (y, m, d) = tz::civil_from_days(days);
    let date = format!("{} {d} {} {y}", WEEKDAYS[tz::weekday(days) as usize], MONTHS[(m - 1) as usize]);
    (date, secs / 3600, (secs / 60) % 60, secs % 60)
}

/// The clock as the menu bar would show it.
pub fn clock_text(h: u32, m: u32, s: u32, h24: bool, seconds: bool) -> String {
    let (hh, suffix) = if h24 {
        (format!("{h:02}"), "")
    } else {
        let h12 = if h.is_multiple_of(12) { 12 } else { h % 12 };
        (format!("{h12}"), if h < 12 { " AM" } else { " PM" })
    };
    if seconds { format!("{hh}:{m:02}:{s:02}{suffix}") } else { format!("{hh}:{m:02}{suffix}") }
}

impl State {
    fn snapshot(&self) -> Arc<TimeDateState> {
        self.svc.as_ref().map(Svc::snapshot).unwrap_or_default()
    }

    pub fn shown(&mut self, cx: &mut PaneCx<G>) {
        let sample = cx.sample();
        if self.svc.is_none() {
            self.svc = Some(if sample {
                Svc::Sample(Cell::new(sample_state(), cx.wake()))
            } else {
                Svc::Live(TimeDate::new(cx.wake()))
            });
        }
        if self.zones.is_none() {
            cx.spawn(|| {
                let dir = aizome_sys::datetime::zones::zoneinfo_dir();
                let zones = aizome_sys::datetime::zones().unwrap_or_else(|_| fallback_zones());
                G::Time(Msg::Zones(Arc::new(zones), dir))
            });
        }
        self.now = if sample { SAMPLE_NOW } else { unix_now() };
        self.ntp_managed = !sample && units_managed(std::path::Path::new("/etc/systemd/system"));
        if !sample && self.tick.is_none() {
            self.tick = Some(cx.every(Duration::from_secs(1), G::Time(Msg::Tick)));
        }
    }

    pub fn hidden(&mut self, cx: &mut PaneCx<G>) {
        if let Some(t) = self.tick.take() {
            cx.cancel(t);
        }
    }

    fn offset_of(&self, id: &str) -> Option<i32> {
        if id == "UTC" {
            return Some(0);
        }
        tz::offset(self.dir.as_deref()?, id, self.now)
    }

    fn set_zone(&mut self, id: String, cx: &mut PaneCx<G>) {
        let task = match &self.svc {
            Some(Svc::Live(t)) => t.set_timezone(&id),
            Some(Svc::Sample(c)) => {
                let i = id.clone();
                c.update(move |s| s.timezone = i)
            }
            None => return,
        };
        self.query.clear();
        cx.wait(task, |r| G::Time(Msg::Done("Changed the time zone", r.map_err(|e| e.to_string()))));
    }

    pub fn update(&mut self, msg: Msg, cx: &mut PaneCx<G>) {
        match msg {
            Msg::Zones(z, dir) => {
                self.zones = Some(z);
                self.dir = dir;
            }
            Msg::Tick => self.now = unix_now(),
            Msg::Ntp(on) => {
                let task = match &self.svc {
                    Some(Svc::Live(t)) => t.set_ntp(on),
                    Some(Svc::Sample(c)) => c.update(move |s| {
                        s.ntp = on;
                        s.ntp_synchronized = on;
                    }),
                    None => return,
                };
                cx.wait(task, |r| G::Time(Msg::Done("Network time", r.map_err(|e| e.to_string()))));
            }
            Msg::Query(q) => self.query = q,
            Msg::Pick(id) => self.set_zone(id, cx),
            Msg::MapPick(lat, lon) => {
                if self.snapshot().timezone_managed {
                    return;
                }
                let Some(zones) = &self.zones else { return };
                if let Some(z) = aizome_sys::datetime::nearest(zones, lat, lon) {
                    let id = z.id.clone();
                    self.set_zone(id, cx);
                }
            }
            Msg::Clock24(on) => cx.change(|s| {
                let mut m = s.menubar_settings();
                m.clock = if on { ClockFormat::H24 } else { ClockFormat::H12 };
                s.set_shell_section("menubar", &m);
            }),
            Msg::ShowDate(on) => cx.change(|s| {
                let mut m = s.menubar_settings();
                m.clock_date = on;
                s.set_shell_section("menubar", &m);
            }),
            Msg::Seconds(on) => cx.change(|s| {
                let mut m = s.menubar_settings();
                m.clock_seconds = on;
                s.set_shell_section("menubar", &m);
            }),
            Msg::Done(_, Ok(())) => {}
            Msg::Done(what, Err(e)) => cx.failed(what, e),
        }
    }

    fn matches(&self) -> Vec<&Zone> {
        let Some(zones) = &self.zones else { return Vec::new() };
        let words: Vec<String> = self.query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        zones
            .iter()
            .filter(|z| {
                let hay = format!("{} {} {} {}", z.city, z.id.replace('_', " "), z.country_names.join(" "), z.comment)
                    .to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .take(8)
            .collect()
    }

    pub fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let s = self.snapshot();
        let menubar = vcx.settings().menubar_settings();
        let h24 = menubar.clock == ClockFormat::H24;
        let zone = self.zones.as_ref().and_then(|zs| zs.iter().find(|z| z.id == s.timezone)).cloned();
        let offset = self.offset_of(&s.timezone).unwrap_or(0);
        let (date, h, m, sec) = broken(self.now + i64::from(offset));
        let place = match &zone {
            Some(z) => {
                let country = z.country_names.first().cloned().unwrap_or_default();
                if country.is_empty() { z.city.clone() } else { format!("{}, {country}", z.city) }
            }
            None => s.timezone.clone(),
        };
        let clock = column![
            text(clock_text(h, m, sec, h24, menubar.clock_seconds)).mono().size(34.0).weight(600.0).tabular(),
            text(date).role(Role::Text2),
            text(format!("{place} · {}", tz::describe(offset))).size(12.0).role(Role::Text3),
        ]
        .spacing(4.0);

        let auto = if self.ntp_managed && s.available {
            form::managed(
                "Set the time automatically",
                if s.ntp { "On" } else { "Off" },
                "Change services.timesyncd.enable and rebuild.",
            )
        } else if s.can_ntp || !s.available {
            form::row("Set the time automatically", toggle(s.ntp).on_toggle(Msg::Ntp).enabled(s.available)).help(
                if !s.available {
                    "systemd-timedated isn't answering.".to_owned()
                } else if s.ntp && s.ntp_synchronized {
                    "From time servers on the network. In step now.".to_owned()
                } else if s.ntp {
                    "From time servers on the network. Not in step yet.".to_owned()
                } else {
                    "The computer keeps time by itself, and drifts.".to_owned()
                },
            )
        } else {
            form::managed(
                "Set the time automatically",
                "Off",
                "No network time service is installed (services.timesyncd).",
            )
        }
        .entry("auto-time");

        let current = self
            .zones
            .as_ref()
            .and_then(|zs| zs.iter().find(|z| z.id == s.timezone))
            .map(|z| (z.latitude, z.longitude));
        let points: Vec<(f32, f32)> = self
            .zones
            .as_ref()
            .map(|zs| zs.iter().filter(|z| z.id != "UTC").map(|z| (z.latitude as f32, z.longitude as f32)).collect())
            .unwrap_or_default();
        let map_el = zone_map(points, current, !s.timezone_managed, |(lat, lon)| Msg::MapPick(lat, lon));

        let mut zone_rows = vec![
            if s.timezone_managed {
                form::managed(
                    "Time zone",
                    s.timezone.clone(),
                    "To choose it here, set time.timeZone = null in the system configuration.",
                )
            } else {
                form::value("Time zone", s.timezone.clone()).detail(place.clone())
            }
            .entry("timezone"),
        ];
        zone_rows.push(form::whole(container(map_el).padding([10.0, form::INSET]).width(Fill)));
        if !s.timezone_managed {
            zone_rows.push(form::row(
                "Find a city",
                search_field(&self.query).placeholder("City, country or zone").on_change(Msg::Query).width(240.0),
            ));
            for z in self.matches() {
                let off = self.offset_of(&z.id).map(tz::describe).unwrap_or_default();
                let country = z.country_names.join(", ");
                let line = row![
                    column![
                        text(z.city.clone()),
                        text(if country.is_empty() { z.id.clone() } else { country }).size(11.0).role(Role::Text3)
                    ]
                    .spacing(2.0)
                    .width(Fill),
                    text(off).mono().size(11.0).role(Role::Text2),
                    if z.id == s.timezone {
                        icon(Icon::Check).size(14.0).role(Role::Focus).into()
                    } else {
                        space::<Msg>(14.0)
                    },
                ]
                .spacing(10.0)
                .align(Align::Center);
                zone_rows.push(form::whole(
                    press(container(line).padding([6.0, form::INSET]).width(Fill))
                        .style(PressStyle::Plain)
                        .on_press(Msg::Pick(z.id.clone()))
                        .named(z.city.clone())
                        .width(Fill),
                ));
            }
        }

        form::page(vec![
            vcx.section("", vec![form::whole(container(clock).padding([16.0, form::INSET]).width(Fill)), auto]),
            vcx.section("Time Zone", zone_rows),
            vcx.section(
                "Menu Bar Clock",
                vec![
                    form::row("24-hour time", toggle(h24).on_toggle(Msg::Clock24)).entry("clock-24h").detail(if h24 {
                        "21:42"
                    } else {
                        "9:42 PM"
                    }),
                    form::row("Show the date", toggle(menubar.clock_date).on_toggle(Msg::ShowDate))
                        .entry("clock-date")
                        .detail("Sat 17"),
                    form::row("Show seconds", toggle(menubar.clock_seconds).on_toggle(Msg::Seconds))
                        .entry("clock-seconds"),
                ],
            ),
        ])
    }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

// The map.

/// Every zone's city as a dot on a map of the world (equirectangular,
/// 60°S to 80°N), the current one marked; a click picks the nearest.
fn zone_map<Msg: Send + 'static>(
    points: Vec<(f32, f32)>,
    current: Option<(f64, f64)>,
    live: bool,
    f: impl Fn((f64, f64)) -> Msg + 'static,
) -> Element<Msg> {
    let on_pick = live.then(|| MsgMap::new(f));
    let el = Element::new(ZoneMap { points, current, on_pick }, Vec::new());
    el.height(168.0).width(Fill)
}

struct ZoneMap {
    points: Vec<(f32, f32)>,
    current: Option<(f64, f64)>,
    on_pick: Option<MsgMap<(f64, f64)>>,
}

const NORTH: f64 = 80.0;
const SOUTH: f64 = -60.0;

impl ZoneMap {
    fn at(&self, size: Size, lat: f64, lon: f64) -> Point {
        let x = (lon + 180.0) / 360.0 * size.width;
        let y = (NORTH - lat.clamp(SOUTH, NORTH)) / (NORTH - SOUTH) * size.height;
        Point::new(x, y)
    }
}

impl Widget for ZoneMap {
    fn name(&self) -> &'static str {
        "zone-map"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width.min(4000.0), 168.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        draw::sunken(cx.canvas, t, r, t.sunk(), t.line());
        let s = r.size();
        // Meridians and parallels every 30°.
        for lon in (-150..=150).step_by(30) {
            let p = self.at(s, 0.0, f64::from(lon));
            cx.canvas
                .fill_rect(Rect::new(p.x.round(), 1.0, p.x.round() + 1.0, s.height - 1.0), t.line().with_alpha(0.6));
        }
        for lat in [-30, 0, 30, 60] {
            let p = self.at(s, f64::from(lat), 0.0);
            let c = if lat == 0 { t.line2() } else { t.line().with_alpha(0.6) };
            cx.canvas.fill_rect(Rect::new(1.0, p.y.round(), s.width - 1.0, p.y.round() + 1.0), c);
        }
        let dot = t.text3().with_alpha(0.75);
        for (lat, lon) in &self.points {
            let p = self.at(s, f64::from(*lat), f64::from(*lon));
            cx.canvas
                .fill_rect(Rect::new(p.x.round() - 1.0, p.y.round() - 1.0, p.x.round() + 1.0, p.y.round() + 1.0), dot);
        }
        if let Some((lat, lon)) = self.current {
            let p = self.at(s, lat, lon);
            let (x, y) = (p.x.round(), p.y.round());
            let f = t.focus();
            cx.canvas.fill_rect(Rect::new(x, 1.0, x + 1.0, s.height - 1.0), f.with_alpha(0.45));
            cx.canvas.fill_rect(Rect::new(1.0, y, s.width - 1.0, y + 1.0), f.with_alpha(0.45));
            let mark = Rect::new(x - 4.0, y - 4.0, x + 5.0, y + 5.0);
            cx.canvas.fill_rect(mark, f);
            draw::border(cx.canvas, mark, t.bevel_lo(), 1.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.points.len()).add(&self.on_pick.is_some());
        if let Some((a, b)) = self.current {
            h.f64(a).f64(b);
        }
        if let (Some(f), Some(l)) = (self.points.first(), self.points.last()) {
            h.f64(f64::from(f.0)).f64(f64::from(l.1));
        }
    }

    fn hit(&self) -> bool {
        self.on_pick.is_some()
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let Some(pick) = &self.on_pick else { return Status::Ignored };
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                let s = cx.size();
                if s.width > 0.0 && s.height > 0.0 && cx.bounds().contains(*pos) {
                    let lon = pos.x / s.width * 360.0 - 180.0;
                    let lat = NORTH - pos.y / s.height * (NORTH - SOUTH);
                    let m = pick.clone();
                    cx.emit(&MsgFn::from_fn(move || m.make((lat, lon))));
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, label: Some("Time zone map".into()), ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::units_managed;

    #[test]
    fn units_in_the_store_are_the_configurations() {
        let dir = std::env::temp_dir().join(format!("settei-units-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A unit directory of one's own is changeable.
        assert!(!units_managed(&dir));
        // One that's a link into the store is NixOS's (the store exists
        // where these tests run under Nix; elsewhere there's nothing to link).
        let store = std::path::Path::new("/nix/store");
        if store.is_dir() {
            let link = dir.join("system");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(store, &link).unwrap();
            assert!(units_managed(&link));
        }
        assert!(!units_managed(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
