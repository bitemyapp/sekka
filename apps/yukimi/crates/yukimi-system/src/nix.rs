// SPDX-License-Identifier: MIT OR Apache-2.0
//! Running Nix: quick queries that return JSON, and long operations whose
//! progress streams through [`crate::log::Progress`].
use std::io::{BufRead, BufReader};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::log::Progress;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not run {0}: {1}")]
    Spawn(String, std::io::Error),
    #[error("{0}")]
    Failed(String),
    #[error("Nix's answer could not be read: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// `nix` with the features Yukimi uses switched on.
pub fn command() -> Command {
    let mut command = Command::new("nix");
    command.args(["--extra-experimental-features", "nix-command flakes"]);
    command
}

/// Run Nix and parse its output as JSON.
pub fn json(args: &[&str]) -> Result<serde_json::Value> {
    let output = command().args(args).stdin(Stdio::null()).output().map_err(|e| Error::Spawn("nix".into(), e))?;
    if !output.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&output.stderr));
        return Err(Error::Failed(last_error(&stderr)));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// One of Nix's settings, as `nix config show` gives it.
pub fn setting(name: &str) -> Option<String> {
    let output = command().args(["config", "show", name]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The useful part of Nix's error output: from the last `error:` on.
pub fn last_error(stderr: &str) -> String {
    match stderr.rfind("error:") {
        Some(at) => stderr[at..].trim().to_owned(),
        None => stderr.trim().to_owned(),
    }
}

/// A way to stop a [`stream`]ed program. Its standard input is a pipe, and
/// stopping closes it, which Yukimi's helper takes as the request to stop
/// cleanly; a program of this user's own is also sent SIGTERM.
#[derive(Clone, Default)]
pub struct Stop(Arc<Mutex<StopState>>);

#[derive(Default)]
struct StopState {
    requested: bool,
    signal: bool,
    stdin: Option<ChildStdin>,
    pid: Option<u32>,
}

impl Stop {
    /// `signal`: also send SIGTERM, for programs running as this user.
    pub fn new(signal: bool) -> Stop {
        Stop(Arc::new(Mutex::new(StopState { signal, ..StopState::default() })))
    }

    pub fn request(&self) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.requested = true;
        state.stdin = None;
        if let (true, Some(pid)) = (state.signal, state.pid) {
            // SAFETY: signalling a process has no memory-safety preconditions.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
    }

    /// Whether to send SIGTERM too, for the program run next: yes for
    /// programs running as this user, no for the helper (it runs as root).
    pub fn signal(&self, on: bool) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).signal = on;
    }

    pub fn requested(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).requested
    }
}

/// Run a program that speaks Nix's progress log on its error output,
/// calling `update` with the progress after each event. Returns its
/// standard output.
pub fn stream(mut command: Command, stop: &Stop, mut update: impl FnMut(&Progress)) -> Result<(String, Progress)> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = {
        let mut state = stop.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.requested {
            return Err(Error::Failed("Stopped".into()));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Spawn(program, e))?;
        state.stdin = child.stdin.take();
        state.pid = Some(child.id());
        child
    };
    let stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stdout), &mut out);
        out
    });
    let mut progress = Progress::default();
    for line in BufReader::new(child.stderr.take().expect("piped")).lines().map_while(|l| l.ok()) {
        if progress.feed(&line) {
            update(&progress);
        }
    }
    let status = child.wait().map_err(|e| Error::Spawn("nix".into(), e))?;
    {
        let mut state = stop.0.lock().unwrap_or_else(|p| p.into_inner());
        state.pid = None;
        state.stdin = None;
    }
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        let message = progress
            .errors()
            .last()
            .map(str::to_owned)
            .unwrap_or_else(|| progress.log.iter().rev().take(5).cloned().collect::<Vec<_>>().join("\n"));
        return Err(Error::Failed(message));
    }
    Ok((out, progress))
}

/// The Nixpkgs a flake's `nixpkgs` input is, as a store path: for systems
/// whose facts don't say. Asks Nix, which reads the flake.
pub fn flake_nixpkgs(flake: &str) -> Result<String> {
    let expr = format!("(builtins.getFlake {}).inputs.nixpkgs.outPath", yukimi_config::edit::nix_string(flake));
    let output = command()
        .args(["eval", "--raw", "--impure", "--expr", &expr])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Spawn("nix".into(), e))?;
    if !output.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&output.stderr));
        return Err(Error::Failed(last_error(&stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_start_at_the_last_error() {
        let stderr = "warning: Git tree is dirty\nerror: builder failed\n\nerror: attribute 'x' missing\n  at line 3";
        assert_eq!(last_error(stderr), "error: attribute 'x' missing\n  at line 3");
        assert_eq!(last_error("plain"), "plain");
    }

    #[test]
    fn streams_progress_and_output() {
        let mut command = Command::new("sh");
        command.args(["-c", r#"echo '@nix {"action":"msg","level":1,"msg":"warning: hello"}' >&2; echo result"#]);
        let mut updates = 0;
        let (out, progress) = stream(command, &Stop::default(), |_| updates += 1).unwrap();
        assert_eq!(out.trim(), "result");
        assert_eq!(updates, 1);
        assert_eq!(progress.messages[0].text, "warning: hello");
        let mut failing = Command::new("sh");
        failing.args(["-c", r#"echo '@nix {"action":"msg","level":0,"msg":"error: nope"}' >&2; exit 1"#]);
        assert_eq!(stream(failing, &Stop::default(), |_| {}).unwrap_err().to_string(), "error: nope");
    }

    #[test]
    fn stopping_closes_input_or_signals() {
        // The helper's way: it notices its input close, and says so.
        let mut polite = Command::new("sh");
        polite.args([
            "-c",
            r#"cat >/dev/null; echo '@nix {"action":"msg","level":0,"msg":"error: Stopped"}' >&2; exit 1"#,
        ]);
        let stop = Stop::new(false);
        let later = stop.clone();
        let timer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            later.request();
        });
        assert_eq!(stream(polite, &stop, |_| {}).unwrap_err().to_string(), "error: Stopped");
        timer.join().unwrap();
        assert!(stop.requested());

        // A program of this user's own that ignores its input is signalled.
        let mut stubborn = Command::new("sleep");
        stubborn.arg("30");
        let stop = Stop::new(true);
        let later = stop.clone();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            later.request();
        });
        assert!(stream(stubborn, &stop, |_| {}).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));

        // Once stopped, nothing more starts.
        assert!(stream(Command::new("true"), &stop, |_| {}).is_err());
    }
}
