// SPDX-License-Identifier: MIT OR Apache-2.0
//! BlueZ's typed ObjectManager snapshot. Device identities are object paths,
//! never names or addresses (the same device may appear on two adapters).
use gtk::glib::{self, variant::ObjectPath};
use std::collections::HashMap;

pub type Properties = HashMap<String, glib::Variant>;
pub type Objects = HashMap<ObjectPath, HashMap<String, Properties>>;
pub const ADAPTER: &str = "org.bluez.Adapter1";
pub const DEVICE: &str = "org.bluez.Device1";

fn get<T: glib::variant::FromVariant + Default>(p: &Properties, key: &str) -> T {
    p.get(key).and_then(glib::Variant::get).unwrap_or_default()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Adapter {
    pub path: String,
    pub name: String,
    pub address: String,
    pub powered: bool,
    pub blocked: bool,
    pub discovering: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Device {
    pub path: String,
    pub adapter: String,
    pub name: String,
    pub address: String,
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub battery: Option<u8>,
}

impl Device {
    pub fn status(&self) -> String {
        let state = if self.blocked {
            "Blocked"
        } else if self.connected {
            "Connected"
        } else if self.paired {
            "Not connected"
        } else {
            "Ready to pair"
        };
        match self.battery {
            Some(level) if self.connected => format!("{state} · {level}% battery"),
            _ => state.into(),
        }
    }
    pub fn icon(&self) -> &str {
        match self.icon.as_str() {
            "audio-headset" => "audio-headset-symbolic",
            "audio-headphones" => "audio-headphones-symbolic",
            "audio-card" | "audio-speakers" => "audio-speakers-symbolic",
            "input-keyboard" => "input-keyboard-symbolic",
            "input-mouse" => "input-mouse-symbolic",
            "input-gaming" => "input-gaming-symbolic",
            "phone" => "phone-symbolic",
            "computer" => "computer-symbolic",
            _ => "bluetooth-symbolic",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub adapters: Vec<Adapter>,
    pub devices: Vec<Device>,
}

impl Snapshot {
    pub fn parse(objects: Objects) -> Self {
        let mut state = Self::default();
        for (path, interfaces) in objects {
            if let Some(p) = interfaces.get(ADAPTER) {
                state.adapters.push(Adapter {
                    path: path.to_string(),
                    name: get(p, "Alias"),
                    address: get(p, "Address"),
                    powered: get(p, "Powered"),
                    blocked: get::<String>(p, "PowerState") == "off-blocked",
                    discovering: get(p, "Discovering"),
                });
            }
            if let Some(p) = interfaces.get(DEVICE) {
                let address: String = get(p, "Address");
                let name: String = get(p, "Alias");
                state.devices.push(Device {
                    path: path.to_string(),
                    adapter: p
                        .get("Adapter")
                        .and_then(|v| v.get::<ObjectPath>())
                        .map(|p| p.to_string())
                        .unwrap_or_default(),
                    name: if name.is_empty() {
                        address.clone()
                    } else {
                        name
                    },
                    address,
                    icon: get(p, "Icon"),
                    paired: get::<bool>(p, "Paired") || get::<bool>(p, "Bonded"),
                    connected: get(p, "Connected"),
                    trusted: get(p, "Trusted"),
                    blocked: get(p, "Blocked"),
                    battery: interfaces
                        .get("org.bluez.Battery1")
                        .and_then(|p| p.get("Percentage"))
                        .and_then(|v| v.get::<u8>())
                        .filter(|v| *v <= 100),
                });
            }
        }
        state.adapters.sort_by(|a, b| a.path.cmp(&b.path));
        // Stable name order: connection and battery updates never move focus.
        state.devices.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.path.cmp(&b.path))
        });
        state
    }

    pub fn adapter(&self, selected: &str) -> Option<&Adapter> {
        self.adapters
            .iter()
            .find(|a| a.path == selected)
            .or_else(|| self.adapters.iter().find(|a| a.powered))
            .or_else(|| self.adapters.first())
    }
}

pub fn passkey(text: &str) -> Option<u32> {
    (text.len() == 6 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

pub fn pin(text: &str) -> bool {
    (1..=16).contains(&text.len()) && text.is_ascii() && !text.chars().any(char::is_control)
}

pub fn reason(error: &str) -> String {
    if error.contains("NotAuthorized") || error.contains("AccessDenied") {
        "The system did not allow this Bluetooth change. Check that your session has permission to manage Bluetooth.".into()
    } else if error.contains("AuthenticationRejected") {
        "Pairing was declined. Try again when the device is ready.".into()
    } else if error.contains("AuthenticationCanceled") {
        "Pairing was cancelled.".into()
    } else if error.contains("AuthenticationTimeout") || error.contains("AuthenticationFailed") {
        "Pairing failed. Put the device in pairing mode and check the code before trying again."
            .into()
    } else if error.contains("NotReady") || error.contains("rfkill") {
        "Turn Bluetooth on. If it is blocked, check airplane mode or the computer’s wireless switch.".into()
    } else if error.contains("ConnectionAttemptFailed") || error.contains("br-connection") {
        "Could not connect. Check that the device is on, nearby and available.".into()
    } else {
        error.trim_start_matches("GDBus.Error:").to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::variant::ToVariant;
    #[test]
    fn adapter_choice_survives_turning_it_off_and_falls_back_when_removed() {
        let state = Snapshot {
            adapters: vec![
                Adapter {
                    path: "/a".into(),
                    ..Default::default()
                },
                Adapter {
                    path: "/b".into(),
                    powered: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(state.adapter("/a").unwrap().path, "/a");
        assert_eq!(state.adapter("/removed").unwrap().path, "/b");
    }
    #[test]
    fn typed_snapshot_keeps_identity_battery_and_missing_names() {
        let path = ObjectPath::try_from("/org/bluez/hci0/dev_A").unwrap();
        let adapter = ObjectPath::try_from("/org/bluez/hci0").unwrap();
        let device = HashMap::from([
            ("Address".into(), "AA:BB".to_variant()),
            ("Adapter".into(), adapter.to_variant()),
            ("Bonded".into(), true.to_variant()),
            ("Connected".into(), true.to_variant()),
        ]);
        let objects = HashMap::from([(
            path.clone(),
            HashMap::from([
                (DEVICE.into(), device),
                (
                    "org.bluez.Battery1".into(),
                    HashMap::from([("Percentage".into(), 0u8.to_variant())]),
                ),
            ]),
        )]);
        let state = Snapshot::parse(objects);
        let d = &state.devices[0];
        assert_eq!(d.path, path.as_str());
        assert_eq!(d.adapter, adapter.as_str());
        assert_eq!(d.name, "AA:BB");
        assert!(d.paired);
        assert_eq!(d.status(), "Connected · 0% battery");
    }
    #[test]
    fn pairing_input_does_not_accept_partial_or_unicode_numbers() {
        assert_eq!(passkey("000042"), Some(42));
        for s in ["42", "1234567", "12345x", "１２３４５６", " 12345"] {
            assert_eq!(passkey(s), None);
        }
        assert!(pin("abcd-1234"));
        for s in ["", "12345678901234567", "123\n"] {
            assert!(!pin(s));
        }
    }
}
