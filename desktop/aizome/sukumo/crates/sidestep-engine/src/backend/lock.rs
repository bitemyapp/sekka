//! The session lock (ext-session-lock-v1): a lock screen's program asks
//! the compositor to lock the session (`ToRender::LockSession`), which
//! hides everything else and gives all input to the lock's surfaces, one
//! on each output, then says it has (`locked`); unlocking ends it.
//!
//! A lock surface is a window's content surface itself, as a layer
//! surface's is: no frame, no decorations, its role the root's, so it
//! takes the keyboard and pointer as any window's content. The
//! compositor sizes it (the output's size) with a configure. A lock the
//! compositor refuses or ends (`finished`) goes, and the main thread
//! hears it; its surfaces then show nothing. Unlocking a lock that was
//! never confirmed withdraws it.
//!
//! Each request names itself (a `LockRequest`), and what the render
//! thread says about a lock is a `LockAnswer` naming the request it was
//! for, so the main thread can drop what it hears about a lock it has
//! withdrawn since. Only the render thread makes answers.
//!
//! `state.session_lock` must stay the only strong `SessionLock` handle:
//! the toolkit hands `locked` and `finished` only for a lock still alive,
//! and the handlers below take them to be about the lock held (0.19's
//! `SessionLock` has no equality to check that by). A clone kept anywhere
//! else would let an old lock's `finished` end the new one.

use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockSurface, SessionLockSurfaceConfigure,
};

use super::{Role, Shell, State};
use crate::protocol::{FromRender, LockAnswer, LockRequest};

/// The lock asked for or held, and the main thread's request for it.
pub(super) struct Held {
    pub(super) lock: SessionLock,
    request: LockRequest,
}

/// Ask for the lock under `request`. A lock asked for or held already
/// becomes `request`'s, and says so if it's held (the main thread asks
/// again only after unlocking, so this doesn't happen).
pub(super) fn lock(state: &mut State, request: LockRequest) {
    if let Some(held) = &mut state.session_lock {
        held.request = request;
        if held.lock.is_locked() {
            state.send(FromRender::SessionLocked { lock: LockAnswer::heard(request) });
        }
        return;
    }
    match state.session_locks.lock(&state.qh) {
        Ok(lock) => state.session_lock = Some(Held { lock, request }),
        Err(_) => {
            eprintln!("sidestep: the compositor has no ext-session-lock");
            state.send(FromRender::SessionLockFinished { lock: LockAnswer::heard(request) });
        }
    }
}

/// Unlock, or withdraw a lock not yet confirmed (dropping the last handle
/// destroys it, which a compositor takes as that).
pub(super) fn unlock(state: &mut State) {
    if let Some(held) = state.session_lock.take() {
        held.lock.unlock();
    }
}

/// `surface` as the lock's surface on output `output` (by its id), for
/// the compositor to configure; `None` without a lock asked for, without
/// that output, or with a lock surface there already (a second is a
/// protocol error).
pub(super) fn create(state: &State, surface: &WlSurface, output: u32) -> Option<SessionLockSurface> {
    let held = state.session_lock.as_ref()?;
    if state.windows.values().any(|w| matches!(w.shell, Shell::Lock(_, on) if on == output)) {
        return None;
    }
    let on = super::layer::output(state, output)?;
    Some(held.lock.create_lock_surface(surface.clone(), &on, &state.qh))
}

// The toolkit hands these events only for a lock object that's still
// alive, and `state.session_lock` holds the only handle to one: they are
// about the lock held, under its request. A lock withdrawn or unlocked
// was dropped, and hears nothing more.
impl SessionLockHandler for State {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        if let Some(held) = &self.session_lock {
            self.send(FromRender::SessionLocked { lock: LockAnswer::heard(held.request) });
        }
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        // Refused or ended: the lock object goes, unlocked if it was
        // locked (as the protocol asks), else destroyed.
        if let Some(held) = self.session_lock.take() {
            held.lock.unlock();
            self.send(FromRender::SessionLockFinished { lock: LockAnswer::heard(held.request) });
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        let Some(Role::Root(window)) = self.role(surface.wl_surface()) else { return };
        let (width, height) = configure.new_size;
        self.resize(window, width, height);
    }
}
