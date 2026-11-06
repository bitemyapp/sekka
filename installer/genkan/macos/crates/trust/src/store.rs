// SPDX-License-Identifier: GPL-3.0-or-later
//! The highest catalog accepted on each channel, kept so an older catalog
//! can't be replayed.
//!
//! The app keeps one in its own Application Support folder and the helper
//! keeps the authoritative one in its root-owned state directory, which the
//! person running the app cannot reset. A missing store is a first use; a
//! damaged one is reported, never quietly replaced by an empty one (that
//! would undo the rollback protection it exists for).
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sekka_mac_contract::{Digest, Timestamp, canonical, schema_tag};

use crate::{TrustError, Verified};

schema_tag!(AcceptedSchema, "sekka.mac.accepted/1");

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: AcceptedSchema,
    channels: BTreeMap<String, Entry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    serial: u64,
    digest: Digest,
    accepted: Timestamp,
}

pub struct AcceptedStore {
    path: PathBuf,
    document: Document,
}

impl AcceptedStore {
    pub fn open(path: &Path) -> Result<Self, TrustError> {
        let document = match fs::read(path) {
            Ok(bytes) => canonical::from_slice(&bytes, 64 * 1024, AcceptedSchema::VALUE)
                .map_err(|e| TrustError::StoreDamaged(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Document::default(),
            Err(e) => return Err(TrustError::StoreDamaged(format!("{}: {e}", path.display()))),
        };
        Ok(Self {
            path: path.into(),
            document,
        })
    }

    /// The serial accepted on a channel, if any.
    pub fn accepted(&self, channel: &str) -> Option<u64> {
        self.document.channels.get(channel).map(|e| e.serial)
    }

    /// Check a verified catalog against what was accepted before, without
    /// recording it.
    pub fn check(&self, verified: &Verified) -> Result<(), TrustError> {
        let c = verified.catalog();
        match self.document.channels.get(c.channel.as_str()) {
            Some(e) if c.serial < e.serial => Err(TrustError::Rollback {
                offered: c.serial,
                accepted: e.serial,
            }),
            Some(e) if c.serial == e.serial && e.digest != verified.digest() => {
                Err(TrustError::Conflict(c.serial))
            }
            _ => Ok(()),
        }
    }

    /// Check, then record the catalog as the channel's newest, durably.
    pub fn accept(&mut self, verified: &Verified, now: Timestamp) -> Result<(), TrustError> {
        self.check(verified)?;
        let c = verified.catalog();
        let entry = Entry {
            serial: c.serial,
            digest: verified.digest(),
            accepted: now,
        };
        if self.document.channels.get(c.channel.as_str()).map(|e| (e.serial, e.digest))
            == Some((entry.serial, entry.digest))
        {
            return Ok(());
        }
        let mut next = self.document.clone();
        next.channels.insert(c.channel.to_string(), entry);
        write_atomically(&self.path, &canonical::to_vec(&next).map_err(|e| TrustError::StoreDamaged(e.to_string()))?)
            .map_err(|e| TrustError::StoreDamaged(format!("{}: {e}", self.path.display())))?;
        self.document = next;
        Ok(())
    }
}

/// Replace a file so a reader sees the old bytes or the new ones, never a
/// mixture, and the new ones survive a power loss once this returns.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("store")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    fs::File::open(dir)?.sync_all()
}
