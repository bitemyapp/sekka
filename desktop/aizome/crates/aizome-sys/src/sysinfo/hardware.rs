//! The summary for "About this computer": processor, memory, graphics,
//! disks, operating system, kernel, make and model, and name.
//!
//! - **Processor**: the model from `/proc/cpuinfo` (`model name`; on ARM
//!   `Hardware`, else `/proc/device-tree/model`); physical cores from the
//!   distinct `topology/core_cpus_list` sets in `/sys/devices/system/cpu`
//!   (or distinct `physical id`/`core id` pairs in `/proc/cpuinfo`), and
//!   logical threads from the `processor` entries (online CPUs).
//! - **Disks**: `/sys/block`, whole disks only, without virtual devices
//!   (`loop`, `ram`, `zram`, `dm-`, `md`: anything without a `device`
//!   link), hidden multipath paths, or empty drives (size 0, such as a card
//!   reader without a card).
//! - **Kernel**: `/proc/sys/kernel/osrelease`; **architecture**: `uname`'s
//!   machine; **hostname**: `/proc/sys/kernel/hostname`; the **pretty
//!   hostname** from `/etc/machine-info`.
//!
//! [`hardware`] reads everything at once, including `pci.ids` for the GPU
//! names; it takes a few milliseconds, so call it when the page opens, not
//! on every frame.

use super::dmi::{HostModel, host_model};
use super::gpu::{Gpu, gpus_in, read_pci_ids};
use super::os_release::{OsRelease, os_release, parse_assignments};
use super::{online_cpus, read_attr, read_u64, uname, utsname_field};
use std::collections::HashSet;
use std::path::Path;

const CPUINFO: &str = "/proc/cpuinfo";
const CPU_ROOT: &str = "/sys/devices/system/cpu";
const DEVICE_TREE_MODEL: &str = "/proc/device-tree/model";
const SYS_BLOCK: &str = "/sys/block";
const DRM: &str = "/sys/class/drm";
const MACHINE_INFO: &str = "/etc/machine-info";

/// What "About this computer" shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hardware {
    /// The processor.
    pub cpu: CpuInfo,
    /// Usable memory in bytes (`MemTotal`: installed memory less what the
    /// firmware and kernel reserve, so a bit under the installed size).
    pub memory: Option<u64>,
    /// The graphics cards.
    pub gpus: Vec<Gpu>,
    /// The physical disks.
    pub disks: Vec<Disk>,
    /// The operating system.
    pub os: OsRelease,
    /// The kernel release, such as "7.2.9".
    pub kernel: Option<String>,
    /// The machine architecture, such as `x86_64` or `aarch64`.
    pub architecture: String,
    /// The make and model.
    pub model: HostModel,
    /// The host name.
    pub hostname: String,
    /// The pretty host name ("Chris's ThinkPad"), if one is set.
    pub pretty_hostname: Option<String>,
}

/// The processor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CpuInfo {
    /// The model as the kernel reports it, such as "Intel(R) Core(TM) Ultra
    /// 7 365" or "AMD Ryzen 7 7840U w/ Radeon 780M Graphics".
    pub model: Option<String>,
    /// The vendor id, such as `GenuineIntel` or `AuthenticAMD` (x86 only).
    pub vendor: Option<String>,
    /// Physical cores, when the topology is known.
    pub cores: Option<u32>,
    /// Logical CPUs (threads) online.
    pub threads: u32,
}

impl CpuInfo {
    /// [`model`](Self::model) tidied for display: without "(R)", "(TM)",
    /// a trailing "CPU" or "@ 2.80GHz", such as "Intel Core Ultra 7 365".
    pub fn display_name(&self) -> Option<String> {
        let model = self.model.as_deref()?;
        let model = model.split(" @ ").next().unwrap_or(model);
        let mut words: Vec<&str> = Vec::new();
        for word in model.split_ascii_whitespace() {
            let mut word = word;
            for mark in ["(R)", "(r)", "(TM)", "(tm)", "®", "™"] {
                word = word.strip_suffix(mark).unwrap_or(word);
            }
            if !word.is_empty() && !["(R)", "(TM)"].contains(&word) {
                words.push(word);
            }
        }
        if words.len() > 1 && words.last() == Some(&"CPU") {
            words.pop();
        }
        (!words.is_empty()).then(|| words.join(" "))
    }
}

