//! Loading, calling and swapping the compositor's logic.
//!
//! The logic is a Rust dylib (`aizome-comp-logic`) that exports
//! `aizome_logic_entry`, which returns a [`LogicVTable`]: plain Rust
//! function pointers the core calls at its hook points (input after the
//! core's own keys, windows and layers coming and going, outputs, the
//! frame's scene, IPC, timers, settings). The logic keeps its state in a
//! box the core holds for it ([`LogicHost`]) and reaches it only through
//! the `&mut State` each hook gets, so nested calls stay sound.
//!
//! A reload:
//!
//! 1. reads the candidate's ABI fingerprint from its file and refuses it,
//!    saying what differs, unless it matches the core's;
//! 2. copies the file to a path of its own (dlopen caches by path, and a
//!    build may overwrite the original) and opens it with `RTLD_NOW |
//!    RTLD_LOCAL`, so every symbol it needs from the core resolves now or
//!    the load fails;
//! 3. asks the running logic to let go (close overlays, end its pointer
//!    grabs) and to save its state, as JSON;
//! 4. starts the new logic from that state, between two frames;
//! 5. keeps the old library loaded for good: grabs, timers and other code
//!    it handed out may still point into it.
//!
//! Every call into the logic runs under `catch_unwind`. A panic marks the
//! logic poisoned (nothing more is asked of it), and at the end of the
//! event loop's iteration the core rolls back to the logic before the last
//! reload, or restarts the current one from its saved state, and tells the
//! user. A logic that keeps panicking is set aside: the session goes on
//! with the core's recovery path (`fallback.rs`) until a reload brings a
//! working one.

use std::any::TypeId;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use aizome_ipc::{Event, Request, Response};
use smithay::input::keyboard::{Keycode, Keysym, ModifiersState};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::utils::{Logical, Point, SERIAL_COUNTER, Serial};
use smithay::wayland::shell::xdg::ToplevelSurface;
use tracing::{error, info, warn};

use crate::scene::{Purpose, Scene};
use crate::state::State;
use crate::{ABI, ABI_PARTS, Rect, WinId};

/// What the core asks of a logic library. Every function gets the whole
/// compositor state; the logic finds its own through `state.lg`.
pub struct LogicVTable {
    /// The core ABI the library was compiled against ([`crate::ABI`]).
    pub abi: &'static str,
    /// The library's build: its version and a hash of its sources.
    pub build: &'static str,
    /// `TypeId::of::<State>()` as the library sees it: the same core.
    pub state_type: fn() -> TypeId,

    // ---- life ------------------------------------------------------------
    /// Sets the logic up, from a state another logic saved (or none), and
    /// takes in the windows the core has.
    pub init: fn(&mut State, Option<&[u8]>) -> Result<(), String>,
    /// Its state, to hand to the next logic.
    pub save: fn(&mut State) -> Result<Vec<u8>, String>,
    /// Lets go of what can't outlive it: overlays, pointer grabs, drags.
    pub quiesce: fn(&mut State),
    /// Drops its state.
    pub release: fn(&mut State),
    /// Brings the core's view of the windows up to date.
    pub publish: fn(&mut State),

    // ---- windows and layers -------------------------------------------------
    /// A toplevel's first configure: its bounds and size.
    pub first_configure: fn(&mut State, &ToplevelSurface),
    /// A toplevel drew its first frame and is in the core's registry.
    pub window_mapped: fn(&mut State, WinId, &MapInfo),
    /// A mapped toplevel committed (`true`: its root surface).
    pub window_committed: fn(&mut State, WinId, bool),
    /// A toplevel unmapped or went away; the core has forgotten it.
    pub window_unmapped: fn(&mut State, WinId),
    /// Something a client asked of its toplevel.
    pub toplevel_request: fn(&mut State, WinId, ToplevelRequest),
    /// The area left for windows on an output changed (layer surfaces'
    /// exclusive zones).
    pub usable_area_changed: fn(&mut State, &Output),
    /// Outputs came, went or changed.
    pub outputs_changed: fn(&mut State),

