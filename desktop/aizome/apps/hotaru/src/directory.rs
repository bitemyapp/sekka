//! Read the focused pane's live directory without guessing which child is its
//! shell. Linux abstract sockets disappear with the process, including on crash.

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use aizome_kit::MsgSender;

use crate::app::Msg;

const TIMEOUT: Duration = Duration::from_millis(500);
const MAX_PATH: usize = 4096;

fn address(pid: u32) -> io::Result<SocketAddr> {
    // SAFETY: geteuid takes no arguments and cannot fail.
    let uid = unsafe { libc::geteuid() };
    SocketAddr::from_abstract_name(format!("aizome-hotaru.cwd.{uid}.{pid}"))
}

fn peer(stream: &UnixStream) -> io::Result<libc::ucred> {
    let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the stream owns its fd; credentials and length are writable,
    // correctly sized storage. A successful SO_PEERCRED initializes ucred.
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: getsockopt succeeded and initialized the credential structure.
    Ok(unsafe { credentials.assume_init() })
}

pub struct Server(UnixListener);

impl Server {
    pub fn bind() -> io::Result<Self> {
        UnixListener::bind_addr(&address(std::process::id())?).map(Self)
    }

    pub fn serve(self, tx: MsgSender<Msg>) -> io::Result<()> {
        std::thread::Builder::new().name("hotaru-directory".into()).spawn(move || {
            for incoming in self.0.incoming() {
                let Ok(mut stream) = incoming else { continue };
                // Abstract sockets have no filesystem permissions. Only this
                // user's clients may ask, and clients verify our PID as well.
                if !peer(&stream).is_ok_and(|peer| peer.uid == unsafe { libc::geteuid() }) {
                    continue;
                }
                let (reply, answer) = mpsc::channel();
                if !tx.send(Msg::QueryDirectory(reply)) {
                    return;
                }
                let directory = answer.recv_timeout(TIMEOUT).ok().flatten();
                let bytes = directory.as_deref().map_or(&[][..], |path| path.as_os_str().as_bytes());
                let bytes = if bytes.len() <= MAX_PATH { bytes } else { &[] };
                let _ = stream.set_write_timeout(Some(TIMEOUT));
                let _ = stream.write_all(&(bytes.len() as u32).to_le_bytes()).and_then(|_| stream.write_all(bytes));
            }
        })?;
        Ok(())
    }
}

/// Query exactly this user's Hotaru process. No shell, text escaping or newline
/// trimming is involved, so every filename byte survives the handoff.
pub fn query(pid: u32) -> io::Result<PathBuf> {
    let mut stream = UnixStream::connect_addr(&address(pid)?)?;
    let credentials = peer(&stream)?;
    // SAFETY: geteuid takes no arguments and cannot fail.
    if credentials.pid as u32 != pid || credentials.uid != unsafe { libc::geteuid() } {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Not the requested Hotaru process"));
    }
    stream.set_read_timeout(Some(TIMEOUT * 2))?;
    read_directory(&mut stream)
}

fn read_directory(stream: &mut impl Read) -> io::Result<PathBuf> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if !(1..=MAX_PATH).contains(&length) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "No live pane directory"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    if !path.is_absolute() || !path.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Pane directory no longer exists"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_oversized_and_truncated_answers_are_rejected() {
        for bytes in [0u32.to_le_bytes().to_vec(), 4097u32.to_le_bytes().to_vec(), vec![5, 0, 0, 0, b'/']] {
            assert!(read_directory(&mut bytes.as_slice()).is_err());
        }
    }
}
