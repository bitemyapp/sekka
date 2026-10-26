//! Input policy: what the pointer and keys do, once the core has kept its
//! own keys (VT switching, ⌃❖R, ⌃⌥Delete).
//!
//! The pointer belongs to the compositor over tabs, frame edges and
//! overlays, and to clients over their surfaces; a press on a window
//! focuses and raises it. Keys go through slow and sticky keys, the
//! overlays, the binding table, then the command-key translation, then to
//! the focused client.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use aizome_comp_core::focus::Focus;
use aizome_comp_core::input::{BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, MODIFIER_KEYS, Owner, layer_under};
use aizome_comp_core::logic::{AxisEvent, KeyDecision, KeyEvent};
use aizome_comp_core::timers::TimerId;
use smithay::backend::input::{Axis, AxisSource};
use smithay::desktop::{WindowSurfaceType, layer_map_for_output};
use smithay::input::keyboard::{ModifiersState, keysyms};
use smithay::input::pointer::{AxisFrame, CursorIcon, MotionEvent};
use smithay::utils::{Logical, Point, SERIAL_COUNTER, Serial};
use smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitorSeat;
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, Layer};

use crate::bindings::{Action, Mods};
use crate::decor::{Edges, Hit};
use crate::prelude::*;

/// What the compositor tracks about pointer interaction.
#[derive(Default)]
pub struct Interactive {
    pub hover: Option<Hit>,
    /// The last press on a tab, for double-clicks.
    pub last_tab_click: Option<(Instant, WinId, (f64, f64))>,
    /// A tab being dragged: where it would stack.
    pub drag: Option<TabDrag>,
    /// Buttons whose presses an overlay took. Their releases are the
    /// overlay's too, and nobody's once the press has closed it (a window
    /// pick ends on the press): a client given one would see a release
    /// with no press.
    pub overlay_presses: HashSet<u32>,
}

#[derive(Clone, Debug)]
pub struct TabDrag {
    /// The window being dragged.
    pub win: WinId,
    /// The window whose tab it hovers, to stack with on release.
    pub target: Option<WinId>,
    /// Where the dropped tab would go (the slot), in global points.
    pub slot: Option<crate::wm::Rect>,
}

/// What lies under a point.
pub enum Under {
    /// A client surface, with its origin in global points.
    Surface(Focus, Point<f64, Logical>, Owner),
    /// A window's decorations.
    Decor(WinId, Hit),
    /// Nothing: the desktop colour.
    Nothing,
}

/// The logic's input paths, on the compositor's state.
pub trait InputExt {
    fn logic_pointer_motion(&mut self, pos: Point<f64, Logical>, time: u32);
    fn logic_pointer_button(&mut self, button: u32, pressed: bool, time: u32);
    fn logic_pointer_axis(&mut self, a: &AxisEvent);
    fn logic_touch_down(&mut self, pos: Point<f64, Logical>);
    fn logic_key(&mut self, code: u32, pressed: bool, time: u32);
    fn slow_key_counted(&mut self, code: u32);
    fn decide_key(&mut self, ev: &KeyEvent<'_>) -> KeyDecision;
    fn run_action(&mut self, action: Action);
    /// What lies under a point, top to bottom.
    fn under(&mut self, pos: Point<f64, Logical>) -> Under;
}

impl InputExt for State {
    fn logic_pointer_motion(&mut self, pos: Point<f64, Logical>, time: u32) {
        let Some(pointer) = self.az.seat.get_pointer() else { return };
        let under = self.under(pos);
        // The compositor's cursor over decorations; the client's over
        // surfaces.
        let (hover, override_cursor) = match &under {
            Under::Decor(_, hit @ Hit::Edge(_, e)) => (Some(*hit), Some(e.cursor())),
            Under::Decor(_, hit) => (Some(*hit), Some(CursorIcon::Default)),
            Under::Nothing => (None, Some(CursorIcon::Default)),
            Under::Surface(..) => (None, None),
        };
        if !pointer.is_grabbed() {
            self.lg.m().interactive.hover = hover;
            self.az.cursor_override = override_cursor;
        }
        crate::overlays::pointer_moved(self, pos);
        let focus = match under {
            Under::Surface(f, origin, _) => Some((f, origin)),
            _ => None,
        };
        // Focus follows the mouse, when asked.
        if self.az.settings.windows.focus_follows == aizome_comp_core::aizome_config::FocusFollows::Mouse
            && !pointer.is_grabbed()
            && let Some((f, _)) = &focus
            && let Some(id) = self.az.window_for_root(&f.0)
            && self.lg.r().wm.focused() != Some(id)
        {
            let r = self.lg.r().wm.get(id).map(|w| w.rect);
            self.focus_window(Some(id));
            if let Some(r) = r {
                self.az.queue_redraw_rect(r);
            }
        }
        pointer.motion(self, focus, &MotionEvent { location: pos, serial: SERIAL_COUNTER.next_serial(), time });
        pointer.frame(self);
    }

