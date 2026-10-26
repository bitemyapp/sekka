//! Disk and network throughput, from `/proc/diskstats` and `/proc/net/dev`.
//!
//! Both files count bytes since boot (diskstats in 512-byte sectors,
//! whatever the device's real sector size). [`DiskIoSampler`] and
//! [`NetSampler`] turn them into bytes per second over the time since the
//! previous `sample()`; the first sample after `new()` reports zero rates,
//! with the totals since boot. A counter that goes backwards (a device
//! removed and added again, a 32-bit counter wrapping) counts as no
//! traffic for that interval.
//!
//! **Disks.** Whole disks are the names that have a directory in
//! `/sys/block`; the rest are partitions, given with the disk they are on.
//! Disks without a `device` link there (`loop`, `dm-`, `md`, `zram`) are
//! *virtual*: their traffic is also counted on the disks beneath them, so
//! [`DiskIo`]'s totals add up only physical whole disks. `loop*`, `ram*`
//! and `zram*` devices that have never been used are left out.
//!
//! **Network.** Interfaces without a `/sys/class/net/NAME/device` link
//! (bridges, VPN tunnels, `veth`, `docker0`) are *virtual*; the loopback
//! interface has `IFF_LOOPBACK` in its `flags`. [`NetIo`]'s totals add up
//! only physical interfaces, since traffic on a VPN or bridge also crosses
//! a physical one.

use super::{parse_hex, read_attr};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

const DISKSTATS: &str = "/proc/diskstats";
const SYS_BLOCK: &str = "/sys/block";
const NET_DEV: &str = "/proc/net/dev";
const SYS_CLASS_NET: &str = "/sys/class/net";

/// `/proc/diskstats` sectors are always this many bytes.
const SECTOR: u64 = 512;
/// `IFF_LOOPBACK` in `/sys/class/net/NAME/flags`.
const IFF_LOOPBACK: u64 = 0x8;

/// Disk throughput over the time since the previous sample.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskIo {
    /// Each disk and partition, in `/proc/diskstats` order (a disk, then
    /// its partitions).
    pub devices: Vec<DiskIoStat>,
    /// Bytes read per second, summed over physical whole disks.
    pub read_per_sec: f64,
    /// Bytes written per second, summed over physical whole disks.
    pub write_per_sec: f64,
}

/// One block device's throughput.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskIoStat {
    /// The kernel's name, such as `nvme0n1`, `nvme0n1p2` or `sda`.
    pub name: String,
    /// Whether it is a whole disk rather than a partition.
    pub whole_disk: bool,
    /// For a partition, the whole disk it is on.
    pub parent: Option<String>,
    /// Whether it is (or is on) a virtual device such as `dm-0`, `loop0`,
    /// `md0` or `zram0`, whose traffic also shows on the disks beneath it.
    pub virtual_device: bool,
    /// Bytes read per second.
    pub read_per_sec: f64,
    /// Bytes written per second.
    pub write_per_sec: f64,
    /// Bytes read since boot.
    pub read_total: u64,
    /// Bytes written since boot.
    pub written_total: u64,
    /// Share of the interval the device was busy with I/O, 0–100 (like
    /// `iostat`'s `%util`).
    pub busy_percent: f32,
}

/// One `/proc/diskstats` line's counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DiskCounters {
    pub name: String,
    pub sectors_read: u64,
    pub sectors_written: u64,
    /// Milliseconds spent doing I/O.
    pub io_ms: u64,
}

