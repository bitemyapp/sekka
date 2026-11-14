//! Each operating system's drives, listed as anyone may. Writing one is the
//! helper's alone, in `kakehashi-write`, which the window doesn't depend on;
//! it builds on the primitives in [`sys`].

use crate::drive::Drive;
use anyhow::Result;
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

/// Every whole drive, offered or not.
pub fn drives() -> Result<Vec<Drive>> {
    imp::drives()
}

/// The whole drive holding `image`, which is never offered.
pub fn image_drive(image: &Path) -> Option<String> {
    imp::image_drive(image)
}

/// What listing shares with the helper's write step: running the system's
/// tools, and on Windows opening a raw handle and sending it IOCTLs. None
/// of it finds, checks or writes a drive by itself.
pub mod sys {
    #[cfg(windows)]
    pub use super::imp::{ioctl, no_output, open_raw, volumes};
    #[cfg(unix)]
    use anyhow::Result;
    #[cfg(unix)]
    use std::path::Path;

    /// A program the platform code runs, found where it is installed even when
    /// the helper's environment was reset by the administrator prompt: pkexec
    /// keeps a PATH that, on NixOS, holds none of them.
    #[cfg(unix)]
    pub fn tool(name: &str) -> std::path::PathBuf {
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
    pub fn run(program: &str, args: &[&str]) -> Result<Vec<u8>> {
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
}