    fn logic_pointer_button(&mut self, button: u32, pressed: bool, time: u32) {
        let Some(pointer) = self.az.seat.get_pointer() else { return };
        let serial = SERIAL_COUNTER.next_serial();
        if pressed {
            let pos = pointer.current_location();
            let under = match self.under(pos) {
                Under::Surface(f, _, _) => Some(f.0),
                _ => None,
            };
            self.press_outside_menus(under.as_ref(), serial, time);
        }
        // Whether an overlay took this button's press (a new press starts
        // afresh).
        let overlay_press = self.lg.m().interactive.overlay_presses.remove(&button);
        if pressed && !pointer.is_grabbed() {
            let pos = pointer.current_location();
            if crate::overlays::pointer_button(self, button, true, pos) {
                self.lg.m().interactive.overlay_presses.insert(button);
                return;
            }
            if press_binding(self, button, pos, serial, time) {
                return;
            }
            match self.under(pos) {
                Under::Decor(id, hit) => {
                    decor_press(self, id, hit, button, pos, serial, time);
                    return;
                }
                Under::Surface(_, _, Owner::Window(id)) => {
                    let l = self.lg.r();
                    if l.wm.focused() != Some(id) || !l.wm.is_shown(id) {
                        self.focus_window(Some(id));
                    } else {
                        self.lg.m().wm.raise(id);
                        self.redraw_window(id);
                    }
                    if self.az.layer_focus.take().is_some() {
                        self.update_keyboard_focus();
                    }
                }
                Under::Surface(f, _, Owner::Layer) => {
                    let interactive = self.az.outputs.iter().find_map(|o| {
                        layer_map_for_output(o)
                            .layer_for_surface(&f.0, WindowSurfaceType::ALL)
                            .map(|l| l.cached_state().keyboard_interactivity)
                    });
                    if interactive == Some(KeyboardInteractivity::OnDemand) {
                        self.az.layer_focus = Some(f.0.clone());
                        self.update_keyboard_focus();
                    }
                }
                _ => {}
            }
        } else if !pressed
            && !self.az.forwarded_buttons.contains(&button)
            && !pointer.is_grabbed()
            && (crate::overlays::pointer_button(self, button, false, pointer.current_location()) || overlay_press)
        {
            // A release for an overlay that took its press, up still or
            // closed by it. One whose press the seat saw goes to the seat,
            // or its grabs (and the pointer) stay stuck.
            return;
        }
        self.forward_button(button, pressed, serial, time);
    }

