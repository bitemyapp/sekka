//! Running a program: the runtime that turns messages into updates and
//! views into windows, and the glue that runs it on Sidestep.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sidestep_ui::{
    Background, Canvas, Cursor, Drag, DropResponse, PopupPosition, ScrollDelta, TimerId, WindowEvent, WindowId,
    WindowOptions,
};

use crate::cx::{Cx, Pool};
use crate::element::RawElement;
use crate::engine::{Core, KitMsg, TimerUse};
use crate::hash::hash_of;
use crate::layout::{Rect, Size, Vec2};
use crate::platform::{LayerChange, OpenKind, OpenSpec, Placement, Platform, Waker, WinId};
use crate::program::Program;
use crate::shell::{ShellLink, ShellPlace, ShellWatch};
use crate::theme::{Appearance, ThemeChoice};

/// Points a wheel's detent scrolls.
pub const LINE: f64 = 40.0;

/// Run `program` until its last window closes (or it quits). Exits the
/// process with an error if there's no Wayland display.
pub fn run<P: Program>(program: P) {
    if let Err(e) = try_run(program) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Run `program`, and give it back when it stops.
pub fn try_run<P: Program>(program: P) -> Result<P, sidestep_ui::Error> {
    crate::fonts::install();
    let info = program.info();
    let quit = program.quit_when_last_window_closes();
    let app = sidestep_ui::App::new().app_id(info.app_id.clone()).quit_when_last_window_closes(quit);
    let glue = Glue { rt: Runtime::new(program, Appearance::load()), maps: Maps::default() };
    app.run(glue).map(|g| g.rt.program)
}

/// The program, the engine, and what joins them.
pub(crate) struct Runtime<P: Program> {
    pub program: P,
    pub core: Core,
    pub pool: Pool,
    pub shell: Option<ShellLink>,
    /// Where the shell is to be found, if anywhere.
    pub shell_place: Option<ShellPlace>,
    /// Looking for the shell while there's none.
    pub shell_watch: Option<ShellWatch>,
    /// The settings file changed (set by its watcher's thread).
    pub settings_changed: Arc<AtomicBool>,
    /// Watches the settings file while the app runs.
    pub watcher: Option<aizome_config::Watcher>,
    menus_published: u64,
    /// What each window's state was last, to tell the program of changes.
    pub window_states: HashMap<WinId, crate::program::WindowState>,
}

impl<P: Program> Runtime<P> {
    pub fn new(program: P, appearance: Appearance) -> Runtime<P> {
        let info = program.info();
        let core = Core::new(info, appearance);
        Runtime {
            program,
            core,
            pool: Pool::default(),
            shell: None,
            shell_place: None,
            shell_watch: None,
            settings_changed: Arc::new(AtomicBool::new(false)),
            watcher: None,
            menus_published: 0,
            window_states: HashMap::new(),
        }
    }

    /// Talk to the shell at `place`: now if it's there, else as soon as
    /// it is; the menus are in the windows meanwhile.
    pub fn find_shell(&mut self, place: ShellPlace) {
        self.shell = ShellLink::connect_to(&place.path, &self.core.info, place.wake.clone());
        self.core.in_window_menus = self.shell.is_none();
        if self.shell.is_none() {
            self.shell_watch = Some(ShellWatch::start(&place, &self.core.info));
        }
        self.shell_place = Some(place);
    }

    /// The shell came or went: the menus follow it, to the bar or into
    /// the windows. Whether anything changed.
    pub fn check_shell(&mut self, platform: &mut dyn Platform) -> bool {
        let link = if self.shell.as_ref().is_some_and(|s| !s.alive()) {
            // A restarted shell listens before the old one goes: straight to
            // it, and the menus never leave the bar.
            let again =
                self.shell_place.as_ref().and_then(|p| ShellLink::reconnect(&p.path, &self.core.info, p.wake.clone()));
            if again.is_none() {
                log::info!("the shell went away; menus go in the windows");
                self.shell = None;
                self.shell_watch = self.shell_place.as_ref().map(|p| ShellWatch::start(p, &self.core.info));
                self.menus_go_in_windows(true, platform);
                return true;
            }
            log::info!("the shell restarted; menus go to the new one");
            again.unwrap()
        } else {
            let Some(link) = self.shell_watch.as_ref().and_then(ShellWatch::take) else { return false };
            log::info!("the shell is here; menus go to it");
            link
        };
        self.shell_watch = None;
        self.shell = Some(link);
        if self.core.in_window_menus {
            self.menus_go_in_windows(false, platform);
        }
        // The menus as they are now, at once.
        let published = crate::shell::published(&self.core);
        self.menus_published = hash_of(&serde_json::to_string(&published).unwrap_or_default());
        if let Some(s) = &mut self.shell {
            s.send_menus(published);
        }
        true
    }

    fn menus_go_in_windows(&mut self, on: bool, platform: &mut dyn Platform) {
        // Menus open from the windows' bars go with the bars.
        for (win, _) in self.core.app_windows() {
            self.core.kit(KitMsg::CloseMenus { window: win }, platform);
        }
        self.core.in_window_menus = on;
        self.core.mark_all_dirty();
    }

    pub fn launch(&mut self, platform: &mut dyn Platform, paths: Vec<PathBuf>) {
        self.refresh_menus(platform);
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.launched(&mut cx);
        if !paths.is_empty() {
            self.program.open(paths, &mut cx);
        }
        self.core.mark_all_dirty();
        self.flush(platform);
    }

    /// Handle every message waiting, then view and lay out every window
    /// that needs it.
    pub fn flush(&mut self, platform: &mut dyn Platform) {
        // A program sending itself messages forever still draws.
        for _ in 0..64 {
            while let Ok(m) = self.core.bg_rx.try_recv() {
                self.core.queue.push_back(m);
            }
            let mut updated = false;
            while let Some(msg) = self.core.queue.pop_front() {
                match msg.downcast::<KitMsg>() {
                    Ok(k) => self.kit(*k, platform),
                    Err(msg) => match msg.downcast::<P::Msg>() {
                        Ok(m) => {
                            let started = Instant::now();
                            let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
                            self.program.update(*m, &mut cx);
                            self.core.stats.update(started.elapsed());
                            updated = true;
                        }
                        Err(_) => log::warn!("a message of a type the program doesn't take was dropped"),
                    },
                }
            }
            if updated {
                self.core.mark_all_dirty();
                self.refresh_menus(platform);
            }
            let dirty = self.core.dirty_windows();
            let rebuilt = !dirty.is_empty();
            for win in dirty {
                self.rebuild(win, platform);
            }
            if rebuilt {
                // Dialog cancellation can be a kit-only message. Publish
                // the newly uncovered commands and restored field focus.
                self.refresh_menus(platform);
            }
            for win in self.core.relayout_windows() {
                self.core.relayout(win, platform);
            }
            if self.core.queue.is_empty() && self.core.dirty_windows().is_empty() {
                break;
            }
        }
    }

    /// View `win` again (with its sheets) and put it in place.
    pub fn rebuild(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.core.wins.get(&win) else { return };
        let Some(key) = w.key() else { return };
        let sheet_keys = w.sheets.clone();
        let main = self.program.view(key).into_raw();
        let sheets: Vec<RawElement> = sheet_keys.iter().map(|k| self.program.view(*k).into_raw()).collect();
        self.core.install(win, main, sheets, platform);
    }

    fn kit(&mut self, msg: KitMsg, platform: &mut dyn Platform) {
        match msg {
            KitMsg::Quit => {
                for (_, key) in self.core.app_windows() {
                    let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
                    if !self.program.close_requested(key, &mut cx) {
                        return;
                    }
                }
                self.core.quit = true;
                platform.quit();
            }
            KitMsg::CloseRequest(key) => {
                let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
                if self.program.close_requested(key, &mut cx) {
                    self.core.close_key(key, platform);
                }
            }
            KitMsg::DragStarted { window, drag } => {
                let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
                self.program.drag_started(window, drag, &mut cx);
                self.core.mark_all_dirty();
            }
            KitMsg::AskShell(request) => match &mut self.shell {
                Some(s) => s.send(&aizome_shell_proto::AppMessage::Request { request }),
                None => log::debug!("no shell to ask {request:?}"),
            },
            other => self.core.kit(other, platform),
        }
    }

    /// The user asked to close a window (its close box, a shortcut).
    pub fn close_requested(&mut self, win: WinId, platform: &mut dyn Platform) {
        // The dialog's Cancel/default controls decide what happens first.
        // Re-entering the document's close handler could discard data or
        // stack another unsaved-changes alert over this one.
        if self.core.has_modal(win) {
            return;
        }
        if let Some(key) = self.core.wins.get(&win).and_then(|w| w.key()) {
            self.core.queue.push_back(Box::new(KitMsg::CloseRequest(key)));
        }
        self.flush(platform);
    }

    /// Ask the program for its menus again; publish them if they changed.
    pub fn refresh_menus(&mut self, platform: &mut dyn Platform) {
        self.core.menubar = self.program.menus().into_specs();
        if self.shell.is_some() {
            let published = crate::shell::published(&self.core);
            let h = hash_of(&serde_json::to_string(&published).unwrap_or_default());
            if h != self.menus_published {
                self.menus_published = h;
                // A burst of changes goes out once.
                if !self.core.timers.values().any(|t| matches!(t, TimerUse::Menus)) {
                    let token = platform.set_timer(Duration::from_millis(40), false);
                    self.core.timers.insert(token, TimerUse::Menus);
                }
            }
        }
    }

    /// Send the menus to the shell now.
    pub fn publish_menus(&mut self) {
        if let Some(shell) = &mut self.shell {
            let menus = crate::shell::published(&self.core);
            shell.send_menus(menus);
        }
    }

    pub fn timer(&mut self, token: u64, platform: &mut dyn Platform) {
        if matches!(self.core.timers.get(&token), Some(TimerUse::Menus)) {
            self.core.timers.remove(&token);
            self.publish_menus();
            return;
        }
        self.core.timer(token, platform);
        self.flush(platform);
    }

    /// Settings, the shell, background work: whatever woke the loop.
    pub fn woken(&mut self, platform: &mut dyn Platform) {
        if self.settings_changed.swap(false, Ordering::AcqRel) {
            let appearance = Appearance::load();
            self.core.retheme(appearance, platform);
        }
        let incoming = self.shell.as_mut().map(|s| s.incoming()).unwrap_or_default();
        for m in incoming {
            crate::shell::handle(self, m, platform);
        }
        self.check_shell(platform);
        self.flush(platform);
    }

    /// A window's state changed (full screen, maximised): tell the
    /// program, once per change.
    pub fn window_state(&mut self, win: WinId, state: crate::program::WindowState, platform: &mut dyn Platform) {
        if self.window_states.insert(win, state) == Some(state) {
            return;
        }
        let Some(key) = self.core.wins.get(&win).and_then(|w| w.key()) else { return };
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.window_state(key, state, &mut cx);
        self.core.mark_all_dirty();
        self.refresh_menus(platform);
        self.flush(platform);
    }

    pub fn outputs_changed(&mut self, platform: &mut dyn Platform) {
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.outputs_changed(&mut cx);
        self.core.mark_all_dirty();
        self.flush(platform);
    }

    pub fn pointer_presence(&mut self, win: WinId, inside: bool, platform: &mut dyn Platform) {
        let Some(key) = self.core.wins.get(&win).and_then(|w| w.key()) else { return };
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.pointer_presence(key, inside, &mut cx);
        self.core.mark_all_dirty();
        self.flush(platform);
    }

    pub fn session_locked(&mut self, platform: &mut dyn Platform) {
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.session_locked(&mut cx);
        self.flush(platform);
    }

    pub fn session_lock_finished(&mut self, platform: &mut dyn Platform) {
        self.core.close_lock_windows(platform);
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.session_lock_finished(&mut cx);
        self.flush(platform);
    }

    pub fn activation_token(&mut self, request: u64, token: Option<String>, platform: &mut dyn Platform) {
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.activation_token(crate::program::ActivationRequest(request), token, &mut cx);
        self.flush(platform);
    }

    pub fn provide_drag_data(&mut self, drag: u64, mime: &str, platform: &mut dyn Platform) -> Option<Vec<u8>> {
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        let data = self.program.provide_drag_data(crate::program::DragId(drag), mime, &mut cx);
        self.flush(platform);
        data
    }

    pub fn drag_ended(&mut self, drag: u64, action: sidestep_ui::DropAction, platform: &mut dyn Platform) {
        let mut cx = Cx { core: &mut self.core, platform, pool: &mut self.pool, _m: PhantomData };
        self.program.drag_ended(crate::program::DragId(drag), action, &mut cx);
        self.core.mark_all_dirty();
        self.flush(platform);
    }

    /// Auto follows the clock: look again (each hour, and when the desktop
    /// changes its look).
    pub fn recheck_auto(&mut self, platform: &mut dyn Platform) {
        if self.core.appearance.theme == ThemeChoice::Auto {
            let a = self.core.appearance;
            self.core.retheme(a, platform);
            self.flush(platform);
        }
    }
}

/// The kit's windows and timers as Sidestep knows them.
#[derive(Default)]
struct Maps {
    to_side: HashMap<WinId, WindowId>,
    to_kit: HashMap<WindowId, WinId>,
    timers: HashMap<TimerId, (u64, bool)>,
    tokens: HashMap<u64, TimerId>,
    next_token: u64,
    waker: Option<Waker>,
    activations: HashMap<sidestep_ui::ActivationRequest, u64>,
    next_activation: u64,
    drags: HashMap<sidestep_ui::DragId, u64>,
    next_drag: u64,
    /// Windows drawn ahead: the picture for each one's next showing, and
    /// what changed since it showed (see [`crate::ahead`]).
    pictures: HashMap<WinId, Kept>,
}

/// A hidden window's picture drawn ahead, kept for its next showing.
struct Kept {
    picture: crate::ahead::Picture,
    /// It shows, and its first frame is still to come.
    shown: bool,
    /// What changed since it showed, to draw over the picture.
    changed: Vec<Rect>,
}

struct Glue<P: Program> {
    rt: Runtime<P>,
    maps: Maps,
}

/// The engine's platform, through Sidestep's `Cx`.
struct SidestepPlatform<'a> {
    sx: &'a mut sidestep_ui::Cx,
    maps: &'a mut Maps,
}