    // ---- input -------------------------------------------------------------------
    /// A key from a keyboard or IPC, before the core decides it: the logic
    /// may delay or repeat it (slow and sticky keys) and passes it on with
    /// [`State::key_through`].
    pub key: fn(&mut State, u32, bool, u32),
    /// What a key does, once the core's own keys are out of the way.
    pub decide_key: fn(&mut State, &KeyEvent<'_>) -> KeyDecision,
    /// Runs the action [`KeyDecision::Act`] stood for.
    pub key_action: fn(&mut State),
    /// The pointer moved to a point in the global layout.
    pub pointer_motion: fn(&mut State, Point<f64, Logical>, u32),
    pub pointer_button: fn(&mut State, u32, bool, u32),
    pub pointer_axis: fn(&mut State, &AxisEvent),
    /// A touch went down at a point: focus what's there.
    pub touch_down: fn(&mut State, Point<f64, Logical>),

    // ---- frames -------------------------------------------------------------------
    /// What an output shows under the pointer and the drag icon, front
    /// first (the core adds those, and the lock screen instead while
    /// locked).
    pub scene: fn(&mut State, &Output, Purpose) -> Scene,
    /// A window with its decorations and shadow, for screenshots.
    pub window_bounds: fn(&mut State, WinId) -> Option<Rect>,

    // ---- the rest ------------------------------------------------------------------
    /// An IPC request the core doesn't answer itself; `None`: not the
    /// logic's either.
    pub request: fn(&mut State, &Request) -> Option<Response>,
    /// A timer the logic asked for ([`State::schedule`]).
    pub timer: fn(&mut State, u64),
    /// Settings were applied (the old ones given).
    pub settings_changed: fn(&mut State, &aizome_config::Settings),
    /// After every batch of events, before frames.
    pub loop_iteration: fn(&mut State),
}

/// A toplevel joining the desktop.
#[derive(Clone, Debug)]
pub struct MapInfo {
    pub app_id: String,
    pub title: String,
    pub pid: Option<i32>,
    pub parent: Option<WinId>,
    /// Server-side decorations.
    pub ssd: bool,
    /// Its geometry's size.
    pub size: (i32, i32),
}

/// What a client asked of its toplevel.
#[derive(Clone, Debug)]
pub enum ToplevelRequest {
    Move {
        seat: smithay::input::Seat<State>,
        serial: Serial,
    },
    Resize {
        seat: smithay::input::Seat<State>,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    },
    Maximize(bool),
    Fullscreen(bool),
    Minimize,
    /// The window menu, at a point relative to the window's geometry.
    WindowMenu(Point<i32, Logical>),
    AppIdChanged(String),
    TitleChanged(String),
    ParentChanged(Option<WinId>),
    /// xdg-activation: `valid` when the token came from recent input.
    Activate {
        valid: bool,
    },
    /// The client set or changed its decoration mode.
    Decorations,
}

/// A key, as the core hands it to the logic to decide.
pub struct KeyEvent<'a> {
    pub keycode: Keycode,
    pub pressed: bool,
    pub mods: ModifiersState,
    /// The unshifted Latin keysym first, then the modified one, then the
    /// raw ones.
    pub syms: &'a [Keysym],
    pub modified: Keysym,
    pub utf8: Option<char>,
}

/// What a key does.
#[derive(Clone, Copy, Debug)]
pub enum KeyDecision {
    Forward,
    /// Forward with these modifiers as the client sees them (❖ → Ctrl).
    Translate(ModifiersState),
    Swallow,
    /// The logic runs an action ([`LogicVTable::key_action`]).
    Act,
}

/// A scroll from a device or IPC.
#[derive(Clone, Copy, Debug)]
pub struct AxisEvent {
    pub source: smithay::backend::input::AxisSource,
    pub time: u32,
    /// Points, before natural scrolling and the scroll factor.
    pub h: f64,
    pub v: f64,
    pub h120: Option<f64>,
    pub v120: Option<f64>,
    /// A finger lifted (amount exactly zero on a touchpad).
    pub h_stop: bool,
    pub v_stop: bool,
}

/// A library loaded once.
struct Loaded {
    vt: &'static LogicVTable,
    /// Where it was loaded from (before the private copy).
    path: PathBuf,
    loaded_at: SystemTime,
}

/// What a reload did, for IPC and `aizomectl`.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub build: String,
    pub path: PathBuf,
    pub millis: u64,
}

