//! Monitors' own brightness, for the displays that aren't a laptop's
//! panel (those go through [`crate::power::Backlight`]):
//!
//! - **DDC/CI** (most external monitors): MCCS VCP feature 0x10 over the
//!   connector's I2C bus, `/dev/i2c-N` ([`ddc`]). Many monitors don't
//!   answer, or have DDC/CI turned off in their own menu, and docks and
//!   adapters often don't pass it on: each is asked once, and only those
//!   that answer with a brightness get a control.
//! - **Apple Studio Displays**, which have no DDC/CI: a HID feature report
//!   on their USB interface 7, `/dev/hidrawN` ([`hid`]).
//!
//! A tiled monitor (a Studio Display over Thunderbolt is two DisplayPort
//! connectors) is one [`Monitor`] with every connector it shows on.
//! Displays that are connected but can't be set are listed too
//! ([`Unavailable`]), with why ([`Why`]).
//!
//! The devices are opened as the user. On NixOS the Aizome module's
//! `programs.aizome.monitorBrightness` loads `i2c-dev` and has udev give
//! whoever is at the computer (`uaccess`) the display adapters' I2C buses
//! and the Studio Displays' brightness interface.
//!
//! Everything slow happens on one thread of the handle's: DDC/CI takes
//! 50 ms an answer and longer to retry. Levels asked for while one is on
//! its way are folded into the last, so a dragged slider sends what's
//! current, not a queue. The thread looks again when sysfs changes
//! (connectors, HID devices, device permissions; checked every 2 s, from
//! sysfs only) and when asked ([`Monitors::refresh`]).
//!
//! A Studio Display has no buttons, and after losing power comes back at
//! a level of its own, so its level is remembered, in
//! `$XDG_STATE_HOME/aizome/monitor-brightness.json` by make, model and
//! serial, and a handle made with [`Monitors::restoring`] (the shell's)
//! sets it again when the display appears. DDC/CI monitors keep their own
//! level, which their buttons change too, so theirs is left to them.

pub mod ddc;
mod discover;
pub mod edid;
pub mod hid;

use crate::error::{Error, Result};
use crate::service::{Done, Model, Task, Wake, service_handle};
use discover::{Group, HidDisplay};
use hid::Field;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How a monitor's brightness is set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// DDC/CI on `/dev/i2c-<bus>`.
    Ddc { bus: u32 },
    /// A Studio Display's HID interface, `/dev/<node>`.
    StudioDisplay { node: String, product: u16 },
    /// Made up: Settings' sample, and tests.
    Sample,
}

impl Control {
    /// Whether Aizome keeps this monitor's level (it doesn't itself).
    pub fn remembered(&self) -> bool {
        matches!(self, Control::StudioDisplay { .. })
    }

    /// How it's set, for people: "DDC/CI", "USB".
    pub fn through(&self) -> &'static str {
        match self {
            Control::Ddc { .. } | Control::Sample => "DDC/CI",
            Control::StudioDisplay { .. } => "USB",
        }
    }
}

/// A monitor whose brightness can be set.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Its first connector's sysfs name (`card0-DP-1`).
    pub id: String,
    /// The compositor's names for its connectors (`DP-1`, `DP-2`): one
    /// for most monitors, each tile's for a tiled one.
    pub connectors: Vec<String>,
    /// From its EDID, as the compositor reads them.
    pub make: String,
    pub model: String,
    pub serial: String,
    pub control: Control,
    /// The raw level, from `min` to `max`.
    pub level: u32,
    pub min: u32,
    pub max: u32,
}

impl Monitor {
    /// The level as a fraction of its range, 0.0–1.0.
    pub fn fraction(&self) -> f32 {
        fraction(self.level, self.min, self.max)
    }

    /// The raw level for a fraction.
    pub fn level_for(&self, fraction: f32) -> u32 {
        level_for(fraction, self.min, self.max)
    }

    /// Whether it shows on the compositor's output `connector`.
    pub fn shows_on(&self, connector: &str) -> bool {
        self.connectors.iter().any(|c| c == connector)
    }

    /// Its name as Settings names displays: the maker and the model.
    pub fn title(&self) -> String {
        title(&self.make, &self.model, &self.connectors)
    }
}

fn fraction(level: u32, min: u32, max: u32) -> f32 {
    if max <= min { 0.0 } else { (level.clamp(min, max) - min) as f32 / (max - min) as f32 }
}

fn level_for(fraction: f32, min: u32, max: u32) -> u32 {
    let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    min + (fraction * max.saturating_sub(min) as f32).round() as u32
}

fn title(make: &str, model: &str, connectors: &[String]) -> String {
    let make = make.split_whitespace().next().unwrap_or("").trim_end_matches([',', '.']);
    let model = model.trim();
    let unknown = |s: &str| s.is_empty() || s.eq_ignore_ascii_case("unknown");
    match (unknown(make), unknown(model)) {
        (false, false) => format!("{make} {model}"),
        (true, false) => model.to_owned(),
        (false, true) => make.to_owned(),
        (true, true) => connectors.first().cloned().unwrap_or_default(),
    }
}

/// Why a connected display's brightness can't be set from here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// No answer over DDC/CI.
    NoDdc,
    /// DDC/CI, but no luminance feature.
    NoLuminance,
    /// Its device (`/dev/i2c-N`, `/dev/hidrawN`) is missing or can't be
    /// opened as this user.
    NoAccess(String),
    /// A kernel driver (ddcci) holds its DDC/CI address.
    Busy,
    /// A Studio Display without its USB devices.
    NoUsb,
    /// A Studio Display on Thunderbolt that the computer hasn't
    /// authorized, so its USB devices aren't there.
    NotAuthorized,
    /// More than one Studio Display, and no telling whose USB is whose.
    Ambiguous,
    /// The kernel gives the connector no DDC channel.
    NoChannel,
    /// Reading the level failed otherwise.
    Failed(String),
}

