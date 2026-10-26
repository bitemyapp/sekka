//! Sending signals to the caller's own processes.
//!
//! [`signal_process`] refuses pid 0 and 1 and the calling process itself
//! ([`Error::InvalidArgument`]), and any process whose *real* uid (the
//! first `Uid:` field of `/proc/PID/status`) isn't the caller's
//! ([`Error::NotAuthorized`]), even when the caller has the privilege to do
//! more: a monitor run as root must not end system services by accident.
//! The kernel's own permission check still applies after that.
//!
//! The process is pinned with a pidfd (`pidfd_open(2)`, Linux 5.3) before
//! its uid is checked and is then signalled through it
//! (`pidfd_send_signal(2)`), so a pid reused in between can't receive the
//! signal. Where pidfds aren't available, it falls back to `kill(2)`.

use super::process::{parse_stat, parse_status_uid};
use crate::{Error, Result};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// A signal the UI may send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signal {
    /// `SIGTERM`: ask it to quit ("End Process").
    Terminate,
    /// `SIGKILL`: end it at once, without letting it clean up ("Kill").
    Kill,
    /// `SIGINT`: as Ctrl+C in a terminal.
    Interrupt,
    /// `SIGHUP`: its terminal went away; many daemons reload instead.
    Hangup,
    /// `SIGSTOP`: pause it.
    Stop,
    /// `SIGCONT`: resume it after [`Stop`](Signal::Stop).
    Continue,
}

impl Signal {
    /// The signal number.
    pub fn number(self) -> libc::c_int {
        match self {
            Signal::Terminate => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Interrupt => libc::SIGINT,
            Signal::Hangup => libc::SIGHUP,
            Signal::Stop => libc::SIGSTOP,
            Signal::Continue => libc::SIGCONT,
        }
    }

    /// The signal's name, such as `SIGTERM`.
    pub fn name(self) -> &'static str {
        match self {
            Signal::Terminate => "SIGTERM",
            Signal::Kill => "SIGKILL",
            Signal::Interrupt => "SIGINT",
            Signal::Hangup => "SIGHUP",
            Signal::Stop => "SIGSTOP",
            Signal::Continue => "SIGCONT",
        }
    }
}

/// Send `signal` to process `pid`, if it is the caller's own.
///
/// Errors: [`Error::InvalidArgument`] for pid 0 or 1 or the caller's own
/// pid; [`Error::NotFound`] if there is no such process (or it exited
/// meanwhile); [`Error::NotAuthorized`] if its real uid isn't the caller's
/// or the kernel refuses (`EPERM`).
pub fn signal_process(pid: u32, signal: Signal) -> Result<()> {
    send_checked(pid, signal, None)
}

/// Ask process `pid` to quit (`SIGTERM`); see [`signal_process`].
pub fn terminate(pid: u32) -> Result<()> {
    signal_process(pid, Signal::Terminate)
}

/// End process `pid` at once (`SIGKILL`); see [`signal_process`].
pub fn kill(pid: u32) -> Result<()> {
    signal_process(pid, Signal::Kill)
}

/// Send `signal` to process `pid` only if it is still the one that started
/// at `start_ticks` (clock ticks after boot, as [`Process::start_ticks`]):
/// a pid reused since it was listed gets [`Error::NotFound`], not the
/// signal. For lists kept as plain data, without the [`Process`] at hand.
///
/// [`Process::start_ticks`]: super::Process::start_ticks
/// [`Process`]: super::Process
pub fn signal_started(pid: u32, start_ticks: u64, signal: Signal) -> Result<()> {
    send_checked(pid, signal, Some(start_ticks))
}

/// [`signal_process`], also refusing with [`Error::NotFound`] if the
/// process's start time (clock ticks since boot) isn't `start_ticks`: the
/// pid now belongs to another process.
pub(crate) fn send_checked(pid: u32, signal: Signal, start_ticks: Option<u64>) -> Result<()> {
    let raw = checked_pid(pid)?;
    let pidfd = open_pidfd(pid, raw)?;
    let gone = || Error::NotFound(format!("process {pid} has exited"));

    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).map_err(|_| gone())?;
    let uid = parse_status_uid(&status).ok_or_else(|| Error::Failed(format!("/proc/{pid}/status has no Uid line")))?;
    // SAFETY: getuid has no preconditions and cannot fail.
    if uid != unsafe { libc::getuid() } {
        return Err(Error::NotAuthorized(format!("process {pid} belongs to another user")));
    }
    if let Some(expected) = start_ticks {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|_| gone())?;
        if parse_stat(&stat).map(|s| s.start_ticks) != Some(expected) {
            return Err(gone());
        }
    }

    if let Some(fd) = &pidfd {
        // SAFETY: the fd is a valid pidfd we own; a null siginfo asks for
        // the same siginfo kill(2) would send.
        let sent = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                signal.number(),
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if sent == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOSYS) {
            return Err(signal_error(pid, signal, &error));
        }
    }
    // SAFETY: kill has no memory-safety preconditions.
    if unsafe { libc::kill(raw, signal.number()) } == 0 {
        Ok(())
    } else {
        Err(signal_error(pid, signal, &std::io::Error::last_os_error()))
    }
}

