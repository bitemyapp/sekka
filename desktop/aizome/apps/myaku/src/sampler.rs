//! The sampler: a thread of its own that reads the system every second
//! (or as often as asked) and hands the window a [`Frame`]. Rates come from
//! two readings. The costlier figures are read less often:
//!
//! - every reading: each process's `stat` (aizome-sys's
//!   [`ProcessSampler`]), `status` (wake-ups, swap), and for your own
//!   processes `io` (storage);
//! - a process's memory maps (`smaps_rollup`: its proportional and private
//!   sizes) every fifth reading, every other one while the Memory view
//!   shows, and every reading for the processes on show in the inspector;
//! - the DRM devices a process has open every tenth reading, their usage
//!   statistics every reading;
//! - the launcher entries every half minute (if they changed).
//!
//! Ported from Tatami's Activity (`sampler.rs`), on aizome-sys's readers.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aizome_sys::apps::AppIndex;
use aizome_sys::sysinfo::detail::{ProcReader, ProcessMaps, pressure};
use aizome_sys::sysinfo::drm::{ClientUse, DrmClient, gpu_stats, parse_fdinfo};
use aizome_sys::sysinfo::power::{
    Floor, RaplReading, battery_draw, boottime_us, joules, meter_counters, meters_package, rapl_counters, rapl_domains,
    split,
};
use aizome_sys::sysinfo::{self, CpuSampler, DiskIoSampler, NetSampler, ProcessSampler};

use crate::group::{self, Node};
use crate::model::{self, Frame, GpuUsage, GpuUse, Group, Lens, Meter, Process, Usage};
use crate::nvml::Nvml;

type Key = (u32, u64);

/// Only this card's measured clients can supply a fallback utilization.
fn client_busy(processes: &[Process], device: usize) -> Option<f64> {
    let mut values = processes.iter().flat_map(|p| &p.gpus).filter(|g| g.device == device).map(|g| g.usage.busy);
    let first = values.next()??;
    Some((first + values.sum::<Option<f64>>()?).min(100.0))
}

fn merge_nvidia(gpus: &mut [model::Gpu], processes: &mut [Process], readings: &[crate::nvml::Reading]) {
    let pids: HashMap<u32, usize> = processes.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    for reading in readings {
        let Some(device) = gpus.iter().position(|g| g.pci == reading.pci) else { continue };
        let gpu = &mut gpus[device];
        if !reading.name.is_empty() {
            gpu.name = reading.name.clone();
        }
        gpu.busy = reading.busy;
        gpu.video = reading.video;
        gpu.vram_used = reading.vram_used;
        gpu.vram_total = reading.vram_total;
        gpu.power = reading.power;
        gpu.temperature = reading.temperature;
        gpu.clock = reading.clock;
        gpu.unlisted_usage = reading.unlisted_usage;
        // NVML is authoritative for this card, including if a future NVIDIA DRM
        // driver starts exporting overlapping counters.
        for process in processes.iter_mut() {
            process.gpus.retain(|g| g.device != device);
        }
        for (pid, usage) in &reading.processes {
            if let Some(&index) = pids.get(pid) {
                processes[index].gpus.push(GpuUse { device, usage: *usage });
            }
        }
    }
}

/// The meters, as `Power` gives them: the processor package's counters
/// (RAPL) where readable, else the package's meter as an AMD APU's
/// integrated graphics report it (the processor included), else the
/// battery while it discharges, else nothing for the processor; and beside
/// any of them, the discrete cards' own meters. `rapl` is RAPL's power
/// when its counters are read: `Some(None)` until two readings give one.
pub(crate) fn meters(
    rapl: Option<Option<f64>>,
    battery: Option<f64>,
    discharging: bool,
    gpus: &[model::Gpu],
) -> model::Power {
    let package = gpus.iter().filter(|g| g.package_meter).find_map(|g| g.power);
    let graphics = gpus.iter().filter(|g| !g.integrated).filter_map(|g| g.power).reduce(|a, b| a + b);
    let (meter, cpu) = match (rapl, package) {
        (Some(w), _) => (Meter::Rapl, w),
        (None, Some(w)) => (Meter::Package, Some(w)),
        (None, None) if discharging && battery.is_some() => (Meter::Battery, None),
        (None, None) if graphics.is_some() => (Meter::Graphics, None),
        (None, None) => (Meter::None, None),
    };
    let total = match meter {
        Meter::Battery => battery,
        Meter::None => None,
        Meter::Graphics => graphics,
        Meter::Rapl | Meter::Package => cpu.map(|w| w + graphics.unwrap_or(0.0)),
    };
    model::Power { meter, cpu, graphics, battery, on_battery: discharging, total, attributed: 0.0 }
}

/// Each process's part of what the meters measure (`None`: none of them
/// measures it): of the processor's meter, or of the battery less the cards,
/// by processor time; of each discrete card's, by its time on that card.
/// What is left is the devices idling.
pub(crate) fn attribute(
    power: &model::Power,
    gpus: &[model::Gpu],
    processes: &[Process],
    ncpu: f64,
    floor: &mut Floor,
    gpu_floors: &mut [Floor],
) -> Vec<Option<f64>> {
    let mut parts: Vec<Option<f64>> = vec![None; processes.len()];
    let processor = match power.meter {
        Meter::Battery => power.total.map(|b| (b - power.graphics.unwrap_or(0.0)).max(0.0)),
        _ => power.cpu,
    };
    if let Some(watts) = processor {
        let shares: Vec<f64> = processes.iter().map(|p| p.usage.cpu / (100.0 * ncpu)).collect();
        for (part, w) in parts.iter_mut().zip(split(watts, floor.update(watts), &shares)) {
            *part = Some(w);
        }
    }
    for (device, (gpu, floor)) in gpus.iter().zip(gpu_floors).enumerate() {
        let Some(watts) = gpu.power.filter(|_| !gpu.integrated) else { continue };
        let shares: Vec<f64> = processes
            .iter()
            .map(|p| p.gpus.iter().filter(|g| g.device == device).filter_map(|g| g.usage.busy).sum::<f64>() / 100.0)
            .collect();
        for ((part, w), p) in parts.iter_mut().zip(split(watts, floor.update(watts), &shares)).zip(processes) {
            if p.uses(device) {
                *part = Some(part.unwrap_or(0.0) + w);
            }
        }
    }
    parts
}