impl Why {
    /// What to tell someone, in a sentence or two.
    pub fn explain(&self) -> String {
        match self {
            Why::NoDdc => "It doesn't answer over DDC/CI. Some monitors have DDC/CI turned off in their own menu; \
                           others don't have it, or a dock or adapter doesn't pass it on."
                .into(),
            Why::NoLuminance => "It answers over DDC/CI, but has no brightness control there.".into(),
            Why::NoAccess(path) => format!(
                "Aizome can't open {path}. The Aizome module's programs.aizome.monitorBrightness gives whoever is \
                 at the computer access; it takes effect after a restart."
            ),
            Why::Busy => "A kernel driver (ddcci) has its DDC/CI: its brightness is that driver's.".into(),
            Why::NoUsb => {
                "A Studio Display takes its brightness over USB, and its USB devices aren't connected.".into()
            }
            Why::NotAuthorized => "A Studio Display takes its brightness over USB. Over Thunderbolt its USB devices \
                                   come only once the computer authorizes the display, which it hasn't (boltctl)."
                .into(),
            Why::Ambiguous => {
                "With more than one Studio Display connected, Aizome can't tell whose USB connection is whose.".into()
            }
            Why::NoChannel => "The kernel gives its connector no DDC channel.".into(),
            Why::Failed(e) => format!("Reading its brightness failed: {e}"),
        }
    }
}

/// A connected display whose brightness can't be set from here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable {
    pub connectors: Vec<String>,
    pub make: String,
    pub model: String,
    pub why: Why,
}

impl Unavailable {
    pub fn title(&self) -> String {
        title(&self.make, &self.model, &self.connectors)
    }
}

/// The external monitors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorsState {
    pub monitors: Vec<Monitor>,
    pub unavailable: Vec<Unavailable>,
    /// Whether the first look has finished (until then nothing is known
    /// to be missing).
    pub ready: bool,
}

impl MonitorsState {
    /// The monitor showing on the compositor's output `connector`.
    pub fn on(&self, connector: &str) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.shows_on(connector))
    }

    /// Why the display on `connector` has no control, if it hasn't.
    pub fn unavailable_on(&self, connector: &str) -> Option<&Unavailable> {
        self.unavailable.iter().find(|u| u.connectors.iter().any(|c| c == connector))
    }

    pub fn get(&self, id: &str) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.id == id)
    }
}

/// The next fraction for a brightness key: a step of `step`, landing on
/// the step grid (as [`crate::power::stepped_brightness`] does), from 0 to 1.
/// A monitor's lowest level is still lit.
pub fn stepped(fraction: f32, step: f32) -> f32 {
    if !step.is_finite() || step == 0.0 || !fraction.is_finite() {
        return fraction;
    }
    let size = step.abs();
    let steps = fraction / size;
    let near = steps.round();
    let base = if (steps - near).abs() < 0.02 {
        near
    } else if step > 0.0 {
        steps.floor()
    } else {
        steps.ceil()
    };
    ((base + step.signum()) * size).clamp(0.0, 1.0)
}

/// `$XDG_STATE_HOME/aizome/monitor-brightness.json`, or under
/// `~/.local/state`.
pub fn remembered_path() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/")).join(".local/state")
    });
    state.join("aizome").join("monitor-brightness.json")
}

/// Where to look and what to do.
#[derive(Clone, Debug)]
pub struct Options {
    /// `/sys`.
    pub sys: PathBuf,
    /// `/dev`.
    pub dev: PathBuf,
    /// Set remembered levels again when a monitor that forgets its own
    /// appears.
    pub restore: bool,
    /// Where levels are remembered (none: nowhere).
    pub remembered: Option<PathBuf>,
    /// How often to check sysfs for changes.
    pub poll: Duration,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            sys: PathBuf::from("/sys"),
            dev: PathBuf::from("/dev"),
            restore: false,
            remembered: Some(remembered_path()),
            poll: Duration::from_secs(2),
        }
    }
}

/// External monitors' brightness.
pub struct Monitors {
    model: Arc<Model<MonitorsState>>,
    /// The worker's queue (none for a sample).
    tx: Option<mpsc::Sender<Cmd>>,
}

service_handle!(Monitors, MonitorsState);

impl Monitors {
    /// This machine's monitors.
    pub fn new(wake: Wake) -> Monitors {
        Monitors::with(Options::default(), wake)
    }

    /// This machine's monitors, setting remembered levels again as they
    /// appear: the shell's, one a session.
    pub fn restoring(wake: Wake) -> Monitors {
        Monitors::with(Options { restore: true, ..Options::default() }, wake)
    }

    pub fn with(opts: Options, wake: Wake) -> Monitors {
        let opener = Box::new(Devices { dev: opts.dev.clone() });
        Monitors::with_opener(opts, opener, wake)
    }

    pub(crate) fn with_opener(opts: Options, opener: Box<dyn Opener>, wake: Wake) -> Monitors {
        let model = Arc::new(Model::new(MonitorsState::default(), wake));
        let (tx, rx) = mpsc::channel();
        let worker = Worker::new(opts, opener, model.clone());
        let spawned = std::thread::Builder::new().name("aizome-sys monitors".into()).spawn(move || worker.run(rx));
        if let Err(e) = spawned {
            log::warn!("no thread for the monitors' brightness: {e}");
        }
        Monitors { model, tx: Some(tx) }
    }

    /// Made-up monitors that take levels in memory, for samples and
    /// tests.
    pub fn sample(monitors: Vec<Monitor>, wake: Wake) -> Monitors {
        Monitors::sample_state(MonitorsState { monitors, unavailable: Vec::new(), ready: true }, wake)
    }

    /// A made-up state, with displays that have no control too.
    pub fn sample_state(state: MonitorsState, wake: Wake) -> Monitors {
        Monitors { model: Arc::new(Model::new(state, wake)), tx: None }
    }

