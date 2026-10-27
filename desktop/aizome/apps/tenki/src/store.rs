//! What Tenki keeps on disk, where Aizome's conventions put it:
//!
//! - `$XDG_CACHE_HOME/aizome/tenki/forecast-<lat>_<lon>.json`: the last
//!   forecast at each place, as Open-Meteo sent it, with when it came,
//!   so the window opens at once and shows something offline;
//! - `$XDG_DATA_HOME/aizome/tenki/places.json`: the places kept, in order;
//! - `$XDG_STATE_HOME/aizome/tenki/state.json`: the place showing, the last
//!   position GeoClue gave;
//! - `$XDG_CONFIG_HOME/aizome/tenki.toml`: units and whether to use the
//!   location.
//!
//! Every file is written to a temporary name and renamed over the old, so
//! a crash leaves the old file or the new one, never half of one. Writes
//! go to a thread of their own ([`Store::deferred`]): replacing a file can
//! wait on the disk for a long while when it's busy, and the window must
//! not; a burst of writes to one file writes it once, and [`flush`] waits
//! for what's queued (Tenki calls it before exiting).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::places::Place;
use crate::units::Units;

/// Where the files go.
#[derive(Clone, Debug, PartialEq)]
pub struct Store {
    pub cache: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub config: PathBuf,
    /// Whether writes go to the writer thread (else they're done at once).
    pub deferred: bool,
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(fallback)
    })
}

impl Store {
    /// The XDG directories, written from the writer thread.
    pub fn xdg() -> Store {
        Store {
            deferred: true,
            cache: xdg("XDG_CACHE_HOME", ".cache").join("aizome/tenki"),
            data: xdg("XDG_DATA_HOME", ".local/share").join("aizome/tenki"),
            state: xdg("XDG_STATE_HOME", ".local/state").join("aizome/tenki"),
            config: xdg("XDG_CONFIG_HOME", ".config").join("aizome/tenki.toml"),
        }
    }

    /// Everything under `root` (tests), written at once.
    pub fn at(root: &Path) -> Store {
        Store {
            cache: root.join("cache"),
            data: root.join("data"),
            state: root.join("state"),
            config: root.join("tenki.toml"),
            deferred: false,
        }
    }

    /// The same, writing from the writer thread or not.
    pub fn deferred(mut self, on: bool) -> Store {
        self.deferred = on;
        self
    }

    fn write(&self, path: &Path, text: String) {
        if !self.deferred {
            return write(path, &text);
        }
        let sent = writer().lock().unwrap_or_else(|e| e.into_inner()).send(Job::Write(path.to_owned(), text));
        if let Err(std::sync::mpsc::SendError(Job::Write(_, text))) = sent {
            write(path, &text);
        }
    }

    fn forecast_path(&self, key: &str) -> PathBuf {
        self.cache.join(format!("forecast-{key}.json"))
    }

    pub fn load_forecast(&self, key: &str) -> Option<Cached> {
        let text = std::fs::read_to_string(self.forecast_path(key)).ok()?;
        let file: CachedFile = serde_json::from_str(&text).ok()?;
        Some(Cached { fetched_at: file.fetched_at, body: file.response.to_string() })
    }

    pub fn save_forecast(&self, key: &str, cached: &Cached) {
        let Ok(raw) = serde_json::from_str::<serde_json::Value>(&cached.body) else { return };
        let file = CachedFile { fetched_at: cached.fetched_at, response: raw };
        if let Ok(text) = serde_json::to_string(&file) {
            self.write(&self.forecast_path(key), text);
        }
    }

