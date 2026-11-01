//! The compositor's IPC: types, socket path and a blocking client.
//!
//! The compositor listens on `$XDG_RUNTIME_DIR/aizome.$WAYLAND_DISPLAY.sock`
//! and exports the path as `AIZOME_SOCKET`. A client writes one [`Request`]
//! per line as JSON and reads one [`Response`] per line back. After
//! [`Request::Subscribe`] (answered with [`Response::Ok`]) the connection
//! carries only [`Event`]s, one per line, until it is closed.
//!
//! Enums are tagged by a `"request"`, `"response"` or `"event"` field in
//! kebab-case, e.g. `{"request":"focus","id":7}`. `docs/ipc.md` lists
//! every message.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod setup;

/// Bumped when a message changes incompatibly.
pub const PROTOCOL_VERSION: u32 = 1;

/// A window's identity for as long as it is open. Never reused within a
/// compositor's lifetime.
pub type WindowId = u64;

/// A rectangle in the global layout, in points (logical pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
}

/// Picks a Desk: by its position (1-based, as ⌃❖1…9 count) or by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeskSel {
    Index(u32),
    Name(String),
}

impl std::fmt::Display for DeskSel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeskSel::Index(i) => write!(f, "{i}"),
            DeskSel::Name(n) => write!(f, "{n}"),
        }
    }
}

impl std::str::FromStr for DeskSel {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.parse::<u32>() {
            Ok(i) => DeskSel::Index(i),
            Err(_) => DeskSel::Name(s.to_string()),
        })
    }
}

/// Picks an app: by app_id or by process ID. With neither, the focused
/// window's app.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transform {
    #[default]
    Normal,
    #[serde(rename = "90")]
    _90,
    #[serde(rename = "180")]
    _180,
    #[serde(rename = "270")]
    _270,
    Flipped,
    #[serde(rename = "flipped-90")]
    Flipped90,
    #[serde(rename = "flipped-180")]
    Flipped180,
    #[serde(rename = "flipped-270")]
    Flipped270,
}

