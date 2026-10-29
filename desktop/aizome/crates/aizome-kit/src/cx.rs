//! [`Cx`]: what a program acts with in `launched` and `update`.

use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::dialogs::{Alert, FileDialog};
use crate::element::{AnyMsg, MsgFn, MsgMap};
use crate::engine::{Core, TimerUse};
use crate::layout::Rect;
use crate::layout::{Point, Size};
use crate::menu::{Menu, MenuRole};
use crate::platform::LayerChange;
use crate::platform::{Platform, Waker, WinId};
use crate::program::{
    ActivationRequest, Anchors, AppInfo, KeyboardInteractivity, Layer, Margins, Output, WindowKey, WindowSpec,
};
use crate::theme::Theme;
use crate::widgets::menus::MenuStyle;

/// What only the compositor can do for the Window and app menus (hide,
/// fill, stack, bring to front): `aizome-ipc` implements it. Without
/// one, those items log and do nothing; Minimise asks the window's
/// compositor through Wayland.
pub trait WindowManager {
    fn perform(&mut self, role: MenuRole, window: WinId);
    fn minimise(&mut self, window: WinId);
}

/// A timer from [`Cx::after`] or [`Cx::every`], to cancel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerHandle(u64);

/// Sends messages to the program from other threads: [`Cx::stream`].
pub struct MsgSender<Msg> {
    tx: Sender<AnyMsg>,
    waker: Waker,
    _m: PhantomData<fn(Msg)>,
}

impl<Msg> Clone for MsgSender<Msg> {
    fn clone(&self) -> Self {
        MsgSender { tx: self.tx.clone(), waker: self.waker.clone(), _m: PhantomData }
    }
}

impl<Msg: Send + 'static> MsgSender<Msg> {
    /// Send `msg`: whether the app is still there to get it.
    pub fn send(&self, msg: Msg) -> bool {
        let ok = self.tx.send(Box::new(msg)).is_ok();
        (self.waker)();
        ok
    }
}

type Job = Box<dyn FnOnce() + Send>;

/// The threads `spawn` runs work on, started on first use.
#[derive(Default)]
pub(crate) struct Pool {
    tx: Option<Sender<Job>>,
}

impl Pool {
    fn run(&mut self, job: Job) {
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            let n = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 8);
            for i in 0..n {
                let rx = rx.clone();
                let _ = std::thread::Builder::new().name(format!("aizome-kit-{i}")).spawn(move || {
                    loop {
                        let job = { rx.lock().unwrap_or_else(|e| e.into_inner()).recv() };
                        match job {
                            Ok(job) => job(),
                            Err(_) => return,
                        }
                    }
                });
            }
            tx
        });
        let _ = tx.send(job);
    }
}

/// What the program acts with: windows, background work, timers, the
/// clipboard, the focus, menus, sheets and panels.
pub struct Cx<'a, Msg> {
    pub(crate) core: &'a mut Core,
    pub(crate) platform: &'a mut dyn Platform,
    pub(crate) pool: &'a mut Pool,
    pub(crate) _m: PhantomData<fn(Msg)>,
}

impl<Msg: Clone + Send + 'static> Cx<'_, Msg> {
    // Windows.

    /// Open a window; its view is asked for once it's shown.
    pub fn open_window(&mut self, spec: WindowSpec) -> WindowKey {
        self.core.open_app_window(spec, self.platform)
    }

    /// Close a window or a sheet (without asking the program).
    pub fn close_window(&mut self, window: WindowKey) {
        self.core.close_key(window, self.platform);
    }

    /// Show a window opened hidden ([`WindowSpec::hidden`]) or hidden
    /// since, as it was, its view asked for again at once.
    pub fn show_window(&mut self, window: WindowKey) {
        self.core.show_key(window, true, self.platform);
    }

    /// Take a window off the screen, keeping it (and its key, state and
    /// widgets' state) for [`show_window`](Cx::show_window); its popups
    /// close, its widgets' ticks wait, and the focus leaves it (to come
    /// back when it shows, unless [`focus`](Cx::focus) says otherwise
    /// meanwhile). Its view isn't asked for while it's hidden, unless it's
    /// drawn ahead.
    pub fn hide_window(&mut self, window: WindowKey) {
        self.core.show_key(window, false, self.platform);
    }