    /// Whether any monitor's brightness can be set.
    pub fn available(&self) -> bool {
        !self.model.get().monitors.is_empty()
    }

    /// Look at every monitor again: ask those that didn't answer, and read
    /// the levels (a monitor's buttons may have changed them).
    pub fn refresh(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Refresh);
        }
    }

    /// Set a monitor to a fraction of its range. The snapshot shows it at
    /// once; levels asked for before this one reached the monitor are
    /// skipped (their tasks finish too).
    pub fn set_fraction(&self, id: &str, fraction: f32) -> Task<()> {
        let Some(m) = self.model.get().get(id).cloned() else {
            return Task::ready(Err(Error::NotFound(id.to_owned())));
        };
        let level = m.level_for(fraction);
        self.show(id, level);
        let Some(tx) = &self.tx else { return Task::ready(Ok(())) };
        let (task, done) = Task::pending(self.model.wake());
        let _ = tx.send(Cmd::Set { id: id.to_owned(), level, done });
        task
    }

    /// The brightness keys: step a monitor by `step` (`0.05`, `-0.05`) on
    /// the step grid. The snapshot shows the new level at once; the
    /// result is the new fraction (from the monitor's own level when it
    /// hasn't been read lately).
    pub fn step(&self, id: &str, step: f32) -> Task<f32> {
        let Some(m) = self.model.get().get(id).cloned() else {
            return Task::ready(Err(Error::NotFound(id.to_owned())));
        };
        let fraction = stepped(m.fraction(), step);
        self.show(id, m.level_for(fraction));
        let Some(tx) = &self.tx else { return Task::ready(Ok(fraction)) };
        let (task, done) = Task::pending(self.model.wake());
        let _ = tx.send(Cmd::Step { id: id.to_owned(), step, done });
        task
    }

    /// Write the levels to remember now, rather than a second after the
    /// last change (for a program about to exit).
    pub fn flush(&self) -> Task<()> {
        let Some(tx) = &self.tx else { return Task::ready(Ok(())) };
        let (task, done) = Task::pending(self.model.wake());
        let _ = tx.send(Cmd::Flush(done));
        task
    }

    fn show(&self, id: &str, level: u32) {
        self.model.update(|s| {
            if let Some(m) = s.monitors.iter_mut().find(|m| m.id == id) {
                m.level = level;
            }
        });
    }
}

enum Cmd {
    Refresh,
    Flush(Done<()>),
    Set { id: String, level: u32, done: Done<()> },
    Step { id: String, step: f32, done: Done<f32> },
}

/// A Studio Display's HID interface, opened.
pub(crate) trait HidDevice {
    fn get_feature(&mut self, report: &mut [u8]) -> io::Result<usize>;
    fn set_feature(&mut self, report: &[u8]) -> io::Result<()>;
}

/// Opens the devices: `/dev`'s, or a test's.
pub(crate) trait Opener: Send {
    fn ddc(&self, bus: u32) -> io::Result<Box<dyn ddc::Bus>>;
    fn hid(&self, node: &str) -> io::Result<Box<dyn HidDevice>>;
}

struct Devices {
    dev: PathBuf,
}

struct HidFile(std::fs::File);

impl HidDevice for HidFile {
    fn get_feature(&mut self, report: &mut [u8]) -> io::Result<usize> {
        hid::get_feature(&self.0, report)
    }
    fn set_feature(&mut self, report: &[u8]) -> io::Result<()> {
        hid::set_feature(&self.0, report)
    }
}

impl Opener for Devices {
    fn ddc(&self, bus: u32) -> io::Result<Box<dyn ddc::Bus>> {
        Ok(Box::new(ddc::I2c::open(&self.dev.join(format!("i2c-{bus}")))?))
    }

    fn hid(&self, node: &str) -> io::Result<Box<dyn HidDevice>> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(self.dev.join(node))?;
        Ok(Box::new(HidFile(file)))
    }
}

/// What the worker knows of a monitor it can set.
struct Known {
    control: Control,
    field: Option<Field>,
    min: u32,
    max: u32,
    /// The level it last read or set, and when (none: read it again
    /// before stepping from it).
    level: u32,
    last_io: Option<Instant>,
}

/// A DDC/CI probe's answer, kept until the connector, its display or its
/// device's permissions change.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ProbeKey {
    sysfs: String,
    bus: u32,
    edid: u64,
    access: bool,
}

#[derive(Clone, Debug)]
enum Probe {
    Found { current: u16, max: u16 },
    Missing(Why),
}

struct Found {
    control: Control,
    field: Option<Field>,
    level: u32,
    min: u32,
    max: u32,
}

/// A level read this long ago is read again before a key steps from it.
const STALE: Duration = Duration::from_secs(5);
/// Remembered levels are written this long after the last change.
const WRITE_AFTER: Duration = Duration::from_secs(1);

struct Worker {
    opts: Options,
    opener: Box<dyn Opener>,
    model: Arc<Model<MonitorsState>>,
    known: HashMap<String, Known>,
    probes: HashMap<ProbeKey, Probe>,
    /// Monitors restored since they appeared.
    seen: HashSet<String>,
    /// Levels to remember, and when the last changed.
    unwritten: Vec<remembered::Entry>,
    changed: Option<Instant>,
    /// When each bus may take its next message.
    bus_ready: HashMap<u32, Instant>,
    sleep: fn(Duration),
}

fn io_why(path: &Path, e: &io::Error) -> Why {
    match e.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound => Why::NoAccess(path.display().to_string()),
        _ if e.raw_os_error() == Some(libc::EBUSY) => Why::Busy,
        _ => Why::Failed(e.to_string()),
    }
}

fn io_error(path: &Path, e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::PermissionDenied => Error::NotAuthorized(format!("{}: {e}", path.display())),
        io::ErrorKind::NotFound => Error::NotFound(path.display().to_string()),
        _ => Error::Io(format!("{}: {e}", path.display())),
    }
}