/// A request to the compositor. Optional `id` fields mean the focused
/// window when left out; optional `state` fields toggle when left out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    /// → [`Response::Version`].
    Version,
    /// → [`Response::Outputs`].
    Outputs,
    /// → [`Response::Windows`], bottom of the stacking order first.
    Windows,
    /// → [`Response::Desks`].
    Desks,
    /// → [`Response::Window`].
    FocusedWindow,
    /// Focuses and raises a window, switching to its Desk and showing it
    /// if it was minimised or its app hidden.
    Focus {
        id: WindowId,
    },
    /// Asks a window to close (the app may ask to save first).
    Close {
        #[serde(default)]
        id: Option<WindowId>,
    },
    Minimise {
        #[serde(default)]
        id: Option<WindowId>,
    },
    Unminimise {
        id: WindowId,
    },
    /// Hides every window of an app (❖H).
    HideApp {
        #[serde(default, flatten)]
        app: AppSel,
    },
    /// Hides every app but the focused one's (❖⌥H).
    HideOthers,
    ShowApp {
        #[serde(default, flatten)]
        app: AppSel,
    },
    /// Fills the usable area (the zoom box).
    Fill {
        #[serde(default)]
        id: Option<WindowId>,
        #[serde(default)]
        state: Option<bool>,
    },
    /// Takes the left or right half of the usable area.
    Split {
        #[serde(default)]
        id: Option<WindowId>,
        side: Side,
    },
    Fullscreen {
        #[serde(default)]
        id: Option<WindowId>,
        #[serde(default)]
        state: Option<bool>,
    },
    /// Keeps a window above the others.
    Float {
        #[serde(default)]
        id: Option<WindowId>,
        #[serde(default)]
        state: Option<bool>,
    },
    /// Shows a window on every Desk.
    Sticky {
        #[serde(default)]
        id: Option<WindowId>,
        #[serde(default)]
        state: Option<bool>,
    },
    /// Sends a window (and its tab group) to a Desk; with `follow`, goes
    /// there too.
    MoveToDesk {
        #[serde(default)]
        id: Option<WindowId>,
        desk: DeskSel,
        #[serde(default)]
        follow: bool,
    },
    /// Shows a Desk on a display (the one with the focus by default) and
    /// makes it current; one another display shows swaps places with it.
    SetDesk {
        desk: DeskSel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    AddDesk {
        name: String,
        #[serde(default)]
        folder: Option<String>,
        /// 1-based position; the end when left out.
        #[serde(default)]
        index: Option<u32>,
    },
    RenameDesk {
        desk: DeskSel,
        name: String,
    },
    SetDeskFolder {
        desk: DeskSel,
        #[serde(default)]
        folder: Option<String>,
    },
    /// Removes a Desk; its windows move to the Desk before it.
    RemoveDesk {
        desk: DeskSel,
    },
    /// Moves a Desk to another position (1-based).
    MoveDesk {
        desk: DeskSel,
        index: u32,
    },
    /// Stacks window `b` into window `a`'s tab group.
    Stack {
        a: WindowId,
        b: WindowId,
    },
    /// Takes a window out of its tab group.
    Unstack {
        #[serde(default)]
        id: Option<WindowId>,
    },
    /// Moves a window's content to `x`, `y` (the frame and tab sit
    /// outside it).
    Move {
        #[serde(default)]
        id: Option<WindowId>,
        x: i32,
        y: i32,
    },
    /// Sets a window's content rectangle.
    Resize {
        #[serde(default)]
        id: Option<WindowId>,
        rect: Rect,
    },
    /// The processes playing sound now (the sound server's streams that
    /// aren't paused), from the shell: the overview says which Desks have
    /// audio playing.
    SetAudible {
        #[serde(default)]
        pids: Vec<u32>,
    },
    /// Opens or closes the Desks overview.
    Overview {
        #[serde(default)]
        state: Option<bool>,
    },
    /// Opens the switcher (it stays until Esc or a pick, since no ❖ is
    /// held), or closes it with `state: false`.
    Switcher {
        #[serde(default)]
        reverse: bool,
        #[serde(default)]
        state: Option<bool>,
    },
    /// Runs a program, by default in the current Desk's folder.
    Spawn {
        argv: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default = "yes")]
        in_desk_folder: bool,
    },
    /// Captures an output (or every output, side by side, when left out)
    /// to a PNG. Without a path, the usual Screenshots folder.
    Screenshot {
        #[serde(default)]
        output: Option<String>,
        #[serde(default)]
        path: Option<String>,
        /// Only this window (with its frame and tab).
        #[serde(default)]
        window: Option<WindowId>,
        /// Only this area of the global layout.
        #[serde(default)]
        area: Option<Rect>,
        /// Also put the image on the clipboard as image/png.
        #[serde(default)]
        clipboard: bool,
    },
    /// Re-reads settings.toml and applies it.
    ReloadConfig,
    /// Changes an output for this session (Settings saves to the file).
    OutputConfig {
        name: String,
        /// `"WxH"` or `"WxH@Hz"`.
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        scale: Option<f64>,
        #[serde(default)]
        position: Option<[i32; 2]>,
        #[serde(default)]
        transform: Option<Transform>,
        #[serde(default)]
        enabled: Option<bool>,
    },
    /// → [`Response::Stats`].
    Stats {
        /// Clears the histograms after reading them.
        #[serde(default)]
        reset: bool,
    },
    /// Ends the session.
    Quit,
    PowerOffMonitors,
    PowerOnMonitors,
    /// Asks the shell to lock (sends `binding` `lock` to subscribers).
    Lock,
    /// Turns this connection into an event stream.
    Subscribe,
    /// Performs a key-binding action by name, as if its keys were pressed
    /// (e.g. `"overview"`, `"stack-with-behind"`).
    Action {
        action: String,
    },
    /// Feeds input through the compositor as if it came from a device:
    /// bindings, focus, grabs and all (tests and automation).
    Synthesize {
        events: Vec<Synthetic>,
    },
    /// Shows another output's picture on this one, scaled to fit
    /// (letterboxed); `source: null` gives it a picture of its own again.
    /// For this session (Settings writes `mirror` in the file).
    MirrorOutput {
        name: String,
        #[serde(default)]
        source: Option<String>,
    },
    /// Makes an output the main one for this session (Settings writes
    /// `primary` in the file).
    SetPrimaryOutput {
        name: String,
    },
    /// Night Light: changes what's given (for this session), then
    /// reports it. → [`Response::NightLight`].
    NightLight {
        #[serde(default)]
        enabled: Option<bool>,
        /// The warm temperature, in Kelvin (1900–6500).
        #[serde(default)]
        temperature: Option<u32>,
    },
    /// Reloads what changed without touching the apps: the compositor's
    /// logic (a new library, swapped between frames), the shell (restarted,
    /// its notifications kept) and the settings. With neither `logic` nor
    /// `shell`, whatever changed; with `check`, says what would happen and
    /// does nothing. → [`Response::Reload`].
    Reload {
        /// Reload the logic (even if it's the same build).
        #[serde(default)]
        logic: bool,
        /// Restart the shell.
        #[serde(default)]
        shell: bool,
        /// The logic library to load; default: the installed one (the
        /// system's, after an update).
        #[serde(default)]
        from: Option<String>,
        /// The shell to restart into (a build of one's own); default: the
        /// installed one. It stays the session's shell until a reload
        /// without it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shell_from: Option<String>,
        #[serde(default)]
        check: bool,
    },
    /// Opens or closes the recovery overlay (Ctrl+Alt+Del); left out, it
    /// toggles.
    Recovery {
        #[serde(default)]
        state: Option<bool>,
    },
    /// What the recovery overlay shows. → [`Response::Recovery`].
    RecoveryState,
    /// Does one of the recovery overlay's actions, whether or not it is
    /// open: `force-quit`, `close` (with `app_id`), `reset-input`,
    /// `restart-shell`, `reload-compositor`, `lock`, `log-out`,
    /// `text-console`. → [`Response::Message`].
    RecoveryAction {
        action: String,
        #[serde(default)]
        app_id: Option<String>,
    },
    /// The seat's grabs, pressed buttons and keys, pointer constraint and
    /// cursor. → [`Response::InputState`].
    InputState,
    /// The keyboards that type, one entry for each (a keyboard with several
    /// devices is one), and which typed last. → [`Response::Keyboards`].
    Keyboards,
    /// Swallows the next key pressed (unless the screen is locked) and
    /// tells the connection that asked, alone, which it was and on which
    /// keyboard, by [`Event::KeyCaught`] with this `tag`: Settei's keyboard
    /// setup asks, as an app never hears a modifier key alone. The asking
    /// connection must have subscribed ([`Events::request`]). It waits
    /// while that connection stays open, until it asks again without a
    /// tag (another connection's asking without one changes nothing) or
    /// another connection asks for a key. Answered at once with `ok`, or
    /// an error on a connection that hasn't subscribed.
    CatchKey {
        #[serde(default)]
        tag: Option<String>,
    },
    /// Catches these keys while one of these windows has the keyboard:
    /// each press (with no ⌃, ⌥ or ❖ held, so the system's chords stay
    /// the system's) is swallowed, its release too, and told to the
    /// connection that asked alone by [`Event::KeyCaught`] with this
    /// `tag`; the window hears nothing of it. For an app that opens
    /// another program's window and keeps a way back out of it: Chanoma
    /// keeps its Home key over the browser it started. Keys by name, as
    /// [`Synthetic::Key`] takes them, or evdev code. The asking connection
    /// must have subscribed ([`Events::request`]); the keys are caught
    /// while it stays open, until it asks again (with no keys or no
    /// windows, to stop). Answered at once with `ok`.
    CatchKeys {
        tag: String,
        #[serde(default)]
        keys: Vec<String>,
        #[serde(default)]
        windows: Vec<WindowId>,
    },
    /// Blocks the compositor's main loop for a while (at most 60 s), to
    /// try the watchdog: nothing draws or answers meanwhile.
    DebugStall {
        seconds: u32,
    },
    /// Lets the person pick an area with the screenshot picker (drag over
    /// an area, click for the whole display under the pointer, Space and a
    /// click for a window, Esc to give up), and reports the pick to
    /// subscribers as [`Event::AreaPicked`] with this `tag` instead of
    /// taking a screenshot. Answered at once with `ok`.
    PickArea {
        #[serde(default)]
        tag: String,
        /// A banner across the displays saying what the pick is for.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        /// What may be picked (empty: all three).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<PickTarget>,
    },
    /// What's being shared (screencasts through ext-image-copy-capture:
    /// browsers' screen and window sharing). → [`Response::Shares`].
    Shares,
    /// Stops a share (all of them without `id`): the sharing app is told
    /// the screen or window is no longer available.
    StopShare {
        #[serde(default)]
        id: Option<u64>,
    },
}

