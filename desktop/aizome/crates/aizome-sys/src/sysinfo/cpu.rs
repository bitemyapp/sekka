//! CPU usage from `/proc/stat`, and each core's clock from cpufreq.
//!
//! `/proc/stat`'s `cpu` line (all CPUs) and `cpuN` lines (one per online
//! CPU) count the clock ticks spent in each state since boot: `user nice
//! system idle iowait irq softirq steal guest guest_nice`. Usage over an
//! interval is the share of the ticks counted in it that weren't `idle` or
//! `iowait`. `guest` and `guest_nice` are already part of `user` and `nice`,
//! so they are not added again.
//!
//! [`CpuSampler::sample`] compares with the previous call's counters. The
//! first call after [`CpuSampler::new`] compares with zero, so it reports
//! the **average since boot**; call it again a moment later for current
//! use. A core that went offline and came back starts over the same way.
//!
//! Each core's current clock is `scaling_cur_freq` (kHz) in
//! `/sys/devices/system/cpu/cpuN/cpufreq/`, absent in most virtual machines.

use super::read_attr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PROC_STAT: &str = "/proc/stat";
const CPU_ROOT: &str = "/sys/devices/system/cpu";

/// CPU use over the time since the previous sample.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuUsage {
    /// Busy share of all CPUs together, 0–100: everything but idle and
    /// iowait.
    pub total: f32,
    /// Share spent running programs (`user` + `nice`), 0–100.
    pub user: f32,
    /// Share spent in the kernel (`system` + `irq` + `softirq`), 0–100.
    pub system: f32,
    /// Share spent idle waiting for disk I/O, 0–100; not part of
    /// [`total`](Self::total).
    pub iowait: f32,
    /// Share a hypervisor gave to other virtual machines (`steal`), 0–100.
    pub steal: f32,
    /// Each online CPU (logical core), in index order.
    pub cores: Vec<CoreUsage>,
}

/// One logical CPU's use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreUsage {
    /// The CPU's number: `N` in `cpuN`.
    pub index: u32,
    /// Busy share of this CPU, 0–100.
    pub percent: f32,
    /// Its current clock in MHz, when cpufreq reports one.
    pub frequency_mhz: Option<u32>,
}

/// One `cpu` line's tick counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    fn total(&self) -> u64 {
        [self.user, self.nice, self.system, self.idle, self.iowait, self.irq, self.softirq, self.steal]
            .iter()
            .fold(0u64, |sum, &n| sum.saturating_add(n))
    }

    /// The counters accumulated since `earlier`; a counter that went
    /// backwards (a CPU brought back online) counts as zero.
    fn since(&self, earlier: &CpuTimes) -> CpuTimes {
        CpuTimes {
            user: self.user.saturating_sub(earlier.user),
            nice: self.nice.saturating_sub(earlier.nice),
            system: self.system.saturating_sub(earlier.system),
            idle: self.idle.saturating_sub(earlier.idle),
            iowait: self.iowait.saturating_sub(earlier.iowait),
            irq: self.irq.saturating_sub(earlier.irq),
            softirq: self.softirq.saturating_sub(earlier.softirq),
            steal: self.steal.saturating_sub(earlier.steal),
        }
    }
}

/// The parts of `/proc/stat` the CPU sampler and [`boot_time`] use.
///
/// [`boot_time`]: super::boot_time
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProcStat {
    /// The `cpu` line: all CPUs together.
    pub total: Option<CpuTimes>,
    /// The `cpuN` lines.
    pub cores: Vec<(u32, CpuTimes)>,
    /// `btime`: boot time in seconds since the Unix epoch.
    pub boot_time: Option<u64>,
}

/// Parse `/proc/stat`. Lines it doesn't know, and malformed `cpu` lines,
/// are skipped.
pub(crate) fn parse_proc_stat(text: &str) -> ProcStat {
    let mut stat = ProcStat::default();
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        let Some(key) = fields.next() else { continue };
        if key == "btime" {
            stat.boot_time = fields.next().and_then(|n| n.parse().ok());
        } else if let Some(which) = key.strip_prefix("cpu") {
            let Some(times) = parse_cpu_times(fields) else {
                continue;
            };
            if which.is_empty() {
                stat.total = Some(times);
            } else if let Ok(index) = which.parse::<u32>() {
                stat.cores.push((index, times));
            }
        }
    }
    stat
}

/// The numbers after `cpu`/`cpuN`. Kernels before 2.6 had only the first
/// four; missing ones are zero. Any non-number makes the line malformed.
fn parse_cpu_times<'a>(fields: impl Iterator<Item = &'a str>) -> Option<CpuTimes> {
    let mut values = [0u64; 8];
    let mut count = 0;
    for field in fields {
        let value = field.parse::<u64>().ok()?;
        if let Some(slot) = values.get_mut(count) {
            *slot = value;
        }
        count += 1;
    }
    if count < 4 {
        return None;
    }
    let [user, nice, system, idle, iowait, irq, softirq, steal] = values;
    Some(CpuTimes { user, nice, system, idle, iowait, irq, softirq, steal })
}