/// The loaded logic and its state.
pub struct LogicHost {
    /// The logic's state, a box it made, and the address of a static in the
    /// library that made it: logic code checks the address is its own
    /// before it touches the state, so code from a library swapped out
    /// can't misread a new one's.
    state: *mut (),
    owner: usize,
    vt: Option<&'static LogicVTable>,
    current: Option<Loaded>,
    /// The logic before the last reload, to roll back to.
    previous: Option<Loaded>,
    /// What the logic saved at the last reload: what a rollback starts
    /// from when the failing logic can't save.
    last_saved: Option<Vec<u8>>,
    /// Bumped at every load: timers of an older logic don't fire.
    generation: u64,
    /// The logic panicked: nothing more is asked of it until it is rolled
    /// back or restarted (at the end of this loop iteration).
    poisoned: Option<String>,
    /// Recent panics, to set aside a logic that keeps failing.
    panics: Vec<Instant>,
    /// The serial of the pointer grab the logic holds, if any.
    pub(crate) pointer_grab: Option<Serial>,
    /// How many times the logic was swapped, rolled back or restarted.
    pub reloads: u32,
    pub rollbacks: u32,
    pub last_error: Option<String>,
    /// A reload asked for by a key (or from inside a logic call): done at
    /// the end of the iteration.
    pub(crate) pending_reload: Option<crate::requests::ReloadArgs>,
    /// Calls into the logic on the stack now.
    depth: u32,
}

impl Default for LogicHost {
    fn default() -> Self {
        LogicHost {
            state: std::ptr::null_mut(),
            owner: 0,
            vt: None,
            current: None,
            previous: None,
            last_saved: None,
            generation: 0,
            poisoned: None,
            panics: Vec::new(),
            pointer_grab: None,
            reloads: 0,
            rollbacks: 0,
            last_error: None,
            pending_reload: None,
            depth: 0,
        }
    }
}

impl LogicHost {
    /// The logic's state box and its maker's mark (for the logic's own
    /// accessor).
    pub fn raw(&self) -> (*mut (), usize) {
        (self.state, self.owner)
    }

    /// Where the logic keeps its state (from its `init`).
    pub fn set_raw(&mut self, state: *mut (), owner: usize) {
        self.state = state;
        self.owner = owner;
    }

    /// Takes the state box back (from the logic's `release`).
    pub fn take_raw(&mut self) -> (*mut (), usize) {
        let r = (self.state, self.owner);
        self.state = std::ptr::null_mut();
        self.owner = 0;
        r
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Logic code is on the stack: nothing may swap it now.
    pub fn in_call(&self) -> bool {
        self.depth > 0
    }

    /// A logic is loaded and well.
    pub fn healthy(&self) -> bool {
        self.vt.is_some() && self.poisoned.is_none()
    }

    pub fn build(&self) -> Option<&'static str> {
        self.current.as_ref().map(|l| l.vt.build)
    }

    pub fn path(&self) -> Option<&Path> {
        self.current.as_ref().map(|l| l.path.as_path())
    }

    pub fn loaded_at(&self) -> Option<SystemTime> {
        self.current.as_ref().map(|l| l.loaded_at)
    }
}

