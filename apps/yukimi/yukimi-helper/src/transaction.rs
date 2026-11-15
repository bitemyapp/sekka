// SPDX-License-Identifier: MIT OR Apache-2.0
//! The GUI is a client of a system-owned operation. No worker output is a
//! pipe to that client: a closed window must never interrupt activation.
use crate::process::{self, Failure, PATH, Program, Result, command, watch_for_stop};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The worker's closed set of operations. Arguments are parsed by the
/// operation itself and never interpreted as launcher options or a program.
#[derive(Clone, Copy, Debug)]
pub enum Action<'a> {
    Change(&'a [String]),
    Rollback(&'a [String]),
    Clean(&'a [String]),
    Delete(&'a [String]),
}

impl<'a> Action<'a> {
    pub fn parse(args: &'a [String]) -> Result<Self> {
        let (name, rest) = args.split_first().ok_or_else(|| Failure("Missing operation".into()))?;
        match name.as_str() {
            "change" => Ok(Self::Change(rest)),
            "rollback" => Ok(Self::Rollback(rest)),
            "clean" => Ok(Self::Clean(rest)),
            "delete" => Ok(Self::Delete(rest)),
            _ => Err(Failure("Usage: yukimi-helper change|rollback|clean|delete … (or estimate …)".into())),
        }
    }

    fn arguments(self) -> (&'static str, &'a [String]) {
        match self {
            Self::Change(args) => ("change", args),
            Self::Rollback(args) => ("rollback", args),
            Self::Clean(args) => ("clean", args),
            Self::Delete(args) => ("delete", args),
        }
    }
}

const OPERATIONS: &str = "/var/lib/yukimi/operations";

pub fn worker_directory(value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    let metadata = std::fs::symlink_metadata(&path)?;
    if path.parent() != Some(Path::new(OPERATIONS))
        || !metadata.is_dir()
        || metadata.uid() != 0
        || metadata.mode() & 0o077 != 0
    {
        return Err(Failure("Invalid operation directory".into()));
    }
    Ok(path)
}

pub fn save_result(dir: &Path, json: &str) -> Result<()> {
    let pending = dir.join("result.new");
    let mut file = std::fs::File::create(&pending)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(pending, dir.join("result.json"))?;
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}

/// Forward progress opportunistically. Losing the reader is harmless; the
/// complete log remains in the root-only operation directory.
fn progress(file: &mut std::fs::File) {
    let mut bytes = [0; 16384];
    while let Ok(n) = file.read(&mut bytes) {
        if n == 0 {
            break;
        }
        let _ = std::io::stderr().write_all(&bytes[..n]);
    }
}

/// Launch a Yukimi worker and follow its result. The command never escapes:
/// callers cannot replace the worker with a shell, Nix or another program.
///
/// ```compile_fail,E0308
/// use yukimi_helper::transaction;
/// let _ = transaction::client(&["nix-store".to_string(), "--gc".to_string()]);
/// ```
///
/// ```compile_fail,E0603
/// use yukimi_helper::transaction::worker_command;
/// ```
///
/// ```no_run
/// use yukimi_helper::transaction::{self, Action};
/// let _ = transaction::client(Action::Clean(&[]));
/// ```
pub fn client(action: Action<'_>) -> Result<String> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(OPERATIONS)?;
    let id = format!(
        "{}-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos(),
        std::process::id()
    );
    let unit = format!("yukimi-operation-{id}.service");
    let dir = Path::new(OPERATIONS).join(id);
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let log = dir.join("progress.log");
    std::fs::File::create(&log)?;
    let mut log = std::fs::File::open(log)?;
    let mut start = worker_command(action, &unit, &dir, &std::env::current_exe()?);
    let started = start.output()?;
    if !started.status.success() {
        return Err(Failure(format!(
            "Could not start the system operation: {}",
            String::from_utf8_lossy(&started.stderr).trim()
        )));
    }
    watch_for_stop();
    let mut cancelled = false;
    loop {
        progress(&mut log);
        if let Ok(result) = std::fs::read_to_string(dir.join("result.json")) {
            progress(&mut log);
            return Ok(result);
        }
        if process::stop_requested() && !cancelled {
            // The worker handles this before commit; after commit it finishes.
            let _ = command(Program::SYSTEMCTL).args(["kill", "--kill-whom=main", "--signal=SIGINT", &unit]).output();
            cancelled = true;
        }
        let status = command(Program::SYSTEMCTL).args(["show", &unit, "--property=ActiveState", "--value"]).output()?;
        if !status.status.success()
            || !matches!(String::from_utf8_lossy(&status.stdout).trim(), "active" | "activating" | "deactivating")
        {
            // Completion may have raced the status request.
            if let Ok(result) = std::fs::read_to_string(dir.join("result.json")) {
                progress(&mut log);
                return Ok(result);
            }
            return Err(Failure(format!(
                "The system operation ended without a verified result. Its log is {}. Check update status before retrying.",
                dir.join("progress.log").display()
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

// Private: returning a mutable Command here would expose systemd-run again.
fn worker_command(action: Action<'_>, unit: &str, dir: &Path, helper: &Path) -> Command {
    let mut start = command(Program::new("systemd-run"));
    start.args([
        "--quiet",
        "--collect",
        "--no-ask-password",
        "--service-type=exec",
        "--unit",
        &unit,
        "--property=StandardInput=null",
        "--property=KillMode=control-group",
        "--property=TimeoutStopSec=infinity",
        "--setenv=HOME=/root",
        "--setenv=LANG=C.UTF-8",
    ]);
    start.arg(format!("--setenv=PATH={PATH}"));
    start.arg(format!("--property=StandardOutput=append:{}/progress.log", dir.display()));
    start.arg(format!("--property=StandardError=append:{}/progress.log", dir.display()));
    // However the worker ends, SIGKILL included, systemd runs this after it:
    // a clean-up killed before it rewrote the boot menu is finished.
    start.arg(format!("--property=ExecStopPost={} __recover", helper.display()));
    let (operation, args) = action.arguments();
    start.arg(helper).arg("__worker").arg(dir).arg(operation).args(args);
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_arguments_cannot_replace_the_worker_or_recovery() {
        let args = vec!["--property=ExecStart=nix-store --gc".into(), "--".into(), "sh".into(), "-c".into()];
        let dir = Path::new("/var/lib/yukimi/operations/test");
        let helper = Path::new("/nix/store/test/bin/yukimi-helper");
        for action in [Action::Change(&args), Action::Rollback(&args), Action::Clean(&args), Action::Delete(&args)] {
            let command = worker_command(action, "yukimi-operation-test.service", dir, helper);
            assert_eq!(command.get_program(), "systemd-run");
            let actual: Vec<_> = command.get_args().map(|arg| arg.to_str().unwrap()).collect();
            let worker = actual.iter().position(|arg| *arg == helper.to_str().unwrap()).unwrap();
            assert!(actual[..worker].contains(&"--property=ExecStopPost=/nix/store/test/bin/yukimi-helper __recover"));
            assert_eq!(
                &actual[worker..worker + 4],
                &[helper.to_str().unwrap(), "__worker", dir.to_str().unwrap(), action.arguments().0]
            );
            assert_eq!(&actual[worker + 4..], args);
        }
    }

    #[test]
    fn only_worker_operations_parse() {
        for name in ["change", "rollback", "clean", "delete"] {
            let args = vec![name.into(), "payload".into()];
            let action = Action::parse(&args).unwrap();
            assert_eq!(action.arguments(), (name, &args[1..]));
        }
        for name in ["nix-store", "sh", "__worker", "__recover", "--unit", ""] {
            assert!(Action::parse(&[name.into()]).is_err(), "{name}");
        }
        assert!(Action::parse(&[]).is_err());
    }
}
