//! Bluetooth, through BlueZ's D-Bus API.
//!
//! [`Bluetooth`] follows BlueZ's `ObjectManager`: adapters (power,
//! discoverable, name, scanning) and devices (name, kind, paired, trusted,
//! connected, battery from `Battery1`, signal strength), kept current from
//! BlueZ's signals.
//!
//! Actions pair, connect, disconnect and remove devices, and switch
//! adapters. Pairing needs an agent to show or ask for PINs and passkeys:
//! [`Bluetooth::register_agent`] serves `org.bluez.Agent1` and hands each
//! request to a callback as a [`PairingPrompt`], which the UI answers when
//! the person does (see [`agent`]).
//!
//! Needs BlueZ on the system bus (`hardware.bluetooth.enable`). With no
//! BlueZ, [`Bluetooth::available`] is false and the snapshot is empty.

pub mod agent;

pub use agent::{PairingPrompt, PairingRequest, PairingResponse, Responder};

use crate::bus::{self, Bus, Event, Objects, Opts, Props, PropsExt, signals_from};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, service_handle};
use std::sync::Arc;
use std::time::Duration;
use zbus::zvariant::ObjectPath;

pub(crate) const BLUEZ: &str = "org.bluez";
const IFACE_ADAPTER: &str = "org.bluez.Adapter1";
const IFACE_DEVICE: &str = "org.bluez.Device1";
const IFACE_BATTERY: &str = "org.bluez.Battery1";

/// How long pairing may take: the person has to read and confirm a code.
const PAIR_TIMEOUT: Duration = Duration::from_secs(90);
/// Connecting to a device that is off takes BlueZ a while to give up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// What BlueZ reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BluetoothState {
    /// BlueZ is running.
    pub available: bool,
    pub adapters: Vec<Adapter>,
    /// Known and discovered devices: connected first, then paired, then by
    /// name.
    pub devices: Vec<Device>,
}

impl BluetoothState {
    /// The adapter to show: the first powered one, else the first.
    pub fn default_adapter(&self) -> Option<&Adapter> {
        self.adapters.iter().find(|a| a.powered).or_else(|| self.adapters.first())
    }

    pub fn adapter(&self, path: &str) -> Option<&Adapter> {
        self.adapters.iter().find(|a| a.path == path)
    }

    pub fn device(&self, path: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.path == path)
    }

    pub fn device_by_address(&self, address: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.address.eq_ignore_ascii_case(address))
    }

    /// Paired devices.
    pub fn paired(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| d.paired)
    }

    /// Discovered devices that aren't paired, named ones only.
    pub fn nearby(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| !d.paired && d.has_name)
    }

    /// Any adapter is powered.
    pub fn powered(&self) -> bool {
        self.adapters.iter().any(|a| a.powered)
    }

    /// Any device is connected.
    pub fn connected(&self) -> bool {
        self.devices.iter().any(|d| d.connected)
    }
}

/// A Bluetooth adapter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Adapter {
    pub path: String,
    pub address: String,
    /// The system name.
    pub name: String,
    /// The name other devices see (settable).
    pub alias: String,
    pub powered: bool,
    /// BlueZ's `PowerState`: `on`, `off`, `off-enabling`, `on-disabling`,
    /// `off-blocked` (rfkill).
    pub power_state: String,
    pub discoverable: bool,
    /// Seconds; 0 means forever.
    pub discoverable_timeout: u32,
    pub pairable: bool,
    /// Scanning for devices.
    pub discovering: bool,
}

impl Adapter {
    /// A radio switch (rfkill) keeps it off.
    pub fn blocked(&self) -> bool {
        self.power_state == "off-blocked"
    }
}

/// A Bluetooth device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Device {
    pub path: String,
    pub adapter: String,
    pub address: String,
    /// The name to show: the alias (which defaults to the device's own
    /// name, else its address).
    pub name: String,
    /// The device sent a name (otherwise `name` is its address).
    pub has_name: bool,
    /// BlueZ's icon name (`audio-headset`, `input-mouse`…).
    pub icon: String,
    pub kind: DeviceKind,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub connected: bool,
    /// Battery level, 0–100, for devices that report it.
    pub battery: Option<u8>,
    /// Signal strength in dBm, while discovering.
    pub rssi: Option<i16>,
    pub services_resolved: bool,
    pub legacy_pairing: bool,
    pub uuids: Vec<String>,
}

