//! Reset input: whatever holds the pointer or the keyboard lets go.
//!
//! Every pointer and keyboard grab ends (moves, resizes, tab slides, menus,
//! drags and drops are cancelled), buttons and keys the seat holds as
//! pressed are released (their clients hear the release), pointer
//! constraints are dropped, the compositor's overlays and every menu close,
//! and the cursor shows again. Apps keep running and keep their windows.

use smithay::backend::input::{ButtonState, KeyState};
use smithay::desktop::{PopupKind, PopupManager, find_popup_root_surface};
use smithay::input::keyboard::FilterResult;
use smithay::input::pointer::{ButtonEvent, CursorImageStatus, MotionEvent};
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::pointer_constraints::with_pointer_constraint;

use crate::state::State;

/// What a reset let go of, for the message the overlay shows.
#[derive(Default, Debug)]
pub struct Released {
    pub pointer_grab: bool,
    pub keyboard_grab: bool,
    pub buttons: usize,
    pub keys: usize,
    pub constraint: bool,
    pub menus: usize,
}

impl Released {
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.pointer_grab {
            parts.push("a pointer grab".to_string());
        }
        if self.keyboard_grab {
            parts.push("a keyboard grab".to_string());
        }
        if self.buttons > 0 {
            parts.push(plural(self.buttons, "held button"));
        }
        if self.keys > 0 {
            parts.push(plural(self.keys, "held key"));
        }
        if self.constraint {
            parts.push("a locked pointer".to_string());
        }
        if self.menus > 0 {
            parts.push(plural(self.menus, "menu"));
        }
        if parts.is_empty() {
            "Input reset: nothing was held".into()
        } else {
            format!("Input reset: let go of {}", parts.join(", "))
        }
    }
}

fn plural(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

pub fn reset_input(state: &mut State) -> Released {
    let mut r = Released::default();
    let time = state.az.start.elapsed().as_millis() as u32;

    // Menus: the grab's, then every other popup.
    if state.az.popup_grab.is_some() {
        state.dismiss_popup_grab();
    }
    for popup in state.az.popup_order.clone() {
        if let Ok(root) = find_popup_root_surface(&popup) {
            if PopupManager::dismiss_popup(&root, &popup).is_ok() {
                r.menus += 1;
            }
        } else if let PopupKind::Xdg(p) = &popup {
            p.send_popup_done();
            r.menus += 1;
        }
    }

    // The compositor's own overlays.
    // The logic's overlays close and its drags end.
    state.lg_call((), |vt, st| (vt.quiesce)(st));
    state.az.dnd_icon = None;

    if let Some(pointer) = state.az.seat.get_pointer() {
        // Constraints on whatever has the pointer, or had it when the
        // overlay opened; they stay off until that surface is clicked.
        let held = state.az.recovery.held_constraint.take();
        r.constraint = held.is_some();
        if let Some(surface) = pointer.current_focus().map(|f| f.0).or(held) {
            with_pointer_constraint(&surface, &pointer, |c| {
                if let Some(c) = c
                    && c.is_active()
                {
                    c.deactivate();
                    r.constraint = true;
                }
            });
            state.az.recovery.unconstrained = Some(surface);
        }
        // Away from every surface first, so an unfinished drag and drop
        // is cancelled rather than dropped wherever the pointer is.
        let location = pointer.current_location();
        pointer.motion(state, None, &MotionEvent { location, serial: SERIAL_COUNTER.next_serial(), time });
        pointer.frame(state);
        if pointer.is_grabbed() {
            r.pointer_grab = true;
            pointer.unset_grab(state, SERIAL_COUNTER.next_serial(), time);
        }
        // Buttons the seat still holds.
        let held: Vec<u32> = state.az.forwarded_buttons.drain().collect();
        for button in held {
            r.buttons += 1;
            pointer.button(
                state,
                &ButtonEvent { button, state: ButtonState::Released, serial: SERIAL_COUNTER.next_serial(), time },
            );
        }
        pointer.frame(state);
        if pointer.is_grabbed() {
            pointer.unset_grab(state, SERIAL_COUNTER.next_serial(), time);
        }
    }

    if let Some(keyboard) = state.az.seat.get_keyboard() {
        if keyboard.is_grabbed() {
            r.keyboard_grab = true;
            keyboard.unset_grab(state);
        }
        let mut held: Vec<_> = keyboard.pressed_keys().into_iter().collect();
        held.sort_by_key(|k| k.raw());
        for key in held {
            r.keys += 1;
            keyboard.input::<(), _>(state, key, KeyState::Released, SERIAL_COUNTER.next_serial(), time, |_, _, _| {
                FilterResult::Forward
            });
        }
    }
    state.az.suppressed_keys.clear();
    state.az.translated_keys.clear();
    // Slow and sticky keys let go too (the logic's quiesce, above).

    // The cursor shows again, the client's own until it sets another.
    state.az.cursor_override = None;
    state.az.cursor_status = CursorImageStatus::default_named();

    state.az.layer_focus = None;
    state.update_keyboard_focus();
    state.az.queue_redraw_all();
    tracing::info!(?r, "input reset");
    r
}
