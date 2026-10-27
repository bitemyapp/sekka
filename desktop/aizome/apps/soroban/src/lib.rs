//! Soroban 算盤, Aizome's calculator: Basic, Scientific and Programmer
//! keypads over one expression, typed or pressed, evaluated by
//! `aizome-search`'s `calc` (the palette's arithmetic), with a paper tape
//! of every calculation.

pub mod app;
pub mod bits;
pub mod display;
pub mod entry;
pub mod grouped;
pub mod instance;
pub mod keypad;
pub mod tape;

pub use app::{Msg, Soroban};
