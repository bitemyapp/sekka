//! One instance: a second launch hands its requests to the running Hako
//! through `$XDG_RUNTIME_DIR/aizome-app.hako.sock` and exits.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::MsgSender;

use super::app::Request;
use crate::model::uri;

/// The socket.
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    // A display of its own (a nested or headless session) runs its own
    // Hako.
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if display.is_empty() || display == "wayland-0" || display == "wayland-1" {
        dir.join("aizome-app.hako.sock")
    } else {
        dir.join(format!("aizome-app.hako.{}.sock", display.replace('/', "_")))
    }
}

/// What a launch found.
pub enum Claim {
    /// This is the one instance: serve the listener.
    First(UnixListener),
    /// Another runs and has the requests.
    Handed,
    /// No socket could be had (Hako runs anyway, alone).
    Alone,
}

/// One request a line. Keep ordinary paths compatible with an already-open
/// older Hako; escape newlines and non-UTF-8 names as file URIs.
fn encode(requests: &[Request]) -> String {
    let mut s = String::new();
    for r in requests {
        let (verb, p) = match r {
            Request::Open(p) => ("open", p),
            Request::Reveal(p) => ("reveal", p),
        };
        s.push_str(verb);
        s.push('\0');
        match p.to_str().filter(|p| !p.contains(['\n', '\r'])) {
            Some(p) => s.push_str(p),
            None => s.push_str(&uri::file_uri(p)),
        }
        s.push('\n');
    }
    if requests.is_empty() {
        s.push_str("window\n");
    }
    s
}

pub fn decode(text: &str) -> Vec<Request> {
    text.lines()
        .filter_map(|l| {
            let (verb, p) = l.split_once('\0').unwrap_or((l, ""));
            let p = uri::path_of(p).unwrap_or_else(|| PathBuf::from(p));
            match verb {
                "open" => Some(Request::Open(p)),
                "reveal" => Some(Request::Reveal(p)),
                _ => None,
            }
        })
        .collect()
}

/// Become the one instance, or hand `requests` to it.
pub fn claim(requests: &[Request]) -> Claim {
    let path = socket_path();
    if let Ok(mut s) = UnixStream::connect(&path) {
        let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
        if s.write_all(encode(requests).as_bytes()).is_ok() {
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

/// Answer later launches on a thread of their own.
pub fn serve<Msg: Send + 'static>(
    listener: UnixListener,
    tx: MsgSender<Msg>,
    msg: impl Fn(Vec<Request>) -> Msg + Send + 'static,
) {
    let _ = std::thread::Builder::new().name("hako-instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = String::new();
            let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = s.read_to_string(&mut buf);
            let mut rs = decode(&buf);
            if rs.is_empty() && buf.starts_with("window") {
                rs.push(Request::Open(crate::home_dir()));
            }
            if !tx.send(msg(rs)) {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_travel_whole() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let rs = vec![
            Request::Open(PathBuf::from("/a b/c")),
            Request::Reveal(PathBuf::from("/x/a\nb#%.txt")),
            Request::Reveal(PathBuf::from(OsString::from_vec(b"/x/\xff".to_vec()))),
        ];
        assert_eq!(decode(&encode(&rs)), rs);
        assert_eq!(decode(&encode(&[])), vec![]);
        assert_eq!(decode("reveal\0/x/old.txt\n"), vec![Request::Reveal("/x/old.txt".into())]);
    }
}
