//! One instance: a second launch hands its files to the running one
//! through `$XDG_RUNTIME_DIR/aizome-app.shiori.sock` and exits.

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
    /// Another runs and has the files.
    Handed,
    /// No socket could be had (the app runs anyway, alone).
    Alone,
}

/// Become the one instance of `name`, or hand `paths` (absolute) to it.
pub fn claim(name: &str, paths: &[PathBuf]) -> Claim {
    let path = socket_path(name);
    if let Ok(mut s) = UnixStream::connect(&path) {
        let msg: Vec<u8> =
            paths.iter().flat_map(|p| p.as_os_str().as_encoded_bytes().iter().copied().chain([0])).collect();
        if s.write_all(&msg).is_ok() && s.shutdown(std::net::Shutdown::Write).is_ok() {
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

/// Answer later launches on a thread of its own: each sends `msg(paths)`.
pub fn serve<Msg: Send + 'static>(
    listener: UnixListener,
    tx: MsgSender<Msg>,
    msg: impl Fn(Vec<PathBuf>) -> Msg + Send + 'static,
) {
    let _ = std::thread::Builder::new().name("instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = Vec::new();
            let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
            let _ = s.read_to_end(&mut buf);
            let paths = buf
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                // SAFETY: the bytes came from `as_encoded_bytes` in `claim`.
                .map(|p| PathBuf::from(unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(p) }))
                .collect();
            if !tx.send(msg(paths)) {
                return;
            }
        }
    });
}