    /// Draw hidden `window` ahead of its next showing, at `scale` (the
    /// scale of the output it'll show on): its view is asked for and laid
    /// out now, and drawn offscreen at idle, a band of a millisecond or so
    /// at a time, into pictures the window keeps (width × height × 4 bytes
    /// at `scale`); again on each call, when the focus moves in it, and
    /// when the theme changes. Shown at that size and scale, its first
    /// frame copies them in and draws only what changed since: a panel
    /// whose first opening would rasterise all of it is drawn at idle
    /// instead. Whether it's drawn ahead (a hidden window of a size).
    pub fn prepare_window(&mut self, window: WindowKey, scale: f64) -> bool {
        self.core.prepare_key(window, scale, self.platform)
    }

    pub fn set_title(&mut self, window: WindowKey, title: impl Into<String>) {
        let title = title.into();
        if let Some(win) = self.core.window_of(window)
            && let Some(w) = self.core.wins.get_mut(&win)
            && w.title != title
        {
            w.title.clone_from(&title);
            self.platform.set_title(win, &title);
        }
    }

    /// Ask for a new size (of the content, as [`WindowSpec::size`] is: the
    /// in-window menu bar, where there is one, comes on top).
    pub fn request_size(&mut self, window: WindowKey, size: Size) {
        if let Some(win) = self.core.window_of(window) {
            let strip = self.core.menu_strip_of(win);
            self.platform.request_size(win, Size::new(size.width, size.height + strip));
        }
    }

    /// Ask for `window` full screen, or not: the program hears what the
    /// compositor did in [`Program::window_state`](crate::Program::window_state).
    pub fn set_fullscreen(&mut self, window: WindowKey, on: bool) {
        if let Some(win) = self.core.window_of(window) {
            self.platform.set_fullscreen(win, on);
        }
    }

    pub fn minimise(&mut self, window: WindowKey) {
        if let Some(win) = self.core.window_of(window) {
            self.platform.minimize(win);
        }
    }

    /// Bring `window` forward and give it the keyboard, as far as the
    /// compositor allows (a second launch of a one-instance app asks for
    /// its window this way).
    pub fn activate(&mut self, window: WindowKey) {
        if let Some(win) = self.core.window_of(window) {
            self.platform.activate(win);
        }
    }

    /// The open windows (not sheets), oldest first.
    pub fn windows(&self) -> Vec<WindowKey> {
        self.core.app_windows().into_iter().map(|(_, k)| k).collect()
    }

    /// The window with the keyboard, or the last one that had it: the one
    /// menu commands are for in an app with a window per document.
    pub fn key_window(&self) -> Option<WindowKey> {
        let win = self.core.app_window_for(self.core.key_window?);
        self.core.wins.get(&win)?.key()
    }

    // Work and time.

    /// Run `work` on a background thread; its result comes back as a
    /// message.
    pub fn spawn(&mut self, work: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.core.bg_tx.clone();
        let wake = self.platform.waker();
        self.pool.run(Box::new(move || {
            let msg = work();
            let _ = tx.send(Box::new(msg));
            wake();
        }));
    }

    /// A sender for a thread that keeps sending messages (a file watcher,
    /// a D-Bus listener, a timer of its own).
    pub fn stream(&mut self) -> MsgSender<Msg> {
        MsgSender { tx: self.core.bg_tx.clone(), waker: self.platform.waker(), _m: PhantomData }
    }

    /// Send `msg` once, after `delay`.
    pub fn after(&mut self, delay: Duration, msg: Msg) -> TimerHandle {
        let token = self.platform.set_timer(delay, false);
        self.core.timers.insert(token, TimerUse::App { msg: MsgFn::new(msg), repeat: false });
        TimerHandle(token)
    }

    /// Send `msg` every `period`, until cancelled.
    pub fn every(&mut self, period: Duration, msg: Msg) -> TimerHandle {
        let token = self.platform.set_timer(period, true);
        self.core.timers.insert(token, TimerUse::App { msg: MsgFn::new(msg), repeat: true });
        TimerHandle(token)
    }

    pub fn cancel(&mut self, timer: TimerHandle) {
        if self.core.timers.remove(&timer.0).is_some() {
            self.platform.cancel_timer(timer.0);
        }
    }

