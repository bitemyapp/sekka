//! Surfaces and shells: commits, xdg toplevels and popups, decorations,
//! layer surfaces and the session lock.
//!
//! The commit handling and popup grab follow anvil's (smithay, MIT
//! licence: Copyright (c) 2017 Victor Berger and Victor Brekenfeld).

use std::time::Instant;

use aizome_ipc::Event;
use smithay::backend::renderer::utils::{on_commit_buffer_handler, with_renderer_surface_state};
use smithay::desktop::{
    LayerSurface, PopupKeyboardGrab, PopupKind, PopupPointerGrab, PopupUngrabStrategy, Window, WindowSurfaceType,
    find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output,
};
use smithay::input::Seat;
use smithay::input::pointer::Focus as PointerFocusMode;
use smithay::output::Output;
use smithay::reexports::calloop::Interest;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Resource};
use smithay::utils::{Logical, Point, Rectangle, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes, add_blocker,
    add_pre_commit_hook, get_parent, get_role, is_sync_subsurface, with_states,
};
use smithay::wayland::dmabuf::get_dmabuf;
use smithay::wayland::input_method::InputMethodSeat;
use smithay::wayland::session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker};
use smithay::wayland::shell::wlr_layer::{
    LAYER_SURFACE_ROLE, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
    WlrLayerShellState,
};
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState, XdgToplevelSurfaceData,
};
use smithay::{
    delegate_compositor, delegate_layer_shell, delegate_session_lock, delegate_xdg_decoration, delegate_xdg_shell,
};
use tracing::{debug, info, warn};

use crate::WinId;
use crate::focus::Focus;
use crate::logic::{MapInfo, ToplevelRequest};
use crate::state::{ClientState, LockState, Mapped, State, Unmapped};
use crate::view::WinView;

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.az.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().expect("client data").compositor_state
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        // A commit waiting on a fifo barrier or a time gets a frame or a
        // timer to let it through. This hook runs before smithay's own for
        // those (added with the surface's fifo or timer object, later), so
        // what the commit asks is still there to read.
        add_pre_commit_hook::<Self, _>(surface, |state, _dh, surface| {
            crate::pacing::commit_requested(state, surface);
        });
        // Wait for a dmabuf's fence before applying the commit, so frames
        // never sample unfinished buffers.
        add_pre_commit_hook::<Self, _>(surface, |state, _dh, surface| {
            let dmabuf = with_states(surface, |data| {
                data.cached_state.get::<SurfaceAttributes>().pending().buffer.as_ref().and_then(|a| match a {
                    BufferAssignment::NewBuffer(b) => get_dmabuf(b).cloned().ok(),
                    _ => None,
                })
            });
            if let Some(dmabuf) = dmabuf
                && let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ)
                && let Some(client) = surface.client()
            {
                let res = state.az.lh.insert_source(source, move |_, _, state| {
                    let dh = state.az.dh.clone();
                    state.client_compositor_state(&client).blocker_cleared(state, &dh);
                    Ok(())
                });
                if res.is_ok() {
                    add_blocker(surface, blocker);
                }
            }
        });
    }

    fn destroyed(&mut self, surface: &WlSurface) {
        // Also when its client goes: an idle inhibitor on it stops counting.
        self.drop_idle_inhibitor(surface);
    }

    fn commit(&mut self, surface: &WlSurface) {
        if aizome_trace::enabled() {
            let pid = self
                .az
                .dh
                .get_client(surface.id())
                .ok()
                .and_then(|c| c.get_credentials(&self.az.dh).ok())
                .map(|c| c.pid as u64)
                .unwrap_or(0);
            aizome_trace::instant(aizome_trace::Phase::SurfaceCommit, surface.id().protocol_id() as u64, pid);
        }
        let fresh_shm = crate::backend::fresh_shm_buffers(surface);
        // A layer surface taking its buffer down leaves its last frame for
        // the logic to move away (before the buffer is let go).
        self.ghost_if_unmapping(surface);
        on_commit_buffer_handler::<Self>(surface);
        self.backend.early_import(surface);
        if !fresh_shm.is_empty() {
            self.backend.release_shm_after_import(surface, &fresh_shm);
        }

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(i) =
                self.az.unmapped.iter().position(|u| u.window.toplevel().is_some_and(|t| t.wl_surface() == &root))
            {
                if &root == surface {
                    self.commit_unmapped(i);
                }
            } else if let Some(id) = self.az.window_for_surface(&root) {
                self.commit_mapped(id, &root == surface);
            }
        }

        self.az.popups.commit(surface);
        if let Some(popup) = self.az.popups.find_popup(surface) {
            if let PopupKind::Xdg(ref p) = popup
                && !p.is_initial_configure_sent()
            {
                let _ = p.send_configure();
            }
            if let Some(r) = self.az.popup_bbox(&popup) {
                self.az.queue_redraw_rect(crate::state::to_rect(r));
            }
            if let Ok(root) = find_popup_root_surface(&popup) {
                self.redraw_for_surface(&root);
            }
        }

        self.commit_layer(surface);
        self.commit_misc(surface);
    }
}

