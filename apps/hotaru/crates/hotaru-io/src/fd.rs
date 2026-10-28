use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};

pub(crate) fn nonblocking(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: fcntl only reads/changes flags on the borrowed, live descriptor.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: F_SETFL consumes an integer, not a pointer.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // Clipboard descriptors can arrive through SCM_RIGHTS. Never let an offer
    // keep a pipe alive in a subsequently spawned shell after our owner closes.
    // SAFETY: both fcntl operations use the live borrowed descriptor.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
    if flags == -1
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Temporarily block SIGPIPE on this thread. Unlike a process-wide SIG_IGN,
/// this neither changes the application policy nor changes a child shell's
/// inherited dispositions. Consume only a newly generated SIGPIPE after EPIPE.
pub(crate) struct BlockSigpipe {
    set: libc::sigset_t,
    previous: libc::sigset_t,
    already_pending: bool,
    _same_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl BlockSigpipe {
    pub(crate) fn new() -> io::Result<Self> {
        // SAFETY: libc initializes these plain C objects before they are read.
        unsafe {
            let mut set = std::mem::zeroed();
            let mut previous = std::mem::zeroed();
            let mut pending = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGPIPE);
            let error = libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut previous);
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            if libc::sigpending(&mut pending) == -1 {
                let error = io::Error::last_os_error();
                libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
                return Err(error);
            }
            Ok(Self {
                set,
                previous,
                already_pending: libc::sigismember(&pending, libc::SIGPIPE) == 1,
                _same_thread: std::marker::PhantomData,
            })
        }
    }

    pub(crate) fn consume_generated_signal(&self) {
        if self.already_pending {
            return;
        }
        let timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: set is initialized, the timeout is zero (no blocking), and
        // siginfo is optional. SIGPIPE remains blocked until this guard drops.
        while unsafe { libc::sigtimedwait(&self.set, std::ptr::null_mut(), &timeout) } == -1 {
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                break;
            }
        }
    }
}

impl Drop for BlockSigpipe {
    fn drop(&mut self) {
        // SAFETY: restore this same thread's initialized mask. The guard never
        // crosses an await point or moves to another thread within a pump call.
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, std::ptr::null_mut());
        }
    }
}
