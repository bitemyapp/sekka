//! The terminal ❖Return starts: the default terminal, the one Settei's
//! Default Apps shows and programs that want a terminal get
//! (`xdg-terminals.list`, read by `aizome-apps`). Nothing else names it, so
//! the two can't disagree.
//!
//! The index of applications is read on a thread, the first time it's
//! wanted, and kept: each ❖Return brings it up to date (a cheap check of
//! what it was read from) and starts the terminal from that thread, so the
//! compositor never waits on the file system. It reads the directories the
//! session's programs see: this process's, with the desktop the session
//! sets (`XDG_CURRENT_DESKTOP=Aizome`, [`session_env`](crate::session::session_env)).
//!
//! Once, in someone's own session, foot (the default before Hotaru) becomes
//! Hotaru, and a terminal the person chose before stays theirs
//! (`AppIndex::move_terminal_to_hotaru`). A file in
//! `$XDG_STATE_HOME/aizome` says it's done.

use std::path::PathBuf;
use std::sync::Mutex;

use aizome_apps::{AppIndex, DESKTOP_NAME, Locale, TerminalMove, XdgDirs, parse_desktops};
use tracing::{info, warn};

use crate::session::spawn_with;
use crate::state::Aizome;

static INDEX: Mutex<Option<AppIndex>> = Mutex::new(None);

/// What says the move to Hotaru is done.
const MOVED: &str = "terminal-moved-to-hotaru";

/// Run `f` on the index, read or brought up to date first. Blocks on the
/// file system: call it from a thread.
fn with_index<T>(f: impl FnOnce(&mut AppIndex) -> T) -> T {
    let mut slot = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    let index = match slot.as_mut() {
        Some(index) => {
            index.refresh_if_changed();
            index
        }
        None => {
            let dirs = XdgDirs { desktops: parse_desktops(Some(DESKTOP_NAME)), ..XdgDirs::from_env() };
            slot.insert(AppIndex::load_with(dirs, Locale::from_env()))
        }
    };
    f(index)
}

/// Start the default terminal, in `cwd` (else the home folder), with the
/// session's environment.
pub fn spawn(az: &Aizome, cwd: Option<PathBuf>) {
    let env = crate::session::session_env(az);
    let started = std::thread::Builder::new().name("aizome-terminal".into()).spawn(move || {
        let Some(argv) = with_index(|index| index.terminal_exec()) else {
            warn!("❖Return: no terminal is installed");
            return;
        };
        if let Err(e) = spawn_with(env, &argv, cwd) {
            warn!("❖Return: {e}");
        }
    });
    if let Err(e) = started {
        warn!("❖Return: {e}");
    }
}

/// Move the default terminal from foot to Hotaru, unless that was done
/// already; `chosen` is the settings' old `[keys] terminal`. On a thread;
/// it reads the index ❖Return will use, so the first one waits for it.
pub fn move_to_hotaru_once(chosen: Option<Vec<String>>) {
    let marker = aizome_config::state_dir().join(MOVED);
    if marker.exists() {
        return;
    }
    let started = std::thread::Builder::new().name("aizome-terminal".into()).spawn(move || {
        let moved = match with_index(|index| index.move_terminal_to_hotaru(chosen.as_deref())) {
            Ok(moved) => moved,
            Err(e) => {
                // Tried again next session.
                warn!("default terminal: can't move it to Hotaru: {e}");
                return;
            }
        };
        match &moved {
            TerminalMove::Hotaru => info!("default terminal: Hotaru, which replaces foot"),
            TerminalMove::Listed(id) => info!("default terminal: {id}, as ❖Return started before"),
            TerminalMove::Unchanged(id) => info!("default terminal: {}", id.as_deref().unwrap_or("none installed")),
            TerminalMove::Unmatched(argv) => warn!(
                "default terminal: ❖Return ran {argv:?}, which no installed terminal's desktop entry runs; \
                 it starts the default terminal now"
            ),
        }
        let written = std::fs::create_dir_all(aizome_config::state_dir())
            .and_then(|()| std::fs::write(&marker, format!("{moved:?}\n")));
        if let Err(e) = written {
            warn!("{}: {e}", marker.display());
        }
    });
    if let Err(e) = started {
        warn!("default terminal: {e}");
    }
}
