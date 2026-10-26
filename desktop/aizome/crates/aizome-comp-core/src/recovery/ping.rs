//! Whether apps answer: xdg_wm_base pings to every client with a window,
//! every couple of seconds (every half second while the recovery overlay
//! is open), and how long each has left one unanswered. A client counts as
//! not responding once a ping has waited [`NOT_RESPONDING_AFTER`].
//!
//! The time a ping went out is kept in the shell client's own data, so it
//! goes away with the client.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::shell::xdg::{ShellClient, ToplevelSurface};

/// How long a ping may go unanswered before the app counts as not
/// responding.
pub const NOT_RESPONDING_AFTER: Duration = Duration::from_millis(2500);

#[derive(Default)]
struct Pending(Mutex<Option<Instant>>);

fn with_pending<T>(client: &ShellClient, f: impl FnOnce(&mut Option<Instant>) -> T) -> Option<T> {
    client
        .with_data(|d| {
            d.insert_if_missing_threadsafe(Pending::default);
            let p = d.get::<Pending>().expect("just inserted");
            let mut g = p.0.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut g)
        })
        .ok()
}

/// Pings each client once (a client with several windows is one client).
pub fn ping(toplevels: impl IntoIterator<Item = ToplevelSurface>) {
    let mut seen: Vec<ShellClient> = Vec::new();
    let now = Instant::now();
    for t in toplevels {
        if !t.alive() {
            continue;
        }
        let client = t.client();
        if seen.contains(&client) {
            continue;
        }
        let waiting = with_pending(&client, |p| p.is_some()).unwrap_or(true);
        if !waiting {
            // Err: one is pending already (someone else's); time it from now.
            let _ = client.send_ping(SERIAL_COUNTER.next_serial());
            with_pending(&client, |p| *p = Some(now));
        }
        seen.push(client);
    }
}

/// A client answered.
pub fn pong(client: &ShellClient) {
    with_pending(client, |p| *p = None);
}

/// How long the window's client has left a ping unanswered.
pub fn waiting(toplevel: &ToplevelSurface) -> Option<Duration> {
    if !toplevel.alive() {
        return None;
    }
    with_pending(&toplevel.client(), |p| p.map(|t| t.elapsed())).flatten()
}

pub fn not_responding(toplevel: &ToplevelSurface) -> bool {
    waiting(toplevel).is_some_and(|d| d >= NOT_RESPONDING_AFTER)
}