/// What [`Request::PickArea`] may pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PickTarget {
    /// An area dragged over.
    Area,
    /// A whole display, with a click.
    Display,
    /// A window, with Space and a click.
    Window,
}

/// A screen or window being shared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareInfo {
    /// For [`Request::StopShare`].
    pub id: u64,
    /// The display shared, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// The window shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowId>,
    /// Whether the pointer is in the picture.
    #[serde(default)]
    pub cursor: bool,
}

/// An input event for [`Request::Synthesize`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Synthetic {
    /// Moves the pointer to a point in the global layout (points).
    PointerMotion { x: f64, y: f64 },
    /// Presses or releases `"left"`, `"right"`, `"middle"` or an evdev
    /// button code.
    PointerButton { button: String, pressed: bool },
    /// Presses and releases a button.
    Click { button: String },
    /// Scrolls by points.
    PointerScroll { dx: f64, dy: f64 },
    /// Presses or releases a key by name (`"super"`, `"tab"`, `"a"`,
    /// `"up"`) or evdev code.
    Key { key: String, pressed: bool },
    /// Presses a chord's keys in order and releases them in reverse:
    /// `"super+shift+3"`.
    Chord { keys: String },
    /// Closes or opens the laptop's lid, as its switch does.
    Lid { closed: bool },
    /// Plugs in a keyboard that types (a space bar and all): `id` is its
    /// vendor and product (`05ac:029c`).
    PlugKeyboard {
        name: String,
        id: String,
        #[serde(default)]
        bus: KeyboardBus,
    },
    /// Unplugs a keyboard [`Synthetic::PlugKeyboard`] plugged in.
    UnplugKeyboard { name: String },
    /// Presses or releases a key, as [`Synthetic::Key`] does, on a keyboard
    /// [`Synthetic::PlugKeyboard`] plugged in: its own keys' settings apply.
    KeyOn { keyboard: String, key: String, pressed: bool },
}