impl State {
    fn commit_unmapped(&mut self, i: usize) {
        let window = self.az.unmapped[i].window.clone();
        let toplevel = window.toplevel().expect("toplevel").clone();
        if !toplevel.is_initial_configure_sent() {
            // The first configure: the logic gives the bounds and the size
            // the app last had (server-side decorations are the default).
            self.lg_call((), |vt, st| (vt.first_configure)(st, &toplevel));
            toplevel.send_configure();
            return;
        }
        let has_buffer = with_renderer_surface_state(toplevel.wl_surface(), |s| s.buffer().is_some()).unwrap_or(false);
        if !has_buffer {
            return;
        }
        let u = self.az.unmapped.remove(i);
        u.window.on_commit();
        self.map_window(u.window);
    }

    /// A toplevel drew its first frame: it joins the desktop.
    fn map_window(&mut self, window: Window) {
        let toplevel = window.toplevel().expect("toplevel").clone();
        let surface = toplevel.wl_surface().clone();
        let (app_id, title) = toplevel_ids(&toplevel);
        let parent = toplevel.parent().and_then(|p| self.az.window_for_surface(&p));
        let pid =
            self.az.dh.get_client(surface.id()).ok().and_then(|c| c.get_credentials(&self.az.dh).ok()).map(|c| c.pid);
        let ssd = toplevel.current_state().decoration_mode == Some(DecorationMode::ServerSide);
        let geo = window.geometry();
        let size = (geo.size.w.max(1), geo.size.h.max(1));
        let id = self.az.next_window_id();
        self.az.windows.insert(id, Mapped { id, window: window.clone(), view: WinView::default() });
        self.az.by_surface.insert(surface.clone(), id);
        info!(id, app_id, title, ssd, "window opened");
        let committed = toplevel.current_state();
        let fullscreen = committed.states.contains(xdg_toplevel::State::Fullscreen);
        let fullscreen_output = committed.fullscreen_output.as_ref().and_then(Output::from_resource);
        let info = MapInfo { app_id, title, pid, parent, ssd, size, fullscreen, fullscreen_output };
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.window_mapped)(st, id, &info));
        } else {
            crate::fallback::window_mapped(self, id, &info);
        }
        self.az.window_outputs_dirty = true;
        crate::protocols::foreign_toplevel::window_opened(self, id);
    }

    fn commit_mapped(&mut self, id: WinId, is_root: bool) {
        let Some(m) = self.az.windows.get(&id) else { return };
        m.window.on_commit();
        if !is_root {
            if self.lg.healthy() {
                self.lg_call((), |vt, st| (vt.window_committed)(st, id, false));
            } else {
                self.redraw_window_now(id);
            }
            return;
        }
        let window = m.window.clone();
        let toplevel = m.toplevel().clone();
        let has_buffer = with_renderer_surface_state(toplevel.wl_surface(), |s| s.buffer().is_some()).unwrap_or(false);
        if !has_buffer {
            // Unmapped: back to waiting for a first frame.
            self.unmap_window(id);
            self.az.unmapped.push(Unmapped { window, wants_csd: false });
            return;
        }
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.window_committed)(st, id, true));
        } else {
            crate::fallback::window_committed(self, id);
        }
    }

    /// Queues redraws where a window is, as the logic last said.
    pub fn redraw_window_now(&mut self, id: WinId) {
        if let Some(r) = self.az.window_rect(id) {
            self.az.queue_redraw_rect(r);
        }
    }

    /// A window went away (destroyed or unmapped).
    pub fn unmap_window(&mut self, id: WinId) {
        let Some(m) = self.az.windows.remove(&id) else { return };
        self.az.by_surface.retain(|_, w| *w != id);
        self.az.queue_redraw_rect(m.view.rect);
        for o in self.az.outputs.clone() {
            use smithay::desktop::space::SpaceElement;
            m.window.output_leave(&o);
        }
        crate::protocols::foreign_toplevel::window_closed(self, id);
        info!(id, "window closed");
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.window_unmapped)(st, id));
        } else {
            crate::fallback::window_unmapped(self, id);
        }
        self.az.window_outputs_dirty = true;
    }

    /// Queues redraws for whatever a surface belongs to.
    fn redraw_for_surface(&mut self, root: &WlSurface) {
        if let Some(id) = self.az.window_for_surface(root) {
            self.redraw_window_now(id);
            return;
        }
        let outs: Vec<Output> = self
            .az
            .outputs
            .iter()
            .filter(|o| layer_map_for_output(o).layer_for_surface(root, WindowSurfaceType::ALL).is_some())
            .cloned()
            .collect();
        for o in outs {
            self.az.queue_redraw(&o);
        }
    }

    fn commit_layer(&mut self, surface: &WlSurface) {
        let Some(output) = self
            .az
            .outputs
            .iter()
            .find(|o| layer_map_for_output(o).layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).is_some())
            .cloned()
        else {
            return;
        };
        let initial_sent = with_states(surface, |s| {
            s.data_map.get::<LayerSurfaceData>().map(|d| d.lock().unwrap().initial_configure_sent).unwrap_or(true)
        });
        // A restarted shell's surface, drawn, replaces the old shell's.
        if self.az.children.handover.is_some()
            && with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false)
        {
            self.shell_handover_layer(&output, surface);
        }
        let deskbar_before = self.az.out.get(&output).and_then(|o| o.deskbar_geometry);
        let mut map = layer_map_for_output(&output);
        let before = map.non_exclusive_zone();
        map.arrange();
        let after = map.non_exclusive_zone();
        if !initial_sent && let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) {
            layer.layer_surface().send_configure();
        }
        drop(map);
        let deskbar_after = self.az.deskbar_rect(&output);
        if let Some(o) = self.az.out.get_mut(&output) {
            o.deskbar_geometry = deskbar_after;
        }
        if before != after || deskbar_before != deskbar_after {
            // The Deskbar reserves no exclusive zone, but its arrival or
            // size change also changes where windows can fit.
            self.az.window_outputs_dirty = true;
            crate::outputs::outputs_changed(&mut self.az);
            if !self.shell_restart_holds_layout() {
                self.lg_call((), |vt, st| (vt.usable_area_changed)(st, &output));
            }
        }
        self.update_keyboard_focus();
        self.az.queue_redraw(&output);
    }

    fn commit_misc(&mut self, surface: &WlSurface) {
        // The pointer's own surface, a drag icon or a lock surface.
        use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};
        if let CursorImageStatus::Surface(s) = &self.az.cursor_status
            && s == surface
        {
            with_states(surface, |states| {
                if let Some(attrs) = states.data_map.get::<CursorImageSurfaceData>() {
                    let delta = states.cached_state.get::<SurfaceAttributes>().current().buffer_delta.take();
                    if let Some(d) = delta {
                        attrs.lock().unwrap().hotspot -= d;
                    }
                }
            });
            if let Some(o) = self.az.active_output() {
                self.az.queue_redraw(&o);
            }
        }
        if self.az.dnd_icon.as_ref() == Some(surface) {
            // The icon moves by its offsets (wl_surface.offset, or attach's x
            // and y before version 5) relative to the pointer.
            let delta = with_states(surface, |states| {
                states.cached_state.get::<SurfaceAttributes>().current().buffer_delta.take()
            });
            if let Some(d) = delta {
                self.az.dnd_offset += d;
            }
            if let Some(o) = self.az.active_output() {
                self.az.queue_redraw(&o);
            }
        }
        let lock_out: Option<Output> = self
            .az
            .out
            .iter()
            .find(|(_, o)| o.lock_surface.as_ref().is_some_and(|l| l.wl_surface() == surface))
            .map(|(o, _)| o.clone());
        if let Some(o) = lock_out {
            self.az.queue_redraw(&o);
        }
    }

    /// Fits a popup on its root's output, by its positioner's constraint
    /// adjustments: popups may go anywhere on the output, not only over
    /// their parent.
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else { return };
        let Some((origin, Some(output), _)) = self.az.popup_root(&root) else { return };
        let mut target: Rectangle<i32, Logical> = self.az.output_geometry(&output);
        target.loc -= origin;
        target.loc -= get_popup_toplevel_coords(&kind);
        popup.with_pending_state(|s| {
            s.geometry = s.positioner.get_unconstrained_geometry(target);
        });
    }
}

