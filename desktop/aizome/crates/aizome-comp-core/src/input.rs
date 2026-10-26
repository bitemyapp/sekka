//! Input: pointer, keyboard, touch, gestures and the lid switch, from
//! libinput, winit or IPC (synthetic events for tests), all through the
//! same paths.
//!
//! The core turns each event into a call the logic understands and keeps
//! a few keys for itself, which work whatever the logic does: switching
//! virtual terminals, reloading Aizome (⌃❖R) and the recovery overlay
//! (⌃⌥Delete). Everything else — bindings, the command-key translation,
//! decorations, overlays, focus policy — is the logic's. Without a logic
//! the recovery path (`fallback.rs`) clicks windows to focus and passes
//! keys on.

use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Device as _, DeviceCapability, Event as _, GestureBeginEvent,
    GestureEndEvent, GesturePinchUpdateEvent as _, GestureSwipeUpdateEvent as _, InputBackend, InputEvent, KeyState,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent, Switch, SwitchState,
    SwitchToggleEvent as _, TouchEvent,
};
use smithay::desktop::{WindowSurfaceType, layer_map_for_output};
use smithay::input::keyboard::{Keycode, Keysym, KeysymHandle, ModifiersState, keysyms};
use smithay::input::pointer::RelativeMotionEvent;
use smithay::input::touch::{DownEvent, UpEvent};
use smithay::output::Output;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};
use smithay::wayland::pointer_constraints::{PointerConstraint, with_pointer_constraint};
use smithay::wayland::shell::wlr_layer::Layer;

use crate::WinId;
use crate::focus::Focus;
use crate::logic::{AxisEvent, KeyDecision, KeyEvent};
use crate::state::State;

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// Whose surface is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Window(WinId),
    Layer,
    Lock,
}

/// A client surface under a point: the surface, its origin in global
/// points, and whose it is.
pub type SurfaceAt = (Focus, Point<f64, Logical>, Owner);

/// What the core does with a key of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreAction {
    ChangeVt(i32),
    /// Reload what changed (⌃❖R).
    Reload,
}

/// How a key was decided.
pub(crate) enum Decision {
    Forward,
    Translate(ModifiersState),
    Swallow,
    Logic,
    Core(CoreAction),
}

