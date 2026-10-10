// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed-argument subprocesses. Call only from a worker, never the GUI thread.
use anyhow::{Context, Result, ensure};
use std::os::unix::process::CommandExt;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

/// Set only while non-destructive preparation may be abandoned (the GUI
/// closed its request pipe before confirming). Never set after confirmation.
pub static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn cancelled() -> bool {
    CANCEL.load(Ordering::Relaxed)
}

pub fn command(program: &str) -> Command {
    let mut command = Command::new(program);
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

pub fn output(program: &str, args: &[&str], seconds: u64) -> Result<String> {
    run(
        command(program),
        program,
        args,
        seconds,
        None,
        Capture::Tail,
    )
}

/// As `output`, but returns all of stdout (up to 256 MiB) instead of its tail.
/// For machine-readable listings such as a system's closure.
pub fn output_full(program: &str, args: &[&str], seconds: u64) -> Result<String> {
    run(
        command(program),
        program,
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
    program: &str,
    args: &[&str],
    seconds: u64,
    on_line: &(dyn Fn(&str) + Sync),
) -> Result<String> {
    run(
        command(program),
        program,
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
    ensure!(!cancelled(), "Preparation was cancelled");
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
                Ok(None) if cancelled() => anyhow::anyhow!("Preparation was cancelled"),
                Ok(None) => {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(error) => error.into(),
            };
            // A group created for this child only; do not leave partitioning or
            // installation descendants running after reporting a timeout. The
            // pipe drains finish once the group is gone.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
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
