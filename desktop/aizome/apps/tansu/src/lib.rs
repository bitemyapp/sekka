//! Tansu 箪笥 (Disks): GParted's job with Aizome's care. Drives, their
//! partitions on a map and in a table, operations queued and described in
//! plain words, then run through UDisks2 behind confirmations that hold
//! only for the device the person saw.
//!
//! - [`layout`]: a disk's segments, as they are or will be;
//! - [`plan`]: the pending operations, checked, merged and described;
//! - [`run`]: running them, one step at a time, with confirmation tokens;
//! - [`backend`]: UDisks2 (`aizome_sys::disks`), or the [`mock`] with
//!   sample disks for tests and `--sample`;
//! - [`app`] and `ui`: the window.

pub mod app;
pub mod backend;
pub mod drives;
pub mod fs;
pub mod instance;
pub mod layout;
pub mod mock;
pub mod plan;
pub mod run;
pub mod ui;

pub use app::{APP_ID, Msg, Tansu};
