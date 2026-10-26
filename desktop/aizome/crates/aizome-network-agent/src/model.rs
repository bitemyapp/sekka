use crate::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zbus::zvariant::{OwnedValue, Value};

pub type Settings = HashMap<String, HashMap<String, OwnedValue>>;
pub const ALLOW_INTERACTION: u32 = 1;
pub const REQUEST_NEW: u32 = 2;
pub const AGENT_OWNED: u32 = 1;
pub const NOT_SAVED: u32 = 2;
pub const NOT_REQUIRED: u32 = 4;

pub fn string(settings: &Settings, group: &str, key: &str) -> String {
    settings.get(group).and_then(|g| g.get(key)).and_then(|v| <&str>::try_from(v).ok()).unwrap_or("").into()
}
pub fn number(settings: &Settings, group: &str, key: &str) -> u32 {
    settings.get(group).and_then(|g| g.get(key)).and_then(|v| u32::try_from(v).ok()).unwrap_or(0)
}
pub fn dict(settings: &Settings, group: &str, key: &str) -> HashMap<String, String> {
    settings
        .get(group)
        .and_then(|g| g.get(key))
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| HashMap::try_from(v).ok())
        .unwrap_or_default()
}
pub fn text_value(s: String) -> OwnedValue {
    Value::from(s).try_to_owned().unwrap()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub value: String,
    pub secret: bool,
    pub required: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub title: String,
    pub message: String,
    pub fields: Vec<Field>,
}

