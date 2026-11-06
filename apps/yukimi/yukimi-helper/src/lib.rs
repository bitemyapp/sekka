// SPDX-License-Identifier: MIT OR Apache-2.0
//! The typed steps of `yukimi-helper`'s privileged operations, in a
//! library so that their guarantees are shown by doctests (see the
//! `compile_fail` examples). The helper's program parses its arguments and
//! runs these steps; nothing here runs without it.
//!
//! - [`change`]: a change to the system as a transaction, which ends in
//!   its commit or in putting back everything it covered.
//! - [`snapshots`]: the configuration kept for each generation.
//! - [`process`]: running programs, and stopping when Yukimi asks.
pub mod change;
pub mod process;
pub mod snapshots;