    fn logic_pointer_axis(&mut self, a: &AxisEvent) {
        let natural = match a.source {
            AxisSource::Finger => self.az.settings.input.touchpad.natural_scroll,
            _ => self.az.settings.input.pointer.natural_scroll,
        };
        // Natural scrolling turns it round; `scroll_factor` scales it.
        let factor = match a.source {
            AxisSource::Finger => self.az.settings.input.touchpad.scroll_factor(),
            _ => self.az.settings.input.pointer.scroll_factor(),
        };
        let sign = if natural { -factor } else { factor };
        let mut frame = AxisFrame::new(a.time).source(a.source);
        if a.h != 0.0 {
            frame = frame.value(Axis::Horizontal, a.h * sign);
            if let Some(d) = a.h120 {
                frame = frame.v120(Axis::Horizontal, (d * sign) as i32);
            }
        }
        if a.v != 0.0 {
            frame = frame.value(Axis::Vertical, a.v * sign);
            if let Some(d) = a.v120 {
                frame = frame.v120(Axis::Vertical, (d * sign) as i32);
            }
        }
        if a.h_stop {
            frame = frame.stop(Axis::Horizontal);
        }
        if a.v_stop {
            frame = frame.stop(Axis::Vertical);
        }
        if crate::overlays::scroll(self, a.h, a.v) {
            return;
        }
        let Some(pointer) = self.az.seat.get_pointer() else { return };
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    fn logic_touch_down(&mut self, pos: Point<f64, Logical>) {
        if let Under::Surface(_, _, Owner::Window(id)) = self.under(pos)
            && self.lg.r().wm.focused() != Some(id)
        {
            self.focus_window(Some(id));
        }
    }

    /// A key from a keyboard (or IPC): slow keys, then sticky keys, then on
    /// through the core ([`State::key_through`]).
    fn logic_key(&mut self, code: u32, pressed: bool, time: u32) {
        if let Some(ms) = self.az.settings.input.keyboard.slow_keys() {
            if pressed {
                if self.lg.r().access.slow.contains_key(&code) {
                    return;
                }
                // It counts once held this long.
                let timer = self.schedule(Duration::from_millis(ms as u64), crate::hooks::TIMER_SLOW_KEY | code as u64);
                self.lg.m().access.slow.insert(code, timer);
                return;
            }
            // Let go too soon: it never happened.
            if let Some(Some(timer)) = self.lg.m().access.slow.remove(&code) {
                self.cancel_timer(timer);
                return;
            }
        } else if !self.lg.r().access.slow.is_empty() {
            let timers: Vec<TimerId> = self.lg.m().access.slow.drain().filter_map(|(_, t)| t).collect();
            for t in timers {
                self.cancel_timer(t);
            }
        }
        sticky_key(self, code, pressed, time);
    }

    /// A slow key was held long enough: it counts.
    fn slow_key_counted(&mut self, code: u32) {
        if let Some(k) = self.lg.m().access.slow.get_mut(&code) {
            *k = None;
            let time = self.az.start.elapsed().as_millis() as u32;
            sticky_key(self, code, true, time);
        }
    }

    fn decide_key(&mut self, ev: &KeyEvent<'_>) -> KeyDecision {
        let (keycode, pressed, mods, syms) = (ev.keycode, ev.pressed, &ev.mods, ev.syms);
        let m = Mods::from_state(mods);

        // Overlays (switcher, overview, marking menu, area picker) take
        // keys first, releases of ❖ included.
        if let Some(d) = crate::overlays::key(self, syms, m, pressed, ev.utf8) {
            if pressed && let KeyDecision::Swallow | KeyDecision::Act = d {
                self.az.suppressed_keys.insert(keycode);
            }
            return d;
        }
        if !pressed {
            if self.az.translated_keys.remove(&keycode)
                && let Some(fake) = translation(self, mods)
            {
                return KeyDecision::Translate(fake);
            }
            return KeyDecision::Forward;
        }
        if self.az.lock.is_locked() {
            return KeyDecision::Forward;
        }
        let inhibited = self
            .az
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .and_then(|f| self.az.seat.keyboard_shortcuts_inhibitor_for_surface(&f.0))
            .is_some_and(|i| i.is_active());
        if let Some(action) = self.lg.r().bindings.find(m, syms).cloned() {
            // A bare F10 opens the menu bar only over Aizome's apps and the
            // desktop; other apps (mc, htop) keep it.
            let f10 = m == Mods::default() && syms.iter().any(|s| s.raw() == keysyms::KEY_F10);
            let l = self.lg.r();
            let other_app =
                l.wm.focused()
                    .filter(|w| l.wm.is_shown(*w))
                    .and_then(|w| l.wm.get(w))
                    .is_some_and(|w| !crate::cmdkey::is_kit_app(&w.app_id));
            let terminal_command = matches!(action, Action::TerminalClose | Action::TerminalQuit);
            let keep_for_app = (f10 && other_app && matches!(&action, Action::Shell(a) if a == "menu-bar"))
                || (terminal_command && !terminal_commands(self));
            // Inhibitors (VMs, remote desktops) take everything but the
            // core's keys.
            if !inhibited && !keep_for_app {
                self.az.suppressed_keys.insert(keycode);
                self.lg.m().pending_action = Some(action);
                return KeyDecision::Act;
            }
        }
        if !inhibited && let Some(fake) = translation(self, mods) {
            self.az.translated_keys.insert(keycode);
            return KeyDecision::Translate(fake);
        }
        KeyDecision::Forward
    }

    /// Runs a binding's action.
    fn run_action(&mut self, action: Action) {
        let focused = self.lg.r().wm.focused();
        match action {
            Action::GoToDesk(n) => {
                if let Some(d) = self.lg.r().wm.desk_at(n) {
                    self.switch_desk(d);
                }
            }
            Action::TakeToDesk(n) => {
                if let (Some(d), Some(id)) = (self.lg.r().wm.desk_at(n), focused) {
                    self.move_to_desk(id, d, true);
                }
            }
            Action::PrevDesk | Action::NextDesk => {
                let d = self.lg.r().wm.neighbour_desk(action == Action::NextDesk);
                self.switch_desk(d);
            }
            Action::TakeToPrevDesk | Action::TakeToNextDesk => {
                let d = self.lg.r().wm.neighbour_desk(action == Action::TakeToNextDesk);
                if let Some(id) = focused {
                    self.move_to_desk(id, d, true);
                } else {
                    self.switch_desk(d);
                }
            }
            Action::Overview => crate::overlays::toggle_overview(self),
            Action::Switcher { reverse } => crate::overlays::open_switcher(self, reverse, true),
            Action::NextWindowOfApp => {
                if let Some(id) = focused {
                    let l = self.lg.r();
                    let app = l.wm.get(id).map(|w| w.app_id.clone()).unwrap_or_default();
                    let mut same: Vec<WinId> =
                        l.wm.stacking()
                            .iter()
                            .copied()
                            .filter(|w| l.wm.get(*w).is_some_and(|w| w.app_id == app) && l.wm.on_current_desk(*w))
                            .collect();
                    same.sort();
                    if let Some(i) = same.iter().position(|w| *w == id) {
                        let next = same[(i + 1) % same.len()];
                        if next != id {
                            self.activate_window(next);
                        }
                    }
                }
            }
            Action::HideApp => {
                if let Some(app) = focused.and_then(|id| self.lg.r().wm.get(id).map(|w| w.app_id.clone())) {
                    self.hide_app(&app);
                }
            }
            Action::HideOthers => {
                if let Some(app) = focused.and_then(|id| self.lg.r().wm.get(id).map(|w| w.app_id.clone())) {
                    self.hide_others(&app);
                }
            }
            Action::Minimise => {
                if let Some(id) = focused {
                    self.minimise(id);
                }
            }
            Action::Fill => {
                if let Some(id) = focused {
                    self.set_fill(id, None);
                }
            }
            Action::SplitLeft => {
                if let Some(id) = focused {
                    self.set_split(id, crate::wm::Side::Left);
                }
            }
            Action::SplitRight => {
                if let Some(id) = focused {
                    self.set_split(id, crate::wm::Side::Right);
                }
            }
            Action::StackWithBehind => {
                if let Some(id) = focused {
                    self.stack_with_behind(id);
                }
            }
            Action::Fullscreen => {
                if let Some(id) = focused {
                    self.set_fullscreen(id, None);
                }
            }
            Action::Terminal => {
                let argv = self.az.settings.keys.terminal.clone();
                let cwd = self.desk_folder();
                if let Err(e) = self.spawn(&argv, cwd) {
                    tracing::warn!("{e}");
                }
            }
            Action::TerminalClose | Action::TerminalQuit => {
                // Match the shell's fallback Close/Quit: a polite close
                // request, for one window or every window with its app id.
                // Recheck focus if this action came through IPC/settings.
                if terminal_commands(self)
                    && let Some(id) = focused
                {
                    let l = self.lg.r();
                    let ids = if action == Action::TerminalQuit {
                        let app = &l.wm.get(id).expect("focused terminal").app_id;
                        l.wm.windows().filter(|w| &w.app_id == app).map(|w| w.id).collect()
                    } else {
                        vec![id]
                    };
                    for id in ids {
                        self.close_window(id);
                    }
                }
            }
            Action::Spawn(argv) => {
                let cwd = self.desk_folder();
                if let Err(e) = self.spawn(&argv, cwd) {
                    tracing::warn!("{e}");
                }
            }
            Action::ScreenshotScreen => {
                use aizome_comp_core::screenshot;
                if let Err(e) = screenshot::take(self, screenshot::What::Screen, None, true) {
                    tracing::warn!("screenshot: {e}");
                }
            }
            Action::ScreenshotArea => crate::overlays::start_area_pick(self, None),
            Action::ScreenshotWindow => {
                use aizome_comp_core::screenshot;
                if let Some(id) = focused
                    && let Err(e) = screenshot::take(self, screenshot::What::Window(id), None, true)
                {
                    tracing::warn!("screenshot: {e}");
                }
            }
            Action::ChangeVt(vt) => self.backend.change_vt(vt),
            Action::Quit => self.quit(),
            Action::Reload => self.reload_soon(Default::default()),
            Action::Shell(name) => {
                let output = self.az.output_at(self.az.pointer_pos).map(|o| o.name());
                self.az.ipc.emit(&aizome_comp_core::aizome_ipc::Event::Binding { action: name, pointer: None, output });
            }
            Action::None => {}
        }
    }

    fn under(&mut self, pos: Point<f64, Logical>) -> Under {
        let Some(output) = self.az.output_at(pos) else { return Under::Nothing };
        let out_loc = self.az.output_geometry(&output).loc.to_f64();
        if self.az.lock.is_locked() {
            return match aizome_comp_core::input::lock_under(&self.az, &output, pos) {
                Some((f, o, w)) => Under::Surface(f, o, w),
                None => Under::Nothing,
            };
        }
        // The core's view of where popups' parents are must be current.
        crate::hooks::publish(self);
        // Popups (menus) are above everything.
        if let Some((f, o, w)) = self.az.popup_under(pos) {
            return Under::Surface(f, o, w);
        }
        if let Some((f, o, w)) = layer_under(&output, &[Layer::Overlay], pos, out_loc) {
            return Under::Surface(f, o, w);
        }
        let l = self.lg.r();
        let shown = l.wm.shown_stacking();
        let top_fullscreen = shown
            .last()
            .copied()
            .filter(|id| l.wm.get(*id).is_some_and(|w| w.fullscreen()) && l.wm.focused() == Some(*id));
        if let Some(id) = top_fullscreen
            && let Some(u) = window_under(self, id, pos)
        {
            return u;
        }
        if let Some((f, o, w)) = layer_under(&output, &[Layer::Top], pos, out_loc) {
            return Under::Surface(f, o, w);
        }
        for id in shown.iter().rev() {
            if Some(*id) == top_fullscreen {
                continue;
            }
            if let Some(u) = window_under(self, *id, pos) {
                return u;
            }
        }
        if let Some((f, o, w)) = layer_under(&output, &[Layer::Bottom, Layer::Background], pos, out_loc) {
            return Under::Surface(f, o, w);
        }
        Under::Nothing
    }
}

fn window_under(st: &mut State, id: WinId, pos: Point<f64, Logical>) -> Option<Under> {
    let (rect, fullscreen) = st.lg.r().wm.get(id).map(|w| (w.rect, w.fullscreen()))?;
    let geo = st.az.windows.get(&id)?.window.geometry();
    let origin = Point::<f64, Logical>::from(((rect.x - geo.loc.x) as f64, (rect.y - geo.loc.y) as f64));
    if !fullscreen
        && let Some(layout) = st.lg.r().decor_layout(&mut st.az.labels, id)
        && let Some(hit) = crate::decor::hit(&layout, pos.x, pos.y)
    {
        return Some(Under::Decor(id, hit));
    }
    let m = st.az.windows.get(&id)?;
    if let Some((s, loc)) =
        m.window.surface_under(pos - origin, WindowSurfaceType::TOPLEVEL | WindowSurfaceType::SUBSURFACE)
    {
        return Some(Under::Surface(Focus(s), loc.to_f64() + origin, Owner::Window(id)));
    }
    None
}

/// Pointer bindings: ⌃❖-drag moves, ⌃❖-right-drag resizes,
/// ❖-right-click is the shell's app menu.
fn press_binding(st: &mut State, button: u32, pos: Point<f64, Logical>, serial: Serial, time: u32) -> bool {
    let Some(kb) = st.az.seat.get_keyboard() else { return false };
    let mods = Mods::from_state(&kb.modifier_state());
    if !mods.logo || st.az.lock.is_locked() {
        return false;
    }
    let target = match st.under(pos) {
        Under::Surface(_, _, Owner::Window(id)) | Under::Decor(id, _) => Some(id),
        _ => None,
    };
    if mods.ctrl && !mods.alt {
        let Some(id) = target else { return false };
        st.focus_window(Some(id));
        match button {
            BTN_LEFT => crate::grabs::start_move(st, id, pos, BTN_LEFT, serial, time, false),
            BTN_RIGHT => {
                let edges =
                    st.lg.r().wm.get(id).map(|w| nearest_corner(w.rect, pos)).unwrap_or(Edges::BOTTOM | Edges::RIGHT);
                crate::grabs::start_resize(st, id, edges, pos, BTN_RIGHT, serial, time);
            }
            _ => return false,
        }
        return true;
    }
    if !mods.ctrl && !mods.alt && !mods.shift && button == BTN_RIGHT {
        let output = st.az.output_at(pos).map(|o| o.name());
        st.az.ipc.emit(&aizome_comp_core::aizome_ipc::Event::Binding {
            action: "app-menu".into(),
            pointer: Some((pos.x, pos.y)),
            output,
        });
        return true;
    }
    false
}

fn decor_press(st: &mut State, id: WinId, hit: Hit, button: u32, pos: Point<f64, Logical>, serial: Serial, time: u32) {
    let shift = st.az.seat.get_keyboard().is_some_and(|k| k.modifier_state().shift);
    match (button, hit) {
        (BTN_LEFT, Hit::Close(w)) | (BTN_LEFT, Hit::Zoom(w)) => {
            st.focus_window(Some(w));
            crate::grabs::start_box_click(st, hit, pos, serial, time);
        }
        (BTN_LEFT, Hit::Tab(w)) => {
            // A double click fills (the zoom box), parks, or does nothing,
            // as `[windows] tab_double_click` says.
            let now = Instant::now();
            let double = st.lg.r().interactive.last_tab_click.is_some_and(|(t, last, p)| {
                last == w
                    && now.duration_since(t) < Duration::from_millis(400)
                    && (p.0 - pos.x).abs() < 4.0
                    && (p.1 - pos.y).abs() < 4.0
            });
            if double {
                st.lg.m().interactive.last_tab_click = None;
                match st.az.settings.windows.tab_double_click() {
                    aizome_comp_core::aizome_config::TabDoubleClick::Fill => st.set_fill(w, None),
                    aizome_comp_core::aizome_config::TabDoubleClick::Minimise => st.minimise(w),
                    aizome_comp_core::aizome_config::TabDoubleClick::Nothing => {}
                }
                return;
            }
            st.lg.m().interactive.last_tab_click = Some((now, w, (pos.x, pos.y)));
            // A background tab shows on release (a click), or tears out
            // of its group when dragged (the move grab decides).
            if st.lg.r().wm.is_group_active(w) {
                st.focus_window(Some(w));
            }
            if shift {
                crate::grabs::start_tab_slide(st, w, pos, serial, time);
            } else {
                crate::grabs::start_move(st, w, pos, BTN_LEFT, serial, time, true);
            }
        }
        (BTN_LEFT, Hit::Edge(w, edges)) => {
            st.focus_window(Some(w));
            crate::grabs::start_resize(st, w, edges, pos, BTN_LEFT, serial, time);
        }
        (BTN_MIDDLE, Hit::Tab(w)) | (BTN_MIDDLE, Hit::Close(w)) | (BTN_MIDDLE, Hit::Zoom(w)) => {
            st.lower_window(w);
        }
        (BTN_RIGHT, Hit::Tab(w)) | (BTN_RIGHT, Hit::Close(w)) | (BTN_RIGHT, Hit::Zoom(w)) => {
            st.focus_window(Some(w));
            crate::overlays::marking::press(st, w, pos, serial, time);
        }
        _ => {
            let _ = id;
        }
    }
}

/// Sticky keys: a modifier tapped alone latches until the next key
/// (tapped again, it lets go); that key then comes with it.
fn sticky_key(st: &mut State, code: u32, pressed: bool, time: u32) {
    if !st.az.settings.input.keyboard.sticky_keys() {
        let a = &mut st.lg.m().access;
        a.latched.clear();
        a.alone = None;
        return st.key_through(code, pressed, time);
    }
    if MODIFIER_KEYS.contains(&code) {
        let a = &mut st.lg.m().access;
        if pressed {
            a.alone = Some(code);
        } else if a.alone.take() == Some(code) {
            match a.latched.iter().position(|m| *m == code) {
                Some(i) => {
                    a.latched.remove(i);
                }
                None => a.latched.push(code),
            }
        }
        return st.key_through(code, pressed, time);
    }
    st.lg.m().access.alone = None;
    if pressed && !st.lg.r().access.latched.is_empty() {
        let mods = std::mem::take(&mut st.lg.m().access.latched);
        for m in &mods {
            st.key_through(*m, true, time);
        }
        st.key_through(code, true, time);
        st.lg.m().access.applied.push((code, mods));
        return;
    }
    if !pressed && let Some(i) = st.lg.r().access.applied.iter().position(|(k, _)| *k == code) {
        let (_, mods) = st.lg.m().access.applied.remove(i);
        st.key_through(code, false, time);
        for m in mods.iter().rev() {
            st.key_through(*m, false, time);
        }
        return;
    }
    st.key_through(code, pressed, time);
}

/// Only the terminal client itself may receive fallback window commands;
/// an open shell layer or popup keeps its own keyboard decisions.
fn terminal_commands(st: &State) -> bool {
    let l = st.lg.r();
    let Some(id) = l.wm.focused() else { return false };
    let Some(w) = l.wm.get(id) else { return false };
    let client_has_focus = st
        .az
        .seat
        .get_keyboard()
        .and_then(|k| k.current_focus())
        .is_some_and(|f| st.az.window_for_root(&f.0) == Some(id));
    crate::cmdkey::terminal_commands(
        &w.app_id,
        &st.az.settings.keys.terminals,
        st.az.settings.keys.command_key_for_all_apps,
        client_has_focus,
    )
}

/// The modifiers a non-kit client sees for a ❖ chord: Ctrl (and Shift in
/// terminals) instead of the logo key.
fn translation(st: &State, mods: &ModifiersState) -> Option<ModifiersState> {
    if !mods.logo || !st.az.settings.keys.command_key_for_all_apps {
        return None;
    }
    let l = st.lg.r();
    let id = l.wm.focused()?;
    let w = l.wm.get(id)?;
    let focused_is_window =
        st.az.seat.get_keyboard()?.current_focus().is_some_and(|f| st.az.window_for_root(&f.0) == Some(id));
    if !focused_is_window || crate::cmdkey::is_kit_app(&w.app_id) {
        return None;
    }
    let terminal = st.az.settings.keys.terminals.iter().any(|t| t == &w.app_id);
    let mut fake = *mods;
    fake.logo = false;
    fake.ctrl = true;
    if terminal {
        fake.shift = true;
    }
    Some(fake)
}

/// The corner of a rect nearest a point.
fn nearest_corner(r: crate::wm::Rect, p: Point<f64, Logical>) -> Edges {
    let (cx, cy) = r.center();
    let mut e = Edges::empty();
    e |= if p.x < cx as f64 { Edges::LEFT } else { Edges::RIGHT };
    e |= if p.y < cy as f64 { Edges::TOP } else { Edges::BOTTOM };
    e
}

/// A window closed: interactive state forgets it.
pub fn window_gone(st: &mut State, id: WinId) {
    let i = &mut st.lg.m().interactive;
    if matches!(i.hover, Some(Hit::Tab(w) | Hit::Close(w) | Hit::Zoom(w) | Hit::Edge(w, _)) if w == id) {
        i.hover = None;
    }
    if i.drag.as_ref().is_some_and(|d| d.win == id || d.target == Some(id)) {
        i.drag = None;
    }
}

/// Accessibility state of the keyboard: slow keys waiting to count, and
/// sticky modifiers.
#[derive(Default)]
pub struct Access {
    /// Keys held under slow keys: the timer while it hasn't counted yet.
    pub slow: HashMap<u32, Option<TimerId>>,
    /// A modifier pressed with nothing else since.
    pub alone: Option<u32>,
    /// Modifiers latched for the next key.
    pub latched: Vec<u32>,
    /// Keys down with latched modifiers pressed for them.
    pub applied: Vec<(u32, Vec<u32>)>,
}