fn yes() -> bool {
    true
}

/// The compositor's answer to one request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "kebab-case")]
pub enum Response {
    Ok,
    Error {
        message: String,
    },
    Version {
        version: String,
        protocol: u32,
        /// The compositor's process and program (what aizome-dev asks how
        /// the session was built).
        #[serde(default)]
        pid: u32,
        #[serde(default)]
        exe: Option<String>,
        /// How its core was built (`aizome --build-info`).
        #[serde(default)]
        build: Option<serde_json::Value>,
    },
    Outputs {
        outputs: Vec<OutputInfo>,
    },
    Windows {
        windows: Vec<WindowInfo>,
    },
    Window {
        window: Option<WindowInfo>,
    },
    Desks {
        desks: Vec<DeskInfo>,
        current: u32,
    },
    Stats {
        stats: Stats,
    },
    Screenshot {
        path: String,
    },
    Spawned {
        pid: u32,
    },
    NightLight {
        state: NightLightState,
    },
    Recovery {
        state: RecoveryState,
    },
    /// What an action did, in a sentence.
    Message {
        message: String,
    },
    InputState {
        state: InputState,
    },
    Keyboards {
        keyboards: Vec<KeyboardInfo>,
    },
    Shares {
        shares: Vec<ShareInfo>,
    },
    Reload {
        report: ReloadReport,
    },
}

/// What a reload did (or, checking, would do), part by part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReloadReport {
    pub logic: ReloadPart,
    pub shell: ReloadPart,
    pub settings: ReloadPart,
    /// The compositor's core, which a reload can't replace: `same`, or
    /// `needs-restart` when the installed one differs (log out and in).
    pub core: ReloadPart,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReloadPart {
    /// `reloaded`, `restarted`, `unchanged`, `would-reload`, `refused`,
    /// `failed`, `skipped`, `same` or `needs-restart`.
    pub action: String,
    /// What and why, in a sentence.
    #[serde(default)]
    pub detail: String,
}

impl ReloadPart {
    pub fn new(action: &str, detail: impl Into<String>) -> ReloadPart {
        ReloadPart { action: action.into(), detail: detail.into() }
    }
}

/// The compositor's logic: what's loaded and how it has fared.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LogicStats {
    /// Its build (version and source hash), or empty when none is running.
    pub build: String,
    pub path: String,
    /// When it was loaded, seconds since the epoch.
    pub loaded_at: u64,
    /// Loaded and not failing.
    pub healthy: bool,
    pub reloads: u32,
    /// Rolled back or restarted after a panic.
    pub rollbacks: u32,
    #[serde(default)]
    pub last_error: Option<String>,
    /// The core's ABI fingerprint, which a logic must be built against.
    pub core_abi: String,
}

/// The recovery overlay, as `recovery-state` reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecoveryState {
    pub open: bool,
    /// The session is locked: only safe actions are offered and no apps
    /// are listed.
    pub locked: bool,
    /// One row per app, as the overlay lists them (problems first).
    pub apps: Vec<RecoveryApp>,
    /// The selected row's app ID.
    pub selected: Option<String>,
    /// The actions offered, in order (`force-quit`, `reset-input`, …).
    pub actions: Vec<String>,
    /// The last action's result, or a notice (a restart after a stall).
    pub message: Option<String>,
    /// Where the panel is, in the global layout, while open.
    #[serde(default)]
    pub panel: Option<Rect>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecoveryApp {
    pub app_id: String,
    pub name: String,
    pub windows: u32,
    /// The processes behind its windows (Wayland credentials).
    pub pids: Vec<i32>,
    /// The systemd units (scopes) or cgroups Force Quit would kill.
    pub units: Vec<String>,
    /// CPU use over the last second, percent of one core (`None` until
    /// measured).
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    /// An xdg_wm_base ping has gone unanswered for a while.
    pub not_responding: bool,
    /// Its process is gone, a zombie, stopped or dumping core while its
    /// windows linger.
    pub crashed: bool,
}

