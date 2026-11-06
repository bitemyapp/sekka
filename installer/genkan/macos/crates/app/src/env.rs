// SPDX-License-Identifier: GPL-3.0-or-later
//! What the app needs from the world, behind one trait: the live Mac
//! (`mac::live`), or, in development builds, a simulation that touches no
//! disk, helper, network or account (`sim`).
use std::sync::atomic::AtomicBool;

use sekka_mac_contract::{
    ArtifactRole, Begin, HelperRefusal, InstallState, Inspection, Progress, Record, Release,
    Summary, Uuid,
};
use sekka_mac_stage::StagedFile;
use sekka_mac_trust::Verified;
use zeroize::Zeroizing;

pub type Secret = Zeroizing<Vec<u8>>;

/// A downloaded file and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub role: ArtifactRole,
    pub file: StagedFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    pub role: ArtifactRole,
    /// Bytes on disk across every file of the release.
    pub done: u64,
    pub total: u64,
    /// Why the current part is being tried again, if it is.
    pub retrying: Option<String>,
    pub verifying: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registration {
    /// The helper runs and answers.
    Ready,
    /// macOS needs the owner to allow it in System Settings › Login Items.
    NeedsApproval,
    /// Not registered yet.
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Events {
    pub records: Vec<Record>,
    pub state: InstallState,
    pub running: bool,
    pub progress: Option<Progress>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelperProblem {
    /// Not installed or not allowed yet: no installation can be known.
    Unavailable,
    /// The helper answered with a refusal or something unreadable.
    Other(String),
}

pub trait Environment: Send + Sync + 'static {
    /// The account running the app: short name and full name.
    fn user(&self) -> (String, String);
    /// Inspect this Mac with the bundled engine, without changing anything.
    fn inspect(&self) -> Result<Inspection, String>;
    /// Fetch and verify this build's release catalog.
    fn catalog(&self) -> Result<Verified, String>;
    fn helper_status(&self) -> Result<Vec<Summary>, HelperProblem>;
    /// Bytes of the release still to download (zero when all are staged and
    /// verified).
    fn still_to_download(&self, release: &Release) -> u64;
    /// Download and verify the release's files. Blocks; reports progress.
    fn download(
        &self,
        release: &Release,
        cancel: &AtomicBool,
        on: &mut dyn FnMut(DownloadProgress),
    ) -> Result<Vec<Staged>, String>;
    fn registration(&self) -> Registration;
    fn register_helper(&self) -> Result<Registration, String>;
    fn open_login_items(&self);
    fn begin(&self, begin: Begin, password: Secret) -> Result<Uuid, HelperRefusal>;
    fn events(&self, installation: Uuid, after: u64) -> Result<Events, String>;
    fn cancel(&self, installation: Uuid) -> Result<(), HelperRefusal>;
    fn retry_startup_disk(&self, installation: Uuid, password: Secret) -> Result<Events, HelperRefusal>;
    fn diagnostics(&self, installation: Uuid) -> Vec<String>;
    /// Delete the app's downloads once the helper has its own copies.
    fn remove_staging(&self);
    fn copy_to_pasteboard(&self, text: &str);
    fn shut_down(&self) -> Result<(), String>;
}