impl State {
    /// Calls into the logic, if one is loaded and well, catching a panic;
    /// else (or after a panic) gives `default`.
    pub fn lg_call<R>(&mut self, default: R, f: impl FnOnce(&'static LogicVTable, &mut State) -> R) -> R {
        let Some(vt) = self.lg.vt else { return default };
        if self.lg.poisoned.is_some() {
            return default;
        }
        self.lg.depth += 1;
        let r = catch_unwind(AssertUnwindSafe(|| f(vt, self)));
        self.lg.depth -= 1;
        match r {
            Ok(r) => r,
            Err(payload) => {
                let message = panic_message(&payload);
                self.mark_logic_panicked(message);
                default
            }
        }
    }

    /// Notes a panic in the logic (from [`State::lg_call`] or a guarded
    /// grab): it is rolled back at the end of this iteration.
    pub fn mark_logic_panicked(&mut self, message: String) {
        if self.lg.poisoned.is_none() {
            error!("the compositor logic panicked: {message}");
            self.lg.poisoned = Some(message);
        }
    }

    /// Loads a logic library and starts it, from the running logic's state
    /// when there is one (`true` for a reload asked for by the user).
    pub fn load_logic(&mut self, path: &Path, _reload: bool) -> Result<Outcome, String> {
        let started = Instant::now();
        let next_gen = self.lg.generation + 1;
        let vt = open(path, next_gen)?;
        let loaded = Loaded { vt, path: path.to_path_buf(), loaded_at: SystemTime::now() };

        // The running logic lets go and hands over its state.
        let saved = self.stop_logic();
        let old = self.lg.current.take();
        self.lg.generation = next_gen;
        self.lg.vt = Some(vt);
        self.lg.poisoned = None;
        let res = catch_unwind(AssertUnwindSafe(|| (vt.init)(self, saved.as_deref())));
        let failure = match res {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(p) => Some(format!("panicked: {}", panic_message(&p))),
        };
        if let Some(e) = failure {
            // Back to what ran before, from the state it saved.
            warn!("the new compositor logic failed to start: {e}");
            self.discard_logic();
            self.lg.vt = None;
            if let Some(old) = old {
                self.lg.generation += 1;
                self.lg.vt = Some(old.vt);
                self.lg.current = Some(old);
                let res = catch_unwind(AssertUnwindSafe(|| (self.lg.vt.unwrap().init)(self, saved.as_deref())));
                if !matches!(res, Ok(Ok(()))) {
                    error!("the previous logic didn't start again either");
                    self.discard_logic();
                    self.lg.vt = None;
                    self.lg.current = None;
                }
            }
            self.after_logic_change();
            let e = format!("{}: the new logic failed to start ({e}); kept the one running", path.display());
            self.lg.last_error = Some(e.clone());
            return Err(e);
        }
        if old.is_some() {
            self.lg.reloads += 1;
        }
        self.lg.previous = old.or(self.lg.previous.take());
        self.lg.last_saved = saved;
        self.lg.current = Some(loaded);
        self.lg.panics.clear();
        self.after_logic_change();
        let millis = started.elapsed().as_millis() as u64;
        info!(build = vt.build, "compositor logic loaded from {} in {millis} ms", path.display());
        Ok(Outcome { build: vt.build.to_string(), path: path.to_path_buf(), millis })
    }

    /// Asks the running logic to let go and save its state, then drops it.
    /// Returns what it saved.
    fn stop_logic(&mut self) -> Option<Vec<u8>> {
        let vt = self.lg.vt?;
        let healthy = self.lg.poisoned.is_none();
        let _ = catch_unwind(AssertUnwindSafe(|| (vt.quiesce)(self)));
        self.end_logic_grabs();
        let saved = match catch_unwind(AssertUnwindSafe(|| (vt.save)(self))) {
            Ok(Ok(bytes)) => Some(bytes),
            Ok(Err(e)) => {
                warn!("the compositor logic couldn't save its state: {e}");
                None
            }
            Err(p) => {
                warn!("the compositor logic panicked saving its state: {}", panic_message(&p));
                None
            }
        };
        // A poisoned logic's state may be inconsistent; what it saved is
        // still the best there is, else what the last good one saved.
        let saved = saved.or_else(|| (!healthy).then(|| self.lg.last_saved.clone()).flatten());
        self.discard_logic();
        saved
    }

    /// Drops the logic's state (its own `release`, or leaks the box when
    /// that fails).
    fn discard_logic(&mut self) {
        if let Some(vt) = self.lg.vt
            && !self.lg.state.is_null()
            && catch_unwind(AssertUnwindSafe(|| (vt.release)(self))).is_err()
        {
            warn!("the compositor logic panicked releasing its state; leaking it");
        }
        self.lg.take_raw();
    }

    /// Ends a pointer grab the logic set (its code may be gone).
    pub fn end_logic_grabs(&mut self) {
        let Some(serial) = self.lg.pointer_grab.take() else { return };
        if let Some(pointer) = self.az.seat.get_pointer()
            && pointer.has_grab(serial)
        {
            let time = self.az.start.elapsed().as_millis() as u32;
            pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), time);
        }
        self.az.cursor_override = None;
    }

    fn after_logic_change(&mut self) {
        self.lg_call((), |vt, st| (vt.publish)(st));
        self.az.window_outputs_dirty = true;
        self.update_keyboard_focus();
        self.az.queue_redraw_all();
    }

    /// At the end of an iteration: a logic that panicked is rolled back to
    /// the one before it, or restarted, or (failing again and again) set
    /// aside for the recovery path. A reload asked for by a key happens
    /// here too.
    pub fn recover_logic(&mut self) {
        if let Some(args) = self.lg.pending_reload.take() {
            let report = self.reload(&args);
            let text = crate::requests::reload_summary(&report);
            self.notify("Aizome reloaded", &text);
        }
        let Some(message) = self.lg.poisoned.clone() else { return };
        let now = Instant::now();
        self.lg.panics.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        self.lg.panics.push(now);
        self.lg.rollbacks += 1;
        let build = self.lg.build().unwrap_or("?").to_string();
        self.lg.last_error = Some(format!("logic {build} panicked: {message}"));
        let saved = self.stop_logic();
        self.lg.poisoned = None;
        self.lg.vt = None;
        let failed = self.lg.current.take();

        // The logic before the last reload, if this one came in by a
        // reload; else this one again, unless it keeps failing.
        let (next, what) = match self.lg.previous.take() {
            Some(prev) => (Some(prev), "rolled back to the logic that ran before"),
            None if self.lg.panics.len() < 3 => (failed, "restarted it"),
            None => (None, "set it aside: the session runs on the recovery path until a reload"),
        };
        if let Some(next) = next {
            self.lg.generation += 1;
            self.lg.vt = Some(next.vt);
            self.lg.current = Some(next);
            let vt = self.lg.vt.unwrap();
            let res = catch_unwind(AssertUnwindSafe(|| (vt.init)(self, saved.as_deref())));
            if !matches!(res, Ok(Ok(()))) {
                // Not even from its state: from nothing.
                self.discard_logic();
                let res = catch_unwind(AssertUnwindSafe(|| (vt.init)(self, None)));
                if !matches!(res, Ok(Ok(()))) {
                    self.discard_logic();
                    self.lg.vt = None;
                    self.lg.current = None;
                }
            }
        }
        self.after_logic_change();
        let body = format!("{message}\nAizome {what}. Your windows are as they were.");
        error!("compositor logic {build} panicked; {what}");
        self.az.ipc.emit(&Event::LogicFailed { message: message.clone(), action: what.into() });
        self.notify("Aizome's window manager failed", &body);
    }

    /// Tells the user something about the session, as a notification (the
    /// shell shows it) and in the log.
    pub fn notify(&mut self, summary: &str, body: &str) {
        info!("{summary}: {body}");
        if self.az.options.no_session && std::env::var_os("AIZOME_NOTIFY").is_none() {
            return;
        }
        let argv = vec!["notify-send".to_string(), "--app-name=Aizome".into(), summary.into(), body.into()];
        if let Err(e) = crate::session::spawn(&self.az, &argv, None) {
            warn!("{e}");
        }
    }

    pub fn notify_logic_problem(&mut self, summary: &str, detail: &str) {
        self.lg.last_error = Some(detail.to_string());
        self.notify(summary, detail);
    }
}

