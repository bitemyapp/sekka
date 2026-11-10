//! One instance: a second launch (the Toolchest's, while the television's
//! autostarted one runs) asks the running one to come forward through
//! `$XDG_RUNTIME_DIR/aizome-app.chanoma.sock`, and exits. Two would grab the
//! same controllers from each other.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use aizome_kit::MsgSender;

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("aizome-app.chanoma.sock")
}

/// What a launch found.
pub enum Claim {
    /// This is the one instance: serve the listener.
    First(UnixListener),
    /// Another runs, and was asked to come forward.
    Handed,
    /// No socket could be had: run alone.
    Alone,
}

pub fn claim() -> Claim {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Claim::Handed;
    }
    // Nobody answered: a socket left behind by a crash goes.
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(l) => Claim::First(l),
        Err(e) => {
            log::warn!("{}: {e}", path.display());
            Claim::Alone
        }
    }
}

/// Answer later launches on a thread of its own: each sends `msg`.
pub fn serve<Msg: Clone + Send + 'static>(listener: UnixListener, tx: MsgSender<Msg>, msg: Msg) {
    let _ = std::thread::Builder::new().name("instance".into()).spawn(move || {
        for stream in listener.incoming() {
            if stream.is_ok() && !tx.send(msg.clone()) {
                return;
            }
        }
    });
}
