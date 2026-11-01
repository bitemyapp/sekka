//! Chanoma 茶の間 (TV), Aizome's living-room app: streaming services,
//! films and shows from the disk, and games, all from a controller, with a
//! grown-up's PIN on what the kids may do and for how long.
//!
//! - [`pad`]: controllers, read from their event devices and held
//!   exclusively while Chanoma runs.
//! - [`retro`]: games, through libretro cores Chanoma drives itself.
//! - [`browser`]: services, full screen in a browser the controller drives
//!   through the compositor.
//! - [`rules`]: the PIN, what kids may open, and for how long.
//! - [`app`] and [`ui`]: the program and what it draws.

pub mod app;
pub mod browser;
pub mod clock;
pub mod instance;
pub mod library;
pub mod pad;
pub mod retro;
pub mod rules;
pub mod services;
pub mod signals;
pub mod store;
pub mod ui;

pub use app::{Chanoma, Config, Msg};

pub const APP_ID: &str = "org.aizome.Chanoma";
