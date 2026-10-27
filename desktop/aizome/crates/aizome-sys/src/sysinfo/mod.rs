//! What the computer is and what it is doing, read from `/proc` and `/sys`:
//! CPU and memory use, load, processes, disk and network throughput,
//! temperatures, GPUs, and the hardware summary for "About this computer".
//!
//! Nothing here talks to a daemon, so nothing here needs a [`Wake`] or a
//! [`Task`]: the UI calls these functions and samplers when it wants to,
//! typically on a one-second timer (Myaku, the menu bar's meters) or once
//! (Settings' "About this computer").
//!
//! [`Wake`]: crate::Wake
//! [`Task`]: crate::Task
//!
//! # Conventions
//!
//! - **Units.** Sizes and byte counts are bytes (`u64`); throughput is bytes
//!   per second (`f64`); percentages are `f32` from 0 to 100, except a
//!   process's [`cpu_percent`](Process::cpu_percent), which is a
//!   percentage of *one* CPU as in `top` and can exceed 100; temperatures
//!   are degrees Celsius (`f32`); clock speeds are MHz.
//! - **Samplers.** [`CpuSampler`], [`ProcessSampler`], [`DiskIoSampler`] and
//!   [`NetSampler`] keep the previous counters, and each `sample()` reports
//!   use over the time since the previous call. Create one per view and
//!   call it on a timer. The first `sample()` of a [`CpuSampler`] reports
//!   the average since boot; the first of the others reports zero rates
//!   (there is nothing to measure against yet).
//! - **Odd data never panics.** Malformed lines are skipped, files that
//!   can't be read give `None`, empty lists or zeros, and processes that
//!   exit while being read are left out silently.
//! - **Thin readers.** Each parser takes the file's text (or, for `/sys`
//!   trees, a root directory), and the functions that read the live system
//!   are thin wrappers, so everything is tested with fixtures.
//!
//! # What each part reads
//!
//! | Part | Reads |
//! |------|-------|
//! | [`CpuSampler`] | `/proc/stat` (`cpu` and `cpuN` lines); `/sys/devices/system/cpu/cpuN/cpufreq/scaling_cur_freq` |
//! | [`memory()`] | `/proc/meminfo` |
//! | [`load_average`], [`uptime`], [`boot_time`] | `/proc/loadavg`, `/proc/uptime`, `btime` in `/proc/stat` |
//! | [`ProcessSampler`] | `/proc/PID/stat` every sample; `cmdline`, `exe`, `status` and `cgroup` once per process (see [`ProcessSampler`]) |
//! | [`signal_process`] | `/proc/PID/status` (the real uid), then `pidfd_send_signal(2)` or `kill(2)` |
//! | [`DiskIoSampler`] | `/proc/diskstats`; `/sys/block/NAME` to tell disks from partitions |
//! | [`NetSampler`] | `/proc/net/dev`; `/sys/class/net/NAME/{flags,device}` |
//! | [`temperatures`] | `/sys/class/hwmon/hwmon*/{name,temp*_input,temp*_label,temp*_max,temp*_crit}` |
//! | [`gpus`] | `/sys/class/drm/card*/device/{vendor,device,driver,boot_vga,mem_info_vram_total}`; `pci.ids` |
//! | [`hardware()`] | `/proc/cpuinfo`, CPU topology, `/sys/block`, `/etc/os-release`, `/sys/class/dmi/id`, kernel release, `uname`, hostname, `/etc/machine-info` |
//! | [`detail`] | `/proc/PID/{status,smaps_rollup,io,cwd,fd,fdinfo}`, `/proc/pressure/*` |
//! | [`drm`] | `/sys/class/drm/card*/device/{gpu_busy_percent,mem_info_vram_*,hwmon}`; DRM usage statistics in `fdinfo` |
//! | [`power`] | `/sys/class/powercap/intel-rapl:N` (root only), `/sys/class/power_supply/*` |
//!
//! # What the system must provide
//!
//! - `/proc` mounted without `hidepid=` restrictions, or other users'
//!   processes are invisible (the sampler then lists only the caller's).
//! - **GPU names** come from `pci.ids` (the hwdata package). The places
//!   searched are listed at [`PCI_IDS_PATHS`]; none of them exists on NixOS
//!   unless hwdata is in the system profile, so Sekka's NixOS module should
//!   add `pkgs.hwdata` to `environment.systemPackages` (giving
//!   `/run/current-system/sw/share/hwdata/pci.ids`). Without it GPUs are
//!   named from a small vendor table, such as "Intel graphics (8086:a7a0)".
//!   [`gpus_with_pci_ids`] takes an explicit path instead.
//! - **Temperatures** need the CPU's hwmon driver (`coretemp`, `k10temp`,
//!   `zenpower`), which udev loads on its own.
//! - Some DMI fields (serial numbers, the product UUID) are readable only by
//!   root; nothing here reads them.