/// A physical disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Disk {
    /// The kernel's name, such as `nvme0n1` or `sda`.
    pub name: String,
    /// The model, such as "SAMSUNG MZVL8512HFLU-00BLL".
    pub model: Option<String>,
    /// The vendor, where the device reports one (SCSI and USB disks;
    /// "ATA" is left out as it names the bus, not the maker).
    pub vendor: Option<String>,
    /// Size in bytes.
    pub size: u64,
    /// Whether it spins (a hard disk) rather than being solid state.
    pub rotational: bool,
    /// Whether it is removable media (USB sticks, SD cards).
    pub removable: bool,
}

/// Everything for "About this computer", with GPU names from `pci.ids` at
/// the standard places.
pub fn hardware() -> Hardware {
    hardware_with_pci_ids(None)
}

/// [`hardware`], with GPU names from `pci_ids` if given and readable.
pub fn hardware_with_pci_ids(pci_ids: Option<&Path>) -> Hardware {
    let ids = read_pci_ids(pci_ids);
    Hardware {
        cpu: cpu_info(),
        memory: super::memory().ok().map(|m| m.total),
        gpus: gpus_in(Path::new(DRM), ids.as_deref()),
        disks: disks(),
        os: os_release(),
        kernel: kernel_release(),
        architecture: architecture(),
        model: host_model(),
        hostname: hostname(),
        pretty_hostname: pretty_hostname(),
    }
}

/// The processor's model, cores and threads.
pub fn cpu_info() -> CpuInfo {
    let parsed = std::fs::read_to_string(CPUINFO).map(|text| parse_cpuinfo(&text)).unwrap_or_default();
    let topology = cpu_topology_in(Path::new(CPU_ROOT));
    CpuInfo {
        model: parsed.model.or_else(|| read_attr(DEVICE_TREE_MODEL)),
        vendor: parsed.vendor,
        cores: topology.map(|(cores, _)| cores).or(parsed.cores),
        threads: Some(parsed.processors)
            .filter(|&n| n > 0)
            .or(topology.map(|(_, threads)| threads))
            .unwrap_or_else(online_cpus),
    }
}

/// What `/proc/cpuinfo` says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedCpuinfo {
    pub model: Option<String>,
    pub vendor: Option<String>,
    /// `processor` entries.
    pub processors: u32,
    /// Distinct (`physical id`, `core id`) pairs, if every processor has
    /// both.
    pub cores: Option<u32>,
}

/// Parse `/proc/cpuinfo`: a block of `key<TAB>: value` lines per processor
/// (x86 and ARM alike), with ARM's board-wide `Hardware` line at the end.
pub(crate) fn parse_cpuinfo(text: &str) -> ParsedCpuinfo {
    let mut parsed = ParsedCpuinfo::default();
    let mut hardware = None;
    let mut pairs = HashSet::new();
    let mut ids_missing = false;
    let mut current: (Option<&str>, Option<&str>) = (None, None);
    let mut in_processor = false;
    let mut finish = |current: &mut (Option<&str>, Option<&str>), in_processor: &mut bool| {
        if *in_processor {
            match *current {
                (Some(package), Some(core)) => {
                    pairs.insert((package.to_owned(), core.to_owned()));
                }
                _ => ids_missing = true,
            }
        }
        *current = (None, None);
        *in_processor = false;
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            if line.trim().is_empty() {
                finish(&mut current, &mut in_processor);
            }
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "processor" if value.parse::<u32>().is_ok() => {
                finish(&mut current, &mut in_processor);
                in_processor = true;
                parsed.processors = parsed.processors.saturating_add(1);
            }
            "model name" if parsed.model.is_none() && !value.is_empty() => {
                parsed.model = Some(value.to_owned());
            }
            "vendor_id" if parsed.vendor.is_none() && !value.is_empty() => {
                parsed.vendor = Some(value.to_owned());
            }
            "Hardware" if !value.is_empty() => hardware = Some(value.to_owned()),
            "physical id" => current.0 = Some(value),
            "core id" => current.1 = Some(value),
            _ => {}
        }
    }
    finish(&mut current, &mut in_processor);
    parsed.model = parsed.model.or(hardware);
    parsed.cores = (!ids_missing && !pairs.is_empty()).then(|| u32::try_from(pairs.len()).ok()).flatten();
    parsed
}