/// The seat's input state, for tests and for seeing what is stuck.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputState {
    /// The pointer grab in force: `move`, `resize`, `click`, `popup`,
    /// `dnd`, `marking`… or `None`.
    pub pointer_grab: Option<String>,
    /// Buttons the seat holds as pressed (evdev codes).
    pub pressed_buttons: Vec<u32>,
    pub keyboard_grab: bool,
    /// Keys the seat holds as pressed (evdev codes).
    pub pressed_keys: Vec<u32>,
    /// The focused surface's pointer constraint while active: `locked` or
    /// `confined`.
    pub pointer_constraint: Option<String>,
    pub cursor_hidden: bool,
    /// The effective named cursor, or `None` for a client surface or hidden cursor.
    #[serde(default)]
    pub cursor_name: Option<String>,
    /// The app whose surface has the pointer, and the keyboard (`the lock
    /// screen on <output>` for a lock surface).
    pub pointer_focus: Option<String>,
    pub keyboard_focus: Option<String>,
}

impl Response {
    pub fn error(message: impl Into<String>) -> Response {
        Response::Error { message: message.into() }
    }
}

/// A keyboard, as `[[input.keyboards]]` names it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardInfo {
    /// What it calls itself: `Apple Inc. Magic Keyboard`.
    pub name: String,
    /// Its vendor and product, as `lsusb` writes them: `05ac:029c`.
    pub id: String,
    #[serde(default)]
    pub bus: KeyboardBus,
    /// It typed the last key.
    #[serde(default)]
    pub last: bool,
}

/// How a keyboard is connected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyboardBus {
    Usb,
    Bluetooth,
    /// A laptop's own keyboard (PS/2, I²C, SPI).
    BuiltIn,
    /// Virtual (a VM's, uinput) or unknown.
    #[default]
    Other,
}

impl KeyboardBus {
    /// Plugged in or paired, rather than one the computer came with or a
    /// virtual one.
    pub fn plugged(self) -> bool {
        matches!(self, KeyboardBus::Usb | KeyboardBus::Bluetooth)
    }
}

/// One window, as the shell's Deskbar and the switcher see it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: WindowId,
    pub app_id: String,
    pub title: String,
    pub pid: Option<i32>,
    /// 1-based Desk position; `None` when sticky.
    pub desk: Option<u32>,
    pub sticky: bool,
    /// The tab group, when stacked with others.
    pub group: Option<u64>,
    /// The members of the group in tab order (empty when not stacked).
    pub group_members: Vec<WindowId>,
    /// Whether this is the member of its group that shows.
    pub group_active: bool,
    pub focused: bool,
    pub minimised: bool,
    pub hidden: bool,
    /// Asked for attention (activation without a valid token).
    pub urgent: bool,
    pub fullscreen: bool,
    pub fill: bool,
    pub float: bool,
    /// The content rectangle (inside the frame).
    pub geometry: Rect,
    /// The output showing most of it.
    pub output: Option<String>,
    /// Draws its own title bar; Aizome adds only the tab.
    pub client_side_decorations: bool,
    /// Uses Aizome's kit (app_id `org.aizome.…`, or Genkan's): no
    /// command-key translation.
    pub kit_app: bool,
    /// The window this one is a dialog of.
    pub parent: Option<WindowId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeskInfo {
    /// 1-based position.
    pub index: u32,
    /// Stable identity while the compositor runs.
    pub id: u64,
    pub name: String,
    pub folder: Option<String>,
    pub windows: u32,
    /// The Desk of the display with the focus.
    pub current: bool,
    /// The display showing it (none while it's hidden): each display
    /// shows a Desk of its own.
    #[serde(default)]
    pub output: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mode {
    pub width: i32,
    pub height: i32,
    /// Millihertz.
    pub refresh: i32,
    pub preferred: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub name: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    pub enabled: bool,
    pub current_mode: Option<Mode>,
    pub modes: Vec<Mode>,
    pub scale: f64,
    pub transform: Transform,
    /// Position and size in the global layout, in points.
    pub logical: Rect,
    /// The area left for windows (outside layer-shell exclusive zones).
    pub usable: Rect,
    pub physical_size_mm: (i32, i32),
    pub powered: bool,
    /// The main display: `primary = true` in the settings, else the first.
    #[serde(default)]
    pub primary: bool,
    /// The output whose picture this one shows (it has no place in the
    /// layout of its own then).
    #[serde(default)]
    pub mirror: Option<String>,
    /// Night Light (and gamma ramps) reach this display.
    #[serde(default)]
    pub gamma: bool,
}

/// Night Light, as `night-light` reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NightLightState {
    pub enabled: bool,
    /// The warm temperature, Kelvin.
    pub temperature: u32,
    /// `sunset`, `manual` or `always`.
    pub schedule: String,
    /// Warm now: enabled, and the schedule says so.
    pub active: bool,
    /// What the displays show now, Kelvin (6500 is neutral).
    pub current: u32,
    /// The outputs that can show it (DRM gamma); the others can't.
    pub outputs: Vec<String>,
    /// When the schedule warms the displays today, local `"HH:MM"`: from
    /// (sunset, for the sunset schedule) to (sunrise); none when it's
    /// always or never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// What sunset and sunrise were reckoned for: a time zone
    /// (`Europe/Paris`), `located` (GeoClue) or `settings` (set by hand);
    /// none for the other schedules, or where it isn't known (19:00 to
    /// 07:00 then).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
}

