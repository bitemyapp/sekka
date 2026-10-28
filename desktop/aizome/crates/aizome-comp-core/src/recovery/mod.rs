//! The recovery overlay (Ctrl+Alt+Del): a short list of the running apps,
//! one row per app with its CPU, memory and whether it answers, and the
//! few things that get a stuck desktop going again: Force Quit, Close,
//! Reset Input, Restart Shell, Reload Aizome, Lock, Log Out and Text
//! Console.
//!
//! The compositor draws it itself (with the overlays' painter), so it
//! works when the shell or an app is hung, over full-screen games, over
//! the lock screen (offering only what is safe there), and with keyboard
//! shortcuts inhibited. Ctrl+Alt+Del is checked before anything else
//! looks at a key: bindings, grabs, inhibitors and the lock. While open
//! the overlay has the keyboard and the pointer; Escape or Ctrl+Alt+Del
//! closes it.
//!
//! The rest of the compositor reaches it through a few hooks: [`key`],
//! [`pointer_button`], [`pointer_moved`] and [`scroll`] in input,
//! [`elements`] in the scene, [`handle_request`] for IPC and [`start`] at
//! start-up. Process statistics come from a thread ([`procinfo`]); apps'
//! answers to pings from [`ping`].

pub mod ping;
pub mod procinfo;
pub mod reset;
mod view;
mod x11;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use aizome_ipc::{RecoveryApp, RecoveryState, Request, Response};
use smithay::input::keyboard::{Keycode, Keysym, ModifiersState, keysyms};
use smithay::input::pointer::{CursorIcon, MotionEvent};
use smithay::output::Output;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{IsAlive, Logical, Point, SERIAL_COUNTER};
use smithay::wayland::pointer_constraints::with_pointer_constraint;

pub use view::items;

use crate::WinId;
use crate::input::Decision;
use crate::state::State;
use procinfo::{AppProcs, ProcInfo, Report, Sample};

/// What the overlay can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Act {
    Close,
    ForceQuit,
    ResetInput,
    RestartShell,
    ReloadCompositor,
    Lock,
    LogOut,
    TextConsole,
}

impl Act {
    pub const APP: [Act; 2] = [Act::Close, Act::ForceQuit];
    pub const SYSTEM: [Act; 6] =
        [Act::ResetInput, Act::RestartShell, Act::ReloadCompositor, Act::Lock, Act::LogOut, Act::TextConsole];
    /// What is offered over the lock screen: nothing that ends anyone's
    /// work or shows what is open.
    pub const LOCKED: [Act; 3] = [Act::ResetInput, Act::RestartShell, Act::TextConsole];

    pub fn name(self) -> &'static str {
        match self {
            Act::Close => "close",
            Act::ForceQuit => "force-quit",
            Act::ResetInput => "reset-input",
            Act::RestartShell => "restart-shell",
            Act::ReloadCompositor => "reload-compositor",
            Act::Lock => "lock",
            Act::LogOut => "log-out",
            Act::TextConsole => "text-console",
        }
    }

    pub fn parse(s: &str) -> Option<Act> {
        [Act::APP.as_slice(), Act::SYSTEM.as_slice()].concat().into_iter().find(|a| a.name() == s)
    }

    pub fn label(self, locked: bool) -> &'static str {
        match self {
            Act::Close => "Close",
            Act::ForceQuit => "Force Quit",
            Act::ResetInput => "Reset Input",
            Act::RestartShell if locked => "Restart Lock Screen",
            Act::RestartShell => "Restart Shell",
            Act::ReloadCompositor => "Reload Aizome",
            Act::Lock => "Lock",
            Act::LogOut => "Log Out",
            Act::TextConsole => "Text Console",
        }
    }

    /// Its key while the overlay is open.
    pub fn key_cap(self) -> &'static str {
        match self {
            Act::Close => "W",
            Act::ForceQuit => "Del",
            Act::ResetInput => "R",
            Act::RestartShell => "S",
            Act::ReloadCompositor => "A",
            Act::Lock => "L",
            Act::LogOut => "O",
            Act::TextConsole => "T",
        }
    }

    fn for_key(sym: u32) -> Option<Act> {
        Some(match sym {
            keysyms::KEY_w | keysyms::KEY_W => Act::Close,
            keysyms::KEY_r | keysyms::KEY_R => Act::ResetInput,
            keysyms::KEY_s | keysyms::KEY_S => Act::RestartShell,
            keysyms::KEY_a | keysyms::KEY_A => Act::ReloadCompositor,
            keysyms::KEY_l | keysyms::KEY_L => Act::Lock,
            keysyms::KEY_o | keysyms::KEY_O => Act::LogOut,
            keysyms::KEY_t | keysyms::KEY_T => Act::TextConsole,
            _ => return None,
        })
    }

    pub fn per_app(self) -> bool {
        matches!(self, Act::Close | Act::ForceQuit)
    }
}

/// What has the keyboard's attention: an app's row or an action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    App(String),
    Act(Act),
}

/// What lies under the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    App(String),
    Act(Act),
    Panel,
}

/// One app: its windows, the processes behind them, and how it's doing.
#[derive(Clone, Debug)]
pub struct Row {
    pub app_id: String,
    pub name: String,
    pub windows: Vec<WinId>,
    pub pids: Vec<i32>,
    pub not_responding: bool,
    pub crashed: bool,
    pub stopped: bool,
}

impl Row {
    pub fn in_trouble(&self) -> bool {
        self.not_responding || self.crashed || self.stopped
    }
}

