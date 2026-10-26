//! What Tenki asks of the world: forecasts and place searches from
//! Open-Meteo, the position from GeoClue, the time zone and the clock.
//!
//! [`Live`] asks the real ones, over HTTPS with `ureq` (rustls), from the
//! kit's background threads. [`Recorded`] answers from JSON recorded from
//! Open-Meteo once (`aizome-tenki --record-fixtures DIR`), for tests and
//! golden images: they never touch the network.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use crate::location::{LocateError, Located};
use crate::places::cache_key;

/// Who Tenki says it is to Open-Meteo.
pub const USER_AGENT: &str = concat!(
    "Aizome-Tenki/",
    env!("CARGO_PKG_VERSION"),
    " (Aizome desktop weather; +https://github.com/bitemyapp/aizome)"
);

/// Why a fetch failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// No way through: no network, DNS, a timeout.
    Offline(String),
    /// Open-Meteo answered with an error status.
    Status(u16, String),
    /// The answer didn't read.
    Bad(String),
}

impl FetchError {
    pub fn is_offline(&self) -> bool {
        matches!(self, FetchError::Offline(_))
    }

    pub fn message(&self) -> String {
        match self {
            FetchError::Offline(_) => "Tenki can't reach Open-Meteo.".into(),
            FetchError::Status(429, _) => "Open-Meteo asked Tenki to wait a little.".into(),
            FetchError::Status(code, reason) if !reason.is_empty() => format!("Open-Meteo said no ({code}): {reason}"),
            FetchError::Status(code, _) => format!("Open-Meteo said no ({code})."),
            FetchError::Bad(e) => format!("Open-Meteo's answer didn't make sense: {e}"),
        }
    }
}

/// The world, as Tenki asks it. Calls block: they run on background
/// threads unless [`inline`](Services::inline).
pub trait Services: Send + Sync + 'static {
    /// Open-Meteo's forecast at a position, as JSON.
    fn forecast(&self, lat: f64, lon: f64) -> Result<String, FetchError>;
    /// Open-Meteo's places named like `name`, as JSON.
    fn search(&self, name: &str) -> Result<String, FetchError>;
    /// The machine's position.
    fn locate(&self) -> Result<Located, LocateError>;
    /// The machine's time zone (`America/Chicago`).
    fn timezone(&self) -> Option<String>;
    /// The Unix time now.
    fn now(&self) -> i64;
    /// Run calls on the program's thread (tests: answers come at once).
    fn inline(&self) -> bool {
        false
    }
}

/// The real world.
pub struct Live {
    agent: ureq::Agent,
}

impl Default for Live {
    fn default() -> Live {
        Live::new()
    }
}

impl Live {
    pub fn new() -> Live {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(20)))
            .timeout_connect(Some(Duration::from_secs(8)))
            .http_status_as_error(false)
            .build()
            .into();
        Live { agent }
    }

    fn get(&self, url: &str, query: &[(&str, String)]) -> Result<String, FetchError> {
        let mut req = self.agent.get(url).header("Accept", "application/json");
        for (k, v) in query {
            req = req.query(*k, v);
        }
        let mut resp = req.call().map_err(|e| match e {
            ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Protocol(_)
            | ureq::Error::Tls(_)
            | ureq::Error::Rustls(_)
            | ureq::Error::BodyStalled => FetchError::Offline(e.to_string()),
            other => FetchError::Bad(other.to_string()),
        })?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().map_err(|e| FetchError::Offline(e.to_string()))?;
        if status >= 400 {
            let reason = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("reason").and_then(|r| r.as_str()).map(str::to_owned))
                .unwrap_or_default();
            return Err(FetchError::Status(status, reason));
        }
        Ok(body)
    }
}

impl Services for Live {
    fn forecast(&self, lat: f64, lon: f64) -> Result<String, FetchError> {
        self.get(crate::forecast::FORECAST_URL, &crate::forecast::query(lat, lon))
    }

    fn search(&self, name: &str) -> Result<String, FetchError> {
        self.get(crate::places::SEARCH_URL, &crate::places::query(name))
    }

    fn locate(&self) -> Result<Located, LocateError> {
        crate::location::locate(crate::APP_ID, Duration::from_secs(20))
    }

    fn timezone(&self) -> Option<String> {
        crate::places::system_timezone()
    }

    fn now(&self) -> i64 {
        crate::time::unix_now()
    }
}

/// Answers recorded from Open-Meteo, from a directory of
/// `forecast-<lat>_<lon>.json` and `search-<name>.json` files and a
/// `recorded.json` saying when (`{"at": <unix time>}`).
pub struct Recorded {
    dir: PathBuf,
    state: Mutex<RecordedState>,
}

