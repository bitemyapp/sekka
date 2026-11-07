// SPDX-License-Identifier: GPL-3.0-or-later
//! What the app needs from the world, behind one trait: the live Mac
//! (`mac::live`), or, in development builds, a simulation that touches no
//! disk, helper, network or account (`sim`).
use std::sync::atomic::AtomicBool;

use sekka_mac_contract::{
    ArtifactRole, Begin, Digest, HelperRefusal, InstallState, Inspection, Progress, Record,
    Release, Summary, Uuid,
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

/// Why the helper's installations aren't known. Until they are, nothing is
/// planned: one of them may be unfinished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelperProblem {
    /// macOS has no helper registered for this app: nothing has been
    /// installed through it from here, unless the disk says otherwise.
    NotRegistered,
    /// Registered, but macOS won't run it until the owner allows it in
    /// System Settings › Login Items, where it may have been turned off.
    NeedsApproval,
    /// Allowed to run, and it didn't answer.
    Unreachable(String),
    /// It answered with a refusal (a record it can't read, say) or
    /// something unreadable.
    Other(String),
}

impl std::fmt::Display for HelperProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HelperProblem::NotRegistered => f.write_str("the helper isn't set up"),
            HelperProblem::NeedsApproval => f.write_str("the helper isn't allowed to run (Login Items)"),
            HelperProblem::Unreachable(e) => write!(f, "the helper couldn't be reached: {e}"),
            HelperProblem::Other(m) => write!(f, "the helper said: {m}"),
        }
    }
}

/// What is left to download of a release, given what an earlier run left
/// in staging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToDownload {
    /// Bytes still to fetch. A whole file of the release's size counts as
    /// fetched (it is checked byte for byte before the review), and a
    /// download in progress counts only for the digest it was started for.
    pub fetch: u64,
    /// The peak extra space fetching them takes: what staging removes
    /// first (another release's files) is room they get back.
    pub space: u64,
}

impl ToDownload {
    pub const NOTHING: Self = Self { fetch: 0, space: 0 };

    /// All of the release, when nothing of it is staged.
    pub fn all(release: &Release) -> Self {
        let bytes = release.download_bytes();
        Self {
            fetch: bytes,
            space: bytes,
        }
    }
}

pub trait Environment: Send + Sync + 'static {
    /// The account running the app: short name and full name.
    fn user(&self) -> (String, String);
    /// Inspect this Mac with the bundled engine, without changing anything.
    fn inspect(&self) -> Result<Inspection, String>;
    /// The digest of the engine archive this app carries. The helper installs
    /// a release only with the engine it was made with.
    fn bundled_engine(&self) -> Result<Digest, String>;
    /// Fetch and verify this build's release catalog.
    fn catalog(&self) -> Result<Verified, String>;
    /// The installations the helper keeps a record of. Asked only while
    /// macOS lets it run: see [`status`].
    fn helper_status(&self) -> Result<Vec<Summary>, HelperProblem>;
    /// What is left to download of the release.
    fn still_to_download(&self, release: &Release) -> ToDownload;
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
    /// Ask the helper to look at the Mac as root and bring a stopped
    /// installation's record up to date with it: Recovery's steps once the
    /// boot policy shows them, or that nothing of it is left. It may also
    /// find nothing to change, or refuse: busy while an engine holds the
    /// disk, or because the Mac couldn't be inspected.
    fn reconcile(&self, installation: Uuid) -> Result<Events, HelperRefusal>;
    /// An account's full name, as Users & Groups and recoveryOS show it,
    /// when this Mac knows the account.
    fn full_name(&self, account: &str) -> Option<String>;
    fn diagnostics(&self, installation: Uuid) -> Vec<String>;
    /// Delete the app's downloads once the helper has its own copies.
    fn remove_staging(&self);
    fn copy_to_pasteboard(&self, text: &str);
    fn shut_down(&self) -> Result<(), String>;
}

/// The helper's installations, asked for only while macOS lets it run;
/// otherwise why they can't be known.
pub fn status(env: &dyn Environment) -> Result<Vec<Summary>, HelperProblem> {
    match env.registration() {
        Registration::Ready => env.helper_status(),
        Registration::NeedsApproval => Err(HelperProblem::NeedsApproval),
        Registration::Absent => Err(HelperProblem::NotRegistered),
    }
}
