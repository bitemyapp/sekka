// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reading /proc and /sys: each process's counters, and the system's. Only
//! parsing lives here; the sampler (sampler.rs) turns two readings into
//! rates. Another user's memory maps, storage counters and file descriptors
//! can't be read without privileges, so those come back as `None`.
use std::{fs, io::Read, path::Path};

/// Read a small file into a reused buffer; false if it can't be read.
pub fn read_into(path: impl AsRef<Path>, buffer: &mut String) -> bool {
    buffer.clear();
    match fs::File::open(path) {
        Ok(mut file) => file.read_to_string(buffer).is_ok(),
        Err(_) => false,
    }
}

pub fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// A number from a sysfs file.
pub fn number(path: impl AsRef<Path>) -> Option<u64> {
    read(path)?.trim().parse().ok()
}

pub fn clock_ticks() -> f64 {
    // SAFETY: sysconf has no preconditions.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 { ticks as f64 } else { 100.0 }
}

/// The kernel marks its own threads with PF_KTHREAD.
const PF_KTHREAD: u64 = 0x0020_0000;

/// /proc/PID/stat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stat {
    pub comm: String,
    pub state: char,
    pub ppid: i32,
    pub kernel: bool,
    /// User and system time, in clock ticks.
    pub time: u64,
    pub nice: i32,
    pub threads: u64,
    /// Start, in clock ticks after boot.
    pub start: u64,
    pub vsize: u64,
}

/// The command name is parenthesized and may itself hold spaces and
/// parentheses, so the fields are read after the last ')'.
pub fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?.to_owned();
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    let field = |index: usize| fields.get(index).copied();
    let num = |index: usize| field(index).and_then(|f| f.parse::<u64>().ok());
    Some(Stat {
        comm,
        state: field(0)?.chars().next()?,
        ppid: field(1)?.parse().ok()?,
        kernel: num(6)? & PF_KTHREAD != 0,
        time: num(11)? + num(12)?,
        nice: field(16)?.parse().ok()?,
        threads: num(17)?,
        start: num(19)?,
        vsize: num(20)?,
    })
}

/// /proc/PID/status: who owns the process, its memory by kind, and its
/// context switches.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub uid: u32,
    pub resident: u64,
    pub anon: u64,
    pub file: u64,
    pub shmem: u64,
    pub swap: u64,
    pub voluntary: u64,
    pub involuntary: u64,
}

pub fn parse_status(text: &str) -> Status {
    let mut status = Status::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let first = value.split_whitespace().next().unwrap_or("");
        let kib = || first.parse::<u64>().unwrap_or(0) * 1024;
        match key {
            "Uid" => status.uid = first.parse().unwrap_or(0),
            "VmRSS" => status.resident = kib(),
            "RssAnon" => status.anon = kib(),
            "RssFile" => status.file = kib(),
            "RssShmem" => status.shmem = kib(),
            "VmSwap" => status.swap = kib(),
            "voluntary_ctxt_switches" => status.voluntary = first.parse().unwrap_or(0),
            "nonvoluntary_ctxt_switches" => status.involuntary = first.parse().unwrap_or(0),
            _ => {}
        }
    }
    status
}

/// /proc/PID/smaps_rollup: the memory maps summed, in bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smaps {
    pub pss: u64,
    pub private: u64,
    pub shared: u64,
    pub swap: u64,
    pub swap_pss: u64,
}

pub fn parse_smaps(text: &str) -> Option<Smaps> {
    let mut smaps = Smaps::default();
    let mut seen = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let kib = || {
            value
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0)
                * 1024
        };
        match key {
            "Pss" => {
                smaps.pss = kib();
                seen = true;
            }
            "Private_Clean" | "Private_Dirty" => smaps.private += kib(),
            "Shared_Clean" | "Shared_Dirty" => smaps.shared += kib(),
            "Swap" => smaps.swap = kib(),
            "SwapPss" => smaps.swap_pss = kib(),
            _ => {}
        }
    }
    seen.then_some(smaps)
}

