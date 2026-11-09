//! Chanoma starting again after it crashes, saying what happened, so
//! nobody needs Ctrl+Alt+Delete to get the television back.
//!
//! `aizome-chanoma` watches itself: the process started (the watcher)
//! runs Chanoma as its child and waits. When the child dies of a crash (a
//! game's core faulting, say, or a panic on Chanoma's own thread) the
//! watcher starts it again, with what happened in `CHANOMA_AFTER_CRASH`,
//! which Chanoma shows as it comes back. Leaving, logging out, a signal to
//! stop (passed on to the child) and a force quit (SIGKILL) end the
//! watcher too; three crashes in two minutes end it rather than loop.
//!
//! In the child, a crash's handler adds the signal to the note the
//! watcher reads (`CHANOMA_WATCH_NOTE`, beside what's being played, which
//! a game's session writes there) and leaves no core dump, so the window
//! goes at once, rather than standing frozen while a gigabyte's dump is
//! written. `CHANOMA_CORE_DUMPS=1` keeps them, for looking into a crash.

use std::ffi::{OsString, c_int, c_void};
use std::os::fd::IntoRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

/// The child's note, where its crash is written down.
pub const NOTE: &str = "CHANOMA_WATCH_NOTE";
/// What happened, given to a child started again after a crash.
pub const AFTER_CRASH: &str = "CHANOMA_AFTER_CRASH";
/// `1` keeps core dumps of crashes.
pub const CORE_DUMPS: &str = "CHANOMA_CORE_DUMPS";

/// Signals a crash dies of.
const CRASHES: [c_int; 7] =
    [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT, libc::SIGSYS, libc::SIGTRAP];
/// Crashes this close together aren't started again: something keeps
/// crashing it, and the television would only flicker.
const TOO_MANY: usize = 3;
const WITHIN: Duration = Duration::from_secs(120);

/// Whether this is the child, watched.
pub fn watched() -> bool {
    std::env::var_os(NOTE).is_some()
}

/// How a child ended, for the watcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// Of itself, or stopped: the watcher ends with this code.
    Exit(i32),
    /// A crash: by this signal, or 0 for a panic.
    Crashed(i32),
}

pub fn ended(status: ExitStatus) -> Ended {
    if let Some(sig) = status.signal() {
        return if CRASHES.contains(&sig) { Ended::Crashed(sig) } else { Ended::Exit(128 + sig) };
    }
    match status.code() {
        // A panic on Chanoma's own thread.
        Some(101) => Ended::Crashed(0),
        Some(code) => Ended::Exit(code),
        None => Ended::Exit(1),
    }
}

fn signal_name(sig: c_int) -> &'static str {
    match sig {
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGBUS => "SIGBUS",
        libc::SIGILL => "SIGILL",
        libc::SIGFPE => "SIGFPE",
        libc::SIGABRT => "SIGABRT",
        libc::SIGSYS => "SIGSYS",
        libc::SIGTRAP => "SIGTRAP",
        0 => "a panic",
        _ => "a signal",
    }
}

/// What happened, said for whoever's on the sofa, from the note: the
/// game's player and the game, if one was being played.
pub fn told(note: &str) -> String {
    let mut lines = note.lines().filter(|l| !l.starts_with("signal "));
    match (lines.next(), lines.next()) {
        (Some(core), Some(title)) if !core.is_empty() && !title.is_empty() => format!(
            "It closed suddenly while {core} was playing {title}: the game's player crashed. \
             Play since the game last saved may be lost."
        ),
        _ => "It closed suddenly, and started again by itself.".into(),
    }
}

/// Where the watcher's child keeps its note.
fn note_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("aizome-chanoma-{}.note", std::process::id()))
}

/// Watch Chanoma: this program again, with its arguments. The result is
/// the code to exit with.
pub fn run() -> i32 {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            log::error!("chanoma: can't find itself to start: {e}");
            return 1;
        }
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    run_as(&exe, &args, &note_path())
}

/// Watch `exe` with `args`, its note at `note`.
pub fn run_as(exe: &Path, args: &[OsString], note: &Path) -> i32 {
    let child = Arc::new(AtomicI32::new(0));
    let stopping = Arc::new(AtomicBool::new(false));
    pass_on_signals(child.clone(), stopping.clone());
    let mut crashes: Vec<Instant> = Vec::new();
    let mut after: Option<String> = None;
    let code = loop {
        if stopping.load(Ordering::Acquire) {
            break 0;
        }
        let _ = std::fs::write(note, "");
        let mut cmd = Command::new(exe);
        cmd.args(args).env(NOTE, note);
        match &after {
            Some(a) => cmd.env(AFTER_CRASH, a),
            None => cmd.env_remove(AFTER_CRASH),
        };
        let mut c = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                log::error!("chanoma: {}: {e}", exe.display());
                break 1;
            }
        };
        child.store(c.id() as i32, Ordering::Release);
        let status = c.wait();
        child.store(0, Ordering::Release);
        let Ok(status) = status else { break 1 };
        match ended(status) {
            Ended::Exit(code) => break code,
            Ended::Crashed(_) if stopping.load(Ordering::Acquire) => break 0,
            Ended::Crashed(sig) => {
                let said = std::fs::read_to_string(note).unwrap_or_default();
                let what = said.lines().take(2).collect::<Vec<_>>().join(", ");
                let now = Instant::now();
                crashes.retain(|t| now.duration_since(*t) < WITHIN);
                crashes.push(now);
                if crashes.len() >= TOO_MANY {
                    log::error!(
                        "chanoma: crashed ({}) {TOO_MANY} times in two minutes; not starting it again",
                        signal_name(sig)
                    );
                    break 1;
                }
                log::warn!("chanoma: crashed ({}) [{what}]; starting it again", signal_name(sig));
                after = Some(told(&said));
            }
        }
    };
    let _ = std::fs::remove_file(note);
    code
}

