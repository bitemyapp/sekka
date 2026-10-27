//! One instance: a second launch raises the window that's open, through
//! `$XDG_RUNTIME_DIR/aizome-app.annai.sock`, and exits.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use aizome_kit::MsgSender;

pub fn socket_path(name: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("aizome-app.{name}.sock"))
}

pub enum Claim {
    First(UnixListener),
    Handed,
    Alone,
}

/// Become the one instance of `name`, or tell the one there is.
pub fn claim(name: &str) -> Claim {
    let path = socket_path(name);
    if let Ok(mut s) = UnixStream::connect(&path)
        && s.write_all(b"raise\n").is_ok()
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

/// Answer later launches on a thread of its own: each sends `msg()`.
pub fn serve<Msg: Send + 'static>(listener: UnixListener, tx: MsgSender<Msg>, msg: impl Fn() -> Msg + Send + 'static) {
    let _ = std::thread::Builder::new().name("instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = Vec::new();
            let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
            let _ = s.read_to_end(&mut buf);
            if !tx.send(msg()) {
                return;
            }
        }
    });
}