/// Physical cores and logical CPUs from `cpuN/topology` under `root`
/// (normally `/sys/devices/system/cpu`): CPUs sharing a core list the same
/// `core_cpus_list` (`thread_siblings_list` before Linux 5.6). `None` if no
/// CPU has topology.
pub(crate) fn cpu_topology_in(root: &Path) -> Option<(u32, u32)> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut cores = HashSet::new();
    let mut threads = 0u32;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(index) = name.to_str().and_then(|n| n.strip_prefix("cpu")) else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let topology = entry.path().join("topology");
        let Some(siblings) =
            read_attr(topology.join("core_cpus_list")).or_else(|| read_attr(topology.join("thread_siblings_list")))
        else {
            continue;
        };
        threads = threads.saturating_add(1);
        cores.insert(siblings);
    }
    let cores = u32::try_from(cores.len()).ok()?;
    (threads > 0).then_some((cores, threads))
}

/// The physical disks, by name.
pub fn disks() -> Vec<Disk> {
    disks_in(Path::new(SYS_BLOCK))
}

/// The physical disks under `root` (normally `/sys/block`).
pub(crate) fn disks_in(root: &Path) -> Vec<Disk> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut disks = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if ["loop", "ram", "zram", "dm-", "md", "nbd"].iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let dir = entry.path();
        if !dir.join("device").exists() || read_attr(dir.join("hidden")).as_deref() == Some("1") {
            continue;
        }
        let size = read_u64(dir.join("size")).unwrap_or(0).saturating_mul(512);
        if size == 0 {
            continue;
        }
        disks.push(Disk {
            model: read_attr(dir.join("device/model")),
            vendor: read_attr(dir.join("device/vendor")).filter(|v| v != "ATA"),
            size,
            rotational: read_attr(dir.join("queue/rotational")).as_deref() == Some("1"),
            removable: read_attr(dir.join("removable")).as_deref() == Some("1"),
            name,
        });
    }
    disks.sort_by(|a, b| a.name.cmp(&b.name));
    disks
}

/// The kernel release, such as "7.2.9".
pub fn kernel_release() -> Option<String> {
    read_attr("/proc/sys/kernel/osrelease")
        .or_else(|| uname().map(|u| utsname_field(&u.release)).filter(|r| !r.is_empty()))
}

/// The machine architecture from `uname`, such as `x86_64` or `aarch64`.
pub fn architecture() -> String {
    uname()
        .map(|u| utsname_field(&u.machine))
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| std::env::consts::ARCH.to_owned())
}

/// The host name (`/proc/sys/kernel/hostname`, else `uname`'s), or
/// "localhost".
pub fn hostname() -> String {
    read_attr("/proc/sys/kernel/hostname")
        .or_else(|| uname().map(|u| utsname_field(&u.nodename)))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".to_owned())
}

/// The pretty host name (`PRETTY_HOSTNAME` in `/etc/machine-info`), if set.
pub fn pretty_hostname() -> Option<String> {
    let text = std::fs::read_to_string(MACHINE_INFO).ok()?;
    parse_pretty_hostname(&text)
}