impl State {
    /// Hands a client's request about its toplevel to the logic (without
    /// one, the recovery path does what it can).
    pub fn toplevel_request(&mut self, id: WinId, req: ToplevelRequest) {
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.toplevel_request)(st, id, req));
        } else {
            crate::fallback::toplevel_request(self, id, req);
        }
    }
}

pub fn toplevel_ids(t: &ToplevelSurface) -> (String, String) {
    with_states(t.wl_surface(), |s| {
        let d = s.data_map.get::<XdgToplevelSurfaceData>().unwrap().lock().unwrap();
        (d.app_id.clone().unwrap_or_default(), d.title.clone().unwrap_or_default())
    })
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.az.xdg_shell
    }

    fn client_pong(&mut self, client: smithay::wayland::shell::xdg::ShellClient) {
        crate::recovery::ping::pong(&client);
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        self.az.unmapped.push(Unmapped { window, wants_csd: true });
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        self.az.popup_order.push(PopupKind::from(surface.clone()));
        if let Err(e) = self.az.popups.track_popup(PopupKind::from(surface)) {
            warn!("tracking a popup: {e}");
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|s| {
            s.geometry = positioner.get_geometry();
            s.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: WlSeat, serial: Serial) {
        let Some(id) = self.az.window_for_surface(surface.wl_surface()) else { return };
        let Some(seat) = Seat::<State>::from_resource(&seat) else { return };
        self.toplevel_request(id, ToplevelRequest::Move { seat, serial });
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let Some(id) = self.az.window_for_surface(surface.wl_surface()) else { return };
        let Some(seat) = Seat::<State>::from_resource(&seat) else { return };
        self.toplevel_request(id, ToplevelRequest::Resize { seat, serial, edges });
    }

    fn grab(&mut self, surface: PopupSurface, seat: WlSeat, serial: Serial) {
        let Some(seat) = Seat::<State>::from_resource(&seat) else { return };
        let kind = PopupKind::Xdg(surface.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            debug!(target: "aizome::focus", "popup grab without a parent");
            surface.send_popup_done();
            return;
        };
        // The lock and exclusive layer surfaces keep the keyboard: other
        // menus don't open over them.
        if let Some(t) = self.az.exclusive_keyboard_target()
            && t.as_ref() != Some(&root)
        {
            debug!(target: "aizome::focus", "popup grab refused: another surface has the keyboard exclusively");
            surface.send_popup_done();
            return;
        }
        // A grabbing popup of a toplevel or layer surface starts a new
        // chain of menus: one already open closes (rather than the client
        // getting an error for not nesting it), and one smithay has closed
        // lets go of the keyboard.
        if surface.get_parent_surface().as_ref() == Some(&root) {
            self.dismiss_popup_grab();
        }
        let mut grab = match self.az.popups.grab_popup(Focus(root.clone()), kind, &seat, serial) {
            Ok(g) => g,
            Err(e) => {
                debug!(target: "aizome::focus", "popup grab refused: {e:?}");
                return;
            }
        };
        debug!(target: "aizome::focus", root = ?root.id(), "popup grab");
        // Refused or not, decided before anything changes: a refused grab
        // leaves the keyboard and the pointer as they were.
        let keyboard = seat.get_keyboard();
        let pointer = seat.get_pointer();
        let chain = [serial, grab.previous_serial().unwrap_or(serial)];
        let keyboard_taken = keyboard.as_ref().is_some_and(|k| k.is_grabbed() && !chain.iter().any(|&s| k.has_grab(s)));
        // An input method holds the keyboard while a text field has it
        // (ibus does). A menu of the client typed into or pointed at (the
        // field's context menu), or of a layer surface (the shell's, for a
        // binding: it could take the keys outright anyway), opens over that
        // grab and leaves it be; the menu takes the keyboard once the input
        // method lets go (`check_popup_grab`).
        let over_input_method = keyboard_taken && seat.input_method().keyboard_grabbed() && {
            let same_client = |f: Option<Focus>| f.is_some_and(|f| f.0.id().same_client_as(&root.id()));
            get_role(&root) == Some(LAYER_SURFACE_ROLE)
                || same_client(keyboard.as_ref().and_then(|k| k.current_focus()))
                || same_client(pointer.as_ref().and_then(|p| p.current_focus()))
        };
        if keyboard_taken && !over_input_method {
            debug!(target: "aizome::focus", "popup grab refused: another grab has the keyboard");
            grab.ungrab(PopupUngrabStrategy::All);
            return;
        }
        if pointer.as_ref().is_some_and(|p| p.is_grabbed() && !chain.iter().any(|&s| p.has_grab(s))) {
            debug!(target: "aizome::focus", "popup grab refused: another grab has the pointer");
            grab.ungrab(PopupUngrabStrategy::All);
            return;
        }
        if let Some(keyboard) = keyboard {
            keyboard.set_focus(self, grab.current_grab(), serial);
            if !over_input_method {
                keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
            }
        }
        if let Some(pointer) = pointer {
            pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, PointerFocusMode::Keep);
        }
        self.az.popup_grab = Some(crate::focus::PopupGrabbed { grab, root, dismissed: false });
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        match self.az.window_for_surface(surface.wl_surface()) {
            Some(id) => self.toplevel_request(id, ToplevelRequest::Maximize(true)),
            None => {
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
            }
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            self.toplevel_request(id, ToplevelRequest::Maximize(false));
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, output: Option<WlOutput>) {
        let target = output.as_ref().and_then(Output::from_resource).filter(|o| self.az.outputs.contains(o));
        match self.az.window_for_surface(surface.wl_surface()) {
            Some(id) => self.toplevel_request(id, ToplevelRequest::Fullscreen { on: true, output: target }),
            None => {
                // Games commonly request fullscreen before their first commit.
                // Agree on its state and size in the first configure, then carry
                // that state into the window manager when its buffer arrives.
                // Recovery has no fullscreen layout policy, just as it declines
                // requests for already mapped windows. Don't promise a state
                // that fallback placement cannot provide.
                if self.lg.healthy() {
                    let target = target.or_else(|| self.az.active_output()).or_else(|| self.az.primary_output());
                    let size = target.map(|o| self.az.output_geometry(&o).size);
                    surface.with_pending_state(|s| {
                        s.states.set(xdg_toplevel::State::Fullscreen);
                        s.fullscreen_output = output;
                        s.size = size;
                        s.bounds = size;
                    });
                }
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
            }
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            self.toplevel_request(id, ToplevelRequest::Fullscreen { on: false, output: None });
        } else {
            surface.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Fullscreen);
                s.fullscreen_output = None;
                s.size = None;
            });
            if surface.is_initial_configure_sent() {
                self.lg_call((), |vt, st| (vt.first_configure)(st, &surface));
                surface.send_configure();
            }
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            self.toplevel_request(id, ToplevelRequest::Minimize);
        }
    }

    fn show_window_menu(
        &mut self,
        surface: ToplevelSurface,
        _seat: WlSeat,
        _serial: Serial,
        location: Point<i32, Logical>,
    ) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            self.toplevel_request(id, ToplevelRequest::WindowMenu(location));
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            self.unmap_window(id);
            crate::recovery::windows_changed(self);
        }
        self.az.unmapped.retain(|u| u.window.toplevel().is_some_and(|t| t != &surface));
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {
        // Popups show anywhere on their output.
        self.az.queue_redraw_all();
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            let (app_id, _) = toplevel_ids(&surface);
            self.toplevel_request(id, ToplevelRequest::AppIdChanged(app_id));
            self.redraw_window_now(id);
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            let (_, title) = toplevel_ids(&surface);
            self.toplevel_request(id, ToplevelRequest::TitleChanged(title));
            self.redraw_window_now(id);
        }
    }

    fn parent_changed(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.az.window_for_surface(surface.wl_surface()) {
            let parent = surface.parent().and_then(|p| self.az.window_for_surface(&p));
            self.toplevel_request(id, ToplevelRequest::ParentChanged(parent));
        }
    }
}

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // Aizome prefers to draw the frame.
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
        if let Some(u) = self.az.unmapped.iter_mut().find(|u| u.window.toplevel() == Some(&toplevel)) {
            u.wants_csd = false;
        }
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecorationMode) {
        // Clients that insist draw their own; they get the tab only.
        toplevel.with_pending_state(|s| s.decoration_mode = Some(mode));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
}

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.az.layer_shell
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = output.as_ref().and_then(Output::from_resource).or_else(|| self.az.active_output());
        let Some(output) = output else {
            surface.send_close();
            return;
        };
        debug!(namespace, output = output.name(), "layer surface");
        // A client that destroys the surface with its role (in one go, as
        // Sidestep does) takes its buffer away before the role's end is
        // heard: the last frame is kept as the surface goes (this hook runs
        // before the one that lets the buffer go, added at its first commit).
        smithay::wayland::compositor::add_destruction_hook(surface.wl_surface(), |st: &mut State, s: &WlSurface| {
            st.ghost_on_destroy(s)
        });
        let mut map = layer_map_for_output(&output);
        if let Err(e) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            warn!("mapping a layer surface: {e}");
        }
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let found = self.az.outputs.iter().find_map(|o| {
            let map = layer_map_for_output(o);
            let layer = map.layers().find(|l| l.layer_surface() == &surface).cloned();
            layer.map(|l| (o.clone(), l))
        });
        if let Some((output, layer)) = found {
            // Its last frame, for the logic to move away (not while a
            // restarted shell takes over: its own surface is already up).
            if !self.shell_restart_holds_layout() {
                self.keep_ghost(&output, &layer);
            }
            let deskbar_before = self.az.out.get(&output).and_then(|o| o.deskbar_geometry);
            let mut map = layer_map_for_output(&output);
            let before = map.non_exclusive_zone();
            map.unmap_layer(&layer);
            let after = map.non_exclusive_zone();
            drop(map);
            if self.az.layer_focus.as_ref() == Some(surface.wl_surface()) {
                self.az.layer_focus = None;
            }
            let deskbar_after = self.az.deskbar_rect(&output);
            if let Some(o) = self.az.out.get_mut(&output) {
                o.deskbar_geometry = deskbar_after;
            }
            if before != after || deskbar_before != deskbar_after {
                crate::outputs::outputs_changed(&mut self.az);
                if !self.shell_restart_holds_layout() {
                    self.lg_call((), |vt, st| (vt.usable_area_changed)(st, &output));
                }
            }
            self.update_keyboard_focus();
            self.az.queue_redraw(&output);
        }
    }
}

