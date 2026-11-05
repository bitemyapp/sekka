//! Game controllers, read straight from their event devices on a thread of
//! their own.
//!
//! - **Found as they come**: `/dev/input` is watched, so a controller
//!   switched on (Bluetooth reconnects a paired one by itself) is there at
//!   once, as player 1, 2, 3 or 4. A controller that comes back keeps its
//!   number.
//! - **Held exclusively**: while Chanoma runs it grabs each controller, so
//!   a browser's Gamepad API doesn't also act on presses Chanoma turns into
//!   keys for it.
//! - **By position**: buttons are named by where they are (`South` is the
//!   bottom face button), whatever the controller prints on them; see
//!   [`mapping`].
//! - **Menus** hear a direction from the arrows or the stick that repeats
//!   while it's held ([`nav`]). A controller without a Home button gets one
//!   by holding Select and Start together for a second.
//! - **Games** read [`Shared`], which the thread keeps current, without
//!   waiting for messages.

pub mod evdev;
pub mod mapping;
pub mod nav;

use std::collections::{HashMap, HashSet};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use evdev::{AbsInfo, Caps, Device, EV_ABS, EV_KEY, EV_SYN, InputEvent, SYN_DROPPED, SYN_REPORT};
use mapping::{Database, Input, Mapping, Output, Source};
pub use nav::Dir;

/// A button, by where it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Button {
    #[default]
    South,
    East,
    West,
    North,
    L1,
    R1,
    /// The triggers, pressed past half way.
    L2,
    R2,
    Select,
    Start,
    Guide,
    L3,
    R3,
    Up,
    Down,
    Left,
    Right,
}

impl Button {
    pub const ALL: [Button; 17] = [
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::L1,
        Button::R1,
        Button::L2,
        Button::R2,
        Button::Select,
        Button::Start,
        Button::Guide,
        Button::L3,
        Button::R3,
        Button::Up,
        Button::Down,
        Button::Left,
        Button::Right,
    ];

    pub fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// An axis: sticks −1 to 1 (right and down positive), triggers 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    L2,
    R2,
}

impl Axis {
    pub const ALL: [Axis; 6] = [Axis::LeftX, Axis::LeftY, Axis::RightX, Axis::RightY, Axis::L2, Axis::R2];

    fn trigger(self) -> bool {
        matches!(self, Axis::L2 | Axis::R2)
    }
}

/// Whose controller it is, for its buttons' names and colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    Xbox,
    PlayStation,
    Nintendo,
    #[default]
    Generic,
    /// No controller: the keyboard, as hints show its keys.
    Keyboard,
}

impl Family {
    pub fn of(caps: &Caps) -> Family {
        let name = caps.name.to_ascii_lowercase();
        match caps.id.vendor {
            0x045e => return Family::Xbox,
            0x054c => return Family::PlayStation,
            0x057e => return Family::Nintendo,
            _ => {}
        }
        if caps.driver == "xpad" || name.contains("xbox") || name.contains("x-box") {
            Family::Xbox
        } else if name.contains("dualsense")
            || name.contains("dualshock")
            || name.contains("ps4")
            || name.contains("ps5")
        {
            Family::PlayStation
        } else if name.contains("pro controller") || name.contains("joy-con") || name.contains("nintendo")
            // 8BitDo's are lettered as Nintendo's are.
            || caps.id.vendor == 0x2dc8
        {
            Family::Nintendo
        } else {
            Family::Generic
        }
    }
}

/// Players.
pub const SLOTS: usize = 4;

/// A controller as the program sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct PadInfo {
    /// This connection's number.
    pub id: u32,
    /// The player: 0 is player 1.
    pub slot: usize,
    pub name: String,
    pub family: Family,
    pub bluetooth: bool,
    /// SDL's identifier, for setting its buttons up.
    pub guid: String,
    /// Percent, and whether it's charging.
    pub battery: Option<(u8, bool)>,
    pub source: Source,
    pub rumble: bool,
}

/// What the controllers did.
#[derive(Clone, Debug, PartialEq)]
pub enum PadEvent {
    Connected(PadInfo),
    Disconnected {
        id: u32,
        slot: usize,
    },
    Button {
        id: u32,
        slot: usize,
        button: Button,
        pressed: bool,
    },
    /// A direction for menus: started, or again while held.
    Nav {
        id: u32,
        slot: usize,
        dir: Dir,
        repeat: bool,
    },
    Battery {
        id: u32,
        battery: Option<(u8, bool)>,
    },
    /// While setting a controller up: an input moved far enough to mean it.
    Raw {
        id: u32,
        input: Input,
    },
}