impl Platform for SidestepPlatform<'_> {
    fn open_window(&mut self, id: WinId, spec: &OpenSpec) {
        let background = spec.background.map_or(Background::Transparent, Background::Color);
        let options = match &spec.kind {
            OpenKind::Toplevel { parent, utility } => {
                let mut o = WindowOptions::new(spec.title.clone())
                    .size(spec.size.width, spec.size.height)
                    .resizable(spec.resizable)
                    .close_on_request(false)
                    .background(background);
                if let Some(m) = spec.min_size {
                    o = o.min_size(m.width, m.height);
                }
                if let Some(m) = spec.max_size {
                    o = o.max_size(m.width, m.height);
                }
                if *utility {
                    o = o.minimizable(false);
                }
                if let Some(p) = parent.and_then(|p| self.maps.to_side.get(&p)) {
                    o = o.transient_for(*p);
                }
                if let Some(handle) = &spec.foreign_parent {
                    o = o.foreign_parent(handle.clone());
                }
                o
            }
            OpenKind::Layer(o) => WindowOptions::layer(o.clone())
                .size(spec.size.width, spec.size.height)
                .close_on_request(false)
                .background(background),
            OpenKind::Lock { output } => WindowOptions::lock(*output).background(background),
            OpenKind::Popup { parent, anchor, placement, grab } => {
                let Some(&p) = self.maps.to_side.get(parent) else { return };
                let position = match placement {
                    Placement::Below => PopupPosition::BELOW,
                    Placement::AtPoint => PopupPosition::AT_POINT,
                    Placement::Beside => PopupPosition::BESIDE.offset(2, -5),
                    Placement::Above => PopupPosition {
                        anchor: sidestep_ui::Corner::TopLeft,
                        gravity: sidestep_ui::Corner::TopRight,
                        ..PopupPosition::BELOW
                    },
                };
                WindowOptions::popup(p, *anchor, spec.size, *grab).popup_position(position).background(background)
            }
        };
        let sid = self.sx.open_window(options.visible(spec.visible));
        self.maps.to_side.insert(id, sid);
        self.maps.to_kit.insert(sid, id);
    }

    fn close_window(&mut self, id: WinId) {
        self.maps.pictures.remove(&id);
        if let Some(sid) = self.maps.to_side.remove(&id) {
            self.maps.to_kit.remove(&sid);
            self.sx.close_window(sid);
        }
    }

    fn set_title(&mut self, id: WinId, title: &str) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_title(title);
        }
    }

    fn invalidate(&mut self, id: WinId, rect: Rect) {
        if let Some(k) = self.maps.pictures.get_mut(&id).filter(|k| k.shown) {
            k.changed.push(rect);
        }
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.invalidate(rect);
        }
    }

    fn invalidate_all(&mut self, id: WinId) {
        // All of it changes: the picture drawn ahead is no use.
        self.maps.pictures.remove(&id);
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.redraw();
        }
    }

    fn set_cursor(&mut self, id: WinId, cursor: Cursor) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_cursor(cursor);
        }
    }

    fn set_text_input(&mut self, id: WinId, wanted: bool, caret: Option<Rect>) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_text_input(wanted, caret);
        }
    }

    fn request_size(&mut self, id: WinId, size: Size) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.request_size(size);
        }
    }

    fn minimize(&mut self, id: WinId) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.minimize();
        }
    }

    fn set_maximized(&mut self, id: WinId, on: bool) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_maximized(on);
        }
    }

    fn set_fullscreen(&mut self, id: WinId, on: bool) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_fullscreen(on);
        }
    }

    fn activate(&mut self, id: WinId) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.activate();
        }
    }

    fn begin_move(&mut self, id: WinId) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.begin_move();
        }
    }

    fn clipboard_text(&mut self) -> Option<String> {
        self.sx.clipboard_text()
    }

    fn set_clipboard_text(&mut self, text: &str) {
        self.sx.set_clipboard_text(text);
    }

    fn clipboard_read(&mut self) -> sidestep_ui::ClipboardRead {
        self.sx.clipboard_read()
    }

    fn set_prepared_clipboard_text(&mut self, text: sidestep_ui::PreparedClipboardText) {
        self.sx.set_prepared_clipboard_text(text);
    }

    fn set_timer(&mut self, after: Duration, repeat: bool) -> u64 {
        let tid = if repeat { self.sx.set_repeating_timer(after) } else { self.sx.set_timer(after) };
        self.maps.next_token += 1;
        let token = self.maps.next_token;
        self.maps.timers.insert(tid, (token, repeat));
        self.maps.tokens.insert(token, tid);
        token
    }

    fn cancel_timer(&mut self, token: u64) {
        if let Some(tid) = self.maps.tokens.remove(&token) {
            self.maps.timers.remove(&tid);
            self.sx.cancel_timer(tid);
        }
    }

    fn quit(&mut self) {
        self.sx.quit();
    }

    fn desktop_dark(&self) -> bool {
        self.sx.appearance().dark
    }

    fn waker(&self) -> Waker {
        self.maps.waker.clone().unwrap_or_else(|| Arc::new(|| {}))
    }

    fn set_layer(&mut self, id: WinId, change: LayerChange) {
        let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) else { return };
        match change {
            LayerChange::Size(s) => w.set_layer_size(s),
            LayerChange::Layer(l) => w.set_layer(l),
            LayerChange::Anchors(a) => w.set_layer_anchors(a),
            LayerChange::ExclusiveZone(z) => w.set_layer_exclusive_zone(z),
            LayerChange::Margins(m) => w.set_layer_margins(m),
            LayerChange::Keyboard(k) => w.set_layer_keyboard(k),
        }
    }

    fn show_window(&mut self, id: WinId, shown: bool) {
        let Some(&sid) = self.maps.to_side.get(&id) else { return };
        if shown {
            // Its first frame starts from the picture drawn ahead, if any.
            if let Some(k) = self.maps.pictures.get_mut(&id) {
                k.shown = true;
                k.changed.clear();
            }
            self.sx.show_window(sid);
        } else {
            self.maps.pictures.remove(&id);
            self.sx.hide_window(sid);
        }
    }

    fn keep_picture(&mut self, id: WinId, picture: Option<crate::ahead::Picture>) {
        match picture {
            Some(picture) => {
                self.maps.pictures.insert(id, Kept { picture, shown: false, changed: Vec::new() });
            }
            None => {
                self.maps.pictures.remove(&id);
            }
        }
    }

    fn set_input_region(&mut self, id: WinId, region: Option<Vec<Rect>>) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_input_region(region.as_deref());
        }
    }

    fn set_popup_anchor(&mut self, id: WinId, anchor: Rect) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_popup_anchor(anchor);
        }
    }

    fn set_opaque_region(&mut self, id: WinId, region: Option<Vec<Rect>>) {
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.set_opaque_region(region.as_deref());
        }
    }

    fn scroll(&mut self, id: WinId, rect: Rect, delta: Vec2) {
        if let Some(k) = self.maps.pictures.get_mut(&id).filter(|k| k.shown) {
            k.changed.push(rect);
        }
        if let Some(mut w) = self.maps.to_side.get(&id).and_then(|s| self.sx.window(*s)) {
            w.scroll(rect, delta);
        }
    }

    fn start_drag(&mut self, id: WinId, source: sidestep_ui::DragSource) -> Option<u64> {
        let side = *self.maps.to_side.get(&id)?;
        let drag = self.sx.start_drag(side, source)?;
        self.maps.next_drag += 1;
        self.maps.drags.insert(drag, self.maps.next_drag);
        Some(self.maps.next_drag)
    }

    fn outputs(&self) -> Vec<crate::program::Output> {
        self.sx.outputs()
    }

    fn lock_session(&mut self) {
        self.sx.lock_session();
    }

    fn unlock_session(&mut self) {
        self.sx.unlock_session();
    }

    fn is_session_locked(&self) -> bool {
        self.sx.is_session_locked()
    }

    fn request_activation_token(&mut self, app_id: Option<&str>) -> u64 {
        let request = self.sx.request_activation_token(app_id);
        self.maps.next_activation += 1;
        self.maps.activations.insert(request, self.maps.next_activation);
        self.maps.next_activation
    }

    fn reserve_buffers(&mut self, bytes: usize) {
        self.sx.reserve_buffers(bytes);
    }
}

