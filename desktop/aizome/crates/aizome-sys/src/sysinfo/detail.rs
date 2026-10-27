//! What [`ProcessSampler`](super::ProcessSampler) doesn't read every time,
//! for an activity monitor: a process's memory by kind and its context
//! switches (`/proc/PID/status`), its proportional and private memory
//! (`/proc/PID/smaps_rollup`), what it read and wrote to storage
//! (`/proc/PID/io`), its working directory, the DRM devices it has open
//! (`/proc/PID/fd`); and the system's pressure stall figures
//! (`/proc/pressure/*`).
//!
//! These cost more than `stat`: `smaps_rollup` walks the process's page
//! tables, and `fd` lists every open file. A [`ProcReader`] reuses one
//! buffer for all of them; read the costly ones less often than every
//! sample, or only for the processes on show. Another user's `smaps_rollup`,
//! `io`, `cwd` and `fd` can't be read without privileges: those come back
//! `None`.

use super::read_into;
use std::path::{Path, PathBuf};

/// `/proc/PID/status`: memory by kind and context switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessStatus {
    pub uid: u32,
    /// Resident memory, all of it (`VmRSS`), in bytes.
    pub resident: u64,
    /// Its own memory (`RssAnon`), file pages (`RssFile`), shared memory
    /// (`RssShmem`).
    pub anon: u64,
    pub file: u64,
    pub shmem: u64,
    /// Swapped out (`VmSwap`).
    pub swap: u64,
    /// The address space (`VmSize`).
    pub virt: u64,
    /// Times it gave up the processor to wait (wake-ups), and times it was
    /// made to.
    pub voluntary_switches: u64,
    pub involuntary_switches: u64,
}

/// `/proc/PID/smaps_rollup`: the memory maps summed, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessMaps {
    /// Proportional set size: private memory plus its share of what it
    /// shares. Summed over processes, it is what they use.
    pub pss: u64,
    /// Memory no other process shares.
    pub private: u64,
    pub shared: u64,
    pub swap: u64,
    pub swap_pss: u64,
}

/// `/proc/PID/io`: bytes read from and written to storage (not the page
/// cache), since the process started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessIo {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

pub fn parse_process_status(text: &str) -> ProcessStatus {
    let mut s = ProcessStatus::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let first = value.split_ascii_whitespace().next().unwrap_or("");
        let n = || first.parse::<u64>().unwrap_or(0);
        match key {
            "Uid" => s.uid = first.parse().unwrap_or(0),
            "VmRSS" => s.resident = n() * 1024,
            "RssAnon" => s.anon = n() * 1024,
            "RssFile" => s.file = n() * 1024,
            "RssShmem" => s.shmem = n() * 1024,
            "VmSwap" => s.swap = n() * 1024,
            "VmSize" => s.virt = n() * 1024,
            "voluntary_ctxt_switches" => s.voluntary_switches = n(),
            "nonvoluntary_ctxt_switches" => s.involuntary_switches = n(),
            _ => {}
        }
    }
    s
}

/// `None` when the text has no `Pss` line (not a rollup).
pub fn parse_smaps_rollup(text: &str) -> Option<ProcessMaps> {
    let mut m = ProcessMaps::default();
    let mut seen = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let kib = || value.split_ascii_whitespace().next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0) * 1024;
        match key {
            "Pss" => {
                m.pss = kib();
                seen = true;
            }
            "Private_Clean" | "Private_Dirty" => m.private += kib(),
            "Shared_Clean" | "Shared_Dirty" => m.shared += kib(),
            "Swap" => m.swap = kib(),
            "SwapPss" => m.swap_pss = kib(),
            _ => {}
        }
    }
    seen.then_some(m)
}

pub fn parse_process_io(text: &str) -> Option<ProcessIo> {
    let mut read = None;
    let mut write = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("read_bytes:") {
            read = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("write_bytes:") {
            write = v.trim().parse().ok();
        }
    }
    Some(ProcessIo { read_bytes: read?, write_bytes: write? })
}

/// Reads the costlier per-process files, reusing one buffer.
#[derive(Debug)]
pub struct ProcReader {
    root: PathBuf,
    buf: Vec<u8>,
}

impl Default for ProcReader {
    fn default() -> ProcReader {
        ProcReader::new()
    }
}

impl ProcReader {
    pub fn new() -> ProcReader {
        ProcReader::with_root("/proc")
    }

    /// A reader of a `/proc`-like tree at `root` (for tests).
    pub fn with_root(root: impl Into<PathBuf>) -> ProcReader {
        ProcReader { root: root.into(), buf: Vec::with_capacity(2048) }
    }

    fn text(&mut self, pid: u32, file: &str) -> Option<&str> {
        let path = self.root.join(pid.to_string()).join(file);
        read_into(&path, &mut self.buf).ok()?;
        std::str::from_utf8(&self.buf).ok()
    }

    pub fn status(&mut self, pid: u32) -> Option<ProcessStatus> {
        self.text(pid, "status").map(parse_process_status)
    }

    pub fn maps(&mut self, pid: u32) -> Option<ProcessMaps> {
        self.text(pid, "smaps_rollup").and_then(parse_smaps_rollup)
    }

    pub fn io(&mut self, pid: u32) -> Option<ProcessIo> {
        self.text(pid, "io").and_then(parse_process_io)
    }

    pub fn cwd(&self, pid: u32) -> Option<PathBuf> {
        std::fs::read_link(self.root.join(pid.to_string()).join("cwd")).ok()
    }