    /// Send `msg` to `update` once this one returns.
    pub fn send(&mut self, msg: Msg) {
        self.core.queue.push_back(Box::new(msg));
    }

    // The desktop.

    pub fn clipboard_text(&mut self) -> Option<String> {
        self.platform.clipboard_text()
    }

    pub fn set_clipboard_text(&mut self, text: &str) {
        self.platform.set_clipboard_text(text);
    }

    /// Snapshot the current selection without waiting. Send the returned handle
    /// to a worker, call `read(timeout, maximum)` there, and check `is_current()`
    /// before applying the result. Its clones share cancellation.
    pub fn clipboard_read(&mut self) -> sidestep_ui::ClipboardRead {
        self.platform.clipboard_read()
    }

    /// Offer text already prepared on a worker, without copying its contents on
    /// the interface thread. The native backend sends to readers asynchronously.
    pub fn set_prepared_clipboard_text(&mut self, text: sidestep_ui::PreparedClipboardText) {
        self.platform.set_prepared_clipboard_text(text);
    }

    /// Publish immutable MIME representations prepared by a worker in one step.
    pub fn set_prepared_clipboard(&mut self, data: sidestep_ui::PreparedClipboard) {
        self.platform.set_prepared_clipboard(data);
    }

    /// Snapshot the primary selection (what was last selected anywhere, which
    /// a middle click pastes) for a worker, as `clipboard_read` does.
    pub fn primary_read(&mut self) -> sidestep_ui::ClipboardRead {
        self.platform.primary_read()
    }

    /// Make worker-prepared contents the primary selection. Where the
    /// compositor has none, nothing happens.
    pub fn set_prepared_primary(&mut self, data: sidestep_ui::PreparedClipboard) {
        self.platform.set_prepared_primary(data);
    }

    /// Open a link or a file in its app (xdg-open).
    pub fn open_url(&mut self, url: &str) {
        if let Err(error) = self.platform.open_url(
            url,
            Box::new(|result| {
                if let Err(error) = result {
                    log::warn!("Desktop opener: {error}");
                }
            }),
        ) {
            log::warn!("Desktop opener: {error}");
        }
    }

    /// Queue an opener and report its exit as an app message. Immediate rejection
    /// (including a full queue) is returned; it never invokes `finished`.
    pub fn open_url_with_result(
        &mut self,
        url: &str,
        finished: impl FnOnce(Result<(), crate::urls::OpenUrlError>) -> Msg + Send + 'static,
    ) -> Result<(), crate::urls::OpenUrlError> {
        let tx = self.stream();
        self.platform.open_url(
            url,
            Box::new(move |result| {
                tx.send(finished(result));
            }),
        )
    }

    /// Post a desktop notification (through `notify-send`, which speaks
    /// org.freedesktop.Notifications; the shell is that server).
    pub fn notify(&mut self, summary: &str, body: &str) {
        let app = self.core.info.name.clone();
        let spawned =
            std::process::Command::new("notify-send").arg("--app-name").arg(app).arg(summary).arg(body).spawn();
        if let Err(e) = spawned {
            log::info!("notification ({e}): {summary}: {body}");
        }
    }

    /// Where the element given `.id(name)` is in `window`, in the window's
    /// points, as last laid out: none before it shows, or without one (to
    /// put something by an element the program didn't get a press from,
    /// such as the shell's panel under a systray widget asked for over its
    /// socket).
    pub fn element_rect(&self, window: WindowKey, name: &str) -> Option<Rect> {
        let win = self.core.window_of(window)?;
        let w = self.core.wins.get(&win)?;
        let id = w.tree.names.get(name).copied()?;
        let i = w.tree.get(id)?;
        Some(w.tree.nodes[i].rect)
    }

    /// Focus the element given `.id(name)`, in the window with the
    /// keyboard (now, or once it shows); right after `open_window`, in the
    /// window just opened, once it shows.
    pub fn focus(&mut self, name: &str) {
        let opening =
            self.core.wins.values().filter(|w| w.key().is_some() && w.tree.root.is_none()).map(|w| w.id).max();
        if let Some(w) = opening {
            self.core.pending_focus.push((Some(w), name.into()));
            return;
        }
        let wins: Vec<WinId> = match self.core.key_window {
            Some(w) => vec![w],
            None => self.core.app_windows().into_iter().map(|(w, _)| w).collect(),
        };
        for w in wins {
            if self.core.focus_name(w, name, self.platform) {
                return;
            }
        }
        self.core.pending_focus.push((None, name.into()));
    }

