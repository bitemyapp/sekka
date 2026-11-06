// SPDX-License-Identifier: GPL-3.0-or-later
//! What the helper needs from macOS, behind one trait so the rest of it is
//! tested anywhere.
use std::path::{Path, PathBuf};

use sekka_mac_contract::{Digest, Inspection, Timestamp, Uuid};

use zeroize::Zeroizing;

use crate::engine::EngineProcess;

/// A password, wiped when dropped.
pub type Secret = Zeroizing<Vec<u8>>;

pub use sekka_mac_engine::EngineInstall;

/// Which of the engine's changing modes to run.
#[derive(Clone, Copy, Debug)]
pub enum EngineRun<'a> {
    /// The whole installation, from the helper's imported assets.
    Execute { assets: &'a Path },
    /// Only the startup-disk stage again.
    StartupDisk,
}

/// The longest password handed to the engine, as it reads it.
pub const PASSWORD_LIMIT: usize = 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlatformError {
    #[error("{0}")]
    Authorization(String),
    #[error("{0}")]
    Engine(String),
    #[error("{0}")]
    Inspection(String),
    #[error("{0}")]
    Other(String),
}

pub trait Platform: Send + Sync + 'static {
    fn now(&self) -> Timestamp;

    /// The engine whose archive has `expected` as its digest: the one
    /// unpacked into the helper's own cache earlier, whatever this copy of
    /// the app carries now (an installation keeps the engine that made it
    /// across app updates), or else the bundled archive, unpacked, if it is
    /// that one.
    fn engine(&self, expected: &Digest) -> Result<EngineInstall, PlatformError>;

    /// Run the engine's `inspect` as root and return what it found, already
    /// checked against the transcript protocol.
    fn inspect(&self, engine: &EngineInstall) -> Result<Inspection, PlatformError>;

    /// Whether `user` is an administrator and an owner of this Mac, and
    /// `password` is that account's password.
    fn authenticate_owner(&self, user: &str, password: &[u8]) -> Result<(), PlatformError>;

    /// The app's staging folder for the person with this uid.
    fn user_staging(&self, uid: u32) -> Result<PathBuf, PlatformError>;

    /// Start the engine for the plan file, with the owner's credentials
    /// (handed to it privately): `execute` with the imported assets, or
    /// `startup-disk` to set the startup disk again.
    fn start_engine(
        &self,
        engine: &EngineInstall,
        run: EngineRun<'_>,
        installation: Uuid,
        plan: &Path,
        owner: &str,
        password: Secret,
    ) -> Result<Box<dyn EngineProcess>, PlatformError>;
}