/// How often apps are pinged with the overlay closed, and open.
const PING_EVERY: Duration = Duration::from_secs(2);
const PING_EVERY_OPEN: Duration = Duration::from_millis(500);
/// How often CPU and memory are measured while open.
const SAMPLE_EVERY: Duration = Duration::from_secs(1);
/// A second press of Log Out within this confirms it.
const CONFIRM_WITHIN: Duration = Duration::from_secs(4);
/// How long a force-quit app's windows may linger before its connection
/// is cut.
const LINGER: Duration = Duration::from_millis(1500);

pub struct Recovery {
    pub open: bool,
    /// The output the panel is on (the pointer's when it opened).
    pub output: Option<Output>,
    pub rows: Vec<Row>,
    pub sel: Option<Sel>,
    /// The app the keyboard was last on: Close and Force Quit act on it
    /// while an action has the selection.
    pub last_app: Option<String>,
    pub hover: Option<Hit>,
    /// The first row showing.
    pub scroll: usize,
    /// The last action's result.
    pub message: Option<String>,
    /// Said once, the next time it opens (a restart after a stall).
    pub notice: Option<String>,
    /// An action waiting for a second press.
    confirm: Option<(Act, Instant)>,
    /// Keys and buttons pressed into the overlay: their releases are its
    /// too, even after it closes.
    swallow_keys: HashSet<u32>,
    swallow_buttons: HashSet<u32>,
    samples: HashMap<String, Sample>,
    names: HashMap<String, String>,
    procinfo: Option<ProcInfo>,
    last_ping: Option<Instant>,
    last_sample: Option<Instant>,
    /// When it opened and when its first frame was drawn (for the log).
    opened_at: Option<Instant>,
    /// Drawing state, taken out while drawing.
    view: Option<Box<view::View>>,
    /// The surface whose pointer constraint was active when it opened
    /// (released while it shows, back when it closes).
    pub held_constraint: Option<WlSurface>,
    /// After Reset Input, this surface's constraints stay off until it is
    /// clicked: a hung game can't lock the pointer again at once.
    pub unconstrained: Option<WlSurface>,
    /// The scales and palette its textures were last drawn offscreen for.
    gpu_warm: Option<(Vec<u32>, aizome_theme::Palette)>,
}

impl Default for Recovery {
    fn default() -> Recovery {
        Recovery {
            open: false,
            output: None,
            rows: Vec::new(),
            sel: None,
            last_app: None,
            hover: None,
            scroll: 0,
            message: None,
            notice: None,
            confirm: None,
            swallow_keys: HashSet::new(),
            swallow_buttons: HashSet::new(),
            samples: HashMap::new(),
            names: HashMap::new(),
            procinfo: None,
            last_ping: None,
            last_sample: None,
            opened_at: None,
            view: Some(Box::default()),
            held_constraint: None,
            unconstrained: None,
            gpu_warm: None,
        }
    }
}

impl Recovery {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The actions offered now, in order.
    pub fn actions(&self, locked: bool) -> Vec<Act> {
        if locked { Act::LOCKED.to_vec() } else { [Act::APP.as_slice(), Act::SYSTEM.as_slice()].concat() }
    }

    fn selected_app(&self) -> Option<&Row> {
        match &self.sel {
            Some(Sel::App(a)) => self.rows.iter().find(|r| &r.app_id == a),
            _ => None,
        }
        .or_else(|| self.last_app.as_ref().and_then(|a| self.rows.iter().find(|r| &r.app_id == a)))
    }
}

/// Starts the process thread and the ping timer.
pub fn start(state: &mut State) {
    state.az.recovery.procinfo = ProcInfo::start(&state.az.lh, on_report);
    // Its labels now, and their textures once there is something to draw
    // with, so even the first Ctrl+Alt+Del draws nothing new.
    warm(state);
    let _ = state.az.lh.insert_source(Timer::from_duration(Duration::from_millis(300)), |_, _, state| {
        prewarm_gpu(state);
        TimeoutAction::Drop
    });
    if let Some(notice) = crate::watchdog::previous_stall() {
        state.az.recovery.notice = Some(notice);
        // Say it as soon as there is something to show it on.
        let _ = state.az.lh.insert_source(Timer::from_duration(Duration::from_millis(500)), |_, _, state| {
            if state.az.outputs.is_empty() {
                return TimeoutAction::ToDuration(Duration::from_millis(500));
            }
            open(state);
            TimeoutAction::Drop
        });
    }
    let _ = state.az.lh.insert_source(Timer::from_duration(PING_EVERY), |_, _, state: &mut State| {
        tick(state);
        TimeoutAction::ToDuration(if state.az.recovery.open { PING_EVERY_OPEN } else { PING_EVERY })
    });
}

/// Pings, measures and keeps the rows current; warms the labels and icons
/// the overlay will draw so opening it draws nothing new.
fn tick(state: &mut State) {
    let now = Instant::now();
    let r = &state.az.recovery;
    let due = r.last_ping.is_none_or(|t| now.duration_since(t) >= if r.open { PING_EVERY_OPEN } else { PING_EVERY });
    if due {
        ping_all(state);
    }
    warm(state);
    prewarm_gpu(state);
    if state.az.recovery.open {
        let before = rows_signature(&state.az.recovery.rows);
        refresh_rows(state);
        if state.az.recovery.last_sample.is_none_or(|t| now.duration_since(t) >= SAMPLE_EVERY) {
            request_sample(state);
        }
        if rows_signature(&state.az.recovery.rows) != before {
            redraw(state);
        }
    }
}