/// What kind of device it is, from BlueZ's icon or the class of device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    Headset,
    Headphones,
    Speaker,
    Microphone,
    Keyboard,
    Mouse,
    Gamepad,
    Tablet,
    Phone,
    Computer,
    Watch,
    Printer,
    Camera,
    Tv,
    Network,
    #[default]
    Other,
}

impl DeviceKind {
    /// From BlueZ's `Icon` property, else the `Class` of device.
    pub fn of(icon: &str, class: u32, appearance: u32) -> DeviceKind {
        match icon {
            "audio-headset" => return DeviceKind::Headset,
            "audio-headphones" => return DeviceKind::Headphones,
            "audio-card" | "audio-speakers" => return DeviceKind::Speaker,
            "audio-input-microphone" => return DeviceKind::Microphone,
            "input-keyboard" => return DeviceKind::Keyboard,
            "input-mouse" => return DeviceKind::Mouse,
            "input-gaming" => return DeviceKind::Gamepad,
            "input-tablet" => return DeviceKind::Tablet,
            "phone" => return DeviceKind::Phone,
            "computer" => return DeviceKind::Computer,
            "printer" => return DeviceKind::Printer,
            "camera-photo" | "camera-video" => return DeviceKind::Camera,
            "video-display" => return DeviceKind::Tv,
            "network-wireless" | "modem" => return DeviceKind::Network,
            _ => {}
        }
        // Low Energy appearance categories (bits 6–15).
        match appearance >> 6 {
            0x003 => return DeviceKind::Watch,
            0x00F => {
                return match appearance & 0x3F {
                    1 => DeviceKind::Keyboard,
                    2 => DeviceKind::Mouse,
                    3 | 4 => DeviceKind::Gamepad,
                    5 => DeviceKind::Tablet,
                    _ => DeviceKind::Other,
                };
            }
            0x025 => return DeviceKind::Headphones,
            _ => {}
        }
        // Classic class of device: major class in bits 8–12.
        match (class >> 8) & 0x1F {
            1 => DeviceKind::Computer,
            2 => DeviceKind::Phone,
            3 => DeviceKind::Network,
            4 => match (class >> 2) & 0x3F {
                1 | 2 => DeviceKind::Headset,
                4 => DeviceKind::Microphone,
                6 => DeviceKind::Headphones,
                5 | 7 | 8 | 10 => DeviceKind::Speaker,
                12 | 15 => DeviceKind::Tv,
                _ => DeviceKind::Speaker,
            },
            5 => match (class >> 6) & 0x3 {
                1 => DeviceKind::Keyboard,
                2 => DeviceKind::Mouse,
                _ => match (class >> 2) & 0xF {
                    1 | 2 => DeviceKind::Gamepad,
                    5 => DeviceKind::Tablet,
                    _ => DeviceKind::Other,
                },
            },
            6 => DeviceKind::Printer,
            7 => DeviceKind::Watch,
            _ => DeviceKind::Other,
        }
    }

    /// A symbolic icon name for the kind.
    pub fn icon(self) -> &'static str {
        match self {
            DeviceKind::Headset => "audio-headset-symbolic",
            DeviceKind::Headphones => "audio-headphones-symbolic",
            DeviceKind::Speaker => "audio-speakers-symbolic",
            DeviceKind::Microphone => "audio-input-microphone-symbolic",
            DeviceKind::Keyboard => "input-keyboard-symbolic",
            DeviceKind::Mouse => "input-mouse-symbolic",
            DeviceKind::Gamepad => "input-gaming-symbolic",
            DeviceKind::Tablet => "input-tablet-symbolic",
            DeviceKind::Phone => "phone-symbolic",
            DeviceKind::Computer => "computer-symbolic",
            DeviceKind::Watch => "smartwatch-symbolic",
            DeviceKind::Printer => "printer-symbolic",
            DeviceKind::Camera => "camera-photo-symbolic",
            DeviceKind::Tv => "video-display-symbolic",
            DeviceKind::Network => "network-wireless-symbolic",
            DeviceKind::Other => "bluetooth-symbolic",
        }
    }
}

