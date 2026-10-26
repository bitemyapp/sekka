//! Batteries and power supplies, through UPower: the laptop's batteries
//! (percent, state, time to empty or full, energy rate, health), line power,
//! peripherals' batteries (mice, keyboards, headsets), the lid, and keyboard
//! backlights.

use crate::bus::{self, Bus, Event, Objects, Opts, Props, PropsExt, signals_from};
use crate::error::Error;
use crate::service::{Model, Stop, Task, Wake, service_handle};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use zbus::zvariant::OwnedObjectPath;

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const IFACE_DEVICE: &str = "org.freedesktop.UPower.Device";
const IFACE_KBD: &str = "org.freedesktop.UPower.KbdBacklight";
/// UPower before 1.90.3 had one keyboard backlight, here.
const LEGACY_KBD: &str = "/org/freedesktop/UPower/KbdBacklight";
/// Where we keep a keyboard backlight's levels in the cache.
const KBD_LEVELS: &str = "aizome.KbdBacklightLevels";

/// What UPower reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpowerState {
    /// UPower is running.
    pub available: bool,
    /// Running on battery.
    pub on_battery: bool,
    pub lid_present: bool,
    pub lid_closed: bool,
    /// The combined battery the menu bar shows (UPower's display device),
    /// when there is a battery.
    pub display: Option<PowerDevice>,
    /// Every power device: the system's batteries first, then line power,
    /// then peripherals.
    pub devices: Vec<PowerDevice>,
    pub keyboard_backlights: Vec<KeyboardBacklight>,
}

impl UpowerState {
    /// Batteries that power the computer.
    pub fn batteries(&self) -> impl Iterator<Item = &PowerDevice> {
        self.devices.iter().filter(|d| d.kind == PowerKind::Battery && d.power_supply)
    }

    /// Batteries of peripherals (mouse, keyboard, headset…).
    pub fn peripherals(&self) -> impl Iterator<Item = &PowerDevice> {
        self.devices.iter().filter(|d| !d.power_supply && d.kind != PowerKind::LinePower && d.is_present)
    }

    /// Whether any line power supply is connected.
    pub fn plugged_in(&self) -> bool {
        self.devices.iter().any(|d| d.kind == PowerKind::LinePower && d.online)
    }
}

/// A battery or power supply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerDevice {
    pub path: String,
    pub native_path: String,
    pub kind: PowerKind,
    pub vendor: String,
    pub model: String,
    /// It powers the computer (rather than a peripheral).
    pub power_supply: bool,
    pub is_present: bool,
    /// Line power: connected.
    pub online: bool,
    /// 0–100.
    pub percentage: f64,
    pub state: BatteryState,
    /// Seconds until empty while discharging, if UPower can tell.
    pub time_to_empty: Option<Duration>,
    /// Seconds until full while charging.
    pub time_to_full: Option<Duration>,
    /// Watts flowing in or out.
    pub energy_rate: f64,
    /// Watt-hours now, when full, and when new.
    pub energy: f64,
    pub energy_full: f64,
    pub energy_full_design: f64,
    /// Health: full capacity as a percentage of the design capacity.
    pub capacity: f64,
    pub charge_cycles: Option<u32>,
    pub temperature: Option<f64>,
    pub warning: WarningLevel,
    /// UPower's icon name (`battery-good-charging-symbolic`).
    pub icon_name: String,
    /// Charging stops at the end threshold (laptops that support it).
    pub charge_threshold_enabled: bool,
    pub charge_threshold_supported: bool,
    pub charge_start_threshold: Option<u32>,
    pub charge_end_threshold: Option<u32>,
}

/// What a power device is (`UpDeviceKind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PowerKind {
    LinePower,
    Battery,
    Ups,
    Monitor,
    Mouse,
    Keyboard,
    Phone,
    Tablet,
    Gamepad,
    Pen,
    Touchpad,
    Headset,
    Speakers,
    Headphones,
    Other,
    #[default]
    Unknown,
}

impl PowerKind {
    pub fn from_u32(n: u32) -> PowerKind {
        match n {
            1 => PowerKind::LinePower,
            2 => PowerKind::Battery,
            3 => PowerKind::Ups,
            4 => PowerKind::Monitor,
            5 => PowerKind::Mouse,
            6 => PowerKind::Keyboard,
            8 => PowerKind::Phone,
            10 => PowerKind::Tablet,
            12 => PowerKind::Gamepad,
            13 => PowerKind::Pen,
            14 => PowerKind::Touchpad,
            17 => PowerKind::Headset,
            18 => PowerKind::Speakers,
            19 => PowerKind::Headphones,
            0 => PowerKind::Unknown,
            _ => PowerKind::Other,
        }
    }
}

