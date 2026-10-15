//! Open one device with Apple's authorization broker. Unlike an AppleScript
//! root process, authopen can use the responsible GUI's removable-volume
//! consent. Its documented -stdoutpipe interface transfers an owned descriptor
//! over a private socket; only the writer helper ever receives it.

use crate::image::Cancelled;
use anyhow::{Context, Result, bail, ensure};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

// Never create or truncate: opening the drive must not change any bytes.
// authopen rejects O_NOFOLLOW with EINVAL (even though open(2) accepts it).
// The caller instead rejects symlinks before opening and verifies that the
// returned descriptor is the same raw device. /dev is system-controlled.
const OPEN_FLAGS: i32 = libc::O_RDWR | libc::O_EXLOCK | libc::O_NONBLOCK;

pub(super) fn open(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<File> {
    let mut command = Command::new("/usr/libexec/authopen");
    command.arg("-stdoutpipe").arg("-o").arg(OPEN_FLAGS.to_string()).arg(path);
    authorized_file(command, cancelled).with_context(|| format!("Could not open {}", path.display()))
}

fn authorized_file(mut command: Command, cancelled: &dyn Fn() -> bool) -> Result<File> {
    if cancelled() {
        bail!(Cancelled);
    }
    let (socket, sender) = UnixStream::pair()?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    command.stdin(Stdio::null()).stdout(Stdio::from(OwnedFd::from(sender))).stderr(Stdio::piped());
    let mut child = command.spawn().context("could not start macOS authopen")?;
    // Command retains its stdout handle. Close our copy so a child that exits
    // without sending a descriptor produces EOF instead of an endless wait.
    drop(command);
    let received = loop {
        if cancelled() {
            break Err(anyhow::Error::new(Cancelled));
        }
        match receive_file(&socket) {
            Err(e)
                if e.downcast_ref::<io::Error>().is_some_and(|e| {
                    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted)
                }) =>
            {
                continue;
            }
            result => break result,
        }
    };
    if received.is_err() {
        let _ = child.kill();
    }
    let output = child.wait_with_output().context("could not wait for macOS authopen")?;
    if received.as_ref().is_err_and(|e| e.is::<Cancelled>()) {
        return received;
    }
    ensure!(
        output.status.success(),
        "macOS authopen failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    received
}

fn receive_file(socket: &UnixStream) -> Result<File> {
    let mut byte = 0u8;
    let mut iov = libc::iovec { iov_base: (&mut byte as *mut u8).cast(), iov_len: 1 };
    // Word alignment is sufficient for cmsghdr; capacity also allows us to
    // close surplus descriptors before rejecting an unexpected response.
    let mut control = [0usize; 32];
    // SAFETY: zero is valid for msghdr. All buffers below outlive recvmsg;
    // ancillary headers are produced and bounded by the kernel, not the child.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&control) as _;
        let n = libc::recvmsg(socket.as_raw_fd(), &mut message, 0);
        if n < 0 {
            return Err(io::Error::last_os_error().into());
        }
        let mut files = Vec::new();
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let length = (*header).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                for offset in (0..length).step_by(size_of::<i32>()) {
                    let fd = libc::CMSG_DATA(header).add(offset).cast::<i32>().read_unaligned();
                    // SCM_RIGHTS installed a new descriptor owned by this process.
                    files.push(File::from_raw_fd(fd));
                }
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
        ensure!(message.msg_flags & libc::MSG_CTRUNC == 0, "authopen sent truncated descriptors");
        ensure!(n > 0 && files.len() == 1, "authopen did not send exactly one device descriptor");
        let file = files.pop().unwrap();
        // macOS has no MSG_CMSG_CLOEXEC. Do not pass the handle to later tools.
        ensure!(
            libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) == 0,
            "could not protect the device handle"
        );
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};

    #[test]
    fn real_authopen_accepts_device_flags_and_returns_a_writable_handle() {
        // Exercise the actual macOS broker and the full production receive
        // path. /dev/null needs no authorization or writes to any storage.
        let file = open(Path::new("/dev/null"), &|| false).unwrap();
        use std::os::unix::fs::MetadataExt;
        assert_eq!(file.metadata().unwrap().rdev(), std::fs::metadata("/dev/null").unwrap().rdev());
        // SAFETY: the file owns a live descriptor.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        assert_eq!(flags & libc::O_ACCMODE, libc::O_RDWR);
    }

    #[test]
    fn real_authopen_reproduces_rejected_nofollow_without_permission_advice() {
        let mut command = Command::new("/usr/libexec/authopen");
        command.args(["-stdoutpipe", "-o", &(OPEN_FLAGS | libc::O_NOFOLLOW).to_string(), "/dev/null"]);
        let error = authorized_file(command, &|| false).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("Invalid argument"), "{message}");
        assert!(!message.contains("authorize"), "{message}");
    }

    fn send_files(socket: &UnixStream, files: &[File]) {
        let mut byte = 0u8;
        let mut iov = libc::iovec { iov_base: (&mut byte as *mut u8).cast(), iov_len: 1 };
        let mut control = [0usize; 32];
        // SAFETY: correctly sized and aligned SCM_RIGHTS message, with live
        // file descriptors; sendmsg duplicates them into the recipient.
        unsafe {
            let mut message: libc::msghdr = std::mem::zeroed();
            message.msg_iov = &mut iov;
            message.msg_iovlen = 1;
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = libc::CMSG_SPACE((files.len() * size_of::<i32>()) as _) as _;
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN((files.len() * size_of::<i32>()) as _) as _;
            for (i, file) in files.iter().enumerate() {
                libc::CMSG_DATA(header).add(i * size_of::<i32>()).cast::<i32>().write_unaligned(file.as_raw_fd());
            }
            assert_eq!(libc::sendmsg(socket.as_raw_fd(), &message, 0), 1);
        }
    }

    #[test]
    fn transferred_descriptor_is_owned_seekable_and_does_not_truncate() {
        let (receiver, sender) = UnixStream::pair().unwrap();
        let mut original = tempfile::tempfile().unwrap();
        original.write_all(b"untouched contents").unwrap();
        send_files(&sender, &[original]);
        let mut file = receive_file(&receiver).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "untouched contents");
        // SAFETY: file owns a live descriptor.
        assert_ne!(unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
        assert_eq!(OPEN_FLAGS & (libc::O_TRUNC | libc::O_CREAT), 0);
    }

    #[test]
    fn missing_or_multiple_descriptors_are_rejected() {
        let (receiver, mut sender) = UnixStream::pair().unwrap();
        sender.write_all(b"x").unwrap();
        assert!(receive_file(&receiver).is_err());
        send_files(&sender, &[tempfile::tempfile().unwrap(), tempfile::tempfile().unwrap()]);
        assert!(receive_file(&receiver).is_err());
        drop(sender);
        assert!(receive_file(&receiver).is_err());
    }

    #[test]
    fn failed_child_returns_its_diagnostic_without_hanging() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "echo 'authorization denied' >&2; exit 1"]);
        let error = authorized_file(command, &|| false).unwrap_err();
        assert!(format!("{error:#}").contains("authorization denied"));
    }

    #[test]
    fn cancellation_reaps_a_waiting_child() {
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        let started = std::time::Instant::now();
        let error = authorized_file(command, &|| started.elapsed() > Duration::from_millis(200)).unwrap_err();
        assert!(error.is::<Cancelled>());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
