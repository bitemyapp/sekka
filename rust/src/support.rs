use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

pub fn init_signals() -> Result<()> {
    if INTERRUPTED.get().is_none() {
        let flag = Arc::new(AtomicBool::new(false));
        for signal in [libc::SIGINT, libc::SIGTERM] {
            signal_hook::flag::register(signal, flag.clone())?;
        }
        let _ = INTERRUPTED.set(flag);
    }
    Ok(())
}

pub fn check_interrupt() -> Result<()> {
    ensure!(
        !INTERRUPTED.get().is_some_and(|f| f.load(Ordering::Relaxed)),
        "Interrupted"
    );
    Ok(())
}

pub fn pause(duration: Duration) -> Result<()> {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        check_interrupt()?;
        thread::sleep(
            Duration::from_millis(100).min(end.saturating_duration_since(Instant::now())),
        );
    }
    Ok(())
}

pub fn repo() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("RESPIN_REPO") {
        return Ok(PathBuf::from(path).canonicalize()?);
    }
    let origin = std::env::var_os("RUST_SCRIPT_PATH")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    for path in origin.ancestors() {
        if path.join("flake.nix").is_file() && path.join("rust/Cargo.toml").is_file() {
            return Ok(path.to_path_buf());
        }
    }
    bail!("Run from the repository or set RESPIN_REPO")
}

pub fn run(command: &mut Command) -> Result<()> {
    let description = format!("{command:?}");
    let mut child = Process(command.spawn().with_context(|| description.clone())?);
    let status = child.wait(Duration::from_secs(7200))?;
    ensure!(status.success(), "{description} failed: {status}");
    Ok(())
}

pub fn output(command: &mut Command) -> Result<String> {
    check_interrupt()?;
    let result = command.output().with_context(|| format!("{command:?}"))?;
    ensure!(
        result.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?)
}

pub struct Process(pub Child);
impl Process {
    /// Bounded shutdown grace that must still run after Ctrl-C. Only use for
    /// cleanup (for example flushing and powering off the persistent builder).
    pub fn wait_cleanup(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let end = Instant::now() + timeout;
        loop {
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            ensure!(Instant::now() < end, "Cleanup process timed out");
            thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn wait(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let end = Instant::now() + timeout;
        loop {
            check_interrupt()?;
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            ensure!(Instant::now() < end, "Child process timed out");
            pause(Duration::from_millis(100))?;
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        // Only the child we spawned, never a process-name match or process group.
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        for _ in 0..150 {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn regular(path: &Path) -> Result<PathBuf> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "Expected a regular file, not a symlink or device: {}",
        path.display()
    );
    Ok(path.canonicalize()?)
}
pub fn sparse(path: &Path, size: u64) -> Result<()> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .set_len(size)?;
    Ok(())
}
pub fn sha256(path: &Path) -> Result<String> {
    let mut source = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 1024 * 128];
    loop {
        check_interrupt()?;
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}
pub fn stamp() -> String {
    format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        std::process::id()
    )
}
pub fn qpath(path: &Path) -> Result<String> {
    let text = path.to_str().context("QEMU path is not UTF-8")?;
    ensure!(!text.contains(['\n', '\r']), "Invalid QEMU path");
    Ok(text.replace(',', ",,"))
}
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
pub fn optional_copy(source: &Path, dest: &Path) -> Result<()> {
    if source.is_file() {
        fs::copy(source, dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quote_shell() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
    #[test]
    fn escapes_qemu_commas() {
        assert_eq!(qpath(Path::new("/a,b")).unwrap(), "/a,,b");
    }
    #[test]
    fn refuses_symlinks_and_existing_disks() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("disk");
        sparse(&file, 1024).unwrap();
        assert!(sparse(&file, 1024).is_err());
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(regular(&link).is_err());
        assert_eq!(fs::metadata(file).unwrap().len(), 1024);
    }
    #[test]
    fn cleanup_wait_reaps_a_completed_child() {
        let mut child = Process(Command::new("true").spawn().unwrap());
        assert!(
            child
                .wait_cleanup(Duration::from_secs(2))
                .unwrap()
                .success()
        );
    }
}