/// `part` as a percentage of `whole`, 0 when `whole` is 0.
fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 { 0.0 } else { ((part as f64 / whole as f64) * 100.0).clamp(0.0, 100.0) as f32 }
}

/// Busy share of the ticks in `delta`.
fn busy_percent(delta: &CpuTimes) -> f32 {
    let total = delta.total();
    let idle = delta.idle.saturating_add(delta.iowait);
    percent(total.saturating_sub(idle), total)
}

/// Measures CPU use between calls to [`sample`](Self::sample).
///
/// The first sample reports the average since boot (see the module
/// documentation).
#[derive(Debug)]
pub struct CpuSampler {
    previous_total: Option<CpuTimes>,
    previous_cores: HashMap<u32, CpuTimes>,
    stat_path: PathBuf,
    cpu_root: PathBuf,
}

impl Default for CpuSampler {
    fn default() -> CpuSampler {
        CpuSampler::new()
    }
}

impl CpuSampler {
    /// A sampler with no previous reading: its first sample reports the
    /// average since boot.
    pub fn new() -> CpuSampler {
        CpuSampler {
            previous_total: None,
            previous_cores: HashMap::new(),
            stat_path: PathBuf::from(PROC_STAT),
            cpu_root: PathBuf::from(CPU_ROOT),
        }
    }

    /// Read `/proc/stat` and cpufreq and report use since the previous
    /// call. If `/proc/stat` can't be read, everything is zero.
    pub fn sample(&mut self) -> CpuUsage {
        let text = match std::fs::read_to_string(&self.stat_path) {
            Ok(text) => text,
            Err(error) => {
                log::debug!("{}: {error}", self.stat_path.display());
                return CpuUsage::default();
            }
        };
        let root = self.cpu_root.clone();
        self.update(parse_proc_stat(&text), |index| core_frequency_mhz(&root, index))
    }

    /// Fold a new reading in and report use since the previous one.
    pub(crate) fn update(&mut self, stat: ProcStat, frequency: impl Fn(u32) -> Option<u32>) -> CpuUsage {
        let mut usage = CpuUsage::default();
        if let Some(now) = stat.total {
            let delta = now.since(&self.previous_total.unwrap_or_default());
            let total = delta.total();
            usage.total = busy_percent(&delta);
            usage.user = percent(delta.user.saturating_add(delta.nice), total);
            usage.system = percent(delta.system.saturating_add(delta.irq).saturating_add(delta.softirq), total);
            usage.iowait = percent(delta.iowait, total);
            usage.steal = percent(delta.steal, total);
            self.previous_total = Some(now);
        }

        let mut cores = HashMap::with_capacity(stat.cores.len());
        for (index, now) in stat.cores {
            let previous = self.previous_cores.get(&index).copied().unwrap_or_default();
            usage.cores.push(CoreUsage {
                index,
                percent: busy_percent(&now.since(&previous)),
                frequency_mhz: frequency(index),
            });
            cores.insert(index, now);
        }
        usage.cores.sort_by_key(|core| core.index);
        // Cores that went offline are forgotten, so they start over from
        // zero when they come back.
        self.previous_cores = cores;
        usage
    }
}