impl<P: Program> Glue<P> {
    fn split<'a>(&'a mut self, sx: &'a mut sidestep_ui::Cx) -> (&'a mut Runtime<P>, SidestepPlatform<'a>) {
        (&mut self.rt, SidestepPlatform { sx, maps: &mut self.maps })
    }
}

/// Files named on the command line.
fn argv_paths() -> Vec<PathBuf> {
    std::env::args_os().skip(1).filter(|a| !a.to_string_lossy().starts_with('-')).map(PathBuf::from).collect()
}

impl<P: Program> sidestep_ui::Handler for Glue<P> {
    fn launched(&mut self, cx: &mut sidestep_ui::Cx) {
        let proxy = cx.proxy();
        self.maps.waker = Some(Arc::new(move || proxy.wake()));
        let wake = self.maps.waker.clone().expect("a waker");
        let (flag, w) = (self.rt.settings_changed.clone(), wake.clone());
        self.rt.watcher = Some(aizome_config::Watcher::spawn(move || {
            flag.store(true, Ordering::Release);
            w();
        }));
        if !self.rt.program.menus_to_shell() {
            // The shell itself: no one to publish to, and no menu bars.
            self.rt.core.in_window_menus = false;
        } else if let Some(path) = aizome_shell_proto::socket_path() {
            self.rt.find_shell(ShellPlace { path, wake, period: ShellWatch::PERIOD });
        } else {
            // Another desktop: the menus are the windows' own.
            self.rt.core.in_window_menus = true;
        }
        let (rt, mut p) = self.split(cx);
        rt.launch(&mut p, argv_paths());
    }