struct RecordedState {
    now: i64,
    offline: bool,
    located: Result<Located, LocateError>,
    timezone: Option<String>,
    /// What was asked, in order: `forecast 35.69_139.69`, `search tokyo`.
    asked: Vec<String>,
}

impl Recorded {
    /// The answers in `dir`, the clock at the time they were recorded.
    pub fn new(dir: impl Into<PathBuf>) -> Recorded {
        let dir = dir.into();
        let now = std::fs::read_to_string(dir.join("recorded.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("at").and_then(|a| a.as_i64()))
            .unwrap_or(0);
        Recorded {
            dir,
            state: Mutex::new(RecordedState {
                now,
                offline: false,
                located: Err(LocateError::Unavailable("no GeoClue in tests".into())),
                timezone: None,
                asked: Vec::new(),
            }),
        }
    }

    fn st(&self) -> std::sync::MutexGuard<'_, RecordedState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// When the answers were recorded.
    pub fn recorded_at(&self) -> i64 {
        std::fs::read_to_string(self.dir.join("recorded.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("at").and_then(|a| a.as_i64()))
            .unwrap_or(0)
    }

    pub fn set_now(&self, now: i64) {
        self.st().now = now;
    }

    /// Answer nothing, as with no network.
    pub fn set_offline(&self, offline: bool) {
        self.st().offline = offline;
    }

    pub fn set_located(&self, located: Result<Located, LocateError>) {
        self.st().located = located;
    }

    pub fn set_timezone(&self, tz: Option<&str>) {
        self.st().timezone = tz.map(str::to_owned);
    }

    /// What was asked so far.
    pub fn asked(&self) -> Vec<String> {
        self.st().asked.clone()
    }

    fn read(&self, name: &str) -> Result<String, FetchError> {
        if self.st().offline {
            return Err(FetchError::Offline("offline (test)".into()));
        }
        std::fs::read_to_string(self.dir.join(name))
            .map_err(|_| FetchError::Status(404, format!("no recording {name}")))
    }
}

/// The file a search for `name` is recorded in.
pub fn search_file(name: &str) -> String {
    let slug: String = name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    format!("search-{slug}.json")
}

/// The file a forecast at a position is recorded in.
pub fn forecast_file(lat: f64, lon: f64) -> String {
    format!("forecast-{}.json", cache_key(lat, lon))
}

impl Services for Recorded {
    fn forecast(&self, lat: f64, lon: f64) -> Result<String, FetchError> {
        self.st().asked.push(format!("forecast {}", cache_key(lat, lon)));
        self.read(&forecast_file(lat, lon))
    }

    fn search(&self, name: &str) -> Result<String, FetchError> {
        self.st().asked.push(format!("search {}", name.trim().to_lowercase()));
        match self.read(&search_file(name)) {
            // Searches not recorded find nothing.
            Err(FetchError::Status(404, _)) => Ok(r#"{"generationtime_ms":0.1}"#.into()),
            other => other,
        }
    }

    fn locate(&self) -> Result<Located, LocateError> {
        self.st().located.clone()
    }

    fn timezone(&self) -> Option<String> {
        self.st().timezone.clone()
    }

    fn now(&self) -> i64 {
        self.st().now
    }

    fn inline(&self) -> bool {
        true
    }
}

/// Fetch real answers into `dir` for tests: forecasts at `positions` and
/// searches for `names`, a few requests in all, with a pause between them.
pub fn record(dir: &Path, positions: &[(f64, f64)], names: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let live = Live::new();
    let at = crate::time::unix_now();
    for &(lat, lon) in positions {
        let body = live.forecast(lat, lon).map_err(|e| e.message())?;
        crate::forecast::parse(&body)?;
        std::fs::write(dir.join(forecast_file(lat, lon)), pretty(&body)).map_err(|e| e.to_string())?;
        eprintln!("recorded the forecast at {lat}, {lon}");
        std::thread::sleep(Duration::from_millis(500));
    }
    for name in names {
        let body = live.search(name).map_err(|e| e.message())?;
        crate::places::parse_search(&body)?;
        std::fs::write(dir.join(search_file(name)), pretty(&body)).map_err(|e| e.to_string())?;
        eprintln!("recorded the search for {name:?}");
        std::thread::sleep(Duration::from_millis(500));
    }
    std::fs::write(dir.join("recorded.json"), format!("{{\"at\": {at}}}\n")).map_err(|e| e.to_string())?;
    Ok(())
}

fn pretty(json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .map_or_else(|| json.to_owned(), |s| s + "\n")
}