pub struct Profile {
    pub settings: Settings,
    pub uuid: String,
    pub name: String,
}
impl Profile {
    fn peers(&self) -> Vec<HashMap<String, OwnedValue>> {
        self.settings
            .get("wireguard")
            .and_then(|s| s.get("peers"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::try_from(v).ok())
            .unwrap_or_default()
    }

    pub fn new(settings: Settings) -> Result<Self> {
        let uuid = string(&settings, "connection", "uuid");
        if uuid.len() != 36
            || !uuid
                .bytes()
                .enumerate()
                .all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == b'-' } else { c.is_ascii_hexdigit() })
        {
            return Err(AgentError::InvalidConnection("Connection has no valid UUID".into()));
        }
        let name = string(&settings, "connection", "id");
        Ok(Self { settings, uuid, name })
    }
    pub fn flags(&self, setting: &str, key: &str) -> u32 {
        if setting == "wireguard"
            && let Some(public_key) = key.strip_prefix("peers.").and_then(|k| k.strip_suffix(".preshared-key"))
        {
            return self
                .peers()
                .iter()
                .find(|peer| peer.get("public-key").and_then(|v| <&str>::try_from(v).ok()) == Some(public_key))
                .and_then(|p| p.get("preshared-key-flags"))
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0);
        }
        if setting == "vpn" {
            dict(&self.settings, "vpn", "data").get(&format!("{key}-flags")).and_then(|s| s.parse().ok()).unwrap_or(0)
        } else {
            let key = if setting == "802-11-wireless-security" && key.starts_with("wep-key") { "wep-key" } else { key };
            number(&self.settings, setting, &format!("{key}-flags"))
        }
    }
    /// Whether the profile keeps this secret in the person's keyring.
    pub fn saves(&self, setting: &str, key: &str) -> bool {
        self.flags(setting, key) & (AGENT_OWNED | NOT_SAVED | NOT_REQUIRED) == AGENT_OWNED
    }
    pub fn supplied(&self, setting: &str) -> HashMap<String, String> {
        if setting == "vpn" {
            return dict(&self.settings, setting, "secrets");
        }
        let mut values: HashMap<String, String> = self
            .settings
            .get(setting)
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| {
                let value = if k == "password-raw" {
                    Vec::<u8>::try_from(v.try_clone().ok()?).ok().and_then(|bytes| String::from_utf8(bytes).ok())
                } else {
                    <&str>::try_from(v).ok().map(str::to_owned)
                };
                value.map(|v| (k.clone(), v))
            })
            .collect();
        if setting == "wireguard" {
            for peer in self.peers() {
                if let (Some(public), Some(secret)) = (
                    peer.get("public-key").and_then(|v| <&str>::try_from(v).ok()),
                    peer.get("preshared-key").and_then(|v| <&str>::try_from(v).ok()),
                ) {
                    values.insert(format!("peers.{public}.preshared-key"), secret.into());
                }
            }
        }
        values
    }
    pub fn fields(&self, setting: &str, hints: &[String]) -> Result<Vec<Field>> {
        let supported: &[(&str, &str)] = match setting {
            "802-11-wireless-security" => match string(&self.settings, setting, "key-mgmt").as_str() {
                "none" => {
                    if self.flags(setting, "wep-key") & NOT_REQUIRED != 0 {
                        return Err(AgentError::NoSecrets("Wi-Fi key is not required".into()));
                    }
                    return Ok(vec![self.field(
                        &format!("wep-key{}", number(&self.settings, setting, "wep-tx-keyidx").min(3)),
                        "Wi-Fi key",
                    )]);
                }
                "wpa-psk" | "sae" | "wpa-none" => &[("psk", "Wi-Fi password")],
                _ => return Err(AgentError::NoSecrets("This Wi-Fi security method has no password".into())),
            },
            "802-1x" => &[
                ("password", "Password"),
                ("password-raw", "Password"),
                ("private-key-password", "Private key password"),
                ("phase2-private-key-password", "Inner private key password"),
                ("pin", "Security token PIN"),
            ],
            "gsm" => &[("password", "Mobile network password"), ("pin", "SIM PIN")],
            "cdma" | "pppoe" | "adsl" => &[("password", "Password")],
            "wireguard" => &[("private-key", "WireGuard private key")],
            _ => return Err(AgentError::NoSecrets(format!("Unsupported credential setting: {setting}"))),
        };
        let mut fields = Vec::new();
        for (key, label) in supported {
            if self.flags(setting, key) & NOT_REQUIRED != 0 {
                continue;
            }
            let requested = if !hints.is_empty() {
                hints.iter().any(|h| h == key)
            } else if setting == "802-1x" {
                let tls = self
                    .settings
                    .get(setting)
                    .and_then(|s| s.get("eap"))
                    .and_then(|v| v.try_clone().ok())
                    .and_then(|v| Vec::<String>::try_from(v).ok())
                    .is_some_and(|m| m.iter().any(|m| m == "tls"));
                if tls { *key == "private-key-password" } else { *key == "password" }
            } else if setting == "gsm" {
                *key == "password"
            } else {
                true
            };
            if requested {
                fields.push(self.field(key, label));
            }
        }
        if setting == "wireguard" {
            for peer in self.peers() {
                let Some(public) = peer.get("public-key").and_then(|v| <&str>::try_from(v).ok()) else {
                    continue;
                };
                let key = format!("peers.{public}.preshared-key");
                let flags = self.flags(setting, &key);
                if flags & NOT_REQUIRED == 0
                    && if hints.is_empty() { flags & (AGENT_OWNED | NOT_SAVED) != 0 } else { hints.contains(&key) }
                {
                    fields.push(self.field(&key, &format!("Preshared key for peer {public}")));
                }
            }
        }
        if fields.is_empty() {
            return Err(AgentError::NoSecrets("No supported secrets were requested".into()));
        }
        Ok(fields)
    }
    fn field(&self, key: &str, label: &str) -> Field {
        Field { key: key.into(), label: label.into(), value: String::new(), secret: true, required: true }
    }
    pub fn reply(&self, setting: &str, values: HashMap<String, String>) -> Settings {
        let mut props = HashMap::new();
        if setting == "vpn" {
            props.insert("secrets".into(), Value::from(values).try_to_owned().unwrap());
        } else {
            let mut peers = Vec::<HashMap<&str, OwnedValue>>::new();
            for (key, value) in values {
                if setting == "wireguard"
                    && let Some(public) = key.strip_prefix("peers.").and_then(|k| k.strip_suffix(".preshared-key"))
                {
                    peers.push(HashMap::from([
                        ("public-key", text_value(public.into())),
                        ("preshared-key", text_value(value)),
                    ]));
                    continue;
                }
                let v = if key == "password-raw" {
                    Value::from(value.into_bytes()).try_to_owned().unwrap()
                } else {
                    text_value(value)
                };
                props.insert(key, v);
            }
            if !peers.is_empty() {
                props.insert("peers".into(), Value::from(peers).try_to_owned().unwrap());
            }
        }
        HashMap::from([(setting.into(), props)])
    }
}
