// SPDX-License-Identifier: GPL-3.0-or-later
//! Sekka's first-boot setup helper: the narrowly scoped root service that
//! finishes a freshly installed computer for its owner, asked only by
//! Aizome's login screen. It judges the person's choices with Genkan's
//! rules, makes their account (the password hashed as Genkan hashes it,
//! never written in the clear), applies their host name, time zone,
//! language and keyboard, and writes the same choices as configuration
//! for the first update to build. See docs/apple-silicon/first-boot-setup.md.

pub mod choices;
pub mod config;
pub mod handoff;
pub mod memory;
pub mod protocol;
pub mod render;
pub mod server;
pub mod system;
pub mod transaction;

/// The helper's, and its tests': no freed block keeps what it held.
#[global_allocator]
static ALLOCATOR: memory::Zeroing = memory::Zeroing(std::alloc::System);