/// A window went: an open overlay drops an app whose last window it was
/// at once, rather than at its next tick (a force-quit app would linger
/// for up to a second).
pub fn windows_changed(state: &mut State) {
    if !state.az.recovery.open {
        return;
    }
    let before = rows_signature(&state.az.recovery.rows);
    refresh_rows(state);
    if rows_signature(&state.az.recovery.rows) != before {
        redraw(state);
    }
}

fn ping_all(state: &mut State) {
    state.az.recovery.last_ping = Some(Instant::now());
    let toplevels: Vec<_> = state.az.windows.values().map(|m| m.toplevel().clone()).collect();
    ping::ping(toplevels);
}

/// Names and icons of apps with windows, and the overlay's fixed labels.
fn warm(state: &mut State) {
    let apps: Vec<String> = state.az.apps_mru().into_iter().map(|(a, _)| a).collect();
    let scales: Vec<f64> = state.az.outputs.iter().map(|o| o.current_scale().fractional_scale()).collect();
    for app in apps {
        if !state.az.recovery.names.contains_key(&app) {
            let name = state.az.icons.app_name(&app).unwrap_or_else(|| short_name(&app));
            state.az.recovery.names.insert(app.clone(), name);
        }
        for s in &scales {
            let _ = state.az.icons.texture(&app, view::ICON, s.ceil().max(1.0) as u32, crate::icons::Tint::Color);
        }
    }
    let names: Vec<String> = state.az.recovery.names.values().cloned().collect();
    if let Some(mut v) = state.az.recovery.view.take() {
        v.warm(&state.az.palette, &scales, &names);
        state.az.recovery.view = Some(v);
    }
}

/// Draws the overlay once offscreen, as a screenshot does, so its labels'
/// textures are already on the GPU when it first opens (uploading them
/// then cost the first frame several milliseconds). Again when the scale
/// or the theme changes.
fn prewarm_gpu(state: &mut State) {
    let scales: Vec<u32> =
        state.az.outputs.iter().map(|o| (o.current_scale().fractional_scale() * 4.0).round() as u32).collect();
    let key = (scales, state.az.palette);
    if state.az.recovery.open || state.az.outputs.is_empty() || state.az.recovery.gpu_warm.as_ref() == Some(&key) {
        return;
    }
    state.az.recovery.gpu_warm = Some(key);
    let started = Instant::now();
    let outputs = state.az.outputs.clone();
    refresh_rows(state);
    state.az.recovery.open = true;
    for o in &outputs {
        state.az.recovery.output = Some(o.clone());
        if let Err(e) = state.capture_output(o, false) {
            tracing::debug!("warming the recovery overlay on {}: {e}", o.name());
        }
    }
    state.az.recovery.open = false;
    state.az.recovery.output = None;
    tracing::debug!(took = ?started.elapsed(), "recovery overlay warmed");
}

fn short_name(app_id: &str) -> String {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    let mut c = last.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "Unnamed app".to_string(),
    }
}

/// The rows from the window list: apps in trouble first, then by recent
/// use.
fn refresh_rows(state: &mut State) {
    let az = &mut state.az;
    let mut rows = Vec::new();
    for (app_id, windows) in az.apps_mru() {
        let mut pids: Vec<i32> = windows.iter().filter_map(|w| az.window_pid(*w)).collect();
        pids.sort();
        pids.dedup();
        let not_responding =
            windows.iter().any(|w| az.windows.get(w).is_some_and(|m| ping::not_responding(m.toplevel())));
        let sample = az.recovery.samples.get(&app_id);
        let name = match az.recovery.names.get(&app_id) {
            Some(n) => n.clone(),
            None => {
                let n = az.icons.app_name(&app_id).unwrap_or_else(|| short_name(&app_id));
                az.recovery.names.insert(app_id.clone(), n.clone());
                n
            }
        };
        rows.push(Row {
            name,
            windows,
            pids,
            not_responding,
            crashed: sample.is_some_and(|s| s.crashed),
            stopped: sample.is_some_and(|s| s.stopped),
            app_id,
        });
    }
    rows.sort_by_key(|r| !r.in_trouble());
    let r = &mut az.recovery;
    r.rows = rows;
    // With no app left, the app actions have nothing to act on.
    if r.rows.is_empty() && matches!(r.sel, Some(Sel::Act(a)) if a.per_app()) {
        r.sel = Some(Sel::Act(Act::ResetInput));
    }
    // Keep the selection on its app; an app that went selects the next.
    if let Some(Sel::App(a)) = &r.sel
        && !r.rows.iter().any(|row| &row.app_id == a)
    {
        r.sel = r.rows.first().map(|row| Sel::App(row.app_id.clone())).or(Some(Sel::Act(Act::ResetInput)));
    }
    if r.last_app.as_ref().is_some_and(|a| !r.rows.iter().any(|row| &row.app_id == a)) {
        r.last_app = None;
    }
    r.scroll = r.scroll.min(r.rows.len().saturating_sub(view::VISIBLE_ROWS));
}

fn rows_signature(rows: &[Row]) -> Vec<(String, usize, bool, bool, bool)> {
    rows.iter().map(|r| (r.app_id.clone(), r.windows.len(), r.not_responding, r.crashed, r.stopped)).collect()
}