/// `pid` as a `pid_t`, refusing the ones never to be signalled from here.
fn checked_pid(pid: u32) -> Result<libc::pid_t> {
    if pid <= 1 {
        return Err(Error::InvalidArgument(format!("process {pid} can't be signalled")));
    }
    if pid == std::process::id() {
        return Err(Error::InvalidArgument("refusing to signal this process itself".into()));
    }
    libc::pid_t::try_from(pid).map_err(|_| Error::InvalidArgument(format!("{pid} is not a process ID")))
}

/// A pidfd for `pid`, or `None` where pidfds aren't available (old kernels,
/// seccomp filters) or `pid` is a thread rather than a process.
fn open_pidfd(pid: u32, raw: libc::pid_t) -> Result<Option<OwnedFd>> {
    // SAFETY: pidfd_open takes a pid and flags and returns a new fd or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, raw, 0) };
    if fd >= 0 {
        let Ok(fd) = libc::c_int::try_from(fd) else {
            return Ok(None);
        };
        // SAFETY: the kernel just gave us this fd, and nothing else owns it.
        return Ok(Some(unsafe { OwnedFd::from_raw_fd(fd) }));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Err(Error::NotFound(format!("no process {pid}")));
    }
    Ok(None)
}

fn signal_error(pid: u32, signal: Signal, error: &std::io::Error) -> Error {
    match error.raw_os_error() {
        Some(libc::ESRCH) => Error::NotFound(format!("process {pid} has exited")),
        Some(libc::EPERM) => Error::NotAuthorized(format!("not allowed to send {} to process {pid}", signal.name())),
        _ => Error::Failed(format!("sending {} to process {pid}: {error}", signal.name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    #[test]
    fn init_nothing_and_ourselves_are_refused() {
        assert!(matches!(terminate(0), Err(Error::InvalidArgument(_))));
        assert!(matches!(kill(1), Err(Error::InvalidArgument(_))));
        assert!(matches!(signal_process(std::process::id(), Signal::Stop), Err(Error::InvalidArgument(_))));
        assert!(matches!(signal_process(u32::MAX, Signal::Terminate), Err(Error::InvalidArgument(_))));
    }

    #[test]
    fn signals_have_their_numbers() {
        assert_eq!(Signal::Terminate.number(), 15);
        assert_eq!(Signal::Kill.number(), 9);
        assert_eq!(Signal::Hangup.name(), "SIGHUP");
    }

    fn sleeper() -> Child {
        Command::new("sleep").arg("30").spawn().expect("run sleep")
    }

    fn state_of(pid: u32) -> Option<char> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        parse_stat(&text).map(|s| s.state)
    }

    fn wait_for_state(pid: u32, state: char) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if state_of(pid) == Some(state) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    #[ignore = "starts and signals a process on this machine"]
    fn live_own_children_can_be_stopped_resumed_and_ended() {
        let mut child = sleeper();
        let pid = child.id();
        signal_process(pid, Signal::Stop).unwrap();
        assert!(wait_for_state(pid, 'T'), "stopped");
        signal_process(pid, Signal::Continue).unwrap();
        assert!(wait_for_state(pid, 'S'), "resumed");
        terminate(pid).unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGTERM));
        // Reaped: gone.
        assert!(matches!(kill(pid), Err(Error::NotFound(_))));

        let mut child = sleeper();
        kill(child.id()).unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    #[test]
    #[ignore = "starts and signals a process on this machine"]
    fn live_a_stale_start_time_is_refused() {
        let mut child = sleeper();
        let pid = child.id();
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let start = parse_stat(&text).unwrap().start_ticks;
        assert!(matches!(send_checked(pid, Signal::Kill, Some(start + 1)), Err(Error::NotFound(_))));
        send_checked(pid, Signal::Kill, Some(start)).unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_other_users_processes_are_refused() {
        // SAFETY: getuid has no preconditions.
        let me = unsafe { libc::getuid() };
        let other = std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter(|&pid| pid > 1)
            .find(|pid| {
                std::fs::read_to_string(format!("/proc/{pid}/status"))
                    .ok()
                    .and_then(|s| parse_status_uid(&s))
                    .is_some_and(|uid| uid != me)
            });
        match other {
            Some(pid) => {
                let result = terminate(pid);
                println!("terminate({pid}) -> {result:?}");
                assert!(matches!(result, Err(Error::NotAuthorized(_)) | Err(Error::NotFound(_))));
            }
            None => println!("no other user's process is visible; nothing to check"),
        }
        // A pid that isn't in use.
        let max: u32 = std::fs::read_to_string("/proc/sys/kernel/pid_max")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(32768);
        let unused = (2..max).rev().find(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists()).unwrap();
        assert!(matches!(terminate(unused), Err(Error::NotFound(_))));
    }
}