/// Reads a logic library's ABI fingerprint from its file, without loading
/// it.
pub fn read_abi(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let marker = ABI_MARKER_PREFIX.as_bytes();
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .ok_or_else(|| format!("{} isn't an Aizome logic library (no ABI mark)", path.display()))?;
    let start = at + marker.len();
    let hex = bytes.get(start..start + 64).ok_or("a truncated ABI mark")?;
    Ok(String::from_utf8_lossy(hex).into_owned())
}

/// What precedes the fingerprint in a logic library's mark.
pub const ABI_MARKER_PREFIX: &str = "AIZOME-LOGIC-ABI:";

/// The mark a logic library embeds: the prefix and the core ABI it was
/// built against, readable straight from the file.
pub const fn abi_marker() -> [u8; 81] {
    let mut out = [0u8; 81];
    let p = ABI_MARKER_PREFIX.as_bytes();
    let a = ABI.as_bytes();
    let mut i = 0;
    while i < p.len() {
        out[i] = p[i];
        i += 1;
    }
    let mut j = 0;
    while j < 64 && j < a.len() {
        out[i + j] = a[j];
        j += 1;
    }
    out
}

/// Opens a logic library: checks its fingerprint, loads a private copy and
/// finds its table.
fn open(path: &Path, generation: u64) -> Result<&'static LogicVTable, String> {
    let abi = read_abi(path)?;
    if abi != ABI {
        return Err(format!(
            "{} was built against another compositor core (its ABI {}…, this core's {}…: {ABI_PARTS}). \
             Rebuild it with the toolchain and flags the running session was built with (aizome-dev), \
             or log out and in to run the new core",
            path.display(),
            &abi[..12.min(abi.len())],
            &ABI[..12]
        ));
    }
    let copy = private_copy(path, generation)?;
    // Safety: the library's initialisers are Rust's (none) and its
    // dependencies' (the core's, already run); RTLD_NOW resolves every
    // symbol now, so a mismatch fails here and not on a call.
    let lib = unsafe {
        libloading::os::unix::Library::open(
            Some(&copy),
            libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_LOCAL,
        )
    }
    .map_err(|e| {
        let e = e.to_string();
        if e.contains("undefined symbol") {
            // Same sources and toolchain, but not the same build of them:
            // Cargo resolved the core's dependencies with other features
            // (another set of packages built together), or another profile.
            format!(
                "{} doesn't fit this compositor's core: it was built against another build of the same \
                 sources (other packages or features built together, or another profile). Build it as \
                 aizome-dev does ({e})",
                path.display()
            )
        } else {
            format!("{}: {e}", path.display())
        }
    })?;
    // Safety: the symbol's type is the logic's entry point, checked by the
    // fingerprint (same core, same types).
    let entry: fn() -> &'static LogicVTable = unsafe {
        *lib.get::<fn() -> &'static LogicVTable>(b"aizome_logic_entry\0")
            .map_err(|e| format!("{}: {e}", path.display()))?
    };
    // Never closed: code and data it hands out may outlive it.
    std::mem::forget(lib);
    let vt = catch_unwind(entry).map_err(|p| format!("{}: {}", path.display(), panic_message(&p)))?;
    if vt.abi != ABI || (vt.state_type)() != TypeId::of::<State>() {
        return Err(format!("{}: built against another compositor core", path.display()));
    }
    Ok(vt)
}

