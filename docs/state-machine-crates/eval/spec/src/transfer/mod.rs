//! `transfer`: placing items into a folder, with the conflict prompt,
//! merging folders (a nested machine per folder), and replacing by setting
//! the old item aside until its replacement is in place (catalog HAKO-05).
//! See `SPEC.md`.

#[cfg(feature = "testing")]
pub mod env;
#[cfg(feature = "testing")]
pub mod traces;

use crate::Sink;

/// An item to place: a source path and its target path, named by the
/// driver. Top-level items are `0..items`; a merged folder's children get
/// the ids its `Listed` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u32);

/// The hidden name (`.name.hako-aside-*`) a replaced item was set aside under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AsideId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Copy,
    Move,
}

/// Which operation built the job. The spec never lets this decide what
/// happens to a replaced item (the current code does: HAKO-05).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// Copy or Move into a folder (`transfer`).
    Transfer,
    /// Duplicate beside itself (`duplicate`).
    Duplicate,
    /// Put Back from the Trash, and Undo of Move to Trash (`restore`).
    Restore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Config {
    pub mode: Mode,
    pub kind: JobKind,
    /// The person chose to delete replaced items permanently (else they go
    /// to the Trash).
    pub delete_replaced: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { mode: Mode::Copy, kind: JobKind::Transfer, delete_replaced: false }
    }
}

/// What a probe of the source and its target found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Found {
    /// Nothing has the target's name (a copy onto itself already got a free one).
    Free,
    /// Something else has the name; `can_merge`: both are folders.
    Conflict { can_merge: bool },
    /// A move onto itself or another name of the same file: nothing to do.
    Same,
    /// It can't go there (gone, into itself, a mount point): left out.
    Refused,
}

/// The person's answer to the conflict sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    Skip,
    Cancel,
    KeepBoth,
    Merge,
    Replace,
}

/// How placing (rename, or copy then delete for a move) ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placed {
    Ok,
    /// It failed and the person chose Skip (or nothing could be retried).
    Skip,
    /// The person chose Stop.
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// Begin with top-level items `0..items`.
    Start {
        items: u32,
    },
    Probed {
        item: ItemId,
        found: Found,
    },
    Answered {
        item: ItemId,
        resolution: Resolution,
    },
    /// The target was renamed aside (`None`: it couldn't be).
    SetAside {
        item: ItemId,
        aside: Option<AsideId>,
    },
    Placed {
        item: ItemId,
        outcome: Placed,
    },
    /// The replaced item went to the Trash, or was deleted (`ok: false`: it couldn't).
    Disposed {
        item: ItemId,
        ok: bool,
    },
    /// The replaced item was renamed back (`ok: false`: it couldn't).
    Restored {
        item: ItemId,
        ok: bool,
    },
    /// A folder to merge was read: its children are `first .. first+count`.
    Listed {
        item: ItemId,
        first: ItemId,
        count: u32,
    },
}

/// Where to place an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Name {
    /// The target's own name.
    Target,
    /// A free name beside it ("name 2").
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Problem {
    /// The replaced item couldn't go to the Trash; it's kept under its aside name.
    KeptAside(AsideId),
    /// Placing failed and the replaced item couldn't be put back; it's under its aside name.
    Stranded(AsideId),
    /// The replaced item couldn't all be deleted (permanent deletion was chosen).
    NotAllRemoved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Done,
    /// Skipped at the conflict prompt.
    Skipped,
    /// Left out: refused, or failed and skipped.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Read the source's and the target's metadata.
    Probe {
        item: ItemId,
    },
    /// Show the conflict sheet.
    Ask {
        item: ItemId,
        can_merge: bool,
    },
    /// Place the source under that name.
    PlaceAt {
        item: ItemId,
        name: Name,
    },
    /// Rename what has the target's name aside, under a hidden name.
    SetAside {
        item: ItemId,
    },
    /// Move the set-aside item to the Trash, as the item it was.
    Trash {
        item: ItemId,
        aside: AsideId,
    },
    /// Delete the set-aside item permanently.
    Delete {
        item: ItemId,
        aside: AsideId,
    },
    /// Rename the set-aside item back to the target's name.
    RestoreAside {
        item: ItemId,
        aside: AsideId,
    },
    /// Read a folder's children for a merge.
    List {
        item: ItemId,
    },
    /// A moved folder whose children all arrived: remove it if empty.
    RemoveEmptySource {
        item: ItemId,
    },
    Problem {
        item: ItemId,
        problem: Problem,
    },
    Outcome {
        item: ItemId,
        outcome: Outcome,
    },
    Finished {
        cancelled: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateName {
    Idle,
    Probing,
    Asking,
    Placing,
    SettingAside,
    PlacingReplacement,
    Disposing,
    Restoring,
    Listing,
    Finished,
}

/// An implementation of the transfer machine.
pub trait Machine: Sized {
    fn new(config: Config) -> Self;
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>);
    fn state(&self) -> StateName;
}
