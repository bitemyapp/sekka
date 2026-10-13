//! What Kakehashi's window and its privileged helper share: the drives a
//! computer has and which of them may be written, images and their
//! checksums, Sekka's nightly builds, and the write itself.
//!
//! The window runs as the person using it and only ever reads. The helper,
//! started with the operating system's own administrator prompt, is the one
//! process that opens a drive for writing; it checks the plan it is given
//! again before it does.

pub mod drive;
pub mod image;
pub mod nightly;
pub mod platform;
pub mod write;

pub use drive::{Drive, human};
