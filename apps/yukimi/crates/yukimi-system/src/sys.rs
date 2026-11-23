// SPDX-License-Identifier: MIT OR Apache-2.0
//! The system calls Yukimi makes: the only unsafe code in Yukimi's backend
//! and its privileged helper. Every function here is safe to call. The
//! crate denies unsafe code everywhere else, the helper forbids it, and
//! the helper's Nix check fails if any other module is allowed it.
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// This computer's name, as the kernel has it.
pub fn hostname() -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: gethostname writes at most buffer.len() bytes into buffer.
    let ok = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } == 0;
    ok.then(|| {
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        String::from_utf8_lossy(&buffer[..end]).into_owned()
    })
}

/// The running kernel's release, such as `6.12.4`.
pub fn kernel_release() -> Option<String> {
    // SAFETY: utsname is plain data, and uname fills it in.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: uname writes only into the struct it is given.
    if unsafe { libc::uname(&mut name) } != 0 {
        return None;
    }
    let release = &name.release;
    let end = release.iter().position(|&c| c == 0).unwrap_or(release.len());
    Some(release[..end].iter().map(|&c| c as u8 as char).collect())
}

/// Whether this process runs as root.
pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// Asks process `pid` to end (SIGTERM).
pub fn terminate(pid: u32) {
    // SAFETY: signalling a process has no memory-safety preconditions.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
}

/// Whether standard input is a pipe.
pub fn stdin_is_pipe() -> bool {
    // SAFETY: stat is plain data, and fstat writes only into it.
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        libc::fstat(0, &mut stat) == 0 && (stat.st_mode & libc::S_IFMT) == libc::S_IFIFO
    }
}

/// The room, in bytes, left to unprivileged users on the filesystem
/// holding `path`.
pub fn free_space(path: &Path) -> Option<u64> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs is plain data, and statvfs writes only into it.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(path.as_ptr(), &mut stat) == 0).then(|| stat.f_bavail as u64 * stat.f_frsize as u64)
    }
}

/// The free blocks of the filesystem holding `path`, in bytes, counting
/// those kept for root: what deleting files changes.
pub fn blocks_free(path: &Path) -> Option<u64> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs is plain data, and statvfs writes only into it.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(path.as_ptr(), &mut stat) == 0).then(|| stat.f_bfree as u64 * stat.f_frsize as u64)
    }
}

/// Runs `command`'s program at the lowest CPU priority.
pub fn at_lowest_priority(command: &mut Command) {
    // SAFETY: setpriority is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, 19);
            Ok(())
        });
    }
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

fn catch(signal: libc::c_int) {
    // SAFETY: the handler only stores to a lock-free atomic. A caught
    // signal reverts to its default in a child at exec.
    unsafe { libc::signal(signal, interrupted as *const () as libc::sighandler_t) };
}

/// Ctrl-C no longer ends the process: it is noted, for
/// [`take_interrupt`].
pub fn catch_sigint() {
    catch(libc::SIGINT);
}

/// SIGTERM no longer ends the process: it is noted, for
/// [`take_interrupt`].
pub fn catch_sigterm() {
    catch(libc::SIGTERM);
}

/// Whether a caught signal arrived since the last call.
pub fn take_interrupt() -> bool {
    INTERRUPTED.swap(false, Ordering::Relaxed)
}

/// A Unix stream socket connected to `path` in a single nonblocking
/// attempt: a full accept queue fails at once (EAGAIN) rather than waiting.
/// The stream it returns blocks.
pub fn connect_once(path: &Path) -> io::Result<UnixStream> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: sockaddr_un consists of integer fields; all-zero is valid.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid Unix socket path"));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, byte) in address.sun_path.iter_mut().zip(bytes) {
        *target = *byte as libc::c_char;
    }
    // SAFETY: the flags and protocol describe a Unix stream socket.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: socket returned a new, owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let length = (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    // SAFETY: address is initialized and length covers its family and terminated path.
    if unsafe { libc::connect(fd.as_raw_fd(), (&address as *const libc::sockaddr_un).cast(), length) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

/// Lets `listener` queue no connections beyond the one being accepted.
#[cfg(test)]
pub(crate) fn listen_without_backlog(listener: &std::os::unix::net::UnixListener) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: this is the live descriptor of the caller's listener.
    if unsafe { libc::listen(listener.as_raw_fd(), 0) } == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Sets a symlink's own modification time, `ahead` seconds from now.
#[cfg(test)]
pub(crate) fn set_link_modified(link: &Path, ahead: i64) {
    let path = std::ffi::CString::new(link.as_os_str().as_bytes()).unwrap();
    let modified = libc::timespec { tv_sec: crate::now() + ahead, tv_nsec: 0 };
    let stamps = [modified, modified];
    // SAFETY: utimensat reads the path and the two timestamps it is given.
    let r = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), stamps.as_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
    assert_eq!(r, 0);
}
