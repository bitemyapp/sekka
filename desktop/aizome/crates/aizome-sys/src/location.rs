//! City-level positions from GeoClue for Tenki and the shell's sunrise
//! schedule. The desktop id must be allowed by GeoClue's agent or system
//! configuration. Setup, D-Bus calls and waiting share one deadline.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_io::Timer;
use futures_lite::future::{block_on, or};
use zbus::proxy::{Builder, CacheProperties};
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

const SERVICE: &str = "org.freedesktop.GeoClue2";
const MANAGER: &str = "/org/freedesktop/GeoClue2/Manager";

/// GeoClue's accuracy levels: a city's is enough for weather and the sun.
pub const ACCURACY_CITY: u32 = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres.
    pub accuracy: f64,
    /// The source's place name, if it supplied one.
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocateError {
    Unavailable(String),
    Denied,
    TimedOut,
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for LocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "GeoClue isn't available ({e})"),
            Self::Denied => f.write_str("GeoClue didn't allow it"),
            Self::TimedOut => f.write_str("GeoClue found no position in time"),
            Self::Cancelled => f.write_str("Location request cancelled"),
            Self::Failed(e) => write!(f, "GeoClue failed: {e}"),
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

/// Cancels one request, including a D-Bus call that never replies.
/// A fresh request needs a fresh token.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<CancelState>);

#[derive(Debug, Default)]
struct CancelState {
    cancelled: AtomicBool,
    event: event_listener::Event,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.event.notify(usize::MAX);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        let changed = self.0.event.listen();
        if !self.is_cancelled() {
            changed.await;
        }
    }
}

/// Ask GeoClue for a position. All work shares `timeout`, with at most
/// another 250 ms to send cleanup messages. No worker remains waiting for
/// a service reply after the deadline.
pub fn locate(desktop_id: &str, timeout: Duration) -> Result<Located, LocateError> {
    locate_cancellable(desktop_id, timeout, &Cancellation::default())
}

pub fn locate_cancellable(desktop_id: &str, timeout: Duration, cancel: &Cancellation) -> Result<Located, LocateError> {
    block_on(async {
        let deadline = Instant::now() + timeout;
        let conn = until(deadline, cancel, async {
            Connection::system().await.map_err(|e| LocateError::Unavailable(e.to_string()))
        })
        .await?;
        // Only this request owns the connection. Dropping it also removes
        // GeoClue's client if GetClient or Start outlived our wait.
        locate_until(&conn, desktop_id, deadline, cancel).await
    })
}

/// Like [`locate`], on a caller-owned connection (private buses in tests).
pub fn locate_on(
    conn: &zbus::blocking::Connection,
    desktop_id: &str,
    timeout: Duration,
) -> Result<Located, LocateError> {
    locate_on_cancellable(conn, desktop_id, timeout, &Cancellation::default())
}

pub fn locate_on_cancellable(
    conn: &zbus::blocking::Connection,
    desktop_id: &str,
    timeout: Duration,
    cancel: &Cancellation,
) -> Result<Located, LocateError> {
    block_on(locate_until(conn.inner(), desktop_id, Instant::now() + timeout, cancel))
}

async fn until<T>(
    deadline: Instant,
    cancel: &Cancellation,
    work: impl Future<Output = Result<T, LocateError>>,
) -> Result<T, LocateError> {
    or(
        async {
            cancel.cancelled().await;
            Err(LocateError::Cancelled)
        },
        or(
            async {
                Timer::at(deadline).await;
                Err(LocateError::TimedOut)
            },
            work,
        ),
    )
    .await
}

async fn locate_until(
    conn: &Connection,
    desktop_id: &str,
    deadline: Instant,
    cancel: &Cancellation,
) -> Result<Located, LocateError> {
    let mut client = None;
    let result = until(deadline, cancel, async {
        let manager = proxy(conn, MANAGER.try_into().expect("a path"), "org.freedesktop.GeoClue2.Manager").await?;
        let path: OwnedObjectPath = manager.call("GetClient", &()).await.map_err(classify)?;
        client = Some(proxy(conn, path, "org.freedesktop.GeoClue2.Client").await?);
        let client = client.as_ref().expect("created above");
        client.set_property("DesktopId", desktop_id).await.map_err(fdo)?;
        client.set_property("RequestedAccuracyLevel", ACCURACY_CITY).await.map_err(fdo)?;
        client.call::<_, _, ()>("Start", &()).await.map_err(classify)?;
        loop {
            let at: OwnedObjectPath = client.get_property("Location").await.map_err(classify)?;
            if at.as_str() != "/" {
                return read_location(conn, at).await;
            }
            Timer::after(Duration::from_millis(200)).await;
        }
    })
    .await;
    if let Some(client) = client {
        // No reply expected: cleanup cannot hold a result hostage. Bound
        // even sending, in case the bus stops draining its socket.
        or(
            async {
                let _ = client.call_noreply("Stop", &()).await;
                if let Ok(manager) =
                    proxy(conn, MANAGER.try_into().expect("a path"), "org.freedesktop.GeoClue2.Manager").await
                {
                    let _ = manager.call_noreply("DeleteClient", &(client.path(),)).await;
                }
            },
            async {
                Timer::after(Duration::from_millis(250)).await;
            },
        )
        .await;
    }
    result
}

async fn proxy(conn: &Connection, path: OwnedObjectPath, iface: &'static str) -> Result<Proxy<'static>, LocateError> {
    Builder::<Proxy<'static>>::new(conn)
        .destination(SERVICE)
        .and_then(|b| b.path(path))
        .and_then(|b| b.interface(iface))
        .map(|b| b.cache_properties(CacheProperties::No))
        .map_err(classify)?
        .build()
        .await
        .map_err(classify)
}

async fn read_location(conn: &Connection, path: OwnedObjectPath) -> Result<Located, LocateError> {
    let loc = proxy(conn, path, "org.freedesktop.GeoClue2.Location").await?;
    let latitude: f64 = loc.get_property("Latitude").await.map_err(classify)?;
    let longitude: f64 = loc.get_property("Longitude").await.map_err(classify)?;
    let accuracy: f64 = loc.get_property("Accuracy").await.unwrap_or(f64::NAN);
    let description = loc.get_property::<String>("Description").await.ok().filter(|s| !s.trim().is_empty());
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return Err(LocateError::Failed(format!("an impossible position ({latitude}, {longitude})")));
    }
    Ok(Located { latitude, longitude, accuracy, description })
}
