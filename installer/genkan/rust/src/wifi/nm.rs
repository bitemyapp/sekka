// SPDX-License-Identifier: GPL-3.0-or-later
//! Small libnm serialization boundary; D-Bus calls are read-only and bounded.
//! libnm parses/validates NetworkManager's native format, not a homemade INI parser.
use anyhow::{Result, ensure};
use gio::{DBusCallFlags, DBusConnection};
use glib::prelude::ToVariant;
use glib::{KeyFile, KeyFileFlags, Object, Variant, translate::*};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void},
    ptr,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[link(name = "nm")]
unsafe extern "C" {
    fn nm_simple_connection_new_from_dbus(
        settings: *mut glib::ffi::GVariant,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut glib::gobject_ffi::GObject;
    fn nm_connection_update_secrets(
        connection: *mut glib::gobject_ffi::GObject,
        setting: *const c_char,
        secrets: *mut glib::ffi::GVariant,
        error: *mut *mut glib::ffi::GError,
    ) -> i32;
    fn nm_keyfile_write(
        connection: *mut glib::gobject_ffi::GObject,
        flags: u32,
        handler: *const c_void,
        data: *mut c_void,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut glib::ffi::GKeyFile;
    fn nm_keyfile_read(
        keyfile: *mut glib::ffi::GKeyFile,
        base_dir: *const c_char,
        flags: u32,
        handler: *const c_void,
        data: *mut c_void,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut glib::gobject_ffi::GObject;
    fn nm_connection_verify(
        connection: *mut glib::gobject_ffi::GObject,
        error: *mut *mut glib::ffi::GError,
    ) -> i32;
    fn nm_connection_need_secrets(
        connection: *mut glib::gobject_ffi::GObject,
        hints: *mut *mut glib::ffi::GPtrArray,
    ) -> *const c_char;
}

fn dbus(
    bus: &DBusConnection,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&Variant>,
) -> Result<Variant> {
    bus.call_sync(Some("org.freedesktop.NetworkManager"), path, interface, method, args, None,
        DBusCallFlags::NONE, 5000, gio::Cancellable::NONE)
        // D-Bus errors may include private connection names. Never propagate them.
        .map_err(|_| anyhow::anyhow!("Cannot read live Wi-Fi settings. Check NetworkManager is running and the live user's wallet is unlocked, or turn off Wi-Fi transfer."))
}

fn serialize(connection: &Object) -> Result<KeyFile> {
    // All pointers are borrowed for this call. A successful libnm return is a
    // full reference adopted exactly once; no error text or secret is logged.
    let raw = unsafe {
        nm_keyfile_write(
            connection.to_glib_none().0,
            0,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    ensure!(!raw.is_null(), "Unable to serialize a Wi-Fi profile");
    Ok(unsafe { from_glib_full(raw) })
}

pub(crate) fn snapshot() -> Result<Vec<String>> {
    let context = glib::MainContext::new();
    context.with_thread_default(|| -> Result<Vec<String>> {
        let bus = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)
            .map_err(|_| anyhow::anyhow!("NetworkManager's system bus is unavailable"))?;
        let reply = dbus(&bus, "/org/freedesktop/NetworkManager/Settings", "org.freedesktop.NetworkManager.Settings", "ListConnections", None)?;
        let paths = reply.child_value(0);
        ensure!(paths.n_children() <= 256, "Too many live network connections");
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut profiles = Zeroizing::new(Vec::new());
        for index in 0..paths.n_children() {
            ensure!(Instant::now() < deadline, "Wi-Fi collection timed out; no disk has been erased");
            let path_value = paths.child_value(index);
            let path = path_value.str().ok_or_else(|| anyhow::anyhow!("Invalid NetworkManager object path"))?;
            let interface = "org.freedesktop.NetworkManager.Settings.Connection";
            let settings = dbus(&bus, path, interface, "GetSettings", None)?.child_value(0);
            let values = settings.get::<HashMap<String, HashMap<String, Variant>>>()
                .ok_or_else(|| anyhow::anyhow!("Invalid NetworkManager settings response"))?;
            if values.get("connection").and_then(|c| c.get("type")).and_then(|v| v.get::<String>()).as_deref() != Some("802-11-wireless") { continue; }
            ensure!(profiles.len() < 32, "More than 32 Wi-Fi profiles; turn off Wi-Fi transfer to continue");
            let raw = unsafe { nm_simple_connection_new_from_dbus(settings.to_glib_none().0, ptr::null_mut()) };
            ensure!(!raw.is_null(), "Invalid live Wi-Fi connection");
            let connection: Object = unsafe { from_glib_full(raw) };
            for section in ["802-11-wireless-security", "802-1x"] {
                if values.contains_key(section) {
                    // This call can consult the live user's Secret Agent. It
                    // must run before privilege elevation, in the GUI worker.
                    let args = (section,).to_variant();
                    let secrets = dbus(&bus, path, interface, "GetSecrets", Some(&args))?.child_value(0);
                    let ok = unsafe { nm_connection_update_secrets(connection.to_glib_none().0, ptr::null(), secrets.to_glib_none().0, ptr::null_mut()) };
                    ensure!(ok != 0, "Cannot retrieve saved Wi-Fi credentials; unlock the live wallet or disable transfer");
                }
            }
            let serialized = Zeroizing::new(serialize(&connection)?.to_data().to_string());
            // The plan parser retains the normalized result for the target
            // username. Do not normalize and discard it here first.
            profiles.push(serialized.to_string());
        }
        Ok(std::mem::take(&mut *profiles))
    }).map_err(|_| anyhow::anyhow!("Cannot create the Wi-Fi worker context"))?
}

pub(super) fn parse_profile(
    profile: &str,
    username: &crate::Username,
) -> Result<super::WifiProfile> {
    ensure!(profile.len() <= 16384, "Wi-Fi profile exceeds size limit");
    let key = KeyFile::new();
    key.load_from_data(profile, KeyFileFlags::NONE)
        .map_err(|_| anyhow::anyhow!("Invalid Wi-Fi profile format"))?;
    for group in key.groups().iter() {
        ensure!(
            [
                "connection",
                "wifi",
                "wifi-security",
                "802-1x",
                "ipv4",
                "ipv6",
                "proxy"
            ]
            .contains(&group.as_str()),
            "Unsupported Wi-Fi profile settings; disable transfer and configure this network after installation"
        );
    }
    ensure!(
        key.string("connection", "type").ok().as_deref() == Some("wifi"),
        "Only Wi-Fi connections may be transferred"
    );
    // External EAP files/tokens cannot safely be copied by a privileged helper
    // from client-supplied paths. Never silently leave a broken reference.
    for field in [
        "ca-cert",
        "client-cert",
        "private-key",
        "phase2-ca-cert",
        "phase2-client-cert",
        "phase2-private-key",
        "pac-file",
        "ca-path",
        "phase2-ca-path",
    ] {
        ensure!(
            key.string("802-1x", field).is_err(),
            "A Wi-Fi profile uses enterprise certificates or external keys. Disable Wi-Fi transfer and configure that network after installation."
        );
    }
    if key
        .string("connection", "permissions")
        .is_ok_and(|p| !p.is_empty())
    {
        key.set_string(
            "connection",
            "permissions",
            &format!("user:{}:;", username.as_str()),
        );
    }
    // A transferred system keyfile owns its saved secret; the live wallet does
    // not exist in the target. Preserve intentionally NOT_REQUIRED secrets.
    for group in ["wifi-security", "802-1x"] {
        for field in [
            "psk",
            "wep-key0",
            "wep-key1",
            "wep-key2",
            "wep-key3",
            "password",
            "password-raw",
            "pin",
        ] {
            if key.string(group, field).is_ok() {
                let flags = if field.starts_with("wep-key") {
                    "wep-key-flags".to_string()
                } else {
                    format!("{field}-flags")
                };
                key.set_integer(group, &flags, 0);
            }
        }
    }
    if key
        .string("wifi-security", "key-mgmt")
        .is_ok_and(|s| matches!(s.as_str(), "wpa-psk" | "sae"))
    {
        ensure!(
            key.string("wifi-security", "psk")
                .is_ok_and(|s| !s.is_empty()),
            "A saved Wi-Fi password is unavailable. Unlock the live wallet or save the connection for all users, then retry; alternatively disable Wi-Fi transfer."
        );
    }
    let raw = unsafe {
        nm_keyfile_read(
            key.to_glib_none().0,
            c"/".as_ptr(),
            0,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    ensure!(!raw.is_null(), "Invalid NetworkManager Wi-Fi profile");
    let connection: Object = unsafe { from_glib_full(raw) };
    ensure!(
        unsafe { nm_connection_verify(connection.to_glib_none().0, ptr::null_mut()) } != 0,
        "Wi-Fi profile validation failed"
    );
    ensure!(
        unsafe { nm_connection_need_secrets(connection.to_glib_none().0, ptr::null_mut()) }
            .is_null(),
        "A Wi-Fi profile is missing credentials. Unlock the live wallet, save its password or disable transfer before continuing."
    );
    let normalized = serialize(&connection)?;
    let uuid = normalized
        .string("connection", "uuid")
        .map_err(|_| anyhow::anyhow!("Missing Wi-Fi profile identity"))?
        .to_string();
    Ok(super::WifiProfile {
        uuid,
        keyfile: Zeroizing::new(normalized.to_data().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn normalize(profile: &str, username: &str) -> Result<Zeroizing<String>> {
        Ok(parse_profile(profile, &crate::Username::parse(username)?)?.keyfile)
    }
    const WPA: &str = "[connection]\nid=Synthetic WiFi\nuuid=135ea3d9-d456-44b1-ae42-1e7081f66666\ntype=wifi\npermissions=user:nixos:;\n[wifi]\nssid=Test\\sSSID\\\\special\nmode=infrastructure\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=WiFi-Synthetic-Only-123!\npsk-flags=1\n[ipv4]\nmethod=auto\n[ipv6]\nmethod=auto\n";
    #[test]
    fn secret_preserved_private_user_remapped_and_no_ethernet() {
        let out = normalize(WPA, "alice").unwrap();
        assert!(out.contains("psk=WiFi-Synthetic-Only-123!"));
        assert!(!out.contains("psk-flags=1"));
        assert!(out.contains("user:alice:;"));
        assert!(!out.contains("user:nixos"));
        let again = normalize(&out, "alice").unwrap();
        assert_eq!(*out, *again);
        assert!(normalize(&WPA.replace("type=wifi", "type=ethernet"), "alice").is_err());
    }
    #[test]
    fn missing_password_and_external_keys_fail_without_disclosure() {
        let missing = WPA.replace("psk=WiFi-Synthetic-Only-123!\n", "");
        assert!(normalize(&missing, "alice").is_err());
        let certificate = format!("{WPA}\n[802-1x]\nca-cert=/private/sensitive-name\n");
        let err = normalize(&certificate, "alice").err().unwrap().to_string();
        assert!(!err.contains("sensitive-name"));
        assert!(!err.contains("Synthetic"));
    }
    #[test]
    fn open_sae_and_duplicate_profiles() {
        let open = WPA.replace(
            "[wifi-security]\nkey-mgmt=wpa-psk\npsk=WiFi-Synthetic-Only-123!\npsk-flags=1\n",
            "",
        );
        assert!(normalize(&open, "alice").is_ok());
        assert!(normalize(&WPA.replace("wpa-psk", "sae"), "alice").is_ok());
        assert!(
            super::super::WifiTransfer::parse(
                true,
                vec![WPA.into(), WPA.into()],
                &crate::Username::parse("alice").unwrap()
            )
            .is_err()
        );
    }
    #[test]
    fn target_files_are_private_and_outside_flake() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let transfer = super::super::WifiTransfer::parse(
            true,
            vec![WPA.into()],
            &crate::Username::parse("alice").unwrap(),
        )
        .unwrap();
        transfer.write_to(root.path()).unwrap();
        let file = root
            .path()
            .join("etc/NetworkManager/system-connections/installer-wifi-0.nmconnection");
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!root.path().join("etc/nixos").exists());
        let expected = normalize(WPA, "alice").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), *expected);
        let (enabled, raw) = transfer.into_raw();
        let transfer = super::super::WifiTransfer::parse(
            enabled,
            raw,
            &crate::Username::parse("alice").unwrap(),
        )
        .unwrap();
        let second = tempfile::tempdir().unwrap();
        transfer.write_to(second.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(
                second
                    .path()
                    .join("etc/NetworkManager/system-connections/installer-wifi-0.nmconnection")
            )
            .unwrap(),
            *expected
        );
        assert!(transfer.write_to(root.path()).is_err());
    }

    #[test]
    fn duplicate_detection_uses_parsed_connection_identity() {
        let user = crate::Username::parse("alice").unwrap();
        // Different display names still identify the same connection.
        let other = WPA.replace("id=Synthetic WiFi", "id=Renamed connection");
        let error = super::super::WifiTransfer::parse(true, vec![WPA.into(), other], &user)
            .err()
            .unwrap()
            .to_string();
        assert_eq!(error, "Duplicate Wi-Fi profile identity");
    }
}