    fn window_event(&mut self, cx: &mut sidestep_ui::Cx, window: WindowId, event: WindowEvent) {
        let Some(&win) = self.maps.to_kit.get(&window) else { return };
        let (rt, mut p) = self.split(cx);
        let core = &mut rt.core;
        match event {
            WindowEvent::Resized(size) => core.resized(win, size),
            WindowEvent::Focused(on) => core.window_focus(win, on, &mut p),
            WindowEvent::Key(k) => core.key(win, k, &mut p),
            WindowEvent::ModifiersChanged(m) => core.mods = m,
            WindowEvent::PointerEntered(at) => {
                core.pointer_move(win, at, &mut p);
                rt.pointer_presence(win, true, &mut p);
            }
            WindowEvent::PointerMoved { position, modifiers } => {
                core.mods = modifiers;
                core.pointer_move(win, position, &mut p);
            }
            WindowEvent::PointerLeft => {
                core.pointer_leave(win, &mut p);
                rt.pointer_presence(win, false, &mut p);
            }
            WindowEvent::PointerButton { position, button, pressed, clicks, modifiers, .. } => {
                core.mods = modifiers;
                core.pointer_button(win, position, button, pressed, clicks, &mut p);
            }
            WindowEvent::Scroll { position, delta, phase, momentum, modifiers, .. } => {
                core.mods = modifiers;
                let d = match delta {
                    ScrollDelta::Points(v) => v,
                    ScrollDelta::Lines(v) => Vec2::new(v.x * LINE, v.y * LINE),
                };
                core.scroll(win, position, d, phase, momentum, &mut p);
            }
            WindowEvent::Pinch { position, phase, magnification, modifiers, .. } => {
                core.mods = modifiers;
                core.pinch(win, position, 1.0 + magnification, phase, &mut p);
            }
            WindowEvent::StateChanged(s) => {
                let state = crate::program::WindowState { fullscreen: s.fullscreen, maximized: s.maximized };
                rt.window_state(win, state, &mut p);
                return;
            }
            WindowEvent::Ime(ime) => core.ime(win, ime, &mut p),
            WindowEvent::CloseRequested => {
                rt.close_requested(win, &mut p);
                return;
            }
            WindowEvent::PopupDismissed => {
                // Sidestep closed it already.
                p.maps.to_side.remove(&win);
                p.maps.to_kit.remove(&window);
                core.popup_dismissed(win, &mut p);
            }
            _ => {}
        }
        rt.flush(&mut p);
    }

