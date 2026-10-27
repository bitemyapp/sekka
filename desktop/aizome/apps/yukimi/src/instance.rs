//! One Yukimi at a time: a second launch hands its arguments to the
//! running one, over `$XDG_RUNTIME_DIR/aizome-app.yukimi.sock`, and exits.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

/// Where the running Yukimi listens.
pub fn socket_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty())?;
    Some(PathBuf::from(dir).join("aizome-app.yukimi.sock"))
}

/// Either the running Yukimi took `args` (this launch should exit), or
/// this one is the first: it listens for later launches.
pub enum Instance {
    Handed,
    First(Option<UnixListener>),
}

pub fn claim(args: &[String]) -> Instance {
    let Some(path) = socket_path() else { return Instance::First(None) };
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let line = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_owned());
        if writeln!(stream, "{line}").is_ok() {
            return Instance::Handed;
        }
    }
    // Nobody answered: a socket left by a Yukimi that's gone.
    let _ = std::fs::remove_file(&path);
    Instance::First(UnixListener::bind(&path).ok())
}

/// Hear later launches' arguments, calling `take` with each.
pub fn listen(listener: UnixListener, take: impl Fn(Vec<String>) -> bool + Send + 'static) {
    let _ = std::thread::Builder::new().name("yukimi-instance".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok() {
                let args: Vec<String> = serde_json::from_str(line.trim()).unwrap_or_default();
                if !take(args) {
                    break;
                }
            }
        }
    });
}

/// Take the socket away when Yukimi quits.
pub fn release() {
    if let Some(path) = socket_path() {
        let _ = std::fs::remove_file(path);
    }
}
