//! Per-user instance handoff. A lock distinguishes startup from a stale socket.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aizome_kit::MsgSender;
use hotaru_io::PtyCommand;
use serde::{Deserialize, Serialize};

use crate::app::Msg;

const MAX_REQUEST: usize = 1024 * 1024;

pub enum Claim {
    First(Instance),
    Forwarded,
}

pub struct Instance {
    listener: UnixListener,
    _lock: File,
}

#[derive(Serialize, Deserialize)]
struct Request {
    program: Vec<u8>,
    args: Vec<Vec<u8>>,
    cwd: Option<Vec<u8>>,
    env: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Request {
    fn from_command(command: &PtyCommand) -> Self {
        Self {
            program: command.program.as_bytes().to_vec(),
            args: command.args.iter().map(|s| s.as_bytes().to_vec()).collect(),
            cwd: command.cwd.as_ref().map(|p| p.as_os_str().as_bytes().to_vec()),
            env: command.env.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec())).collect(),
        }
    }
    fn command(self) -> PtyCommand {
        use std::ffi::OsString;
        PtyCommand {
            program: OsString::from_vec(self.program),
            args: self.args.into_iter().map(OsString::from_vec).collect(),
            cwd: self.cwd.map(|p| PathBuf::from(OsString::from_vec(p))),
            env: self.env.into_iter().map(|(k, v)| (OsString::from_vec(k), OsString::from_vec(v))).collect(),
        }
    }
}

pub fn claim(command: &PtyCommand) -> io::Result<Claim> {
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
    let path = runtime.join("aizome-app.hotaru.sock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(runtime.join("aizome-app.hotaru.lock"))?;
    // SAFETY: lock owns a live file descriptor; flock neither borrows nor writes memory.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        if let Ok(metadata) = path.symlink_metadata() {
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "Instance path is not our socket"));
            }
            std::fs::remove_file(&path)?;
        }
        return Ok(Claim::First(Instance { listener: UnixListener::bind(&path)?, _lock: lock }));
    }
    let err = io::Error::last_os_error();
    if err.kind() != io::ErrorKind::WouldBlock {
        return Err(err);
    }
    let bytes = serde_json::to_vec(&Request::from_command(command))?;
    if bytes.len() > MAX_REQUEST {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Command is too large"));
    }
    let started = Instant::now();
    let mut stream = loop {
        match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(e)
                if started.elapsed() < Duration::from_secs(2)
                    && matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    };
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    let mut acknowledgement = [0u8];
    stream.read_exact(&mut acknowledgement)?;
    if acknowledgement != [1] {
        return Err(io::Error::other("Instance refused command"));
    }
    Ok(Claim::Forwarded)
}

impl Instance {
    pub fn serve(self, tx: MsgSender<Msg>) -> io::Result<()> {
        std::thread::Builder::new().name("hotaru-instance".into()).spawn(move || {
            // Keep the startup lock alive until the listener thread exits.
            let _lock = self._lock;
            for incoming in self.listener.incoming() {
                let Ok(mut stream) = incoming else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                let mut length = [0u8; 4];
                if stream.read_exact(&mut length).is_err() {
                    continue;
                }
                let length = u32::from_le_bytes(length) as usize;
                if length > MAX_REQUEST {
                    continue;
                }
                let mut bytes = vec![0u8; length];
                if stream.read_exact(&mut bytes).is_err() {
                    continue;
                }
                let Ok(request) = serde_json::from_slice::<Request>(&bytes) else { continue };
                if !tx.send(Msg::Launch(request.command())) {
                    return;
                }
                let _ = stream.write_all(&[1]);
            }
        })?;
        Ok(())
    }
}