    fn draw(&mut self, _cx: &mut sidestep_ui::Cx, window: WindowId, canvas: &mut Canvas) {
        let Some(&win) = self.maps.to_kit.get(&window) else { return };
        // The first frame of a window drawn ahead, at the size and scale
        // drawn: the picture, and what changed since drawn over it.
        if let Some(k) = self.maps.pictures.remove(&win).filter(|k| k.shown)
            && k.picture.fits(canvas.size(), canvas.scale(), &canvas.damage())
            && let Some(changed) = crate::ahead::changed(&k.changed, canvas.size())
        {
            let core = &mut self.rt.core;
            crate::ahead::draw(&k.picture, &changed, canvas, |c| core.paint(win, c));
            return;
        }
        self.rt.core.paint(win, canvas);
    }

    fn timer(&mut self, cx: &mut sidestep_ui::Cx, timer: TimerId) {
        let Some(&(token, repeat)) = self.maps.timers.get(&timer) else { return };
        if !repeat {
            self.maps.timers.remove(&timer);
            self.maps.tokens.remove(&token);
        }
        let (rt, mut p) = self.split(cx);
        rt.timer(token, &mut p);
    }

    fn woken(&mut self, cx: &mut sidestep_ui::Cx) {
        let (rt, mut p) = self.split(cx);
        rt.woken(&mut p);
    }

