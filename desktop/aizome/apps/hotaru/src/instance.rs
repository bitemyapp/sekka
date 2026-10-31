//! One process per terminal window, with explicit, session-scoped handoff.
//! The compositor chooses the window on the current Desk; an ordinary launch
//! opens a new one. Neither window ownership nor routing depends on its title.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_kit::MsgSender;
use hotaru_io::PtyCommand;
use serde::{Deserialize, Serialize};

use crate::app::Msg;

const MAX_REQUEST: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);

pub struct Instance {
    listener: UnixListener,
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
enum Request {
    Launch(Command),
    Quit,
}

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
            args: command.args.iter().map(|s| s.as_bytes().to_vec()).collect(),
            cwd: command.cwd.as_ref().map(|p| p.as_os_str().as_bytes().to_vec()),
            env: command.env.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec())).collect(),
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

fn private_directory(path: &Path) -> io::Result<()> {
    let metadata = path.symlink_metadata()?;
    // SAFETY: geteuid takes no arguments and cannot fail.
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Hotaru's runtime directory must be private and owned by this user",
        ));
    }
    Ok(())
}

fn directory() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unset"))?;
    private_directory(&runtime)?;
    // Two nested compositors may share a runtime directory. Quit and tab
    // requests must never cross from one Wayland display into another.
    let mut session = DefaultHasher::new();
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").unwrap_or_default());
    runtime.join(display).hash(&mut session);
    let path = runtime.join(format!("hotaru-{:016x}", session.finish()));
    match fs::DirBuilder::new().mode(0o700).create(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    private_directory(&path)?;
    Ok(path)
}

fn socket(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{pid}.sock"))
}

pub fn listen() -> io::Result<Instance> {
    Instance::bind(socket(&directory()?, std::process::id()))
}

/// False only when the selected window has already gone. Once a request
/// may have reached it, an error must not start the command a second time.
pub fn forward(pid: u32, command: &PtyCommand) -> io::Result<bool> {
    send(&directory()?, pid, &Request::Launch(Command::from_command(command)))
}

/// Close the other windows in this Wayland session. Each uses its ordinary
/// close path, including any pending settings save. The caller closes last.
pub fn quit_others(dir: &Path) -> io::Result<()> {
    let mut first_error = None;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "sock") {
            continue;
        }
        let Some(pid) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok()) else { continue };
        if pid == std::process::id() || !entry.file_type()?.is_socket() {
            continue;
        }
        if let Err(error) = send(&dir, pid, &Request::Quit) {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn send(dir: &Path, pid: u32, request: &Request) -> io::Result<bool> {
    let bytes = serde_json::to_vec(request)?;
    if bytes.len() > MAX_REQUEST {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Command is too large"));
    }
    let mut stream = match UnixStream::connect(socket(dir, pid)) {
        Ok(stream) => stream,
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    // A stale socket after PID reuse must not target a different process.
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&peer) as libc::socklen_t;
    // SAFETY: the live fd and writable ucred/length storage match SO_PEERCRED.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut peer as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if peer.pid as u32 != pid || peer.uid != unsafe { libc::geteuid() } {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Hotaru window identity changed"));
    }
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    let mut acknowledgement = [0u8];
    stream.read_exact(&mut acknowledgement)?;
    if acknowledgement != [1] {
        return Err(io::Error::other("Window refused command"));
    }
    Ok(true)
}

impl Instance {
    pub fn directory(&self) -> &Path {
        self.path.parent().unwrap()
    }

    fn bind(path: PathBuf) -> io::Result<Self> {
        if let Ok(metadata) = path.symlink_metadata() {
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "Window path is not our socket"));
            }
            // Our PID cannot belong to another live process.
            fs::remove_file(&path)?;
        }
        Ok(Self { listener: UnixListener::bind(&path)?, path })
    }

    pub fn serve(self, tx: MsgSender<Msg>) -> io::Result<()> {
        std::thread::Builder::new().name("hotaru-instance".into()).spawn(move || {
            for incoming in self.listener.incoming() {
                let Ok(mut stream) = incoming else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                let Ok(request) = read_request(&mut stream) else { continue };
                let message = match request {
                    Request::Launch(command) => Msg::Launch(command.command()),
                    Request::Quit => Msg::CloseWindow,
                };
                if !tx.send(message) {
                    return;
                }
                let _ = stream.write_all(&[1]);
            }
        })?;
        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
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
                "hotaru-windows-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn handoff_preserves_command_bytes_and_requires_acknowledgement() {
        let dir = Directory::new();
        let instance = Instance::bind(socket(&dir.0, std::process::id())).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = instance.listener.accept().unwrap();
            let Request::Launch(command) = read_request(&mut stream).unwrap() else { panic!("expected a launch") };
            stream.write_all(&[1]).unwrap();
            command.command()
        });
        let mut command = PtyCommand::new("/bin/echo");
        command.args = vec!["spaces ' $() stay literal".into(), OsString::from_vec(vec![0xff])];
        command.cwd = Some(PathBuf::from(OsString::from_vec(b"/tmp/non-utf8-\xff".to_vec())));
        command.env = vec![("TEST".into(), "value".into())];
        assert!(send(&dir.0, std::process::id(), &Request::Launch(Command::from_command(&command))).unwrap());
        let received = server.join().unwrap();
        assert_eq!(received.program, command.program);
        assert_eq!(received.args, command.args);
        assert_eq!(received.cwd, command.cwd);
        assert_eq!(received.env, command.env);
        assert_eq!(received.integrate_shell, command.integrate_shell);
    }

    #[test]
    fn vanished_window_can_fall_back_but_an_unacknowledged_launch_cannot() {
        let dir = Directory::new();
        let pid = std::process::id();
        assert!(!send(&dir.0, pid, &Request::Quit).unwrap());
        let instance = Instance::bind(socket(&dir.0, pid)).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = instance.listener.accept().unwrap();
            let _ = read_request(&mut stream).unwrap();
            // The command may already have run. Losing its acknowledgement
            // must report an error instead of inviting duplicate execution.
        });
        assert!(send(&dir.0, pid, &Request::Launch(Command::from_command(&PtyCommand::shell()))).is_err());
        server.join().unwrap();
        assert!(!send(&dir.0, pid, &Request::Quit).unwrap());
        // A socket left by an unclean exit also falls back safely.
        drop(UnixListener::bind(socket(&dir.0, pid)).unwrap());
        assert!(!send(&dir.0, pid, &Request::Quit).unwrap());
    }

    #[test]
    fn handoff_refuses_the_wrong_process_and_oversized_requests() {
        let dir = Directory::new();
        let pid = std::process::id();
        let _wrong = Instance::bind(socket(&dir.0, pid + 1)).unwrap();
        assert_eq!(send(&dir.0, pid + 1, &Request::Quit).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        let mut command = PtyCommand::shell();
        command.args = vec!["x".repeat(MAX_REQUEST).into()];
        assert_eq!(
            send(&dir.0, pid, &Request::Launch(Command::from_command(&command))).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(read_request(&mut ((MAX_REQUEST + 1) as u32).to_le_bytes().as_slice()).is_err());
        let path = socket(&dir.0, pid);
        fs::write(&path, b"keep this file").unwrap();
        assert!(Instance::bind(path.clone()).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"keep this file");
    }
}
