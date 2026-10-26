// SPDX-License-Identifier: GPL-3.0-or-later
//! LUKS2 containers. Secrets are passed on stdin, never saved in the target.
use crate::{
    filesystem,
    process::{output, output_secret},
};
use anyhow::{Context, Result, ensure};
use std::path::Path;

/// Explicit payload offset, also reserved by disk::Layout (512-byte sectors).
pub const HEADER_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identities {
    pub root: String,
    pub swap: Option<String>,
}

pub fn validate_passphrase(value: &str) -> Result<()> {
    ensure!(
        value.chars().count() >= 12 && value.len() <= 1024 && !value.chars().any(char::is_control),
        "Disk passphrase must have at least 12 characters, at most 1024 bytes, and no control characters"
    );
    Ok(())
}

pub(crate) fn preflight() -> Result<()> {
    output("cryptsetup", &["--version"], 15)
        .context("The installation media is missing cryptsetup; no disk writes were made")?;
    output("modprobe", &["dm-crypt"], 120)
        .context("Could not load disk encryption support; no disk writes were made")?;
    Ok(())
}

/// Declare before the mount guard so unwinding unmounts before closing mappings.
/// Only names created by this installation are eligible for cleanup.
#[derive(Default)]
pub(crate) struct Mappings(Vec<String>);
impl Mappings {
    pub(crate) fn format_open(
        &mut self,
        device: &str,
        uuid: &str,
        passphrase: &str,
    ) -> Result<String> {
        filesystem::check_uuid(uuid, "crypto_LUKS")?;
        validate_passphrase(passphrase)?;
        let name = format!("genkan-{uuid}");
        let path = format!("/dev/mapper/{name}");
        ensure!(
            !Path::new(&path).exists(),
            "Encryption mapping already exists: {name}"
        );
        output("wipefs", &["--all", "--force", device], 60)?;
        output_secret(
            "cryptsetup",
            &[
                "luksFormat",
                "--batch-mode",
                "--type",
                "luks2",
                "--pbkdf",
                "argon2id",
                "--offset",
                "32768",
                "--uuid",
                uuid,
                "--key-file",
                "-",
                device,
            ],
            passphrase.as_bytes(),
            300,
        )?;
        ensure!(
            filesystem::probe_uuid(device, "crypto_LUKS")? == uuid,
            "LUKS identity does not match the prepared configuration"
        );
        // Register before spawning: a timed-out open can still have made a mapping.
        self.0.push(name.clone());
        output_secret(
            "cryptsetup",
            &[
                "open",
                "--type",
                "luks2",
                "--allow-discards",
                "--key-file",
                "-",
                device,
                &name,
            ],
            passphrase.as_bytes(),
            120,
        )?;
        Ok(path)
    }
    pub(crate) fn paths(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|name| format!("/dev/mapper/{name}"))
            .collect()
    }
    pub(crate) fn close(&mut self) -> Result<()> {
        while let Some(name) = self.0.last() {
            output("cryptsetup", &["close", name], 60)?;
            self.0.pop();
        }
        Ok(())
    }
}
impl Drop for Mappings {
    fn drop(&mut self) {
        // Never force removal of a mapping that is still mounted.
        for name in self.0.iter().rev() {
            let _ = output("cryptsetup", &["close", name], 60);
        }
    }
}