fn parse_pretty_hostname(text: &str) -> Option<String> {
    parse_assignments(text)
        .into_iter()
        .rev()
        .find(|(key, _)| key == "PRETTY_HOSTNAME")
        .map(|(_, value)| value)
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    // This machine's first two processors, trimmed of flags.
    const CPUINFO_INTEL: &str = "\
processor\t: 0
vendor_id\t: GenuineIntel
cpu family\t: 6
model\t\t: 204
model name\t: Intel(R) Core(TM) Ultra 7 365
physical id\t: 0
siblings\t: 8
core id\t\t: 0
cpu cores\t: 8
flags\t\t: fpu vme de pse

processor\t: 1
vendor_id\t: GenuineIntel
model name\t: Intel(R) Core(TM) Ultra 7 365
physical id\t: 0
core id\t\t: 4

processor\t: 2
vendor_id\t: GenuineIntel
model name\t: Intel(R) Core(TM) Ultra 7 365
physical id\t: 0
core id\t\t: 4

";

    // A Raspberry Pi 4 (aarch64): no model name or core ids.
    const CPUINFO_PI: &str = "\
processor\t: 0
BogoMIPS\t: 108.00
Features\t: fp asimd evtstrm crc32 cpuid
CPU implementer\t: 0x41
CPU part\t: 0xd08

processor\t: 1
BogoMIPS\t: 108.00
CPU implementer\t: 0x41
CPU part\t: 0xd08

Hardware\t: BCM2835
Revision\t: c03114
Model\t\t: Raspberry Pi 4 Model B Rev 1.4
";

    #[test]
    fn x86_cpuinfo_parses() {
        let parsed = parse_cpuinfo(CPUINFO_INTEL);
        assert_eq!(parsed.model.as_deref(), Some("Intel(R) Core(TM) Ultra 7 365"));
        assert_eq!(parsed.vendor.as_deref(), Some("GenuineIntel"));
        assert_eq!(parsed.processors, 3);
        assert_eq!(parsed.cores, Some(2), "two threads share core 4");
    }

    #[test]
    fn arm_cpuinfo_falls_back_to_hardware() {
        let parsed = parse_cpuinfo(CPUINFO_PI);
        assert_eq!(parsed.model.as_deref(), Some("BCM2835"));
        assert_eq!(parsed.processors, 2);
        assert_eq!(parsed.cores, None);
        assert_eq!(parse_cpuinfo(""), ParsedCpuinfo::default());
        assert_eq!(parse_cpuinfo("garbage\n:\n: :\nprocessor : x\n").processors, 0);
    }

    #[test]
    fn cpu_model_names_are_tidied() {
        let name = |model: &str| CpuInfo { model: Some(model.into()), ..CpuInfo::default() }.display_name();
        assert_eq!(name("Intel(R) Core(TM) Ultra 7 365").as_deref(), Some("Intel Core Ultra 7 365"));
        assert_eq!(name("Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz").as_deref(), Some("Intel Core i7-8550U"));
        assert_eq!(
            name("11th Gen Intel(R) Core(TM) i7-1165G7 @ 2.80GHz").as_deref(),
            Some("11th Gen Intel Core i7-1165G7")
        );
        assert_eq!(
            name("AMD Ryzen 7 7840U w/ Radeon  780M Graphics").as_deref(),
            Some("AMD Ryzen 7 7840U w/ Radeon 780M Graphics")
        );
        assert_eq!(name("Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz").as_deref(), Some("Intel Xeon CPU E5-2680 v4"));
        assert_eq!(name("(R) (TM)"), None);
        assert_eq!(CpuInfo::default().display_name(), None);
    }

    #[test]
    fn topology_counts_cores_by_sibling_sets() {
        let tree = TestTree::new("topology");
        // Two cores with two threads each, and one offline CPU without
        // topology.
        tree.file("cpu0/topology/core_cpus_list", "0,2\n")
            .file("cpu1/topology/core_cpus_list", "1,3\n")
            .file("cpu2/topology/core_cpus_list", "0,2\n")
            .file("cpu3/topology/thread_siblings_list", "1,3\n")
            .dir("cpu4")
            .dir("cpufreq")
            .file("online", "0-3\n");
        assert_eq!(cpu_topology_in(&tree.0), Some((2, 4)));
        assert_eq!(cpu_topology_in(Path::new("/nonexistent/cpu")), None);
    }

    #[test]
    fn disks_skip_virtual_hidden_and_empty_devices() {
        let tree = TestTree::new("sys-block-disks");
        tree.file("nvme0n1/size", "1000215216\n")
            .file("nvme0n1/removable", "0\n")
            .file("nvme0n1/queue/rotational", "0\n")
            .file("nvme0n1/hidden", "0\n")
            .file("nvme0n1/device/model", "SAMSUNG MZVL8512HFLU-00BLL               \n")
            .file("sda/size", "3907029168\n")
            .file("sda/queue/rotational", "1\n")
            .file("sda/removable", "0\n")
            .file("sda/device/model", "ST2000DM008-2UB1\n")
            .file("sda/device/vendor", "ATA     \n")
            .file("sdb/size", "60437492\n")
            .file("sdb/removable", "1\n")
            .file("sdb/device/vendor", "SanDisk \n")
            .file("sdc/size", "0\n")
            .dir("sdc/device")
            .file("nvme0c0n1/size", "1000215216\n")
            .file("nvme0c0n1/hidden", "1\n")
            .dir("nvme0c0n1/device")
            .file("loop0/size", "1000\n")
            .file("zram0/size", "1000\n")
            .file("dm-0/size", "1000\n")
            .file("md0/size", "1000\n")
            .dir("md0/device")
            .file("vdx/size", "1000\n");
        let disks = disks_in(&tree.0);
        let names: Vec<&str> = disks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["nvme0n1", "sda", "sdb"]);
        assert_eq!(disks[0].size, 1000215216 * 512);
        assert_eq!(disks[0].model.as_deref(), Some("SAMSUNG MZVL8512HFLU-00BLL"));
        assert!(!disks[0].rotational && !disks[0].removable);
        assert!(disks[1].rotational);
        assert_eq!(disks[1].vendor, None);
        assert!(disks[2].removable);
        assert_eq!(disks[2].vendor.as_deref(), Some("SanDisk"));
        assert_eq!(disks[2].model, None);
    }

    #[test]
    fn machine_info_gives_the_pretty_hostname() {
        assert_eq!(
            parse_pretty_hostname("PRETTY_HOSTNAME=\"Chris's ThinkPad\"\nICON_NAME=computer-laptop\n").as_deref(),
            Some("Chris's ThinkPad")
        );
        assert_eq!(parse_pretty_hostname("PRETTY_HOSTNAME=\n"), None);
        assert_eq!(parse_pretty_hostname("ICON_NAME=computer\n"), None);
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_hardware_summary_is_sane() {
        let started = std::time::Instant::now();
        let hardware = hardware();
        let took = started.elapsed();
        println!("{hardware:#?}");
        println!("CPU: {:?}; model: {:?}; took {took:?}", hardware.cpu.display_name(), hardware.model.display_name());
        assert!(hardware.cpu.model.is_some());
        assert!(hardware.cpu.threads >= 1);
        assert!(hardware.cpu.cores.is_some_and(|c| c >= 1 && c <= hardware.cpu.threads));
        assert!(hardware.memory.is_some_and(|m| m > 256 << 20));
        assert!(!hardware.disks.is_empty());
        assert!(!hardware.gpus.is_empty());
        assert!(hardware.kernel.is_some());
        assert!(!hardware.architecture.is_empty());
        assert!(!hardware.hostname.is_empty());
        assert_ne!(hardware.os.pretty_name, "");
    }
}
