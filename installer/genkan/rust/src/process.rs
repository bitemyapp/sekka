// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed-argument subprocesses. Call only from a worker, never the GUI thread.
use anyhow::{Context, Result, ensure};
use std::os::unix::process::CommandExt;
use std::{
    fmt,
    io::Read,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
    thread,
    time::{Duration, Instant},
};

/// Whether preparation may still be abandoned. A latch starts open; the
/// GUI's withdrawal closes it as cancelled, the installation's start as
/// committed, and whichever comes first wins: a committed installation can't
/// be cancelled, and a cancelled preparation can't be committed to. A helper
/// process prepares once, so its latch never reopens.
pub(crate) struct Latch {
    state: AtomicU8,
    /// Set once the latch is cancelled: what subprocesses and cache warming
    /// poll.
    cancel: AtomicBool,
}

const OPEN: u8 = 0;
const CANCELLED: u8 = 1;
const COMMITTED: u8 = 2;

impl Latch {
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(OPEN),
            cancel: AtomicBool::new(false),
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Abandons preparation: running and later subprocesses stop. Refused,
    /// returning false, once the installation is committed.
    pub fn cancel(&self) -> bool {
        let won = self
            .state
            .compare_exchange(OPEN, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if won {
            self.cancel.store(true, Ordering::Relaxed);
        }
        won
    }

    /// Commits to the installation: nothing can cancel it from now on. Fails
    /// if preparation was cancelled first.
    pub fn commit(&self) -> Result<()> {
        match self
            .state
            .compare_exchange(OPEN, COMMITTED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) | Err(COMMITTED) => Ok(()),
            Err(_) => Err(Cancelled.into()),
        }
    }

    /// The flag cache warming polls.
    pub(crate) fn flag(&self) -> &AtomicBool {
        &self.cancel
    }
}

impl Default for Latch {
    fn default() -> Self {
        Self::new()
    }
}

/// This process's preparation.
pub(crate) static LATCH: Latch = Latch::new();

pub fn cancelled() -> bool {
    LATCH.cancelled()
}

/// Preparation stopped because it was cancelled, as opposed to failing.
#[derive(Debug)]
pub struct Cancelled;
impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Preparation was cancelled")
    }
}
impl std::error::Error for Cancelled {}

/// Whether `error` is a cancellation, beneath whatever context it gathered.
pub fn is_cancellation(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<Cancelled>())
}

pub use crate::storage::Program;

/// A command for `program`, with the tool path and nothing else of the
/// caller's environment.
// The one place the installer starts a program (clippy.toml).
#[allow(clippy::disallowed_methods)]
pub fn command(program: Program) -> Command {
    let mut command = Command::new(program.name());
    command
        .stdin(Stdio::null())
        .env_clear()
        .env(
            "PATH",
            option_env!("CALAMARES_TOOL_PATH")
                .unwrap_or("/run/current-system/sw/bin:/usr/bin:/bin"),
        )
        .env("HOME", "/root")
        .env("LC_ALL", "C.UTF-8")
        .env("NIX_REMOTE", "daemon");
    command
}

/// The installation helper, started from the window through Polkit's pkexec:
/// the only other program the installer starts, and no disk tool.
// The window's one start of a program (clippy.toml).
#[allow(clippy::disallowed_methods)]
pub(crate) fn pkexec(helper: &std::path::Path) -> Command {
    let mut command = Command::new(option_env!("CALAMARES_PKEXEC").unwrap_or("pkexec"));
    command.arg(helper);
    command
}

pub fn output(program: Program, args: &[&str], seconds: u64) -> Result<String> {
    run(
        command(program),
        program.name(),
        args,
        seconds,
        None,
        Capture::Tail,
    )
}

/// Secret input is an anonymous memory file, never argv, environment or a path.
/// The file descriptor is closed when the child exits; callers own zeroizing buffers.
pub(crate) fn output_secret(
    program: Program,
    args: &[&str],
    secret: &[u8],
    seconds: u64,
) -> Result<String> {
    use std::io::{Seek, Write};
    let mut input = crate::sys::memory_file(c"genkan-secret")
        .map_err(|error| anyhow::anyhow!("Could not allocate secret input: {error}"))?;
    input.write_all(secret)?;
    input.rewind()?;
    let mut cmd = command(program);
    cmd.stdin(Stdio::from(input));
    run(cmd, program.name(), args, seconds, None, Capture::Tail)
}