/// xwayland-satellite's process and display, while it runs.
fn x11_info(state: &State) -> Option<procinfo::X11> {
    let satellite = state.az.children.pid_of("xwayland-satellite")? as i32;
    let display = state.az.children.env.iter().find(|(k, _)| k == "DISPLAY").map(|(_, v)| v.clone())?;
    Some(procinfo::X11 { satellite, display })
}

fn request_sample(state: &mut State) {
    let x11 = x11_info(state);
    let r = &mut state.az.recovery;
    r.last_sample = Some(Instant::now());
    let apps: Vec<AppProcs> = r
        .rows
        .iter()
        .map(|row| AppProcs { key: row.app_id.clone(), pids: row.pids.clone(), x11: x11.clone() })
        .collect();
    if let Some(p) = &r.procinfo {
        p.send(procinfo::Request::Sample(apps));
    }
}

/// The process thread answered.
fn on_report(state: &mut State, report: Report) {
    match report {
        Report::Samples(samples) => {
            let r = &mut state.az.recovery;
            for s in samples {
                r.samples.insert(s.key.clone(), s);
            }
            if r.open {
                refresh_rows(state);
                redraw(state);
            }
        }
        Report::Killed { key, result } => {
            let name = state.az.recovery.names.get(&key).cloned().unwrap_or(key.clone());
            let msg = match result {
                Ok(what) => format!("Force quit {name} ({what})"),
                Err(e) => format!("Couldn't force quit {name}: {e}"),
            };
            state.az.recovery.message = Some(msg);
            redraw(state);
        }
    }
}

fn redraw(state: &mut State) {
    if state.az.recovery.open {
        state.az.queue_redraw_all();
    }
}

// ---- opening and closing ----------------------------------------------------

pub fn open(state: &mut State) {
    if state.az.recovery.open {
        return;
    }
    let started = Instant::now();
    let locked = state.az.lock.is_locked();
    let output = state.az.active_output().or_else(|| state.az.outputs.first().cloned());
    {
        let r = &mut state.az.recovery;
        r.open = true;
        r.output = output;
        r.hover = None;
        r.confirm = None;
        r.scroll = 0;
        r.opened_at = Some(started);
        r.message = r.notice.take();
    }
    refresh_rows(state);
    // The app in trouble, else the focused window's app, else the first.
    let focused_app = state.az.view.focused.and_then(|w| state.az.window_app(w));
    let r = &mut state.az.recovery;
    let first_trouble = r.rows.iter().find(|row| row.in_trouble()).map(|row| row.app_id.clone());
    let pick = if locked {
        None
    } else {
        first_trouble
            .or(focused_app.filter(|a| r.rows.iter().any(|row| &row.app_id == a)))
            .or_else(|| r.rows.first().map(|row| row.app_id.clone()))
    };
    r.last_app = pick.clone();
    r.sel = Some(match pick {
        Some(a) => Sel::App(a),
        None => Sel::Act(Act::ResetInput),
    });
    take_input(state);
    ping_all(state);
    request_sample(state);
    state.az.queue_redraw_all();
    tracing::info!(took = ?started.elapsed(), locked, "recovery overlay opened");
}

pub fn close(state: &mut State) {
    if !state.az.recovery.open {
        return;
    }
    {
        let r = &mut state.az.recovery;
        r.open = false;
        r.hover = None;
        r.confirm = None;
        r.message = None;
    }
    state.az.cursor_override = None;
    state.update_keyboard_focus();
    // The pointer goes back to whatever is under it.
    let pos = state.az.pointer_pos;
    let time = state.az.start.elapsed().as_millis() as u32;
    state.pointer_motion_absolute(pos, time);
    state.az.queue_redraw_all();
    tracing::info!("recovery overlay closed");
}

pub fn toggle(state: &mut State) {
    if state.az.recovery.open { close(state) } else { open(state) }
}

/// The overlay takes the keyboard (clients hear leave, so their held keys
/// are let go) and the pointer: grabs end, menus close, nothing has the
/// pointer's focus, a locked pointer is free, and the cursor shows.
fn take_input(state: &mut State) {
    if state.az.popup_grab.is_some() {
        state.dismiss_popup_grab();
    }
    let time = state.az.start.elapsed().as_millis() as u32;
    if let Some(pointer) = state.az.seat.get_pointer() {
        // A locked or confined pointer is free while it shows.
        state.az.recovery.held_constraint = None;
        if let Some(f) = pointer.current_focus() {
            let mut active = false;
            with_pointer_constraint(&f.0, &pointer, |c| {
                if let Some(c) = c
                    && c.is_active()
                {
                    c.deactivate();
                    active = true;
                }
            });
            state.az.recovery.held_constraint = active.then(|| f.0.clone());
        }
        let location = pointer.current_location();
        // Off every surface first, so a drag and drop is cancelled rather
        // than dropped where the pointer is.
        pointer.motion(state, None, &MotionEvent { location, serial: SERIAL_COUNTER.next_serial(), time });
        if pointer.is_grabbed() {
            pointer.unset_grab(state, SERIAL_COUNTER.next_serial(), time);
            pointer.motion(state, None, &MotionEvent { location, serial: SERIAL_COUNTER.next_serial(), time });
        }
        pointer.frame(state);
    }
    // (Ending the grab ended the logic's drags.)
    state.az.cursor_override = Some(CursorIcon::Default);
    state.update_keyboard_focus();
}

// ---- input ----------------------------------------------------------------