/// Frame-time statistics, per output.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub renderer: String,
    pub backend: String,
    pub outputs: Vec<OutputStats>,
    /// What the settings ask for (`[performance] renderer`: `auto`,
    /// `gpu` or `software`); it takes effect at the next start.
    #[serde(default)]
    pub renderer_setting: String,
    /// The renderer setting changed since this session started. A fallback
    /// or start-time override alone does not require another login.
    #[serde(default)]
    pub renderer_after_restart: bool,
    /// The compositor's logic.
    #[serde(default)]
    pub logic: LogicStats,
    /// wlr-screencopy copies (screencasts, recorders): CPU time per copy,
    /// once a client has made one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screencopy: Option<OutputStats>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputStats {
    pub name: String,
    /// Frames rendered (with damage) since start or the last reset.
    pub frames_rendered: u64,
    /// Redraws that found no damage and so drew nothing.
    pub frames_skipped: u64,
    /// CPU time per rendered frame (collect, render, submit), µs.
    pub cpu_us: Percentiles,
    /// GPU time per frame, µs, when the renderer can measure it.
    pub gpu_us: Option<Percentiles>,
    /// The samples behind the percentiles (the rolling window).
    pub samples: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

/// Something that happened, sent to subscribed connections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    WindowOpened {
        window: WindowInfo,
    },
    WindowClosed {
        id: WindowId,
    },
    /// Title, app_id, desk, state, geometry or group changed.
    WindowChanged {
        window: WindowInfo,
    },
    WindowFocused {
        id: Option<WindowId>,
    },
    /// The current Desk changed.
    DeskChanged {
        index: u32,
        id: u64,
        name: String,
    },
    /// Desks were added, removed, renamed or reordered.
    DesksChanged {
        desks: Vec<DeskInfo>,
    },
    OutputsChanged {
        outputs: Vec<OutputInfo>,
    },
    ConfigChanged,
    /// A key or pointer binding that belongs to the shell (`palette`,
    /// `toolchest`, `lock`, `screenshot-tool`, the media keys, `app-menu`
    /// for ❖-right-click with the pointer position, …; docs/ipc.md lists
    /// them).
    Binding {
        action: String,
        /// Pointer position in the global layout, for pointer bindings.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pointer: Option<(f64, f64)>,
        /// The output under the pointer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    ScreenshotTaken {
        path: String,
    },
    /// No input for the configured time: `lock` or `monitors-off`.
    Idle {
        kind: String,
    },
    Resumed,
    LockStateChanged {
        locked: bool,
    },
    /// Night Light turned on or off, warmed or cooled.
    NightLightChanged {
        state: NightLightState,
    },
    /// Something was reloaded (the compositor's logic, the shell, the
    /// settings).
    Reloaded {
        report: ReloadReport,
    },
    /// The compositor's logic panicked; `action` says what the core did
    /// (rolled back, restarted it, set it aside).
    LogicFailed {
        message: String,
        action: String,
    },
    /// What a [`Request::PickArea`] picked, or `area: null` when the
    /// person gave up (Esc).
    AreaPicked {
        tag: String,
        /// In the global layout, in points; a whole output's rectangle
        /// when it was clicked.
        #[serde(default)]
        area: Option<Rect>,
        /// The output holding the area's centre.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// The window picked (Space, then a click): `area` is its frame
        /// with the tab.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<WindowId>,
        /// The picked window's ext-foreign-toplevel-list identifier, as
        /// xdg-desktop-portal-wlr names windows.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        toplevel: Option<String>,
    },
    /// The picker a [`Request::PickArea`] asked for is up (so a script can
    /// start picking).
    PickStarted {
        tag: String,
    },
    /// What's being shared changed (see [`Request::Shares`]).
    SharesChanged {
        shares: Vec<ShareInfo>,
    },
    /// A keyboard that types appeared, for the first time since the
    /// compositor started: one plugged in again isn't new.
    KeyboardAdded {
        keyboard: KeyboardInfo,
    },
    /// The key a [`Request::CatchKey`] (or [`Request::CatchKeys`]) caught,
    /// sent to the connection that asked alone: its evdev code, the same
    /// whatever the keymap makes of the key, and the keyboard that typed it
    /// (none for a key from IPC).
    KeyCaught {
        tag: String,
        code: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keyboard: Option<KeyboardInfo>,
    },
}

