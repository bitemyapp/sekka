//! Tokei 時計, Aizome's clock: world clocks, alarms that ring through
//! systemd user timers whether Tokei runs or not, a timer and a
//! stopwatch, in IBM Plex Mono's tabular digits.

pub mod alarms;
pub mod app;
pub mod cities;
pub mod clock;
pub mod instance;
pub mod ring;
pub mod sound;
pub mod timing;
pub mod views;
pub mod widgets;

pub use app::{Msg, Tokei};