/// A key, before anything else sees it (bindings, inhibitors, the lock,
/// grabs). `None` lets it through.
pub(crate) fn key(
    state: &mut State,
    keycode: Keycode,
    pressed: bool,
    mods: &ModifiersState,
    syms: &[Keysym],
) -> Option<Decision> {
    let code = keycode.raw();
    let r = &mut state.az.recovery;
    if !pressed {
        if r.swallow_keys.remove(&code) {
            return Some(Decision::Swallow);
        }
        if r.open {
            // A key pressed before it opened: its client has had leave.
            state.az.suppressed_keys.remove(&keycode);
            state.az.translated_keys.remove(&keycode);
            return Some(Decision::Swallow);
        }
        return None;
    }
    let is = |k: u32| syms.iter().any(|s| s.raw() == k);
    let delete = is(keysyms::KEY_Delete) || is(keysyms::KEY_KP_Delete) || code == 111 + 8;
    if delete && mods.ctrl && mods.alt {
        r.swallow_keys.insert(code);
        toggle(state);
        return Some(Decision::Swallow);
    }
    if !r.open {
        return None;
    }
    r.swallow_keys.insert(code);
    let sym = syms.iter().map(|s| s.raw()).find(|s| {
        !matches!(
            *s,
            keysyms::KEY_Control_L
                | keysyms::KEY_Control_R
                | keysyms::KEY_Alt_L
                | keysyms::KEY_Alt_R
                | keysyms::KEY_Shift_L
                | keysyms::KEY_Shift_R
                | keysyms::KEY_Super_L
                | keysyms::KEY_Super_R
        )
    });
    if let Some(sym) = sym {
        key_open(state, sym, mods, delete);
    }
    Some(Decision::Swallow)
}

fn key_open(state: &mut State, sym: u32, mods: &ModifiersState, delete: bool) {
    let locked = state.az.lock.is_locked();
    match sym {
        keysyms::KEY_Escape => close(state),
        keysyms::KEY_Up | keysyms::KEY_KP_Up => step(state, -1, false),
        keysyms::KEY_Down | keysyms::KEY_KP_Down => step(state, 1, false),
        keysyms::KEY_Left | keysyms::KEY_KP_Left => step(state, -1, true),
        keysyms::KEY_Right | keysyms::KEY_KP_Right => step(state, 1, true),
        keysyms::KEY_Tab => step(state, if mods.shift { -1 } else { 1 }, true),
        keysyms::KEY_ISO_Left_Tab => step(state, -1, true),
        keysyms::KEY_Home => {
            let first = state.az.recovery.rows.first().map(|r| Sel::App(r.app_id.clone()));
            select(state, first.or(Some(Sel::Act(Act::ResetInput))));
        }
        keysyms::KEY_End => {
            let last = *state.az.recovery.actions(locked).last().unwrap_or(&Act::ResetInput);
            select(state, Some(Sel::Act(last)));
        }
        keysyms::KEY_Return | keysyms::KEY_KP_Enter | keysyms::KEY_space => activate(state),
        _ if delete => {
            if !locked {
                do_act(state, Act::ForceQuit);
            }
        }
        s => {
            if let Some(a) = Act::for_key(s)
                && state.az.recovery.actions(locked).contains(&a)
            {
                do_act(state, a);
            }
        }
    }
}

/// Moves the selection: through the rows, then the actions (`across`:
/// left and right, Tab).
fn step(state: &mut State, d: isize, across: bool) {
    let locked = state.az.lock.is_locked();
    let r = &state.az.recovery;
    let mut order: Vec<Sel> = Vec::new();
    if !locked {
        order.extend(r.rows.iter().map(|row| Sel::App(row.app_id.clone())));
    }
    let rows = order.len();
    order.extend(r.actions(locked).into_iter().map(Sel::Act));
    let i = r.sel.as_ref().and_then(|s| order.iter().position(|o| o == s)).unwrap_or(0) as isize;
    let in_rows = (i as usize) < rows;
    let next = if !across && !in_rows {
        // Up and down move between the action rows, and from the first
        // action row back up to the apps.
        let acts = r.actions(locked);
        let ai = (i as usize - rows) as isize;
        let per_row = view::layout_rows(&acts);
        let (row_i, col) = per_row
            .iter()
            .enumerate()
            .find_map(|(ri, rr)| rr.iter().position(|a| *a == acts[ai as usize]).map(|c| (ri, c)))
            .unwrap_or((0, 0));
        let target_row = row_i as isize + d;
        if target_row < 0 {
            if rows > 0 { rows as isize - 1 } else { i }
        } else if let Some(rr) = per_row.get(target_row as usize) {
            let a = rr[col.min(rr.len() - 1)];
            rows as isize + acts.iter().position(|x| *x == a).unwrap_or(0) as isize
        } else {
            i
        }
    } else if !across && in_rows && i + d >= rows as isize {
        rows as isize
    } else {
        (i + d).clamp(0, order.len() as isize - 1)
    };
    let sel = order.get(next as usize).cloned();
    select(state, sel);
}

fn select(state: &mut State, sel: Option<Sel>) {
    let r = &mut state.az.recovery;
    if let Some(Sel::App(a)) = &sel {
        r.last_app = Some(a.clone());
        // Keep it in view.
        if let Some(i) = r.rows.iter().position(|row| &row.app_id == a) {
            if i < r.scroll {
                r.scroll = i;
            } else if i >= r.scroll + view::VISIBLE_ROWS {
                r.scroll = i + 1 - view::VISIBLE_ROWS;
            }
        }
    }
    if r.sel != sel {
        r.sel = sel;
        r.confirm = None;
        redraw(state);
    }
}

