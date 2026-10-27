// SPDX-License-Identifier: GPL-3.0-or-later
//! Genkan's window, on Aizome's kit. The kit owns the window and its
//! widgets; disk discovery, validation, Wi-Fi snapshots, page cache warming,
//! authorization and installation run on worker threads that answer with
//! messages, as the GTK interface's workers did.
//!
//! The library is the program, so tests can drive it headlessly
//! (`aizome_kit::testing::Harness`); `main.rs` only runs it.
pub mod app;
pub mod format;
pub mod media;
pub mod preview;
pub mod view;
pub mod zone;

pub use app::{Genkan, Msg, Step};

pub fn run() {
    aizome_kit::run(Genkan::new(preview::requested()));
}
