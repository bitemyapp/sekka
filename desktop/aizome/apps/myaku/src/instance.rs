//! One instance: a second launch hands its arguments (`--pid`, `--lens`,
//! `--search`) to the running one through
//! `$XDG_RUNTIME_DIR/aizome-app.myaku.sock` and exits.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("aizome-app.myaku.sock")
}

pub enum Claim {
    First(UnixListener),
    Handed,
    Alone,
}

pub fn claim(args: &[String]) -> Claim {
    let path = socket_path();
    if let Ok(mut s) = UnixStream::connect(&path)
        && s.write_all(args.join("\0").as_bytes()).is_ok()
    {
        return Claim::Handed;
    }
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(l) => Claim::First(l),
        Err(e) => {
            log::warn!("{}: {e}", path.display());
            Claim::Alone
        }
    }
}

/// Hear later launches on a thread: `heard(args)` for each, until it
/// returns false.
pub fn serve(listener: UnixListener, heard: impl Fn(Vec<String>) -> bool + Send + 'static) {
    let _ = std::thread::Builder::new().name("myaku-instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
            let args = String::from_utf8_lossy(&buf).split('\0').filter(|a| !a.is_empty()).map(str::to_owned).collect();
            if !heard(args) {
                return;
            }
        }
    });
}