/// What the window tells the sampler.
pub enum Command {
    Interval(Duration),
    /// What shows: the lens, and the processes whose costlier figures to
    /// read every time (the inspector's).
    Focus {
        lens: Lens,
        pids: Vec<u32>,
    },
}

/// Start the sampler: `deliver` gets each frame until it returns false.
pub fn spawn(interval: Duration, deliver: impl Fn(Arc<Frame>) -> bool + Send + 'static) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    let _ = std::thread::Builder::new().name("myaku-sampler".into()).spawn(move || {
        let mut sampler = Sampler::new();
        let mut interval = interval;
        // A first reading to measure rates against, and the first frame
        // soon after, so the window has something to show.
        let _ = sampler.sample();
        let mut next = Instant::now() + Duration::from_millis(300);
        loop {
            match receiver.recv_timeout(next.saturating_duration_since(Instant::now())) {
                Ok(Command::Interval(d)) => {
                    interval = d;
                    next = next.min(Instant::now() + interval);
                    continue;
                }
                Ok(Command::Focus { lens, pids }) => {
                    sampler.lens = lens;
                    sampler.focus = pids.into_iter().collect();
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            let frame = sampler.sample();
            if !deliver(Arc::new(frame)) {
                break;
            }
            next += interval;
            let now = Instant::now();
            if next < now {
                next = now + interval;
            }
        }
    });
    commands
}

/// What the sampler remembers of a process between readings.
#[derive(Default)]
struct Known {
    status_at: Option<Instant>,
    voluntary: u64,
    wakeups: f64,
    swap: u64,
    virt: u64,
    io: Option<(u64, u64)>,
    io_at: Option<Instant>,
    rates: (f64, f64),
    maps: Option<ProcessMaps>,
    maps_rss: u64,
    maps_tick: u64,
}

struct Card {
    gpu: sysinfo::Gpu,
    pci: String,
    integrated: bool,
    package_meter: bool,
}

/// The package's power from RAPL's counters, read directly or through
/// Aizome's power meter. The meter answers with its last reading until it
/// may take the next (once a second), so the same counters again leave the
/// last rate standing. Each reading says when it was taken, so a rate is
/// over the time between two readings, not between Myaku's questions.
#[derive(Default)]
struct Rapl {
    /// The last new reading.
    before: Option<RaplReading>,
    watts: Option<f64>,
}

impl Rapl {
    /// The power by `reading`, for [`meters`]: `None` when the counters
    /// can't be read, `Some(None)` until a second reading gives it.
    fn read(&mut self, reading: Option<RaplReading>) -> Option<Option<f64>> {
        let Some(reading) = reading else {
            *self = Rapl::default();
            return None;
        };
        match self.before.take() {
            Some(before) if before.counters == reading.counters => self.before = Some(before),
            Some(before) if before.counters.len() == reading.counters.len() && reading.at > before.at => {
                let joules: f64 = before.counters.iter().zip(&reading.counters).map(|(b, a)| joules(b, a)).sum();
                self.watts = Some(joules / ((reading.at - before.at) as f64 / 1e6));
                self.before = Some(reading);
            }
            _ => {
                self.watts = None;
                self.before = Some(reading);
            }
        }
        Some(self.watts)
    }
}

/// A reading's time on both clocks: `Instant` (`CLOCK_MONOTONIC`), which
/// stops while the computer sleeps, and `CLOCK_BOOTTIME`, which doesn't.
#[derive(Clone, Copy, Debug)]
struct Moment {
    monotonic: Instant,
    /// Microseconds of `CLOCK_BOOTTIME`.
    boot: u64,
}

/// More time asleep between readings than this, and the interval is
/// measured no more: readings a second apart differ on the two clocks by
/// microseconds, and a suspend takes seconds.
const ASLEEP: Duration = Duration::from_millis(250);

impl Moment {
    fn now() -> Moment {
        Moment { monotonic: Instant::now(), boot: boottime_us() }
    }
}

/// The seconds from the last reading to `now`, to measure rates and energy
/// over, and whether the computer slept between them: then `0.0`. RAPL's
/// counters, and some cards' energy counters, counted on while it slept;
/// over the second awake, that would be a night's energy in a second.
fn interval(last: Option<Moment>, now: Moment) -> (f64, bool) {
    let Some(last) = last else { return (0.0, false) };
    let awake = now.monotonic.saturating_duration_since(last.monotonic);
    let passed = Duration::from_micros(now.boot.saturating_sub(last.boot));
    if passed.saturating_sub(awake) > ASLEEP { (0.0, true) } else { (awake.as_secs_f64(), false) }
}