impl Worker {
    fn new(opts: Options, opener: Box<dyn Opener>, model: Arc<Model<MonitorsState>>) -> Worker {
        Worker {
            opts,
            opener,
            model,
            known: HashMap::new(),
            probes: HashMap::new(),
            seen: HashSet::new(),
            unwritten: Vec::new(),
            changed: None,
            bus_ready: HashMap::new(),
            sleep: std::thread::sleep,
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Cmd>) {
        let mut fingerprint = discover::fingerprint(&self.opts.sys, &self.opts.dev);
        self.rescan();
        let mut checked = Instant::now();
        loop {
            let mut wait = self.opts.poll.saturating_sub(checked.elapsed());
            if let Some(t) = self.changed {
                wait = wait.min(WRITE_AFTER.saturating_sub(t.elapsed()));
            }
            match rx.recv_timeout(wait) {
                Ok(first) => {
                    let mut batch = vec![first];
                    batch.extend(rx.try_iter());
                    self.batch(batch);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if checked.elapsed() >= self.opts.poll {
                checked = Instant::now();
                let now = discover::fingerprint(&self.opts.sys, &self.opts.dev);
                if now != fingerprint {
                    fingerprint = now;
                    self.rescan();
                }
            }
            if self.changed.is_some_and(|t| t.elapsed() >= WRITE_AFTER) {
                self.write_remembered();
            }
        }
        self.write_remembered();
    }

    /// Commands that came together: a refresh first, then sets and steps
    /// in order, each set skipped when a later one is for the same monitor,
    /// and steps in a row for one monitor (a key held down) sent as one level.
    fn batch(&mut self, batch: Vec<Cmd>) {
        if batch.iter().any(|c| matches!(c, Cmd::Refresh)) {
            self.probes.clear();
            for k in self.known.values_mut() {
                k.last_io = None;
            }
            self.rescan();
        }
        let mut last_set: HashMap<String, usize> = HashMap::new();
        for (i, c) in batch.iter().enumerate() {
            if let Cmd::Set { id, .. } = c {
                last_set.insert(id.clone(), i);
            }
        }
        let mut cmds = batch.into_iter().enumerate().peekable();
        while let Some((i, c)) = cmds.next() {
            match c {
                Cmd::Refresh => {}
                Cmd::Flush(done) => {
                    self.write_remembered();
                    done.finish(Ok(()));
                }
                Cmd::Set { id, done, .. } if last_set.get(&id) != Some(&i) => done.finish(Ok(())),
                Cmd::Set { id, level, done } => done.finish(self.apply(&id, level, true)),
                Cmd::Step { id, step, done } => {
                    let (mut steps, mut dones) = (vec![step], vec![done]);
                    while let Some((_, Cmd::Step { id: next, .. })) = cmds.peek()
                        && *next == id
                        && let Some((_, Cmd::Step { step, done, .. })) = cmds.next()
                    {
                        steps.push(step);
                        dones.push(done);
                    }
                    let result = self.step(&id, &steps);
                    for done in dones {
                        done.finish(result.clone());
                    }
                }
            }
        }
    }

    /// Look at sysfs and ask what's new.
    fn rescan(&mut self) {
        let scan = discover::scan(&self.opts.sys);
        let pairs = scan.pairs();
        let previous = self.model.get();
        let mut monitors = Vec::new();
        let mut unavailable = Vec::new();
        let mut keep = HashSet::new();
        for (g, group) in scan.groups.iter().enumerate() {
            let (make, model, serial) = group
                .edid()
                .map(|e| (e.make.clone(), e.model.clone(), e.serial.clone()))
                .unwrap_or_else(|| ("Unknown".into(), "Unknown".into(), String::new()));
            let connectors: Vec<String> = group.connectors.iter().map(|c| c.name.clone()).collect();
            let id = group.connectors[0].sysfs.clone();
            let found = match pairs[g] {
                Some(h) => self.studio(&scan.hid[h]),
                None if group.studio_display() => Err(if scan.unauthorized_display {
                    Why::NotAuthorized
                } else if scan.unpaired_hid() > 0 {
                    Why::Ambiguous
                } else {
                    Why::NoUsb
                }),
                None => self.ddc_probe(group, &mut keep),
            };
            match found {
                Ok(f) => {
                    // A level set since the probe is the level now.
                    let recent = |k: &Known| k.last_io.is_some_and(|t| t.elapsed() < STALE);
                    let level = match (self.known.get(&id), previous.get(&id)) {
                        (Some(k), Some(_)) if k.control == f.control && recent(k) => k.level,
                        _ => f.level,
                    };
                    let last_io = self.known.get(&id).map_or_else(|| Some(Instant::now()), |k| k.last_io);
                    self.known.insert(
                        id.clone(),
                        Known { control: f.control.clone(), field: f.field, min: f.min, max: f.max, level, last_io },
                    );
                    monitors.push(Monitor {
                        id,
                        connectors,
                        make,
                        model,
                        serial,
                        control: f.control,
                        level,
                        min: f.min,
                        max: f.max,
                    });
                }
                Err(why) => unavailable.push(Unavailable { connectors, make, model, why }),
            }
        }
        self.known.retain(|id, _| monitors.iter().any(|m| m.id == *id));
        self.probes.retain(|k, _| keep.contains(k));
        let state = MonitorsState { monitors, unavailable, ready: true };
        if *previous != state {
            for m in state.monitors.iter().filter(|m| previous.get(&m.id).is_none()) {
                log::info!(
                    "{} on {}: brightness over {} at {:.0}%",
                    m.title(),
                    m.connectors.join(", "),
                    m.control.through(),
                    m.fraction() * 100.0
                );
            }
            for u in state.unavailable.iter().filter(|u| !previous.unavailable.contains(u)) {
                log::info!("{} on {}: no brightness control ({:?})", u.title(), u.connectors.join(", "), u.why);
            }
        }
        self.model.set_if_changed(state);
        if self.opts.restore {
            self.restore();
        }
    }

    fn studio(&mut self, h: &HidDisplay) -> std::result::Result<Found, Why> {
        let path = self.opts.dev.join(&h.node);
        let mut d = self.opener.hid(&h.node).map_err(|e| io_why(&path, &e))?;
        let level = read_hid(d.as_mut(), &h.field).map_err(|e| io_why(&path, &e))?;
        let (min, max) = (h.field.min.max(0) as u32, h.field.max.max(0) as u32);
        Ok(Found {
            control: Control::StudioDisplay { node: h.node.clone(), product: h.product },
            field: Some(h.field),
            level: (level.max(0) as u32).clamp(min, max),
            min,
            max,
        })
    }

    fn ddc_probe(&mut self, group: &Group, keep: &mut HashSet<ProbeKey>) -> std::result::Result<Found, Why> {
        let mut why = Why::NoChannel;
        for c in &group.connectors {
            let Some(bus) = c.bus else { continue };
            let path = self.opts.dev.join(format!("i2c-{bus}"));
            let key = ProbeKey { sysfs: c.sysfs.clone(), bus, edid: c.edid_hash, access: discover::accessible(&path) };
            keep.insert(key.clone());
            let probe = match self.probes.get(&key) {
                Some(p) => p.clone(),
                None => {
                    let p = self.probe_bus(bus, &path);
                    self.probes.insert(key, p.clone());
                    p
                }
            };
            match probe {
                Probe::Found { current, max } => {
                    return Ok(Found {
                        control: Control::Ddc { bus },
                        field: None,
                        level: u32::from(current.min(max)),
                        min: 0,
                        max: u32::from(max),
                    });
                }
                // The first reason given is the connector's own.
                Probe::Missing(w) if why == Why::NoChannel => why = w,
                Probe::Missing(_) => {}
            }
        }
        Err(why)
    }

    fn probe_bus(&mut self, bus: u32, path: &Path) -> Probe {
        if !path.exists() {
            return Probe::Missing(Why::NoAccess(path.display().to_string()));
        }
        let mut d = match self.opener.ddc(bus) {
            Ok(d) => d,
            Err(e) => return Probe::Missing(io_why(path, &e)),
        };
        self.wait_for(bus);
        let answer = ddc::get(d.as_mut(), ddc::LUMINANCE, &self.sleep);
        self.bus_ready.insert(bus, Instant::now() + ddc::SET_DELAY);
        match answer {
            Ok(ddc::Reply::Value { current, max }) if max > 0 => Probe::Found { current, max },
            Ok(ddc::Reply::Value { .. } | ddc::Reply::Unsupported) => Probe::Missing(Why::NoLuminance),
            Ok(ddc::Reply::Null) | Err(_) => Probe::Missing(Why::NoDdc),
        }
    }

    fn wait_for(&self, bus: u32) {
        if let Some(t) = self.bus_ready.get(&bus) {
            let left = t.saturating_duration_since(Instant::now());
            if !left.is_zero() {
                (self.sleep)(left);
            }
        }
    }

    /// Read a monitor's level from it.
    fn read(&mut self, id: &str) -> Result<u32> {
        let k = self.known.get(id).ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let (control, field) = (k.control.clone(), k.field);
        let level = match &control {
            Control::Ddc { bus } => {
                let path = self.opts.dev.join(format!("i2c-{bus}"));
                let mut d = self.opener.ddc(*bus).map_err(|e| io_error(&path, e))?;
                self.wait_for(*bus);
                let answer = ddc::get(d.as_mut(), ddc::LUMINANCE, &self.sleep);
                self.bus_ready.insert(*bus, Instant::now() + ddc::SET_DELAY);
                match answer.map_err(|e| io_error(&path, e))? {
                    ddc::Reply::Value { current, .. } => u32::from(current),
                    _ => return Err(Error::Failed("no clear answer over DDC/CI".into())),
                }
            }
            Control::StudioDisplay { node, .. } => {
                let path = self.opts.dev.join(node);
                let field = field.ok_or_else(|| Error::Failed("no brightness field".into()))?;
                let mut d = self.opener.hid(node).map_err(|e| io_error(&path, e))?;
                read_hid(d.as_mut(), &field).map_err(|e| io_error(&path, e))?.max(0) as u32
            }
            Control::Sample => self.known[id].level,
        };
        if let Some(k) = self.known.get_mut(id) {
            k.level = level.clamp(k.min, k.max);
            k.last_io = Some(Instant::now());
        }
        Ok(level)
    }

    /// Set a monitor's level; `remember` when it's the person's choice
    /// (not a restore).
    fn apply(&mut self, id: &str, level: u32, remember: bool) -> Result<()> {
        let k = self.known.get(id).ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let level = level.clamp(k.min, k.max);
        let (control, field, before) = (k.control.clone(), k.field, k.level);
        let result = match &control {
            Control::Ddc { bus } => {
                let path = self.opts.dev.join(format!("i2c-{bus}"));
                self.wait_for(*bus);
                let sent = self
                    .opener
                    .ddc(*bus)
                    .and_then(|mut d| ddc::set(d.as_mut(), ddc::LUMINANCE, level.min(u32::from(u16::MAX)) as u16));
                self.bus_ready.insert(*bus, Instant::now() + ddc::SET_DELAY);
                sent.map_err(|e| io_error(&path, e))
            }
            Control::StudioDisplay { node, .. } => {
                let path = self.opts.dev.join(node);
                let field = field.ok_or_else(|| Error::Failed("no brightness field".into()))?;
                self.opener
                    .hid(node)
                    .and_then(|mut d| d.set_feature(&field.report(i64::from(level))))
                    .map_err(|e| io_error(&path, e))
            }
            Control::Sample => Ok(()),
        };
        let shown = if result.is_ok() { level } else { before };
        if let Some(k) = self.known.get_mut(id) {
            k.level = shown;
            k.last_io = Some(Instant::now());
        }
        self.model.update(|s| {
            if let Some(m) = s.monitors.iter_mut().find(|m| m.id == id) {
                m.level = shown;
            }
        });
        if result.is_ok()
            && remember
            && control.remembered()
            && let Some(m) = self.model.get().get(id)
        {
            self.unwritten.retain(|e| !e.is(&m.make, &m.model, &m.serial));
            self.unwritten.push(remembered::Entry {
                make: m.make.clone(),
                model: m.model.clone(),
                serial: m.serial.clone(),
                level: m.fraction(),
            });
            self.changed = Some(Instant::now());
        }
        result
    }

    /// Step a monitor by each of `steps` in turn, and set the level once.
    fn step(&mut self, id: &str, steps: &[f32]) -> Result<f32> {
        let k = self.known.get(id).ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let stale = k.last_io.is_none_or(|t| t.elapsed() >= STALE);
        if stale {
            // Changed elsewhere (its buttons, another program) since?
            if let Err(e) = self.read(id) {
                log::debug!("reading {id} before a step: {e}");
            }
        }
        let k = &self.known[id];
        let (min, max) = (k.min, k.max);
        let next = steps.iter().fold(fraction(k.level, min, max), |f, s| stepped(f, *s));
        self.apply(id, level_for(next, min, max), true)?;
        Ok(next)
    }

    /// Set remembered levels again on monitors that just appeared.
    fn restore(&mut self) {
        let state = self.model.get();
        self.seen.retain(|id| state.get(id).is_some());
        let fresh: Vec<&Monitor> =
            state.monitors.iter().filter(|m| m.control.remembered() && !self.seen.contains(&m.id)).collect();
        if fresh.is_empty() {
            return;
        }
        let saved = self.opts.remembered.as_deref().map(remembered::load).unwrap_or_default();
        for m in fresh {
            self.seen.insert(m.id.clone());
            let Some(e) = saved.iter().find(|e| e.is(&m.make, &m.model, &m.serial)) else { continue };
            let level = m.level_for(e.level);
            if level != m.level {
                log::info!("{}: back to {:.0}%, as it was left", m.title(), e.level * 100.0);
                if let Err(err) = self.apply(&m.id, level, false) {
                    log::warn!("{}: setting its brightness again: {err}", m.title());
                }
            }
        }
    }

    fn write_remembered(&mut self) {
        self.changed = None;
        let (Some(path), false) = (self.opts.remembered.clone(), self.unwritten.is_empty()) else { return };
        // Read again, so another process's levels aren't lost.
        let mut saved = remembered::load(&path);
        for e in self.unwritten.drain(..) {
            saved.retain(|s| !s.is(&e.make, &e.model, &e.serial));
            saved.push(e);
        }
        if let Err(e) = remembered::save(&path, &saved) {
            log::warn!("{}: {e}", path.display());
        }
    }
}

fn read_hid(d: &mut dyn HidDevice, field: &Field) -> io::Result<i64> {
    let mut report = vec![0u8; field.report_len];
    if field.numbered {
        report[0] = field.report_id;
    }
    d.get_feature(&mut report)?;
    field.read(&report).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "a short feature report"))
}

/// The levels Aizome keeps for monitors that don't keep their own.
mod remembered {
    use serde::{Deserialize, Serialize};
    use std::path::Path;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct Entry {
        pub make: String,
        pub model: String,
        pub serial: String,
        /// A fraction of its range, 0–1.
        pub level: f32,
    }