/// Return: an action does itself; an app's row closes it, or force quits
/// it when it doesn't answer.
fn activate(state: &mut State) {
    match state.az.recovery.sel.clone() {
        Some(Sel::Act(a)) => do_act(state, a),
        Some(Sel::App(a)) => {
            let trouble = state.az.recovery.rows.iter().find(|r| r.app_id == a).is_some_and(|r| r.in_trouble());
            do_act(state, if trouble { Act::ForceQuit } else { Act::Close });
        }
        None => {}
    }
}

/// The default for an app's row.
pub fn default_act(row: &Row) -> Act {
    if row.in_trouble() { Act::ForceQuit } else { Act::Close }
}

/// A client asked for a new pointer constraint: Reset Input's hold on its
/// surface's constraints ends.
pub fn constraint_requested(az: &mut crate::state::Aizome, surface: &WlSurface) {
    if az.recovery.unconstrained.as_ref() == Some(surface) {
        az.recovery.unconstrained = None;
    }
}

/// Whether a surface's pointer constraint may come on (not after Reset
/// Input, until the surface is clicked or asks for a new one).
pub fn may_constrain(az: &crate::state::Aizome, surface: &WlSurface) -> bool {
    az.recovery.unconstrained.as_ref() != Some(surface)
}

/// A button: true when the overlay took it (a press while open, or the
/// release of one).
pub fn pointer_button(state: &mut State, button: u32, pressed: bool) -> bool {
    // A click on a surface whose constraints a reset turned off lets
    // them back on.
    if pressed
        && !state.az.recovery.open
        && let Some(s) = state.az.recovery.unconstrained.clone()
    {
        let pos = state.az.pointer_pos;
        if matches!(state.surface_at(pos), Some((f, _, _)) if f.0 == s) || !s.alive() {
            state.az.recovery.unconstrained = None;
        }
    }
    let r = &mut state.az.recovery;
    if !pressed {
        return r.swallow_buttons.remove(&button);
    }
    if !r.open {
        return false;
    }
    r.swallow_buttons.insert(button);
    if button != crate::input::BTN_LEFT {
        return true;
    }
    let pos = state.az.pointer_pos;
    match view::hit(state, pos) {
        Some(Hit::App(a)) => {
            let again = state.az.recovery.sel == Some(Sel::App(a.clone()));
            select(state, Some(Sel::App(a.clone())));
            // A second click on the selected row does its default.
            if again && let Some(row) = state.az.recovery.rows.iter().find(|r| r.app_id == a) {
                let act = default_act(row);
                do_act(state, act);
            }
        }
        Some(Hit::Act(act)) => {
            select(state, Some(Sel::Act(act)));
            do_act(state, act);
        }
        Some(Hit::Panel) => {}
        None => close(state),
    }
    true
}

/// The pointer moved: true when the overlay is open (it has the pointer).
pub fn pointer_moved(state: &mut State, old: Point<f64, Logical>, pos: Point<f64, Logical>, time: u32) -> bool {
    if !state.az.recovery.open {
        return false;
    }
    if let Some(pointer) = state.az.seat.get_pointer() {
        pointer.motion(state, None, &MotionEvent { location: pos, serial: SERIAL_COUNTER.next_serial(), time });
        pointer.frame(state);
    }
    state.az.cursor_override = Some(CursorIcon::Default);
    let hover = view::hit(state, pos);
    if hover != state.az.recovery.hover {
        state.az.recovery.hover = hover;
        redraw(state);
    }
    for p in [old, pos] {
        if let Some(o) = state.az.output_at(p) {
            state.az.queue_redraw(&o);
        }
    }
    true
}

/// Scrolling: the list, while open.
pub fn scroll(state: &mut State, dy: f64) -> bool {
    let r = &mut state.az.recovery;
    if !r.open {
        return false;
    }
    let max = r.rows.len().saturating_sub(view::VISIBLE_ROWS);
    let before = r.scroll;
    if dy > 0.0 {
        r.scroll = (r.scroll + 1).min(max);
    } else if dy < 0.0 {
        r.scroll = r.scroll.saturating_sub(1);
    }
    if r.scroll != before {
        redraw(state);
    }
    true
}

// ---- actions --------------------------------------------------------------

fn do_act(state: &mut State, act: Act) {
    let app = state.az.recovery.selected_app().map(|r| r.app_id.clone());
    let result = run(state, act, app);
    let message = match result {
        Ok(m) | Err(m) => m,
    };
    if state.az.recovery.open {
        state.az.recovery.message = Some(message);
        redraw(state);
    }
}

