//! The parts of Kakehashi's window that decide whether a drive is written:
//! the "Erase …?" dialog, whose OK alone makes the person's [`Pressed`]
//! (`dialog`), and starting the helper, only for a plan holding that press
//! (`elevate`). The window itself is the binary.
//!
//! [`Pressed`]: dialog::Pressed

// GPUI's input dispatch would press the dialog's OK for the program
// (clippy.toml): never here.
#![forbid(clippy::disallowed_methods)]

pub mod dialog;
pub mod elevate;