pub struct Sampler {
    procs: ProcessSampler,
    reader: ProcReader,
    cpu: CpuSampler,
    disks: DiskIoSampler,
    net: NetSampler,
    tick: u64,
    last: Option<Moment>,
    before: HashMap<Key, Known>,
    drm_fds: HashMap<Key, Vec<u32>>,
    clients: HashMap<(String, u64), (DrmClient, Instant)>,
    cards: Vec<Card>,
    nvml: Option<Nvml>,
    rapl_domains: Vec<PathBuf>,
    rapl: Rapl,
    /// Each card's energy counter at the last reading, in joules.
    gpu_energy: Vec<Option<f64>>,
    floor: Floor,
    gpu_floors: Vec<Floor>,
    energy: HashMap<Key, (f64, f64)>,
    apps: Option<AppIndex>,
    uid: u32,
    cpu_name: String,
    pub lens: Lens,
    pub focus: HashSet<u32>,
    /// What the last reading spent on each part.
    pub phases: Vec<(&'static str, Duration)>,
    /// The processor's temperature sensor's file, found once (reading every
    /// sensor wakes slow ones, such as an NVMe drive's).
    sensor: Option<Option<PathBuf>>,
    /// Names and kinds by process identity and every membership/label input.
    identities: HashMap<(Key, u64), group::Identity>,
}

impl Default for Sampler {
    fn default() -> Sampler {
        Sampler::new()
    }
}

impl Sampler {
    pub fn new() -> Sampler {
        let cards: Vec<Card> = sysinfo::gpus()
            .into_iter()
            .map(|g| {
                let integrated = aizome_sys::sysinfo::drm::card_integrated(&g.card);
                Card {
                    pci: aizome_sys::sysinfo::drm::card_pci(&g.card).unwrap_or_default(),
                    integrated,
                    package_meter: meters_package(&g, integrated),
                    gpu: g,
                }
            })
            .collect();
        let nvml = cards.iter().any(|c| c.gpu.driver.as_deref() == Some("nvidia")).then(Nvml::open).flatten();
        let count = cards.len();
        Sampler {
            procs: ProcessSampler::new(),
            reader: ProcReader::new(),
            cpu: CpuSampler::new(),
            disks: DiskIoSampler::new(),
            net: NetSampler::new(),
            tick: 0,
            last: None,
            before: HashMap::new(),
            drm_fds: HashMap::new(),
            clients: HashMap::new(),
            cards,
            nvml,
            rapl_domains: rapl_domains(),
            rapl: Rapl::default(),
            gpu_energy: vec![None; count],
            floor: Floor::default(),
            gpu_floors: vec![Floor::default(); count],
            energy: HashMap::new(),
            apps: Some(AppIndex::load()),
            // SAFETY: getuid has no preconditions.
            uid: unsafe { libc::getuid() },
            cpu_name: tidy_cpu_name(&sysinfo::cpu_info().model.unwrap_or_default()),
            lens: Lens::Cpu,
            focus: HashSet::new(),
            phases: Vec::new(),
            sensor: None,
            identities: HashMap::new(),
        }
    }

    /// One reading.
    pub fn sample(&mut self) -> Frame {
        self.sample_at(Moment::now(), |domains| rapl_counters(domains).or_else(meter_counters))
    }