/// What games read, kept current by the thread: each player's buttons
/// (`Button::bit`) and axes (−32767 to 32767; triggers 0 to 32767).
#[derive(Default)]
pub struct Shared {
    slots: [SlotState; SLOTS],
}

#[derive(Default)]
struct SlotState {
    connected: AtomicBool,
    buttons: AtomicU32,
    axes: [AtomicI32; 6],
}

impl Shared {
    pub fn connected(&self, slot: usize) -> bool {
        self.slots.get(slot).is_some_and(|s| s.connected.load(Ordering::Acquire))
    }

    pub fn buttons(&self, slot: usize) -> u32 {
        self.slots.get(slot).map_or(0, |s| s.buttons.load(Ordering::Acquire))
    }

    pub fn axis(&self, slot: usize, axis: Axis) -> i16 {
        self.slots.get(slot).map_or(0, |s| s.axes[axis as usize].load(Ordering::Acquire) as i16)
    }

    /// Set a player's state (the thread, and tests).
    pub fn set(&self, slot: usize, connected: bool, buttons: u32, axes: [f32; 6]) {
        let Some(s) = self.slots.get(slot) else { return };
        s.buttons.store(buttons, Ordering::Release);
        for (a, v) in s.axes.iter().zip(axes) {
            a.store((v.clamp(-1.0, 1.0) * 32767.0) as i32, Ordering::Release);
        }
        s.connected.store(connected, Ordering::Release);
    }
}

/// A controller's state from its events.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PadState {
    /// Buttons from buttons.
    direct: u32,
    /// Axes from axes.
    axes: [f32; 6],
    /// Axes from buttons: each axis's positive and negative push.
    halves: [[f32; 2]; 6],
    hats: [(i32, i32); 4],
    /// Raw values, by code, for axes mapped to buttons.
    abs: HashMap<u16, f32>,
}

/// −1 to 1 from an axis's range, with a dead zone in the middle.
fn stick(v: i32, info: &AbsInfo) -> f32 {
    let half = (f64::from(info.maximum) - f64::from(info.minimum)) / 2.0;
    if half <= 0.0 {
        return 0.0;
    }
    let centre = (f64::from(info.maximum) + f64::from(info.minimum)) / 2.0;
    let x = ((f64::from(v) - centre) / half).clamp(-1.0, 1.0);
    let dead = (f64::from(info.flat) / half).clamp(0.10, 0.30);
    if x.abs() < dead { 0.0 } else { (x.signum() * (x.abs() - dead) / (1.0 - dead)) as f32 }
}

/// 0 to 1 from a trigger's range.
fn trigger(v: i32, info: &AbsInfo) -> f32 {
    let span = f64::from(info.maximum) - f64::from(info.minimum);
    if span <= 0.0 {
        return 0.0;
    }
    let x = ((f64::from(v) - f64::from(info.minimum)) / span).clamp(0.0, 1.0);
    if x < 0.04 { 0.0 } else { x as f32 }
}