/// /proc/PID/io: bytes read from and written to storage.
pub fn parse_io(text: &str) -> Option<(u64, u64)> {
    let mut read = None;
    let mut written = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("read_bytes:") {
            read = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("write_bytes:") {
            written = value.trim().parse().ok();
        }
    }
    Some((read?, written?))
}

/// /proc/PID/cmdline: NUL-separated arguments.
pub fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect()
}

/// The unified (v2) control group from /proc/PID/cgroup.
pub fn parse_cgroup(text: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix("0::"))
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// A systemd unit name with its escapes undone: "google\x2dchrome".
pub fn unescape_unit(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(index) = rest.find("\\x") {
        out.push_str(&rest[..index]);
        let hex = rest.get(index + 2..index + 4);
        match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
            Some(byte) => {
                out.push(byte as char);
                rest = &rest[index + 4..];
            }
            None => {
                out.push_str("\\x");
                rest = &rest[index + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One line of /proc/stat's processor times, in clock ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CpuTimes {
    pub user: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub other: u64,
}

impl CpuTimes {
    pub fn total(&self) -> u64 {
        self.user + self.system + self.idle + self.iowait + self.other
    }

    pub fn busy(&self) -> u64 {
        self.user + self.system + self.other
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcStat {
    pub total: CpuTimes,
    pub cores: Vec<CpuTimes>,
    /// Boot time, in seconds since the epoch.
    pub boot: u64,
}

pub fn parse_proc_stat(text: &str) -> ProcStat {
    let mut stat = ProcStat::default();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        if let Some(rest) = name.strip_prefix("cpu") {
            let n: Vec<u64> = fields.map(|f| f.parse().unwrap_or(0)).collect();
            let at = |i: usize| n.get(i).copied().unwrap_or(0);
            // user nice system idle iowait irq softirq steal (guest time is
            // already in user).
            let times = CpuTimes {
                user: at(0) + at(1),
                system: at(2) + at(5) + at(6),
                idle: at(3),
                iowait: at(4),
                other: at(7),
            };
            if rest.is_empty() {
                stat.total = times;
            } else {
                stat.cores.push(times);
            }
        } else if name == "btime" {
            stat.boot = fields.next().and_then(|f| f.parse().ok()).unwrap_or(0);
        }
    }
    stat
}

/// /proc/meminfo, in bytes.
pub fn parse_meminfo(text: &str) -> crate::model::Memory {
    let mut values = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let kib: u64 = value
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            values.insert(key.trim(), kib * 1024);
        }
    }
    let get = |key: &str| values.get(key).copied().unwrap_or(0);
    crate::model::Memory {
        total: get("MemTotal"),
        available: get("MemAvailable"),
        free: get("MemFree"),
        apps: get("AnonPages"),
        cached: get("Cached") + get("SReclaimable"),
        buffers: get("Buffers"),
        shmem: get("Shmem"),
        kernel: get("SUnreclaim") + get("KernelStack") + get("PageTables"),
        swap_total: get("SwapTotal"),
        swap_free: get("SwapFree"),
        compressed: None,
    }
}

/// A /proc/pressure file: the ten-second "some" and "full" averages.
pub fn parse_pressure(text: &str) -> (f64, f64) {
    let avg10 = |kind: &str| {
        text.lines()
            .find(|line| line.starts_with(kind))
            .and_then(|line| {
                line.split_whitespace()
                    .find_map(|field| field.strip_prefix("avg10="))
            })
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0)
    };
    (avg10("some"), avg10("full"))
}

/// One whole disk's counters from /proc/diskstats.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskCounters {
    pub name: String,
    pub sectors_read: u64,
    pub sectors_written: u64,
    /// Milliseconds spent doing I/O.
    pub busy_ms: u64,
}

