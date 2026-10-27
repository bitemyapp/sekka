//! Tenki 天気, Aizome's weather: current conditions, the next 48 hours and
//! ten days ahead from Open-Meteo, for the places the user keeps, the
//! city of the machine's time zone and (if the user allows it) where
//! GeoClue says the machine is. Forecasts are cached, so the window opens
//! at once and still shows the last one offline.

pub mod app;
pub mod forecast;
pub mod instance;
pub mod location;
pub mod places;
pub mod services;
pub mod store;
pub mod time;
pub mod ui;
pub mod units;
pub mod weather;

pub use app::{Msg, Tenki};

/// Tenki's app id: its desktop file's name, and what GeoClue knows it by.
pub const APP_ID: &str = "org.aizome.Tenki";