/// Build the state from BlueZ's objects.
pub(crate) fn build(objects: &Objects) -> BluetoothState {
    let adapters = objects
        .with(IFACE_ADAPTER)
        .map(|(path, p)| Adapter {
            path: path.to_owned(),
            address: p.str("Address"),
            name: p.str("Name"),
            alias: p.str("Alias"),
            powered: p.bool("Powered"),
            power_state: p
                .opt_str("PowerState")
                .unwrap_or_else(|| if p.bool("Powered") { "on".into() } else { "off".into() }),
            discoverable: p.bool("Discoverable"),
            discoverable_timeout: p.u32("DiscoverableTimeout"),
            pairable: p.bool("Pairable"),
            discovering: p.bool("Discovering"),
        })
        .collect();
    let mut devices: Vec<Device> =
        objects.with(IFACE_DEVICE).map(|(path, p)| device(path, p, objects.get(path, IFACE_BATTERY))).collect();
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.paired.cmp(&a.paired))
            .then(b.has_name.cmp(&a.has_name))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.address.cmp(&b.address))
    });
    BluetoothState { available: true, adapters, devices }
}

fn device(path: &str, p: &Props, battery: Option<&Props>) -> Device {
    let address = p.str("Address");
    let own_name = p.opt_str("Name").filter(|n| !n.trim().is_empty());
    let alias = p.opt_str("Alias").filter(|a| !a.trim().is_empty());
    // BlueZ makes the alias from the address when the device sent no name
    // (with dashes for colons).
    let alias_is_address = alias.as_deref().is_some_and(|a| a.replace('-', ":").eq_ignore_ascii_case(&address));
    let has_name = own_name.is_some() || (alias.is_some() && !alias_is_address);
    let icon = p.str("Icon");
    Device {
        path: path.to_owned(),
        adapter: p.path("Adapter").unwrap_or_default(),
        name: alias.or(own_name).unwrap_or_else(|| address.clone()),
        has_name,
        kind: DeviceKind::of(&icon, p.u32("Class"), p.u32("Appearance")),
        icon,
        paired: p.bool("Paired"),
        bonded: p.bool("Bonded"),
        trusted: p.bool("Trusted"),
        blocked: p.bool("Blocked"),
        connected: p.bool("Connected"),
        battery: battery.and_then(|b| b.opt_i64("Percentage")).and_then(|n| u8::try_from(n.clamp(0, 100)).ok()),
        rssi: p.opt_i64("RSSI").and_then(|n| i16::try_from(n).ok()),
        services_resolved: p.bool("ServicesResolved"),
        legacy_pairing: p.bool("LegacyPairing"),
        uuids: p.strings("UUIDs"),
        address,
    }
}

/// BlueZ: adapters and devices.
pub struct Bluetooth {
    bus: Option<Bus>,
    model: Arc<Model<BluetoothState>>,
    stop: Stop,
    agent: agent::Worker,
    rfkill: std::path::PathBuf,
}

service_handle!(Bluetooth, BluetoothState);

impl Bluetooth {
    /// Follow BlueZ on the system bus.
    pub fn new(wake: Wake) -> Bluetooth {
        match Bus::system() {
            Ok(bus) => Bluetooth::with_bus(bus, wake),
            Err(error) => {
                log::info!("aizome-sys: no system bus for BlueZ: {error}");
                Bluetooth {
                    bus: None,
                    model: Arc::new(Model::new(BluetoothState::default(), wake)),
                    stop: Stop::default(),
                    agent: agent::Worker::default(),
                    rfkill: crate::network::rfkill::DEVICE.into(),
                }
            }
        }
    }

