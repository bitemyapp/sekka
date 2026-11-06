// SPDX-License-Identifier: GPL-3.0-or-later
//! Finish Installing Sekka: the app that opens by itself when the owner
//! starts up from Sekka's own Recovery, and does the one thing only
//! Recovery can: allow Sekka's startup software, with the owner's password.
//!
//! It is the same signed app on every Mac. What belongs to one installation
//! is beside it on the stub's System volume (`Sekka/InstallData`): the
//! configuration, the boot object and the firmware baseline, bound by
//! identity, size and digest, and checked against the volume the app was
//! started from before anything is asked. It can't resize, partition or
//! write Linux: it runs `bputil`, `kmutil configure-boot` and `bless` for
//! one volume, and retrying it repeats only those, with the same boot object.
pub mod finish;
pub mod install;
pub mod pty;
pub mod receipt;
pub mod screen;


#[cfg(target_os = "macos")]
pub mod mac;