/// Whole disks only: partitions, loop, RAM, zram and device-mapper devices
/// are left out (`whole` tells whether a name is a disk of its own).
pub fn parse_diskstats(text: &str, whole: impl Fn(&str) -> bool) -> Vec<DiskCounters> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let name = *fields.get(2)?;
            let skip = ["loop", "ram", "zram", "dm-", "sr", "fd"];
            if skip.iter().any(|prefix| name.starts_with(prefix)) || !whole(name) {
                return None;
            }
            let n = |i: usize| fields.get(i).and_then(|f| f.parse::<u64>().ok());
            Some(DiskCounters {
                name: name.to_owned(),
                sectors_read: n(5)?,
                sectors_written: n(9)?,
                busy_ms: n(12)?,
            })
        })
        .collect()
}

/// User names by ID, from /etc/passwd.
pub fn parse_passwd(text: &str) -> std::collections::HashMap<u32, String> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let uid = fields.nth(1)?.parse().ok()?;
            Some((uid, name.to_owned()))
        })
        .collect()
}

/// The processor's model name from /proc/cpuinfo.
pub fn parse_cpu_name(text: &str) -> String {
    text.lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "model name").then(|| value.trim().to_owned())
        })
        .map(|name| {
            // "AMD Ryzen 7 9800X3D 8-Core Processor" reads better without
            // the core count the window shows anyway.
            let name = name
                .replace("(R)", "")
                .replace("(TM)", "")
                .replace("(tm)", "");
            let words: Vec<&str> = name
                .split_whitespace()
                .filter(|word| !word.ends_with("-Core") && *word != "Processor" && *word != "CPU")
                .collect();
            words.join(" ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_survives_odd_command_names() {
        let line = "2591 (.Hyprland-wrapp) S 2580 2580 2580 0 -1 4194560 1 0 0 0 \
                    1234 567 0 0 20 0 9 0 4521 1048576000 40960 18446744073709551615";
        let stat = parse_stat(line).unwrap();
        assert_eq!(stat.comm, ".Hyprland-wrapp");
        assert_eq!(stat.state, 'S');
        assert_eq!(stat.ppid, 2580);
        assert_eq!(stat.time, 1801);
        assert_eq!(stat.nice, 0);
        assert_eq!(stat.threads, 9);
        assert_eq!(stat.start, 4521);
        assert!(!stat.kernel);
        let odd = "7 (a (b) c) R 2 0 0 0 -1 2129984 0 0 0 0 5 5 0 0 0 -20 1 0 10 0 0";
        let stat = parse_stat(odd).unwrap();
        assert_eq!(stat.comm, "a (b) c");
        assert!(stat.kernel);
        assert_eq!(stat.nice, -20);
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn status_and_smaps() {
        let status = "Name:\tchrome\nUid:\t1000\t1000\t1000\t1000\nVmRSS:\t  20480 kB\n\
                      RssAnon:\t  10240 kB\nRssFile:\t   8192 kB\nRssShmem:\t   2048 kB\n\
                      VmSwap:\t     4 kB\nvoluntary_ctxt_switches:\t150\n\
                      nonvoluntary_ctxt_switches:\t7\n";
        let status = parse_status(status);
        assert_eq!(status.uid, 1000);
        assert_eq!(status.resident, 20480 * 1024);
        assert_eq!(status.anon, 10240 * 1024);
        assert_eq!(status.swap, 4096);
        assert_eq!(status.voluntary, 150);
        let smaps = "55d0-7ffd ---p 00000000 00:00 0 [rollup]\nRss: 20480 kB\nPss: 9000 kB\n\
                     Pss_Anon: 7000 kB\nShared_Clean: 6000 kB\nShared_Dirty: 1000 kB\n\
                     Private_Clean: 3000 kB\nPrivate_Dirty: 5000 kB\nSwap: 8 kB\nSwapPss: 4 kB\n";
        let smaps = parse_smaps(smaps).unwrap();
        assert_eq!(smaps.pss, 9000 * 1024);
        assert_eq!(smaps.private, 8000 * 1024);
        assert_eq!(smaps.shared, 7000 * 1024);
        assert_eq!(smaps.swap_pss, 4096);
        assert_eq!(parse_smaps(""), None);
        assert_eq!(
            parse_io("rchar: 5\nwchar: 6\nread_bytes: 4096\nwrite_bytes: 8192\n"),
            Some((4096, 8192))
        );
        assert_eq!(parse_io("rchar: 5\n"), None);
    }

    #[test]
    fn command_lines_groups_and_units() {
        assert_eq!(
            parse_cmdline(b"/nix/store/x/bin/chrome\0--type=renderer\0\0"),
            ["/nix/store/x/bin/chrome", "--type=renderer"]
        );
        let cgroup = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-com.google.Chrome-5203.scope\n";
        assert!(parse_cgroup(cgroup).ends_with("app-com.google.Chrome-5203.scope"));
        assert_eq!(parse_cgroup(""), "");
        assert_eq!(
            unescape_unit("app-google\\x2dchrome-1.scope"),
            "app-google-chrome-1.scope"
        );
        assert_eq!(unescape_unit("odd\\xZZ"), "odd\\xZZ");
    }

    #[test]
    fn system_files() {
        let stat = "cpu  100 10 50 1000 20 5 5 0 0 0\ncpu0 50 5 25 500 10 2 3 0 0 0\n\
                    cpu1 50 5 25 500 10 3 2 0 0 0\nintr 1 2\nbtime 1760000000\n";
        let stat = parse_proc_stat(stat);
        assert_eq!(stat.total.user, 110);
        assert_eq!(stat.total.system, 60);
        assert_eq!(stat.total.idle, 1000);
        assert_eq!(stat.cores.len(), 2);
        assert_eq!(stat.boot, 1760000000);
        assert_eq!(stat.total.total(), 1190);
        let memory = parse_meminfo(
            "MemTotal: 1000 kB\nMemFree: 100 kB\nMemAvailable: 600 kB\nCached: 300 kB\n\
             SReclaimable: 50 kB\nAnonPages: 200 kB\nSUnreclaim: 10 kB\nKernelStack: 5 kB\n\
             PageTables: 5 kB\nSwapTotal: 100 kB\nSwapFree: 40 kB\n",
        );
        assert_eq!(memory.used(), 400 * 1024);
        assert_eq!(memory.cached, 350 * 1024);
        assert_eq!(memory.kernel, 20 * 1024);
        assert_eq!(memory.swap_used(), 60 * 1024);
        assert_eq!(
            parse_pressure(
                "some avg10=1.50 avg60=0.40 avg300=0.10 total=1\nfull avg10=0.25 avg60=0 avg300=0 total=1\n"
            ),
            (1.5, 0.25)
        );
        let disks = parse_diskstats(
            " 259 0 nvme0n1 100 0 2000 50 200 0 4000 80 0 120 130\n\
              259 1 nvme0n1p1 1 0 2 0 0 0 0 0 0 0 0\n   7 0 loop0 1 0 2 0 0 0 0 0 0 0 0\n",
            |name| !name.contains('p') || name == "nvme0n1",
        );
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].sectors_read, 2000);
        assert_eq!(disks[0].sectors_written, 4000);
        assert_eq!(disks[0].busy_ms, 120);
        let users =
            parse_passwd("root:x:0:0::/root:/bin/sh\ncallen:x:1000:100::/home/callen:/bin/bash\n");
        assert_eq!(users[&1000], "callen");
        assert_eq!(
            parse_cpu_name("processor\t: 0\nmodel name\t: AMD Ryzen 7 9800X3D 8-Core Processor\n"),
            "AMD Ryzen 7 9800X3D"
        );
    }
}
