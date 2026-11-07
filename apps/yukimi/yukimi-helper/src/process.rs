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
//! The helper runs only the programs this crate names ([`Program`]). Those
//! that change what a restore point or the boot menu must cover
//! (`nix-channel`, `nix-store`, `nix-env`, `nix flake update`) are named
//! only in the step that covers them, so the helper's program can't run
//! them around that step. Running a program by name doesn't compile:
//!
//! ```compile_fail,E0308
//! use yukimi_helper::process::{command, run};
//! # fn main() -> yukimi_helper::process::Result<()> {
//! // Update root's channels outside the change's restore point.
//! let mut channels = command("nix-channel");
//! channels.arg("--update");
//! run(channels, "Updating the channels")?;
//! # Ok(()) }
//! ```
//!
//! nor naming one that isn't offered:
//!
//! ```compile_fail,E0624
//! use yukimi_helper::process::{Program, command, wait};
//! # fn main() -> yukimi_helper::process::Result<()> {
//! // Collect garbage before the boot menu is rewritten.
//! wait(command(Program::new("nix-store")).arg("--gc"))?;
//! # Ok(()) }
//! ```
//!
//! ```no_run
//! use yukimi_helper::process::{Program, command, output};
//! # fn main() -> yukimi_helper::process::Result<()> {
//! let mut users = command(Program::LOGINCTL);
//! users.args(["list-users", "--no-legend"]);
//! let _ = output(users, "Listing signed-in users")?;
//! # Ok(()) }
//! ```
//!
//! `std::process::Command::new` is no way around it (Clippy's
//! `disallowed_methods`, see `clippy.toml`): the helper's programs forbid
//! the lint, so allowing it there is an error (E0453), and this library
//! denies it, allowing it only in the one function that makes commands
//! (which the helper's Nix check enforces).
//!
//! [`change::RestorePoint::attempt`]: crate::change::RestorePoint::attempt
//! [`clean`]: crate::clean
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use yukimi_system::sys;

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

/// A program the helper may run, found on [`PATH`]. Only this crate names
/// programs; its program gets the ones that change nothing a restore point
/// or the boot menu must cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program(&'static str);

impl Program {
    /// systemd's manager: unit state, and services in users' sessions.
    pub const SYSTEMCTL: Program = Program("systemctl");
    /// logind: who is signed in.
    pub const LOGINCTL: Program = Program("loginctl");
    /// Starts the operation as a service of its own.
    pub const SYSTEMD_RUN: Program = Program("systemd-run");

    /// Names a program, for the step of this crate that runs it.
    pub(crate) const fn new(name: &'static str) -> Program {
        Program(name)
    }
}

/// What the helper's program asks the `nix` command. None of it changes
/// the store's roots, root's channels or the configuration; `nix flake
/// update` is run only by a change that covers flake.lock
/// ([`Changing::update_lock`](crate::change::Changing::update_lock)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nix {
    Build,
    Eval,
    Config,
}

/// The lock that keeps Yukimi's operations, from the GUI, the terminal or
/// a recovery, from running at once. The kernel releases it even after a
/// crash.
pub const LOCK: &str = "/run/lock/yukimi.lock";

/// The one privileged operation this process runs, holding [`LOCK`]. A
/// change and a clean-up borrow it for as long as they can still change the
/// system, so that one can't start inside or alongside another in this
/// process, and the lock keeps other processes out; only the first
/// [`begin`](Self::begin) in a process gets it.
#[derive(Debug)]
pub struct Operation {
    _lock: Option<std::fs::File>,
}

static BEGUN: AtomicBool = AtomicBool::new(false);

impl Operation {
    /// Takes [`LOCK`], unless another operation holds it.
    pub fn begin() -> Result<Operation> {
        Operation::begin_at(Path::new(LOCK))
    }

    /// The same with another lock file.
    pub fn begin_at(lock: &Path) -> Result<Operation> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock)
            .and_then(|file| {
                file.try_lock()?;
                Ok(file)
            })
            .map_err(|e| Failure(format!("Another Yukimi operation is running, or its lock is unavailable: {e}")))?;
        if BEGUN.swap(true, Ordering::SeqCst) {
            return Err(Failure("This process has run its operation already".into()));
        }
        Ok(Operation { _lock: Some(file) })
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Operation {
        Operation { _lock: None }
    }
}

/// Where the work is, which decides whether it can still be stopped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Working,
    /// Yukimi asked to stop.
    Stopping,
    /// A step that a stop must not split is running: what it is finishing,
    /// and whether a stop waits for it to end.
    Unstoppable {
        finishing: &'static str,
        asked: bool,
    },
    /// The new system is being started: too late to stop.
    Committed,
}

/// The phase, and the program running now (to stop when asked).
static STATE: Mutex<(Phase, Option<u32>)> = Mutex::new((Phase::Working, None));
fn state() -> std::sync::MutexGuard<'static, (Phase, Option<u32>)> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Ask the work to stop.
pub fn stop() {
    let mut state = state();
    match state.0 {
        Phase::Committed => return,
        Phase::Unstoppable { finishing, asked } => {
            state.0 = Phase::Unstoppable { finishing, asked: true };
            drop(state);
            if !asked {
                report(1, &format!("{finishing} first, then stopping"));
            }
            return;
        }
        Phase::Working | Phase::Stopping => state.0 = Phase::Stopping,
    }
    if let Some(pid) = state.1 {
        sys::terminate(pid);
    }
}

