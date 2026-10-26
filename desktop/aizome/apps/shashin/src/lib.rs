//! Shashin 写真 (Images), Aizome's image viewer: the picture on `sunk`,
//! fitted to the window; zoom about the pointer and pan; step through the
//! folder in Hako's order with a filmstrip; turn without loss, copy, set
//! as wallpaper, move to the Trash and take it back; full screen and a
//! slideshow. Pictures decode off the interface thread, a preview first,
//! and every frame at rest is a drawing made for exactly that zoom, drawn
//! one to one.

pub mod app;
pub mod clipboard;
pub mod folder;
pub mod instance;
pub mod load;
pub mod orient;
pub mod platform;
pub mod save;
pub mod thumbs;
pub mod trash;
pub mod ui;
pub mod view;

pub use app::{Msg, Shashin};
