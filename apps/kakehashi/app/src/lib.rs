//! The parts of Kakehashi's window that decide whether a drive is written:
//! the "Erase …?" dialog, whose OK alone makes the person's [`Pressed`]
//! (`dialog`), and starting the helper, only for a plan holding that press
//! (`elevate`). The window itself is the binary.
//!
//! [`Pressed`]: dialog::Pressed

pub mod dialog;
pub mod elevate;
