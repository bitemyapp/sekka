// SPDX-License-Identifier: GPL-3.0-or-later
//! Profiles without their secrets. The store keeps a copy of each saved
//! connection with its secrets taken out, in plaintext beside the image:
//! the same exposure as before the store, less the secrets. When the store
//! can't be opened, the copies go where NetworkManager reads, so saved
//! networks stay listed and ask for their password instead of vanishing.

use std::fs;
use std::io;
use std::path::Path;

use crate::files;
use crate::seed::Profile;

/// Properties NetworkManager marks secret (`NM_SETTING_PARAM_SECRET` in
/// libnm-core 1.58), by their keyfile names: Wi-Fi, 802.1X, mobile, PPPoE,
/// ADSL and MACsec. WireGuard's and VPNs' secrets are matched by group.
const SECRETS: &[&str] = &[
    "psk",
    "wep-key0",
    "wep-key1",
    "wep-key2",
    "wep-key3",
    "leap-password",
    "password",
    "password-raw",
    "pin",
    "ca-cert-password",
    "client-cert-password",
    "phase2-ca-cert-password",
    "phase2-client-cert-password",
    "private-key-password",
    "phase2-private-key-password",
    "initial-eps-bearer-password",
    "mka-cak",
];

/// Whether `key` in `[group]` holds a secret.
fn secret(group: &str, key: &str, value: &str) -> bool {
    match group {
        "vpn-secrets" => true,
        "wireguard" => key == "private-key" || SECRETS.contains(&key),
        g if g.starts_with("wireguard-peer.") => key == "preshared-key",
        // A key held in the profile itself rather than in a file beside it.
        "802-1x" if matches!(key, "private-key" | "phase2-private-key") => {
            value.starts_with("data:")
        }
        _ => SECRETS.contains(&key),
    }
}

/// `profile` (a keyfile) with every secret line taken out. Everything else,
/// the secrets' flags included, stays as it was.
pub fn strip(profile: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(profile.len());
    let mut group = String::new();
    for line in profile.split_inclusive(|&b| b == b'\n') {
        let text = String::from_utf8_lossy(line);
        let trimmed = text.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            group = name.to_owned();
            if group == "vpn-secrets" {
                continue;
            }
        } else if let Some((key, value)) = trimmed.split_once('=')
            && !trimmed.starts_with('#')
            && secret(&group, key.trim(), value.trim())
        {
            continue;
        } else if group == "vpn-secrets" {
            continue;
        }
        out.extend_from_slice(line);
    }
    out
}

/// Whether a file in the keyfile directory is a profile worth a secret-less
/// copy: not hidden or a backup, and not one of the certificates and keys
/// NetworkManager writes beside profiles.
pub fn is_profile(name: &str) -> bool {
    !name.starts_with('.')
        && !name.ends_with('~')
        && ![".pem", ".der", ".p12", ".pfx", ".crt", ".key"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

/// Bring the secret-less copies in `skeleton` in line with the profiles in
/// `connections` (the open store).
pub fn refresh(connections: &Path, skeleton: &Path) -> io::Result<()> {
    files::ensure_dir(skeleton, 0o700)?;
    let mut keep = Vec::new();
    for entry in fs::read_dir(connections)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_profile(&name) || !entry.file_type()?.is_file() {
            continue;
        }
        let stripped = strip(&fs::read(entry.path())?);
        let copy = skeleton.join(&name);
        if fs::read(&copy).ok().as_deref() != Some(stripped.as_slice()) {
            files::write_atomic(&copy, &stripped, 0o600)?;
        }
        keep.push(name);
    }
    for entry in fs::read_dir(skeleton)? {
        let entry = entry?;
        if !keep.contains(&entry.file_name().to_string_lossy().into_owned()) {
            fs::remove_file(entry.path())?;
        }
    }
    files::sync_dir(skeleton)
}

/// Secret-less copies of the profiles a store was just made with.
pub fn add(skeleton: &Path, profiles: &[Profile]) -> io::Result<()> {
    files::ensure_dir(skeleton, 0o700)?;
    for profile in profiles {
        files::write_atomic(&skeleton.join(&profile.name), &strip(&profile.bytes), 0o600)?;
    }
    Ok(())
}

/// Put the secret-less copies where NetworkManager reads (`keyfiles`, not
/// the store), except where a profile of that name already is. Returns how
/// many were put back.
pub fn restore(skeleton: &Path, keyfiles: &Path) -> io::Result<usize> {
    let Ok(entries) = fs::read_dir(skeleton) else {
        return Ok(0);
    };
    let mut restored = 0;
    for entry in entries {
        let entry = entry?;
        let target = keyfiles.join(entry.file_name());
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        files::write_atomic(&target, &fs::read(entry.path())?, 0o600)?;
        restored += 1;
    }
    if restored > 0 {
        files::sync_dir(keyfiles)?;
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WPA: &str = "[connection]\nid=Home\nuuid=1\ntype=wifi\n\n[wifi]\nssid=Home\n\n\
        [wifi-security]\nkey-mgmt=wpa-psk\npsk=correct horse\npsk-flags=0\n\n[ipv4]\nmethod=auto\n";

    #[test]
    fn a_wifi_password_goes_and_its_flags_stay() {
        let out = String::from_utf8(strip(WPA.as_bytes())).unwrap();
        assert!(!out.contains("correct horse"), "{out}");
        assert!(
            out.contains("psk-flags=0") && out.contains("key-mgmt=wpa-psk"),
            "{out}"
        );
        assert!(out.contains("[ipv4]\nmethod=auto\n"), "{out}");
    }

    #[test]
    fn enterprise_wireguard_and_vpn_secrets_go() {
        let profile = "[802-1x]\neap=peap;\nidentity=ann\npassword=pw\n\
            private-key=data:;base64,AAAA\nphase2-private-key=/etc/key.pem\n\
            [wireguard]\nprivate-key=wgkey\nlisten-port=51820\n\
            [wireguard-peer.abc=]\nendpoint=1.2.3.4:51820\npreshared-key=psk2\n\
            [vpn]\nservice-type=openvpn\npassword-flags=0\n\
            [vpn-secrets]\npassword=vpnpw\ncert-pass=x\n\
            [gsm]\napn=internet\npin=1234\n";
        let out = String::from_utf8(strip(profile.as_bytes())).unwrap();
        for secret in [
            "pw\n",
            "AAAA",
            "wgkey",
            "psk2",
            "vpnpw",
            "cert-pass",
            "1234",
            "[vpn-secrets]",
        ] {
            assert!(!out.contains(secret), "{secret} survived:\n{out}");
        }
        for kept in [
            "identity=ann",
            "phase2-private-key=/etc/key.pem",
            "listen-port=51820",
            "endpoint=1.2.3.4:51820",
            "password-flags=0",
            "apn=internet",
        ] {
            assert!(out.contains(kept), "{kept} went:\n{out}");
        }
    }

    #[test]
    fn a_profile_without_secrets_is_unchanged() {
        let open = "[connection]\nid=Cafe\n[wifi]\nssid=Cafe\n# psk=not a key\n";
        assert_eq!(strip(open.as_bytes()), open.as_bytes());
    }

    #[test]
    fn certificates_and_hidden_files_get_no_copy() {
        assert!(is_profile("Home.nmconnection") && is_profile("legacy-profile"));
        for name in [
            ".nmmeta-tmp",
            "Home.nmconnection~",
            "1-ca-cert.pem",
            "1-private-key.p12",
        ] {
            assert!(!is_profile(name), "{name}");
        }
    }
}
