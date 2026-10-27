//! One instance: a second launch hands its arguments to the running one
//! through `$XDG_RUNTIME_DIR/aizome-app.koe.sock` and exits.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use aizome_kit::MsgSender;

/// The socket of app `name`.
pub fn socket_path(name: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("aizome-app.{name}.sock"))
}

/// What a launch found.
pub enum Claim {
    /// This is the one instance: serve the listener.
    First(UnixListener),
    /// Another runs and has the arguments.
    Handed,
    /// No socket could be had (the app runs anyway, alone).
    Alone,
}

/// Become the one instance of `name`, or hand `args` to it.
pub fn claim(name: &str, args: &[String]) -> Claim {
    let path = socket_path(name);
    if let Ok(mut s) = UnixStream::connect(&path) {
        let msg = args.join("\0") + "\n";
        if s.write_all(msg.as_bytes()).is_ok() {
            return Claim::Handed;
        }
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

/// Answer later launches on a thread of its own: each sends `msg(args)`.
pub fn serve<Msg: Send + 'static>(
    listener: UnixListener,
    tx: MsgSender<Msg>,
    msg: impl Fn(Vec<String>) -> Msg + Send + 'static,
) {
    let _ = std::thread::Builder::new().name("instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = Vec::new();
            let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
            let _ = s.read_to_end(&mut buf);
            let text = String::from_utf8_lossy(&buf);
            let args = text.trim_end_matches('\n').split('\0').filter(|a| !a.is_empty()).map(str::to_owned).collect();
            if !tx.send(msg(args)) {
                return;
            }
        }
    });
}
