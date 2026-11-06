// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the helper keeps its state: one root-owned directory, mode 0700.
use std::path::{Path, PathBuf};

use sekka_mac_contract::{Digest, Uuid};

#[derive(Clone, Debug)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// The installed helper's state: survives app updates, the app being
    /// deleted, and helper replacement, so an unfinished installation's
    /// record is never lost.
    pub const SYSTEM: &'static str = "/var/db/computer.sekka.installer";

    pub fn system() -> Self {
        Self::at(Path::new(Self::SYSTEM))
    }
    pub fn at(root: &Path) -> Self {
        Self { root: root.into() }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Each installation's journal, by installation identity.
    pub fn journals(&self) -> PathBuf {
        self.root.join("installations")
    }
    /// Every installation's staging, by installation identity.
    pub fn all_staging(&self) -> PathBuf {
        self.root.join("staging")
    }
    /// The imported downloads and the plan for one installation. The
    /// downloads are removed once its run is over; the plan stays.
    pub fn staging(&self, installation: Uuid) -> PathBuf {
        self.all_staging().join(installation.to_string())
    }
    pub fn accepted(&self) -> PathBuf {
        self.root.join("accepted-catalogs.json")
    }
    /// Held while an installation runs.
    pub fn lock(&self) -> PathBuf {
        self.root.join("installing.lock")
    }
    /// The engine unpacked from the archive with this digest.
    pub fn engine(&self, digest: &Digest) -> PathBuf {
        self.root.join("engine").join(digest.hex())
    }
}
