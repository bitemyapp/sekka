//! Each operating system's drives: listing them as anyone may, and, in the
//! helper, opening one for writing.

use crate::drive::Drive;
use anyhow::Result;
use std::fs::File;
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as imp;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use unsupported as imp;

/// A whole drive opened for writing, exclusively.
pub struct Device {
    pub file: File,
    pub bytes: u64,
    /// Reads and writes are whole multiples of this.
    pub sector: u64,
    /// Whatever must stay open while the drive is written, such as locked
    /// volumes on Windows.
    #[allow(dead_code)]
    pub(crate) held: imp::Held,
}

/// Every whole drive, offered or not.
pub fn drives() -> Result<Vec<Drive>> {
    imp::drives()
}

/// The whole drive holding `image`, which is never offered.
pub fn image_drive(image: &Path) -> Option<String> {
    imp::image_drive(image)
}

pub(crate) fn unmount(drive: &Drive) -> Result<()> {
    imp::unmount(drive)
}

pub(crate) fn open(drive: &Drive, _cancelled: &dyn Fn() -> bool) -> Result<Device> {
    #[cfg(target_os = "macos")]
    return imp::open(drive, _cancelled);
    #[cfg(not(target_os = "macos"))]
    return imp::open(drive);
}

/// Forget cached blocks, so reading back reads the drive.
pub(crate) fn drop_cache(device: &Device) {
    imp::drop_cache(device)
}

pub(crate) fn eject(drive: &Drive) -> Result<()> {
    imp::eject(drive)
}

/// A program the platform code runs, found where it is installed even when
/// the helper's environment was reset by the administrator prompt: pkexec
/// keeps a PATH that, on NixOS, holds none of them.
#[cfg(unix)]
pub(crate) fn tool(name: &str) -> std::path::PathBuf {
    let built_in = option_env!("KAKEHASHI_TOOL_PATH").unwrap_or("");
    let path = std::env::var("PATH").unwrap_or_default();
    let fixed = "/run/current-system/sw/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    for dir in [built_in, path.as_str(), fixed].iter().flat_map(|p| p.split(':')).filter(|d| !d.is_empty()) {
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    std::path::PathBuf::from(name)
}

/// Run a program and return what it printed.
#[cfg(unix)]
pub(crate) fn run(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    use anyhow::{Context, ensure};
    let path = if program.starts_with('/') { std::path::PathBuf::from(program) } else { tool(program) };
    let output = std::process::Command::new(&path)
        .args(args)
        .output()
        .with_context(|| format!("could not run {}", path.display()))?;
    ensure!(
        output.status.success(),
        "{program} {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
}
