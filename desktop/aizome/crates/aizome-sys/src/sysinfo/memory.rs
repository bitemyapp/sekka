//! Memory and swap from `/proc/meminfo`.
//!
//! `/proc/meminfo` gives sizes in kB (KiB); everything here is bytes.
//! "Used" is `MemTotal − MemAvailable`, the kernel's own estimate of what
//! can't be reclaimed without swapping, which is what `free` and GNOME
//! System Monitor show. Kernels before 3.14 lack `MemAvailable`; there it is
//! estimated as `MemFree + Buffers + Cached`.

use super::read_text;
use crate::{Error, Result};

const MEMINFO: &str = "/proc/meminfo";

/// Memory and swap, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    /// Usable RAM (`MemTotal`): installed memory less what the firmware and
    /// kernel image reserve.
    pub total: u64,
    /// What programs can still get without swapping (`MemAvailable`).
    pub available: u64,
    /// `total − available`.
    pub used: u64,
    /// Memory holding nothing at all (`MemFree`); usually small, since the
    /// kernel uses spare memory for caches.
    pub free: u64,
    /// Block-device buffers (`Buffers`).
    pub buffers: u64,
    /// The page cache (`Cached`), including shared memory and tmpfs files.
    pub cached: u64,
    /// Shared memory and tmpfs (`Shmem`), part of `cached`.
    pub shared: u64,
    /// Swap space (`SwapTotal`).
    pub swap_total: u64,
    /// Unused swap (`SwapFree`).
    pub swap_free: u64,
    /// `swap_total − swap_free`.
    pub swap_used: u64,
}

impl Memory {
    /// [`used`](Self::used) as a percentage of [`total`](Self::total).
    pub fn used_percent(&self) -> f32 {
        fraction(self.used, self.total)
    }

    /// [`swap_used`](Self::swap_used) as a percentage of
    /// [`swap_total`](Self::swap_total); 0 without swap.
    pub fn swap_used_percent(&self) -> f32 {
        fraction(self.swap_used, self.swap_total)
    }
}

fn fraction(part: u64, whole: u64) -> f32 {
    if whole == 0 { 0.0 } else { ((part as f64 / whole as f64) * 100.0).clamp(0.0, 100.0) as f32 }
}

/// Read `/proc/meminfo`.
pub fn memory() -> Result<Memory> {
    let text = read_text(MEMINFO)?;
    parse_meminfo(&text).ok_or_else(|| Error::Failed(format!("{MEMINFO} has no MemTotal")))
}

/// Parse `/proc/meminfo`; `None` without a `MemTotal` line.
pub(crate) fn parse_meminfo(text: &str) -> Option<Memory> {
    let mut total = None;
    let mut available = None;
    let mut memory = Memory::default();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let mut words = rest.split_ascii_whitespace();
        let Some(Ok(number)) = words.next().map(str::parse::<u64>) else {
            continue;
        };
        let bytes = match words.next() {
            Some("kB") => number.saturating_mul(1024),
            // Counts such as HugePages_Total have no unit.
            None => number,
            Some(_) => continue,
        };
        match key.trim() {
            "MemTotal" => total = Some(bytes),
            "MemAvailable" => available = Some(bytes),
            "MemFree" => memory.free = bytes,
            "Buffers" => memory.buffers = bytes,
            "Cached" => memory.cached = bytes,
            "Shmem" => memory.shared = bytes,
            "SwapTotal" => memory.swap_total = bytes,
            "SwapFree" => memory.swap_free = bytes,
            _ => {}
        }
    }
    memory.total = total?;
    memory.available = available
        .unwrap_or_else(|| memory.free.saturating_add(memory.buffers).saturating_add(memory.cached))
        .min(memory.total);
    memory.used = memory.total - memory.available;
    memory.swap_free = memory.swap_free.min(memory.swap_total);
    memory.swap_used = memory.swap_total - memory.swap_free;
    Some(memory)
}

#[cfg(test)]
mod tests {
    use super::*;

    // From this machine, trimmed.
    const MEMINFO_FIXTURE: &str = "\
MemTotal:       32354264 kB
MemFree:        10035628 kB
MemAvailable:   23512128 kB
Buffers:         1588364 kB
Cached:         10973768 kB
SwapCached:       160832 kB
Active:          5358664 kB
Inactive:       12340660 kB
SwapTotal:      32505852 kB
SwapFree:       30637772 kB
Zswap:            429236 kB
Shmem:            184440 kB
HugePages_Total:       0
Hugepagesize:       2048 kB
";

    #[test]
    fn meminfo_parses_into_bytes() {
        let memory = parse_meminfo(MEMINFO_FIXTURE).unwrap();
        assert_eq!(memory.total, 32354264 * 1024);
        assert_eq!(memory.available, 23512128 * 1024);
        assert_eq!(memory.used, (32354264 - 23512128) * 1024);
        assert_eq!(memory.free, 10035628 * 1024);
        assert_eq!(memory.buffers, 1588364 * 1024);
        assert_eq!(memory.cached, 10973768 * 1024);
        assert_eq!(memory.shared, 184440 * 1024);
        assert_eq!(memory.swap_total, 32505852 * 1024);
        assert_eq!(memory.swap_used, (32505852 - 30637772) * 1024);
        assert!((memory.used_percent() - 27.33).abs() < 0.01);
    }

    #[test]
    fn old_kernels_without_memavailable_are_estimated() {
        let memory = parse_meminfo("MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 250 kB\n").unwrap();
        assert_eq!(memory.available, 400 * 1024);
        assert_eq!(memory.used, 600 * 1024);
        assert_eq!(memory.swap_used_percent(), 0.0);
    }

    #[test]
    fn odd_meminfo_never_panics() {
        assert_eq!(parse_meminfo(""), None);
        assert_eq!(parse_meminfo("MemFree: 5 kB\n"), None);
        // Available larger than total, garbage lines, swap free > total.
        let memory = parse_meminfo(
            "MemTotal: 10 kB\nMemAvailable: 20 kB\nnonsense\nCached: x kB\nSwapTotal: 1 kB\nSwapFree: 2 kB\nOdd: 3 MB\n",
        )
        .unwrap();
        assert_eq!(memory.used, 0);
        assert_eq!(memory.swap_used, 0);
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_memory_is_sane() {
        let memory = memory().unwrap();
        println!("{memory:#?}");
        assert!(memory.total > 256 << 20);
        assert!(memory.available <= memory.total);
        assert_eq!(memory.used + memory.available, memory.total);
    }
}