impl SessionLockHandler for State {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.az.session_lock
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        aizome_trace::stop_all();
        info!("locking the session");
        self.az.lock = LockState::Locking(confirmation, Instant::now());
        self.az.queue_redraw_all();
        self.update_keyboard_focus();
        self.az.ipc.emit(&Event::LockStateChanged { locked: true });
    }

    fn unlock(&mut self) {
        info!("unlocking the session");
        self.az.lock = LockState::Unlocked;
        for o in self.az.out.values_mut() {
            o.lock_surface = None;
        }
        self.az.queue_redraw_all();
        self.update_keyboard_focus();
        self.az.ipc.emit(&Event::LockStateChanged { locked: false });
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else { return };
        let geo = self.az.output_geometry(&output);
        surface.with_pending_state(|s| {
            s.size = Some((geo.size.w as u32, geo.size.h as u32).into());
        });
        surface.send_configure();
        if let Some(o) = self.az.out.get_mut(&output) {
            o.lock_surface = Some(surface);
        }
        self.update_keyboard_focus();
        self.az.queue_redraw(&output);
    }
}

impl State {
    /// Confirms a pending lock once every output has drawn a frame since.
    pub fn check_lock(&mut self) {
        let LockState::Locking(_, since) = &self.az.lock else { return };
        let since = *since;
        let drawn = self.az.out.values().all(|o| !o.powered || o.last_frame.is_some_and(|t| t > since));
        if drawn {
            if let LockState::Locking(locker, _) = std::mem::take(&mut self.az.lock) {
                locker.lock();
            }
            self.az.lock = LockState::Locked;
        }
    }
}

delegate_compositor!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_layer_shell!(State);
delegate_session_lock!(State);
