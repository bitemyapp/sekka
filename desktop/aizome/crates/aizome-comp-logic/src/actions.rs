//! Window-management actions: the model changes, then the windows behind
//! it hear about it (configures, focus, redraws, IPC events). Also what
//! happens when windows come and go, and the logic taking in the windows
//! the core has when it starts (after a reload, say).

use aizome_comp_core::aizome_ipc::{DeskInfo, Event, Rect as IpcRect, WindowInfo};
use aizome_comp_core::logic::{MapInfo, ToplevelRequest};
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData};
use tracing::info;

use crate::prelude::*;
use crate::wm::{DeskId, DeskMove, Placement, Refocus, ScreenGeom, Side};

/// One work area for initial bounds, restored geometry, fill/split and
/// subsequent shell/output changes. The Deskbar does not use an exclusive
/// zone, but automatic placement must reserve its column all the same.
fn window_area(az: &Aizome, output: &Output) -> Rect {
    crate::wm::beside(az.usable_area(output), az.deskbar_rect(output))
}

/// What the logic does to windows, on the compositor's state.
pub trait Actions {
    fn focus_window(&mut self, id: Option<WinId>);
    fn apply_refocus(&mut self, r: Refocus);
    fn activate_window(&mut self, id: WinId);
    fn close_window(&mut self, id: WinId);
    fn minimise(&mut self, id: WinId);
    fn unminimise(&mut self, id: WinId);
    fn hide_app(&mut self, app_id: &str);
    fn hide_others(&mut self, app_id: &str);
    fn show_app(&mut self, app_id: &str);
    fn set_fill(&mut self, id: WinId, on: Option<bool>);
    fn set_split(&mut self, id: WinId, side: Side);
    fn set_fullscreen(&mut self, id: WinId, on: Option<bool>);
    fn move_window(&mut self, id: WinId, x: i32, y: i32);
    fn resize_window(&mut self, id: WinId, rect: Rect);
    fn set_float(&mut self, id: WinId, on: Option<bool>);
    fn set_sticky(&mut self, id: WinId, on: Option<bool>);
    fn move_to_desk(&mut self, id: WinId, desk: DeskId, follow: bool);
    /// Shows a Desk on the display with the focus (see [`Actions::show_desk`]).
    fn switch_desk(&mut self, desk: DeskId);
    /// Shows a Desk on a display (the one with the focus by default) and
    /// makes it current; one another display shows swaps places with it.
    fn show_desk(&mut self, output: Option<&str>, desk: DeskId);
    /// Gives the focus to a display (its Desk current).
    fn focus_screen(&mut self, output: &str);
    /// Parks a Desk (its windows are told they're suspended) or unparks
    /// it; the current Desk isn't parked. True if that changed it.
    fn set_desk_parked(&mut self, desk: DeskId, park: bool) -> bool;
    fn emit_desks(&mut self);
    fn stack(&mut self, a: WinId, b: WinId) -> bool;
    fn unstack(&mut self, id: WinId, to: Option<(i32, i32)>) -> bool;
    fn show_group_member(&mut self, id: WinId);
    fn lower_window(&mut self, id: WinId);
    fn stack_with_behind(&mut self, id: WinId);
    fn spawn(&mut self, argv: &[String], cwd: Option<std::path::PathBuf>) -> Result<u32, String>;
    fn desk_folder(&self) -> Option<std::path::PathBuf>;
    /// Queues redraws wherever a window (with its decorations) shows.
    fn redraw_window(&mut self, id: WinId);
    /// The usable area and decoration insets for placing a window.
    fn usable_for(&self, id: WinId) -> (Rect, crate::wm::Insets);
    /// Asks the client for a size (sent with the next configure).
    fn request_size(&mut self, id: WinId, w: i32, h: i32);
    /// Sends a window its state.
    fn configure(&mut self, id: WinId);
    /// Describes a window for IPC.
    fn window_info(&self, id: WinId) -> Option<WindowInfo>;
}

impl Actions for State {
    /// Focuses a window (or nothing): the model, keyboard focus, activated
    /// states, decorations and IPC.
    fn focus_window(&mut self, id: Option<WinId>) {
        let l = self.lg.m();
        let prev = l.wm.focused();
        let desk = l.wm.current_desk();
        l.wm.focus(id);
        let now = l.wm.focused();
        // A window on another display takes the focus there.
        if l.wm.current_desk() != desk {
            current_desk_changed(self);
        }
        let l = self.lg.m();
        // Selecting a tab also changes the previous active member's IPC
        // state, even when the previously focused window was outside it.
        if let Some(id) = now {
            for member in l.wm.group_members(id) {
                l.window_changed(member);
            }
        }
        if prev != now {
            for (w, on) in [(prev, false), (now, true)] {
                if let Some(w) = w {
                    if let Some(m) = self.az.windows.get(&w) {
                        m.window.set_activated(on);
                    }
                    self.configure(w);
                    self.lg.m().window_changed(w);
                    self.redraw_window(w);
                }
            }
            self.az.ipc.emit(&Event::WindowFocused { id: now });
            // Focus moving to another window closes the open menus.
            let surface = now.and_then(|w| self.az.windows.get(&w)).map(|m| m.surface().clone());
            if self.az.popup_grab.as_ref().is_some_and(|g| g.live() && surface.as_ref() != Some(&g.root)) {
                self.dismiss_popup_grab();
            }
            // And takes the keys from a layer surface clicked earlier (the
            // bar, the Deskbar): a window that opens or is switched to gets
            // what's typed next.
            if now.is_some() {
                self.az.layer_focus = None;
            }
        }
        if let Some(w) = now {
            // Raising changes what overlaps what.
            self.redraw_window(w);
            self.az.window_outputs_dirty = true;
        }
        self.update_keyboard_focus();
    }

    fn apply_refocus(&mut self, r: Refocus) {
        if let Refocus::To(id) = r {
            self.focus_window(id);
        } else {
            self.update_keyboard_focus();
        }
    }