    impl Entry {
        pub fn is(&self, make: &str, model: &str, serial: &str) -> bool {
            self.make == make && self.model == model && self.serial == serial
        }
    }

    /// The file's entries (none when it's missing or broken).
    pub fn load(path: &Path) -> Vec<Entry> {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(entries)?)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::discover::tests::{Fake, ganymede};
    use super::edid::tests::CONTAINER;
    use super::*;
    use std::sync::Mutex;

    /// A Studio Display's interface 7 at the other end.
    struct FakeHid {
        level: u32,
        sets: Vec<Vec<u8>>,
    }

    #[derive(Default)]
    struct FakeDevices {
        ddc: HashMap<u32, Arc<Mutex<ddc::tests::Display>>>,
        hid: HashMap<String, Arc<Mutex<FakeHid>>>,
        denied: HashSet<String>,
    }

    struct Shared<T>(Arc<Mutex<T>>);

    impl ddc::Bus for Shared<ddc::tests::Display> {
        fn write(&mut self, m: &[u8]) -> io::Result<()> {
            self.0.lock().unwrap().write(m)
        }
        fn read(&mut self, b: &mut [u8]) -> io::Result<()> {
            self.0.lock().unwrap().read(b)
        }
    }

    impl HidDevice for Shared<FakeHid> {
        fn get_feature(&mut self, r: &mut [u8]) -> io::Result<usize> {
            r[1..5].copy_from_slice(&self.0.lock().unwrap().level.to_le_bytes());
            Ok(r.len())
        }
        fn set_feature(&mut self, r: &[u8]) -> io::Result<()> {
            let mut h = self.0.lock().unwrap();
            h.sets.push(r.to_vec());
            h.level = u32::from_le_bytes(r[1..5].try_into().unwrap());
            Ok(())
        }
    }