    /// Follow BlueZ on `bus`.
    pub fn with_bus(bus: Bus, wake: Wake) -> Bluetooth {
        let model = Arc::new(Model::new(BluetoothState::default(), wake));
        let stop = Stop::default();
        let mut objects = Objects::default();
        let mut present = false;
        let mut dirty = false;
        let follower_model = model.clone();
        bus::follow(
            bus.clone(),
            BLUEZ,
            vec![signals_from(BLUEZ)],
            false,
            stop.clone(),
            move |bus, event| match event {
                Event::Appeared => {
                    present = true;
                    objects = Objects::load(bus, BLUEZ, "/").unwrap_or_else(|error| {
                        log::warn!("aizome-sys: reading BlueZ's objects: {error}");
                        Objects::default()
                    });
                    dirty = true;
                }
                Event::Vanished => {
                    present = false;
                    objects = Objects::default();
                    follower_model.set_if_changed(BluetoothState::default());
                }
                Event::Signal(msg) => {
                    let applied = objects.apply(&msg);
                    if !applied.invalidated.is_empty() {
                        objects.refetch(bus, BLUEZ, applied.invalidated);
                    }
                    dirty |= applied.changed;
                }
                Event::Idle => {
                    if present && dirty {
                        follower_model.set_if_changed(build(&objects));
                        dirty = false;
                    }
                }
            },
        );
        Bluetooth {
            bus: Some(bus),
            model,
            stop,
            agent: agent::Worker::default(),
            rfkill: crate::network::rfkill::DEVICE.into(),
        }
    }

    /// Use `device` instead of `/dev/rfkill` to unblock Bluetooth (tests).
    pub fn with_rfkill_device(mut self, device: std::path::PathBuf) -> Bluetooth {
        self.rfkill = device;
        self
    }

    /// Whether BlueZ is running.
    pub fn available(&self) -> bool {
        self.model.get().available
    }

