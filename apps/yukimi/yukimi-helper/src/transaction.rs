// SPDX-License-Identifier: MIT OR Apache-2.0
//! The GUI is a client of a system-owned operation. No worker output is a
//! pipe to that client: a closed window must never interrupt activation.
use super::*;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};

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

pub fn client(args: &[String]) -> Result<String> {
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
    let mut start = command("systemd-run");
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
    start.arg(std::env::current_exe()?).arg("__worker").arg(&dir).args(args);
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
        if state().0 == Phase::Stopping && !cancelled {
            // The worker handles this before commit; after commit it finishes.
            let _ = command("systemctl").args(["kill", "--kill-whom=main", "--signal=SIGINT", &unit]).output();
            cancelled = true;
        }
        let status = command("systemctl").args(["show", &unit, "--property=ActiveState", "--value"]).output()?;
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