impl State {
    /// Feeds an event from a backend.
    pub fn process_input<B: InputBackend>(&mut self, event: InputEvent<B>) {
        match event {
            InputEvent::Keyboard { event } => {
                let pressed = event.state() == KeyState::Pressed;
                self.keyboard_key(event.key_code().raw().saturating_sub(8), pressed, event.time_msec());
            }
            InputEvent::PointerMotion { event } => {
                self.pointer_motion_relative(event.delta(), event.delta_unaccel(), event.time(), event.time_msec());
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let Some(output) = self.az.outputs.first().cloned() else { return };
                let geo = self.az.output_geometry(&output);
                let pos = event.position_transformed(geo.size) + geo.loc.to_f64();
                self.pointer_motion_absolute(pos, event.time_msec());
            }
            InputEvent::PointerButton { event } => {
                let pressed = event.state() == ButtonState::Pressed;
                self.pointer_button(event.button_code(), pressed, event.time_msec());
            }
            InputEvent::PointerAxis { event } => {
                let source = event.source();
                let amount = |axis| event.amount(axis);
                let axis = AxisEvent {
                    source,
                    time: event.time_msec(),
                    h: amount(Axis::Horizontal)
                        .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.0),
                    v: amount(Axis::Vertical)
                        .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0),
                    h120: event.amount_v120(Axis::Horizontal),
                    v120: event.amount_v120(Axis::Vertical),
                    h_stop: source == AxisSource::Finger && amount(Axis::Horizontal) == Some(0.0),
                    v_stop: source == AxisSource::Finger && amount(Axis::Vertical) == Some(0.0),
                };
                self.pointer_axis(&axis);
            }
            InputEvent::GestureSwipeBegin { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_swipe_begin(
                    self,
                    &smithay::input::pointer::GestureSwipeBeginEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                        fingers: event.fingers(),
                    },
                );
            }
            InputEvent::GestureSwipeUpdate { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_swipe_update(
                    self,
                    &smithay::input::pointer::GestureSwipeUpdateEvent { time: event.time_msec(), delta: event.delta() },
                );
            }
            InputEvent::GestureSwipeEnd { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_swipe_end(
                    self,
                    &smithay::input::pointer::GestureSwipeEndEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                        cancelled: event.cancelled(),
                    },
                );
            }
            InputEvent::GesturePinchBegin { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_pinch_begin(
                    self,
                    &smithay::input::pointer::GesturePinchBeginEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                        fingers: event.fingers(),
                    },
                );
            }
            InputEvent::GesturePinchUpdate { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_pinch_update(
                    self,
                    &smithay::input::pointer::GesturePinchUpdateEvent {
                        time: event.time_msec(),
                        delta: event.delta(),
                        scale: event.scale(),
                        rotation: event.rotation(),
                    },
                );
            }
            InputEvent::GesturePinchEnd { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_pinch_end(
                    self,
                    &smithay::input::pointer::GesturePinchEndEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                        cancelled: event.cancelled(),
                    },
                );
            }
            InputEvent::GestureHoldBegin { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_hold_begin(
                    self,
                    &smithay::input::pointer::GestureHoldBeginEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                        fingers: event.fingers(),
                    },
                );
            }
            InputEvent::GestureHoldEnd { event } => {
                let Some(pointer) = self.az.seat.get_pointer() else { return };
                pointer.gesture_hold_end(
                    self,
                    &smithay::input::pointer::GestureHoldEndEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                        cancelled: event.cancelled(),
                    },
                );
            }
            InputEvent::TouchDown { event } => {
                let Some(output) = self.az.outputs.first().cloned() else { return };
                let geo = self.az.output_geometry(&output);
                let pos = event.position_transformed(geo.size) + geo.loc.to_f64();
                self.touch_down(event.slot(), pos, event.time_msec());
            }
            InputEvent::TouchUp { event } => {
                let Some(touch) = self.az.seat.get_touch() else { return };
                touch.up(
                    self,
                    &UpEvent { slot: event.slot(), serial: SERIAL_COUNTER.next_serial(), time: event.time_msec() },
                );
            }
            InputEvent::TouchMotion { event } => {
                let Some(touch) = self.az.seat.get_touch() else { return };
                let Some(output) = self.az.outputs.first().cloned() else { return };
                let geo = self.az.output_geometry(&output);
                let pos = event.position_transformed(geo.size) + geo.loc.to_f64();
                let under = self.surface_at(pos).map(|(f, o, _)| (f, o));
                touch.motion(
                    self,
                    under,
                    &smithay::input::touch::MotionEvent { slot: event.slot(), location: pos, time: event.time_msec() },
                );
            }
            InputEvent::TouchFrame { .. } => {
                if let Some(touch) = self.az.seat.get_touch() {
                    touch.frame(self);
                }
            }
            InputEvent::TouchCancel { .. } => {
                if let Some(touch) = self.az.seat.get_touch() {
                    touch.cancel(self);
                }
            }
            InputEvent::SwitchToggle { event } => {
                if event.switch() == Some(Switch::Lid) {
                    self.lid_switched(event.state() == SwitchState::On);
                }
            }
            // libinput says nothing of a lid opened while its device was
            // gone (another VT had the seat), so a lid whose device goes
            // counts as open until it says otherwise.
            InputEvent::DeviceRemoved { device } if device.has_capability(DeviceCapability::Switch) => {
                self.lid_switched(false);
            }
            InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. } => {}
            _ => {}
        }
    }

    fn touch_down(&mut self, slot: smithay::backend::input::TouchSlot, pos: Point<f64, Logical>, time: u32) {
        self.note_activity();
        let Some(touch) = self.az.seat.get_touch() else { return };
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.touch_down)(st, pos));
        } else {
            crate::fallback::focus_at(self, pos);
        }
        let under = self.surface_at(pos).map(|(f, o, _)| (f, o));
        touch.down(self, under, &DownEvent { slot, location: pos, serial: SERIAL_COUNTER.next_serial(), time });
    }

    // ---- pointer -------------------------------------------------------

    pub fn pointer_motion_relative(
        &mut self,
        delta: Point<f64, Logical>,
        delta_unaccel: Point<f64, Logical>,
        utime: u64,
        time: u32,
    ) {
        let Some(pointer) = self.az.seat.get_pointer() else { return };
        let pos = pointer.current_location();
        // A locked pointer stays put; relative motion still goes out.
        let focus = pointer.current_focus();
        let mut locked = false;
        if let Some(f) = &focus {
            with_pointer_constraint(&f.0, &pointer, |c| {
                if let Some(c) = c
                    && c.is_active()
                    && let PointerConstraint::Locked(_) = &*c
                {
                    locked = true;
                }
            });
        }
        let surface_loc = pointer.current_focus().and_then(|f| self.az.surface_origin(&f.0));
        pointer.relative_motion(self, focus.zip(surface_loc), &RelativeMotionEvent { delta, delta_unaccel, utime });
        if locked {
            pointer.frame(self);
            return;
        }
        self.pointer_motion_absolute(pos + delta, time);
    }

    /// The pointer moves to a point in the global layout (kept on the
    /// outputs).
    pub fn pointer_motion_absolute(&mut self, pos: Point<f64, Logical>, time: u32) {
        self.note_activity();
        let pos = self.clamp_to_outputs(pos);
        let old = self.az.pointer_pos;
        self.az.pointer_pos = pos;
        if crate::recovery::pointer_moved(self, old, pos, time) {
            return;
        }
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.pointer_motion)(st, pos, time));
        } else {
            crate::fallback::pointer_motion(self, pos, time);
        }
        // Activate a pointer constraint the client asked for.
        if let Some(pointer) = self.az.seat.get_pointer()
            && let Some(f) = pointer.current_focus()
            && crate::recovery::may_constrain(&self.az, &f.0)
        {
            with_pointer_constraint(&f.0, &pointer, |c| {
                if let Some(c) = c
                    && !c.is_active()
                {
                    c.activate();
                }
            });
        }
        for p in [old, pos] {
            if let Some(o) = self.az.output_at(p) {
                self.az.queue_redraw(&o);
            }
        }
    }

    pub fn clamp_to_outputs(&self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        if self.az.outputs.iter().any(|o| self.az.output_geometry(o).to_f64().contains(pos)) {
            return pos;
        }
        // Nearest point on any output.
        let mut best = pos;
        let mut best_d = f64::MAX;
        for o in &self.az.outputs {
            let g = self.az.output_geometry(o).to_f64();
            let x = pos.x.clamp(g.loc.x, g.loc.x + g.size.w - 1.0);
            let y = pos.y.clamp(g.loc.y, g.loc.y + g.size.h - 1.0);
            let d = (x - pos.x).powi(2) + (y - pos.y).powi(2);
            if d < best_d {
                best_d = d;
                best = Point::from((x, y));
            }
        }
        best
    }

    pub fn pointer_button(&mut self, button: u32, pressed: bool, time: u32) {
        self.note_activity();
        if crate::recovery::pointer_button(self, button, pressed) {
            return;
        }
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.pointer_button)(st, button, pressed, time));
        } else {
            crate::fallback::pointer_button(self, button, pressed, time);
        }
    }

    pub fn pointer_axis(&mut self, axis: &AxisEvent) {
        self.note_activity();
        if crate::recovery::scroll(self, axis.v) {
            return;
        }
        if self.lg.healthy() {
            self.lg_call((), |vt, st| (vt.pointer_axis)(st, axis));
        } else {
            crate::fallback::pointer_axis(self, axis);
        }
    }

    /// Scrolls by points (synthetic input).
    pub fn pointer_scroll(&mut self, dx: f64, dy: f64, time: u32) {
        let axis = AxisEvent {
            source: AxisSource::Wheel,
            time,
            h: dx,
            v: dy,
            h120: None,
            v120: None,
            h_stop: false,
            v_stop: false,
        };
        self.pointer_axis(&axis);
    }

    /// The client surface under a point, as the core sees it (popups,
    /// layer surfaces, windows where the logic last put them; not their
    /// decorations).
    pub fn surface_at(&mut self, pos: Point<f64, Logical>) -> Option<SurfaceAt> {
        crate::fallback::surface_at(self, pos)
    }

    // ---- keyboard ------------------------------------------------------

    /// A key from a keyboard (or IPC). The logic sees it first (slow and
    /// sticky keys), then [`State::key_through`] decides it.
    pub fn keyboard_key(&mut self, code: u32, pressed: bool, time: u32) {
        self.note_activity();
        if self.lg.healthy() {
            let handled = self.lg_call(false, |vt, st| {
                (vt.key)(st, code, pressed, time);
                true
            });
            if handled {
                return;
            }
        }
        self.key_through(code, pressed, time);
    }

    /// A key on its way to the focused client: the core's own keys, then
    /// the logic's decision (bindings, overlays, the command key), then
    /// the client.
    pub fn key_through(&mut self, code: u32, pressed: bool, time: u32) {
        let Some(keyboard) = self.az.seat.get_keyboard() else { return };
        let keycode = Keycode::new(code + 8);
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        let serial = SERIAL_COUNTER.next_serial();
        let (decision, mods_changed) = keyboard.input_intercept(self, keycode, state, |state, mods, handle| {
            state.decide_key(keycode, pressed, mods, &handle)
        });
        match decision {
            Decision::Forward => keyboard.input_forward(self, keycode, state, serial, time, mods_changed),
            Decision::Translate(fake) => {
                let real = keyboard.modifier_state();
                keyboard.set_modifier_state(fake);
                keyboard.input_forward(self, keycode, state, serial, time, true);
                keyboard.set_modifier_state(real);
            }
            Decision::Swallow => {}
            Decision::Logic => self.lg_call((), |vt, st| (vt.key_action)(st)),
            Decision::Core(a) => self.run_core_action(a),
        }
    }

    fn decide_key(
        &mut self,
        keycode: Keycode,
        pressed: bool,
        mods: &ModifiersState,
        handle: &KeysymHandle<'_>,
    ) -> Decision {
        let mut syms: Vec<Keysym> = Vec::new();
        if let Some(s) = handle.raw_latin_sym_or_raw_current_sym() {
            syms.push(s);
        }
        syms.push(handle.modified_sym());
        syms.extend(handle.raw_syms());
        // Ctrl+Alt+Del, before anything else: bindings, inhibitors, the
        // lock, overlays and grabs.
        if let Some(d) = crate::recovery::key(self, keycode, pressed, mods, &syms) {
            return d;
        }
        // A swallowed press swallows its release.
        if !pressed && self.az.suppressed_keys.remove(&keycode) {
            return Decision::Swallow;
        }
        if pressed && let Some(a) = core_binding(mods, &syms, self.az.lock.is_locked()) {
            self.az.suppressed_keys.insert(keycode);
            return Decision::Core(a);
        }
        if !self.lg.healthy() {
            if !pressed {
                self.az.translated_keys.remove(&keycode);
            }
            return Decision::Forward;
        }
        let ev = KeyEvent {
            keycode,
            pressed,
            mods: *mods,
            syms: &syms,
            modified: handle.modified_sym(),
            utf8: handle.modified_sym().key_char(),
        };
        match self.lg_call(KeyDecision::Forward, |vt, st| (vt.decide_key)(st, &ev)) {
            KeyDecision::Forward => Decision::Forward,
            KeyDecision::Translate(m) => Decision::Translate(m),
            KeyDecision::Swallow => Decision::Swallow,
            KeyDecision::Act => Decision::Logic,
        }
    }

    /// Runs one of the core's own keys.
    pub fn run_core_action(&mut self, a: CoreAction) {
        match a {
            CoreAction::ChangeVt(vt) => self.backend.change_vt(vt),
            // Not now: logic code may be on the stack. At the end of the
            // iteration.
            CoreAction::Reload => self.lg.pending_reload = Some(Default::default()),
        }
    }

    /// Hands a button to the seat (and so to the client under the pointer,
    /// or the grab in force), keeping count of the buttons it holds.
    pub fn forward_button(&mut self, button: u32, pressed: bool, serial: smithay::utils::Serial, time: u32) {
        let Some(pointer) = self.az.seat.get_pointer() else { return };
        if pressed {
            self.az.forwarded_buttons.insert(button);
        } else {
            self.az.forwarded_buttons.remove(&button);
        }
        let state = if pressed {
            smithay::backend::input::ButtonState::Pressed
        } else {
            smithay::backend::input::ButtonState::Released
        };
        pointer.button(self, &smithay::input::pointer::ButtonEvent { button, state, serial, time });
        pointer.frame(self);
    }

    /// Asks a window to close.
    pub fn close_window(&mut self, id: WinId) {
        if let Some(m) = self.az.windows.get(&id) {
            m.toplevel().send_close();
        }
    }

    /// Ends the session.
    pub fn quit(&mut self) {
        crate::session::notify_stopping();
        self.az.signal.stop();
    }
}

