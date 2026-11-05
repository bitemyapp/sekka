//! `system_usage`: what's using this computer, every figure labelled. Memory,
//! the processes using the most memory and processor time, and each disk's
//! space, read from /proc and statvfs. `free`, `ps`, `top` and `df` print
//! bare columns a model has to line up with their headers, and small models
//! slip: Gemma 4 26B-A4B once read `ps`'s %CPU column as memory.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

/// How long processor time is measured over.
const SAMPLE: Duration = Duration::from_millis(500);
const TOP_MEMORY: usize = 8;
const TOP_PROCESSOR: usize = 5;
/// Processes below this share of one thread aren't listed as busy.
const BUSY_PERCENT: f64 = 1.0;

#[derive(Clone, Debug, PartialEq)]
struct Process {
    pid: u32,
    name: String,
    user: String,
    /// Resident memory.
    rss: u64,
    /// Processor time so far, in clock ticks.
    ticks: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct Memory {
    total: u64,
    available: u64,
    swap_total: u64,
    swap_free: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct Disk {
    mount: String,
    source: String,
    fstype: String,
    size: u64,
    /// What an unprivileged user can still write.
    free: u64,
    /// Used, as `df` counts it: of what's used plus what's free to users.
    used_percent: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct Usage {
    memory: Option<Memory>,
    /// Busy share of all threads over the sample, and the thread count.
    busy: Option<(f64, usize)>,
    /// (process, share of one thread over the sample).
    by_memory: Vec<Process>,
    by_processor: Vec<(Process, f64)>,
    disks: Vec<Disk>,
}

/// The report the model and the person see.
pub(crate) fn report() -> String {
    render(&gather())
}

fn gather() -> Usage {
    let users = users();
    let before = processes(&users);
    let busy_before = processor_ticks();
    std::thread::sleep(SAMPLE);
    let after = processes(&users);
    let busy_after = processor_ticks();

    let hz = match unsafe { libc::sysconf(libc::_SC_CLK_TCK) } {
        n if n > 0 => n as f64,
        _ => 100.0,
    };
    let earlier: HashMap<u32, u64> = before.iter().map(|p| (p.pid, p.ticks)).collect();
    let mut by_processor: Vec<(Process, f64)> = after
        .iter()
        .filter_map(|p| {
            let spent = p.ticks.saturating_sub(*earlier.get(&p.pid)?);
            Some((p.clone(), spent as f64 / hz / SAMPLE.as_secs_f64() * 100.0))
        })
        .filter(|(_, pct)| *pct >= BUSY_PERCENT)
        .collect();
    by_processor.sort_by(|a, b| b.1.total_cmp(&a.1));
    by_processor.truncate(TOP_PROCESSOR);

    let mut by_memory: Vec<Process> = after.into_iter().filter(|p| p.rss > 0).collect();
    by_memory.sort_by_key(|p| std::cmp::Reverse(p.rss));
    by_memory.truncate(TOP_MEMORY);

    let busy = match (busy_before, busy_after) {
        (Some((b0, t0, n)), Some((b1, t1, _))) if t1 > t0 => Some(((b1 - b0) as f64 / (t1 - t0) as f64 * 100.0, n)),
        _ => None,
    };
    Usage {
        memory: fs::read_to_string("/proc/meminfo").ok().and_then(|s| meminfo(&s)),
        busy,
        by_memory,
        by_processor,
        disks: fs::read_to_string("/proc/self/mounts").map(|s| disks(&s)).unwrap_or_default(),
    }
}

fn render(u: &Usage) -> String {
    let mut out = String::new();
    match &u.memory {
        Some(m) => {
            let swap = if m.swap_total == 0 {
                "Swap: none.".to_owned()
            } else {
                format!("Swap: {} of {} in use.", size(m.swap_total.saturating_sub(m.swap_free)), size(m.swap_total))
            };
            out += &format!(
                "Memory: {} in all; {} available to programs ({:.0}%), {} in use. {swap}\n",
                size(m.total),
                size(m.available),
                percent(m.available, m.total),
                size(m.total.saturating_sub(m.available)),
            );
        }
        None => out += "Memory: unknown (no /proc/meminfo).\n",
    }
    if let Some((busy, threads)) = u.busy {
        out += &format!(
            "Processor: {busy:.0}% busy over the last {:.1} s, across {threads} threads.\n",
            SAMPLE.as_secs_f64()
        );
    }
    let who = |p: &Process| format!("{}, pid {}, user {}", p.name, p.pid, p.user);
    out += "\nUsing the most memory (resident memory, and its share of all memory):\n";
    let total = u.memory.as_ref().map_or(0, |m| m.total);
    for p in &u.by_memory {
        out += &format!("- {}: {}, {:.1}% of memory\n", who(p), size(p.rss), percent(p.rss, total));
    }
    out += &format!(
        "\nUsing the most processor time over the last {:.1} s (100% is one thread kept busy):\n",
        SAMPLE.as_secs_f64()
    );
    if u.by_processor.is_empty() {
        out += &format!("- No process used {BUSY_PERCENT:.0}% of a thread or more.\n");
    }
    for (p, pct) in &u.by_processor {
        out += &format!("- {}: {pct:.0}% of a thread\n", who(p));
    }
    out += "\nDisks (free is what can still be written):\n";
    if u.disks.is_empty() {
        out += "- None found.\n";
    }
    for d in &u.disks {
        out += &format!(
            "- {} on {} ({}): {} free of {}, {:.0}% used\n",
            d.mount,
            d.source,
            d.fstype,
            size(d.free),
            size(d.size),
            d.used_percent
        );
    }
    out
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { part as f64 / whole as f64 * 100.0 }
}

/// Bytes in binary units, as `df -h` and `free -h` mean them, but named.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["bytes", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} bytes"),
        _ if v < 10.0 => format!("{v:.1} {}", UNITS[unit]),
        _ => format!("{v:.0} {}", UNITS[unit]),
    }
}

fn meminfo(text: &str) -> Option<Memory> {
    let kib = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map(|v| v * 1024)
    };
    Some(Memory {
        total: kib("MemTotal")?,
        available: kib("MemAvailable")?,
        swap_total: kib("SwapTotal").unwrap_or(0),
        swap_free: kib("SwapFree").unwrap_or(0),
    })
}