impl PadState {
    /// Take one event: whether anything changed.
    pub fn feed(&mut self, ev: &InputEvent, caps: &Caps, mapping: &Mapping) -> bool {
        let mut changed = false;
        match ev.kind {
            EV_KEY => {
                let down = ev.value != 0;
                for out in mapping.output_of_key(ev.code) {
                    changed = true;
                    match out {
                        Output::Button(b) => set_bit(&mut self.direct, b, down),
                        Output::HalfAxis(a, sign) => self.halves[a as usize][usize::from(sign < 0)] = f32::from(down),
                        Output::Axis(a) => self.axes[a as usize] = f32::from(down),
                    }
                }
            }
            EV_ABS if (evdev::ABS_HAT0X..=evdev::ABS_HAT3Y).contains(&ev.code) => {
                let hat = usize::from((ev.code - evdev::ABS_HAT0X) / 2);
                let v = ev.value.signum();
                let h = &mut self.hats[hat];
                if (ev.code - evdev::ABS_HAT0X).is_multiple_of(2) {
                    h.0 = v
                } else {
                    h.1 = v
                }
                changed = true;
                for (input, out) in &mapping.binds {
                    if let (Input::Hat { hat: n, mask }, Output::Button(b)) = (input, out)
                        && usize::from(*n) == hat
                    {
                        let on = match mask {
                            1 => h.1 < 0,
                            2 => h.0 > 0,
                            4 => h.1 > 0,
                            8 => h.0 < 0,
                            _ => false,
                        };
                        set_bit(&mut self.direct, *b, on);
                    }
                }
            }
            EV_ABS => {
                let Some(info) = caps.range(ev.code) else { return false };
                for (input, out) in &mapping.binds {
                    let Input::Abs { code, half, invert } = *input else { continue };
                    if code != ev.code {
                        continue;
                    }
                    changed = true;
                    let full = stick(ev.value, &info);
                    let full = if invert { -full } else { full };
                    let part = match half {
                        1 => full.max(0.0),
                        -1 => (-full).max(0.0),
                        _ => full,
                    };
                    self.abs.insert(code, part);
                    match *out {
                        Output::Axis(a) if a.trigger() && half == 0 => {
                            let t = trigger(ev.value, &info);
                            self.axes[a as usize] = if invert { 1.0 - t } else { t };
                        }
                        Output::Axis(a) if a.trigger() => self.axes[a as usize] = part,
                        Output::Axis(a) => self.axes[a as usize] = part,
                        Output::HalfAxis(a, sign) => self.halves[a as usize][usize::from(sign < 0)] = part,
                        Output::Button(b) => set_bit(&mut self.direct, b, part > 0.5),
                    }
                }
            }
            _ => {}
        }
        changed
    }

    /// The axes, halves pushed in.
    pub fn axes(&self) -> [f32; 6] {
        let mut out = self.axes;
        for (i, h) in self.halves.iter().enumerate() {
            if h[0] != 0.0 || h[1] != 0.0 {
                out[i] = h[0] - h[1];
            }
        }
        out
    }

    /// The buttons, triggers past half way among them.
    pub fn buttons(&self) -> u32 {
        let axes = self.axes();
        let mut b = self.direct;
        if axes[Axis::L2 as usize] > 0.45 {
            b |= Button::L2.bit();
        }
        if axes[Axis::R2 as usize] > 0.45 {
            b |= Button::R2.bit();
        }
        b
    }

    /// The arrows pressed: the first of up, down, left, right.
    pub fn arrows(&self) -> Option<Dir> {
        let b = self.buttons();
        [(Button::Up, Dir::Up), (Button::Down, Dir::Down), (Button::Left, Dir::Left), (Button::Right, Dir::Right)]
            .into_iter()
            .find(|(btn, _)| b & btn.bit() != 0)
            .map(|(_, d)| d)
    }
}

fn set_bit(bits: &mut u32, b: Button, on: bool) {
    if on {
        *bits |= b.bit();
    } else {
        *bits &= !b.bit();
    }
}

/// How long Select and Start together make Home.
pub const HOME_CHORD: Duration = Duration::from_millis(1000);
/// How often batteries are read.
const BATTERY_EVERY: Duration = Duration::from_secs(30);
/// How often devices that couldn't be opened yet are tried again.
const RETRY_EVERY: Duration = Duration::from_secs(2);

enum Cmd {
    Grab(bool),
    Rumble { slot: usize, strong: u16, weak: u16, ms: u16 },
    Raw(bool),
    Layouts(HashMap<String, Mapping>),
    Stop,
}