/// Does an action, open or not (IPC), and says what happened.
pub fn run(state: &mut State, act: Act, app: Option<String>) -> Result<String, String> {
    let locked = state.az.lock.is_locked();
    if locked && !Act::LOCKED.contains(&act) {
        return Err(format!("{} isn't offered while the screen is locked", act.label(false)));
    }
    tracing::info!(action = act.name(), app = ?app, "recovery action");
    match act {
        Act::ForceQuit => {
            let app = app.ok_or("no app is selected")?;
            force_quit(state, &app)
        }
        Act::Close => {
            let app = app.ok_or("no app is selected")?;
            let ws: Vec<WinId> = state.az.windows_of_app(&app);
            if ws.is_empty() {
                return Err(format!("{app} has no windows"));
            }
            for w in &ws {
                state.close_window(*w);
            }
            let name = state.az.recovery.names.get(&app).cloned().unwrap_or(app);
            Ok(format!("Asked {name} to close"))
        }
        Act::ResetInput => {
            let was_open = state.az.recovery.open;
            if was_open {
                close(state);
            }
            let released = reset::reset_input(state);
            Ok(released.describe())
        }
        Act::RestartShell => {
            let r = crate::session::restart_supervised(state, "aizome-shell")?;
            if locked {
                relock_when_shell_returns(state);
            }
            Ok(r)
        }
        Act::ReloadCompositor => crate::reload::reload_compositor(state),
        Act::Lock => {
            if state.az.ipc.subscribers() == 0 {
                return Err("The shell isn't running to lock the screen; restart it first".into());
            }
            close(state);
            state.az.ipc.emit(&aizome_ipc::Event::Binding { action: "lock".into(), pointer: None, output: None });
            Ok("Locking".into())
        }
        Act::LogOut => {
            let now = Instant::now();
            let confirmed =
                state.az.recovery.confirm.is_some_and(|(a, t)| a == act && now.duration_since(t) < CONFIRM_WITHIN)
                    || !state.az.recovery.open;
            if !confirmed {
                state.az.recovery.confirm = Some((act, now));
                return Ok("Log out? Every app closes. Press Log Out again to confirm".into());
            }
            state.az.recovery.confirm = None;
            state.quit();
            Ok("Logging out".into())
        }
        Act::TextConsole => {
            if !matches!(state.backend, crate::backend::Backend::Udev(_)) {
                return Err("Text consoles are only there on a real display".into());
            }
            // Once there, nothing of this overlay shows: the way back is
            // said first, and the second press goes.
            let vt = free_console();
            let back = current_console().unwrap_or(1);
            let now = Instant::now();
            let confirmed =
                state.az.recovery.confirm.is_some_and(|(a, t)| a == act && now.duration_since(t) < CONFIRM_WITHIN)
                    || !state.az.recovery.open;
            if !confirmed {
                state.az.recovery.confirm = Some((act, now));
                return Ok(format!(
                    "Switch to text console {vt}? Press Text Console again\nCtrl+Alt+F{back} comes back to Aizome"
                ));
            }
            state.az.recovery.confirm = None;
            close(state);
            state.backend.change_vt(vt);
            Ok(format!("Switched to console {vt}; Ctrl+Alt+F{back} comes back"))
        }
    }
}

/// SIGKILL to the app's scope or processes (on the process thread), then,
/// if its windows are still there a moment later, its connections are
/// cut so they go anyway.
fn force_quit(state: &mut State, app: &str) -> Result<String, String> {
    let pids: Vec<i32> = {
        let mut p: Vec<i32> = state.az.windows_of_app(app).into_iter().filter_map(|w| state.az.window_pid(w)).collect();
        p.sort();
        p.dedup();
        p
    };
    if pids.is_empty() {
        return Err(format!("{app} has no process to stop"));
    }
    let x11 = x11_info(state);
    let Some(procinfo) = &state.az.recovery.procinfo else { return Err("the process thread isn't running".into()) };
    procinfo.send(procinfo::Request::Kill(AppProcs { key: app.to_string(), pids: pids.clone(), x11: x11.clone() }));
    // Satellite's connection carries every X11 app: never cut it (it closes
    // a dead X client's windows itself).
    let pids: Vec<i32> = pids.into_iter().filter(|p| x11.as_ref().is_none_or(|x| x.satellite != *p)).collect();
    let app = app.to_string();
    let name = state.az.recovery.names.get(&app).cloned().unwrap_or(app.clone());
    let _ = state.az.lh.insert_source(Timer::from_duration(LINGER), move |_, _, state: &mut State| {
        disconnect_lingering(state, &app, &pids);
        TimeoutAction::Drop
    });
    Ok(format!("Stopping {name}"))
}

/// Cuts the Wayland connections of an app's windows whose clients are
/// among `pids` (killed, but still connected: stuck in the kernel, or
/// dumping core).
fn disconnect_lingering(state: &mut State, app: &str, pids: &[i32]) {
    let mut clients = Vec::new();
    for m in state.az.windows.values() {
        if state.az.window_app(m.id).as_deref() != Some(app) {
            continue;
        }
        if let Ok(client) = state.az.dh.get_client(m.surface().id()) {
            let pid = client.get_credentials(&state.az.dh).ok().map(|c| c.pid);
            if pid.is_some_and(|p| pids.contains(&p)) && !clients.contains(&client.id()) {
                clients.push(client.id());
            }
        }
    }
    for id in clients {
        tracing::warn!(app, "its windows outlived the process: closing its connection");
        state
            .az
            .dh
            .backend_handle()
            .kill_client(id, smithay::reexports::wayland_server::backend::DisconnectReason::ConnectionClosed);
    }
}

/// While locked, a restarted shell must lock again: ask it once it is
/// listening.
fn relock_when_shell_returns(state: &mut State) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let subscribers = state.az.ipc.subscribers();
    let _ =
        state.az.lh.insert_source(Timer::from_duration(Duration::from_millis(300)), move |_, _, state: &mut State| {
            let has_lock_surface = state.az.out.values().any(|o| o.lock_surface.as_ref().is_some_and(|l| l.alive()));
            if !state.az.lock.is_locked() || has_lock_surface || Instant::now() > deadline {
                return TimeoutAction::Drop;
            }
            if state.az.ipc.subscribers() >= subscribers.max(1) {
                state.az.ipc.emit(&aizome_ipc::Event::Binding { action: "lock".into(), pointer: None, output: None });
            }
            TimeoutAction::ToDuration(Duration::from_secs(1))
        });
}