/// (busy ticks, all ticks, threads) from /proc/stat.
fn processor_ticks() -> Option<(u64, u64, usize)> {
    let text = fs::read_to_string("/proc/stat").ok()?;
    let first = text.lines().next()?.strip_prefix("cpu ")?;
    let v: Vec<u64> = first.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    // user nice system idle iowait irq softirq steal …
    let all: u64 = v.iter().take(8).sum();
    let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
    let threads = text.lines().filter(|l| l.starts_with("cpu") && !l.starts_with("cpu ")).count();
    Some((all.saturating_sub(idle), all, threads))
}

/// Login names by uid, from /etc/passwd.
fn users() -> HashMap<u32, String> {
    fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            Some((f.get(2)?.parse().ok()?, (*f.first()?).to_owned()))
        })
        .collect()
}

fn processes(users: &HashMap<u32, String>) -> Vec<Process> {
    let Ok(dir) = fs::read_dir("/proc") else { return Vec::new() };
    dir.filter_map(|e| {
        let pid: u32 = e.ok()?.file_name().to_str()?.parse().ok()?;
        let base = Path::new("/proc").join(pid.to_string());
        let (comm, ticks) = stat(&fs::read_to_string(base.join("stat")).ok()?)?;
        let status = fs::read_to_string(base.join("status")).unwrap_or_default();
        let field = |key: &str| status.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':').map(str::trim));
        let rss = field("VmRSS").and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0) * 1024;
        let uid: Option<u32> = field("Uid").and_then(|v| v.split_whitespace().next()?.parse().ok());
        let user = uid.map(|u| users.get(&u).cloned().unwrap_or_else(|| u.to_string())).unwrap_or_default();
        Some(Process { pid, name: name(&comm, &fs::read(base.join("cmdline")).unwrap_or_default()), user, rss, ticks })
    })
    .collect()
}

/// (comm, utime + stime) from a /proc/PID/stat line. The name is in
/// parentheses and may hold spaces or parentheses itself.
fn stat(line: &str) -> Option<(String, u64)> {
    let (open, close) = (line.find('(')?, line.rfind(')')?);
    let comm = line.get(open + 1..close)?.to_owned();
    let rest: Vec<&str> = line.get(close + 2..)?.split_whitespace().collect();
    // After the name: state ppid … utime is the 12th field, stime the 13th.
    let utime: u64 = rest.get(11)?.parse().ok()?;
    let stime: u64 = rest.get(12)?.parse().ok()?;
    Some((comm, utime + stime))
}

/// The kernel cuts a process's name to 15 bytes; the command line's first
/// word has the whole one.
fn name(comm: &str, cmdline: &[u8]) -> String {
    if comm.len() < 15 {
        return comm.to_owned();
    }
    let argv0 = cmdline.split(|&b| b == 0).next().unwrap_or_default();
    let base = argv0.rsplit(|&b| b == b'/').next().unwrap_or_default();
    match std::str::from_utf8(base) {
        Ok(s) if s.starts_with(comm) => s.to_owned(),
        _ => comm.to_owned(),
    }
}