    fn task<T: Send + 'static>(&self, work: impl FnOnce(&Bus) -> Result<T> + Send + 'static) -> Task<T> {
        bus::spawn(&self.bus, "BlueZ", self.model.wake(), work)
    }

    fn set(
        &self,
        path: &str,
        iface: &'static str,
        name: &'static str,
        value: impl Into<zbus::zvariant::Value<'static>> + Send + 'static,
    ) -> Task<()> {
        let path = path.to_owned();
        self.task(move |bus| bus.set(BLUEZ, &path, iface, name, value))
    }

    /// Switch an adapter on or off. An adapter a radio switch keeps off
    /// (`PowerState` `off-blocked`) is unblocked through `/dev/rfkill`
    /// first, as a Bluetooth switch in the UI means.
    pub fn set_powered(&self, adapter: &str, on: bool) -> Task<()> {
        let blocked = on && self.snapshot().adapter(adapter).is_some_and(Adapter::blocked);
        let adapter = adapter.to_owned();
        let device = self.rfkill.clone();
        self.task(move |bus| {
            if blocked {
                crate::network::rfkill::block_type(&device, crate::network::rfkill::TYPE_BLUETOOTH, false)?;
                // BlueZ notices the unblock and the adapter comes back first.
                std::thread::sleep(Duration::from_millis(300));
            }
            bus.set(BLUEZ, &adapter, IFACE_ADAPTER, "Powered", on)
        })
    }

    /// Let other devices find this one (for `DiscoverableTimeout`
    /// seconds, as the adapter is set up).
    pub fn set_discoverable(&self, adapter: &str, on: bool) -> Task<()> {
        self.set(adapter, IFACE_ADAPTER, "Discoverable", on)
    }

    pub fn set_pairable(&self, adapter: &str, on: bool) -> Task<()> {
        self.set(adapter, IFACE_ADAPTER, "Pairable", on)
    }

    /// The name other devices see.
    pub fn set_adapter_name(&self, adapter: &str, name: &str) -> Task<()> {
        self.set(adapter, IFACE_ADAPTER, "Alias", name.to_owned())
    }

    /// Start scanning for devices; they appear in the snapshot as they're
    /// found. BlueZ stops when [`stop_discovery`](Bluetooth::stop_discovery)
    /// is called or this process leaves the bus.
    pub fn start_discovery(&self, adapter: &str) -> Task<()> {
        let adapter = adapter.to_owned();
        self.task(move |bus| {
            match bus.call_with::<_, ()>(Opts::ACTION, BLUEZ, &adapter, IFACE_ADAPTER, "StartDiscovery", &()) {
                Err(Error::DBus { name, .. }) if name == "org.bluez.Error.InProgress" => Ok(()),
                other => other,
            }
        })
    }

    pub fn stop_discovery(&self, adapter: &str) -> Task<()> {
        let adapter = adapter.to_owned();
        self.task(move |bus| {
            match bus.call_with::<_, ()>(Opts::ACTION, BLUEZ, &adapter, IFACE_ADAPTER, "StopDiscovery", &()) {
                Err(Error::DBus { name, .. }) if name == "org.bluez.Error.Failed" => Ok(()),
                other => other,
            }
        })
    }

    /// Pair with a device, trust it (so it may reconnect by itself) and
    /// connect. The agent ([`register_agent`](Bluetooth::register_agent))
    /// is asked for codes as needed; a refused code ends in
    /// [`Error::Rejected`].
    pub fn pair(&self, device: &str) -> Task<()> {
        let device = device.to_owned();
        self.task(move |bus| {
            match bus.call_with::<_, ()>(Opts::long(PAIR_TIMEOUT), BLUEZ, &device, IFACE_DEVICE, "Pair", &()) {
                Ok(()) => {}
                Err(Error::DBus { name, .. }) if name == "org.bluez.Error.AlreadyExists" => {}
                Err(error) => return Err(error),
            }
            bus.set(BLUEZ, &device, IFACE_DEVICE, "Trusted", true)?;
            connect(bus, &device)
        })
    }

    /// Stop pairing in progress.
    pub fn cancel_pairing(&self, device: &str) -> Task<()> {
        let device = device.to_owned();
        self.task(move |bus| bus.call_with(Opts::ACTION, BLUEZ, &device, IFACE_DEVICE, "CancelPairing", &()))
    }

    /// Connect a paired device's profiles.
    pub fn connect(&self, device: &str) -> Task<()> {
        let device = device.to_owned();
        self.task(move |bus| connect(bus, &device))
    }

    pub fn disconnect(&self, device: &str) -> Task<()> {
        let device = device.to_owned();
        self.task(move |bus| {
            bus.call_with(Opts::long(CONNECT_TIMEOUT), BLUEZ, &device, IFACE_DEVICE, "Disconnect", &())
        })
    }

    /// Whether the device may connect without asking.
    pub fn set_trusted(&self, device: &str, on: bool) -> Task<()> {
        self.set(device, IFACE_DEVICE, "Trusted", on)
    }

    /// Refuse the device's connections.
    pub fn set_blocked(&self, device: &str, on: bool) -> Task<()> {
        self.set(device, IFACE_DEVICE, "Blocked", on)
    }

    pub fn rename_device(&self, device: &str, name: &str) -> Task<()> {
        self.set(device, IFACE_DEVICE, "Alias", name.to_owned())
    }

    /// Forget a device: unpair and remove it.
    pub fn remove(&self, device: &str) -> Task<()> {
        let state = self.snapshot();
        let adapter = state.device(device).map(|d| d.adapter.clone());
        let device = device.to_owned();
        self.task(move |bus| {
            let adapter = adapter.ok_or_else(|| Error::NotFound(device.clone()))?;
            let path = ObjectPath::try_from(device.as_str())?;
            bus.call_with(Opts::ACTION, BLUEZ, &adapter, IFACE_ADAPTER, "RemoveDevice", &(path,))
        })
    }

    /// Serve the pairing agent and make it BlueZ's default: each request
    /// for a PIN, a passkey or a confirmation goes to `prompt`, which shows
    /// it and answers through the prompt's [`Responder`] when the person
    /// does. Replaces an agent registered before. Fails when BlueZ isn't
    /// running. It takes a few round trips to BlueZ, made on the agent's
    /// own thread: this returns at once.
    pub fn register_agent(&self, prompt: impl Fn(PairingPrompt) + Send + Sync + 'static) -> Task<()> {
        let Some(bus) = self.bus.as_ref() else { return Task::ready(Err(Error::Unavailable("BlueZ".into()))) };
        self.agent.register(bus, Arc::new(prompt), self.model.wake())
    }

    /// Stop serving the agent (after any registration asked for before).
    pub fn unregister_agent(&self) -> Task<()> {
        self.agent.unregister(self.model.wake())
    }
}

