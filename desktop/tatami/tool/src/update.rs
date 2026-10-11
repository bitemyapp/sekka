// SPDX-License-Identifier: MIT OR Apache-2.0
//! A receipt for what this session loaded. Shared JSON protocol with Yukimi;
//! neither component needs to depend on the other's Cargo workspace.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    #[serde(default)]
    boot_id: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    session_invocation: String,
    session: BTreeMap<String, String>,
    reload: BTreeMap<String, String>,
    keep_awake: String,
    #[serde(default)]
    error: Option<String>,
}

fn path() -> Result<PathBuf, String> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|p| PathBuf::from(p).join("tatami/update-session.json"))
        .ok_or_else(|| "No session runtime directory".into())
}
fn desired() -> Result<Manifest, String> {
    let text = std::fs::read_to_string("/etc/xdg/tatami/update.json").map_err(|e| e.to_string())?;
    let mut manifest: Manifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if manifest.version != 1 {
        return Err("Unsupported update manifest".into());
    }
    manifest.boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|e| e.to_string())?
        .trim()
        .into();
    manifest.session_id = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    Ok(manifest)
}
fn save(manifest: &Manifest) -> Result<(), String> {
    let path = path()?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("new");
    std::fs::write(
        &temporary,
        serde_json::to_vec(manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}
pub fn remember() -> Result<(), String> {
    let mut manifest = desired()?;
    manifest.session_invocation = std::env::var("INVOCATION_ID").unwrap_or_default();
    save(&manifest)
}

pub fn awake_stopped() {
    let result = (|| -> Result<(), String> {
        let mut old: Manifest =
            serde_json::from_slice(&std::fs::read(path()?).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        old.keep_awake = desired()?.keep_awake;
        save(&old)
    })();
    if let Err(error) = result {
        eprintln!("tatami: could not record Keep Awake update: {error}");
    }
}

pub fn apply() -> Result<(), String> {
    let mut old: Manifest =
        serde_json::from_slice(&std::fs::read(path()?).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let new = desired()?;
    if old.version != 1 || old.boot_id != new.boot_id || old.session_id != new.session_id {
        return Err("Cannot verify this session's loaded configuration".into());
    }
    if old.reload != new.reload {
        let hypr_changed = old.reload.get("hyprland") != new.reload.get("hyprland");
        let pinned = match crate::reload_parts(hypr_changed) {
            Ok(pinned) => pinned,
            Err(error) => {
                old.error = Some(error.clone());
                save(&old)?;
                return Err(error);
            }
        };
        let hypr = old.reload.get("hyprland").cloned();
        old.reload = new.reload;
        // A skipped reload is not success. Retain the old fingerprint so
        // Yukimi asks for exactly one sign-out, unless reboot supersedes it.
        if pinned {
            if let Some(hypr) = hypr {
                old.reload.insert("hyprland".into(), hypr);
            } else {
                old.reload.remove("hyprland");
            }
        }
    }
    old.error = None;
    if !crate::util::run(
        "systemctl",
        &[
            "--user",
            "is-active",
            "--quiet",
            "tatami-keep-awake.service",
        ],
    ) {
        old.keep_awake = new.keep_awake;
    }
    // Never replace the running compositor or an active sleep inhibitor here.
    // Their session fingerprints advance only at the next login.
    save(&old)
}