    /// Shows a window wherever it is (its Desk, unminimised, its app
    /// unhidden) and focuses it.
    fn activate_window(&mut self, id: WinId) {
        let before = self.lg.r().wm.current_desk();
        let hidden_app = self.lg.r().wm.get(id).filter(|w| w.hidden).map(|w| w.app_id.clone());
        let moves = self.lg.m().wm.reveal(id);
        desks_moved(self, before, &moves, true);
        if let Some(app) = hidden_app {
            after_app_change(self, &app);
        }
        self.lg.m().window_changed(id);
        // Asked for (the switcher, a Deskbar row, IPC): the keys go to it,
        // even from a layer surface clicked to ask, and even when it had
        // the focus already.
        self.az.layer_focus = None;
        self.focus_window(Some(id));
        self.az.window_outputs_dirty = true;
        self.az.queue_redraw_all();
    }

    fn close_window(&mut self, id: WinId) {
        if let Some(m) = self.az.windows.get(&id) {
            m.toplevel().send_close();
        }
    }

    fn minimise(&mut self, id: WinId) {
        let r = self.lg.m().wm.minimise(id);
        after_visibility_change(self, id);
        self.apply_refocus(r);
    }

    fn unminimise(&mut self, id: WinId) {
        self.lg.m().wm.unminimise(id);
        after_visibility_change(self, id);
        self.activate_window(id);
    }

    fn hide_app(&mut self, app_id: &str) {
        let r = self.lg.m().wm.hide_app(app_id);
        after_app_change(self, app_id);
        self.apply_refocus(r);
    }

    fn hide_others(&mut self, app_id: &str) {
        let l = self.lg.m();
        let r = l.wm.hide_others(app_id);
        let ids: Vec<WinId> = l.wm.windows().map(|w| w.id).collect();
        for id in ids {
            l.window_changed(id);
        }
        self.az.window_outputs_dirty = true;
        self.az.queue_redraw_all();
        self.apply_refocus(r);
    }

    fn show_app(&mut self, app_id: &str) {
        let l = self.lg.m();
        l.wm.show_app(app_id);
        after_app_change(self, app_id);
        // Focus the app's most recent window.
        let l = self.lg.r();
        let next =
            l.wm.mru().iter().copied().find(|w| l.wm.get(*w).is_some_and(|w| w.app_id == app_id) && l.wm.is_shown(*w));
        if let Some(w) = next {
            self.focus_window(Some(w));
        }
    }

    /// Fill (the zoom box) on, off or toggled.
    fn set_fill(&mut self, id: WinId, on: Option<bool>) {
        if !self.lg.r().wm.get(id).is_some_and(|w| w.fill()) && group_has_fixed_size(self, id) {
            return;
        }
        let (usable, insets) = self.usable_for(id);
        let l = self.lg.m();
        let old = l.wm.get(id).map(|w| w.rect);
        if let Some(rect) = l.wm.set_fill(id, on, usable, insets) {
            after_placement(self, id, old, rect);
        }
    }

    fn set_split(&mut self, id: WinId, side: Side) {
        let restoring = self.lg.r().wm.get(id).is_some_and(|w| {
            matches!((w.placement, side), (Placement::SplitLeft, Side::Left) | (Placement::SplitRight, Side::Right))
        });
        if !restoring && group_has_fixed_size(self, id) {
            return;
        }
        let (usable, insets) = self.usable_for(id);
        let l = self.lg.m();
        let old = l.wm.get(id).map(|w| w.rect);
        if let Some(rect) = l.wm.set_split(id, side, usable, insets) {
            after_placement(self, id, old, rect);
        }
    }

    fn set_fullscreen(&mut self, id: WinId, on: Option<bool>) {
        set_fullscreen_on(self, id, on, None);
    }

    /// Moves a window's content to a point (its group with it).
    fn move_window(&mut self, id: WinId, x: i32, y: i32) {
        let l = self.lg.m();
        let Some(w) = l.wm.get(id) else { return };
        let old = w.rect;
        let rect = Rect::new(x, y, old.w, old.h);
        if rect == old {
            return;
        }
        for m in l.wm.set_rect(id, rect) {
            if let Some(w) = l.wm.get_mut(m)
                && w.placement != Placement::Free
            {
                w.placement = Placement::Free;
                w.restore = None;
            }
            l.window_changed(m);
        }
        self.az.queue_redraw_rect(old);
        self.az.queue_redraw_rect(rect);
        self.az.window_outputs_dirty = true;
    }

    /// Sets a window's content rectangle (asking the client for the size).
    fn resize_window(&mut self, id: WinId, rect: Rect) {
        let Some(w) = self.lg.r().wm.get(id) else { return };
        let old = w.rect;
        // Within the sizes the client allows, as a resize by the edges is
        // (Tenki under 640pt wide stacks its words a letter to a line).
        let (min, max) = crate::grabs::size_limits(self, id);
        let rect = Rect::new(rect.x, rect.y, rect.w.clamp(min.0, max.0), rect.h.clamp(min.1, max.1));
        let members = self.lg.m().wm.set_rect(id, rect);
        for m in members {
            if let Some(w) = self.lg.m().wm.get_mut(m) {
                w.placement = Placement::Free;
                w.restore = None;
            }
            self.request_size(m, rect.w, rect.h);
            self.configure(m);
            self.lg.m().window_changed(m);
        }
        self.az.queue_redraw_rect(old);
        self.az.queue_redraw_rect(rect);
        self.az.window_outputs_dirty = true;
    }

    fn set_float(&mut self, id: WinId, on: Option<bool>) {
        let l = self.lg.m();
        for m in l.wm.group_members(id) {
            if let Some(w) = l.wm.get_mut(m) {
                w.float = on.unwrap_or(!w.float);
            }
            l.window_changed(m);
        }
        self.redraw_window(id);
    }

    fn set_sticky(&mut self, id: WinId, on: Option<bool>) {
        let l = self.lg.m();
        let now = l.wm.get(id).is_some_and(|w| w.sticky);
        l.wm.set_sticky(id, on.unwrap_or(!now));
        for m in l.wm.group_members(id) {
            l.window_changed(m);
        }
        self.az.window_outputs_dirty = true;
        self.az.queue_redraw_all();
    }