/// A battery's state (`UpDeviceState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BatteryState {
    #[default]
    Unknown,
    Charging,
    Discharging,
    Empty,
    FullyCharged,
    /// Plugged in but not charging (held at a threshold, or waiting).
    PendingCharge,
    PendingDischarge,
}

impl BatteryState {
    pub fn from_u32(n: u32) -> BatteryState {
        match n {
            1 => BatteryState::Charging,
            2 => BatteryState::Discharging,
            3 => BatteryState::Empty,
            4 => BatteryState::FullyCharged,
            5 => BatteryState::PendingCharge,
            6 => BatteryState::PendingDischarge,
            _ => BatteryState::Unknown,
        }
    }
}

/// How urgent a battery's level is (`UpDeviceLevel` as warning level).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WarningLevel {
    #[default]
    None,
    /// UPS on battery.
    Discharging,
    Low,
    Critical,
    /// The critical action (hibernate, power off) is about to happen.
    Action,
}

impl WarningLevel {
    pub fn from_u32(n: u32) -> WarningLevel {
        match n {
            2 => WarningLevel::Discharging,
            3 => WarningLevel::Low,
            4 => WarningLevel::Critical,
            5 => WarningLevel::Action,
            _ => WarningLevel::None,
        }
    }
}

/// A point of a device's history ([`Upower::history`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HistoryPoint {
    /// Unix time, in seconds.
    pub time: u32,
    /// Percent for `"charge"`, watts for `"rate"`.
    pub value: f64,
    pub state: BatteryState,
}

/// A keyboard backlight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyboardBacklight {
    pub path: String,
    pub brightness: i32,
    pub max_brightness: i32,
}

fn seconds(n: i64) -> Option<Duration> {
    u64::try_from(n).ok().filter(|&s| s > 0).map(Duration::from_secs)
}

pub(crate) fn power_device(path: &str, p: &Props) -> PowerDevice {
    PowerDevice {
        path: path.to_owned(),
        native_path: p.str("NativePath"),
        kind: PowerKind::from_u32(p.u32("Type")),
        vendor: p.str("Vendor"),
        model: p.str("Model"),
        power_supply: p.bool("PowerSupply"),
        is_present: p.bool("IsPresent"),
        online: p.bool("Online"),
        percentage: p.f64("Percentage").clamp(0.0, 100.0),
        state: BatteryState::from_u32(p.u32("State")),
        time_to_empty: seconds(p.i64("TimeToEmpty")),
        time_to_full: seconds(p.i64("TimeToFull")),
        energy_rate: p.f64("EnergyRate"),
        energy: p.f64("Energy"),
        energy_full: p.f64("EnergyFull"),
        energy_full_design: p.f64("EnergyFullDesign"),
        capacity: p.f64("Capacity"),
        charge_cycles: p.opt_i64("ChargeCycles").and_then(|n| u32::try_from(n).ok()).filter(|&n| n > 0),
        temperature: Some(p.f64("Temperature")).filter(|&t| t > 0.0),
        warning: WarningLevel::from_u32(p.u32("WarningLevel")),
        icon_name: p.str("IconName"),
        charge_threshold_enabled: p.bool("ChargeThresholdEnabled"),
        charge_threshold_supported: p.bool("ChargeThresholdSupported"),
        charge_start_threshold: p
            .opt_i64("ChargeStartThreshold")
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n > 0 && n < 100),
        charge_end_threshold: p
            .opt_i64("ChargeEndThreshold")
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n > 0 && n <= 100),
    }
}

