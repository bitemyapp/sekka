//! Chanoma 茶の間 (TV), Aizome's living-room app: streaming services,
//! films and shows from the disk, and games, all from a controller, with a
//! grown-up's PIN on what the kids may do and for how long.
//!
//! - [`pad`]: controllers, read from their event devices and held
//!   exclusively while Chanoma runs.
//! - [`retro`]: games, through libretro cores Chanoma drives itself.
//! - [`browser`]: services, full screen in a browser the controller drives
//!   through the compositor; [`devtools`] keeps its pages to the
//!   service's sites.
//! - [`rules`]: the PIN, what kids may open, and for how long.
//! - [`folders`]: choosing a games or videos folder from the sofa.
//! - [`library`], [`collection`] and [`metadata`]: the games and videos
//!   found (through the files index Aizome's apps share), which of them
//!   show, and what the games' databases say of them; [`zip`]: games kept
//!   zipped.
//! - [`app`] and [`ui`]: the program and what it draws.

pub mod app;
pub mod art;
pub mod browser;
pub mod clock;
pub mod collection;
pub mod devtools;
pub mod folders;
pub mod instance;
pub mod library;
pub mod metadata;
pub use aizome_gamepad as pad;
pub mod retro;
pub mod rules;
pub mod services;
pub mod signals;
pub mod store;
pub mod ui;
pub mod zip;

pub use app::{Chanoma, Config, Msg};

pub const APP_ID: &str = "org.aizome.Chanoma";
