//! Sumi 墨, Aizome's text editor: the board's editor, with Markdown styled
//! as it's typed and code coloured in its language; find and replace with
//! regular expressions; files in UTF-8, UTF-16 or Latin-1 with their line
//! endings kept; atomic saves; everything unsaved kept through a crash,
//! a logout or Quit; recent files shared with the desktop.

pub mod app;
pub mod autosave;
pub mod doc;
pub mod files;
pub mod find;
pub mod format;
pub mod instance;
pub mod menus;
pub mod recent;
pub mod settings;
pub mod syntax;
pub mod view;

pub use app::{Msg, Places, Sumi};