pub(crate) fn build(objects: &Objects, display: Option<&str>) -> UpowerState {
    let empty = Props::new();
    let manager = objects.get(UPOWER_PATH, UPOWER).unwrap_or(&empty);
    let mut devices: Vec<PowerDevice> = objects
        .with(IFACE_DEVICE)
        .filter(|(path, _)| Some(*path) != display)
        .map(|(path, p)| power_device(path, p))
        .collect();
    let rank = |d: &PowerDevice| match (d.kind, d.power_supply) {
        (PowerKind::Battery, true) => 0,
        (PowerKind::LinePower, _) => 1,
        _ => 2,
    };
    devices.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.native_path.cmp(&b.native_path)));
    let display = display
        .and_then(|path| Some(power_device(path, objects.get(path, IFACE_DEVICE)?)))
        .filter(|d| d.is_present && d.kind != PowerKind::Unknown);
    let keyboard_backlights = objects
        .with(KBD_LEVELS)
        .map(|(path, p)| KeyboardBacklight {
            path: path.to_owned(),
            brightness: i32::try_from(p.i64("Brightness")).unwrap_or(0),
            max_brightness: i32::try_from(p.i64("MaxBrightness")).unwrap_or(0),
        })
        .filter(|k| k.max_brightness > 0)
        .collect();
    UpowerState {
        available: true,
        on_battery: manager.bool("OnBattery"),
        lid_present: manager.bool("LidIsPresent"),
        lid_closed: manager.bool("LidIsClosed"),
        display,
        devices,
        keyboard_backlights,
    }
}

/// UPower: batteries, power supplies, the lid, keyboard backlights.
pub struct Upower {
    bus: Option<Bus>,
    model: Arc<Model<UpowerState>>,
    stop: Stop,
}

service_handle!(Upower, UpowerState);

impl Upower {
    /// Follow UPower on the system bus (starting it if it is activatable).
    pub fn new(wake: Wake) -> Upower {
        match Bus::system() {
            Ok(bus) => Upower::with_bus(bus, wake),
            Err(_) => {
                Upower { bus: None, model: Arc::new(Model::new(UpowerState::default(), wake)), stop: Stop::default() }
            }
        }
    }

    pub fn with_bus(bus: Bus, wake: Wake) -> Upower {
        let model = Arc::new(Model::new(UpowerState::default(), wake));
        let stop = Stop::default();
        let mut objects = Objects::default();
        let mut display: Option<String> = None;
        let mut present = false;
        let mut dirty = false;
        let follower = model.clone();
        bus::follow(
            bus.clone(),
            UPOWER,
            vec![signals_from(UPOWER)],
            true,
            stop.clone(),
            move |bus, event| match event {
                Event::Appeared => {
                    present = true;
                    (objects, display) = load(bus);
                    dirty = true;
                }
                Event::Vanished => {
                    present = false;
                    objects = Objects::default();
                    follower.set_if_changed(UpowerState::default());
                }
                Event::Signal(msg) => {
                    let (iface, member) = bus::member(&msg);
                    let path = bus::path(&msg);
                    match (iface.as_str(), member.as_str()) {
                        (UPOWER, "DeviceAdded") => {
                            if let Ok(device) = msg.body().deserialize::<OwnedObjectPath>()
                                && let Ok(props) = bus.get_all(UPOWER, device.as_str(), IFACE_DEVICE)
                            {
                                objects.map.entry(device.to_string()).or_default().insert(IFACE_DEVICE.into(), props);
                            }
                            dirty = true;
                        }
                        (UPOWER, "DeviceRemoved") => {
                            if let Ok(device) = msg.body().deserialize::<OwnedObjectPath>() {
                                objects.map.remove(device.as_str());
                            }
                            dirty = true;
                        }
                        (IFACE_KBD, "BrightnessChanged" | "BrightnessChangedWithSource") => {
                            if let Some(levels) = objects.map.get_mut(&path).and_then(|o| o.get_mut(KBD_LEVELS))
                                && let Ok(value) = msg
                                    .body()
                                    .deserialize::<(i32,)>()
                                    .map(|(v,)| v)
                                    .or_else(|_| msg.body().deserialize::<(i32, String)>().map(|(v, _)| v))
                            {
                                levels.insert("Brightness".into(), zbus::zvariant::OwnedValue::from(value));
                                dirty = true;
                            }
                        }
                        _ => {
                            let applied = objects.apply(&msg);
                            if !applied.invalidated.is_empty() {
                                objects.refetch(bus, UPOWER, applied.invalidated);
                            }
                            dirty |= applied.changed;
                        }
                    }
                }
                Event::Idle => {
                    if present && dirty {
                        follower.set_if_changed(build(&objects, display.as_deref()));
                        dirty = false;
                    }
                }
            },
        );
        Upower { bus: Some(bus), model, stop }
    }

