//! Baseline "implicit": each machine in the current code's shape (flags,
//! `Option`s and counters on a struct, decided by `if`s), as a sans-I/O
//! machine against the same spec.
//!
//! Where the current code has a confirmed bug, this keeps it, marked
//! `BUG(<catalog id>)` and guarded by [`BUGS`]; those traces fail here, and
//! only those. Where the spec resolves a hazard that isn't a confirmed bug,
//! this follows the spec within the same shape, marked `SPEC-FIX`.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;

/// Whether the confirmed bugs are kept (off with the `fixed` feature).
pub const BUGS: bool = !cfg!(feature = "fixed");
