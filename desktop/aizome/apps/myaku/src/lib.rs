//! Myaku 脈 (Activity): what each application and process uses of the
//! processor, memory, energy, graphics and storage; graphs of the system
//! over time; quitting what misbehaves. A port of Tatami's Activity (Sekka,
//! MIT OR Apache-2.0, the same author) to Aizome's kit, on aizome-sys's
//! readers.
//!
//! - [`sampler`]: a thread that reads the system every second into a
//!   [`model::Frame`];
//! - [`group`]: which processes make an application, and its name;
//! - [`history`]: the graphs' last two minutes;
//! - [`app`] and `ui`: the window.

pub mod app;
pub mod fixture;
pub mod group;
pub mod history;
pub mod instance;
pub mod model;
pub mod sampler;
pub mod ui;

pub use app::{APP_ID, Msg, Myaku};
