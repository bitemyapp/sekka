// SPDX-License-Identifier: GPL-3.0-or-later
//! RAM-derived sizing. /proc/meminfo reports kibibytes.
use anyhow::{Context, Result, ensure};
use std::fs;

pub const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemInfo {
    pub total: u64,
    pub available: u64,
    pub free: u64,
}

pub fn parse(text: &str) -> Result<MemInfo> {
    let field = |name: &str| -> Result<u64> {
        let line = text
            .lines()
            .find(|line| line.split(':').next() == Some(name))
            .with_context(|| format!("Missing {name} in /proc/meminfo"))?;
        let mut words = line.split_whitespace().skip(1);
        let value: u64 = words.next().context("Missing meminfo value")?.parse()?;
        ensure!(words.next() == Some("kB"), "Unexpected meminfo unit");
        value.checked_mul(1024).context("meminfo overflow")
    };
    let info = MemInfo {
        total: field("MemTotal")?,
        available: field("MemAvailable")?,
        free: field("MemFree")?,
    };
    ensure!(info.total > 0, "Invalid MemTotal");
    Ok(info)
}

pub fn read() -> Result<MemInfo> {
    parse(&fs::read_to_string("/proc/meminfo")?)
}

/// Swap matched to installed RAM. MemTotal excludes firmware and kernel
/// reservations, so a 16 GiB machine reports slightly less: round up.
pub fn swap_bytes(total: u64) -> u64 {
    total.div_ceil(GIB) * GIB
}

/// Memory that page-cache warming and live-store downloads must leave free.
pub fn reserve(total: u64) -> u64 {
    (total / 8).max(GIB)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_kibibytes_and_rounds_swap_to_whole_gibibytes() {
        let info = parse(
            "MemTotal:       16165536 kB\nMemFree:         1000000 kB\nMemAvailable:    8000000 kB\n",
        )
        .unwrap();
        assert_eq!(info.total, 16165536 * 1024);
        assert_eq!(swap_bytes(info.total), 16 * GIB);
        assert_eq!(swap_bytes(32 * GIB), 32 * GIB);
        assert_eq!(swap_bytes(32 * GIB + 1), 33 * GIB);
        assert_eq!(reserve(4 * GIB), GIB);
        assert_eq!(reserve(64 * GIB), 8 * GIB);
        assert!(parse("MemTotal: 1 MB\nMemFree: 1 kB\nMemAvailable: 1 kB\n").is_err());
        assert!(parse("MemFree: 1 kB\n").is_err());
    }
}
