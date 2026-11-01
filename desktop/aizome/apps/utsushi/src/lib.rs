//! Utsushi 映し (Media Player), Aizome's video and music player.
//!
//! Battery first: frames a GPU decodes go to the compositor as they are,
//! to be scanned out on a plane of their own while nothing covers the
//! picture (subtitles sit in the letterbox where they fit, and the
//! full-screen controls hide); nothing is decoded while the window is out
//! of sight; music plays in long PipeWire cycles; and the window redraws
//! a digit when a second passes. See `aizome-media` for the engine.

pub mod app;
pub mod instance;
pub mod picture;
pub mod store;
pub mod ui;
pub mod widgets;

pub use app::{Msg, Utsushi};