/// `$XDG_RUNTIME_DIR/aizome.$WAYLAND_DISPLAY.sock`.
pub fn socket_path_for(wayland_display: &str) -> PathBuf {
    runtime_dir().join(format!("aizome.{wayland_display}.sock"))
}

/// `$AIZOME_SOCKET`, or the path for `$WAYLAND_DISPLAY`.
pub fn socket_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AIZOME_SOCKET").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let display = std::env::var("WAYLAND_DISPLAY").ok().filter(|v| !v.is_empty())?;
    Some(socket_path_for(&display))
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// A blocking connection to the compositor.
///
/// ```no_run
/// let mut c = aizome_ipc::Client::connect()?;
/// let windows = c.request(&aizome_ipc::Request::Windows)?;
/// for event in c.events()? {
///     println!("{:?}", event?);
/// }
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Client {
    /// Connects to `$AIZOME_SOCKET` (or the socket for `$WAYLAND_DISPLAY`).
    pub fn connect() -> io::Result<Client> {
        let path = socket_path().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "neither AIZOME_SOCKET nor WAYLAND_DISPLAY is set")
        })?;
        Client::connect_to(&path)
    }

    pub fn connect_to(path: &Path) -> io::Result<Client> {
        let stream = UnixStream::connect(path)?;
        Self::from_stream(stream)
    }

    /// Use an established connection, retaining its caller's timeout policy.
    pub fn from_stream(stream: UnixStream) -> io::Result<Client> {
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Client { writer: stream, reader })
    }

    /// Sends a request and waits for its response.
    pub fn request(&mut self, request: &Request) -> io::Result<Response> {
        let mut line = serde_json::to_string(request).map_err(io::Error::other)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        self.writer.flush()?;
        let mut buf = String::new();
        if self.reader.read_line(&mut buf)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the compositor closed the connection"));
        }
        serde_json::from_str(buf.trim_end()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Like `request`, but turns `Response::Error` into an `Err`.
    pub fn call(&mut self, request: &Request) -> io::Result<Response> {
        match self.request(request)? {
            Response::Error { message } => Err(io::Error::other(message)),
            r => Ok(r),
        }
    }

    /// Subscribes and returns the event stream. Requests can still go on
    /// it, answered among the events ([`Events::request`]).
    pub fn events(mut self) -> io::Result<Events> {
        match self.request(&Request::Subscribe)? {
            Response::Ok => Ok(Events { reader: self.reader, writer: self.writer, early: VecDeque::new() }),
            Response::Error { message } => Err(io::Error::other(message)),
            other => Err(io::Error::new(io::ErrorKind::InvalidData, format!("unexpected {other:?}"))),
        }
    }

    /// The underlying socket, e.g. to set a read timeout.
    pub fn stream(&self) -> &UnixStream {
        &self.writer
    }
}

/// Events from a subscribed connection. Ends when the compositor closes it.
pub struct Events {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    /// Events that came while a request waited for its response.
    early: VecDeque<Event>,
}

impl Events {
    pub fn stream(&self) -> &UnixStream {
        self.reader.get_ref()
    }

    /// Sends a request on this connection and waits for its response,
    /// keeping the events that come first for [`Iterator::next`]: what
    /// only this connection hears (a [`Request::CatchKey`]'s key) is asked
    /// for here. Responses come in order, after the events sent before.
    pub fn request(&mut self, request: &Request) -> io::Result<Response> {
        let mut line = serde_json::to_string(request).map_err(io::Error::other)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        self.writer.flush()?;
        let bad = |e: serde_json::Error| io::Error::new(io::ErrorKind::InvalidData, e);
        loop {
            let mut buf = String::new();
            if self.reader.read_line(&mut buf)? == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the compositor closed the connection"));
            }
            let v: serde_json::Value = serde_json::from_str(buf.trim_end()).map_err(bad)?;
            if v.get("event").is_none() {
                return serde_json::from_value(v).map_err(bad);
            }
            // One this side doesn't know (a newer compositor's) is skipped.
            if let Ok(e) = serde_json::from_value(v) {
                self.early.push_back(e);
            }
        }
    }
}

