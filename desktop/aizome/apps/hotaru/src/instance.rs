//! One Hotaru per Wayland display, with all its windows: a later launch
//! hands its command to the running one through a private socket and
//! exits. It names the window to open the command in as a tab (the
//! compositor's identifier for it, which Aizome's ❖Return passes), or asks
//! for a window of its own. A lock tells startup from a socket left by a
//! crash.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aizome_kit::MsgSender;
use hotaru_io::PtyCommand;
use serde::{Deserialize, Serialize};

use crate::app::Msg;

const MAX_REQUEST: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);

/// Where a launch's command goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// A window of its own.
    NewWindow,
    /// A tab in the window the compositor knows by this identifier, or a
    /// window of its own if no window of ours is (any longer) called that.
    Window(String),
}

/// A launch, handed to the running Hotaru or run by this one.
#[derive(Clone)]
pub struct Launch {
    pub target: Target,
    pub command: PtyCommand,
    /// The launcher's activation token, for the window to come forward
    /// with the keyboard.
    pub activation_token: Option<String>,
}

/// What a launch found.
pub enum Claim {
    /// This is the display's Hotaru: serve the listener.
    First(Instance),
    /// The running Hotaru took the launch.
    Forwarded,
    /// Neither could be had (no private runtime directory, no lock): run
    /// alone.
    Alone(io::Error),
    /// The request went to the running Hotaru but wasn't acknowledged: it
    /// may have started the command, so it mustn't be started again.
    Unanswered(io::Error),
}

pub struct Instance {
    listener: UnixListener,
    _lock: File,
}

#[derive(Serialize, Deserialize)]
struct Request {
    target: Target,
    command: Command,
    activation_token: Option<String>,
}

/// A command as bytes: arguments, folders and environments needn't be
/// UTF-8.
#[derive(Serialize, Deserialize)]
struct Command {
    program: Vec<u8>,
    args: Vec<Vec<u8>>,
    cwd: Option<Vec<u8>>,
    env: Vec<(Vec<u8>, Vec<u8>)>,
    integrate_shell: bool,
}

impl Command {
    fn from_command(command: &PtyCommand) -> Self {
        Self {
            program: command.program.as_bytes().to_vec(),
            args: command.args.iter().map(|arg| arg.as_bytes().to_vec()).collect(),
            cwd: command.cwd.as_ref().map(|path| path.as_os_str().as_bytes().to_vec()),
            env: command.env.iter().map(|(key, value)| (key.as_bytes().to_vec(), value.as_bytes().to_vec())).collect(),
            integrate_shell: command.integrate_shell,
        }
    }

    fn command(self) -> PtyCommand {
        use std::ffi::OsString;
        PtyCommand {
            program: OsString::from_vec(self.program),
            args: self.args.into_iter().map(OsString::from_vec).collect(),
            cwd: self.cwd.map(|p| PathBuf::from(OsString::from_vec(p))),
            env: self.env.into_iter().map(|(k, v)| (OsString::from_vec(k), OsString::from_vec(v))).collect(),
            integrate_shell: self.integrate_shell,
        }
    }
}

impl Request {
    fn from_launch(launch: &Launch) -> Self {
        Self {
            target: launch.target.clone(),
            command: Command::from_command(&launch.command),
            activation_token: launch.activation_token.clone(),
        }
    }

    fn launch(self) -> Launch {
        Launch { target: self.target, command: self.command.command(), activation_token: self.activation_token }
    }
}

/// The socket and the lock for Wayland display `display`: a nested or
/// headless session (a test's) has a Hotaru of its own.
fn paths(runtime: &Path, display: &str) -> (PathBuf, PathBuf) {
    let display = display.replace('/', "_");
    (
        runtime.join(format!("aizome-app.hotaru.{display}.sock")),
        runtime.join(format!("aizome-app.hotaru.{display}.lock")),
    )
}

/// Become the display's Hotaru, or hand `launch` to it.
pub fn claim(launch: &Launch) -> Claim {
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => return Claim::Alone(error),
    };
    let (path, lock) = paths(&runtime, &std::env::var("WAYLAND_DISPLAY").unwrap_or_default());
    let lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(lock)
    {
        Ok(lock) => lock,
        Err(error) => return Claim::Alone(error),
    };
    // SAFETY: lock owns a live file descriptor; flock neither borrows nor writes memory.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return match Instance::bind(&path, lock) {
            Ok(instance) => Claim::First(instance),
            Err(error) => Claim::Alone(error),
        };
    }
    let error = io::Error::last_os_error();
    if error.kind() != io::ErrorKind::WouldBlock {
        return Claim::Alone(error);
    }
    match send(&path, &Request::from_launch(launch)) {
        Ok(()) => Claim::Forwarded,
        Err(Failed::Before(error)) => Claim::Alone(error),
        Err(Failed::After(error)) => Claim::Unanswered(error),
    }
}

fn runtime() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unset"))?;
    let metadata = runtime.metadata()?;
    // SAFETY: geteuid takes no arguments and cannot fail.
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "XDG_RUNTIME_DIR must be private and owned by this user",
        ));
    }
    Ok(runtime)
}

/// A handoff that failed before the request could reach the running
/// Hotaru, or after it might have.
enum Failed {
    Before(io::Error),
    After(io::Error),
}

