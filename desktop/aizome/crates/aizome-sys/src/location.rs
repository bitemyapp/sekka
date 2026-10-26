//! Where the computer is, from GeoClue 2 on the system bus, asked for at
//! city accuracy: what the shell writes to `settings.toml`'s `[location]`
//! so Night Light and Auto's theme follow sunset and sunrise there.
//!
//! GeoClue gives a position only to apps it trusts: with an agent running,
//! the agent asks; without one, the system's configuration must allow the
//! app by its desktop id (Aizome's NixOS module allows `org.aizome.Shell`).
//! Without GeoClue, or without a position, Aizome reckons from the time
//! zone's city instead (`aizome_config::location`).
//!
//! The same client as Tenki's (`apps/tenki/src/location.rs`), which asks
//! as `org.aizome.Tenki`.

use std::time::{Duration, Instant};

use zbus::blocking::proxy::Builder;
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

const SERVICE: &str = "org.freedesktop.GeoClue2";

/// GeoClue's accuracy levels: a city's is enough for the sun.
pub const ACCURACY_CITY: u32 = 4;

/// A position GeoClue found.
#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres.
    pub accuracy: f64,
}

/// Why there's no position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocateError {
    /// No GeoClue on this machine (or no system bus).
    Unavailable(String),
    /// GeoClue (or its agent) said no.
    Denied,
    /// GeoClue didn't find one in time (no network, no source).
    TimedOut,
    Failed(String),
}

impl std::fmt::Display for LocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocateError::Unavailable(e) => write!(f, "GeoClue isn't available ({e})"),
            LocateError::Denied => f.write_str("GeoClue didn't allow it"),
            LocateError::TimedOut => f.write_str("GeoClue found no position in time"),
            LocateError::Failed(e) => write!(f, "GeoClue failed: {e}"),
        }
    }
}

fn classify(e: zbus::Error) -> LocateError {
    let s = e.to_string();
    if s.contains("AccessDenied") || s.contains("NotAuthorized") || s.contains("PermissionDenied") {
        LocateError::Denied
    } else if s.contains("ServiceUnknown")
        || s.contains("NameHasNoOwner")
        || s.contains("Spawn")
        || s.contains("No such file")
    {
        LocateError::Unavailable(s)
    } else {
        LocateError::Failed(s)
    }
}

fn fdo(e: zbus::fdo::Error) -> LocateError {
    classify(zbus::Error::from(e))
}

/// Ask the GeoClue on the system bus where the computer is, as
/// `desktop_id`, waiting at most `timeout`.
pub fn locate(desktop_id: &str, timeout: Duration) -> Result<Located, LocateError> {
    let conn = Connection::system().map_err(|e| LocateError::Unavailable(e.to_string()))?;
    locate_on(&conn, desktop_id, timeout)
}

/// Ask the GeoClue on `conn`'s bus (tests serve a fake one on a private
/// bus).
pub fn locate_on(conn: &Connection, desktop_id: &str, timeout: Duration) -> Result<Located, LocateError> {
    let manager = proxy(
        conn,
        "/org/freedesktop/GeoClue2/Manager".try_into().expect("a path"),
        "org.freedesktop.GeoClue2.Manager",
    )?;
    let client_path: OwnedObjectPath = manager.call("GetClient", &()).map_err(classify)?;
    let client = proxy(conn, client_path, "org.freedesktop.GeoClue2.Client")?;
    client.set_property("DesktopId", desktop_id).map_err(fdo)?;
    client.set_property("RequestedAccuracyLevel", ACCURACY_CITY).map_err(fdo)?;
    client.call::<_, _, ()>("Start", &()).map_err(classify)?;
    let deadline = Instant::now() + timeout;
    let found = loop {
        let at: OwnedObjectPath = match client.get_property("Location") {
            Ok(p) => p,
            Err(e) => break Err(classify(e)),
        };
        if at.as_str() != "/" {
            break read_location(conn, at);
        }
        if Instant::now() >= deadline {
            break Err(LocateError::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let _ = client.call::<_, _, ()>("Stop", &());
    found
}

/// An uncached proxy for `iface` at `path` of GeoClue.
fn proxy(conn: &Connection, path: OwnedObjectPath, iface: &'static str) -> Result<Proxy<'static>, LocateError> {
    Builder::<Proxy<'static>>::new(conn)
        .destination(SERVICE)
        .and_then(|b| b.path(path))
        .and_then(|b| b.interface(iface))
        .map(|b| b.cache_properties(CacheProperties::No))
        .and_then(|b| b.build())
        .map_err(classify)
}

fn read_location(conn: &Connection, path: OwnedObjectPath) -> Result<Located, LocateError> {
    let loc = proxy(conn, path, "org.freedesktop.GeoClue2.Location")?;
    let latitude: f64 = loc.get_property("Latitude").map_err(classify)?;
    let longitude: f64 = loc.get_property("Longitude").map_err(classify)?;
    let accuracy: f64 = loc.get_property("Accuracy").unwrap_or(f64::NAN);
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return Err(LocateError::Failed(format!("an impossible position ({latitude}, {longitude})")));
    }
    Ok(Located { latitude, longitude, accuracy })
}