    /// The file descriptors that are DRM devices (`/dev/dri/*`), for
    /// [`super::drm`]'s per-process GPU use.
    pub fn drm_fds(&self, pid: u32) -> Vec<u32> {
        let dir = self.root.join(pid.to_string()).join("fd");
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut fds: Vec<u32> = entries
            .flatten()
            .filter(|e| std::fs::read_link(e.path()).is_ok_and(|t| t.starts_with("/dev/dri/")))
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .collect();
        fds.sort_unstable();
        fds
    }

    /// `/proc/PID/fdinfo/FD`'s text.
    pub fn fdinfo(&mut self, pid: u32, fd: u32) -> Option<&str> {
        let path = self.root.join(pid.to_string()).join("fdinfo").join(fd.to_string());
        read_into(&path, &mut self.buf).ok()?;
        std::str::from_utf8(&self.buf).ok()
    }
}

/// Pressure stall information: the share of the last ten seconds some (or
/// all) tasks waited for a resource, in percent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pressure {
    pub cpu_some: f32,
    pub memory_some: f32,
    pub memory_full: f32,
    pub io_some: f32,
    pub io_full: f32,
}

/// How hard pressed memory is, in words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Strain {
    Low,
    Moderate,
    High,
}

impl Pressure {
    pub fn memory_strain(&self) -> Strain {
        if self.memory_full >= 5.0 || self.memory_some >= 30.0 {
            Strain::High
        } else if self.memory_full >= 0.5 || self.memory_some >= 5.0 {
            Strain::Moderate
        } else {
            Strain::Low
        }
    }
}

/// `avg10` of a pressure file's `some` and `full` lines.
pub fn parse_pressure(text: &str) -> (f32, f32) {
    let avg10 = |kind: &str| {
        text.lines()
            .find(|l| l.starts_with(kind))
            .and_then(|l| l.split_ascii_whitespace().find_map(|f| f.strip_prefix("avg10=")))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    };
    (avg10("some"), avg10("full"))
}

/// The system's pressure, or `None` without PSI (`CONFIG_PSI`).
pub fn pressure() -> Option<Pressure> {
    pressure_at(Path::new("/proc/pressure"))
}

pub(crate) fn pressure_at(dir: &Path) -> Option<Pressure> {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok().map(|t| parse_pressure(&t));
    let (cpu_some, _) = read("cpu")?;
    let (memory_some, memory_full) = read("memory").unwrap_or_default();
    let (io_some, io_full) = read("io").unwrap_or_default();
    Some(Pressure { cpu_some, memory_some, memory_full, io_some, io_full })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    #[test]
    fn status_maps_and_io_parse() {
        let s = parse_process_status(
            "Name:\tchrome\nUid:\t1000\t1000\t1000\t1000\nVmSize:\t 100000 kB\nVmRSS:\t  20480 kB\nRssAnon:\t  10240 kB\n\
             RssFile:\t   8192 kB\nRssShmem:\t   2048 kB\nVmSwap:\t     4 kB\nvoluntary_ctxt_switches:\t150\nnonvoluntary_ctxt_switches:\t7\n",
        );
        assert_eq!(
            (s.uid, s.resident, s.anon, s.swap, s.virt),
            (1000, 20480 * 1024, 10240 * 1024, 4096, 100000 * 1024)
        );
        assert_eq!((s.voluntary_switches, s.involuntary_switches), (150, 7));
        let m = parse_smaps_rollup(
            "55a0-7ffd ---p 00000000 00:00 0 [rollup]\nRss: 2000 kB\nPss: 1500 kB\nShared_Clean: 400 kB\nShared_Dirty: 100 kB\n\
             Private_Clean: 300 kB\nPrivate_Dirty: 1200 kB\nSwap: 8 kB\nSwapPss: 4 kB\n",
        )
        .unwrap();
        assert_eq!(
            (m.pss, m.private, m.shared, m.swap, m.swap_pss),
            (1500 * 1024, 1500 * 1024, 500 * 1024, 8192, 4096)
        );
        assert_eq!(parse_smaps_rollup("nothing"), None);
        let io =
            parse_process_io("rchar: 1\nwchar: 2\nread_bytes: 4096\nwrite_bytes: 8192\ncancelled_write_bytes: 0\n")
                .unwrap();
        assert_eq!((io.read_bytes, io.write_bytes), (4096, 8192));
        assert_eq!(parse_process_io("rchar: 1\n"), None);
    }

    #[test]
    fn a_reader_reads_a_proc_tree() {
        let tree = TestTree::new("detail");
        tree.file("42/status", "Uid:\t7\t7\t7\t7\nVmRSS:\t 8 kB\n").file("42/io", "read_bytes: 1\nwrite_bytes: 2\n");
        let mut r = ProcReader::with_root(&tree.0);
        assert_eq!(r.status(42).unwrap().resident, 8192);
        assert_eq!(r.io(42).unwrap().write_bytes, 2);
        assert_eq!(r.maps(42), None, "unreadable or missing: None");
        assert_eq!(r.status(43), None);
        assert!(r.drm_fds(42).is_empty());
    }

    #[test]
    fn pressure_and_strain() {
        let tree = TestTree::new("psi");
        tree.file("cpu", "some avg10=1.50 avg60=0.10 avg300=0.00 total=1\n")
            .file("memory", "some avg10=8.00 avg60=1 avg300=0 total=2\nfull avg10=0.40 avg60=0 avg300=0 total=3\n");
        let p = pressure_at(&tree.0).unwrap();
        assert_eq!((p.cpu_some, p.memory_some, p.memory_full, p.io_some), (1.5, 8.0, 0.4, 0.0));
        assert_eq!(p.memory_strain(), Strain::Moderate);
        assert_eq!(Pressure::default().memory_strain(), Strain::Low);
        assert_eq!(Pressure { memory_full: 6.0, ..Default::default() }.memory_strain(), Strain::High);
    }
}
