// SPDX-License-Identifier: MIT OR Apache-2.0
//! Running programs for the helper, and stopping when Yukimi asks.
//!
//! A stop is a phase of the whole process: once asked for, no new program
//! starts and the running one is sent SIGTERM. Two kinds of step change
//! that. After the commit ([`change::RestorePoint::attempt`]) the work
//! finishes whatever happens, so that the system is never left half
//! switched. An [`Unstoppable`] step (deleting generations and updating the
//! boot menu, [`clean`]) finishes first, and a stop asked for meanwhile
//! takes effect when it ends.
//!
//! [`change::RestorePoint::attempt`]: crate::change::RestorePoint::attempt
//! [`clean`]: crate::clean
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// pkexec starts programs with an almost empty environment.
pub const PATH: &str = "/run/current-system/sw/bin:/run/wrappers/bin";

/// A failure, in words for the person who asked.
#[derive(Debug)]
pub struct Failure(pub String);

impl<E: std::fmt::Display> From<E> for Failure {
    fn from(e: E) -> Failure {
        Failure(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Failure>;

/// Where the work is, which decides whether it can still be stopped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Working,
    /// Yukimi asked to stop.
    Stopping,
    /// A step that a stop must not split is running; `asked` says whether a
    /// stop waits for it to end.
    Unstoppable {
        asked: bool,
    },
    /// The new system is being started: too late to stop.
    Committed,
}

/// The phase, and the program running now (to stop when asked).
static STATE: Mutex<(Phase, Option<u32>)> = Mutex::new((Phase::Working, None));
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

fn state() -> std::sync::MutexGuard<'static, (Phase, Option<u32>)> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Ask the work to stop.
pub fn stop() {
    let mut state = state();
    match state.0 {
        Phase::Committed => return,
        Phase::Unstoppable { .. } => {
            state.0 = Phase::Unstoppable { asked: true };
            return;
        }
        Phase::Working | Phase::Stopping => state.0 = Phase::Stopping,
    }
    if let Some(pid) = state.1 {
        // SAFETY: signalling a process has no memory-safety preconditions.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    }
}

/// Whether a stop was asked for (and hasn't been refused by a commit).
pub fn stop_requested() -> bool {
    matches!(state().0, Phase::Stopping | Phase::Unstoppable { asked: true })
}

/// Whether the work is past its commit, so it no longer puts anything back.
pub fn committed() -> bool {
    state().0 == Phase::Committed
}

/// Stop when standard input closes, if it is a pipe: that is how Yukimi
/// asks. Standard input that is a terminal or /dev/null means nothing.
pub fn watch_for_stop() {
    // Terminal cancellation must follow the same rollback path as closing
    // the GUI's pipe, not abruptly exit with a half-edited configuration.
    // SAFETY: the signal handler only stores to a lock-free atomic.
    unsafe {
        libc::signal(libc::SIGINT, interrupt as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, interrupt as *const () as libc::sighandler_t);
    }
    std::thread::spawn(|| {
        loop {
            if INTERRUPTED.swap(false, Ordering::Relaxed) {
                stop();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });
    // SAFETY: fstat writes only into the struct it is given.
    let is_pipe = unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        libc::fstat(0, &mut stat) == 0 && (stat.st_mode & libc::S_IFMT) == libc::S_IFIFO
    };
    if !is_pipe {
        return;
    }
    std::thread::spawn(|| {
        let mut buffer = [0u8; 64];
        while matches!(std::io::stdin().read(&mut buffer), Ok(n) if n > 0) {}
        stop();
    });
}

fn check_stopped() -> Result<()> {
    if state().0 == Phase::Stopping { Err(Failure("Stopped".into())) } else { Ok(()) }
}

/// From here on the work finishes even if Yukimi goes away, so that the
/// system is never left half switched. Only a restore point's
/// [`attempt`](crate::change::RestorePoint::attempt) commits.
pub(crate) fn commit() -> Result<()> {
    let mut state = state();
    match state.0 {
        Phase::Stopping | Phase::Unstoppable { asked: true } => Err(Failure("Stopped".into())),
        Phase::Working | Phase::Unstoppable { asked: false } | Phase::Committed => {
            state.0 = Phase::Committed;
            Ok(())
        }
    }
}

/// A step that a stop doesn't split, while this lives: its programs aren't
/// sent SIGTERM, and a stop asked for meanwhile takes effect when it ends.
#[must_use = "the step is unstoppable only while this lives"]
pub(crate) struct Unstoppable {
    /// Only the outermost one ends the step.
    outer: bool,
}

impl Unstoppable {
    /// Starts the step, unless a stop was asked for already.
    pub(crate) fn begin() -> Result<Unstoppable> {
        let mut state = state();
        match state.0 {
            Phase::Stopping => Err(Failure("Stopped".into())),
            Phase::Working => {
                state.0 = Phase::Unstoppable { asked: false };
                Ok(Unstoppable { outer: true })
            }
            Phase::Unstoppable { .. } | Phase::Committed => Ok(Unstoppable { outer: false }),
        }
    }
}

impl Drop for Unstoppable {
    fn drop(&mut self) {
        let mut state = state();
        if let (true, Phase::Unstoppable { asked }) = (self.outer, state.0) {
            state.0 = if asked { Phase::Stopping } else { Phase::Working };
        }
    }
}

/// Start a command, remembering it so it can be stopped, and wait for it.
pub fn wait(c: &mut Command) -> Result<std::process::Output> {
    let child = {
        let mut state = state();
        if state.0 == Phase::Stopping {
            return Err(Failure("Stopped".into()));
        }
        let child = c.spawn()?;
        state.1 = Some(child.id());
        child
    };
    let out = child.wait_with_output();
    state().1 = None;
    check_stopped()?;
    Ok(out?)
}

/// Say what is happening, as an activity Yukimi shows.
pub fn stage(id: u64, text: &str) {
    eprintln!(
        "@nix {}",
        serde_json::json!({"action": "start", "id": id, "level": 0, "type": 0, "text": text, "fields": []})
    );
}

pub fn stage_done(id: u64) {
    eprintln!("@nix {}", serde_json::json!({"action": "stop", "id": id}));
}

pub fn report(level: u8, message: &str) {
    eprintln!("@nix {}", serde_json::json!({"action": "msg", "level": level, "msg": message}));
}

pub fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    // Only the helper handles terminal cancellation. In particular, an
    // activation already committed must finish even when Ctrl-C is pressed.
    c.process_group(0);
    c.env_clear().env("PATH", PATH).env("HOME", "/root").env("LANG", "C.UTF-8").stdin(Stdio::null());
    if let Ok(archive) = std::fs::read_link("/run/current-system/sw/lib/locale/locale-archive") {
        c.env("LOCALE_ARCHIVE", archive);
    }
    c
}

pub fn nix() -> Command {
    let mut c = command("nix");
    c.args(["--extra-experimental-features", "nix-command flakes"]);
    c
}

/// Run a command whose error output goes straight to Yukimi.
pub fn run(mut c: Command, what: &str) -> Result<()> {
    let out = wait(c.stdout(Stdio::inherit()).stderr(Stdio::inherit()))?;
    if out.status.success() { Ok(()) } else { Err(Failure(format!("{what} failed"))) }
}

/// Run a command and keep its standard output.
pub fn output(mut c: Command, what: &str) -> Result<String> {
    let out = wait(c.stdout(Stdio::piped()).stderr(Stdio::inherit()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Failure(format!("{what} failed")))
    }
}

/// Tests that move the process-wide phase hold this: they run in parallel.
#[cfg(test)]
pub(crate) fn phase_for_test() -> std::sync::MutexGuard<'static, ()> {
    static TESTS: Mutex<()> = Mutex::new(());
    let guard = TESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state().0 = Phase::Working;
    guard
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_refuses_the_commit_and_a_commit_absorbs_stops() {
        let _phase = phase_for_test();
        stop();
        assert!(stop_requested() && commit().is_err() && check_stopped().is_err());
        state().0 = Phase::Working;
        commit().unwrap();
        stop();
        assert!(committed() && !stop_requested());
        state().0 = Phase::Working;
    }

    #[test]
    fn an_unstoppable_step_defers_a_stop_until_it_ends() {
        let _phase = phase_for_test();
        {
            let _step = Unstoppable::begin().unwrap();
            stop();
            assert!(stop_requested());
            // Its programs still run.
            assert!(check_stopped().is_ok());
            let _inner = Unstoppable::begin().unwrap();
        }
        assert!(check_stopped().is_err(), "the stop applies once the step ends");
        assert!(Unstoppable::begin().is_err() && commit().is_err());
        state().0 = Phase::Working;
        {
            let _step = Unstoppable::begin().unwrap();
        }
        assert!(!stop_requested());
        commit().unwrap();
        stop();
        assert!(committed() && !stop_requested());
        state().0 = Phase::Working;
    }
}
