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
//! What the window has, the plan of a confirmed write and the press that
//! confirmed it:
//!
//! ```
//! use kakehashi_core::choice::{ConfirmedWrite, Target};
//! use kakehashi_core::write::Plan;
//! fn plan<P: AsRef<Target>>(write: ConfirmedWrite, pressed: P) -> Option<String> {
//!     serde_json::to_string(&Plan::new(write, pressed).ok()?).ok()
//! }
//! ```

use crate::choice::{ConfirmedWrite, Target, Unconfirmed};
use crate::drive::Drive;
use aizome_typestate::{Changed, SameTarget};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Cleared at the end of the drive: a previous GPT's backup header lives in
/// the last sectors, and would confuse firmware.
pub const TAIL: u64 = 1024 * 1024;

/// What the window asks the helper to do, as the window writes it to
/// `plan.json`. The helper reads the same JSON as a `kakehashi_write::Requested`.
///
/// The window makes one only from the drive and image the open dialog
/// described, found again when OK was pressed ([`ConfirmedWrite`]), and the
/// press itself, `P`: only the window's dialog makes its press type
/// (`kakehashi_window::dialog::Pressed`), and only a plan with that press
/// starts the helper (`kakehashi_window::elevate`). This crate can't name
/// that type, so it takes any; a plan made with another (a drive list run
/// by code of its own) is one the helper is never started for. The press
/// isn't written to `plan.json`.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Plan<P> {
    image: PathBuf,
    /// The image's SHA-256 as the window computed it: what is written and
    /// what is read back must both have it.
    sha256: String,
    drive: Drive,
    #[serde(skip)]
    pressed: P,
}

impl<P: AsRef<Target>> Plan<P> {
    /// The write the person confirmed: that image onto that drive, with
    /// their press of OK, which must be of the dialog that described it (the
    /// same drive and image). Refused, writing nothing, for a press of
    /// another dialog's.
    ///
    /// A plan for the drive chosen in the list as it is now, rather than
    /// the one the confirmation dialog named, doesn't compile (KTL-04): the
    /// list keeps refreshing under the dialog, and the next stick plugged
    /// in takes the same name.
    ///
    /// ```compile_fail,E0308
    /// use kakehashi_core::{Drive, choice::{DriveList, Target}, write::Plan};
    /// fn ok_pressed<P: AsRef<Target>>(list: &DriveList, pressed: P) -> Option<Plan<P>> {
    ///     let chosen_now: Drive = list.chosen(None)?.clone();
    ///     Plan::new(chosen_now, pressed).ok()
    /// }
    /// ```
    ///
    /// Nor does reading one from JSON naming any drive:
    ///
    /// ```compile_fail,E0277
    /// use kakehashi_core::{Drive, write::Plan};
    /// fn ok_pressed(chosen_now: &Drive) -> Plan<()> {
    ///     let json = serde_json::json!({ "image": "sekka.iso", "sha256": "0", "drive": chosen_now });
    ///     serde_json::from_value::<Plan<()>>(json).unwrap()
    /// }
    /// ```
    ///
    /// Nor one without the press, as once it could be:
    ///
    /// ```compile_fail,E0061
    /// use kakehashi_core::choice::{ConfirmedWrite, Target};
    /// use kakehashi_core::write::Plan;
    /// fn unpressed(write: ConfirmedWrite) -> Option<Plan<Target>> {
    ///     Plan::new(write).ok()
    /// }
    /// ```
    ///
    /// A press makes one plan: it's spent by the plan it makes.
    ///
    /// ```compile_fail,E0382
    /// use kakehashi_core::choice::{ConfirmedWrite, Target};
    /// use kakehashi_core::write::Plan;
    /// fn twice(a: ConfirmedWrite, b: ConfirmedWrite, pressed: Target) -> Option<(Plan<Target>, Plan<Target>)> {
    ///     Some((Plan::new(a, pressed).ok()?, Plan::new(b, pressed).ok()?))
    /// }
    /// ```
    ///
    /// The open dialog's drive and image, found again when OK is pressed:
    ///
    /// ```
    /// use kakehashi_core::choice::{DriveList, ImageId, ReviewTicket, Target};
    /// use kakehashi_core::write::Plan;
    /// fn ok_pressed<P: AsRef<Target>>(list: &mut DriveList, review: ReviewTicket, image: ImageId, pressed: P) -> Option<Plan<P>> {
    ///     Plan::new(list.confirm(review, None, Some(image)).ok()?, pressed).ok()
    /// }
    /// ```
    pub fn new(write: ConfirmedWrite, pressed: P) -> Result<Plan<P>, Unconfirmed> {
        if !pressed.as_ref().same_target(write.target()) {
            let changed = Changed { reviewed: pressed.as_ref().clone(), found: write.into_target() };
            return Err(Unconfirmed::Changed(Box::new(changed)));
        }
        let (drive, image) = write.into_target().into_parts();
        Ok(Plan { image: image.path, sha256: image.sha256, drive, pressed })
    }
}

impl<P> Plan<P> {
    /// The same write from a copy of the image (on macOS, one the helper
    /// may read). The checksum stays: the copy must have it.
    pub fn with_image(self, image: PathBuf) -> Plan<P> {
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

    /// The press that confirmed it.
    pub fn pressed(&self) -> &P {
        &self.pressed
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

    /// A press of one dialog's OK doesn't make a plan of another drive: a
    /// list of the code's own, reviewed and confirmed, gets nothing from it.
    #[test]
    fn a_press_makes_a_plan_only_of_what_its_dialog_described() {
        use crate::choice::{DriveList, DriveListing, ImageId};
        let stick = |serial: &str| Drive {
            name: "sdb".into(),
            bytes: 32_000_000_000,
            serial: serial.into(),
            usb_or_removable: true,
            ..Drive::default()
        };
        let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
        let reviewed = |drive: Drive| {
            let mut list = DriveList::new();
            let listing = list.request(std::time::Instant::now()).unwrap();
            list.listed(DriveListing::new(listing, Ok(vec![drive.clone()])));
            list.choose(&drive);
            let (review, shown) = list.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).unwrap();
            (list.confirm(review, None, Some(image.clone())).unwrap(), shown)
        };
        let (_, pressed_for_a) = reviewed(stick("A"));
        let (write_b, _) = reviewed(stick("B"));
        assert!(matches!(Plan::new(write_b, pressed_for_a), Err(Unconfirmed::Changed(_))));
        let (write_a, pressed_for_a) = reviewed(stick("A"));
        assert_eq!(Plan::new(write_a, pressed_for_a).unwrap().drive().serial, "A");
    }
}
