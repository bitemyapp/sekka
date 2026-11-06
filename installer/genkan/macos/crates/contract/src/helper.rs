// SPDX-License-Identifier: GPL-3.0-or-later
//! The app–helper protocol: one request, one response, over the helper's
//! authenticated XPC connection.
//!
//! The request is canonical JSON in the XPC message's `request` field. The
//! owner's password, when an operation needs one, is a separate field
//! (`owner-password`, raw bytes) of the same XPC message: it is never part of
//! a document, never logged, never journaled, and the helper wipes its copy
//! once the operation that needed it is over. The helper re-derives
//! everything it acts on (the plan, the release, the downloads' digests)
//! rather than trusting what the request says about them.
use serde::{Deserialize, Serialize};

use crate::{
    Confirmation, Digest, InstallState, PlanRequest, Record, SignedCatalog, Text, Uuid, Version,
    schema_tag,
};

schema_tag!(HelperRequestSchema, "sekka.mac.helper-request/1");
schema_tag!(HelperResponseSchema, "sekka.mac.helper-response/1");

pub const HELPER_PROTOCOL: u32 = 1;
/// The helper's launchd label and XPC service name.
pub const HELPER_SERVICE: &str = "computer.sekka.installer.helper";
/// The app's bundle identifier.
pub const APP_IDENTIFIER: &str = "computer.sekka.installer";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperRequest {
    pub schema: HelperRequestSchema,
    pub id: u64,
    pub request: Request,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    Hello { app: Version, protocol: u32 },
    /// Any installation the helper knows about, and its state.
    Status,
    /// Start installing. Needs `owner-password`.
    Begin(Box<Begin>),
    /// Withdraw an installation that hasn't changed the disk.
    Cancel { installation: Uuid },
    /// Records after `after`, and the state they leave the installation in.
    Events { installation: Uuid, after: u64 },
    /// Verify the target again and retry only the startup-disk step. Needs
    /// `owner-password`.
    RetryStartupDisk { installation: Uuid },
    /// A redacted report of an installation for support.
    Diagnostics { installation: Uuid },
    /// Look at the Mac as root and bring a stopped installation's journal up
    /// to date with it: record Recovery's steps once the boot policy shows
    /// they took effect, or that nothing of the installation is left. The
    /// helper inspects for itself; the app asks only when its own inspection
    /// disagrees with the journal. Answered with `Events` (all of the
    /// installation's records), changed or not, or refused while its engine
    /// runs or when the Mac couldn't be inspected.
    Reconcile { installation: Uuid },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Begin {
    /// The app's version, recorded in the journal.
    pub app: Version,
    /// The catalog the plan was made from, as received; the helper verifies
    /// it against its own trust roots and accepted serial.
    pub catalog: SignedCatalog,
    pub request: PlanRequest,
    /// The plan the owner reviewed, by digest. The helper derives its own and
    /// compares.
    pub plan: Digest,
    pub confirmation: Confirmation,
    /// Where the app left the verified downloads.
    pub staged: Vec<StagedFile>,
    /// The macOS account whose password accompanies the request: its short
    /// name, which must be an administrator and an owner of this Mac.
    pub owner: Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedFile {
    pub role: ArtifactRole,
    /// An absolute path inside the app's staging directory. The helper
    /// opens it without following links and copies it into its own.
    pub path: Text,
    pub size: u64,
    pub digest: Digest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactRole {
    Payload,
    InstallerData,
    Manifest,
    RecoveryApp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperResponse {
    pub schema: HelperResponseSchema,
    pub id: u64,
    pub response: Response,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Response {
    Hello { helper: Version, protocol: u32 },
    Status { installations: Vec<Summary> },
    Begun { installation: Uuid },
    Events {
        records: Vec<Record>,
        state: InstallState,
        /// The engine is still running.
        running: bool,
        /// The current stage's progress, when the engine reports it.
        progress: Option<Progress>,
    },
    Cancelled,
    Diagnostics { lines: Vec<Text> },
    Refused(HelperRefusal),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub installation: Uuid,
    pub release: crate::Slug,
    pub state: InstallState,
    pub opened: crate::Timestamp,
    pub running: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub stage: crate::Stage,
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperRefusal {
    pub code: HelperRefusalCode,
    pub message: Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HelperRefusalCode {
    /// The app and helper don't speak the same protocol.
    Protocol,
    /// The request didn't parse or was too large.
    Malformed,
    /// Another installation is running or needs reconciling.
    Busy,
    /// The catalog failed verification, or is older than one accepted.
    Catalog,
    /// The helper's own inspection or plan differs from the reviewed one.
    PlanChanged,
    /// A download differs from the catalog's size or digest.
    Download,
    /// The account isn't an administrator, or the password was wrong.
    Authorization,
    /// The installation's state doesn't allow this.
    State,
    Unknown,
    Internal,
}