/// Parse `/proc/diskstats`, skipping malformed lines and unused `loop*`,
/// `ram*` and `zram*` devices.
pub(crate) fn parse_diskstats(text: &str) -> Vec<DiskCounters> {
    let mut disks = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_ascii_whitespace().collect();
        let number = |i: usize| fields.get(i).and_then(|f| f.parse::<u64>().ok());
        let (Some(name), Some(sectors_read), Some(sectors_written), Some(io_ms)) =
            (fields.get(2), number(5), number(9), number(12))
        else {
            continue;
        };
        let unused = sectors_read == 0 && sectors_written == 0 && io_ms == 0;
        if unused && ["loop", "ram", "zram"].iter().any(|p| name.starts_with(p)) {
            continue;
        }
        disks.push(DiskCounters { name: (*name).to_owned(), sectors_read, sectors_written, io_ms });
    }
    disks
}

/// What `/sys/block` says about a device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DiskClass {
    whole_disk: bool,
    parent: Option<String>,
    virtual_device: bool,
}

/// Classify `name` against `sys_block` (normally `/sys/block`). `disks`
/// are the other names in the same `/proc/diskstats`, to find a
/// partition's disk.
fn classify_disk(sys_block: &Path, name: &str, disks: &[DiskCounters]) -> DiskClass {
    // sysfs spells a '/' in a device name (cciss/c0d0) as '!'.
    let sys_name = |name: &str| name.replace('/', "!");
    let is_virtual = |disk: &str| !sys_block.join(sys_name(disk)).join("device").exists();
    if sys_block.join(sys_name(name)).is_dir() {
        return DiskClass { whole_disk: true, parent: None, virtual_device: is_virtual(name) };
    }
    let parent = disks
        .iter()
        .map(|d| d.name.as_str())
        .filter(|&d| d != name && name.starts_with(d))
        .find(|&d| sys_block.join(sys_name(d)).join(sys_name(name)).is_dir());
    DiskClass { whole_disk: false, parent: parent.map(str::to_owned), virtual_device: parent.is_none_or(is_virtual) }
}

/// Measures disk throughput between calls to [`sample`](Self::sample); the
/// first sample reports zero rates.
#[derive(Debug)]
pub struct DiskIoSampler {
    previous: HashMap<String, DiskCounters>,
    classes: HashMap<String, DiskClass>,
    last: Option<Instant>,
    diskstats: PathBuf,
    sys_block: PathBuf,
}

impl Default for DiskIoSampler {
    fn default() -> DiskIoSampler {
        DiskIoSampler::new()
    }
}

impl DiskIoSampler {
    /// A sampler with no previous reading.
    pub fn new() -> DiskIoSampler {
        DiskIoSampler::with_paths(DISKSTATS, SYS_BLOCK)
    }

    pub(crate) fn with_paths(diskstats: impl Into<PathBuf>, sys_block: impl Into<PathBuf>) -> DiskIoSampler {
        DiskIoSampler {
            previous: HashMap::new(),
            classes: HashMap::new(),
            last: None,
            diskstats: diskstats.into(),
            sys_block: sys_block.into(),
        }
    }

    /// Read `/proc/diskstats` and report throughput since the previous
    /// call. If it can't be read, the result is empty.
    pub fn sample(&mut self) -> DiskIo {
        match std::fs::read_to_string(&self.diskstats) {
            Ok(text) => self.update(parse_diskstats(&text), Instant::now()),
            Err(error) => {
                log::debug!("{}: {error}", self.diskstats.display());
                DiskIo::default()
            }
        }
    }