    impl Opener for Arc<FakeDevices> {
        fn ddc(&self, bus: u32) -> io::Result<Box<dyn ddc::Bus>> {
            if self.denied.contains(&format!("i2c-{bus}")) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            match self.ddc.get(&bus) {
                Some(d) => Ok(Box::new(Shared(d.clone()))),
                None => Ok(Box::new(ddc::tests::Nobody)),
            }
        }
        fn hid(&self, node: &str) -> io::Result<Box<dyn HidDevice>> {
            if self.denied.contains(node) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            let h = self.hid.get(node).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            Ok(Box::new(Shared(h.clone())))
        }
    }

    /// ganymede's displays, the BenQ answering DDC/CI at 30 of 100 and
    /// the Studio Display's USB connected at 30000 (296 nits).
    type Setup = (Fake, Arc<FakeDevices>, Arc<Mutex<ddc::tests::Display>>, Arc<Mutex<FakeHid>>);

    fn setup(name: &str) -> Setup {
        let f = Fake::new(name);
        ganymede(&f);
        f.studio_hid("3-1.3", 6, 0x1114, Some(CONTAINER));
        let benq = Arc::new(Mutex::new(ddc::tests::Display::new(30, 100)));
        let studio = Arc::new(Mutex::new(FakeHid { level: 30000, sets: Vec::new() }));
        let mut d = FakeDevices::default();
        d.ddc.insert(9, benq.clone());
        d.hid.insert("hidraw7".into(), studio.clone());
        (f, Arc::new(d), benq, studio)
    }