/// Copies a library where nothing else writes, under the cache directory,
/// one directory per compositor process (cleared of dead processes' as it
/// goes).
fn private_copy(path: &Path, generation: u64) -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| aizome_config::home_dir().join(".cache"))
        .join("aizome/logic");
    let pid = std::process::id();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let alive = e
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u32>().ok())
                .is_some_and(|p| p == pid || Path::new(&format!("/proc/{p}")).exists());
            if !alive {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let dir = base.join(pid.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "logic.so".into());
    let copy = dir.join(format!("{generation}-{name}"));
    std::fs::copy(path, &copy).map_err(|e| format!("copying {} to {}: {e}", path.display(), copy.display()))?;
    Ok(copy)
}

/// The logic library to load at start: the one given, else the one
/// installed beside the binary (the package's `lib/aizome`, or Cargo's
/// target directory).
pub fn default_logic_path(given: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = given {
        return Some(p);
    }
    installed_logic_beside_binary()
}

/// The logic a reload loads when not told which: the one installed now.
/// A session running from the Nix store follows the system (after
/// `nixos-rebuild switch`, the new package's, found through the `aizome`
/// on PATH); a development build loads the one beside itself.
pub fn installed_logic(given: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = given {
        return Some(p.to_path_buf());
    }
    let exe = std::env::current_exe().ok();
    let from_store = exe.as_ref().is_some_and(|e| e.starts_with("/nix/store"));
    if from_store
        && let Some(bin) = crate::session::which("aizome").and_then(|p| std::fs::canonicalize(p).ok())
        && let Some(dir) = bin.parent()
    {
        let p = dir.join("../lib/aizome/libaizome_comp_logic.so");
        if p.is_file() {
            return std::fs::canonicalize(&p).ok().or(Some(p));
        }
    }
    installed_logic_beside_binary()
}

pub fn installed_logic_beside_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = "libaizome_comp_logic.so";
    [dir.join("../lib/aizome").join(name), dir.join(name), dir.join("deps").join(name)]
        .into_iter()
        .find(|p| p.is_file())
}

static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Records where a panic happened (for messages), then does what panics
/// normally do.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let what = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "a panic".into()
        };
        if let Ok(mut p) = LAST_PANIC.lock() {
            *p = Some(format!("{what}{at}"));
        }
        default(info);
    }));
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Ok(mut p) = LAST_PANIC.lock()
        && let Some(m) = p.take()
    {
        return m;
    }
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic".into()
    }
}

/// How the core was built, as JSON (`aizome --build-info`): what a logic
/// library must be built with to load into it.
pub fn build_info_json() -> String {
    serde_json::to_string_pretty(&build_info()).unwrap_or_default()
}

/// How this core was built: what a logic library must be built with.
pub fn build_info() -> serde_json::Value {
    serde_json::json!({
        "abi": ABI,
        "abi_parts": ABI_PARTS,
        "rustc": crate::build::RUSTC,
        "target": crate::build::TARGET,
        "profile": crate::build::PROFILE,
        "opt_level": crate::build::OPT_LEVEL,
        "rustflags": crate::build::RUSTFLAGS,
        "explicit_target": crate::build::EXPLICIT_TARGET == "true",
        "sources": crate::build::SOURCES,
        "version": env!("CARGO_PKG_VERSION"),
    })
}
