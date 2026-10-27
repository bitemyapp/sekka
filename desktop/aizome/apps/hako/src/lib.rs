//! Hako 箱 (Files), Aizome's file manager.
//!
//! The library is what works without a window: the file operations
//! ([`fsops`]: copy, move, the Trash, archives, undo), and the model the
//! window shows ([`model`]: folders and their entries, MIME types,
//! places, thumbnails, watching folders). The window is [`ui`].

pub mod fsops;
pub mod model;
#[cfg(feature = "gui")]
pub mod ui;

use std::path::PathBuf;

pub const APP_ID: &str = "org.aizome.Hako";

/// The home folder.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// An XDG base directory: `$var` if it's an absolute path, else
/// `~/<fallback>`.
pub fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var).map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => home_dir().join(fallback),
    }
}

/// Where Hako keeps its state: `$XDG_STATE_HOME/aizome/hako`.
pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("aizome/hako")
}