    fn worker(f: &Fake, devices: &Arc<FakeDevices>, restore: bool) -> Worker {
        let opts = Options {
            sys: f.sys(),
            dev: f.dev(),
            restore,
            remembered: Some(f.root.join("state/monitor-brightness.json")),
            poll: Duration::from_secs(2),
        };
        let model = Arc::new(Model::new(MonitorsState::default(), Wake::none()));
        let mut w = Worker::new(opts, Box::new(devices.clone()), model);
        w.sleep = |_| {};
        w
    }

    #[test]
    fn ganymedes_monitors_one_control_each() {
        let (f, devices, _, _) = setup("ganymede");
        let mut w = worker(&f, &devices, false);
        w.rescan();
        let s = w.model.get();
        assert!(s.ready);
        assert_eq!(s.monitors.len(), 2, "{s:#?}");
        let studio = s.on("DP-2").expect("the Studio Display, on either tile");
        assert_eq!(studio.connectors, vec!["DP-1", "DP-2"]);
        assert_eq!(studio.control, Control::StudioDisplay { node: "hidraw7".into(), product: 0x1114 });
        assert_eq!((studio.min, studio.max, studio.level), (400, 60000, 30000));
        assert_eq!(studio.title(), "APP StudioDisplay");
        let benq = s.on("HDMI-A-1").unwrap();
        assert_eq!(benq.control, Control::Ddc { bus: 9 });
        assert!((benq.fraction() - 0.3).abs() < 1e-6);
        assert_eq!(benq.title(), "BNQ BenQ RD280U");
        assert!(s.on("eDP-1").is_none(), "the panel is the backlight's");
        assert!(s.unavailable.is_empty());
    }

    #[test]
    fn a_dragged_slider_sends_its_last_level() {
        let (f, devices, benq, _) = setup("drag");
        let mut w = worker(&f, &devices, false);
        w.rescan();
        let id = w.model.get().on("HDMI-A-1").unwrap().id.clone();
        let mut tasks = Vec::new();
        let batch: Vec<Cmd> = [40, 50, 60]
            .into_iter()
            .map(|level| {
                let (task, done) = Task::pending(Wake::none());
                tasks.push(task);
                Cmd::Set { id: id.clone(), level, done }
            })
            .collect();
        let before = benq.lock().unwrap().written.len();
        w.batch(batch);
        let b = benq.lock().unwrap();
        assert_eq!(b.written.len() - before, 1, "one message on the bus");
        assert_eq!(b.current, 60);
        assert!(tasks.into_iter().all(|t| t.wait() == Ok(())), "every task finishes");
        assert_eq!(w.model.get().get(&id).unwrap().level, 60);
    }

    #[test]
    fn displays_without_a_control_say_why() {
        let f = Fake::new("why");
        ganymede(&f);
        f.thunderbolt(false);
        // No Studio Display HID; the BenQ has DDC/CI without luminance.
        let mut benq = ddc::tests::Display::new(0, 100);
        benq.has_luminance = false;
        let mut d = FakeDevices::default();
        d.ddc.insert(9, Arc::new(Mutex::new(benq)));
        let devices = Arc::new(d);
        let mut w = worker(&f, &devices, false);
        w.rescan();
        let s = w.model.get();
        assert!(s.monitors.is_empty());
        assert_eq!(s.unavailable_on("DP-1").unwrap().why, Why::NotAuthorized);
        assert_eq!(s.unavailable_on("HDMI-A-1").unwrap().why, Why::NoLuminance);
        // Nobody at 0x37, then a device that can't be opened.
        let f = Fake::new("why2");
        ganymede(&f);
        let devices = Arc::new(FakeDevices::default());
        let mut w = worker(&f, &devices, false);
        w.rescan();
        assert_eq!(w.model.get().unavailable_on("HDMI-A-1").unwrap().why, Why::NoDdc);
        assert_eq!(w.model.get().unavailable_on("DP-1").unwrap().why, Why::NoUsb);
        let mut d = FakeDevices::default();
        d.denied.insert("i2c-9".into());
        let mut w = worker(&f, &Arc::new(d), false);
        w.rescan();
        let path = f.dev().join("i2c-9").display().to_string();
        assert_eq!(w.model.get().unavailable_on("HDMI-A-1").unwrap().why, Why::NoAccess(path));
        // Without i2c-dev there's no /dev/i2c-9 at all.
        std::fs::remove_file(f.dev().join("i2c-9")).unwrap();
        let mut w = worker(&f, &Arc::new(FakeDevices::default()), false);
        w.rescan();
        assert!(matches!(w.model.get().unavailable_on("HDMI-A-1").unwrap().why, Why::NoAccess(_)));
        assert!(Why::NoDdc.explain().contains("DDC/CI"));
    }

