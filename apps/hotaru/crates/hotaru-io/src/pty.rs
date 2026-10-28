//! A Linux PTY with an owned child session and nonblocking master descriptor.
//!
//! All allocation and descriptor preparation happens before `Command::spawn`.
//! Its post-fork callback only calls async-signal-safe libc functions. Dropping a
//! PTY never waits for a child; a shared reaper handles HUP, grace, KILL, and wait.

use crate::fd::nonblocking;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PtyCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// Additional variables, overriding the inherited environment and Hotaru's
    /// TERM, COLORTERM, and TERM_PROGRAM defaults. Values never enter logs.
    pub env: Vec<(OsString, OsString)>,
    /// Integrate only a default shell with no explicit arguments. Never set by
    /// `new`; callers can disable it without changing the selected shell.
    pub integrate_shell: bool,
}

impl PtyCommand {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            integrate_shell: false,
        }
    }

    pub fn shell() -> Self {
        let shell = std::env::var_os("SHELL")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| OsString::from("/bin/sh"));
        let mut command = Self::new(shell);
        command.integrate_shell = true;
        command
    }
}

impl Default for PtyCommand {
    fn default() -> Self {
        Self::shell()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

impl PtySize {
    fn winsize(self) -> io::Result<libc::winsize> {
        if self.rows == 0 || self.cols == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PTY dimensions must be nonzero",
            ));
        }
        Ok(libc::winsize {
            ws_row: self.rows,
            ws_col: self.cols,
            ws_xpixel: self.pixel_width,
            ws_ypixel: self.pixel_height,
        })
    }
}

pub struct Pty {
    // Option allows closing the master before initiating child cleanup.
    master: Option<OwnedFd>,
    child: Option<Child>,
    reaper: mpsc::Sender<Child>,
}

impl Pty {
    pub fn spawn(mut command: PtyCommand, size: PtySize) -> io::Result<Self> {
        let winsize = size.winsize()?;
        let defaults = crate::launch::prepare(&mut command);
        // Initialize the reaper before a process exists: thread-creation failure
        // must fail spawn, not create a child that no one can later reap.
        let reaper = reaper()?.clone();
        // SAFETY: posix_openpt returns a new descriptor, owned below on success.
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
        if master == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this is the unique owner of the successfully opened fd.
        let master = unsafe { OwnedFd::from_raw_fd(master) };
        // SAFETY: these PTY operations reference a live master descriptor.
        if unsafe { libc::grantpt(master.as_raw_fd()) } == -1
            || unsafe { libc::unlockpt(master.as_raw_fd()) } == -1
        {
            return Err(io::Error::last_os_error());
        }
        // TIOCGPTPEER opens the associated slave without a pathname lookup or a
        // /dev/pts mount-namespace race (Linux 4.13+).
        // SAFETY: this ioctl takes integer open flags and returns a new fd.
        let slave = unsafe {
            libc::ioctl(
                master.as_raw_fd(),
                libc::TIOCGPTPEER,
                libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
            )
        };
        if slave == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful ioctl produced a fresh descriptor.
        let slave = unsafe { OwnedFd::from_raw_fd(slave) };
        nonblocking(master.as_fd())?;
        // SAFETY: winsize points at an initialized C struct during the ioctl.
        if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize) } == -1 {
            return Err(io::Error::last_os_error());
        }

        let mut process = Command::new(command.program);
        process
            .args(command.args)
            .env_remove("VTE_VERSION")
            .envs(defaults)
            .envs(command.env)
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        if let Some(cwd) = command.cwd {
            process.current_dir(cwd);
        }
        // SAFETY: std sets up stdin/stdout/stderr before this callback. The
        // callback performs no allocation, locks, formatting, or Rust I/O. All
        // libc operations below are async-signal-safe on Linux. No other thread
        // can change the child's copied address space after fork.
        unsafe {
            process.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                let mut empty: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut empty);
                if libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut()) == -1 {
                    return Err(io::Error::last_os_error());
                }
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigemptyset(&mut action.sa_mask);
                for signal in [
                    libc::SIGPIPE,
                    libc::SIGHUP,
                    libc::SIGINT,
                    libc::SIGQUIT,
                    libc::SIGTERM,
                    libc::SIGTSTP,
                    libc::SIGTTIN,
                    libc::SIGTTOU,
                    libc::SIGWINCH,
                ] {
                    if libc::sigaction(signal, &action, std::ptr::null_mut()) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = process.spawn()?;
        Ok(Self {
            master: Some(master),
            child: Some(child),
            reaper,
        })
    }

    pub fn child_id(&self) -> u32 {
        self.child.as_ref().expect("live Pty owns its child").id()
    }

    /// One nonblocking read. Linux returns EIO when the last slave closes;
    /// expose that as EOF after any already buffered output has been drained.
    pub fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        // SAFETY: the descriptor is live; the buffer is writable for its length.
        let result =
            unsafe { libc::read(self.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EIO) {
                return Ok(0);
            }
            return Err(error);
        }
        Ok(result as usize)
    }

    /// One nonblocking write. A short write or WouldBlock is normal: retain the
    /// unwritten suffix and wait for POLLOUT. Do not buffer unbounded paste data.
    pub fn write(&self, bytes: &[u8]) -> io::Result<usize> {
        // SAFETY: the descriptor is live and bytes is readable for its length.
        let result = unsafe { libc::write(self.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(result as usize)
    }

    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        let winsize = size.winsize()?;
        // SAFETY: winsize lives through the ioctl; the descriptor is live.
        if unsafe { libc::ioctl(self.as_raw_fd(), libc::TIOCSWINSZ, &winsize) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .expect("live Pty owns its child")
            .try_wait()
    }
}

impl AsFd for Pty {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.master
            .as_ref()
            .expect("live Pty owns its master")
            .as_fd()
    }
}