    /// Drop the cached forecasts no place uses any more.
    pub fn prune_forecasts(&self, keep: &[String]) {
        let Ok(dir) = std::fs::read_dir(&self.cache) else { return };
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(key) = name.strip_prefix("forecast-").and_then(|n| n.strip_suffix(".json"))
                && !keep.iter().any(|k| k == key)
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    pub fn load_places(&self) -> Option<Vec<Place>> {
        let text = std::fs::read_to_string(self.data.join("places.json")).ok()?;
        let file: PlacesFile = serde_json::from_str(&text).ok()?;
        Some(file.places)
    }

    pub fn save_places(&self, places: &[Place]) {
        let file = PlacesFile { places: places.to_vec() };
        if let Ok(text) = serde_json::to_string_pretty(&file) {
            self.write(&self.data.join("places.json"), text + "\n");
        }
    }

    pub fn load_state(&self) -> State {
        std::fs::read_to_string(self.state.join("state.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_state(&self, state: &State) {
        if let Ok(text) = serde_json::to_string_pretty(state) {
            self.write(&self.state.join("state.json"), text + "\n");
        }
    }

    pub fn load_settings(&self) -> Settings {
        std::fs::read_to_string(&self.config).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) {
        if let Ok(text) = toml::to_string(settings) {
            self.write(&self.config, text);
        }
    }
}

/// A forecast as it came, and when.
#[derive(Clone, Debug, PartialEq)]
pub struct Cached {
    pub fetched_at: i64,
    /// Open-Meteo's JSON.
    pub body: String,
}

#[derive(Serialize, Deserialize)]
struct CachedFile {
    fetched_at: i64,
    response: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct PlacesFile {
    places: Vec<Place>,
}

/// Where the user left things.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// The id of the place showing.
    pub selected: Option<String>,
    /// The last position GeoClue gave (rounded), so the current location
    /// shows its forecast offline.
    pub located: Option<(f64, f64)>,
}

/// Whether Tenki may ask where the machine is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Consent {
    /// Not asked yet: the main view asks, once.
    #[default]
    Ask,
    Allowed,
    Declined,
}

/// `tenki.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// None: the locale's.
    pub units: Option<Units>,
    pub location: Consent,
}

enum Job {
    Write(PathBuf, String),
    Flush(Sender<()>),
}

/// The writer thread's queue, started on first use.
fn writer() -> &'static Mutex<Sender<Job>> {
    static WRITER: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new().name("tenki-writer".into()).spawn(move || write_from(rx));
        Mutex::new(tx)
    })
}

fn write_from(rx: Receiver<Job>) {
    while let Ok(first) = rx.recv() {
        // What came meanwhile, the latest text for each file.
        let mut pending: Vec<PathBuf> = Vec::new();
        let mut texts: HashMap<PathBuf, String> = HashMap::new();
        let mut flushes = Vec::new();
        for job in std::iter::once(first).chain(rx.try_iter()) {
            match job {
                Job::Write(p, t) => {
                    if texts.insert(p.clone(), t).is_none() {
                        pending.push(p);
                    }
                }
                Job::Flush(done) => flushes.push(done),
            }
        }
        for p in pending {
            if let Some(t) = texts.remove(&p) {
                write(&p, &t);
            }
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}

/// Wait (a few seconds at most) for the writes queued to be written.
pub fn flush() {
    let (done, wait) = channel();
    let sent = writer().lock().unwrap_or_else(|e| e.into_inner()).send(Job::Flush(done)).is_ok();
    if sent {
        let _ = wait.recv_timeout(std::time::Duration::from_secs(5));
    }
}

/// Write `text` to `path` through a temporary file.
fn write(path: &Path, text: &str) {
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("{}: {e}", dir.display());
        return;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let done = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = done {
        log::warn!("{}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::places::Origin;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tenki-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn round_trips() {
        let root = temp("round");
        let s = Store::at(&root);
        assert!(s.load_places().is_none());
        assert_eq!(s.load_settings(), Settings::default());
        let p = Place {
            id: "geonames:1".into(),
            name: "Tokyo".into(),
            region: "Japan".into(),
            latitude: 35.69,
            longitude: 139.69,
            timezone: Some("Asia/Tokyo".into()),
            origin: Origin::Search,
        };
        s.save_places(std::slice::from_ref(&p));
        assert_eq!(s.load_places(), Some(vec![p]));
        let c = Cached { fetched_at: 42, body: r#"{"a":[1,2]}"#.into() };
        s.save_forecast("35.69_139.69", &c);
        assert_eq!(s.load_forecast("35.69_139.69"), Some(c));
        s.prune_forecasts(&["1.00_2.00".into()]);
        assert_eq!(s.load_forecast("35.69_139.69"), None);
        let set = Settings { units: Some(Units::Imperial), location: Consent::Declined };
        s.save_settings(&set);
        assert_eq!(s.load_settings(), set);
        assert!(std::fs::read_to_string(&s.config).unwrap().contains("units = \"imperial\""));
        let st = State { selected: Some("current".into()), located: Some((1.0, 2.0)) };
        s.save_state(&st);
        assert_eq!(s.load_state(), st);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deferred_writes_land_by_the_flush() {
        let root = temp("deferred");
        let s = Store::at(&root).deferred(true);
        for i in 0..20 {
            s.save_state(&State { selected: Some(format!("place {i}")), located: None });
        }
        flush();
        assert_eq!(s.load_state().selected.as_deref(), Some("place 19"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_files_give_defaults() {
        let root = temp("broken");
        let s = Store::at(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&s.config, "units = 7").unwrap();
        assert_eq!(s.load_settings(), Settings::default());
        std::fs::create_dir_all(&s.cache).unwrap();
        std::fs::write(s.cache.join("forecast-x.json"), "{").unwrap();
        assert_eq!(s.load_forecast("x"), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