    /// Whether UPower is running.
    pub fn available(&self) -> bool {
        self.model.get().available
    }

    /// A device's recorded history of `kind` (`"charge"`, in percent, or
    /// `"rate"`, in watts) over the last `timespan` seconds, at most
    /// `resolution` points, oldest first. UPower keeps it for batteries
    /// (`GetHistory`); a query, so it never asks for authorization.
    pub fn history(&self, path: &str, kind: &str, timespan: u32, resolution: u32) -> Task<Vec<HistoryPoint>> {
        let (path, kind) = (path.to_owned(), kind.to_owned());
        bus::spawn(&self.bus, "UPower", self.model.wake(), move |bus| {
            let points: Vec<(u32, f64, u32)> =
                bus.call(UPOWER, &path, IFACE_DEVICE, "GetHistory", &(kind, timespan, resolution))?;
            Ok(history_points(points))
        })
    }

    /// Set a keyboard backlight's level (0–its maximum).
    pub fn set_keyboard_brightness(&self, path: &str, level: i32) -> Task<()> {
        let path = path.to_owned();
        let max = self.snapshot().keyboard_backlights.iter().find(|k| k.path == path).map(|k| k.max_brightness);
        bus::spawn(&self.bus, "UPower", self.model.wake(), move |bus| {
            let max = max.ok_or_else(|| Error::NotFound(path.clone()))?;
            bus.call_with(Opts::ACTION, UPOWER, &path, IFACE_KBD, "SetBrightness", &(level.clamp(0, max),))
        })
    }

    /// Step the first keyboard backlight up or down by one level, wrapping
    /// from the brightest to off (as the keyboard's own key does).
    pub fn cycle_keyboard_brightness(&self) -> Task<i32> {
        let Some(kbd) = self.snapshot().keyboard_backlights.first().cloned() else {
            return Task::ready(Err(Error::NotFound("no keyboard backlight".into())));
        };
        let next = if kbd.brightness >= kbd.max_brightness { 0 } else { kbd.brightness + 1 };
        bus::spawn(&self.bus, "UPower", self.model.wake(), move |bus| {
            bus.call_with::<_, ()>(Opts::ACTION, UPOWER, &kbd.path, IFACE_KBD, "SetBrightness", &(next,))?;
            Ok(next)
        })
    }
}

impl Drop for Upower {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// `GetHistory`'s answer as readings, oldest first. UPower records a
/// point only when the value changes, and marks where its record breaks off
/// with a point of value 0 and state unknown, at the second of the last
/// reading. That isn't a reading: drawn as one, the charge dives to 0% and
/// climbs back across the gap.
fn history_points(mut raw: Vec<(u32, f64, u32)>) -> Vec<HistoryPoint> {
    raw.sort_by_key(|p| p.0);
    raw.into_iter()
        .map(|(time, value, state)| HistoryPoint { time, value, state: BatteryState::from_u32(state) })
        .filter(|p| !(p.state == BatteryState::Unknown && p.value == 0.0))
        .collect()
}

/// Everything UPower knows, read at once.
fn load(bus: &Bus) -> (Objects, Option<String>) {
    let mut objects = Objects::default();
    let mut put = |path: &str, iface: &str, props: Props| {
        objects.map.entry(path.to_owned()).or_default().insert(iface.to_owned(), props);
    };
    if let Ok(props) = bus.get_all(UPOWER, UPOWER_PATH, UPOWER) {
        put(UPOWER_PATH, UPOWER, props);
    }
    let display = bus
        .call::<_, OwnedObjectPath>(UPOWER, UPOWER_PATH, UPOWER, "GetDisplayDevice", &())
        .ok()
        .map(|p| p.to_string());
    let mut devices: Vec<String> = bus
        .call::<_, Vec<OwnedObjectPath>>(UPOWER, UPOWER_PATH, UPOWER, "EnumerateDevices", &())
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.to_string())
        .collect();
    devices.extend(display.clone());
    for device in devices {
        if let Ok(props) = bus.get_all(UPOWER, &device, IFACE_DEVICE) {
            put(&device, IFACE_DEVICE, props);
        }
    }
    let keyboards: Vec<String> =
        match bus.call::<_, Vec<OwnedObjectPath>>(UPOWER, UPOWER_PATH, UPOWER, "EnumerateKbdBacklights", &()) {
            Ok(paths) => paths.into_iter().map(|p| p.to_string()).collect(),
            Err(_) => vec![LEGACY_KBD.to_owned()],
        };
    for keyboard in keyboards {
        let max = bus.call::<_, i32>(UPOWER, &keyboard, IFACE_KBD, "GetMaxBrightness", &());
        let now = bus.call::<_, i32>(UPOWER, &keyboard, IFACE_KBD, "GetBrightness", &());
        if let (Ok(max), Ok(now)) = (max, now) {
            put(
                &keyboard,
                KBD_LEVELS,
                HashMap::from([
                    ("Brightness".to_owned(), zbus::zvariant::OwnedValue::from(now)),
                    ("MaxBrightness".to_owned(), zbus::zvariant::OwnedValue::from(max)),
                ]),
            );
        }
    }
    (objects, display)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::owned;