    #[test]
    fn a_probe_isnt_repeated_until_something_changes() {
        let (f, devices, benq, _) = setup("cache");
        let mut w = worker(&f, &devices, false);
        w.rescan();
        let asked = benq.lock().unwrap().written.len();
        w.rescan();
        assert_eq!(benq.lock().unwrap().written.len(), asked, "the answer was kept");
        w.batch(vec![Cmd::Refresh]);
        assert!(benq.lock().unwrap().written.len() > asked, "a refresh asks again");
    }

    #[test]
    fn keys_step_and_read_a_stale_level_first() {
        let (f, devices, benq, _) = setup("step");
        let mut w = worker(&f, &devices, false);
        w.rescan();
        let id = w.model.get().on("HDMI-A-1").unwrap().id.clone();
        assert!((w.step(&id, &[0.05]).unwrap() - 0.35).abs() < 1e-6);
        assert_eq!(benq.lock().unwrap().current, 35);
        // Its buttons took it to 80 a while ago.
        benq.lock().unwrap().current = 80;
        w.known.get_mut(&id).unwrap().last_io = None;
        assert!((w.step(&id, &[-0.05]).unwrap() - 0.75).abs() < 1e-6);
        assert_eq!(benq.lock().unwrap().current, 75);
        // A key held down: three steps queued behind one go out as one level.
        let before = benq.lock().unwrap().written.len();
        let batch: Vec<Cmd> =
            (0..3).map(|_| Cmd::Step { id: id.clone(), step: 0.05, done: Task::pending(Wake::none()).1 }).collect();
        w.batch(batch);
        let b = benq.lock().unwrap();
        assert_eq!(b.written.len() - before, 1, "one message on the bus");
        assert_eq!(b.current, 90);
        drop(b);
        assert!((stepped(0.48, 0.05) - 0.5).abs() < 1e-6);
        assert!((stepped(0.02, -0.05) - 0.0).abs() < 1e-6, "a monitor's least is still lit");
        assert_eq!(stepped(1.0, 0.05), 1.0);
    }

    #[test]
    fn a_studio_displays_level_is_remembered_and_set_again() {
        let (f, devices, benq, studio) = setup("remember");
        let mut w = worker(&f, &devices, false);
        w.rescan();
        let id = w.model.get().on("DP-1").unwrap().id.clone();
        let level = w.model.get().get(&id).unwrap().level_for(0.8);
        w.apply(&id, level, true).unwrap();
        assert_eq!(
            studio.lock().unwrap().sets.last().unwrap(),
            &Field::report(&w.known[&id].field.unwrap(), level as i64)
        );
        let benq_id = w.model.get().on("HDMI-A-1").unwrap().id.clone();
        w.apply(&benq_id, 90, true).unwrap();
        w.write_remembered();
        let saved = remembered::load(w.opts.remembered.as_ref().unwrap());
        assert_eq!(saved.len(), 1, "only the Studio Display's: the BenQ keeps its own");
        assert_eq!((saved[0].model.as_str(), saved[0].serial.as_str()), ("StudioDisplay", "305419896"));
        assert!((saved[0].level - 0.8).abs() < 1e-3);
        // It lost power and came back dim; the shell's handle sets it again.
        studio.lock().unwrap().level = 400;
        let mut shell = worker(&f, &devices, true);
        shell.rescan();
        assert_eq!(studio.lock().unwrap().level, level);
        assert!((shell.model.get().get(&id).unwrap().fraction() - 0.8).abs() < 1e-3);
        // Only as it appears: dimmed while it's here, it stays dimmed.
        studio.lock().unwrap().level = 1000;
        shell.batch(vec![Cmd::Refresh]);
        assert_eq!(studio.lock().unwrap().level, 1000);
        // A handle that doesn't restore never sets it; nor is the BenQ's
        // level touched by any of this.
        let mut settings = worker(&f, &devices, false);
        studio.lock().unwrap().level = 500;
        settings.rescan();
        assert_eq!(studio.lock().unwrap().level, 500);
        assert_eq!(benq.lock().unwrap().current, 90);
    }

    #[test]
    fn a_service_handle_finds_them_on_its_thread() {
        let (f, devices, benq, _) = setup("thread");
        let opts =
            Options { sys: f.sys(), dev: f.dev(), restore: false, remembered: None, poll: Duration::from_millis(50) };
        let m = Monitors::with_opener(opts, Box::new(devices.clone()), Wake::none());
        let ready = || m.snapshot().ready;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(m.available());
        let id = m.snapshot().on("HDMI-A-1").unwrap().id.clone();
        let task = m.set_fraction(&id, 0.5);
        assert_eq!(m.snapshot().get(&id).unwrap().level, 50, "shown at once");
        assert_eq!(task.wait_timeout(Duration::from_secs(10)), Some(Ok(())));
        assert_eq!(benq.lock().unwrap().current, 50);
        assert!(m.set_fraction("card9-DP-9", 0.5).wait().is_err());
    }

    #[test]
    fn samples_take_levels_in_memory() {
        let m = Monitors::sample(
            vec![Monitor {
                id: "card0-DP-2".into(),
                connectors: vec!["DP-2".into()],
                make: "ASUSTek COMPUTER INC".into(),
                model: "PG27UCDM".into(),
                serial: String::new(),
                control: Control::Sample,
                level: 48,
                min: 0,
                max: 100,
            }],
            Wake::none(),
        );
        assert_eq!(m.set_fraction("card0-DP-2", 0.7).wait(), Ok(()));
        assert_eq!(m.snapshot().on("DP-2").unwrap().level, 70);
        assert!((m.step("card0-DP-2", 0.05).wait().unwrap() - 0.75).abs() < 1e-6);
        assert_eq!(m.snapshot().on("DP-2").unwrap().title(), "ASUSTek PG27UCDM");
    }
}
