//! X11 apps run through xwayland-satellite, whose one Wayland connection
//! carries every X window: by its credentials they are all satellite's.
//! Their own processes come from the X server: for each top-level window
//! whose WM_CLASS is the app's ID (satellite makes the app ID from it), the
//! client that owns it (X-Resource's client IDs, else `_NET_WM_PID`).
//!
//! Runs on the process thread, with a deadline: a stuck X server costs a
//! lost answer, never a stuck thread.

use std::sync::mpsc;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::res::{ClientIdMask, ClientIdSpec, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};

/// The processes of the X clients whose windows belong to `app_id`, or
/// none when the X server can't say (in time).
pub fn pids_for(display: &str, app_id: &str) -> Vec<i32> {
    let (tx, rx) = mpsc::channel();
    let (display, app_id) = (display.to_string(), app_id.to_string());
    let spawned = std::thread::Builder::new().name("aizome-x11-pids".into()).stack_size(128 * 1024).spawn(move || {
        let _ = tx.send(query(&display, &app_id).unwrap_or_default());
    });
    if spawned.is_err() {
        return Vec::new();
    }
    rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default()
}

fn query(display: &str, app_id: &str) -> Option<Vec<i32>> {
    let (conn, screen) = x11rb::connect(Some(display)).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let mask = conn.setup().resource_id_mask;
    let net_wm_pid = conn.intern_atom(true, b"_NET_WM_PID").ok()?.reply().ok()?.atom;
    // Every client's process, by the base of its resource IDs.
    let mut by_client: Vec<(u32, i32)> = Vec::new();
    if let Ok(cookie) = conn.res_query_client_ids(&[ClientIdSpec { client: 0, mask: ClientIdMask::LOCAL_CLIENT_PID }])
        && let Ok(reply) = cookie.reply()
    {
        for id in reply.ids {
            if let Some(pid) = id.value.first() {
                by_client.push((id.spec.client & !mask, *pid as i32));
            }
        }
    }
    let want = app_id.to_lowercase();
    let mut pids = Vec::new();
    let mut windows: Vec<Window> = conn.query_tree(root).ok()?.reply().ok()?.children;
    // Clients' windows may sit one level down (a reparenting manager).
    let mut nested = Vec::new();
    for w in &windows {
        if let Ok(c) = conn.query_tree(*w)
            && let Ok(r) = c.reply()
        {
            nested.extend(r.children);
        }
    }
    windows.extend(nested);
    for w in windows {
        let Ok(class) = conn.get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256) else { continue };
        let Ok(class) = class.reply() else { continue };
        let names: Vec<String> = class
            .value
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).to_lowercase())
            .collect();
        if !names.contains(&want) {
            continue;
        }
        let owner = w & !mask;
        let pid = by_client.iter().find(|(c, _)| *c == owner).map(|(_, p)| *p).or_else(|| {
            let r = conn.get_property(false, w, net_wm_pid, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?;
            r.value32()?.next().map(|p| p as i32)
        });
        if let Some(p) = pid
            && p > 0
            && !pids.contains(&p)
        {
            pids.push(p);
        }
    }
    Some(pids)
}
