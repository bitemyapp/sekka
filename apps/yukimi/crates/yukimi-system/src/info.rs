// SPDX-License-Identifier: MIT OR Apache-2.0
//! This computer at a glance.
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemInfo {
    pub hostname: String,
    /// The running NixOS release, such as `26.11.20261001.c59305b`.
    pub nixos_version: String,
    /// The running kernel, such as `7.2.8`.
    pub kernel: String,
    /// The system running now differs from the one the machine started
    /// with in its kernel, initrd or kernel modules, so a restart would
    /// finish an update.
    pub restart_needed: bool,
}

impl SystemInfo {
    pub fn read() -> SystemInfo {
        SystemInfo {
            hostname: hostname(),
            nixos_version: std::fs::read_to_string("/run/current-system/nixos-version")
                .map(|v| v.trim().to_owned())
                .unwrap_or_default(),
            kernel: kernel_release(),
            restart_needed: restart_needed(Path::new("/run/booted-system"), Path::new("/run/current-system")),
        }
    }

    /// The release without its date and revision: `26.11`.
    pub fn release(&self) -> &str {
        let mut dots = self.nixos_version.match_indices('.');
        match dots.nth(1) {
            Some((i, _)) => &self.nixos_version[..i],
            None => &self.nixos_version,
        }
    }
}

/// Whether the parts of a system that only take effect on restart differ.
pub fn restart_needed(booted: &Path, current: &Path) -> bool {
    ["kernel", "initrd", "kernel-modules"].iter().any(|part| {
        let a = std::fs::canonicalize(booted.join(part)).ok();
        let b = std::fs::canonicalize(current.join(part)).ok();
        a.is_some() && b.is_some() && a != b
    })
}

/// This computer's name, as the kernel has it.
pub fn hostname() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: gethostname writes at most buffer.len() bytes into buffer.
    let ok = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } == 0;
    if !ok {
        return std::fs::read_to_string("/etc/hostname").map(|h| h.trim().to_owned()).unwrap_or_default();
    }
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

fn kernel_release() -> String {
    // SAFETY: utsname is plain data, and uname fills it in.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut name) } != 0 {
        return String::new();
    }
    let release = &name.release;
    let end = release.iter().position(|&c| c == 0).unwrap_or(release.len());
    release[..end].iter().map(|&c| c as u8 as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_from_version() {
        let info = SystemInfo { nixos_version: "26.11.20261001.c59305b".into(), ..Default::default() };
        assert_eq!(info.release(), "26.11");
        assert!(!SystemInfo::read().kernel.is_empty() || cfg!(not(target_os = "linux")));
    }
}
