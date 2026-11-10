//! The compositor's idle inhibitor, on a surface of its own that's never
//! shown: Aizome holds off idleness while any surface has one (as for a
//! video player), whether or not it's on screen. Keep Awake's service
//! holds one while it runs, and the shell holds one while an app asks the
//! session bus to keep the screen awake.

use std::os::fd::{AsFd, AsRawFd};
use std::time::Duration;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_compositor, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::wp::idle_inhibit::zv1::client::{zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1};

/// Idleness held off, until it's dropped. It has a connection of its own
/// to the compositor, so it goes with its process however that ends.
pub struct IdleInhibitor {
    conn: Connection,
    queue: EventQueue<Nothing>,
    surface: wl_surface::WlSurface,
    inhibitor: zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
}

struct Nothing;

impl IdleInhibitor {
    /// Connects to the compositor (`WAYLAND_DISPLAY`) and takes the
    /// inhibitor; it's held once this returns.
    pub fn new() -> Result<IdleInhibitor, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("the compositor: {e}"))?;
        let (globals, mut queue) =
            registry_queue_init::<Nothing>(&conn).map_err(|e| format!("the compositor's globals: {e}"))?;
        let qh = queue.handle();
        let compositor: wl_compositor::WlCompositor =
            globals.bind(&qh, 1..=6, ()).map_err(|e| format!("wl_compositor: {e}"))?;
        let manager: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 =
            globals.bind(&qh, 1..=1, ()).map_err(|e| format!("the compositor offers no idle inhibitor: {e}"))?;
        let surface = compositor.create_surface(&qh, ());
        let inhibitor = manager.create_inhibitor(&surface, &qh, ());
        queue.roundtrip(&mut Nothing).map_err(|e| format!("the compositor: {e}"))?;
        Ok(IdleInhibitor { conn, queue, surface, inhibitor })
    }

    /// Waits up to `timeout` for the compositor and reads what it sent;
    /// an error once it's gone.
    pub fn wait(&mut self, timeout: Duration) -> Result<(), String> {
        let gone = |e: &dyn std::fmt::Display| format!("the compositor went away: {e}");
        self.conn.flush().map_err(|e| gone(&e))?;
        if let Some(guard) = self.conn.prepare_read() {
            let mut pfd =
                libc::pollfd { fd: guard.connection_fd().as_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // Safety: one valid pollfd, for the duration of the call.
            let n = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int) };
            if n > 0 {
                guard.read().map_err(|e| gone(&e))?;
            }
        }
        self.queue.dispatch_pending(&mut Nothing).map_err(|e| gone(&e))?;
        Ok(())
    }
}

impl Drop for IdleInhibitor {
    /// Lets go now, in so many words, without leaving it to the
    /// compositor to notice the connection closing.
    fn drop(&mut self) {
        self.inhibitor.destroy();
        self.surface.destroy();
        let _ = self.conn.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Nothing {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Nothing {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_surface::WlSurface,
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1,
    zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1
);
