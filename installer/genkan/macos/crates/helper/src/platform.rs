// SPDX-License-Identifier: GPL-3.0-or-later
//! What the helper needs from macOS, behind one trait so the rest of it is
//! tested anywhere.
use std::path::{Path, PathBuf};

use sekka_mac_contract::{Digest, Fact, Failure, Inspection, Timestamp};

use zeroize::Zeroizing;

use crate::engine::EngineProcess;

/// A password, wiped when dropped.
pub type Secret = Zeroizing<Vec<u8>>;

pub use sekka_mac_engine::EngineInstall;

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

    /// Unpack (or find) the bundled engine, refusing it unless its archive
    /// has `expected` as its digest.
    fn engine(&self, expected: &Digest) -> Result<EngineInstall, PlatformError>;

    /// Run the engine's `inspect` as root and return what it found, already
    /// checked against the transcript protocol.
    fn inspect(&self, engine: &EngineInstall) -> Result<Inspection, PlatformError>;

    /// Whether `user` is an administrator and an owner of this Mac, and
    /// `password` is that account's password.
    fn authenticate_owner(&self, user: &str, password: &[u8]) -> Result<(), PlatformError>;

    /// The app's staging folder for the person with this uid.
    fn user_staging(&self, uid: u32) -> Result<PathBuf, PlatformError>;

    /// Start the engine's `execute` for the plan file, with the imported
    /// assets and the owner's credentials (handed to it privately).
    fn start_engine(
        &self,
        engine: &EngineInstall,
        plan: &Path,
        assets: &Path,
        owner: &str,
        password: Secret,
    ) -> Result<Box<dyn EngineProcess>, PlatformError>;

    /// Set the new system as the startup disk again, using the stage
    /// facts the journal recorded, and read the selection back.
    fn set_startup_disk(
        &self,
        facts: &[Fact],
        owner: &str,
        password: &[u8],
    ) -> Result<Vec<Fact>, Failure>;
}
