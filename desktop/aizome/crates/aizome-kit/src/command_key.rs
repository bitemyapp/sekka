//! Announce native command keys before opening any windows, independently
//! of the shell and of the application's desktop ID. The compositor ties
//! this connection to its peer credentials and forgets it when it closes.
//!
//! This single IPC exchange stays here so the kit's standalone consumers
//! need no dependency on the rest of the compositor's IPC client.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) fn register() -> Option<UnixStream> {
    let path = std::env::var_os("AIZOME_SOCKET").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| {
        let display = std::env::var("WAYLAND_DISPLAY").ok().filter(|v| !v.is_empty())?;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Some(runtime.join(format!("aizome.{display}.sock")))
    })?;
    // Other desktops have no socket; older Aizome versions reject the
    // request. Neither should stop an application from opening.
    let stream = connect(&path).ok()?;
    match announce(stream, Duration::from_secs(1)) {
        Ok(connection) => Some(connection),
        Err(e) => {
            log::debug!("native command-key announcement unavailable: {e}");
            None
        }
    }
}

/// Connecting must not wait for a stalled compositor to accept another
/// client. A full Unix listener queue returns EAGAIN in nonblocking mode;
/// this optional announcement can simply be unavailable in that case.
fn connect(path: &Path) -> io::Result<UnixStream> {
    // Safety: sockaddr_un consists entirely of integers and a byte array.
    // Zeroes also supply the pathname's terminating NUL.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid compositor socket path"));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (out, byte) in address.sun_path.iter_mut().zip(bytes) {
        *out = *byte as libc::c_char;
    }
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    // Safety: no pointers are passed; socket returns a new owned descriptor.
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // Safety: raw is the successful socket call's new descriptor, owned here.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // Safety: address is initialized and length includes only its family,
    // pathname and terminating NUL, all within sockaddr_un.
    if unsafe { libc::connect(fd.as_raw_fd(), (&raw const address).cast(), length as libc::socklen_t) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

fn announce(mut stream: UnixStream, timeout: Duration) -> io::Result<UnixStream> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"{\"request\":\"native-command-keys\"}\n")?;
    let mut reply = Vec::new();
    BufReader::new(&stream).take(1024).read_until(b'\n', &mut reply)?;
    if reply.last() != Some(&b'\n') {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "incomplete compositor reply"));
    }
    let reply: serde_json::Value = serde_json::from_slice(&reply)?;
    if reply.get("response").and_then(|v| v.as_str()) != Some("ok") {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "compositor did not accept native command keys"));
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct SocketDir(PathBuf);

    impl SocketDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            loop {
                // Socket paths must stay short even when TMPDIR is long.
                let path = PathBuf::from(format!(
                    "/tmp/az-command-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Self(path),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("create socket fixture: {e}"),
                }
            }
        }

        fn socket(&self) -> PathBuf {
            self.0.join("compositor")
        }
    }

    impl Drop for SocketDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.socket());
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn a_full_listener_queue_does_not_block_startup() {
        let dir = SocketDir::new();
        let listener = UnixListener::bind(dir.socket()).unwrap();
        // Safety: listener owns an open listening socket. On Linux a zero
        // backlog permits one waiting connection; the next must not wait.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let first = connect(&dir.socket()).unwrap();
        let path = dir.socket();
        let (tx, rx) = std::sync::mpsc::channel();
        let second = std::thread::spawn(move || tx.send(connect(&path)).unwrap());
        let result = rx.recv_timeout(Duration::from_secs(2));
        // Release even a regressed blocking connection before asserting,
        // so a failure leaves no stuck worker or socket behind.
        drop(first);
        drop(listener);
        second.join().unwrap();
        assert_eq!(
            result.expect("connect must not wait for acceptance").unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn invalid_or_missing_socket_paths_are_unavailable() {
        let dir = SocketDir::new();
        assert_eq!(connect(&dir.socket()).unwrap_err().kind(), io::ErrorKind::NotFound);
        for bytes in [b"/tmp/invalid\0socket".to_vec(), vec![b'x'; 256]] {
            let path = Path::new(std::ffi::OsStr::from_bytes(&bytes));
            assert_eq!(connect(path).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn announcement_keeps_the_connection_until_the_app_finishes() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut request = String::new();
            BufReader::new(&server).read_line(&mut request).unwrap();
            assert_eq!(request, "{\"request\":\"native-command-keys\"}\n");
            server.write_all(b"{\"response\":\"ok\"}\n").unwrap();
            server
        });
        let connection = announce(client, Duration::from_secs(1)).unwrap();
        let mut server = peer.join().unwrap();
        server.set_nonblocking(true).unwrap();
        assert_eq!(server.read(&mut [0]).unwrap_err().kind(), io::ErrorKind::WouldBlock);
        drop(connection);
        assert_eq!(server.read(&mut [0]).unwrap(), 0);
    }

    #[test]
    fn older_or_unresponsive_compositors_do_not_hold_up_startup() {
        for reply in [b"{\"response\":\"error\",\"message\":\"unknown request\"}\n".as_slice(), b"not JSON\n", b""] {
            let (client, mut server) = UnixStream::pair().unwrap();
            server.write_all(reply).unwrap();
            assert!(announce(client, Duration::from_millis(20)).is_err());
        }
    }
}