/// As `output`, but returns all of stdout (up to 256 MiB) instead of its tail.
/// For machine-readable listings such as a system's closure.
pub fn output_full(program: Program, args: &[&str], seconds: u64) -> Result<String> {
    run(
        command(program),
        program.name(),
        args,
        seconds,
        None,
        Capture::Full,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Capture {
    /// Keep a bounded tail: diagnostics for arbitrarily chatty tools.
    Tail,
    /// Keep everything up to FULL_LIMIT, failing beyond it.
    Full,
}
const FULL_LIMIT: usize = 256 << 20;

/// As `output`, also passing each complete stderr line (up to 64 KiB) to
/// `on_line` as it arrives, e.g. Nix `--log-format internal-json` progress.
pub fn output_lines(
    program: Program,
    args: &[&str],
    seconds: u64,
    on_line: &(dyn Fn(&str) + Sync),
) -> Result<String> {
    run(
        command(program),
        program.name(),
        args,
        seconds,
        Some(on_line),
        Capture::Tail,
    )
}

fn run(
    mut command: Command,
    program: &str,
    args: &[&str],
    seconds: u64,
    on_line: Option<&(dyn Fn(&str) + Sync)>,
    capture: Capture,
) -> Result<String> {
    if cancelled() {
        return Err(Cancelled.into());
    }
    let mut child = command
        .args(args)
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Starting {program}"))?;
    // Drain concurrently and cap retained output. Nix builds can produce many MB.
    fn drain(
        mut pipe: impl Read,
        on_line: Option<&(dyn Fn(&str) + Sync)>,
        capture: Capture,
    ) -> (Vec<u8>, bool) {
        let mut retained = Vec::new();
        let mut overflow = false;
        let mut line = Vec::new();
        let mut buf = [0; 8192];
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            match capture {
                Capture::Tail => {
                    retained.extend_from_slice(&buf[..n]);
                    if retained.len() > 131072 {
                        retained.drain(..retained.len() - 65536);
                    }
                }
                // Keep draining past the limit so the child cannot block.
                Capture::Full if retained.len() + n > FULL_LIMIT => overflow = true,
                Capture::Full => retained.extend_from_slice(&buf[..n]),
            }
            if let Some(on_line) = on_line {
                for &byte in &buf[..n] {
                    if byte == b'\n' {
                        on_line(&String::from_utf8_lossy(&line));
                        line.clear();
                    } else if line.len() < 65536 {
                        line.push(byte);
                    }
                }
            }
        }
        (retained, overflow)
    }
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (status, out, err) = thread::scope(|scope| {
        let stdout = scope.spawn(|| drain(stdout, None, capture));
        let stderr = scope.spawn(|| drain(stderr, on_line, Capture::Tail));
        let end = Instant::now() + Duration::from_secs(seconds);
        let status = loop {
            let stop = match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() >= end => {
                    anyhow::anyhow!("{program} timed out; installation is incomplete")
                }
                Ok(None) if cancelled() => Cancelled.into(),
                Ok(None) => {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(error) => error.into(),
            };
            // A group created for this child only; do not leave partitioning or
            // installation descendants running after reporting a timeout. The
            // pipe drains finish once the group is gone.
            crate::sys::kill_group(child.id());
            let _ = child.wait();
            break Err(stop);
        };
        let out = stdout.join().unwrap_or_default();
        let err = stderr.join().unwrap_or_default().0;
        (status, out, err)
    });
    let status = status?;
    let (out, overflow) = out;
    ensure!(!overflow, "{program} produced more than {FULL_LIMIT} bytes");
    let out = String::from_utf8_lossy(&out).into_owned();
    let err = String::from_utf8_lossy(&err).into_owned();
    ensure!(
        status.success(),
        "{program} failed ({status}): {err}\n{out}"
    );
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[test]
    fn streams_stderr_lines_and_returns_stdout() {
        let lines = Mutex::new(vec![]);
        let mut command = Command::new("sh");
        command
            .env_clear()
            .env("PATH", "/run/current-system/sw/bin:/usr/bin:/bin");
        let out = run(
            command,
            "sh",
            &["-c", "echo out; echo one >&2; echo two >&2"],
            10,
            Some(&|line: &str| lines.lock().unwrap().push(line.to_owned())),
            Capture::Tail,
        )
        .unwrap();
        assert_eq!(out, "out\n");
        assert_eq!(*lines.lock().unwrap(), ["one", "two"]);
    }
    #[test]
    fn full_capture_keeps_large_listings_and_tail_does_not() {
        let shell = || {
            let mut command = Command::new("sh");
            command
                .env_clear()
                .env("PATH", "/run/current-system/sw/bin:/usr/bin:/bin");
            command
        };
        // 300 KB: more than the tail retains.
        let script = "i=0; while [ $i -lt 3000 ]; do echo /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-path-$i-padding-padding-padding-padding-padding-xx; i=$((i+1)); done";
        let full = run(shell(), "sh", &["-c", script], 30, None, Capture::Full).unwrap();
        assert_eq!(full.lines().count(), 3000);
        assert!(full.starts_with("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-path-0-"));
        let tail = run(shell(), "sh", &["-c", script], 30, None, Capture::Tail).unwrap();
        assert!(tail.lines().count() < 3000);
    }
}