mod cpu;
pub mod detail;
mod dmi;
pub mod drm;
mod gpu;
mod hardware;
mod io;
mod load;
mod memory;
mod os_release;
pub mod power;
mod process;
mod sensors;
mod signal;

pub use cpu::{CoreUsage, CpuSampler, CpuUsage};
pub use dmi::{Chassis, HostModel, host_model};
pub use gpu::{Gpu, PCI_IDS_PATHS, gpus, gpus_with_pci_ids};
pub use hardware::{
    CpuInfo, Disk, Hardware, architecture, cpu_info, disks, hardware, hardware_with_pci_ids, hostname, kernel_release,
    pretty_hostname,
};
pub use io::{DiskIo, DiskIoSampler, DiskIoStat, InterfaceIo, NetIo, NetSampler};
pub use load::{LoadAverage, boot_time, load_average, uptime};
pub use memory::{Memory, memory};
pub use os_release::{OsRelease, os_release, parse_os_release};
pub use process::{Process, ProcessSampler, ProcessState, SortKey, SystemdUnit, UnitManager, sort_processes};
pub use sensors::{Temperature, cpu_temperature, pick_cpu_temperature, temperatures};
pub use signal::{Signal, kill, signal_process, signal_started, terminate};

use crate::Error;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

/// The error for a failed read of `path`, keeping what kind of failure it
/// was: a missing file is [`Error::NotFound`], a refused one
/// [`Error::NotAuthorized`].
pub(crate) fn file_error(path: &Path, error: &std::io::Error) -> Error {
    let what = format!("{}: {error}", path.display());
    match error.kind() {
        std::io::ErrorKind::NotFound => Error::NotFound(what),
        std::io::ErrorKind::PermissionDenied => Error::NotAuthorized(what),
        _ => Error::Io(what),
    }
}

/// Read a whole file as text, with the path in the error.
pub(crate) fn read_text(path: impl AsRef<Path>) -> crate::Result<String> {
    let path = path.as_ref();
    std::fs::read_to_string(path).map_err(|error| file_error(path, &error))
}

/// Read a file into `buf` (cleared first), reusing its allocation.
pub(crate) fn read_into(path: &Path, buf: &mut Vec<u8>) -> std::io::Result<()> {
    buf.clear();
    std::fs::File::open(path)?.read_to_end(buf)?;
    Ok(())
}

/// A one-value file such as a `/sys` attribute, trimmed of whitespace and
/// NULs (device-tree strings end in one); `None` if it can't be read or is
/// empty.
pub(crate) fn read_attr(path: impl AsRef<Path>) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let value = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    (!value.is_empty()).then(|| value.to_owned())
}

/// A one-number `/sys` attribute, in decimal.
pub(crate) fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_attr(path)?.parse().ok()
}

/// A hexadecimal number with or without a `0x` prefix, such as a PCI id.
pub(crate) fn parse_hex(text: &str) -> Option<u64> {
    let text = text.trim();
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u64::from_str_radix(digits, 16).ok()
}

/// The kernel's clock tick rate (`USER_HZ`, almost always 100), the unit of
/// `/proc/stat` and `/proc/PID/stat` times.
pub(crate) fn clock_ticks() -> u64 {
    static TICKS: OnceLock<u64> = OnceLock::new();
    *TICKS.get_or_init(|| {
        // SAFETY: sysconf has no preconditions.
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        u64::try_from(ticks).ok().filter(|&t| t > 0).unwrap_or(100)
    })
}

/// `ticks` clock ticks at `hz` ticks per second, as a duration.
pub(crate) fn ticks_to_duration(ticks: u64, hz: u64) -> std::time::Duration {
    let hz = hz.max(1);
    let nanos = (ticks % hz).saturating_mul(1_000_000_000) / hz;
    // Below one second, so it fits and never carries into the seconds.
    std::time::Duration::new(ticks / hz, u32::try_from(nanos).unwrap_or(0))
}

