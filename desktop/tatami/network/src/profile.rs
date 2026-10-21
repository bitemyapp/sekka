// SPDX-License-Identifier: MIT OR Apache-2.0
//! Atomic DNS/method + suspended-DNS metadata updates. nmcli does not expose
//! NMSettingUser; Gio is already part of the GTK application. GetSettings
//! excludes secrets, which NetworkManager preserves for a secret-free Update2.
use gtk::{
    gio,
    glib::{self, prelude::*},
};
use std::collections::HashMap;

const SERVICE: &str = "org.freedesktop.NetworkManager";
const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
type Properties = HashMap<String, HashMap<String, glib::Variant>>;

pub struct Profile {
    bus: gio::DBusConnection,
    path: String,
    pub version: u64,
    properties: Properties,
}

fn call(
    bus: &gio::DBusConnection,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
) -> Result<glib::Variant, String> {
    bus.call_sync(
        Some(SERVICE),
        path,
        interface,
        method,
        args,
        None,
        gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
        30_000,
        gio::Cancellable::NONE,
    )
    .map_err(|error| error.to_string())
}

impl Profile {
    pub fn read(uuid: &str) -> Result<Self, String> {
        let bus = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)
            .map_err(|error| error.to_string())?;
        let reply = call(
            &bus,
            "/org/freedesktop/NetworkManager/Settings",
            SETTINGS,
            "GetConnectionByUuid",
            Some(&(uuid,).to_variant()),
        )?;
        let path = reply
            .child_value(0)
            .str()
            .ok_or("Invalid NetworkManager profile path")?
            .to_owned();
        let mut profile = Self {
            bus,
            path,
            version: 0,
            properties: HashMap::new(),
        };
        profile.version = profile.current_version()?;
        let reply = call(&profile.bus, &profile.path, CONNECTION, "GetSettings", None)?;
        profile.properties = reply
            .child_value(0)
            .get()
            .ok_or("Invalid NetworkManager settings")?;
        Ok(profile)
    }

    pub fn current_version(&self) -> Result<u64, String> {
        let reply = call(
            &self.bus,
            &self.path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(CONNECTION, "VersionId").to_variant()),
        )?;
        let value = reply
            .child_value(0)
            .as_variant()
            .ok_or("Missing profile version")?;
        value
            .get::<u64>()
            .filter(|version| *version != 0)
            .ok_or("Invalid profile version".into())
    }

    fn user_data(&self) -> HashMap<String, String> {
        self.properties
            .get("user")
            .and_then(|p| p.get("data"))
            .and_then(|v| v.get())
            .unwrap_or_default()
    }

    pub fn suspended_dns(&self) -> Option<String> {
        self.user_data().get(crate::dns::BACKUP_KEY).cloned()
    }

    pub fn update(
        mut self,
        expected_version: u64,
        changes: &[(&str, String)],
    ) -> Result<(), String> {
        if expected_version == 0 || self.version != expected_version {
            return Err("The connection changed. Check its settings and try again.".into());
        }
        for (name, value) in changes {
            match *name {
                "+user.data" | "-user.data" => {
                    let mut data = self.user_data();
                    if *name == "+user.data" {
                        let (key, value) = value.split_once('=').ok_or("Invalid suspended DNS")?;
                        if key != crate::dns::BACKUP_KEY {
                            return Err("Invalid DNS metadata key".into());
                        }
                        data.insert(key.into(), value.into());
                    } else {
                        if value != crate::dns::BACKUP_KEY {
                            return Err("Invalid DNS metadata key".into());
                        }
                        data.remove(value);
                    }
                    self.properties
                        .entry("user".into())
                        .or_default()
                        .insert("data".into(), data.to_variant());
                }
                "ipv4.dns" | "ipv6.dns" => {
                    let family = name.split_once('.').unwrap().0;
                    let ip = self.properties.entry(family.into()).or_default();
                    // NM serializes both forms. Remove deprecated numeric DNS
                    // so it cannot take precedence over the new string list.
                    ip.remove("dns");
                    let servers: Vec<_> = value.split(',').filter(|s| !s.is_empty()).collect();
                    ip.insert("dns-data".into(), servers.to_variant());
                }
                "ipv4.ignore-auto-dns" | "ipv6.ignore-auto-dns" | "ipv6.method" => {
                    let (family, key) = name.split_once('.').unwrap();
                    let value = if key == "method" {
                        value.to_variant()
                    } else {
                        (value == "yes").to_variant()
                    };
                    self.properties
                        .entry(family.into())
                        .or_default()
                        .insert(key.into(), value);
                }
                _ => return Err("Unsupported DNS profile change".into()),
            }
        }
        let args = HashMap::from([("version-id", self.version.to_variant())]);
        call(
            &self.bus,
            &self.path,
            CONNECTION,
            "Update2",
            Some(&(self.properties, 1u32, args).to_variant()),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Runs only in network-dns-vm, against its named disposable profile.
    #[test]
    #[ignore = "requires the disposable network-dns-vm fixture"]
    fn network_profile_transactions() {
        let uuid = std::env::var("TATAMI_NETWORK_VM_TEST_UUID").expect("VM fixture UUID");
        let nm = |args: &[&str]| {
            let out = Command::new("nmcli").args(args).output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        let profile = Profile::read(&uuid).unwrap();
        assert_eq!(
            profile.properties["connection"]["id"].str(),
            Some("Tatami DNS fixture")
        );
        let original_wireless = profile.properties["802-11-wireless"].clone();
        let original_security = profile.properties["802-11-wireless-security"].clone();
        assert_eq!(
            profile
                .user_data()
                .get("fixture.unrelated")
                .map(String::as_str),
            Some("keep, this = value")
        );

        let settings = crate::nm::settings(&uuid).unwrap();
        let changes = settings.dns_changes(vec!["9.9.9.9".into()], vec!["2620:fe::fe".into()]);
        Profile::read(&uuid)
            .unwrap()
            .update(settings.version, &changes)
            .unwrap();
        let settings = crate::nm::settings(&uuid).unwrap();
        Profile::read(&uuid)
            .unwrap()
            .update(
                settings.version,
                &settings.ipv6_changes("disabled").unwrap(),
            )
            .unwrap();
        let off = Profile::read(&uuid).unwrap();
        assert_eq!(off.properties["802-11-wireless"], original_wireless);
        assert_eq!(
            off.properties["802-11-wireless-security"],
            original_security
        );
        assert_eq!(
            off.user_data().get("fixture.unrelated").map(String::as_str),
            Some("keep, this = value")
        );
        assert_eq!(
            nm(&[
                "--show-secrets",
                "-g",
                "802-11-wireless-security.psk",
                "connection",
                "show",
                "uuid",
                &uuid
            ]),
            "disposable-fixture-password"
        );
        let settings = crate::nm::settings(&uuid).unwrap();
        assert_eq!(
            settings.suspended_dns().unwrap(),
            Some(vec!["2620:fe::fe".into()])
        );
        Profile::read(&uuid)
            .unwrap()
            .update(settings.version, &settings.ipv6_changes("auto").unwrap())
            .unwrap();
        let active = Profile::read(&uuid).unwrap();
        assert!(active.suspended_dns().is_none());
        assert_eq!(
            active.user_data(),
            HashMap::from([("fixture.unrelated".into(), "keep, this = value".into())])
        );
        assert_eq!(
            crate::nm::settings(&uuid).unwrap().ipv6_dns,
            ["2620:fe::fe"]
        );
        println!(
            "Atomic Off/Auto preserved custom DNS, unrelated settings/map data and stored WPA secret"
        );

        // A second writer changes the actual daemon after this snapshot. The
        // server's version guard, not merely our initial comparison, must fail.
        let stale = Profile::read(&uuid).unwrap();
        let version = stale.version;
        let before_ip = stale.properties["ipv4"].clone();
        nm(&[
            "connection",
            "modify",
            "uuid",
            &uuid,
            "connection.metered",
            "yes",
        ]);
        let error = stale
            .update(version, &[("ipv4.dns", "8.8.8.8".into())])
            .unwrap_err();
        assert!(error.contains("version-id"), "{error}");
        let after = Profile::read(&uuid).unwrap();
        assert_eq!(after.properties["ipv4"], before_ip);
        assert_eq!(
            nm(&[
                "-g",
                "connection.metered",
                "connection",
                "show",
                "uuid",
                &uuid
            ]),
            "yes"
        );
        println!(
            "Real concurrent Update2 rejected stale version without partial DNS mutation: {error}"
        );

        // A rejected backend validation must leave metadata and all settings
        // unchanged, not merely report an error after a partial update.
        let before = after.properties.clone();
        let version = after.version;
        let error = after
            .update(version, &[("ipv6.method", "disabled".into())])
            .unwrap_err();
        assert_eq!(Profile::read(&uuid).unwrap().properties, before);
        assert_eq!(
            nm(&[
                "--show-secrets",
                "-g",
                "802-11-wireless-security.psk",
                "connection",
                "show",
                "uuid",
                &uuid
            ]),
            "disposable-fixture-password"
        );
        println!("Backend validation rejected invalid method/DNS atomically: {error}");
    }
}