/// Whether a stop was asked for (and hasn't been refused by a commit).
pub fn stop_requested() -> bool {
    matches!(state().0, Phase::Stopping | Phase::Unstoppable { asked: true, .. })
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
    sys::catch_sigint();
    sys::catch_sigterm();
    std::thread::spawn(|| {
        loop {
            if sys::take_interrupt() {
                stop();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });
    if !sys::stdin_is_pipe() {
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
/// [`attempt`](crate::change::RestorePoint::attempt) commits, once.
pub(crate) fn commit() -> Result<()> {
    let mut state = state();
    match state.0 {
        Phase::Stopping | Phase::Unstoppable { asked: true, .. } => Err(Failure("Stopped".into())),
        Phase::Committed => Err(Failure("The operation committed already".into())),
        Phase::Working | Phase::Unstoppable { asked: false, .. } => {
            state.0 = Phase::Committed;
            Ok(())
        }
    }
}

/// A step that a stop doesn't split, while this lives: its programs aren't
/// sent SIGTERM, and a stop asked for meanwhile takes effect when it ends.
#[must_use = "the step is unstoppable only while this lives"]
#[derive(Debug)]
pub(crate) struct Unstoppable {
    /// Only the outermost one ends the step.
    outer: bool,
}

impl Unstoppable {
    /// Starts the step, unless a stop was asked for already. `finishing`
    /// says what a stop waits for ("Deleting the old generations and
    /// updating the boot menu").
    pub(crate) fn begin(finishing: &'static str) -> Result<Unstoppable> {
        let mut state = state();
        match state.0 {
            Phase::Stopping => Err(Failure("Stopped".into())),
            Phase::Working => {
                state.0 = Phase::Unstoppable { finishing, asked: false };
                Ok(Unstoppable { outer: true })
            }
            Phase::Unstoppable { .. } | Phase::Committed => Ok(Unstoppable { outer: false }),
        }
    }
}

impl Drop for Unstoppable {
    fn drop(&mut self) {
        let mut state = state();
        if let (true, Phase::Unstoppable { asked, .. }) = (self.outer, state.0) {
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

/// A line of Nix's log format on standard error. Writing it never fails
/// the work, and never panics: a full disk under the log must not abort a
/// step that is putting things back.
fn log(event: serde_json::Value) {
    let _ = writeln!(std::io::stderr(), "@nix {event}");
}

/// Say what is happening, as an activity Yukimi shows.
pub fn stage(id: u64, text: &str) {
    log(serde_json::json!({"action": "start", "id": id, "level": 0, "type": 0, "text": text, "fields": []}));
}

pub fn stage_done(id: u64) {
    log(serde_json::json!({"action": "stop", "id": id}));
}

pub fn report(level: u8, message: &str) {
    log(serde_json::json!({"action": "msg", "level": level, "msg": message}));
}

/// Prepares to run `program` as the helper runs everything: with a clean
/// environment, and in a process group of its own.
pub fn command(program: Program) -> Command {
    prepared(Path::new(program.0))
}

/// The same for a program given by its path, such as a system's
/// `switch-to-configuration`.
pub(crate) fn prepared(program: &Path) -> Command {
    let mut c = new_command(program);
    // Only the helper handles terminal cancellation. In particular, an
    // activation already committed must finish even when Ctrl-C is pressed.
    c.process_group(0);
    c.env_clear().env("PATH", PATH).env("HOME", "/root").env("LANG", "C.UTF-8").stdin(Stdio::null());
    if let Ok(archive) = std::fs::read_link("/run/current-system/sw/lib/locale/locale-archive") {
        c.env("LOCALE_ARCHIVE", archive);
    }
    c
}

/// The one place the helper and its terminal client make a command. The
/// programs forbid the lint this allows, and the Nix check allows it only
/// here.
#[allow(clippy::disallowed_methods)]
fn new_command(program: &Path) -> Command {
    Command::new(program)
}

/// The helper beside the running program, through pkexec unless this
/// process is root already: how the terminal client starts an operation.
/// Never a helper found on PATH, and never another program.
pub fn helper_client() -> std::io::Result<Command> {
    let helper = std::env::current_exe()?.with_file_name("yukimi-helper");
    if sys::is_root() {
        return Ok(new_command(&helper));
    }
    let mut pkexec = new_command(Path::new("/run/wrappers/bin/pkexec"));
    pkexec.arg(helper);
    Ok(pkexec)
}

/// `nix <subcommand>`, with flakes enabled.
pub fn nix(subcommand: Nix) -> Command {
    let mut c = command(Program::new("nix"));
    c.args(["--extra-experimental-features", "nix-command flakes"]);
    c.arg(match subcommand {
        Nix::Build => "build",
        Nix::Eval => "eval",
        Nix::Config => "config",
    });
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
        assert!(commit().is_err(), "an operation commits once");
        state().0 = Phase::Working;
    }

    #[test]
    fn an_unstoppable_step_defers_a_stop_until_it_ends() {
        let _phase = phase_for_test();
        {
            let _step = Unstoppable::begin("Finishing").unwrap();
            stop();
            assert!(stop_requested());
            // Its programs still run.
            assert!(check_stopped().is_ok());
            let _inner = Unstoppable::begin("Finishing").unwrap();
        }
        assert!(check_stopped().is_err(), "the stop applies once the step ends");
        assert!(Unstoppable::begin("Finishing").is_err() && commit().is_err());
        state().0 = Phase::Working;
        {
            let _step = Unstoppable::begin("Finishing").unwrap();
        }
        assert!(!stop_requested());
        commit().unwrap();
        stop();
        assert!(committed() && !stop_requested());
        state().0 = Phase::Working;
    }
}