impl AsRawFd for Pty {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.as_fd().as_raw_fd()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Closing the controlling master also notifies the foreground job of
        // hangup. Background cleanup adds bounded escalation for stubborn jobs.
        self.master.take();
        if let Some(child) = self.child.take() {
            // The process-lifetime receiver only exits after all senders drop.
            // A disconnected receiver is therefore an internal unrecoverable
            // reaper failure, never an ordinary child-exit or user error.
            if let Err(error) = self.reaper.send(child) {
                let mut child = error.0;
                let _ = child.kill();
                let _ = std::thread::Builder::new()
                    .name("hotaru-reap-fallback".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
            }
        }
    }
}

fn reaper() -> io::Result<&'static mpsc::Sender<Child>> {
    static REAPER: OnceLock<Result<mpsc::Sender<Child>, String>> = OnceLock::new();
    REAPER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("hotaru-child-reaper".into())
                .spawn(move || reap_children(receiver))
                .map(|_| sender)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))
}

struct ExitingChild {
    child: Child,
    deadline: Instant,
    killed: bool,
}

fn signal_session(child: &Child, signal: libc::c_int) {
    // SAFETY: the child has not been reaped, so its pid cannot have been reused.
    // It was made the session/process-group leader in pre_exec. The negative
    // pid addresses only that child's group. Closing the master separately
    // notifies the foreground group if the shell started a separate job group.
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), signal);
    }
}

fn reap_children(receiver: mpsc::Receiver<Child>) {
    let mut pending: Vec<ExitingChild> = Vec::new();
    loop {
        let next = if pending.is_empty() {
            match receiver.recv() {
                Ok(child) => Some(child),
                Err(_) => return,
            }
        } else {
            receiver.recv_timeout(Duration::from_millis(10)).ok()
        };
        if let Some(mut child) = next {
            match child.try_wait() {
                Ok(Some(_)) => {}
                Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {}
                status => {
                    if matches!(status, Ok(None)) {
                        signal_session(&child, libc::SIGHUP);
                    }
                    // Transient wait errors must not lose ownership. Retry on
                    // the worker before deciding whether signaling is safe.
                    pending.push(ExitingChild {
                        child,
                        deadline: Instant::now() + Duration::from_millis(250),
                        killed: false,
                    });
                }
            }
        }
        pending.retain_mut(|entry| {
            match entry.child.try_wait() {
                Ok(Some(_)) => return false,
                // ECHILD means another process-wide waiter or SIGCHLD policy
                // reaped it. Its pid may have been reused: never signal it.
                Err(error) if error.raw_os_error() == Some(libc::ECHILD) => return false,
                Err(_) => return true,
                Ok(None) => {}
            }
            if !entry.killed && Instant::now() >= entry.deadline {
                signal_session(&entry.child, libc::SIGKILL);
                // kill(pid) also reaches a child that unexpectedly changed its
                // process group. The unreaped child still reserves this pid.
                let _ = entry.child.kill();
                entry.killed = true;
            }
            true
        });
    }
}
