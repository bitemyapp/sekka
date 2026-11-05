//! What Kakehashi's window and its privileged helper share: the drives a
//! computer has and which of them may be written, images and their
//! checksums, Sekka's nightly builds, and the plan the window gives the
//! helper with the progress the helper reports.
//!
//! The window runs as the person using it and only ever reads. The helper,
//! started with the operating system's own administrator prompt, is the one
//! process that opens a drive for writing, with `kakehashi-write`, which the
//! window doesn't depend on; it checks the plan it is given again before it
//! does.
//!
//! The window's side of a write is here too, as typestate (see
//! `aizome-typestate`): [`choice`], the drive list and the drive the person
//! confirmed, which is the only drive a [`write::Plan`] can name, and
//! [`job`], the write as the window follows the helper's reports.

pub mod choice;
pub mod drive;
pub mod image;
pub mod job;
pub mod nightly;
pub mod platform;
pub mod write;

pub use drive::{Drive, human};
