//! `install`: Genkan's privileged install transaction: non-destructive
//! preparation, a typed confirmation, then destructive steps 1–6 past an
//! irreversible point (catalog GEN-07, GEN-06). See `SPEC.md`.

#[cfg(feature = "testing")]
pub mod env;
#[cfg(feature = "testing")]
pub mod traces;

use crate::Sink;

/// Sizes are in MiB.
pub const GIB: u64 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Config {
    /// Room the root filesystem needs beyond the system's closure, for
    /// updates and the installation workspace (8 GiB).
    pub reserve_mib: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self { reserve_mib: 8 * GIB }
    }
}

/// A unit of work the helper runs (subprocesses and file writes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Live guard, firmware, installer lock, disk revalidation, preflight, layout.
    Checks,
    /// Hardware detection and `configuration.nix` in the staging directory.
    Configuration,
    /// `nix build --dry-run`: what to build and how much it downloads.
    Evaluation,
    /// Build the system in the live store (memory).
    Build,
    /// Warm the page cache with the closure.
    Cache,
    /// Step 1: wipe the disk and write the new GPT. The irreversible point.
    Erase,
    /// Step 2: format and mount (and open encrypted mappings).
    Format,
    /// Step 3: configuration, account and Wi-Fi files on the target.
    Files,
    /// Step 4, in-memory route: `nix copy` the built system to the target.
    Copy,
    /// Step 4, deferred route: download and build on the target's store.
    BuildOnTarget,
    /// Step 5: `nixos-install` (profile and bootloader).
    Bootloader,
    /// The `install.json` record for measurements.
    Record,
    /// Step 6: sync, unmount, close mappings.
    Flush,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// The parsed request arrived (the helper session starts).
    Begin,
    /// `Checks` finished; the layout gives the root filesystem this size.
    Checked { root_mib: u64 },
    /// `Configuration` finished.
    Configured,
    /// `Evaluation` finished: what the selection downloads, its unpacked
    /// size, and what live memory can hold without the reserve.
    Evaluated { download_mib: u64, unpacked_mib: u64, budget_mib: u64 },
    /// `Build` finished: the closure's size.
    Built { closure_mib: u64 },
    /// `Cache` finished.
    Cached,
    /// The running destructive step (or `Record`) finished.
    StepDone,
    /// The running stage failed.
    StageFailed,
    /// The GUI sent the typed confirmation (`phrase_ok`: it matches the plan).
    Confirm { phrase_ok: bool },
    /// The GUI withdrew (stdin closed).
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Warning {
    /// `install.json` couldn't be written; the system is installed anyway.
    NoRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    Run(Stage),
    /// Preparation finished: show the summary and wait for confirmation.
    Prepared {
        deferred: bool,
        closure_mib: u64,
    },
    /// Destructive step `n` of 6 began (the GUI's progress).
    Progress(u8),
    /// Hold the sleep/idle inhibitor (GEN-12).
    StayAwake,
    AllowSleep,
    /// Stop the running stage's subprocesses.
    Interrupt,
    /// Preparation refused: the selection doesn't fit (no disk written).
    Refused {
        needed_mib: u64,
        root_mib: u64,
    },
    Warn(Warning),
    /// The session failed; `erased`: the disk had already been written.
    Failed {
        erased: bool,
    },
    /// The session was cancelled (no disk written).
    Cancelled,
    /// Installed.
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateName {
    Idle,
    Checks,
    Configuration,
    Evaluation,
    Build,
    Cache,
    /// Prepared, waiting for the confirmation.
    Prepared,
    /// Cancel arrived during a preparation stage; waiting for it to stop.
    Cancelling,
    Erase,
    Format,
    Files,
    Copy,
    BuildOnTarget,
    Bootloader,
    Record,
    Flush,
    Complete,
    Failed,
    Refused,
    Cancelled,
}

/// An implementation of the install machine.
pub trait Machine: Sized {
    fn new(config: Config) -> Self;
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>);
    fn state(&self) -> StateName;
}
