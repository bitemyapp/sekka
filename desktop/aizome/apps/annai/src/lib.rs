//! Annai 案内 (Welcome): Aizome's welcome. Seven short pages, each with a
//! live illustration drawn in Aizome's own look: the name and the mark,
//! tabs that stack, menus at the pointer, the palette, the Toolchest,
//! Desks and the Deskbar, the command key, and Settei's doors. Shown
//! once at first login (`--first-run`), and from Help ▸ Aizome Tour.

pub mod app;
pub mod art;
pub mod instance;
pub mod pages;
pub mod state;

pub use app::{Annai, Msg};