    fn appearance_changed(&mut self, cx: &mut sidestep_ui::Cx, appearance: sidestep_ui::Appearance) {
        let (rt, mut p) = self.split(cx);
        let _ = appearance;
        rt.recheck_auto(&mut p);
    }

    fn outputs_changed(&mut self, cx: &mut sidestep_ui::Cx) {
        let (rt, mut p) = self.split(cx);
        rt.outputs_changed(&mut p);
    }

    fn session_locked(&mut self, cx: &mut sidestep_ui::Cx) {
        let (rt, mut p) = self.split(cx);
        rt.session_locked(&mut p);
    }

    fn session_lock_finished(&mut self, cx: &mut sidestep_ui::Cx) {
        let (rt, mut p) = self.split(cx);
        rt.session_lock_finished(&mut p);
    }

    fn activation_token(
        &mut self,
        cx: &mut sidestep_ui::Cx,
        request: sidestep_ui::ActivationRequest,
        token: Option<String>,
    ) {
        let Some(id) = self.maps.activations.remove(&request) else { return };
        let (rt, mut p) = self.split(cx);
        rt.activation_token(id, token, &mut p);
    }

    fn provide_drag_data(
        &mut self,
        cx: &mut sidestep_ui::Cx,
        drag: sidestep_ui::DragId,
        mime: &str,
    ) -> Option<Vec<u8>> {
        let id = *self.maps.drags.get(&drag)?;
        let (rt, mut p) = self.split(cx);
        rt.provide_drag_data(id, mime, &mut p)
    }