/// Signals to stop (blocked in every thread, `signals::block`) go on to
/// the child, which puts things away; none is started after one.
fn pass_on_signals(child: Arc<AtomicI32>, stopping: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new().name("chanoma-watch".into()).spawn(move || {
        let set = crate::signals::set();
        loop {
            let mut sig = 0;
            // SAFETY: waiting for signals blocked in every thread.
            if unsafe { libc::sigwait(&set, &mut sig) } != 0 {
                continue;
            }
            stopping.store(true, Ordering::Release);
            let pid = child.load(Ordering::Acquire);
            if pid > 0 {
                // SAFETY: a signal to the child, still unreaped (its pid is
                // cleared before it's waited for no more).
                unsafe { libc::kill(pid, sig) };
            }
        }
    });
}

static NOTE_FD: AtomicI32 = AtomicI32::new(-1);
static KEEP_DUMPS: AtomicBool = AtomicBool::new(false);

/// In the child: a crash adds its signal to the note and leaves no core
/// dump. Cores that handle faults of their own (Flycast's and PCSX2's
/// fast memory) put theirs before this one, and hand it what they don't.
pub fn guard() {
    let Some(path) = std::env::var_os(NOTE) else { return };
    let Ok(file) = std::fs::OpenOptions::new().append(true).open(&path) else { return };
    NOTE_FD.store(file.into_raw_fd(), Ordering::Release);
    KEEP_DUMPS.store(std::env::var(CORE_DUMPS).is_ok_and(|v| v == "1"), Ordering::Release);
    for sig in CRASHES {
        // SAFETY: a zeroed sigaction filled in, for a handler that only
        // makes async-signal-safe calls.
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_crash as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_RESETHAND;
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
    }
}

extern "C" fn on_crash(sig: c_int, _info: *mut libc::siginfo_t, _context: *mut c_void) {
    let fd = NOTE_FD.load(Ordering::Acquire);
    if fd >= 0 {
        let mut line = *b"signal 000\n";
        let n = sig.clamp(0, 999) as u8;
        line[7] = b'0' + n / 100;
        line[8] = b'0' + n / 10 % 10;
        line[9] = b'0' + n % 10;
        // SAFETY: write(2) is async-signal-safe.
        unsafe { libc::write(fd, line.as_ptr().cast(), line.len()) };
    }
    // SAFETY: prctl, signal and raise are system calls, safe here.
    unsafe {
        if !KEEP_DUMPS.load(Ordering::Acquire) {
            libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        }
        // Its default again (as SA_RESETHAND made it), raised again: the
        // child dies of it, which the watcher sees.
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// What a game's session plays, for the note a crash leaves.
pub fn playing(core: &str, title: &str) {
    if let Some(path) = std::env::var_os(NOTE) {
        let one = |s: &str| s.replace(['\n', '\r'], " ");
        let _ = std::fs::write(path, format!("{}\n{}\n", one(core), one(title)));
    }
}

/// Nothing's being played.
pub fn stopped() {
    if let Some(path) = std::env::var_os(NOTE) {
        let _ = std::fs::write(path, "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crashes_are_told_from_how_it_ended_and_what_was_played() {
        assert_eq!(ended(ExitStatus::from_raw(libc::SIGSEGV)), Ended::Crashed(libc::SIGSEGV));
        assert_eq!(ended(ExitStatus::from_raw(libc::SIGABRT)), Ended::Crashed(libc::SIGABRT));
        // A force quit and a stop are someone's choice.
        assert_eq!(ended(ExitStatus::from_raw(libc::SIGKILL)), Ended::Exit(128 + libc::SIGKILL));
        assert_eq!(ended(ExitStatus::from_raw(libc::SIGTERM)), Ended::Exit(128 + libc::SIGTERM));
        assert_eq!(ended(ExitStatus::from_raw(0)), Ended::Exit(0));
        assert_eq!(ended(ExitStatus::from_raw(101 << 8)), Ended::Crashed(0));
        assert_eq!(
            told("Flycast\nCrazy Taxi\nsignal 011\n"),
            "It closed suddenly while Flycast was playing Crazy Taxi: the game's player crashed. \
             Play since the game last saved may be lost."
        );
        assert_eq!(told("signal 011\n"), "It closed suddenly, and started again by itself.");
        assert_eq!(told(""), "It closed suddenly, and started again by itself.");
    }
}