/// The keys the core keeps besides ⌃⌥Delete (the recovery overlay's): VT
/// switching (even when locked) and ⌃❖R to reload.
fn core_binding(mods: &ModifiersState, syms: &[Keysym], locked: bool) -> Option<CoreAction> {
    for s in syms {
        let r = s.raw();
        if (keysyms::KEY_XF86Switch_VT_1..=keysyms::KEY_XF86Switch_VT_12).contains(&r) {
            return Some(CoreAction::ChangeVt((r - keysyms::KEY_XF86Switch_VT_1 + 1) as i32));
        }
    }
    if locked {
        return None;
    }
    let is = |k: u32| syms.iter().any(|s| s.raw() == k);
    if mods.ctrl && mods.logo && !mods.alt && !mods.shift && (is(keysyms::KEY_r) || is(keysyms::KEY_R)) {
        return Some(CoreAction::Reload);
    }
    None
}

impl crate::state::Aizome {
    /// The window a surface (or one of its subsurfaces or popups) belongs to.
    pub fn window_for_root(
        &self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> Option<WinId> {
        let mut root = surface.clone();
        while let Some(p) = smithay::wayland::compositor::get_parent(&root) {
            root = p;
        }
        if let Some(id) = self.window_for_surface(&root) {
            return Some(id);
        }
        // A popup's root.
        let popup = self.popups.find_popup(&root)?;
        let r = smithay::desktop::find_popup_root_surface(&popup).ok()?;
        self.window_for_surface(&r)
    }
}

/// The layer surface (or a subsurface of one) under a point; their popups
/// are in the popup layer.
pub fn layer_under(
    output: &Output,
    layers: &[Layer],
    pos: Point<f64, Logical>,
    out_loc: Point<f64, Logical>,
) -> Option<SurfaceAt> {
    let map = layer_map_for_output(output);
    for layer in layers {
        for l in map.layers_on(*layer).rev() {
            let Some(geo) = map.layer_geometry(l) else { continue };
            let local = pos - out_loc - geo.loc.to_f64();
            if let Some((s, loc)) = l.surface_under(local, WindowSurfaceType::TOPLEVEL | WindowSurfaceType::SUBSURFACE)
            {
                return Some((Focus(s), loc.to_f64() + geo.loc.to_f64() + out_loc, Owner::Layer));
            }
        }
    }
    None
}

/// The lock surface under a point, while locked.
pub fn lock_under(az: &crate::state::Aizome, output: &Output, pos: Point<f64, Logical>) -> Option<SurfaceAt> {
    let out_loc = az.output_geometry(output).loc.to_f64();
    let lock = az.out.get(output).and_then(|o| o.lock_surface.clone())?;
    let (s, loc) = smithay::desktop::utils::under_from_surface_tree(
        lock.wl_surface(),
        pos - out_loc,
        (0, 0),
        WindowSurfaceType::ALL,
    )?;
    Some((Focus(s), loc.to_f64() + out_loc, Owner::Lock))
}

/// evdev codes of the modifier keys: Shift, Ctrl, Alt and Super, left and
/// right.
pub const MODIFIER_KEYS: [u32; 8] = [42, 54, 29, 97, 56, 100, 125, 126];