/// Filesystems on devices (and ZFS, bcachefs), once each, from /proc/self/mounts.
fn disks(mounts: &str) -> Vec<Disk> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in mounts.lines() {
        let f: Vec<String> = line.split_whitespace().map(unescape).collect();
        let [source, mount, fstype, ..] = f.as_slice() else { continue };
        let on_device = source.starts_with("/dev/") || matches!(fstype.as_str(), "zfs" | "bcachefs");
        if !on_device || matches!(fstype.as_str(), "squashfs" | "iso9660") || !seen.insert(source.clone()) {
            continue;
        }
        if let Some((size, free, used_percent)) = space(mount) {
            out.push(Disk {
                mount: mount.clone(),
                source: source.clone(),
                fstype: fstype.clone(),
                size,
                free,
                used_percent,
            });
        }
    }
    out
}

/// /proc/mounts writes space, tab, newline and backslash as octal escapes.
fn unescape(field: &str) -> String {
    let mut out = String::new();
    let mut chars = field.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let digits: String = (0..3).filter_map(|_| chars.next_if(|d| d.is_digit(8))).collect();
            if let (3, Ok(n)) = (digits.len(), u8::from_str_radix(&digits, 8)) {
                out.push(n as char);
                continue;
            }
            out.push(c);
            out.push_str(&digits);
        } else {
            out.push(c);
        }
    }
    out
}

/// (size, free to users, used %) of the filesystem at `mount`.
fn space(mount: &str) -> Option<(u64, u64, f64)> {
    let path = CString::new(Path::new(mount).as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut s) } != 0 || s.f_blocks == 0 {
        return None;
    }
    let unit = s.f_frsize as u64;
    let size = s.f_blocks as u64 * unit;
    let used = (s.f_blocks as u64).saturating_sub(s.f_bfree as u64) * unit;
    let free = s.f_bavail as u64 * unit;
    Some((size, free, percent(used, used + free)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_lines_with_awkward_names() {
        let line = "4242 (Web (Content) x) S 1 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 30 0 1000 0 0";
        assert_eq!(stat(line), Some(("Web (Content) x".into(), 300)));
        assert_eq!(stat("1 (init"), None);
    }

    #[test]
    fn long_names_come_from_the_command_line() {
        assert_eq!(name("aizome", b"/run/x/bin/aizome\0--backend\0udev"), "aizome");
        assert_eq!(name("xdg-desktop-por", b"/nix/store/x/libexec/xdg-desktop-portal-gtk\0"), "xdg-desktop-portal-gtk");
        // A renamed process keeps its own name.
        assert_eq!(name("abcdefghijklmno", b"/usr/bin/python3\0x.py"), "abcdefghijklmno");
    }

    #[test]
    fn mounts_once_per_device_and_unescaped() {
        assert_eq!(unescape("/mnt/My\\040Disk"), "/mnt/My Disk");
        assert_eq!(unescape("a\\b\\12"), "a\\b\\12");
        // The root is on every machine, so its line is measured; the same
        // device's second mount, pseudo filesystems and squashfs aren't.
        let d = disks(
            "/dev/x / ext4 rw 0 0\n/dev/x /nix/store ext4 ro 0 0\nproc /proc proc rw 0 0\n\
             tmpfs /run tmpfs rw 0 0\n/dev/loop0 / squashfs ro 0 0\n",
        );
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!((d[0].mount.as_str(), d[0].source.as_str()), ("/", "/dev/x"));
        assert!(d[0].size > 0 && d[0].free <= d[0].size);
    }

    #[test]
    fn every_figure_is_labelled() {
        let p = |pid, name: &str, rss| Process { pid, name: name.into(), user: "alice".into(), rss, ticks: 0 };
        let u = Usage {
            memory: Some(Memory { total: 4 << 30, available: 3 << 30, swap_total: 0, swap_free: 0 }),
            busy: Some((12.0, 16)),
            by_memory: vec![p(984, "aizome", 91 << 20), p(993, "aizome-shell", 50 << 20)],
            by_processor: vec![(p(1203, "media-indexer", 0), 99.4)],
            disks: vec![Disk {
                mount: "/srv/data".into(),
                source: "/dev/vdb".into(),
                fstype: "ext4".into(),
                size: 59 << 20,
                free: 36 << 10,
                used_percent: 99.9,
            }],
        };
        let r = render(&u);
        assert!(
            r.contains("Memory: 4.0 GiB in all; 3.0 GiB available to programs (75%), 1.0 GiB in use. Swap: none."),
            "{r}"
        );
        assert!(r.contains("Processor: 12% busy over the last 0.5 s, across 16 threads."), "{r}");
        assert!(r.contains("- aizome, pid 984, user alice: 91 MiB, 2.2% of memory"), "{r}");
        assert!(r.contains("- media-indexer, pid 1203, user alice: 99% of a thread"), "{r}");
        assert!(r.contains("- /srv/data on /dev/vdb (ext4): 36 KiB free of 59 MiB, 100% used"), "{r}");
    }

    #[test]
    fn a_real_report() {
        let r = report();
        assert!(r.starts_with("Memory: ") && r.contains("Using the most memory") && r.contains("Disks"), "{r}");
    }
}