    /// Sends a window (its group, its dialogs) to a Desk; with `follow`,
    /// goes along.
    fn move_to_desk(&mut self, id: WinId, desk: DeskId, follow: bool) {
        let l = self.lg.m();
        let r = l.wm.move_to_desk(id, desk);
        let members = l.wm.group_members(id);
        for m in &members {
            l.window_changed(*m);
        }
        // Onto a parked Desk, or off one: suspended or not.
        for m in members {
            self.configure(m);
        }
        self.az.window_outputs_dirty = true;
        self.az.queue_redraw_all();
        if follow {
            // Along to its display, if one shows it.
            let before = self.lg.r().wm.current_desk();
            if let Some((_, moves)) = self.lg.m().wm.visit_desk(desk) {
                desks_moved(self, before, &moves, true);
            }
            self.focus_window(Some(id));
        } else {
            self.apply_refocus(r);
        }
        self.emit_desks();
    }

    fn switch_desk(&mut self, desk: DeskId) {
        self.show_desk(None, desk);
    }

    /// Shows a Desk on a display, sliding over the motion time. A parked
    /// Desk wakes as it's arrived at.
    fn show_desk(&mut self, output: Option<&str>, desk: DeskId) {
        let before = self.lg.r().wm.current_desk();
        let Some((r, moves)) = self.lg.m().wm.show_desk(output, desk) else { return };
        desks_moved(self, before, &moves, true);
        self.apply_refocus(r);
    }

    fn focus_screen(&mut self, output: &str) {
        let before = self.lg.r().wm.current_desk();
        let Some(r) = self.lg.m().wm.focus_screen(output) else { return };
        desks_moved(self, before, &[], false);
        self.apply_refocus(r);
    }

    fn set_desk_parked(&mut self, desk: DeskId, park: bool) -> bool {
        if !self.lg.m().wm.set_parked(desk, park.then(now_secs)) {
            return false;
        }
        for id in self.lg.r().wm.desk_windows(desk) {
            self.configure(id);
        }
        true
    }

    fn emit_desks(&mut self) {
        let desks = desk_infos(self.lg.r());
        self.az.ipc.emit(&Event::DesksChanged { desks });
    }

    /// Stacks `b` into `a`'s group, sliding its tab into place.
    fn stack(&mut self, a: WinId, b: WinId) -> bool {
        let l = self.lg.m();
        let before: Vec<(WinId, Rect)> =
            l.wm.group_members(b)
                .iter()
                .filter_map(|m| l.decor_tab_rect(&mut self.az.labels, *m).map(|r| (*m, r)))
                .collect();
        let old_b = l.wm.get(b).map(|w| w.rect);
        let Some(changed) = l.wm.stack(a, b) else { return false };
        let rect = l.wm.get(a).map(|w| w.rect).unwrap_or_default();
        for m in &changed {
            self.request_size(*m, rect.w, rect.h);
            self.configure(*m);
        }
        let l = self.lg.m();
        for m in l.wm.group_members(a) {
            l.window_changed(m);
        }
        // Tabs slide from where they were.
        let ms = self.az.motion_ms();
        let l = self.lg.m();
        for (m, r0) in before {
            if let Some(r1) = l.decor_tab_rect(&mut self.az.labels, m) {
                l.anims.start_tab(m, (r0.x - r1.x) as f64, (r0.y - r1.y) as f64, ms);
            }
        }
        if let Some(o) = old_b {
            self.az.queue_redraw_rect(o);
        }
        self.az.queue_redraw_rect(rect);
        self.az.window_outputs_dirty = true;
        self.focus_window(Some(b));
        true
    }

    /// Takes a window out of its group, a tab's height down and right (or
    /// at `to`).
    fn unstack(&mut self, id: WinId, to: Option<(i32, i32)>) -> bool {
        let l = self.lg.m();
        let Some(w) = l.wm.get(id) else { return false };
        let rect = w.rect;
        if !l.wm.unstack(id) {
            return false;
        }
        let (x, y) = to.unwrap_or((rect.x + crate::decor::TAB_H + 8, rect.y + crate::decor::TAB_H + 8));
        if let Some(w) = l.wm.get_mut(id) {
            w.rect = Rect::new(x, y, rect.w, rect.h);
            w.tab_offset = 0;
            w.home = None;
        }
        let members = l.wm.group_members(rect_owner(&l.wm, id, rect));
        for m in members {
            l.window_changed(m);
        }
        l.window_changed(id);
        self.az.queue_redraw_rect(rect);
        self.redraw_window(id);
        self.az.window_outputs_dirty = true;
        self.focus_window(Some(id));
        true
    }

    /// Shows one member of a group (clicking its tab).
    fn show_group_member(&mut self, id: WinId) {
        let l = self.lg.m();
        l.wm.set_group_active(id);
        for m in l.wm.group_members(id) {
            l.window_changed(m);
        }
        self.az.window_outputs_dirty = true;
        self.focus_window(Some(id));
        self.redraw_window(id);
    }

    /// Sends a window (its group) to the back.
    fn lower_window(&mut self, id: WinId) {
        let r = self.lg.m().wm.lower(id);
        self.redraw_window(id);
        self.az.window_outputs_dirty = true;
        self.apply_refocus(r);
    }

    /// Stacks the focused window with the one behind it (⌥❖S).
    fn stack_with_behind(&mut self, id: WinId) {
        if let Some(behind) = self.lg.r().wm.window_behind(id) {
            self.stack(behind, id);
        }
    }

    /// Runs a program with the session's environment, in a folder.
    fn spawn(&mut self, argv: &[String], cwd: Option<std::path::PathBuf>) -> Result<u32, String> {
        aizome_comp_core::session::spawn(&self.az, argv, cwd)
    }

    /// The current Desk's folder, expanded.
    fn desk_folder(&self) -> Option<std::path::PathBuf> {
        let l = self.lg.r();
        let d = l.wm.desk(l.wm.current_desk())?;
        let f = d.folder.as_ref()?;
        let p = aizome_comp_core::aizome_config::expand_home(f);
        p.is_dir().then_some(p)
    }

    fn redraw_window(&mut self, id: WinId) {
        if let Some(r) = self.lg.r().wm.get(id).map(|w| w.rect) {
            self.az.queue_redraw_rect(r);
        }
    }

    fn usable_for(&self, id: WinId) -> (Rect, crate::wm::Insets) {
        let l = self.lg.r();
        let rect = l.wm.get(id).map(|w| w.rect).unwrap_or_default();
        // Filling and splitting leave the Deskbar clear, on the window's
        // display.
        let usable = window_output(self, id).map(|o| window_area(&self.az, &o)).unwrap_or(rect);
        (usable, l.insets_for(id))
    }

