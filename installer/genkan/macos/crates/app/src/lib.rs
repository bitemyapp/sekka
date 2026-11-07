// SPDX-License-Identifier: GPL-3.0-or-later
//! Sekka Installer: the Mac app that installs Sekka beside macOS.
//!
//! [`model`] is the whole of the app's behavior, without AppKit, tested
//! here on any platform; [`runtime`] runs its effects on worker threads;
//! [`env`] is what it needs from the world. `mac` is the AppKit interface
//! and the live environment (helper, engine, downloads), compiled only for
//! macOS. [`sim`] is the development builds' simulation.
#[cfg(all(feature = "production", feature = "simulation"))]
compile_error!("production builds don't contain the simulation: build with --no-default-features --features production");

pub mod env;
pub mod model;
pub mod runtime;
#[cfg(feature = "simulation")]
pub mod sim;
pub mod text;

#[cfg(target_os = "macos")]
pub mod mac;
