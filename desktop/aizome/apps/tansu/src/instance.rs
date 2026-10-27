//! One instance: a second launch hands its arguments (an image to attach)
//! to the running one through `$XDG_RUNTIME_DIR/aizome-app.tansu.sock` and
//! exits.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use aizome_kit::MsgSender;

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("aizome-app.tansu.sock")
}

pub enum Claim {
    First(UnixListener),
    Handed,
    Alone,
}

/// Become the one instance, or hand `args` to it.
pub fn claim(args: &[String]) -> Claim {
    let path = socket_path();
    if let Ok(mut s) = UnixStream::connect(&path)
        && s.write_all(args.join("\0").as_bytes()).is_ok()
    {
        return Claim::Handed;
    }
    // Nobody answered: a socket left by a crash goes.
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(l) => Claim::First(l),
        Err(e) => {
            log::warn!("{}: {e}", path.display());
            Claim::Alone
        }
    }
}

/// Hear later launches on a thread: each sends `msg(paths)`.
pub fn serve<Msg: Send + 'static>(
    listener: UnixListener,
    tx: MsgSender<Msg>,
    msg: impl Fn(Vec<PathBuf>) -> Msg + Send + 'static,
) {
    let _ = std::thread::Builder::new().name("tansu-instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
            let text = String::from_utf8_lossy(&buf);
            let paths = text.split('\0').filter(|a| !a.is_empty()).map(PathBuf::from).collect();
            if !tx.send(msg(paths)) {
                return;
            }
        }
    });
}
