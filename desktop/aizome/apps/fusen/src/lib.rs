//! Fusen 付箋, Aizome's notes: Markdown files in `~/Notes` (plain,
//! searchable and agent-readable), folders as subdirectories, a list with
//! search and preview lines, an editor with light Markdown styling and task
//! boxes that tick with a click, colours, and stickies that float above
//! other windows and come back at login where they were.

pub mod app;
pub mod notes;
pub mod view;
pub mod wm;

pub use app::{Fusen, Msg, Places, Request};
