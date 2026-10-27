//! Yukimi 雪見 (Software): Sekka's Nix software manager, made native for
//! Aizome. See what is installed and why, find what is available, try it,
//! install it for yourself or for everyone, update, go back to an earlier
//! version, and keep the store tidy, without editing configuration by hand.
//!
//! What it reads and changes is Sekka's Yukimi's own: the UI-independent
//! crates under `crates/` are copied from Sekka, and every change for
//! everyone goes through Sekka's installed `yukimi-helper`, under its
//! polkit action, exactly as the GTK Yukimi does it.

pub mod app;
pub mod checks;
pub mod comparison;
pub mod fixture;
pub mod instance;
pub mod job;
pub mod model;
pub mod ops;
pub mod paths;
pub mod ui;
pub mod update_history;