/// The memory page size in bytes, the unit of `/proc/PID/stat`'s RSS.
pub(crate) fn page_size() -> u64 {
    static PAGE: OnceLock<u64> = OnceLock::new();
    *PAGE.get_or_init(|| {
        // SAFETY: sysconf has no preconditions.
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        u64::try_from(size).ok().filter(|&s| s > 0).unwrap_or(4096)
    })
}

/// How many CPUs are online now (at least 1).
pub(crate) fn online_cpus() -> u32 {
    // SAFETY: sysconf has no preconditions.
    let count = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    u32::try_from(count).ok().filter(|&c| c > 0).unwrap_or(1)
}

/// Turn a `uname` field (a NUL-terminated `c_char` array) into a string,
/// stopping at the first NUL or the end of the array.
pub(crate) fn utsname_field(field: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = field
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| {
            let [byte] = c.to_ne_bytes();
            byte
        })
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `uname(2)`, or `None` if it fails.
pub(crate) fn uname() -> Option<libc::utsname> {
    // SAFETY: utsname is plain arrays of c_char, for which all-zero is valid;
    // uname fills it in.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: `name` is a valid, writable utsname.
    (unsafe { libc::uname(&mut name) } == 0).then_some(name)
}

/// A scratch directory for tests that build fake `/sys` trees, under the
/// temporary directory (set `TMPDIR` to keep it elsewhere). It is removed
/// when dropped.
#[cfg(test)]
pub(crate) struct TestTree(pub std::path::PathBuf);

#[cfg(test)]
impl TestTree {
    pub(crate) fn new(name: &str) -> TestTree {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("aizome-sys-sysinfo-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test tree");
        TestTree(dir)
    }

    /// Write `contents` to `relative` (creating parent directories).
    pub(crate) fn file(&self, relative: &str, contents: &str) -> &TestTree {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create test directory");
        }
        std::fs::write(path, contents).expect("write test file");
        self
    }

    /// Make a directory (and its parents).
    pub(crate) fn dir(&self, relative: &str) -> &TestTree {
        std::fs::create_dir_all(self.0.join(relative)).expect("create test directory");
        self
    }

    /// Make a symbolic link at `relative` pointing to `target`.
    pub(crate) fn link(&self, relative: &str, target: &str) -> &TestTree {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create test directory");
        }
        std::os::unix::fs::symlink(target, path).expect("create test link");
        self
    }

    pub(crate) fn path(&self, relative: &str) -> std::path::PathBuf {
        self.0.join(relative)
    }
}

#[cfg(test)]
impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_numbers_parse_with_or_without_prefix() {
        assert_eq!(parse_hex("0x8086\n"), Some(0x8086));
        assert_eq!(parse_hex("b090"), Some(0xb090));
        assert_eq!(parse_hex("0x"), None);
        assert_eq!(parse_hex("zz"), None);
    }

    #[test]
    fn attributes_are_trimmed_and_empty_ones_are_none() {
        let tree = TestTree::new("attr");
        tree.file("a", "SAMSUNG MZVL8512HFLU-00BLL               \n")
            .file("b", "Raspberry Pi 4 Model B Rev 1.4\0")
            .file("c", " \n")
            .file("n", "42\n");
        assert_eq!(read_attr(tree.path("a")).as_deref(), Some("SAMSUNG MZVL8512HFLU-00BLL"));
        assert_eq!(read_attr(tree.path("b")).as_deref(), Some("Raspberry Pi 4 Model B Rev 1.4"));
        assert_eq!(read_attr(tree.path("c")), None);
        assert_eq!(read_attr(tree.path("missing")), None);
        assert_eq!(read_u64(tree.path("n")), Some(42));
    }

    #[test]
    fn file_errors_keep_their_kind() {
        let missing = read_text("/proc/definitely-not-here").unwrap_err();
        assert!(matches!(missing, Error::NotFound(_)), "{missing:?}");
    }

    #[test]
    fn system_constants_are_sane() {
        assert!(clock_ticks() > 0);
        assert!(page_size() >= 4096);
        assert!(online_cpus() >= 1);
        let name = uname().map(|u| utsname_field(&u.machine));
        assert!(name.is_some_and(|m| !m.is_empty()));
    }
}
