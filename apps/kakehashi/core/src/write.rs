//! The write as the window asks for it and the helper reports it: the
//! [`Plan`] the window writes to `plan.json`, and the [`Progress`] lines the
//! helper appends to its progress file.
//!
//! The write itself is the helper's, in `kakehashi-write`, which only the
//! helper depends on: the window can't read a request for any drive and
//! write it in its own process. Not from the core:
//!
//! ```compile_fail,E0432
//! use kakehashi_core::write::{Requested, execute};
//! ```
//!
//! nor from the write crate, which isn't among the window's dependencies:
//!
//! ```compile_fail,E0432
//! use kakehashi_write::{Requested, execute};
//! ```
//!
//! What the window has, the plan of a confirmed write:
//!
//! ```
//! use kakehashi_core::choice::ConfirmedWrite;
//! use kakehashi_core::write::Plan;
//! fn plan(write: ConfirmedWrite) -> String {
//!     serde_json::to_string(&Plan::new(write)).unwrap()
//! }
//! ```

use crate::choice::ConfirmedWrite;
use crate::drive::Drive;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Cleared at the end of the drive: a previous GPT's backup header lives in
/// the last sectors, and would confuse firmware.
pub const TAIL: u64 = 1024 * 1024;

/// What the window asks the helper to do, as the window writes it to
/// `plan.json`. The helper reads the same JSON as a `kakehashi_write::Requested`.
///
/// The window makes one only from the drive and image the open dialog
/// described, found again when OK was pressed ([`ConfirmedWrite`]).
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Plan {
    image: PathBuf,
    /// The image's SHA-256 as the window computed it: what is written and
    /// what is read back must both have it.
    sha256: String,
    drive: Drive,
}

impl Plan {
    /// The write the person confirmed: that image onto that drive.
    ///
    /// A plan for the drive chosen in the list as it is now, rather than
    /// the one the confirmation dialog named, doesn't compile (KTL-04): the
    /// list keeps refreshing under the dialog, and the next stick plugged
    /// in takes the same name.
    ///
    /// ```compile_fail,E0308
    /// use kakehashi_core::{Drive, choice::DriveList, write::Plan};
    /// fn ok_pressed(list: &DriveList) -> Option<Plan> {
    ///     let chosen_now: Drive = list.chosen(None)?.clone();
    ///     Some(Plan::new(chosen_now))
    /// }
    /// ```
    ///
    /// Nor does reading one from JSON naming any drive:
    ///
    /// ```compile_fail,E0277
    /// use kakehashi_core::{Drive, write::Plan};
    /// fn ok_pressed(chosen_now: &Drive) -> Plan {
    ///     let json = serde_json::json!({ "image": "sekka.iso", "sha256": "0", "drive": chosen_now });
    ///     serde_json::from_value::<Plan>(json).unwrap()
    /// }
    /// ```
    ///
    /// The open dialog's drive and image, found again when OK is pressed:
    ///
    /// ```
    /// use kakehashi_core::choice::{DriveList, ImageId, ReviewTicket};
    /// use kakehashi_core::write::Plan;
    /// fn ok_pressed(list: &mut DriveList, review: ReviewTicket, image: ImageId) -> Option<Plan> {
    ///     Some(Plan::new(list.confirm(review, None, Some(image)).ok()?))
    /// }
    /// ```
    pub fn new(write: ConfirmedWrite) -> Plan {
        let (drive, image) = write.into_target().into_parts();
        Plan { image: image.path, sha256: image.sha256, drive }
    }

    /// The same write from a copy of the image (on macOS, one the helper
    /// may read). The checksum stays: the copy must have it.
    pub fn with_image(self, image: PathBuf) -> Plan {
        Plan { image, ..self }
    }

    pub fn image(&self) -> &Path {
        &self.image
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// The drive as the person confirmed it.
    pub fn drive(&self) -> &Drive {
        &self.drive
    }
}

/// What the helper reports, one JSON object per line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Progress {
    Preparing,
    Writing {
        done: u64,
        total: u64,
    },
    Verifying {
        done: u64,
        total: u64,
    },
    Finished {
        ejected: bool,
        note: Option<String>,
    },
    /// `touched`: the drive may have been changed before it stopped.
    Failed {
        message: String,
        touched: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_one_json_object_per_line() {
        let line = serde_json::to_string(&Progress::Writing { done: 1, total: 2 }).unwrap();
        assert_eq!(line, r#"{"stage":"writing","done":1,"total":2}"#);
        let back: Progress = serde_json::from_str(&line).unwrap();
        assert_eq!(back, Progress::Writing { done: 1, total: 2 });
    }
}