/// The controllers' thread.
pub struct Pads {
    pub shared: Arc<Shared>,
    control: Control,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// What other threads tell the controllers' thread (a game's rumble).
#[derive(Clone)]
pub struct Control {
    cmds: Arc<Mutex<Vec<Cmd>>>,
    wake: Option<Arc<std::os::fd::OwnedFd>>,
}

impl Control {
    fn send(&self, cmd: Cmd) {
        self.cmds.lock().unwrap_or_else(|e| e.into_inner()).push(cmd);
        if let Some(w) = &self.wake {
            // SAFETY: one byte to the pipe's write end.
            unsafe { libc::write(w.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
        }
    }

    /// Rumble player `slot`'s controller for `ms` (0: until told again).
    pub fn rumble(&self, slot: usize, strong: u16, weak: u16, ms: u16) {
        self.send(Cmd::Rumble { slot, strong, weak, ms });
    }
}

/// What the thread starts with.
pub struct Config {
    /// Where event devices are.
    pub dir: PathBuf,
    /// SDL's database.
    pub database: Database,
    /// Layouts set up in Chanoma, by guid.
    pub layouts: HashMap<String, Mapping>,
    /// Grab controllers from the start.
    pub grab: bool,
}

impl Pads {
    /// Start reading controllers, telling `send` what they do.
    pub fn start(config: Config, send: impl Fn(PadEvent) + Send + 'static) -> Pads {
        let shared = Arc::new(Shared::default());
        let cmds = Arc::new(Mutex::new(Vec::new()));
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors, owned below.
        let piped = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } == 0;
        let (read, write) = if piped {
            // SAFETY: just made, owned here alone.
            unsafe {
                use std::os::fd::FromRawFd;
                (Some(std::os::fd::OwnedFd::from_raw_fd(fds[0])), Some(std::os::fd::OwnedFd::from_raw_fd(fds[1])))
            }
        } else {
            (None, None)
        };
        let worker = Worker::new(config, shared.clone(), cmds.clone(), read, Box::new(send));
        let thread = std::thread::Builder::new().name("chanoma-pads".into()).spawn(move || worker.run()).ok();
        Pads { shared, control: Control { cmds, wake: write.map(Arc::new) }, thread }
    }

    fn send(&self, cmd: Cmd) {
        self.control.send(cmd);
    }

    /// A handle for other threads.
    pub fn control(&self) -> Control {
        self.control.clone()
    }

    /// Take the controllers from other programs, or give them back.
    pub fn grab(&self, on: bool) {
        self.send(Cmd::Grab(on));
    }

    /// Rumble player `slot`'s controller for `ms`.
    pub fn rumble(&self, slot: usize, strong: u16, weak: u16, ms: u16) {
        self.send(Cmd::Rumble { slot, strong, weak, ms });
    }

    /// Report every input moved (setting a controller up).
    pub fn raw(&self, on: bool) {
        self.send(Cmd::Raw(on));
    }

    /// Use these layouts (a controller was just set up).
    pub fn set_layouts(&self, layouts: HashMap<String, Mapping>) {
        self.send(Cmd::Layouts(layouts));
    }
}

impl Drop for Pads {
    fn drop(&mut self) {
        self.send(Cmd::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Pad {
    id: u32,
    slot: usize,
    dev: Device,
    mapping: Mapping,
    state: PadState,
    published: (u32, [f32; 6]),
    repeater: nav::Repeater,
    chord_since: Option<Instant>,
    battery: Option<(u8, bool)>,
}

impl Pad {
    fn info(&self) -> PadInfo {
        PadInfo {
            id: self.id,
            slot: self.slot,
            name: self.dev.caps.name.clone(),
            family: Family::of(&self.dev.caps),
            bluetooth: self.dev.caps.bluetooth(),
            guid: mapping::guid(&self.dev.caps.id),
            battery: self.battery,
            source: self.mapping.source,
            rumble: self.dev.caps.rumble,
        }
    }

    /// Read the state whole again (after a SYN_DROPPED).
    fn resync(&mut self) {
        let keys = self.dev.key_state();
        let mut state = PadState::default();
        for code in self.dev.caps.keys.codes() {
            let ev = InputEvent { kind: EV_KEY, code, value: i32::from(keys.has(code)), ..InputEvent::default() };
            state.feed(&ev, &self.dev.caps, &self.mapping);
        }
        for code in self.dev.caps.abs.codes() {
            if let Some(info) = self.dev.abs_now(code) {
                let ev = InputEvent { kind: EV_ABS, code, value: info.value, ..InputEvent::default() };
                state.feed(&ev, &self.dev.caps, &self.mapping);
            }
        }
        self.state = state;
    }
}

struct Worker {
    config: Config,
    shared: Arc<Shared>,
    cmds: Arc<Mutex<Vec<Cmd>>>,
    wake: Option<std::os::fd::OwnedFd>,
    send: Box<dyn Fn(PadEvent) + Send>,
    pads: Vec<Pad>,
    /// Event nodes that aren't controllers, until they go.
    ignored: HashSet<PathBuf>,
    /// Nodes that couldn't be opened yet (their permissions come later).
    pending: HashSet<PathBuf>,
    /// Each controller's last player number, by its address or serial.
    seats: HashMap<String, usize>,
    next_id: u32,
    grab: bool,
    raw: bool,
    inotify: Option<std::os::fd::OwnedFd>,
}

impl Worker {
    fn new(
        config: Config,
        shared: Arc<Shared>,
        cmds: Arc<Mutex<Vec<Cmd>>>,
        wake: Option<std::os::fd::OwnedFd>,
        send: Box<dyn Fn(PadEvent) + Send>,
    ) -> Worker {
        let grab = config.grab;
        Worker {
            config,
            shared,
            cmds,
            wake,
            send,
            pads: Vec::new(),
            ignored: HashSet::new(),
            pending: HashSet::new(),
            seats: HashMap::new(),
            next_id: 1,
            grab,
            raw: false,
            inotify: None,
        }
    }

    fn watch(&mut self) {
        // SAFETY: inotify's descriptor is owned at once.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return;
        }
        // SAFETY: just made.
        let fd = unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd) };
        let Ok(dir) = std::ffi::CString::new(self.config.dir.as_os_str().as_encoded_bytes()) else { return };
        let mask = libc::IN_CREATE | libc::IN_ATTRIB | libc::IN_DELETE | libc::IN_MOVED_TO;
        // SAFETY: a valid path and descriptor.
        if unsafe { libc::inotify_add_watch(fd.as_raw_fd(), dir.as_ptr(), mask) } >= 0 {
            self.inotify = Some(fd);
        }
    }