impl Iterator for Events {
    type Item = io::Result<Event>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(e) = self.early.pop_front() {
            return Some(Ok(e));
        }
        let mut buf = String::new();
        match self.reader.read_line(&mut buf) {
            Ok(0) => None,
            Ok(_) => {
                Some(serde_json::from_str(buf.trim_end()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboards_are_named_as_settings_name_them() {
        let r: Request = serde_json::from_str(r#"{"request":"catch-key","tag":"setup"}"#).unwrap();
        assert_eq!(r, Request::CatchKey { tag: Some("setup".into()) });
        let r: Request = serde_json::from_str(r#"{"request":"catch-key"}"#).unwrap();
        assert_eq!(r, Request::CatchKey { tag: None });
        let r: Request =
            serde_json::from_str(r#"{"request":"catch-keys","tag":"home","keys":["home","f1"],"windows":[7]}"#)
                .unwrap();
        assert_eq!(
            r,
            Request::CatchKeys { tag: "home".into(), keys: vec!["home".into(), "f1".into()], windows: vec![7] }
        );
        let r: Request = serde_json::from_str(r#"{"request":"catch-keys","tag":"home"}"#).unwrap();
        assert_eq!(r, Request::CatchKeys { tag: "home".into(), keys: Vec::new(), windows: Vec::new() });
        let k = KeyboardInfo {
            name: "Magic Keyboard".into(),
            id: "004c:029c".into(),
            bus: KeyboardBus::Bluetooth,
            last: true,
        };
        let e = Event::KeyCaught { tag: "setup".into(), code: 125, keyboard: Some(k) };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"event":"key-caught","tag":"setup","code":125,"keyboard":{"name":"Magic Keyboard","id":"004c:029c","bus":"bluetooth","last":true}}"#
        );
        let s: Synthetic = serde_json::from_str(r#"{"type":"plug-keyboard","name":"K120","id":"046d:c31c"}"#).unwrap();
        assert_eq!(s, Synthetic::PlugKeyboard { name: "K120".into(), id: "046d:c31c".into(), bus: KeyboardBus::Other });
        assert!(KeyboardBus::Usb.plugged() && !KeyboardBus::BuiltIn.plugged() && !KeyboardBus::Other.plugged());
    }

    #[test]
    fn requests_are_tagged_in_kebab_case() {
        let r = Request::Focus { id: 7 };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"request":"focus","id":7}"#);
        let r: Request = serde_json::from_str(r#"{"request":"close"}"#).unwrap();
        assert_eq!(r, Request::Close { id: None });
        let r: Request = serde_json::from_str(r#"{"request":"move-to-desk","desk":2}"#).unwrap();
        assert_eq!(r, Request::MoveToDesk { id: None, desk: DeskSel::Index(2), follow: false });
        let r: Request = serde_json::from_str(r#"{"request":"set-audible","pids":[42,7]}"#).unwrap();
        assert_eq!(r, Request::SetAudible { pids: vec![42, 7] });
        let r: Request = serde_json::from_str(r#"{"request":"set-desk","desk":"Mail"}"#).unwrap();
        assert_eq!(r, Request::SetDesk { desk: DeskSel::Name("Mail".into()), output: None });
        let r: Request = serde_json::from_str(r#"{"request":"hide-app","app_id":"foot"}"#).unwrap();
        assert_eq!(r, Request::HideApp { app: AppSel { app_id: Some("foot".into()), pid: None } });
        let r: Request = serde_json::from_str(r#"{"request":"spawn","argv":["foot"]}"#).unwrap();
        assert_eq!(r, Request::Spawn { argv: vec!["foot".into()], cwd: None, in_desk_folder: true });
    }

    #[test]
    fn responses_and_events_round_trip() {
        let r = Response::Desks {
            desks: vec![DeskInfo { index: 1, id: 1, name: "1".into(), ..Default::default() }],
            current: 1,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&s).unwrap(), r);
        let e = Event::Binding { action: "palette".into(), pointer: None, output: None };
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"event":"binding","action":"palette"}"#);
        let r: Request = serde_json::from_str(r#"{"request":"pick-area","tag":"record"}"#).unwrap();
        assert_eq!(r, Request::PickArea { tag: "record".into(), prompt: None, targets: Vec::new() });
        let r: Request = serde_json::from_str(
            r#"{"request":"pick-area","tag":"share","prompt":"Share","targets":["display","window"]}"#,
        )
        .unwrap();
        assert_eq!(
            r,
            Request::PickArea {
                tag: "share".into(),
                prompt: Some("Share".into()),
                targets: vec![PickTarget::Display, PickTarget::Window]
            }
        );
        let e = Event::SharesChanged {
            shares: vec![ShareInfo { id: 3, output: Some("eDP-1".into()), ..Default::default() }],
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"event":"shares-changed","shares":[{"id":3,"output":"eDP-1","cursor":false}]}"#
        );
        let e = Event::AreaPicked { tag: "record".into(), area: None, output: None, window: None, toplevel: None };
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"event":"area-picked","tag":"record","area":null}"#);
        assert_eq!(serde_json::from_str::<Event>(r#"{"event":"area-picked","tag":"t"}"#).unwrap(), e_tag("t"));
    }

    fn e_tag(tag: &str) -> Event {
        Event::AreaPicked { tag: tag.into(), area: None, output: None, window: None, toplevel: None }
    }

    #[test]
    fn socket_path_uses_runtime_dir() {
        let p = socket_path_for("wayland-3");
        assert!(p.to_string_lossy().ends_with("aizome.wayland-3.sock"));
    }
}
