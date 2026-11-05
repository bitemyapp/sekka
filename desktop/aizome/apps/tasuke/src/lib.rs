//! Tasuke 助 (Assistant): Sekka's private assistant, in a native Aizome
//! window. See `apps/tasuke/DESIGN.md` for the whole of it.

pub mod agent;
pub mod app;
pub mod approval;
pub mod diff;
pub mod fixture;
pub mod fold;
#[cfg(feature = "live")]
pub mod live;
pub mod markdown;
pub mod models;
pub mod script;
pub mod server;
pub mod thumb;
pub mod time;
pub mod transcript;
pub mod ui;
pub mod widgets;