    pub fn theme(&self) -> &Theme {
        &self.core.theme
    }

    /// The appearance the app shows in.
    pub fn appearance(&self) -> crate::theme::Appearance {
        self.core.appearance
    }

    /// Draw every window with `appearance` now: what Settings does as it
    /// writes a change, rather than waiting for the settings file's watcher
    /// (which looks once a second).
    pub fn set_appearance(&mut self, appearance: crate::theme::Appearance) {
        self.core.retheme(appearance, self.platform);
    }

    pub fn info(&self) -> &AppInfo {
        &self.core.info
    }

    /// Stop the app (without asking: see [`Program::close_requested`](crate::Program::close_requested)).
    pub fn quit(&mut self) {
        self.core.quit = true;
        self.platform.quit();
    }

    /// Give the Window menu a way to the compositor.
    pub fn set_window_manager(&mut self, wm: Box<dyn WindowManager>) {
        self.core.wm = Some(wm);
    }

    // Menus, sheets and panels.

    /// Open `menu` at `at` in `window` (its coordinates).
    pub fn show_context_menu(&mut self, window: WindowKey, at: Point, menu: Menu<Msg>) {
        if let Some(win) = self.core.window_of(window) {
            self.core.open_context_menu(win, at, menu.spec, self.platform);
        }
    }

    /// A sheet over `parent`: a modal dialog attached under its top. Its
    /// view is the program's `view` of the key returned; close it with
    /// [`close_window`](Cx::close_window).
    pub fn show_sheet(&mut self, parent: WindowKey) -> WindowKey {
        self.core.open_sheet(parent).unwrap_or(WindowKey::NONE)
    }

    /// An alert as a sheet over `window`: its buttons send their messages
    /// and close it.
    pub fn alert(&mut self, window: WindowKey, alert: Alert<Msg>) {
        if let Some(win) = self.core.window_of(window) {
            crate::dialogs::show_alert(self.core, win, alert.into_spec());
        }
    }

    /// Ask for files to open: `on_done` gets them, or `None` if cancelled.
    pub fn open_panel(
        &mut self,
        window: WindowKey,
        dialog: FileDialog,
        on_done: impl Fn(Option<Vec<PathBuf>>) -> Msg + 'static,
    ) {
        let map = MsgMap::new(on_done);
        crate::dialogs::show_panel(self.core, window, dialog, false, map, self.platform);
    }

    /// Ask where to save: `on_done` gets the path, or `None` if cancelled.
    pub fn save_panel(
        &mut self,
        window: WindowKey,
        dialog: FileDialog,
        on_done: impl Fn(Option<PathBuf>) -> Msg + 'static,
    ) {
        let map: MsgMap<Option<Vec<PathBuf>>> =
            MsgMap::new(move |paths: Option<Vec<PathBuf>>| on_done(paths.and_then(|p| p.into_iter().next())));
        crate::dialogs::show_panel(self.core, window, dialog, true, map, self.platform);
    }

    /// How long frames took, recently.
    pub fn stats(&self) -> &crate::stats::Stats {
        &self.core.stats
    }

    /// Open `menu` over `window` at `anchor` (a rectangle of it: a menu
    /// bar's title, a button, a 1 × 1 rectangle at the pointer), as
    /// `style` says. It runs as every kit menu does: keyboard, filtering,
    /// submenus. Any window will do, desktop surfaces included.
    pub fn show_menu(&mut self, window: WindowKey, anchor: Rect, menu: Menu<Msg>, style: MenuStyle<Msg>) {
        if let Some(win) = self.core.window_of(window) {
            let placement = style.placement.placement();
            self.core.show_menu(win, anchor, menu.spec, style.look, placement, None, self.platform);
        }
    }

    /// Close the menus open over `window`.
    pub fn close_menus(&mut self, window: WindowKey) {
        if let Some(win) = self.core.window_of(window) {
            self.core.kit(crate::engine::KitMsg::CloseMenus { window: win }, self.platform);
        }
    }

    // The desktop's surfaces.