/// The virtual terminal showing now.
fn current_console() -> Option<i32> {
    let s = std::fs::read_to_string("/sys/class/tty/tty0/active").ok()?;
    s.trim().strip_prefix("tty")?.parse().ok()
}

/// A text console to switch to: the first of 2–6 that isn't this one.
fn free_console() -> i32 {
    let current = current_console().unwrap_or(1);
    (2..=6).find(|n| *n != current).unwrap_or(2)
}

// ---- IPC --------------------------------------------------------------------

/// The recovery requests; `None` for others.
pub fn handle_request(state: &mut State, req: &Request) -> Option<Response> {
    Some(match req {
        Request::Recovery { state: want } => {
            match want {
                Some(true) => open(state),
                Some(false) => close(state),
                None => toggle(state),
            }
            Response::Ok
        }
        Request::RecoveryState => {
            if !state.az.recovery.open {
                refresh_rows(state);
            }
            Response::Recovery { state: describe(state) }
        }
        Request::RecoveryAction { action, app_id } => {
            let Some(act) = Act::parse(action) else {
                return Some(Response::error(format!("unknown recovery action {action}")));
            };
            match run(state, act, app_id.clone()) {
                Ok(message) => Response::Message { message },
                Err(message) => Response::Error { message },
            }
        }
        Request::InputState => Response::InputState { state: input_state(state) },
        _ => return None,
    })
}

fn describe(state: &State) -> RecoveryState {
    let r = &state.az.recovery;
    let locked = state.az.lock.is_locked();
    let apps = if locked {
        Vec::new()
    } else {
        r.rows
            .iter()
            .map(|row| {
                let s = r.samples.get(&row.app_id);
                RecoveryApp {
                    app_id: row.app_id.clone(),
                    name: row.name.clone(),
                    windows: row.windows.len() as u32,
                    pids: row.pids.clone(),
                    units: s.map(|s| s.units.iter().map(|u| u.describe()).collect()).unwrap_or_default(),
                    cpu_percent: s.and_then(|s| s.cpu_percent),
                    memory_bytes: s.and_then(|s| s.memory),
                    not_responding: row.not_responding,
                    crashed: row.crashed,
                }
            })
            .collect()
    };
    RecoveryState {
        open: r.open,
        locked,
        apps,
        selected: if locked { None } else { r.selected_app().map(|a| a.app_id.clone()) },
        actions: r.actions(locked).iter().map(|a| a.name().to_string()).collect(),
        message: r.message.clone().or_else(|| r.notice.clone()),
        panel: r
            .open
            .then(|| r.output.as_ref().map(|o| view::layout(&state.az, o).panel.rect()))
            .flatten()
            .map(|p| aizome_ipc::Rect::new(p.x, p.y, p.w, p.h)),
    }
}

fn input_state(state: &State) -> aizome_ipc::InputState {
    use smithay::input::pointer::CursorImageStatus;
    use smithay::wayland::pointer_constraints::{PointerConstraint, with_pointer_constraint};
    let az = &state.az;
    let app_of = |s: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface| {
        az.window_for_root(s)
            .and_then(|id| az.window_app(id))
            .or_else(|| {
                // Which display's prompt has the keys, while locked.
                az.out
                    .iter()
                    .find(|(_, o)| o.lock_surface.as_ref().is_some_and(|l| l.wl_surface() == s))
                    .map(|(o, _)| format!("the lock screen on {}", o.name()))
            })
            .or_else(|| Some("a surface".into()))
    };
    let mut out = aizome_ipc::InputState::default();
    if let Some(pointer) = az.seat.get_pointer() {
        if pointer.is_grabbed() {
            let popup = az.popup_grab.as_ref().is_some_and(|g| g.owns_serial(|s| pointer.has_grab(s)));
            out.pointer_grab = Some(if popup {
                "popup".to_string()
            } else {
                match pointer.grab_start_data() {
                    Some(d) => match d.focus.as_ref().and_then(|(f, _)| app_of(&f.0)) {
                        Some(app) => format!("button {} on {app}", d.button),
                        None => format!("button {} on the compositor", d.button),
                    },
                    None => "a grab".to_string(),
                }
            });
        }
        if let Some(f) = pointer.current_focus() {
            out.pointer_focus = app_of(&f.0);
            out.pointer_constraint = with_pointer_constraint(&f.0, &pointer, |c| {
                c.filter(|c| c.is_active()).map(|c| match &*c {
                    PointerConstraint::Locked(_) => "locked".to_string(),
                    PointerConstraint::Confined(_) => "confined".to_string(),
                })
            });
        }
    }
    let mut buttons: Vec<u32> = az.forwarded_buttons.iter().copied().collect();
    buttons.sort();
    out.pressed_buttons = buttons;
    if let Some(k) = az.seat.get_keyboard() {
        out.keyboard_grab = k.is_grabbed();
        let mut keys: Vec<u32> = k.pressed_keys().into_iter().map(|k| k.raw().saturating_sub(8)).collect();
        keys.sort();
        out.pressed_keys = keys;
        out.keyboard_focus = k.current_focus().and_then(|f| app_of(&f.0));
    }
    out.cursor_hidden = az.cursor_override.is_none() && matches!(az.cursor_status, CursorImageStatus::Hidden);
    out.cursor_name = match (&az.cursor_override, &az.cursor_status) {
        (Some(icon), _) | (None, CursorImageStatus::Named(icon)) => Some(icon.name().into()),
        _ => None,
    };
    out
}