    /// One reading taken `now`, with RAPL's counters from `rapl` (given the
    /// package domains): what [`Sampler::sample`] does with the clocks and
    /// the counters, which a test can script instead.
    fn sample_at(&mut self, now: Moment, rapl: impl FnOnce(&[PathBuf]) -> Option<RaplReading>) -> Frame {
        let started = now.monotonic;
        self.tick += 1;
        let tick = self.tick;
        // After the computer slept, the energy counters' interval is
        // measured no more: no one's energy grows by it.
        let (seconds, slept) = interval(self.last, now);
        self.last = Some(now);
        if tick.is_multiple_of(30)
            && let Some(a) = &mut self.apps
        {
            a.refresh_if_changed();
        }
        // Memory maps: re-read when they may have changed, at most so many a
        // reading (walking a big process's page tables takes milliseconds).
        let maps_every: u64 = if self.lens == Lens::Memory { 2 } else { 5 };
        let mut maps_budget: usize = if self.tick <= 2 { usize::MAX } else { 48 };
        let scan_drm = !self.cards.is_empty() && tick % 10 == 1;
        let mut phase = Instant::now();
        let mut phases: Vec<(&'static str, Duration)> = Vec::new();
        let mut lap = |name: &'static str, phases: &mut Vec<(&'static str, Duration)>| {
            let now = Instant::now();
            phases.push((name, now - phase));
            phase = now;
        };
        let list = self.procs.sample();
        lap("stat", &mut phases);
        let mut processes: Vec<Process> = Vec::with_capacity(list.len());
        let mut seen: HashSet<Key> = HashSet::with_capacity(list.len());
        let mut fresh_clients: HashMap<(String, u64), (DrmClient, Instant)> = HashMap::new();
        for p in &list {
            let key = (p.pid, p.start_ticks);
            seen.insert(key);
            let own = p.uid == self.uid || self.uid == 0;
            let (name, role) = group::name_and_role(
                &p.name,
                &p.args,
                &p.exe.as_ref().map(|e| e.display().to_string()).unwrap_or_default(),
            );
            let mut usage = Usage {
                cpu: f64::from(p.cpu_percent),
                cpu_time: p.cpu_time.as_secs_f64(),
                threads: u64::from(p.threads),
                resident: p.memory,
                ..Default::default()
            };
            // The costlier files, read when the process ran since the last
            // reading (one that didn't run can't have woken, read or
            // written), else every fifth reading; rates over the time
            // since each was read.
            let ran = p.cpu_percent > 0.0 || self.focus.contains(&p.pid);
            let mut known = self.before.remove(&key).unwrap_or_default();
            let fresh = known.status_at.is_none();
            let stale =
                |at: Option<Instant>| at.is_none_or(|a| started.duration_since(a) >= Duration::from_millis(4500));
            if !p.kernel_thread
                && (ran || stale(known.status_at))
                && let Some(s) = self.reader.status(p.pid)
            {
                if let Some(at) = known.status_at {
                    let dt = started.duration_since(at).as_secs_f64();
                    if dt > 0.0 {
                        known.wakeups = s.voluntary_switches.saturating_sub(known.voluntary) as f64 / dt;
                    }
                }
                known.voluntary = s.voluntary_switches;
                known.swap = s.swap;
                known.virt = s.virt;
                known.status_at = Some(started);
            } else if !ran {
                known.wakeups = 0.0;
            }
            usage.wakeups = known.wakeups;
            usage.swap = known.swap;
            usage.virt = known.virt;
            if own && !p.kernel_thread {
                if (ran || stale(known.io_at))
                    && let Some(io) = self.reader.io(p.pid)
                {
                    if let (Some((r0, w0)), Some(at)) = (known.io, known.io_at) {
                        let dt = started.duration_since(at).as_secs_f64();
                        if dt > 0.0 {
                            known.rates = (
                                io.read_bytes.saturating_sub(r0) as f64 / dt,
                                io.write_bytes.saturating_sub(w0) as f64 / dt,
                            );
                        }
                    }
                    known.io = Some((io.read_bytes, io.write_bytes));
                    known.io_at = Some(started);
                } else if !ran {
                    known.rates = (0.0, 0.0);
                }
                if let Some((r, w)) = known.io {
                    usage.read_total = Some(r);
                    usage.write_total = Some(w);
                    usage.read_rate = Some(known.rates.0);
                    usage.write_rate = Some(known.rates.1);
                }
                // Memory maps: when its resident size moved, now and then
                // anyway, always for what the inspector shows.
                let moved = known.maps_rss.abs_diff(p.memory) > (p.memory / 50).max(1 << 20);
                let old = tick.saturating_sub(known.maps_tick) >= maps_every * 6;
                if (fresh || moved || old || self.focus.contains(&p.pid)) && maps_budget > 0 {
                    maps_budget -= 1;
                    if let Some(m) = self.reader.maps(p.pid) {
                        known.maps = Some(m);
                        known.maps_rss = p.memory;
                        known.maps_tick = tick;
                    }
                }
                if let Some(m) = known.maps {
                    usage.pss = Some(m.pss);
                    usage.private = Some(m.private);
                    usage.shared = Some(m.shared);
                }
            }
            self.before.insert(key, known);
            // Graphics: the DRM devices it has open, scanned now and then.
            let mut gpus: Vec<GpuUse> = Vec::new();
            if own && !self.cards.is_empty() && !p.kernel_thread {
                if scan_drm || self.focus.contains(&p.pid) {
                    let fds = self.reader.drm_fds(p.pid);
                    if fds.is_empty() {
                        self.drm_fds.remove(&key);
                    } else {
                        self.drm_fds.insert(key, fds);
                    }
                }
                if let Some(fds) = self.drm_fds.get(&key).cloned() {
                    let mut counted: HashSet<(String, u64)> = HashSet::new();
                    for fd in fds {
                        let Some(client) = self.reader.fdinfo(p.pid, fd).and_then(parse_fdinfo) else { continue };
                        let id = (client.pdev.clone(), client.id);
                        if !counted.insert(id.clone()) {
                            continue;
                        }
                        let Some(device) = self.cards.iter().position(|c| c.pci == client.pdev) else { continue };
                        let mut u = GpuUse {
                            device,
                            usage: GpuUsage {
                                busy: None,
                                video: None,
                                vram: Some(client.vram),
                                shared: Some(client.shared),
                            },
                        };
                        if let Some((was, at)) = self.clients.get(&id)
                            && client.measures_busy()
                        {
                            let cu = ClientUse::between(was, &client, started.duration_since(*at).as_secs_f64());
                            u.usage.busy = Some(f64::from(cu.busy));
                            u.usage.video = Some(f64::from(cu.video));
                        }
                        fresh_clients.insert(id, (client, started));
                        gpus.push(u);
                    }
                }
            }
            processes.push(Process {
                pid: p.pid,
                ppid: p.ppid,
                start_ticks: p.start_ticks,
                started: p.start_time.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
                name,
                role,
                command: p.command_line(),
                exe: p.exe.as_ref().map(|e| e.display().to_string()).unwrap_or_default(),
                user: p.user.clone(),
                uid: p.uid,
                state: p.state.as_char(),
                nice: p.nice,
                unit: p.unit.as_ref().map(|u| u.name.clone()).unwrap_or_default(),
                cgroup: p.cgroup.clone().unwrap_or_default(),
                kernel: p.kernel_thread,
                readable: own,
                usage,
                gpus,
                group: 0,
            });
        }
        self.clients = fresh_clients;
        lap("status, io, maps, drm", &mut phases);
        self.before.retain(|k, _| seen.contains(k));
        self.drm_fds.retain(|k, _| seen.contains(k));

        // The system.
        let cpu = self.cpu.sample();
        let mem = sysinfo::memory().unwrap_or_default();
        let load = sysinfo::load_average().ok();
        let disk = self.disks.sample();
        let net = self.net.sample();
        let ncpu = cpu.cores.len().max(1) as f64;
        let mut system = model::System {
            cpu: f64::from(cpu.total),
            user: f64::from(cpu.user),
            kernel: f64::from(cpu.system),
            iowait: f64::from(cpu.iowait),
            cores: cpu.cores.iter().map(|c| f64::from(c.percent)).collect(),
            frequencies: cpu.cores.iter().filter_map(|c| c.frequency_mhz.map(f64::from)).collect(),
            load: load.map_or([0.0; 3], |l| [f64::from(l.one), f64::from(l.five), f64::from(l.fifteen)]),
            processes: processes.len(),
            threads: processes.iter().map(|p| p.usage.threads).sum(),
            uptime: sysinfo::uptime().map_or(0.0, |d| d.as_secs_f64()),
            temperature: self.temperature(),
            cpu_name: self.cpu_name.clone(),
            memory: model::Memory {
                total: mem.total,
                available: mem.available,
                free: mem.free,
                cached: mem.cached,
                buffers: mem.buffers,
                shared: mem.shared,
                swap_total: mem.swap_total,
                swap_used: mem.swap_used,
            },
            pressure: pressure(),
            disks: disk
                .devices
                .iter()
                .filter(|d| d.whole_disk && !d.virtual_device)
                .map(|d| model::Disk {
                    name: d.name.clone(),
                    read_rate: d.read_per_sec,
                    write_rate: d.write_per_sec,
                    busy: f64::from(d.busy_percent),
                })
                .collect(),
            disk_read: disk.read_per_sec,
            disk_write: disk.write_per_sec,
            net_rx: net.rx_per_sec,
            net_tx: net.tx_per_sec,
        };
        if system.frequencies.is_empty() {
            system.frequencies = Vec::new();
        }

        lap("system", &mut phases);
        // Graphics cards.
        let stats: Vec<_> = self.cards.iter().map(|c| gpu_stats(&c.gpu.card)).collect();
        let mut counters: Vec<Option<f64>> = stats.iter().map(|s| s.energy).collect();
        let mut gpus: Vec<model::Gpu> = self
            .cards
            .iter()
            .zip(&stats)
            .enumerate()
            .map(|(i, (c, s))| {
                let busy = s.busy.map(f64::from).or_else(|| client_busy(&processes, i));
                model::Gpu {
                    name: c.gpu.name.clone(),
                    driver: c.gpu.driver.clone().unwrap_or_default(),
                    pci: c.pci.clone(),
                    integrated: c.integrated,
                    package_meter: c.package_meter,
                    busy,
                    video: s.video.map(f64::from),
                    vram_used: s.vram_used,
                    vram_total: s.vram_total.or(c.gpu.vram),
                    power: s.power,
                    temperature: s.temperature.map(f64::from),
                    clock: s.clock.map(f64::from),
                    unlisted_usage: if c.gpu.driver.as_deref() == Some("nvidia") {
                        GpuUsage::UNKNOWN
                    } else {
                        GpuUsage::default()
                    },
                }
            })
            .collect();
        if let Some(nvml) = &mut self.nvml {
            let readings = nvml.read();
            for r in &readings {
                if let Some(d) = gpus.iter().position(|g| g.pci == r.pci) {
                    counters[d] = r.energy;
                }
            }
            merge_nvidia(&mut gpus, &mut processes, &readings);
        }
        // A card's energy counter gives its power over the interval; its
        // meter, the power now.
        for ((gpu, counter), before) in gpus.iter_mut().zip(counters).zip(&mut self.gpu_energy) {
            if let (Some(b), Some(a)) = (*before, counter)
                && seconds > 0.0
                && a >= b
            {
                gpu.power = Some((a - b) / seconds);
            }
            *before = counter;
        }

        // Power: the meters, shared by use of what each measures. RAPL
        // directly where Myaku may read it (as root), else through Aizome's
        // power meter. After the computer slept, RAPL's rate starts over:
        // its counters went on through the night.
        if slept {
            self.rapl = Rapl::default();
        }
        let rapl = self.rapl.read(rapl(&self.rapl_domains));
        let (battery, discharging) = battery_draw();
        let mut power = meters(rapl, battery, discharging, &gpus);
        let parts = attribute(&power, &gpus, &processes, ncpu, &mut self.floor, &mut self.gpu_floors);
        power.attributed = parts.iter().flatten().sum();
        for (p, part) in processes.iter_mut().zip(parts) {
            if let Some(w) = part {
                let e = self.energy.entry(p.key()).or_insert((0.0, 0.0));
                e.0 += w * seconds;
                e.1 += seconds;
                p.usage.power = Some(w);
            }
            // What it used while measured, measured now or not.
            if let Some(&(energy, measured)) = self.energy.get(&p.key()) {
                p.usage.energy = energy;
                p.usage.measured = measured;
            }
        }
        self.energy.retain(|k, _| seen.contains(k));

        lap("gpus, power", &mut phases);
        // Applications.
        let nodes: Vec<Node> = processes
            .iter()
            .zip(&list)
            .map(|(p, raw)| Node {
                pid: p.pid,
                ppid: p.ppid,
                started: p.start_ticks,
                uid: p.uid,
                kernel: p.kernel,
                name: p.name.clone(),
                comm: raw.name.clone(),
                role: p.role.clone(),
                exe: p.exe.clone(),
                cgroup: p.cgroup.clone(),
                app_id: raw.unit.as_ref().and_then(|u| u.app_id()),
            })
            .collect();
        let windows = group::windows();
        let mut groups: Vec<Group> = Vec::new();
        if tick.is_multiple_of(30) {
            self.identities.clear();
        }
        let mut used: HashSet<(Key, u64)> = HashSet::new();
        for members in group::group(&nodes, &windows, self.apps.as_ref()) {
            let first = processes[members[0]].key();
            let cache = (first, group::fingerprint(&nodes, &members, &windows));
            used.insert(cache);
            let id = match self.identities.get(&cache) {
                Some(id) => id.clone(),
                None => {
                    let id = group::identify(&nodes, &members, &windows, self.apps.as_ref(), self.uid);
                    self.identities.insert(cache, id.clone());
                    id
                }
            };
            let mut usage = Usage::default();
            for &i in &members {
                usage.add(&processes[i].usage);
            }
            let gi = groups.len();
            for &i in &members {
                processes[i].group = gi;
            }
            groups.push(Group {
                key: processes[members[0]].key(),
                kind: Some(id.kind),
                name: id.name,
                app_id: id.app_id,
                detail: id.detail,
                attribution: id.attribution,
                windows: id.windows,
                focused: id.focused,
                own: members.iter().all(|&i| processes[i].uid == self.uid || self.uid == 0),
                members,
                usage,
            });
        }
        self.identities.retain(|k, _| used.contains(k));
        lap("groups", &mut phases);
        self.phases = phases;
        let mut frame = Frame { tick, system, gpus, power, processes, groups, uid: self.uid, cost: Duration::ZERO };
        frame.total_graphics();
        frame.cost = started.elapsed();
        frame
    }
}

/// "Intel(R) Core(TM) Ultra 7 155H" → "Intel Core Ultra 7 155H".
pub fn tidy_cpu_name(name: &str) -> String {
    let name = name.replace("(R)", "").replace("(TM)", "").replace("(tm)", "");
    name.split_whitespace()
        .filter(|w| !w.ends_with("-Core") && *w != "Processor" && *w != "CPU")
        .collect::<Vec<_>>()
        .join(" ")
}

impl Sampler {
    /// The processor's temperature, from the sensor found the first time.
    fn temperature(&mut self) -> Option<f64> {
        let path = self.sensor.get_or_insert_with(|| {
            let all = sysinfo::temperatures();
            sysinfo::pick_cpu_temperature(&all)
                .map(|t| PathBuf::from(format!("/sys/class/hwmon/{}/temp{}_input", t.hwmon, t.index)))
        });
        let text = std::fs::read_to_string(path.as_ref()?).ok()?;
        text.trim().parse::<f64>().ok().map(|mc| mc / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_sys::sysinfo::power::EnergyCounter;

    #[test]
    fn nvidia_matches_pci_and_keeps_amd_allocations_separate() {
        let mut cards = vec![
            model::Gpu { pci: "0000:78:00.0".into(), ..Default::default() },
            model::Gpu { pci: "0000:01:00.0".into(), ..Default::default() },
        ];
        let mut processes = vec![Process {
            pid: 42,
            gpus: vec![GpuUse { device: 0, usage: GpuUsage { vram: Some(12 << 10), ..Default::default() } }],
            ..Default::default()
        }];
        assert_eq!(client_busy(&processes, 0), Some(0.0));
        assert_eq!(client_busy(&processes, 1), None, "AMD cannot make an unmeasured NVIDIA card look idle");
        let reading = crate::nvml::Reading {
            pci: cards[1].pci.clone(),
            vram_used: Some(2941 << 20),
            processes: vec![
                (42, GpuUsage { vram: Some(355 << 20), ..Default::default() }),
                (999, GpuUsage::default()), // Already exited, or not visible in /proc.
            ],
            ..Default::default()
        };
        merge_nvidia(&mut cards, &mut processes, std::slice::from_ref(&reading));
        merge_nvidia(&mut cards, &mut processes, &[reading]);
        assert_eq!(processes[0].gpus.len(), 2, "never duplicate a device's process figures");
        assert_eq!(processes[0].gpus[0].usage.vram, Some(12 << 10));
        assert_eq!(processes[0].gpus[1].usage.vram, Some(355 << 20));
        assert_eq!(cards[0].vram_used, None);
        assert_eq!(cards[1].vram_used, Some(2941 << 20));
        assert_eq!(cards[1].busy, None);
    }

    #[test]
    fn graphics_cards_measure_without_root_or_a_battery() {
        let apu = model::Gpu {
            driver: "amdgpu".into(),
            integrated: true,
            package_meter: true,
            power: Some(48.0),
            ..Default::default()
        };
        let rtx = model::Gpu { driver: "nvidia".into(), power: Some(400.0), ..Default::default() };
        // RAPL is root's and there's no battery, but an APU's Radeon meters
        // the processor package, and the RTX has its own meter.
        let p = meters(None, None, false, &[apu.clone(), rtx.clone()]);
        assert_eq!((p.meter, p.cpu, p.graphics, p.total), (Meter::Package, Some(48.0), Some(400.0), Some(448.0)));
        let p = meters(Some(Some(60.0)), None, false, &[apu.clone(), rtx.clone()]);
        assert_eq!(
            (p.meter, p.cpu, p.total),
            (Meter::Rapl, Some(60.0), Some(460.0)),
            "the package isn't counted twice"
        );
        // A Ryzen desktop processor's Radeon meters itself alone: a tenth of
        // a watt with every core busy is not the processor's.
        let granite_ridge = model::Gpu { package_meter: false, power: Some(0.11), ..apu };
        let p = meters(None, None, false, &[granite_ridge.clone(), rtx.clone()]);
        assert_eq!((p.meter, p.cpu, p.total), (Meter::Graphics, None, Some(400.0)));
        let p = meters(Some(Some(95.0)), None, false, &[granite_ridge.clone(), rtx.clone()]);
        assert_eq!((p.meter, p.cpu, p.total), (Meter::Rapl, Some(95.0), Some(495.0)));
        let p = meters(Some(None), None, false, &[granite_ridge, rtx.clone()]);
        assert_eq!((p.meter, p.cpu, p.total), (Meter::Rapl, None, None), "RAPL, before its first rate");
        let p = meters(None, None, false, std::slice::from_ref(&rtx));
        assert_eq!((p.meter, p.cpu, p.total), (Meter::Graphics, None, Some(400.0)));
        // A laptop: Intel's integrated graphics have no meter of their own.
        let xe = model::Gpu { driver: "xe".into(), integrated: true, ..Default::default() };
        let p = meters(None, Some(32.0), true, std::slice::from_ref(&xe));
        assert_eq!((p.meter, p.graphics, p.total), (Meter::Battery, None, Some(32.0)));
        assert_eq!(meters(None, Some(32.0), false, &[xe]).meter, Meter::None, "charging measures nothing");
    }

    /// A reading of package-0's counter at `ms` milliseconds of
    /// `CLOCK_BOOTTIME`.
    fn package(ms: u64, microjoules: u64) -> Option<RaplReading> {
        let counters = vec![EnergyCounter { name: "package-0".into(), microjoules, range: 65_532_610_987 }];
        Some(RaplReading { at: ms * 1000, counters })
    }

    #[test]
    fn rapl_through_the_meter_keeps_its_rate_between_new_readings() {
        let mut rapl = Rapl::default();
        assert_eq!(rapl.read(None), None, "neither readable nor answered");
        assert_eq!(rapl.read(package(0, 1_000_000)), Some(None));
        // The first frame comes 300 ms after the first reading, before the
        // meter takes another: its answer is the same.
        assert_eq!(rapl.read(package(0, 1_000_000)), Some(None));
        assert_eq!(rapl.read(package(1250, 96_000_000)), Some(Some(76.0)));
        assert_eq!(rapl.read(package(1250, 96_000_000)), Some(Some(76.0)), "asked again too soon");
        assert_eq!(rapl.read(package(2250, 191_000_000)), Some(Some(95.0)), "from the last new reading");
        // The counter wraps at its range.
        let mut rapl = Rapl::default();
        rapl.read(package(0, 65_532_000_000));
        assert_eq!(rapl.read(package(1000, 10_000_000)), Some(Some(10.610987)));
        assert_eq!(rapl.read(None), None, "the meter went away");
        assert_eq!(rapl.read(package(3000, 20_000_000)), Some(None), "a new start");
    }

    #[test]
    fn rapl_rates_are_over_the_time_between_readings_not_questions() {
        // A package drawing 10 W. The window asks every second, at 2, 3, 4
        // and 5 s; `aizome-myaku --print` asked the meter at 2.6 and 3.6 s,
        // so the window's questions at 3 and 4 s get those readings. Timed
        // by its own questions, the window would show 6 W, then 10 W, then
        // 14 W.
        let ten_watts = |ms: u64| package(ms, ms * 10_000);
        let near = |rate: Option<Option<f64>>| rate.flatten().map(|w| (w * 1e6).round() / 1e6);
        let mut rapl = Rapl::default();
        assert_eq!(rapl.read(ten_watts(2000)), Some(None));
        assert_eq!(near(rapl.read(ten_watts(2600))), Some(10.0));
        assert_eq!(near(rapl.read(ten_watts(3600))), Some(10.0));
        assert_eq!(near(rapl.read(ten_watts(5000))), Some(10.0));
    }

    #[test]
    fn a_night_asleep_measures_nothing() {
        let start = Instant::now();
        let at = |awake_ms: u64, boot_ms: u64| Moment {
            monotonic: start + Duration::from_millis(awake_ms),
            boot: boot_ms * 1000,
        };
        assert_eq!(interval(None, at(0, 5000)), (0.0, false), "the first reading");
        assert_eq!(interval(Some(at(0, 5000)), at(1000, 6000)), (1.0, false));
        // Readings a second apart on Myaku's clock, with eight hours asleep
        // between them: CLOCK_BOOTTIME and RAPL's counters went on.
        let night = 8 * 3600 * 1000;
        assert_eq!(interval(Some(at(1000, 6000)), at(2000, 7000 + night)), (0.0, true));
        // No one's energy grows by that interval, and RAPL's rate starts over
        // (`Sampler::sample`): over the night it would be the night's
        // average, not what the processor draws now. A second later it is
        // all measured again.
        assert_eq!(interval(Some(at(2000, 7000 + night)), at(3000, 8000 + night)), (1.0, false));
        // The two clocks never differ by much more than the time between
        // reading them, awake.
        let jitter = Moment { boot: 9_000_040, ..at(4000, 0) };
        assert_eq!(interval(Some(at(3000, 8000)), jitter), (1.0, false));
    }

    /// Myaku sampling this computer through Aizome's power meter, with the
    /// clocks and the package's counter scripted: the counter reads
    /// `energy(ms)` microjoules at `ms` milliseconds of `CLOCK_BOOTTIME`,
    /// and the meter reads it at most once a second, answering in between
    /// with its last reading, as `aizome-power-meter` does. No card's meter
    /// or energy counter: the processor's alone.
    struct Scripted<E: Fn(u64) -> u64> {
        sampler: Sampler,
        energy: E,
        start: Instant,
        /// When the meter last read the counter, and how many times it has.
        read: Option<u64>,
        reads: usize,
    }

    impl<E: Fn(u64) -> u64> Scripted<E> {
        fn new(energy: E) -> Scripted<E> {
            let mut sampler = Sampler::new();
            sampler.cards.clear();
            sampler.nvml = None;
            sampler.gpu_energy.clear();
            sampler.gpu_floors.clear();
            Scripted { sampler, energy, start: Instant::now(), read: None, reads: 0 }
        }

        /// A frame `awake` milliseconds into Myaku's clock and `boot` into
        /// `CLOCK_BOOTTIME`, asking the meter `late` milliseconds into it
        /// (after the process scan).
        fn frame(&mut self, awake: u64, boot: u64, late: u64) -> Frame {
            // Some processor time, so that this process has a part of the
            // package's power.
            let spin = Instant::now();
            while spin.elapsed() < Duration::from_millis(30) {
                std::hint::spin_loop();
            }
            let asked = boot + late;
            let at = match self.read {
                Some(at) if asked - at < 1000 => at,
                _ => {
                    self.reads += 1;
                    asked
                }
            };
            self.read = Some(at);
            let reading = package(at, (self.energy)(at));
            let now = Moment { monotonic: self.start + Duration::from_millis(awake), boot: boot * 1000 };
            self.sampler.sample_at(now, |_| reading)
        }
    }

    /// The processor's power in a frame, to the microwatt.
    fn processor(f: &Frame) -> Option<f64> {
        f.power.cpu.map(|w| (w * 1e6).round() / 1e6)
    }

    /// No process is held to have used more than the package drew, `watts`.
    fn within(f: &Frame, watts: f64) {
        for p in &f.processes {
            assert!(
                p.usage.energy <= watts * p.usage.measured + 1e-9,
                "{} ({}): {} J in {} s",
                p.name,
                p.pid,
                p.usage.energy,
                p.usage.measured
            );
        }
    }

    fn this_process(f: &Frame) -> &Process {
        f.processes.iter().find(|p| p.pid == std::process::id()).expect("this process is in the frame")
    }

    #[test]
    fn the_meters_repeated_answers_keep_the_processor_measured() {
        // A package drawing 10 W. Myaku's timer gives a frame 300 ms after
        // the first reading, then one every second, each asking the meter a
        // few milliseconds in: a question a moment under a second after the
        // meter's last reading gets that reading again.
        let mut m = Scripted::new(|ms| ms * 10_000);
        let _ = m.frame(0, 5000, 12);
        assert_eq!(processor(&m.frame(300, 5300, 3)), None, "no rate before a second reading");
        let mut f = None;
        for (awake, late) in (1300..).step_by(1000).zip([5, 31, 2, 18, 40, 1, 25, 9, 9, 33]) {
            let frame = m.frame(awake, 5000 + awake, late);
            assert_eq!(processor(&frame), Some(10.0), "at {awake} ms: neither a dash, nor 0 W, nor off");
            assert_eq!(frame.power.total.map(|w| (w * 1e6).round() / 1e6), Some(10.0));
            within(&frame, 10.0);
            f = Some(frame);
        }
        // Of twelve questions, the meter read for eight: the frames at 0.3,
        // 3.3, 6.3 and 8.3 s got the last reading again.
        assert_eq!(m.reads, 8);
        // This process was measured over each of the ten seconds that showed
        // a rate, by its share of the package.
        let me = this_process(f.as_ref().unwrap());
        assert_eq!(me.usage.measured, 10.0);
        assert!(me.usage.energy > 0.0 && me.usage.energy <= 100.0, "{} J", me.usage.energy);
    }

    #[test]
    fn a_night_asleep_neither_spikes_nor_adds_to_anyones_energy() {
        // A package drawing 10 W awake. The lid closes at 8.5 s of
        // CLOCK_BOOTTIME for eight hours, in which it draws 0.5 W and its
        // counter goes on, as RAPL's does: 14.4 kJ. Myaku's clock stops, so
        // its next frame comes a second after the last on that clock, eight
        // hours later on CLOCK_BOOTTIME. Over that second, the night would be
        // 14.4 kW, and each process's share of it its energy for good.
        const NIGHT: u64 = 8 * 3600 * 1000;
        const CLOSED: u64 = 8500;
        let mut m = Scripted::new(|ms: u64| {
            let asleep = ms.saturating_sub(CLOSED).min(NIGHT);
            (ms - asleep) * 10_000 + asleep * 500
        });
        let _ = m.frame(0, 5000, 12);
        let _ = m.frame(300, 5300, 3);
        let mut last = None;
        for (awake, late) in [(1300, 5), (2300, 31), (3300, 2)] {
            let frame = m.frame(awake, 5000 + awake, late);
            assert_eq!(processor(&frame), Some(10.0));
            last = Some(frame);
        }
        let before: HashMap<Key, (f64, f64)> =
            last.unwrap().processes.iter().map(|p| (p.key(), (p.usage.energy, p.usage.measured))).collect();

        let woke = m.frame(4300, 5000 + 4300 + NIGHT, 30);
        assert_eq!(processor(&woke), None, "RAPL's rate starts over: no spike");
        assert_eq!(woke.power.total, None);
        within(&woke, 10.0);
        let mut kept = 0;
        for p in &woke.processes {
            if let Some(&was) = before.get(&p.key()) {
                assert_eq!((p.usage.energy, p.usage.measured), was, "{} ({}) used nothing asleep", p.name, p.pid);
                kept += 1;
            }
        }
        assert!(kept > 0);
        // The next frame asks a moment under a second after the meter's
        // first reading since waking and gets it again: the processor shows
        // a dash for a second or two, then what it draws.
        assert_eq!(processor(&m.frame(5300, 5000 + 5300 + NIGHT, 4)), None);
        assert_eq!(processor(&m.frame(6300, 5000 + 6300 + NIGHT, 10)), Some(10.0));
        let last = m.frame(7300, 5000 + 7300 + NIGHT, 6);
        assert_eq!(processor(&last), Some(10.0));
        within(&last, 10.0);
        // Measured over the five seconds that showed a rate, three before the
        // night and two after: not the night, nor the dash.
        let me = this_process(&last);
        assert_eq!(me.usage.measured, 5.0);
        assert!(me.usage.energy > 0.0 && me.usage.energy <= 50.0, "{} J", me.usage.energy);
    }

    #[test]
    fn each_meter_is_shared_by_use_of_what_it_measures() {
        let gpus = [model::Gpu { driver: "nvidia".into(), power: Some(400.0), ..Default::default() }];
        let on_card = |busy| vec![GpuUse { device: 0, usage: GpuUsage { busy: Some(busy), ..Default::default() } }];
        let cpu = |cpu| Usage { cpu, ..Default::default() };
        let processes = [
            Process { pid: 1, usage: cpu(100.0), gpus: on_card(50.0), ..Default::default() },
            Process { pid: 2, gpus: on_card(0.0), ..Default::default() },
            Process { pid: 3, usage: cpu(400.0), ..Default::default() },
        ];
        let parts = |power: &model::Power| {
            attribute(power, &gpus, &processes, 16.0, &mut Floor::default(), &mut [Floor::default()])
        };
        // The card's meter alone: its idle client is measured, what uses no
        // card isn't.
        assert_eq!(parts(&meters(None, None, false, &gpus)), [Some(200.0), Some(0.0), None]);
        // On battery, the card's part of the draw goes by its time on the
        // card, the rest by processor time.
        assert_eq!(parts(&meters(None, Some(480.0), true, &gpus)), [Some(205.0), Some(0.0), Some(20.0)]);
    }

    #[test]
    fn cpu_names_lose_their_marks() {
        assert_eq!(tidy_cpu_name("Intel(R) Core(TM) Ultra 7 155H"), "Intel Core Ultra 7 155H");
        assert_eq!(tidy_cpu_name("AMD Ryzen 9 7950X 16-Core Processor"), "AMD Ryzen 9 7950X");
    }

    /// A reading of this computer: something to show, and what it costs.
    #[test]
    fn a_reading_of_this_computer() {
        let mut s = Sampler::new();
        let _ = s.sample();
        std::thread::sleep(Duration::from_millis(200));
        let f = s.sample();
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(100));
            let g = s.sample();
            eprintln!("reading {}: {:?} {:?}", g.tick, g.cost, s.phases);
        }
        assert!(f.processes.len() > 5);
        assert!(
            f.groups.iter().any(|g| g.kind == Some(crate::model::Kind::Kernel))
                || f.processes.iter().all(|p| !p.kernel)
        );
        assert!(f.processes.iter().all(|p| p.group < f.groups.len()));
        let members: usize = f.groups.iter().map(|g| g.members.len()).sum();
        assert_eq!(members, f.processes.len(), "every process in one group");
        eprintln!("{} processes in {} groups, read in {:?}", f.processes.len(), f.groups.len(), f.cost);
    }
}