    pub(crate) fn update(&mut self, disks: Vec<DiskCounters>, now: Instant) -> DiskIo {
        let seconds = self.last.map(|last| now.saturating_duration_since(last).as_secs_f64()).filter(|&s| s > 0.0);
        self.last = Some(now);
        self.classes.retain(|name, _| disks.iter().any(|d| &d.name == name));

        let mut io = DiskIo::default();
        for disk in &disks {
            let class = self
                .classes
                .entry(disk.name.clone())
                .or_insert_with(|| classify_disk(&self.sys_block, &disk.name, &disks))
                .clone();
            let mut stat = DiskIoStat {
                name: disk.name.clone(),
                whole_disk: class.whole_disk,
                parent: class.parent,
                virtual_device: class.virtual_device,
                read_total: disk.sectors_read.saturating_mul(SECTOR),
                written_total: disk.sectors_written.saturating_mul(SECTOR),
                ..DiskIoStat::default()
            };
            if let (Some(seconds), Some(before)) = (seconds, self.previous.get(&disk.name)) {
                let delta = |now: u64, then: u64| now.saturating_sub(then) as f64;
                stat.read_per_sec = delta(disk.sectors_read, before.sectors_read) * SECTOR as f64 / seconds;
                stat.write_per_sec = delta(disk.sectors_written, before.sectors_written) * SECTOR as f64 / seconds;
                stat.busy_percent =
                    (delta(disk.io_ms, before.io_ms) / (seconds * 1000.0) * 100.0).clamp(0.0, 100.0) as f32;
            }
            if stat.whole_disk && !stat.virtual_device {
                io.read_per_sec += stat.read_per_sec;
                io.write_per_sec += stat.write_per_sec;
            }
            io.devices.push(stat);
        }
        self.previous = disks.into_iter().map(|d| (d.name.clone(), d)).collect();
        io
    }
}

/// Network throughput over the time since the previous sample.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetIo {
    /// Each interface, in `/proc/net/dev` order.
    pub interfaces: Vec<InterfaceIo>,
    /// Bytes received per second, summed over physical interfaces.
    pub rx_per_sec: f64,
    /// Bytes sent per second, summed over physical interfaces.
    pub tx_per_sec: f64,
}

/// One network interface's throughput.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterfaceIo {
    /// The interface name, such as `wlp0s20f3` or `lo`.
    pub name: String,
    /// Whether it is the loopback interface.
    pub loopback: bool,
    /// Whether it is virtual (no hardware device): bridges, tunnels, VPNs,
    /// container links, and loopback.
    pub virtual_device: bool,
    /// Bytes received per second.
    pub rx_per_sec: f64,
    /// Bytes sent per second.
    pub tx_per_sec: f64,
    /// Bytes received since the interface appeared.
    pub rx_total: u64,
    /// Bytes sent since the interface appeared.
    pub tx_total: u64,
}