    fn drag_ended(&mut self, cx: &mut sidestep_ui::Cx, drag: sidestep_ui::DragId, action: sidestep_ui::DropAction) {
        let Some(id) = self.maps.drags.remove(&drag) else { return };
        let (rt, mut p) = self.split(cx);
        rt.drag_ended(id, action, &mut p);
    }

    fn drag_moved(&mut self, cx: &mut sidestep_ui::Cx, window: WindowId, drag: &Drag) -> DropResponse {
        let Some(&win) = self.maps.to_kit.get(&window) else { return DropResponse::REFUSE };
        let (rt, mut p) = self.split(cx);
        let has_text = drag.mimes.iter().any(|m| m.starts_with("text/plain") || m == "UTF8_STRING");
        if rt.core.drag_over(win, drag.position, &drag.urls, has_text, &mut p) {
            DropResponse::copy()
        } else {
            DropResponse::REFUSE
        }
    }

    fn drag_left(&mut self, cx: &mut sidestep_ui::Cx, window: WindowId) {
        let Some(&win) = self.maps.to_kit.get(&window) else { return };
        let (rt, mut p) = self.split(cx);
        rt.core.drag_left(win, &mut p);
    }

    fn dropped(&mut self, cx: &mut sidestep_ui::Cx, window: WindowId, drag: &Drag, response: &DropResponse) -> bool {
        let Some(&win) = self.maps.to_kit.get(&window) else { return false };
        let text = match response.mime.as_deref() {
            Some(m) if m.starts_with("text/plain") || m == "UTF8_STRING" => {
                cx.drag_data(m).map(|d| String::from_utf8_lossy(&d).into_owned())
            }
            _ => None,
        };
        let (rt, mut p) = self.split(cx);
        let took = rt.core.drop(win, drag.position, drag.urls.clone(), text, &mut p);
        rt.flush(&mut p);
        took
    }
}
