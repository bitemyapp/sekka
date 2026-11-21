// SPDX-License-Identifier: GPL-3.0-or-later
//! The installer's system calls: with NetworkManager's FFI (`wifi::nm`), the
//! only `unsafe` code in the crate (`#![deny(unsafe_code)]` at the root, and
//! the helper's Nix check fails if any other module allows it). Each wrapper
//! is safe to call: it passes only valid descriptors, pointers and flags.
#![allow(unsafe_code)]

use std::{
    ffi::CStr,
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd},
};

/// Whether this process runs as root.
pub(crate) fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and can't fail.
    unsafe { libc::geteuid() == 0 }
}

/// Takes `file`'s exclusive advisory lock without waiting: whether it got it.
pub(crate) fn try_lock_exclusive(file: &File) -> bool {
    // SAFETY: the descriptor is open for as long as `file` is borrowed.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// An anonymous memory file, closed on exec: input that never has a path.
pub(crate) fn memory_file(name: &CStr) -> io::Result<File> {
    // SAFETY: `name` is a valid C string; the flags are valid.
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: memfd_create returned a new descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Kills the process group `leader` leads (a child started as its own
/// group's leader). A failure means the group is already gone.
pub(crate) fn kill_group(leader: u32) {
    // SAFETY: kill takes any pid; a negative one names a process group.
    unsafe {
        libc::kill(-(leader as i32), libc::SIGKILL);
    }
}

/// How `file`'s pages will be used: a hint the kernel may ignore.
#[derive(Clone, Copy)]
pub(crate) enum Advice {
    /// Read ahead: it's read from start to end.
    Sequential,
    /// Its clean pages can go: they won't be read again.
    DontNeed,
}

pub(crate) fn advise(file: &File, advice: Advice) {
    let advice = match advice {
        Advice::Sequential => libc::POSIX_FADV_SEQUENTIAL,
        Advice::DontNeed => libc::POSIX_FADV_DONTNEED,
    };
    // SAFETY: the descriptor is open for as long as `file` is borrowed; a
    // failure only loses the hint.
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, advice);
    }
}
