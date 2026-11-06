// SPDX-License-Identifier: GPL-3.0-or-later
//! Sekka Installer's privileged helper.
//!
//! A root launchd daemon (`computer.sekka.installer.helper`) registered by
//! the app with `SMAppService`, which the owner approves in System
//! Settings. It accepts requests only from Sekka Installer (checked by code
//! signature on every connection, see `sekka-mac-xpc`), and it trusts
//! nothing those requests claim:
//!
//! - it verifies the catalog with its own trust roots and its own record of
//!   accepted catalogs, which the person can't reset;
//! - it checks the owner's account and password itself;
//! - it inspects the Mac itself, as root, derives the plan itself, and
//!   starts only if that plan's digest is the one the owner confirmed;
//! - it copies the app's downloads into its own staging, hashing them
//!   against the catalog as it copies;
//! - it runs one installation at a time, under a journal that records each
//!   stage before the engine may begin it.
//!
//! The operations are the closed set in `sekka_mac_contract::Request`: no
//! request names a command, an executable or a path the helper will run.
//! Everything that needs macOS itself (XPC, PAM, `bless`, the engine's
//! process) sits behind [`Platform`], so the rest is tested on any Unix.
pub mod engine;
mod helper;
pub mod paths;
mod platform;
mod redact;

#[cfg(target_os = "macos")]
pub mod macos;

pub use helper::Helper;
pub use paths::Paths;
pub use platform::{EngineInstall, EngineRun, PASSWORD_LIMIT, Platform, PlatformError, Secret};
pub use redact::redact;

#[cfg(test)]
mod tests;