/// `cpuN`'s current clock in MHz from `scaling_cur_freq` (kHz) under
/// `cpu_root` (normally `/sys/devices/system/cpu`).
pub(crate) fn core_frequency_mhz(cpu_root: &Path, index: u32) -> Option<u32> {
    let path = cpu_root.join(format!("cpu{index}/cpufreq/scaling_cur_freq"));
    let khz: u64 = read_attr(path)?.parse().ok()?;
    u32::try_from(khz / 1000).ok().filter(|&mhz| mhz > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    // From this machine (an 8-thread Intel Core Ultra 7), trimmed.
    const STAT_1: &str = "\
cpu  694041 870675 416463 47257045 572495 89057 28022 0 129 108444
cpu0 87093 130885 61249 5868304 71870 8969 4241 0 27 17524
cpu1 86083 130483 51180 5885892 75651 8576 3060 0 24 17247
intr 123456789 0 9 0 0 0 0 0 0 1 0 0 0 0 0 0 0
ctxt 987654321
btime 1792056739
processes 544567
procs_running 1
procs_blocked 0
softirq 1 2 3 4 5 6 7 8 9 10 11
";

    #[test]
    fn proc_stat_parses_total_cores_and_boot_time() {
        let stat = parse_proc_stat(STAT_1);
        let total = stat.total.unwrap();
        assert_eq!(total.user, 694041);
        assert_eq!(total.nice, 870675);
        assert_eq!(total.steal, 0);
        assert_eq!(stat.cores.len(), 2);
        assert_eq!(stat.cores[1].0, 1);
        assert_eq!(stat.cores[1].1.idle, 5885892);
        assert_eq!(stat.boot_time, Some(1792056739));
    }

    #[test]
    fn malformed_and_short_lines_are_skipped() {
        let stat = parse_proc_stat("cpu 1 2 x 4\ncpu0 1 2 3\ncpu1 10 0 10 80\ncpuX 1 2 3 4\nbtime\n\n");
        assert_eq!(stat.total, None);
        assert_eq!(stat.cores.len(), 1);
        assert_eq!(stat.cores[0].0, 1);
        assert_eq!(stat.boot_time, None);
        assert_eq!(parse_proc_stat(""), ProcStat::default());
    }

    #[test]
    fn the_first_sample_is_since_boot_and_later_ones_are_deltas() {
        let mut sampler = CpuSampler::new();
        // 100 ticks: 20 user, 10 nice, 10 system, 50 idle, 10 iowait.
        let first = sampler
            .update(parse_proc_stat("cpu 20 10 10 50 10 0 0 0 0 0\ncpu0 20 10 10 50 10 0 0 0 0 0\n"), |_| Some(2400));
        assert_eq!(first.total, 40.0);
        assert_eq!(first.user, 30.0);
        assert_eq!(first.system, 10.0);
        assert_eq!(first.iowait, 10.0);
        assert_eq!(first.cores.len(), 1);
        assert_eq!(first.cores[0].frequency_mhz, Some(2400));

        // 200 more ticks: 150 user, 10 system, 10 irq, 30 idle.
        let second = sampler
            .update(parse_proc_stat("cpu 170 10 20 80 10 10 0 0 0 0\ncpu0 170 10 20 80 10 10 0 0 0 0\n"), |_| None);
        assert_eq!(second.total, 85.0);
        assert_eq!(second.user, 75.0);
        assert_eq!(second.system, 10.0);
        assert_eq!(second.iowait, 0.0);
        assert_eq!(second.cores[0].percent, 85.0);
        assert_eq!(second.cores[0].frequency_mhz, None);
    }

    #[test]
    fn counters_going_backwards_or_standing_still_give_zero() {
        let mut sampler = CpuSampler::new();
        let line = "cpu 100 0 100 800 0 0 0 0 0 0\ncpu3 100 0 100 800 0 0 0 0 0 0\n";
        sampler.update(parse_proc_stat(line), |_| None);
        let same = sampler.update(parse_proc_stat(line), |_| None);
        assert_eq!(same.total, 0.0);
        assert_eq!(same.cores[0].index, 3);
        assert_eq!(same.cores[0].percent, 0.0);
        let back = sampler.update(parse_proc_stat("cpu 1 0 1 8 0 0 0 0 0 0\ncpu3 1 0 1 8 0 0 0 0 0 0\n"), |_| None);
        assert_eq!(back.total, 0.0);
        assert_eq!(back.cores[0].percent, 0.0);
    }

    #[test]
    fn cores_come_out_in_index_order() {
        let mut sampler = CpuSampler::new();
        let usage =
            sampler.update(parse_proc_stat("cpu 1 1 1 1\ncpu2 1 1 1 1\ncpu0 1 1 1 1\ncpu1 1 1 1 1\n"), |_| None);
        let indexes: Vec<u32> = usage.cores.iter().map(|c| c.index).collect();
        assert_eq!(indexes, [0, 1, 2]);
    }

    #[test]
    fn frequencies_are_read_from_cpufreq_in_mhz() {
        let tree = TestTree::new("cpufreq");
        tree.file("cpu0/cpufreq/scaling_cur_freq", "3014504\n")
            .file("cpu1/cpufreq/scaling_cur_freq", "garbage\n")
            .dir("cpu2");
        assert_eq!(core_frequency_mhz(&tree.0, 0), Some(3014));
        assert_eq!(core_frequency_mhz(&tree.0, 1), None);
        assert_eq!(core_frequency_mhz(&tree.0, 2), None);
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_cpu_usage_is_sane() {
        let mut sampler = CpuSampler::new();
        let since_boot = sampler.sample();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let usage = sampler.sample();
        println!("since boot: {:.1}%", since_boot.total);
        println!(
            "now: {:.1}% (user {:.1}, system {:.1}, iowait {:.1})",
            usage.total, usage.user, usage.system, usage.iowait
        );
        for core in &usage.cores {
            println!("  cpu{}: {:.1}% at {:?} MHz", core.index, core.percent, core.frequency_mhz);
        }
        assert!(since_boot.total > 0.0);
        assert!((0.0..=100.0).contains(&usage.total));
        assert_eq!(usage.cores.len(), crate::sysinfo::online_cpus() as usize);
        assert!(usage.cores.iter().all(|c| (0.0..=100.0).contains(&c.percent)));
    }
}
