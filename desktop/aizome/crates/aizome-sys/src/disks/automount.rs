//! Mounting drives as they come: what a desktop does for USB sticks and
//! external disks, for a program that wants their files without anyone
//! opening them first (Chanoma, the television, as it starts).
//!
//! [`Automount`] mounts the filesystems UDisks says to mount by themselves
//! (`HintAuto`: on removable and external drives, never the system's own
//! nor those marked to ignore) the first time it sees each of them
//! unmounted, and only once ([`Disks::open`]: one mounted meanwhile by
//! someone else is opened too, where it is). One already mounted when
//! first seen is left alone, and so, when someone unmounts it later, it
//! stays unmounted; one unplugged and plugged in again is new. Locked
//! (encrypted) volumes wait for someone with the passphrase: their unlocked
//! device, if it appears unmounted, is new too.
//!
//! The mounts run on UDisks's threads. The [`Disks`] handle's wake calls
//! when its snapshot changes or one of them finishes, and
//! [`Automount::update`] then starts what's new and returns what finished.

use super::{Disks, DisksState, Volume};
use crate::service::Task;
use std::collections::HashSet;

/// What came of mounting a volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub volume: Volume,
    /// Its filesystem's label and UUID: UDisks names the mount point after
    /// the label (else the UUID), and the UUID knows the volume when it's
    /// plugged in another time.
    pub label: String,
    pub uuid: String,
    /// Where it's mounted, or why it couldn't be.
    pub result: Result<String, String>,
}

/// A volume being mounted.
#[derive(Clone, Debug)]
pub struct Opening {
    pub volume: Volume,
    pub label: String,
    pub uuid: String,
    task: Task<String>,
}

/// See the module's documentation.
#[derive(Debug, Default)]
pub struct Automount {
    /// The volumes seen, by block: each is looked at once.
    seen: HashSet<String>,
    opening: Vec<Opening>,
}

impl Automount {
    pub fn new() -> Automount {
        Automount::default()
    }

    /// The volumes to mount now: those first seen in `state`, unmounted.
    /// A block gone from it (unplugged) is forgotten; one whose
    /// filesystem only drops out of UDisks's view for a moment (as it
    /// probes a drive just plugged in, or as it's being mounted) isn't.
    pub fn plan(&mut self, state: &DisksState) -> Vec<Volume> {
        self.seen.retain(|b| state.block(b).is_some());
        let opening: HashSet<&str> = self.opening.iter().map(|o| o.volume.block.as_str()).collect();
        state
            .volumes()
            .into_iter()
            .filter(|v| !opening.contains(v.block.as_str()) && self.seen.insert(v.block.clone()))
            .filter(|v| state.mounts_by_itself(v))
            .collect()
    }

    /// Start mounting what's new in the disks' snapshot, and return the
    /// mounts that have finished since the last call.
    pub fn update(&mut self, disks: &Disks) -> Vec<Outcome> {
        let state = disks.snapshot();
        for volume in self.plan(&state) {
            let (label, uuid) =
                state.block(&volume.block).map(|b| (b.id_label.clone(), b.id_uuid.clone())).unwrap_or_default();
            log::info!("aizome-sys: mounting {} ({}) as it came", volume.name, volume.block);
            let task = disks.open(&volume.block);
            self.opening.push(Opening { volume, label, uuid, task });
        }
        let mut done = Vec::new();
        let mut i = 0;
        while i < self.opening.len() {
            match self.opening[i].task.take() {
                Some(result) => {
                    let o = self.opening.remove(i);
                    done.push(Outcome {
                        volume: o.volume,
                        label: o.label,
                        uuid: o.uuid,
                        result: result.map_err(|e| e.to_string()),
                    });
                }
                None => i += 1,
            }
        }
        done
    }

    /// The volumes being mounted.
    pub fn opening(&self) -> &[Opening] {
        &self.opening
    }

    /// Look at a volume again at the next [`update`](Automount::update),
    /// as if it had just come (after it couldn't be mounted).
    pub fn try_again(&mut self, block: &str) {
        self.seen.remove(block);
    }
}
