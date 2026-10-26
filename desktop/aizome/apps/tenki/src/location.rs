//! Where the machine is, from GeoClue 2 over the system bus, asked for at
//! city accuracy and only after the user has said yes.
//!
//! GeoClue gives a position only to apps it trusts: with an agent running
//! (GNOME's, KDE's), the agent asks the user; without one, the system's
//! configuration must allow the app by its desktop id. On NixOS:
//!
//! ```nix
//! services.geoclue2.enable = true;
//! services.geoclue2.appConfig."org.aizome.Tenki" = { isAllowed = true; isSystem = false; };
//! ```

use std::time::Duration;

pub use aizome_sys::location::{ACCURACY_CITY, Cancellation, Located};

/// Why there's no position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocateError {
    /// No GeoClue on this machine (or no system bus).
    Unavailable(String),
    /// GeoClue (or its agent) said no.
    Denied,
    /// GeoClue didn't find one in time.
    TimedOut,
    Cancelled,
    Failed(String),
}

impl LocateError {
    /// What the main view says about it.
    pub fn message(&self) -> String {
        match self {
            LocateError::Unavailable(_) => "Location services aren't available on this computer.".into(),
            LocateError::Denied => "Tenki isn't allowed to use your location.".into(),
            LocateError::TimedOut => "Your location couldn't be found in time.".into(),
            LocateError::Cancelled => "Location search stopped.".into(),
            LocateError::Failed(e) => format!("Your location couldn't be found: {e}"),
        }
    }

    /// A line on what to do about it.
    pub fn hint(&self) -> &'static str {
        match self {
            LocateError::Unavailable(_) | LocateError::Denied | LocateError::Cancelled => {
                "Search for a city to see its weather without location services."
            }
            LocateError::TimedOut | LocateError::Failed(_) => {
                "Search for a city, or check your connection and try again."
            }
        }
    }
}

impl From<aizome_sys::location::LocateError> for LocateError {
    fn from(error: aizome_sys::location::LocateError) -> Self {
        use aizome_sys::location::LocateError as Shared;
        match error {
            Shared::Unavailable(e) => Self::Unavailable(e),
            Shared::Denied => Self::Denied,
            Shared::TimedOut => Self::TimedOut,
            Shared::Cancelled => Self::Cancelled,
            Shared::Failed(e) => Self::Failed(e),
        }
    }
}

/// The shared client bounds setup, location lookup and cleanup together.
pub fn locate(desktop_id: &str, timeout: Duration) -> Result<Located, LocateError> {
    aizome_sys::location::locate(desktop_id, timeout).map_err(Into::into)
}

pub fn locate_cancellable(desktop_id: &str, timeout: Duration, cancel: &Cancellation) -> Result<Located, LocateError> {
    aizome_sys::location::locate_cancellable(desktop_id, timeout, cancel).map_err(Into::into)
}

pub fn locate_on(
    conn: &zbus::blocking::Connection,
    desktop_id: &str,
    timeout: Duration,
) -> Result<Located, LocateError> {
    aizome_sys::location::locate_on(conn, desktop_id, timeout).map_err(Into::into)
}
