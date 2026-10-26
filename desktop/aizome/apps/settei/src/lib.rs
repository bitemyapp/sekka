//! Settei 設定 (Settings): Aizome's System Settings.
//!
//! A window of panes in the macOS System Settings model, in Aizome's look:
//! the sidebar (search, the user's card, the panes in groups with Lucide
//! glyphs on dye tiles) and the chosen pane as a form on the right. Changes
//! apply at once through `aizome-config` (and the compositor's
//! `reload-config`) or `aizome-sys`; risky ones confirm.
//!
//! - [`pane`]: the contract every pane implements, and how panes plug in.
//! - [`panes`]: the panes, listed in [`panes::all`].
//! - [`form`]: sections and rows, the look every pane's form has.
//! - [`app`]: the window, its sidebar, search and navigation.
//! - [`listing`]: `--list-settings --json`, for the command palette.
//! - [`system`]: the live system and the sample one.

pub mod app;
pub mod extra;
pub mod form;
pub mod instance;
pub mod listing;
pub mod pane;
pub mod panes;
pub mod preview;
pub mod search;
pub mod shared;
pub mod system;
pub mod tz;
pub mod wallpaper;
pub mod widgets;

pub use app::{AppMsg, Settei, Start};
