//! One instance: a second launch hands its requests to the running Hako
//! through `$XDG_RUNTIME_DIR/aizome-app.hako.sock` and exits.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
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
/// older Hako; escape newlines and non-UTF-8 names as file URIs. The
/// launcher's activation token comes last, where an older Hako ignores it.
fn encode(requests: &[Request], token: Option<&str>) -> String {
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
    if let Some(t) = token.filter(|t| !t.is_empty() && !t.contains(['\n', '\r', '\0'])) {
        s.push_str("token\0");
        s.push_str(t);
        s.push('\n');
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

/// The token that let the launch that handed over `text` take focus.
pub fn token(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix("token\0")).filter(|t| !t.is_empty()).map(str::to_owned)
}

/// Hand `requests` to the Hako listening at `path`, with the launcher's
/// activation token.
pub fn hand_over(path: &Path, requests: &[Request], token: Option<&str>) -> std::io::Result<()> {
    let mut s = UnixStream::connect(path)?;
    s.set_write_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(encode(requests, token).as_bytes())
}

/// Become the one instance, or hand `requests` to it.
pub fn claim(requests: &[Request]) -> Claim {
    claim_at(&socket_path(), requests, std::env::var("XDG_ACTIVATION_TOKEN").ok().as_deref())
}

fn claim_at(path: &Path, requests: &[Request], token: Option<&str>) -> Claim {
    // Launches at the same moment take turns: otherwise each could remove
    // the other's socket and both would run, one of them out of reach.
    // The lock file stays, so that every launch locks the same one.
    let lock = File::options().create(true).truncate(false).write(true).open(path.with_extension("lock"));
    let _turn = lock.and_then(|f| f.lock().map(|()| f));
    if hand_over(path, requests, token).is_ok() {
        return Claim::Handed;
    }
    // Nobody answered: a socket left behind by a crash goes.
    let _ = std::fs::remove_file(path);
    match UnixListener::bind(path) {
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
    msg: impl Fn(Vec<Request>, Option<String>) -> Msg + Send + 'static,
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
            if !tx.send(msg(rs, token(&buf))) {
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
        assert_eq!(decode(&encode(&rs, None)), rs);
        assert_eq!(decode(&encode(&[], None)), vec![]);
        assert_eq!(decode("reveal\0/x/old.txt\n"), vec![Request::Reveal("/x/old.txt".into())]);
    }

    #[test]
    fn the_launchers_token_travels_after_the_requests() {
        let rs = vec![Request::Reveal(PathBuf::from("/x/y.txt"))];
        let text = encode(&rs, Some("launch-token"));
        assert_eq!((decode(&text), token(&text).as_deref()), (rs.clone(), Some("launch-token")));
        // An older Hako reads "window" first and skips the line it doesn't know.
        let text = encode(&[], Some("launch-token"));
        assert!(text.starts_with("window\n"));
        assert_eq!((decode(&text), token(&text).as_deref()), (vec![], Some("launch-token")));
        for bad in ["", "two\nlines"] {
            assert_eq!(token(&encode(&rs, Some(bad))), None);
        }
    }

    #[test]
    fn launches_at_once_leave_one_instance() {
        // Unix socket paths are short (108 bytes): a long $TMPDIR gives way.
        let mut base = std::env::temp_dir();
        if base.as_os_str().len() > 60 && Path::new("/tmp").is_dir() {
            base = PathBuf::from("/tmp");
        }
        let dir = base.join(format!("hako-instance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("hako.sock");
        // A socket left behind by a crash: no launch finds anyone there.
        drop(UnixListener::bind(&path).unwrap());
        let start = std::sync::Barrier::new(8);
        let claims: Vec<Claim> = std::thread::scope(|s| {
            let launches: Vec<_> = (0..8)
                .map(|_| {
                    s.spawn(|| {
                        start.wait();
                        claim_at(&path, &[], None)
                    })
                })
                .collect();
            launches.into_iter().map(|l| l.join().unwrap()).collect()
        });
        let first = claims.iter().filter(|c| matches!(c, Claim::First(_))).count();
        let handed = claims.iter().filter(|c| matches!(c, Claim::Handed)).count();
        assert_eq!((first, handed), (1, 7));
        drop(claims);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