/// One `/proc/net/dev` line's byte counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NetCounters {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Parse `/proc/net/dev`: two header lines, then `NAME: ` and eight receive
/// counters followed by eight transmit counters, bytes first in each.
pub(crate) fn parse_net_dev(text: &str) -> Vec<NetCounters> {
    let mut interfaces = Vec::new();
    for line in text.lines() {
        let Some((name, counters)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.contains('|') {
            continue;
        }
        let counters: Vec<&str> = counters.split_ascii_whitespace().collect();
        let number = |i: usize| counters.get(i).and_then(|f| f.parse::<u64>().ok());
        let (Some(rx_bytes), Some(tx_bytes)) = (number(0), number(8)) else {
            continue;
        };
        interfaces.push(NetCounters { name: name.to_owned(), rx_bytes, tx_bytes });
    }
    interfaces
}

/// Whether interface `name` is loopback, and whether it is virtual, from
/// `sys_class_net` (normally `/sys/class/net`).
fn classify_interface(sys_class_net: &Path, name: &str) -> (bool, bool) {
    let dir = sys_class_net.join(name);
    let loopback = read_attr(dir.join("flags"))
        .and_then(|flags| parse_hex(&flags))
        .map_or(name == "lo", |flags| flags & IFF_LOOPBACK != 0);
    let virtual_device = loopback || !dir.join("device").exists();
    (loopback, virtual_device)
}

/// Measures network throughput between calls to [`sample`](Self::sample);
/// the first sample reports zero rates.
#[derive(Debug)]
pub struct NetSampler {
    previous: HashMap<String, NetCounters>,
    classes: HashMap<String, (bool, bool)>,
    last: Option<Instant>,
    net_dev: PathBuf,
    sys_class_net: PathBuf,
}

impl Default for NetSampler {
    fn default() -> NetSampler {
        NetSampler::new()
    }
}

impl NetSampler {
    /// A sampler with no previous reading.
    pub fn new() -> NetSampler {
        NetSampler::with_paths(NET_DEV, SYS_CLASS_NET)
    }

    pub(crate) fn with_paths(net_dev: impl Into<PathBuf>, sys_class_net: impl Into<PathBuf>) -> NetSampler {
        NetSampler {
            previous: HashMap::new(),
            classes: HashMap::new(),
            last: None,
            net_dev: net_dev.into(),
            sys_class_net: sys_class_net.into(),
        }
    }

    /// Read `/proc/net/dev` and report throughput since the previous call.
    /// If it can't be read, the result is empty.
    pub fn sample(&mut self) -> NetIo {
        match std::fs::read_to_string(&self.net_dev) {
            Ok(text) => self.update(parse_net_dev(&text), Instant::now()),
            Err(error) => {
                log::debug!("{}: {error}", self.net_dev.display());
                NetIo::default()
            }
        }
    }

    pub(crate) fn update(&mut self, interfaces: Vec<NetCounters>, now: Instant) -> NetIo {
        let seconds = self.last.map(|last| now.saturating_duration_since(last).as_secs_f64()).filter(|&s| s > 0.0);
        self.last = Some(now);
        self.classes.retain(|name, _| interfaces.iter().any(|i| &i.name == name));

        let mut io = NetIo::default();
        for interface in &interfaces {
            let (loopback, virtual_device) = *self
                .classes
                .entry(interface.name.clone())
                .or_insert_with(|| classify_interface(&self.sys_class_net, &interface.name));
            let mut stat = InterfaceIo {
                name: interface.name.clone(),
                loopback,
                virtual_device,
                rx_total: interface.rx_bytes,
                tx_total: interface.tx_bytes,
                ..InterfaceIo::default()
            };
            if let (Some(seconds), Some(before)) = (seconds, self.previous.get(&interface.name)) {
                stat.rx_per_sec = interface.rx_bytes.saturating_sub(before.rx_bytes) as f64 / seconds;
                stat.tx_per_sec = interface.tx_bytes.saturating_sub(before.tx_bytes) as f64 / seconds;
            }
            if !stat.virtual_device {
                io.rx_per_sec += stat.rx_per_sec;
                io.tx_per_sec += stat.tx_per_sec;
            }
            io.interfaces.push(stat);
        }
        self.previous = interfaces.into_iter().map(|i| (i.name.clone(), i)).collect();
        io
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;
    use std::time::Duration;

    // From this machine, plus a dm-crypt volume and an unused zram.
    const DISKSTATS_1: &str = "\
 259       0 nvme0n1 2793955 412177 254839442 7491297 1735142 8216436 422136444 302804832 0 2670212 310508192 0 0 0 0 61351 212062
 259       1 nvme0n1p1 360 1833 12024 166 14 0 12 6 0 23 172 0 0 0 0 0 0
 259       2 nvme0n1p2 2793439 410344 254819354 7490708 1735125 8216436 422136432 302804824 0 2536389 310295533 0 0 0 0 0 0
   7       0 loop0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
   7       1 loop1 120 0 4000 10 0 0 0 0 0 12 10 0 0 0 0 0 0
 254       0 dm-0 2793000 0 254800000 7490000 1735000 0 422136000 302804000 0 2536000 310295000 0 0 0 0 0 0
 252       0 zram0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
   8       0 sda 1 2 3
";

    fn fake_sys_block() -> TestTree {
        let tree = TestTree::new("sys-block");
        tree.dir("nvme0n1/device").dir("nvme0n1/nvme0n1p1").dir("nvme0n1/nvme0n1p2").dir("loop1").dir("dm-0");
        tree
    }

    #[test]
    fn diskstats_parses_and_skips_unused_virtual_devices() {
        let disks = parse_diskstats(DISKSTATS_1);
        let names: Vec<&str> = disks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["nvme0n1", "nvme0n1p1", "nvme0n1p2", "loop1", "dm-0"]);
        assert_eq!(disks[0].sectors_read, 254839442);
        assert_eq!(disks[0].sectors_written, 422136444);
        assert_eq!(disks[0].io_ms, 2670212);
        assert!(parse_diskstats("").is_empty());
        assert!(parse_diskstats("garbage\n 8 0 sda x y z\n").is_empty());
    }

    #[test]
    fn disks_partitions_and_virtual_devices_are_told_apart() {
        let tree = fake_sys_block();
        let disks = parse_diskstats(DISKSTATS_1);
        let mut sampler = DiskIoSampler::with_paths("/unused", &tree.0);
        let start = Instant::now();
        let first = sampler.update(disks.clone(), start);
        assert_eq!(first.read_per_sec, 0.0, "no rates in the first sample");
        let get = |io: &DiskIo, name: &str| io.devices.iter().find(|d| d.name == name).unwrap().clone();
        let nvme = get(&first, "nvme0n1");
        assert!(nvme.whole_disk && !nvme.virtual_device && nvme.parent.is_none());
        assert_eq!(nvme.read_total, 254839442 * 512);
        let part = get(&first, "nvme0n1p2");
        assert!(!part.whole_disk && !part.virtual_device);
        assert_eq!(part.parent.as_deref(), Some("nvme0n1"));
        assert!(get(&first, "dm-0").virtual_device);
        assert!(get(&first, "loop1").virtual_device);

        // One second later: nvme0n1 read 2048 sectors (1 MiB), wrote 1024
        // (512 KiB) and was busy 250 ms; dm-0 the same through the crypt
        // layer.
        let mut later = disks;
        for disk in later.iter_mut().filter(|d| d.name == "nvme0n1" || d.name == "dm-0") {
            disk.sectors_read += 2048;
            disk.sectors_written += 1024;
            disk.io_ms += 250;
        }
        let second = sampler.update(later, start + Duration::from_secs(1));
        let nvme = get(&second, "nvme0n1");
        assert_eq!(nvme.read_per_sec, 1048576.0);
        assert_eq!(nvme.write_per_sec, 524288.0);
        assert_eq!(nvme.busy_percent, 25.0);
        // dm-0 isn't counted twice.
        assert_eq!(second.read_per_sec, 1048576.0);
        assert_eq!(second.write_per_sec, 524288.0);
    }

    #[test]
    fn disk_counters_going_backwards_give_zero() {
        let tree = fake_sys_block();
        let mut sampler = DiskIoSampler::with_paths("/unused", &tree.0);
        let start = Instant::now();
        sampler.update(parse_diskstats(DISKSTATS_1), start);
        let io =
            sampler.update(parse_diskstats(" 259 0 nvme0n1 1 0 1 0 1 0 1 0 0 1 0\n"), start + Duration::from_secs(1));
        assert_eq!(io.devices.len(), 1);
        assert_eq!(io.read_per_sec, 0.0);
        assert_eq!(io.devices[0].busy_percent, 0.0);
    }

    // From this machine, plus a WireGuard tunnel and a bridge.
    const NET_DEV_1: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 13099713   79204    0    0    0     0          0         0 13099713   79204    0    0    0     0       0          0
wlp0s20f3: 16408228637 11686318    0    0    0     0          0         0 235674878  587232    0   54    0     0       0          0
   wg0: 5000 10 0 0 0 0 0 0 6000 12 0 0 0 0 0 0
virbr0:    0       0    0    0    0     0          0         0        0       0    0    0    0     0       0          0
";

    fn fake_sys_class_net() -> TestTree {
        let tree = TestTree::new("sys-class-net");
        tree.file("lo/flags", "0x9\n")
            .file("wlp0s20f3/flags", "0x1003\n")
            .dir("wlp0s20f3/device")
            .file("wg0/flags", "0x91\n")
            .file("virbr0/flags", "0x1003\n");
        tree
    }

    #[test]
    fn net_dev_parses() {
        let interfaces = parse_net_dev(NET_DEV_1);
        let names: Vec<&str> = interfaces.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["lo", "wlp0s20f3", "wg0", "virbr0"]);
        assert_eq!(interfaces[1].rx_bytes, 16408228637);
        assert_eq!(interfaces[1].tx_bytes, 235674878);
        // Very old kernels left no space after the colon.
        let old = parse_net_dev("  eth0:123 1 0 0 0 0 0 0 456 2 0 0 0 0 0 0\n");
        assert_eq!(old[0].rx_bytes, 123);
        assert_eq!(old[0].tx_bytes, 456);
        assert!(parse_net_dev("eth0: 1 2 3\n: 1\nfoo\n").is_empty());
    }

    #[test]
    fn interfaces_are_classified_and_only_physical_ones_totalled() {
        let tree = fake_sys_class_net();
        let mut sampler = NetSampler::with_paths("/unused", &tree.0);
        let start = Instant::now();
        let first = sampler.update(parse_net_dev(NET_DEV_1), start);
        let flags: Vec<(bool, bool)> = first.interfaces.iter().map(|i| (i.loopback, i.virtual_device)).collect();
        assert_eq!(flags, [(true, true), (false, false), (false, true), (false, true)]);
        assert_eq!(first.rx_per_sec, 0.0);

        let mut later = parse_net_dev(NET_DEV_1);
        for interface in &mut later {
            interface.rx_bytes += 4000;
            interface.tx_bytes += 1000;
        }
        let second = sampler.update(later, start + Duration::from_secs(2));
        assert_eq!(second.interfaces[1].rx_per_sec, 2000.0);
        assert_eq!(second.interfaces[1].tx_per_sec, 500.0);
        assert_eq!(second.interfaces[0].rx_per_sec, 2000.0);
        assert_eq!(second.rx_per_sec, 2000.0, "only the wireless interface counts");
        assert_eq!(second.tx_per_sec, 500.0);
    }

    #[test]
    fn unknown_interfaces_fall_back_to_their_name() {
        let tree = TestTree::new("sys-class-net-empty");
        assert_eq!(classify_interface(&tree.0, "lo"), (true, true));
        assert_eq!(classify_interface(&tree.0, "eth0"), (false, true));
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_disk_and_network_throughput_is_sane() {
        let mut disks = DiskIoSampler::new();
        let mut net = NetSampler::new();
        let first = disks.sample();
        net.sample();
        std::thread::sleep(Duration::from_millis(300));
        let disk_io = disks.sample();
        let net_io = net.sample();
        for d in &disk_io.devices {
            println!(
                "{:<12} whole={} virtual={} parent={:?} read {:.0} B/s write {:.0} B/s busy {:.1}% (total {} / {})",
                d.name,
                d.whole_disk,
                d.virtual_device,
                d.parent,
                d.read_per_sec,
                d.write_per_sec,
                d.busy_percent,
                d.read_total,
                d.written_total
            );
        }
        for i in &net_io.interfaces {
            println!(
                "{:<12} loopback={} virtual={} rx {:.0} B/s tx {:.0} B/s (total {} / {})",
                i.name, i.loopback, i.virtual_device, i.rx_per_sec, i.tx_per_sec, i.rx_total, i.tx_total
            );
        }
        assert!(!first.devices.is_empty());
        assert!(disk_io.devices.iter().any(|d| d.whole_disk && !d.virtual_device));
        assert!(disk_io.devices.iter().all(|d| d.read_per_sec >= 0.0 && d.busy_percent <= 100.0));
        assert!(net_io.interfaces.iter().any(|i| i.loopback));
    }
}