    #[test]
    fn batteries_and_peripherals() {
        let mut objects = Objects::default();
        let mut put = |path: &str, iface: &str, entries: Vec<(&str, zbus::zvariant::OwnedValue)>| {
            objects
                .map
                .entry(path.to_owned())
                .or_default()
                .insert(iface.to_owned(), entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        };
        put(UPOWER_PATH, UPOWER, vec![("OnBattery", owned(true)), ("LidIsPresent", owned(true))]);
        put(
            "/d/bat0",
            IFACE_DEVICE,
            vec![
                ("Type", owned(2u32)),
                ("PowerSupply", owned(true)),
                ("IsPresent", owned(true)),
                ("Percentage", owned(57.0f64)),
                ("State", owned(2u32)),
                ("TimeToEmpty", owned(7200i64)),
                ("TimeToFull", owned(0i64)),
                ("EnergyRate", owned(6.5f64)),
                ("Capacity", owned(91.2f64)),
                ("ChargeCycles", owned(-1i32)),
                ("NativePath", owned("BAT0")),
            ],
        );
        put("/d/ac", IFACE_DEVICE, vec![("Type", owned(1u32)), ("Online", owned(false)), ("PowerSupply", owned(true))]);
        put(
            "/d/mouse",
            IFACE_DEVICE,
            vec![
                ("Type", owned(5u32)),
                ("IsPresent", owned(true)),
                ("Percentage", owned(20.0f64)),
                ("Model", owned("MX Master")),
            ],
        );
        put(
            "/d/display",
            IFACE_DEVICE,
            vec![("Type", owned(2u32)), ("IsPresent", owned(true)), ("Percentage", owned(57.0f64))],
        );
        put("/k", KBD_LEVELS, vec![("Brightness", owned(1i32)), ("MaxBrightness", owned(2i32))]);
        let state = build(&objects, Some("/d/display"));
        assert!(state.on_battery && state.lid_present && !state.plugged_in());
        assert_eq!(state.display.as_ref().unwrap().percentage, 57.0);
        let kinds: Vec<PowerKind> = state.devices.iter().map(|d| d.kind).collect();
        assert_eq!(kinds, [PowerKind::Battery, PowerKind::LinePower, PowerKind::Mouse]);
        let battery = state.batteries().next().unwrap();
        assert_eq!(battery.state, BatteryState::Discharging);
        assert_eq!(battery.time_to_empty, Some(Duration::from_secs(7200)));
        assert_eq!(battery.time_to_full, None);
        assert_eq!(battery.charge_cycles, None, "-1 means unknown");
        assert_eq!(state.peripherals().map(|d| d.model.as_str()).collect::<Vec<_>>(), ["MX Master"]);
        assert_eq!(state.keyboard_backlights[0].max_brightness, 2);
    }

    #[test]
    fn history_leaves_out_upowers_gap_markers() {
        // As UPower answers, newest first: discharging this morning, and
        // last night's reading at 80% with the marker it left when the
        // machine shut down, at the same second.
        let raw = vec![(1_000_900, 78.0, 2), (1_000_600, 79.0, 2), (1_000_000, 80.0, 5), (1_000_000, 0.0, 0)];
        let points = history_points(raw);
        let values: Vec<(u32, f64)> = points.iter().map(|p| (p.time, p.value)).collect();
        assert_eq!(values, [(1_000_000, 80.0), (1_000_600, 79.0), (1_000_900, 78.0)]);
        assert_eq!(points[0].state, BatteryState::PendingCharge);
        assert_eq!(points[2].state, BatteryState::Discharging);
    }
}