    fn scan(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.config.dir) else { return };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("event")))
            .collect();
        // event2 before event10: the order they came in.
        paths.sort_by_key(|p| {
            p.file_name().and_then(|n| n.to_string_lossy().trim_start_matches("event").parse::<u32>().ok())
        });
        for p in paths {
            self.consider(&p);
        }
    }

    fn consider(&mut self, path: &Path) {
        if self.ignored.contains(path) || self.pads.iter().any(|p| p.dev.path == path) {
            return;
        }
        match Device::open(path) {
            Ok(dev) => {
                self.pending.remove(path);
                if dev.caps.is_controller() {
                    self.add(dev);
                } else {
                    self.ignored.insert(path.to_owned());
                }
            }
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {
                self.pending.remove(path);
            }
            Err(_) => {
                self.pending.insert(path.to_owned());
            }
        }
    }

    fn add(&mut self, dev: Device) {
        let key = if dev.caps.uniq.is_empty() { mapping::guid(&dev.caps.id) } else { dev.caps.uniq.clone() };
        let taken: HashSet<usize> = self.pads.iter().map(|p| p.slot).collect();
        let slot = match self.seats.get(&key) {
            Some(s) if !taken.contains(s) => *s,
            _ => match (0..SLOTS).find(|s| !taken.contains(s)) {
                Some(s) => s,
                // More controllers than players: the fifth isn't one.
                None => SLOTS,
            },
        };
        if slot < SLOTS {
            self.seats.insert(key, slot);
        }
        if self.grab {
            dev.grab(true);
        }
        let mapping = mapping::resolve(&dev.caps, &self.config.layouts, &self.config.database);
        let battery = evdev::battery_of(&dev.path);
        let mut pad = Pad {
            id: self.next_id,
            slot,
            dev,
            mapping,
            state: PadState::default(),
            published: (0, [0.0; 6]),
            repeater: nav::Repeater::default(),
            chord_since: None,
            battery,
        };
        self.next_id += 1;
        pad.resync();
        log::info!(
            "chanoma: controller {} ({}, {:?}, {:?}) is player {}",
            pad.dev.caps.name,
            pad.dev.path.display(),
            pad.dev.caps.driver,
            pad.mapping.source,
            slot + 1
        );
        (self.send)(PadEvent::Connected(pad.info()));
        self.pads.push(pad);
        let i = self.pads.len() - 1;
        self.publish(i, Instant::now());
    }

    fn remove(&mut self, i: usize) {
        let pad = self.pads.remove(i);
        log::info!("chanoma: controller {} went (player {})", pad.dev.caps.name, pad.slot + 1);
        if pad.slot < SLOTS {
            self.shared.set(pad.slot, false, 0, [0.0; 6]);
        }
        (self.send)(PadEvent::Disconnected { id: pad.id, slot: pad.slot });
    }

    /// Tell what changed on pad `i` since last told.
    fn publish(&mut self, i: usize, now: Instant) {
        let pad = &mut self.pads[i];
        let buttons = pad.state.buttons();
        let axes = pad.state.axes();
        if pad.slot < SLOTS {
            self.shared.set(pad.slot, true, buttons, axes);
        }
        let before = pad.published.0;
        pad.published = (buttons, axes);
        for b in Button::ALL {
            let (was, is) = (before & b.bit() != 0, buttons & b.bit() != 0);
            if was != is {
                (self.send)(PadEvent::Button { id: pad.id, slot: pad.slot, button: b, pressed: is });
            }
        }
        // Select and Start held together: Home, once.
        let chord = Button::Select.bit() | Button::Start.bit();
        if buttons & chord == chord {
            pad.chord_since.get_or_insert(now);
        } else {
            pad.chord_since = None;
        }
        let dir = pad.repeater.update(pad.state.arrows(), axes[0], axes[1], now);
        if let Some(dir) = dir {
            (self.send)(PadEvent::Nav { id: pad.id, slot: pad.slot, dir, repeat: false });
        }
    }

    fn timers(&mut self, now: Instant) {
        for pad in &mut self.pads {
            while let Some(dir) = pad.repeater.tick(now) {
                (self.send)(PadEvent::Nav { id: pad.id, slot: pad.slot, dir, repeat: true });
            }
            if pad.chord_since.is_some_and(|t| now >= t + HOME_CHORD) {
                // Far in the future: once per hold.
                pad.chord_since = Some(now + Duration::from_secs(86_400 * 365));
                for pressed in [true, false] {
                    (self.send)(PadEvent::Button { id: pad.id, slot: pad.slot, button: Button::Guide, pressed });
                }
            }
        }
    }

    fn deadline(&self) -> Option<Instant> {
        self.pads.iter().flat_map(|p| [p.repeater.deadline(), p.chord_since.map(|t| t + HOME_CHORD)]).flatten().min()
    }

    fn batteries(&mut self) {
        for pad in &mut self.pads {
            let b = evdev::battery_of(&pad.dev.path);
            if b != pad.battery {
                pad.battery = b;
                (self.send)(PadEvent::Battery { id: pad.id, battery: b });
            }
        }
    }

    fn commands(&mut self) -> bool {
        if let Some(w) = &self.wake {
            let mut buf = [0u8; 64];
            // SAFETY: draining the pipe into a buffer of its size.
            while unsafe { libc::read(w.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
        }
        let cmds = std::mem::take(&mut *self.cmds.lock().unwrap_or_else(|e| e.into_inner()));
        for cmd in cmds {
            match cmd {
                Cmd::Grab(on) => {
                    self.grab = on;
                    for p in &self.pads {
                        p.dev.grab(on);
                    }
                }
                Cmd::Rumble { slot, strong, weak, ms } => {
                    if let Some(p) = self.pads.iter_mut().find(|p| p.slot == slot) {
                        p.dev.rumble(strong, weak, ms);
                    }
                }
                Cmd::Raw(on) => self.raw = on,
                Cmd::Layouts(layouts) => {
                    self.config.layouts = layouts;
                    for p in &mut self.pads {
                        p.mapping = mapping::resolve(&p.dev.caps, &self.config.layouts, &self.config.database);
                        p.resync();
                    }
                    for i in 0..self.pads.len() {
                        let info = self.pads[i].info();
                        (self.send)(PadEvent::Connected(info));
                        self.publish(i, Instant::now());
                    }
                }
                Cmd::Stop => return false,
            }
        }
        true
    }

    fn inotify_events(&mut self) {
        let Some(fd) = &self.inotify else { return };
        let mut buf = [0u8; 4096];
        let mut seen: Vec<(u32, PathBuf)> = Vec::new();
        loop {
            // SAFETY: reading into a buffer of its size.
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            let mut at = 0usize;
            let header = std::mem::size_of::<libc::inotify_event>();
            while at + header <= n as usize {
                // SAFETY: the kernel wrote whole events; read unaligned.
                let ev: libc::inotify_event = unsafe { std::ptr::read_unaligned(buf[at..].as_ptr().cast()) };
                let name = &buf[at + header..at + header + ev.len as usize];
                let name = name.split(|b| *b == 0).next().unwrap_or_default();
                if name.starts_with(b"event") {
                    // SAFETY: names from the kernel are bytes of a path.
                    let os = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(name) };
                    seen.push((ev.mask, self.config.dir.join(os)));
                }
                at += header + ev.len as usize;
            }
        }
        for (mask, path) in seen {
            if mask & libc::IN_DELETE != 0 {
                self.ignored.remove(&path);
                self.pending.remove(&path);
                if let Some(i) = self.pads.iter().position(|p| p.dev.path == path) {
                    self.remove(i);
                }
            } else {
                // Its permissions change after it appears (logind's ACL):
                // a node that couldn't be opened is tried again.
                if mask & libc::IN_ATTRIB != 0 {
                    self.ignored.remove(&path);
                }
                self.consider(&path);
            }
        }
    }

    fn read_pad(&mut self, i: usize, now: Instant) -> bool {
        let mut events = Vec::new();
        if !self.pads[i].dev.read(&mut events) {
            return false;
        }
        let mut changed = false;
        let mut dropped = false;
        for ev in &events {
            let pad = &mut self.pads[i];
            match (ev.kind, ev.code) {
                (EV_SYN, SYN_DROPPED) => dropped = true,
                (EV_SYN, SYN_REPORT) => {
                    if dropped {
                        pad.resync();
                        dropped = false;
                        changed = true;
                    }
                    if changed {
                        self.publish(i, now);
                        changed = false;
                    }
                }
                _ if dropped => {}
                _ => {
                    if self.raw
                        && let Some(input) = raw_input(ev, &pad.dev.caps)
                    {
                        (self.send)(PadEvent::Raw { id: pad.id, input });
                    }
                    changed |= pad.state.feed(ev, &pad.dev.caps, &pad.mapping);
                }
            }
        }
        if changed {
            self.publish(i, now);
        }
        true
    }

    fn run(mut self) {
        self.watch();
        self.scan();
        let mut battery_at = Instant::now() + BATTERY_EVERY;
        let mut retry_at = Instant::now() + RETRY_EVERY;
        loop {
            let now = Instant::now();
            let mut wake_at = battery_at;
            if !self.pending.is_empty() || self.inotify.is_none() {
                wake_at = wake_at.min(retry_at);
            }
            if let Some(d) = self.deadline() {
                wake_at = wake_at.min(d);
            }
            let timeout = wake_at.saturating_duration_since(now).as_millis().min(i32::MAX as u128) as i32;
            let mut fds: Vec<libc::pollfd> = Vec::with_capacity(self.pads.len() + 2);
            let fd_of = |fd: &Option<std::os::fd::OwnedFd>| fd.as_ref().map_or(-1, |f| f.as_raw_fd());
            fds.push(libc::pollfd { fd: fd_of(&self.wake), events: libc::POLLIN, revents: 0 });
            fds.push(libc::pollfd { fd: fd_of(&self.inotify), events: libc::POLLIN, revents: 0 });
            for p in &self.pads {
                fds.push(libc::pollfd { fd: p.dev.fd.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            }
            // SAFETY: polling descriptors that live through the call.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
            let now = Instant::now();
            if n > 0 {
                if fds[0].revents != 0 && !self.commands() {
                    return;
                }
                if fds[1].revents != 0 {
                    self.inotify_events();
                }
                // Back to front, so removing one leaves the rest's places.
                for i in (0..self.pads.len()).rev() {
                    let Some(fd) = fds.get(i + 2) else { continue };
                    let readable = fd.revents & libc::POLLIN != 0;
                    let broken = fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0;
                    // What's left to read first; then gone, if it is.
                    if (broken && !readable) || (readable && !self.read_pad(i, now)) {
                        self.remove(i);
                    }
                }
            } else if n < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                std::thread::sleep(Duration::from_millis(50));
            }
            self.timers(now);
            if now >= battery_at {
                self.batteries();
                battery_at = now + BATTERY_EVERY;
            }
            if now >= retry_at {
                if self.inotify.is_none() {
                    self.scan();
                } else {
                    for p in self.pending.clone() {
                        self.consider(&p);
                    }
                }
                retry_at = now + RETRY_EVERY;
            }
        }
    }
}

/// An input pressed or pushed far enough to mean it, while setting up.
fn raw_input(ev: &InputEvent, caps: &Caps) -> Option<Input> {
    match ev.kind {
        EV_KEY if ev.value == 1 => Some(Input::Key(ev.code)),
        EV_ABS if (evdev::ABS_HAT0X..=evdev::ABS_HAT3Y).contains(&ev.code) && ev.value != 0 => {
            let hat = ((ev.code - evdev::ABS_HAT0X) / 2) as u8;
            let x = (ev.code - evdev::ABS_HAT0X).is_multiple_of(2);
            let mask = match (x, ev.value > 0) {
                (false, false) => 1,
                (true, true) => 2,
                (false, true) => 4,
                (true, false) => 8,
            };
            Some(Input::Hat { hat, mask })
        }
        EV_ABS => {
            let info = caps.range(ev.code)?;
            let v = stick(ev.value, &info);
            (v.abs() > 0.7).then_some(Input::Abs { code: ev.code, half: if v > 0.0 { 1 } else { -1 }, invert: false })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::evdev::*;
    use super::*;

    fn pad() -> Caps {
        let mut absinfo = Vec::new();
        for c in [ABS_X, ABS_Y, ABS_RX, ABS_RY] {
            absinfo.push((c, AbsInfo { minimum: -32768, maximum: 32767, flat: 128, ..AbsInfo::default() }));
        }
        for c in [ABS_Z, ABS_RZ] {
            absinfo.push((c, AbsInfo { minimum: 0, maximum: 255, ..AbsInfo::default() }));
        }
        for c in [ABS_HAT0X, ABS_HAT0Y] {
            absinfo.push((c, AbsInfo { minimum: -1, maximum: 1, ..AbsInfo::default() }));
        }
        Caps {
            keys: Bits::with(&[
                BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_TL, BTN_TR, BTN_SELECT, BTN_START, BTN_MODE,
            ]),
            abs: Bits::with(&[ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ, ABS_HAT0X, ABS_HAT0Y]),
            absinfo,
            driver: "playstation".into(),
            ..Caps::default()
        }
    }

    fn ev(kind: u16, code: u16, value: i32) -> InputEvent {
        InputEvent { kind, code, value, ..InputEvent::default() }
    }

    #[test]
    fn events_become_buttons_and_axes() {
        let caps = pad();
        let m = mapping::from_kernel(&caps);
        let mut s = PadState::default();
        assert!(s.feed(&ev(EV_KEY, BTN_SOUTH, 1), &caps, &m));
        assert!(s.feed(&ev(EV_ABS, ABS_HAT0Y, -1), &caps, &m));
        assert_eq!(s.buttons(), Button::South.bit() | Button::Up.bit());
        assert_eq!(s.arrows(), Some(Dir::Up));
        s.feed(&ev(EV_ABS, ABS_HAT0Y, 0), &caps, &m);
        s.feed(&ev(EV_ABS, ABS_HAT0X, 1), &caps, &m);
        assert_eq!(s.arrows(), Some(Dir::Right));
        // A stick at rest near the middle is still; pushed, it reads.
        s.feed(&ev(EV_ABS, ABS_X, 2000), &caps, &m);
        assert_eq!(s.axes()[Axis::LeftX as usize], 0.0);
        s.feed(&ev(EV_ABS, ABS_Y, 32767), &caps, &m);
        assert_eq!(s.axes()[Axis::LeftY as usize], 1.0);
        // The trigger, and past half way its button.
        s.feed(&ev(EV_ABS, ABS_Z, 200), &caps, &m);
        assert!((s.axes()[Axis::L2 as usize] - 200.0 / 255.0).abs() < 1e-3);
        assert!(s.buttons() & Button::L2.bit() != 0);
        s.feed(&ev(EV_ABS, ABS_Z, 0), &caps, &m);
        assert_eq!(s.buttons() & Button::L2.bit(), 0);
    }

    #[test]
    fn shared_state_is_what_games_read() {
        let shared = Shared::default();
        shared.set(1, true, Button::Start.bit(), [0.5, -1.0, 0.0, 0.0, 1.0, 0.0]);
        assert!(shared.connected(1) && !shared.connected(0));
        assert_eq!(shared.buttons(1), Button::Start.bit());
        assert_eq!(shared.axis(1, Axis::LeftY), -32767);
        assert_eq!(shared.axis(1, Axis::L2), 32767);
        assert_eq!(shared.axis(7, Axis::L2), 0);
    }

    #[test]
    fn families_by_maker_and_name() {
        let named = |vendor, name: &str, driver: &str| Caps {
            name: name.into(),
            id: InputId { vendor, ..InputId::default() },
            driver: driver.into(),
            ..Caps::default()
        };
        assert_eq!(Family::of(&named(0x045e, "Xbox Wireless Controller", "")), Family::Xbox);
        assert_eq!(Family::of(&named(0x054c, "Wireless Controller", "")), Family::PlayStation);
        assert_eq!(Family::of(&named(0x057e, "Pro Controller", "nintendo")), Family::Nintendo);
        assert_eq!(Family::of(&named(0x2dc8, "8BitDo Pro 2", "")), Family::Nintendo);
        assert_eq!(Family::of(&named(0x0e6f, "PDP Wired Controller", "xpad")), Family::Xbox);
        assert_eq!(Family::of(&named(0x1234, "USB Gamepad", "hid-generic")), Family::Generic);
    }
}
