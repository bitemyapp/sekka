// SPDX-License-Identifier: MIT OR Apache-2.0
//! The typed steps of `yukimi-helper`'s privileged operations, in a
//! library so that their guarantees are shown by doctests (see the
//! `compile_fail` examples). The helper's program parses its arguments and
//! runs these steps; nothing here runs without it.
//!
//! - [`change`]: a change to the system as a transaction, which ends in
//!   its commit or in putting back everything it covered.
//! - [`clean`]: deleting old generations together with the boot menu's
//!   entries for them, then collecting garbage.
//! - [`snapshots`]: the configuration kept for each generation.
//! - [`process`]: running programs, and stopping when Yukimi asks.
//!
//! Starting a program other than through [`process::command`] is denied
//! (`clippy.toml`), here and in the helper's programs.
// Denied, not forbidden: process.rs makes the one command, under an allow
// that the Nix check permits nowhere else. The programs forbid it.
#![deny(clippy::disallowed_methods)]
#![forbid(unsafe_code)]
pub mod change;
pub mod clean;
pub mod process;
pub mod snapshots;