    /// The outputs (screens), as the compositor last described them.
    pub fn outputs(&self) -> Vec<Output> {
        self.platform.outputs()
    }

    fn layer(&mut self, window: WindowKey, change: LayerChange) {
        if let Some(win) = self.core.window_of(window) {
            self.platform.set_layer(win, change);
        }
    }

    /// A desktop surface's size (0 along an axis it's anchored at both
    /// ends of: the compositor's choice).
    pub fn set_layer_size(&mut self, window: WindowKey, size: Size) {
        self.layer(window, LayerChange::Size(size));
    }

    pub fn set_layer(&mut self, window: WindowKey, layer: Layer) {
        self.layer(window, LayerChange::Layer(layer));
    }

    pub fn set_layer_anchors(&mut self, window: WindowKey, anchors: Anchors) {
        self.layer(window, LayerChange::Anchors(anchors));
    }

    pub fn set_layer_exclusive_zone(&mut self, window: WindowKey, points: i32) {
        self.layer(window, LayerChange::ExclusiveZone(points));
    }

    /// Move a desktop surface: margins from the edges it's anchored to.
    pub fn set_layer_margins(&mut self, window: WindowKey, margins: Margins) {
        self.layer(window, LayerChange::Margins(margins));
    }

    pub fn set_layer_keyboard(&mut self, window: WindowKey, keyboard: KeyboardInteractivity) {
        self.layer(window, LayerChange::Keyboard(keyboard));
    }

    /// Take the pointer only inside `region` (none: everywhere), letting
    /// it through elsewhere (a shadow round a panel).
    pub fn set_input_region(&mut self, window: WindowKey, region: Option<Vec<Rect>>) {
        if let Some(win) = self.core.window_of(window) {
            self.platform.set_input_region(win, region);
        }
    }

    /// Tell the compositor where the window is opaque.
    pub fn set_opaque_region(&mut self, window: WindowKey, region: Option<Vec<Rect>>) {
        if let Some(win) = self.core.window_of(window) {
            self.platform.set_opaque_region(win, region);
        }
    }

    /// Lock the session: open a [`WindowSpec::lock`] window on each output
    /// at once; the program hears `session_locked` when the compositor
    /// has, or `session_lock_finished` if it won't.
    pub fn lock_session(&mut self) {
        self.platform.lock_session();
    }

    /// Unlock the session: the lock's windows close.
    pub fn unlock_session(&mut self) {
        self.platform.unlock_session();
        self.core.close_lock_windows(self.platform);
    }

    pub fn is_session_locked(&self) -> bool {
        self.platform.is_session_locked()
    }

    /// Ask for an xdg-activation token, for a program about to be launched
    /// (`app_id` its id): the program hears it in `activation_token`.
    pub fn request_activation_token(&mut self, app_id: Option<&str>) -> ActivationRequest {
        ActivationRequest(self.platform.request_activation_token(app_id))
    }

    /// Have `bytes` of the shared memory windows draw into free and
    /// faulted in now, at a moment the program chooses (at start, or
    /// idle), so that a surface it shows later (a panel's first opening, a
    /// desktop's first frame) draws into memory that's there rather than
    /// faulting in a new buffer's pages in that frame: about 5 ms for a
    /// 2880 × 1800 surface. Count a surface's width × height × 4 bytes in
    /// device pixels (twice where the compositor holds on to buffers).
    pub fn reserve_buffers(&mut self, bytes: usize) {
        self.platform.reserve_buffers(bytes);
    }

    /// Start dragging `source` out of `window`, to another program, the
    /// desktop, the Shelf or another window:
    /// `DragSource::new().urls(files).actions(true, false).icon(chip, hotspot)`.
    /// Call it while the button that began the drag is down (from a
    /// message its press or moves sent): the compositor carries on from
    /// that press. The program hears a successful start in
    /// [`Program::drag_started`], how it ended in
    /// [`Program::drag_ended`], and is asked for promised data in
    /// [`Program::provide_drag_data`]. `None` if the window isn't showing.
    pub fn start_drag(&mut self, window: WindowKey, source: sidestep_ui::DragSource) -> Option<crate::program::DragId> {
        let win = self.core.window_of(window)?;
        self.core.start_drag(win, source, self.platform)
    }
}