    fn request_size(&mut self, id: WinId, w: i32, h: i32) {
        if self.az.windows.contains_key(&id) {
            self.lg.m().data(id).requested_size = Some((w, h));
        }
    }

    /// Sends a window its state: size, activated, fill/split/full screen,
    /// decoration mode and bounds.
    fn configure(&mut self, id: WinId) {
        let l = self.lg.m();
        let Some(w) = l.wm.get(id) else { return };
        let Some(m) = self.az.windows.get(&id) else { return };
        let focused = l.wm.focused() == Some(id);
        let parked = l.wm.is_parked_window(id);
        let placement = w.placement;
        let ssd = w.ssd;
        let rect = w.rect;
        let requested = l.wins.get(&id).and_then(|d| d.requested_size);
        let size = requested.or_else(|| (placement != Placement::Free).then_some((rect.w, rect.h)));
        let bounds = self.az.output_for_rect(rect).map(|o| window_area(&self.az, &o));
        let insets = l.insets_for(id);
        let toplevel = m.toplevel().clone();
        toplevel.with_pending_state(|s| {
            use xdg_toplevel::State as S;
            let set = |s: &mut smithay::wayland::shell::xdg::ToplevelState, st: S, on: bool| {
                if on {
                    s.states.set(st);
                } else {
                    s.states.unset(st);
                }
            };
            set(s, S::Activated, focused);
            // A parked Desk's windows: clients stop drawing until it wakes
            // (smithay leaves it out for clients older than xdg-shell 6).
            set(s, S::Suspended, parked);
            set(s, S::Maximized, placement == Placement::Fill);
            set(s, S::Fullscreen, placement == Placement::Fullscreen);
            let left = placement == Placement::SplitLeft;
            let right = placement == Placement::SplitRight;
            set(s, S::TiledLeft, left || placement == Placement::Fill);
            set(s, S::TiledRight, right || placement == Placement::Fill);
            set(s, S::TiledTop, left || right || placement == Placement::Fill);
            set(s, S::TiledBottom, left || right || placement == Placement::Fill);
            if let Some((w, h)) = size {
                s.size = Some((w, h).into());
            }
            if let Some(b) = bounds {
                let b = b.inset(insets);
                s.bounds = Some((b.w, b.h).into());
            }
            s.decoration_mode = Some(if ssd { DecorationMode::ServerSide } else { DecorationMode::ClientSide });
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn window_info(&self, id: WinId) -> Option<WindowInfo> {
        let l = self.lg.r();
        let w = l.wm.get(id)?;
        let group = l.wm.group_of(id);
        let output = self.az.output_for_rect(w.rect).map(|o| o.name());
        Some(WindowInfo {
            id,
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            pid: w.pid,
            desk: if w.sticky { None } else { l.wm.desk_index(w.desk) },
            sticky: w.sticky,
            group: group.map(|g| g.id),
            group_members: group.map(|g| g.members.clone()).unwrap_or_default(),
            group_active: l.wm.is_group_active(id),
            focused: l.wm.focused() == Some(id),
            minimised: w.minimised,
            hidden: w.hidden,
            urgent: w.urgent,
            fullscreen: w.fullscreen(),
            fill: w.fill(),
            float: w.float,
            geometry: IpcRect::new(w.rect.x, w.rect.y, w.rect.w, w.rect.h),
            output,
            client_side_decorations: !w.ssd,
            kit_app: crate::cmdkey::is_kit_app(&w.app_id),
            parent: w.parent,
        })
    }
}

fn after_app_change(st: &mut State, app_id: &str) {
    let l = st.lg.m();
    let ids: Vec<WinId> = l.wm.windows().filter(|w| w.app_id == app_id).map(|w| w.id).collect();
    for id in ids {
        l.window_changed(id);
    }
    st.az.window_outputs_dirty = true;
    st.az.queue_redraw_all();
}

fn after_visibility_change(st: &mut State, id: WinId) {
    st.lg.m().window_changed(id);
    st.az.window_outputs_dirty = true;
    st.redraw_window(id);
}

/// Fixed-size games and dialogs cannot redraw at a zoomed or tiled size.
/// Check every tab because the group shares one content rectangle.
fn group_has_fixed_size(st: &State, id: WinId) -> bool {
    st.lg
        .r()
        .wm
        .group_members(id)
        .iter()
        .any(|id| st.az.windows.get(id).is_some_and(|w| crate::memory::SizeConstraints::of(w.toplevel()).fixed()))
}

fn after_placement(st: &mut State, id: WinId, old: Option<Rect>, mut rect: Rect) {
    if st.lg.r().wm.get(id).is_some_and(|w| w.placement == Placement::Free) {
        // A saved free rectangle can outlive an output or Deskbar change.
        let (area, insets) = st.usable_for(id);
        let fitted = crate::wm::fit_into(rect, area, insets);
        if fitted != rect {
            st.lg.m().wm.fit_rect(id, fitted);
            rect = fitted;
        }
    }
    let members = st.lg.r().wm.group_members(id);
    for m in &members {
        st.request_size(*m, rect.w, rect.h);
        st.configure(*m);
        st.lg.m().window_changed(*m);
    }
    if let Some(o) = old {
        st.az.queue_redraw_rect(o);
    }
    st.az.queue_redraw_rect(rect);
    st.az.window_outputs_dirty = true;
}

/// Displays changed Desks, or the focus went to another display: the
/// Desks arriving slide in (when `slide`), those no display shows now say
/// when they were left, and the shell hears.
fn desks_moved(st: &mut State, before: DeskId, moves: &[DeskMove], slide: bool) {
    let ms = if slide { st.az.motion_ms() } else { 0 };
    let now = now_secs();
    let l = st.lg.m();
    for mv in moves {
        let Some(from) = mv.from else { continue };
        if !l.wm.is_visible(from) {
            l.wm.mark_left(from, now);
        }
        let fi = l.wm.desk_index(from).unwrap_or(0);
        let ti = l.wm.desk_index(mv.to).unwrap_or(0);
        let dir = if ti >= fi { 1.0 } else { -1.0 };
        l.anims.start_desk(&mv.output, from, mv.to, dir, ms);
    }
    if moves.is_empty() && l.wm.current_desk() == before {
        return;
    }
    let ids: Vec<WinId> = l.wm.windows().map(|w| w.id).collect();
    for id in ids {
        l.window_changed(id);
    }
    // A Desk arriving fits its display as it is now (its bar, the
    // Deskbar), as one there all along does.
    for mv in moves {
        if let Some(o) = st.az.output_by_name(&mv.output) {
            refit_placed_windows(st, &o);
        }
    }
    st.az.window_outputs_dirty = true;
    st.az.queue_redraw_all();
    current_desk_changed(st);
}

/// The shell hears which Desk is current, and where every Desk shows.
pub fn current_desk_changed(st: &mut State) {
    let l = st.lg.r();
    let current = l.wm.current_desk();
    let ev = l.wm.desk(current).map(|d| Event::DeskChanged {
        index: l.wm.desk_index(current).unwrap_or(0),
        id: d.id,
        name: d.name.clone(),
    });
    if let Some(ev) = ev {
        st.az.ipc.emit(&ev);
    }
    st.emit_desks();
}

/// The output a window shows on: its Desk's display's (or, before the
/// model knows displays, the one under most of it).
pub fn window_output(st: &State, id: WinId) -> Option<Output> {
    let l = st.lg.r();
    l.wm.screen_of(id)
        .and_then(|s| st.az.output_by_name(&s.output))
        .or_else(|| l.wm.get(id).and_then(|w| st.az.output_for_rect(w.rect)))
}

/// The output showing a Desk, else (before the model knows displays) the
/// one under the pointer.
pub fn desk_output(st: &State, desk: DeskId) -> Option<Output> {
    st.lg.r().wm.screen_showing(desk).and_then(|s| st.az.output_by_name(&s.output)).or_else(|| st.az.active_output())
}

/// The model takes in the outputs as they are (see
/// [`crate::wm::Model::sync_screens`]): after outputs come, go or change,
/// and when exclusive zones or the Deskbar move. While a new shell takes
/// over from the old, outputs the model knows keep the areas they had, so
/// windows don't move for the bars changing hands.
pub fn sync_screens(st: &mut State) {
    let hold = st.shell_restart_holds_layout();
    let l = st.lg.r();
    let geoms: Vec<ScreenGeom> = st
        .az
        .outputs
        .iter()
        .map(|o| {
            let known = l.wm.screens().iter().find(|s| s.output == o.name()).filter(|_| hold);
            ScreenGeom {
                output: o.name(),
                rect: st.az.output_rect(o),
                usable: known.map_or_else(|| st.az.usable_area(o), |s| s.usable),
                area: known.map_or_else(|| window_area(&st.az, o), |s| s.area),
            }
        })
        .collect();
    let primary = st.az.primary_output().map(|o| o.name());
    let same = l.wm.screens().len() == geoms.len()
        && l.wm
            .screens()
            .iter()
            .zip(&geoms)
            .all(|(s, g)| s.output == g.output && s.rect == g.rect && s.usable == g.usable && s.area == g.area);
    if same {
        return;
    }
    let removed = l.wm.screens().iter().any(|s| !geoms.iter().any(|g| g.output == s.output));
    let before = l.wm.current_desk();
    let moves = st.lg.m().wm.sync_screens(&geoms, primary.as_deref());
    desks_moved(st, before, &moves, false);
    if removed && moves.is_empty() && st.lg.r().wm.current_desk() == before {
        // Losing an unfocused display moves no remaining Desk and leaves
        // focus alone, but its Desk is no longer visible. The shell still
        // needs that change to clear its "shown elsewhere" underline.
        st.emit_desks();
    }
    apply_relaid(st);
}

/// Windows the model moved or resized to fit a display (a Desk came from
/// another one, a parked Desk woke) hear their new state.
pub fn apply_relaid(st: &mut State) {
    let ids = st.lg.m().wm.take_relaid();
    for id in ids {
        let Some(rect) = st.lg.r().wm.get(id).map(|w| w.rect) else { continue };
        let drawn = st.az.windows.get(&id).map(|m| m.window.geometry().size);
        if drawn.is_some_and(|s| (s.w, s.h) != (rect.w, rect.h)) {
            st.request_size(id, rect.w, rect.h);
        }
        st.configure(id);
        st.lg.m().window_changed(id);
        st.az.window_outputs_dirty = true;
    }
}

/// A window moved (dragged, or over IPC) mostly onto another display joins
/// its Desk there.
pub fn adopt_screen(st: &mut State, id: WinId) {
    let before = st.lg.r().wm.current_desk();
    if !st.lg.m().wm.adopt_into_screen(id) {
        return;
    }
    let family = st.lg.r().wm.family(id);
    for m in family {
        st.lg.m().window_changed(m);
        st.configure(m);
    }
    if st.lg.r().wm.current_desk() != before {
        current_desk_changed(st);
    } else {
        st.emit_desks();
    }
}

/// Some member of the group the window was in (for change events).
fn rect_owner(wm: &crate::wm::Model, id: WinId, rect: Rect) -> WinId {
    wm.windows().find(|w| w.id != id && w.rect == rect && w.group.is_some()).map(|w| w.id).unwrap_or(id)
}

impl Logic {
    /// The decoration insets for a window: frame and tab outside its
    /// content.
    pub fn insets_for(&self, id: WinId) -> crate::wm::Insets {
        match self.wm.get(id) {
            Some(w) if w.fullscreen() => crate::wm::Insets::default(),
            Some(w) => crate::decor::insets(w.ssd),
            None => crate::decor::insets(true),
        }
    }

    /// Where a window's (first) tab is now, for slides.
    pub fn decor_tab_rect(&self, labels: &mut aizome_comp_core::text::Labels, id: WinId) -> Option<Rect> {
        let l = self.decor_layout(labels, id)?;
        l.tabs.iter().find(|t| t.win == id).map(|t| t.rect)
    }
}

/// Sends change events for windows marked dirty.
pub fn flush_ipc_events(st: &mut State) {
    let dirty: Vec<WinId> = st.lg.m().dirty_windows.drain().collect();
    for id in dirty {
        if let Some(info) = st.window_info(id) {
            st.az.ipc.emit(&Event::WindowChanged { window: info });
        }
    }
}

/// Seconds since the epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn desk_infos(l: &Logic) -> Vec<DeskInfo> {
    l.wm.desks()
        .iter()
        .enumerate()
        .map(|(i, d)| DeskInfo {
            index: i as u32 + 1,
            id: d.id,
            name: d.name.clone(),
            folder: d.folder.clone(),
            windows: l.wm.desk_window_count(d.id) as u32,
            current: d.id == l.wm.current_desk(),
            output: l.wm.screen_showing(d.id).map(|s| s.output.clone()),
        })
        .collect()
}

/// Brings windows that show but ended up off every output back onto their
/// display (a sticky window onto the main one). Windows of Desks no
/// display shows keep their places, for when one does again.
pub fn rescue_windows(st: &mut State) {
    let Some(primary) = st.az.primary_output() else { return };
    let rects: Vec<Rect> = st.az.outputs.iter().map(|o| st.az.output_rect(o)).collect();
    let l = st.lg.m();
    let ids: Vec<WinId> =
        l.wm.windows().filter(|w| l.wm.is_group_active(w.id) && l.wm.is_shown(w.id)).map(|w| w.id).collect();
    for id in ids {
        let l = st.lg.r();
        let Some(w) = l.wm.get(id) else { continue };
        let on_any = rects.iter().any(|r| r.intersects(w.rect));
        if !on_any {
            let output = l.wm.screen_of(id).and_then(|s| st.az.output_by_name(&s.output)).unwrap_or(primary.clone());
            let usable = window_area(&st.az, &output);
            let insets = l.insets_for(id);
            let old = w.rect;
            let r = crate::wm::fit_into(old, usable, insets);
            st.lg.m().wm.set_rect(id, r);
            after_placement(st, id, Some(old), r);
        }
    }
}

/// When a window first maps: where it goes, on its Desk's display (a new
/// window's Desk is the current one), or a dialog on its parent's (a
/// sticky parent's Desk can be on another display, or none).
pub fn place_new_window(st: &mut State, id: WinId, size: (i32, i32), parent: Option<WinId>) {
    let l = st.lg.r();
    let Some(desk) = l.wm.get(id).map(|w| w.desk) else { return };
    let output = match parent.filter(|p| l.wm.contains(*p)) {
        Some(p) => window_output(st, p),
        None => desk_output(st, desk),
    };
    let Some(output) = output else { return };
    let usable = window_area(&st.az, &output);
    let frame = st.az.usable_area(&output);
    let insets = l.insets_for(id);
    let app_id = l.wm.get(id).map(|w| w.app_id.clone()).unwrap_or_default();
    // Where the app was last, on this display; from another display, at the
    // same place on this one in proportion.
    let remembered = (parent.is_none() && st.az.settings.windows.remember_geometry)
        .then(|| l.remembered.get(&app_id))
        .flatten()
        .filter(|r| l.wins.get(&id).is_some_and(|d| d.size_constraints.remembers(*r)))
        .and_then(|r| {
            if frame.intersects(r) {
                return Some(r);
            }
            let other = st.az.outputs.iter().map(|o| st.az.usable_area(o)).find(|u| u.intersects(r))?;
            Some(crate::wm::scale_frame(r, other, frame, insets))
        })
        .filter(|r| !l.wm.windows().any(|w| w.id != id && l.wm.is_shown(w.id) && w.rect.x == r.x && w.rect.y == r.y));
    let rect = match remembered {
        Some(r) => crate::wm::fit_into(Rect::new(r.x, r.y, size.0, size.1), usable, insets),
        None => l.wm.place(size, usable, &[], insets, parent),
    };
    let l = st.lg.m();
    l.wm.set_rect(id, rect);
    l.wm.set_frame(id, frame);
    if (rect.w, rect.h) != size {
        st.request_size(id, rect.w, rect.h);
    }
    info!(id, app_id, ?rect, output = output.name(), "placed");
}

// ---- windows coming and going ------------------------------------------------------

/// Fullscreen can target a client's requested display without changing the
/// free geometry that will be restored on exit.
fn set_fullscreen_on(st: &mut State, id: WinId, on: Option<bool>, output: Option<Output>) {
    let Some(w) = st.lg.r().wm.get(id) else { return };
    let rect = w.rect;
    let out = output
        .filter(|o| st.az.outputs.contains(o))
        .or_else(|| st.az.output_for_rect(rect))
        .map(|o| st.az.output_rect(&o))
        .unwrap_or(rect);
    if let Some(new) = st.lg.m().wm.set_fullscreen(id, on, out) {
        after_placement(st, id, Some(rect), new);
        if st.lg.r().wm.get(id).is_some_and(|w| w.fullscreen()) {
            st.focus_window(Some(id));
        }
    }
}

/// A toplevel's first configure: the usable area as bounds, and the size
/// its app last had.
pub fn first_configure(st: &mut State, toplevel: &ToplevelSurface) {
    // The core already agreed on the full output size for a request made
    // before mapping. Remembered normal-window bounds must not replace it.
    if toplevel.with_pending_state(|s| s.states.contains(xdg_toplevel::State::Fullscreen)) {
        return;
    }
    let (app_id, _) = aizome_comp_core::shell::toplevel_ids(toplevel);
    let parent = toplevel.parent().and_then(|p| st.az.window_for_surface(&p));
    // The display it will open on: its parent's, or the focused one.
    let out = match parent.filter(|p| st.lg.r().wm.contains(*p)) {
        Some(p) => window_output(st, p),
        None => desk_output(st, st.lg.r().wm.current_desk()),
    };
    let usable = out.as_ref().map(|o| window_area(&st.az, o));
    let constraints = crate::memory::SizeConstraints::of(toplevel);
    let remembered = (parent.is_none() && st.az.settings.windows.remember_geometry)
        .then(|| st.lg.r().remembered.get(&app_id))
        .flatten()
        .filter(|r| constraints.remembers(*r));
    toplevel.with_pending_state(|s| {
        let i = crate::decor::insets(s.decoration_mode != Some(DecorationMode::ClientSide));
        if let Some(u) = usable {
            let bounds = u.inset(i);
            s.bounds = Some((bounds.w, bounds.h).into());
        }
        if let Some(r) = remembered {
            let r = usable.map(|u| crate::wm::fit_into(r, u, i)).unwrap_or(r);
            s.size = Some((r.w, r.h).into());
        }
    });
}

/// A toplevel drew its first frame: it joins the desktop.
pub fn window_mapped(st: &mut State, id: WinId, info: &MapInfo) {
    let l = st.lg.m();
    l.wm.insert_as(id, &info.app_id, &info.title, info.pid, Rect::new(0, 0, info.size.0, info.size.1), info.parent);
    if let Some(w) = l.wm.get_mut(id) {
        w.ssd = info.ssd;
    }
    l.wins.insert(id, Default::default());
    if let Some(m) = st.az.windows.get(&id) {
        l.data(id).size_constraints = crate::memory::SizeConstraints::of(m.toplevel());
    }
    place_new_window(st, id, info.size, info.parent);

    // A new window of the focused app joins its group, when asked for.
    let l = st.lg.r();
    let focused = l.wm.focused();
    let same_app = focused.filter(|f| l.wm.get(*f).is_some_and(|w| w.app_id == info.app_id && w.id != id));
    if st.az.settings.windows.stack_new_windows_of_same_app
        && info.parent.is_none()
        && let Some(f) = same_app
    {
        st.stack(f, id);
    }
    if info.fullscreen {
        set_fullscreen_on(st, id, Some(true), info.fullscreen_output.clone());
    }
    st.az.window_outputs_dirty = true;
    st.focus_window(Some(id));
    st.configure(id);
    if let Some(info) = st.window_info(id) {
        st.az.ipc.emit(&Event::WindowOpened { window: info });
    }
    st.emit_desks();
    st.redraw_window(id);
}

/// A mapped window committed: decoration mode and size changes.
pub fn window_committed(st: &mut State, id: WinId, is_root: bool) {
    if !is_root {
        st.redraw_window(id);
        return;
    }
    let Some(m) = st.az.windows.get(&id) else { return };
    let window = m.window.clone();
    let toplevel = m.toplevel().clone();
    // Decoration mode changes.
    let committed = toplevel.current_state();
    let ssd = committed.decoration_mode == Some(DecorationMode::ServerSide);
    let l = st.lg.m();
    l.data(id).size_constraints = crate::memory::SizeConstraints::of(&toplevel);
    if l.wm.get(id).is_some_and(|w| w.ssd != ssd) {
        let old = l.wm.get(id).map(|w| w.rect).unwrap();
        if let Some(w) = l.wm.get_mut(id) {
            w.ssd = ssd;
        }
        l.window_changed(id);
        st.az.queue_redraw_rect(old);
    }
    // Size changes: keep the anchored edges put while resizing from the
    // left or top; only the group's showing member sets its size.
    let geo = window.geometry();
    let l = st.lg.m();
    let Some(w) = l.wm.get(id) else { return };
    let old = w.rect;
    let size = (geo.size.w.max(1), geo.size.h.max(1));
    let requested = l.wins.get(&id).and_then(|d| d.requested_size);
    let latest_committed = requested.is_none()
        || with_states(toplevel.wl_surface(), |states| {
            let data = states.data_map.get::<XdgToplevelSurfaceData>().unwrap().lock().unwrap();
            data.pending_configures().is_empty() && data.current_serial == data.configure_serial
        });
    if !latest_committed || requested.is_some_and(|wanted| committed.size.map(|s| (s.w, s.h)) != Some(wanted)) {
        // The client is still drawing an older configure. In particular,
        // do not copy that obsolete size to every member of a tab group.
        // Check serials too: a grow/shrink/grow can request the same size
        // twice, so matching dimensions alone cannot identify the response.
        // Once it commits the latest configure, accept its actual size
        // even if its own constraints kept it from matching our request.
        st.redraw_window(id);
        return;
    }
    if requested.is_some() {
        l.data(id).requested_size = None;
    }
    if (old.w, old.h) != size && l.wm.is_group_active(id) {
        let d = l.data(id);
        let mut r = Rect::new(old.x, old.y, size.0, size.1);
        if let Some(a) = d.resize_anchor {
            if a.left {
                r.x = a.start.right() - size.0;
            }
            if a.top {
                r.y = a.start.bottom() - size.1;
            }
        }
        // The client's own size, as asked or its constraints allow: where
        // it was on another display still holds.
        let members = l.wm.fit_rect(id, r);
        for other in members.into_iter().filter(|o| *o != id) {
            st.request_size(other, size.0, size.1);
            st.configure(other);
        }
        st.az.queue_redraw_rect(old);
        st.lg.m().window_changed(id);
        st.az.window_outputs_dirty = true;
    }
    st.redraw_window(id);
}

/// A window went away: the model forgets it, and focus moves on.
pub fn window_unmapped(st: &mut State, id: WinId) {
    let remember = st.az.settings.windows.remember_geometry;
    let l = st.lg.m();
    if let Some(w) = l.wm.get(id) {
        if w.placement == Placement::Free
            && w.parent.is_none()
            && remember
            && l.wins.get(&id).is_some_and(|d| d.size_constraints.remembers(w.rect))
        {
            let (app, rect) = (w.app_id.clone(), w.rect);
            l.remembered.set(&app, rect);
        }
        let r = w.rect;
        st.az.queue_redraw_rect(r);
    }
    let l = st.lg.m();
    let r = l.wm.remove(id);
    l.dirty_windows.remove(&id);
    l.anims.tabs.remove(&id);
    l.wins.remove(&id);
    st.az.ipc.emit(&Event::WindowClosed { id });
    st.emit_desks();
    crate::input::window_gone(st, id);
    crate::overlays::window_gone(st.lg.m(), id);
    st.apply_refocus(r);
    st.az.window_outputs_dirty = true;
}

/// A client asked something of its toplevel.
pub fn toplevel_request(st: &mut State, id: WinId, req: ToplevelRequest) {
    if !st.lg.r().wm.contains(id) {
        return;
    }
    match req {
        ToplevelRequest::Move { seat, serial } => crate::grabs::start_client_move(st, &seat, id, serial),
        ToplevelRequest::Resize { seat, serial, edges } => {
            crate::grabs::start_client_resize(st, &seat, id, serial, crate::grabs::edges_from_xdg(edges))
        }
        ToplevelRequest::Maximize(on) => st.set_fill(id, Some(on)),
        ToplevelRequest::Fullscreen { on, output } => set_fullscreen_on(st, id, Some(on), output),
        ToplevelRequest::Minimize => st.minimise(id),
        ToplevelRequest::WindowMenu(location) => {
            let (Some(w), Some(m)) = (st.lg.r().wm.get(id), st.az.windows.get(&id)) else { return };
            let geo = m.window.geometry();
            let p = (w.rect.x - geo.loc.x + location.x, w.rect.y - geo.loc.y + location.y);
            crate::overlays::marking::open(st, id, (p.0 as f64, p.1 as f64), true);
        }
        ToplevelRequest::AppIdChanged(app_id) => {
            let l = st.lg.m();
            if let Some(w) = l.wm.get_mut(id) {
                w.app_id = app_id;
            }
            l.window_changed(id);
            st.redraw_window(id);
        }
        ToplevelRequest::TitleChanged(title) => {
            let l = st.lg.m();
            if let Some(w) = l.wm.get_mut(id) {
                w.title = title;
            }
            l.window_changed(id);
            st.redraw_window(id);
        }
        ToplevelRequest::ParentChanged(parent) => {
            let l = st.lg.m();
            if let Some(w) = l.wm.get_mut(id) {
                w.parent = parent;
            }
            l.window_changed(id);
        }
        ToplevelRequest::Activate { valid } => {
            let l = st.lg.m();
            if l.wm.focused() == Some(id) {
                // Already focused (Chromium asks for its new window with a
                // token of its own): nothing to grant, nothing to flag.
            } else if valid {
                st.activate_window(id);
            } else {
                // No stealing focus: the shell shows it as wanting attention.
                if let Some(w) = l.wm.get_mut(id) {
                    w.urgent = true;
                }
                l.window_changed(id);
            }
        }
        ToplevelRequest::Decorations => st.configure(id),
    }
}

/// Refit each window/group once, on the output that owns it: the windows
/// of the Desk it shows and the sticky ones on it (other Desks' windows
/// keep their places until a display shows them). Updating the rectangle
/// directly preserves its placement and every tab's restore rect.
pub fn refit_placed_windows(st: &mut State, output: &Output) {
    let usable = window_area(&st.az, output);
    let out_rect = st.az.output_rect(output);
    let l = st.lg.r();
    let name = output.name();
    let ids: Vec<WinId> =
        l.wm.windows()
            .filter(|w| l.wm.is_group_active(w.id))
            .filter(|w| match l.wm.screens().is_empty() {
                true => st.az.output_for_rect(w.rect).as_ref() == Some(output),
                false => l.wm.screen_of(w.id).is_some_and(|s| s.output == name),
            })
            .map(|w| w.id)
            .collect();
    for id in ids {
        let l = st.lg.m();
        let Some(w) = l.wm.get(id) else { continue };
        let insets = l.insets_for(id);
        let placement = w.placement;
        let area = match placement {
            Placement::SplitLeft => Rect::new(usable.x, usable.y, usable.w / 2, usable.h),
            Placement::SplitRight => Rect::new(usable.x + usable.w / 2, usable.y, usable.w - usable.w / 2, usable.h),
            _ => usable,
        };
        let old = w.rect;
        let r = match placement {
            Placement::Free => crate::wm::fit_into(old, usable, insets),
            Placement::Fullscreen => out_rect,
            _ => area.inset(insets),
        };
        if r != old {
            l.wm.fit_rect(id, r);
            after_placement(st, id, Some(old), r);
        } else {
            // Bounds may change even when the window itself still fits.
            for m in l.wm.group_members(id) {
                st.configure(m);
            }
        }
    }
}

/// The logic starts (at login, or after a reload): it takes in the
/// windows the core has. Windows the saved model knows keep their places,
/// groups and Desks; new ones (mapped while no logic ran) are placed as
/// usual; ones that went away are forgotten.
pub fn adopt_windows(st: &mut State) {
    let mut ids: Vec<WinId> = st.az.windows.keys().copied().collect();
    ids.sort();
    let l = st.lg.m();
    let gone: Vec<WinId> = l.wm.windows().map(|w| w.id).filter(|id| !st.az.windows.contains_key(id)).collect();
    for id in gone {
        l.wm.remove(id);
    }
    for id in ids {
        let Some(m) = st.az.windows.get(&id) else { continue };
        let toplevel = m.toplevel().clone();
        let has_buffer = with_renderer_surface_state(toplevel.wl_surface(), |s| s.buffer().is_some()).unwrap_or(false);
        if !has_buffer {
            continue;
        }
        let ssd = toplevel.current_state().decoration_mode == Some(DecorationMode::ServerSide);
        let (app_id, title) = aizome_comp_core::shell::toplevel_ids(&toplevel);
        let view = m.view.clone();
        let geo = m.window.geometry();
        let pid = st
            .az
            .dh
            .get_client(toplevel.wl_surface().id())
            .ok()
            .and_then(|c| c.get_credentials(&st.az.dh).ok())
            .map(|c| c.pid);
        let parent = toplevel.parent().and_then(|p| st.az.window_for_surface(&p));
        let l = st.lg.m();
        l.data(id).size_constraints = crate::memory::SizeConstraints::of(&toplevel);
        if let Some(w) = l.wm.get_mut(id) {
            // Known: what it is now (titles change).
            w.title = title;
            w.app_id = app_id;
            w.ssd = ssd;
            continue;
        }
        let size = (geo.size.w.max(1), geo.size.h.max(1));
        let rect = if view.rect.is_empty() { Rect::new(0, 0, size.0, size.1) } else { view.rect };
        l.wm.insert_as(id, &app_id, &title, pid, rect, parent);
        if let Some(w) = l.wm.get_mut(id) {
            w.ssd = ssd;
        }
        if view.rect.is_empty() {
            place_new_window(st, id, size, parent);
        }
        if st.az.view.focused == Some(id) {
            st.lg.m().wm.focus(Some(id));
        }
    }
    st.az.window_outputs_dirty = true;
    st.az.queue_redraw_all();
}