impl Drop for Bluetooth {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

fn connect(bus: &Bus, device: &str) -> Result<()> {
    match bus.call_with::<_, ()>(Opts::long(CONNECT_TIMEOUT), BLUEZ, device, IFACE_DEVICE, "Connect", &()) {
        Err(Error::DBus { name, .. }) if name == "org.bluez.Error.AlreadyConnected" => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::owned;
    use zbus::zvariant::OwnedValue;

    fn props(entries: Vec<(&str, OwnedValue)>) -> Props {
        entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    #[test]
    fn devices_are_named_sorted_and_classified() {
        let mut objects = Objects::default();
        let mut add = |path: &str, iface: &str, p: Props| {
            objects.map.entry(path.to_owned()).or_default().insert(iface.to_owned(), p);
        };
        add("/org/bluez/hci0", IFACE_ADAPTER, props(vec![("Powered", owned(true)), ("Alias", owned("ganymede"))]));
        add(
            "/org/bluez/hci0/dev_A",
            IFACE_DEVICE,
            props(vec![
                ("Address", owned("AA:AA:AA:AA:AA:AA")),
                ("Alias", owned("AA-AA-AA-AA-AA-AA")),
                ("RSSI", owned(-60i16)),
            ]),
        );
        add(
            "/org/bluez/hci0/dev_B",
            IFACE_DEVICE,
            props(vec![
                ("Address", owned("BB:BB:BB:BB:BB:BB")),
                ("Name", owned("WH-1000XM4")),
                ("Alias", owned("WH-1000XM4")),
                ("Icon", owned("audio-headphones")),
                ("Paired", owned(true)),
                ("Connected", owned(true)),
            ]),
        );
        add("/org/bluez/hci0/dev_B", IFACE_BATTERY, props(vec![("Percentage", owned(80u8))]));
        add(
            "/org/bluez/hci0/dev_C",
            IFACE_DEVICE,
            props(vec![
                ("Address", owned("CC:CC:CC:CC:CC:CC")),
                ("Alias", owned("Keyboard K380")),
                ("Class", owned(0x2540u32)),
                ("Paired", owned(true)),
            ]),
        );
        let state = build(&objects);
        let names: Vec<&str> = state.devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["WH-1000XM4", "Keyboard K380", "AA-AA-AA-AA-AA-AA"]);
        assert_eq!(state.devices[0].kind, DeviceKind::Headphones);
        assert_eq!(state.devices[0].battery, Some(80));
        assert_eq!(state.devices[1].kind, DeviceKind::Keyboard);
        assert!(!state.devices[2].has_name);
        assert_eq!(state.devices[2].rssi, Some(-60));
        assert_eq!(state.nearby().count(), 0, "nameless devices aren't offered");
        assert!(state.powered() && state.connected());
        assert_eq!(state.default_adapter().unwrap().alias, "ganymede");
    }

    #[test]
    fn kinds_from_class_and_appearance() {
        assert_eq!(DeviceKind::of("", 0x5a020c, 0), DeviceKind::Phone);
        assert_eq!(DeviceKind::of("", 0x240404, 0), DeviceKind::Headset);
        assert_eq!(DeviceKind::of("", 0x002580, 0), DeviceKind::Mouse);
        assert_eq!(DeviceKind::of("", 0, 0x03C1), DeviceKind::Keyboard);
        assert_eq!(DeviceKind::of("", 0, 0x00C0), DeviceKind::Watch);
        assert_eq!(DeviceKind::of("", 0, 0), DeviceKind::Other);
    }
}