fn send(path: &Path, request: &Request) -> Result<(), Failed> {
    let bytes = serde_json::to_vec(request).map_err(|error| Failed::Before(error.into()))?;
    if bytes.len() > MAX_REQUEST {
        return Err(Failed::Before(io::Error::new(io::ErrorKind::InvalidInput, "Command is too large")));
    }
    // The running Hotaru may still be starting: its listener comes after
    // the lock.
    let started = Instant::now();
    let mut stream = loop {
        match UnixStream::connect(path) {
            Ok(stream) => break stream,
            Err(e)
                if started.elapsed() < TIMEOUT
                    && matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(Failed::Before(e)),
        }
    };
    let sent = (|| {
        stream.set_write_timeout(Some(TIMEOUT))?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
        stream.write_all(&bytes)?;
        let mut acknowledgement = [0u8];
        stream.read_exact(&mut acknowledgement)?;
        if acknowledgement != [1] {
            return Err(io::Error::other("Hotaru refused the command"));
        }
        Ok(())
    })();
    sent.map_err(Failed::After)
}

fn read_request(stream: &mut impl Read) -> io::Result<Request> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_REQUEST {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Command is too large"));
    }
    let mut bytes = vec![0u8; length];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

impl Instance {
    /// Listen at `path` while holding `lock`: a socket there is one a
    /// crashed Hotaru left.
    fn bind(path: &Path, lock: File) -> io::Result<Self> {
        if let Ok(metadata) = path.symlink_metadata() {
            // SAFETY: geteuid takes no arguments and cannot fail.
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "Instance path is not our socket"));
            }
            std::fs::remove_file(path)?;
        }
        Ok(Self { listener: UnixListener::bind(path)?, _lock: lock })
    }

    pub fn serve(self, tx: MsgSender<Msg>) -> io::Result<()> {
        std::thread::Builder::new().name("hotaru-instance".into()).spawn(move || {
            // Keep the startup lock alive until the listener thread exits.
            let _lock = self._lock;
            for incoming in self.listener.incoming() {
                let Ok(mut stream) = incoming else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                let Ok(request) = read_request(&mut stream) else { continue };
                if !tx.send(Msg::Launched(request.launch(), false)) {
                    return;
                }
                let _ = stream.write_all(&[1]);
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "hotaru-instance-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn lock(&self) -> File {
            File::create(self.0.join("lock")).unwrap()
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn handoff_keeps_command_bytes_target_and_token_and_needs_acknowledgement() {
        let dir = Directory::new();
        let path = dir.0.join("socket");
        let instance = Instance::bind(&path, dir.lock()).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = instance.listener.accept().unwrap();
            let request = read_request(&mut stream).unwrap();
            stream.write_all(&[1]).unwrap();
            request.launch()
        });
        let mut command = PtyCommand::new("/bin/echo");
        command.args = vec!["spaces ' $() stay literal".into(), OsString::from_vec(vec![0xff])];
        command.cwd = Some(PathBuf::from(OsString::from_vec(b"/tmp/non-utf8-\xff".to_vec())));
        command.env = vec![("TEST".into(), "value".into())];
        let launch = Launch {
            target: Target::Window("abcDEF0123".into()),
            command: command.clone(),
            activation_token: Some("token".into()),
        };
        assert!(send(&path, &Request::from_launch(&launch)).is_ok());
        let received = server.join().unwrap();
        assert_eq!(received.target, launch.target);
        assert_eq!(received.activation_token, launch.activation_token);
        assert_eq!(received.command.program, command.program);
        assert_eq!(received.command.args, command.args);
        assert_eq!(received.command.cwd, command.cwd);
        assert_eq!(received.command.env, command.env);
        assert_eq!(received.command.integrate_shell, command.integrate_shell);
    }

    #[test]
    fn a_lost_acknowledgement_is_not_a_reason_to_start_the_command_again() {
        let dir = Directory::new();
        let path = dir.0.join("socket");
        let launch = Launch { target: Target::NewWindow, command: PtyCommand::shell(), activation_token: None };
        let instance = Instance::bind(&path, dir.lock()).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = instance.listener.accept().unwrap();
            // The command may already have run: no acknowledgement.
            let _ = read_request(&mut stream).unwrap();
        });
        assert!(matches!(send(&path, &Request::from_launch(&launch)), Err(Failed::After(_))));
        server.join().unwrap();
    }

    #[test]
    fn oversized_requests_and_strangers_files_are_refused() {
        let dir = Directory::new();
        let mut command = PtyCommand::shell();
        command.args = vec!["x".repeat(MAX_REQUEST).into()];
        let launch = Launch { target: Target::NewWindow, command, activation_token: None };
        assert!(matches!(send(&dir.0.join("socket"), &Request::from_launch(&launch)), Err(Failed::Before(_))));
        assert!(read_request(&mut ((MAX_REQUEST + 1) as u32).to_le_bytes().as_slice()).is_err());
        let path = dir.0.join("file");
        std::fs::write(&path, b"keep this file").unwrap();
        assert!(Instance::bind(&path, dir.lock()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");
    }

    #[test]
    fn each_display_has_its_own_socket() {
        let runtime = Path::new("/run/user/1000");
        let one = paths(runtime, "wayland-1");
        let nested = paths(runtime, "/tmp/nested/wayland-9");
        assert_ne!(one, nested);
        assert_eq!(nested.0, runtime.join("aizome-app.hotaru._tmp_nested_wayland-9.sock"));
    }
}
