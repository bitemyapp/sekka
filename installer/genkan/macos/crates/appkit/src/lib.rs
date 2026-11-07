// SPDX-License-Identifier: GPL-3.0-or-later
//! AppKit building blocks shared by Sekka Installer and its Recovery app:
//! closures as action targets, labels, buttons, stacks and constraints,
//! Sekka's colors, the crest, status marks and the disk bar. macOS only;
//! empty elsewhere.
#[cfg(target_os = "macos")]
pub mod ui;
#[cfg(target_os = "macos")]
pub mod views;
