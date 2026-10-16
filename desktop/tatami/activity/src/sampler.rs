// SPDX-License-Identifier: MIT OR Apache-2.0
//! The sampler: a thread of its own that reads the system every few
//! seconds and sends the window a `Frame`. Rates come from two readings.
//! The costlier figures are read less often: a process's memory maps (its
//! proportional and private sizes) every fifth reading, unless the Memory
//! view is showing or the process is the one selected, and the list of
//! devices a process has open every tenth.
use crate::{
    apps,
    gpu::{self, Client},
    model::{self, Frame, GpuUse, Lens, Meter, Process, Usage},
    nvml::Nvml,
    power::{self, Counter, Floor},
    procfs::{self, ProcStat, Smaps},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

pub enum Command {
    Interval(Duration),
    /// What the window shows: the processes whose costlier figures to read
    /// at every reading.
    Focus {
        lens: Lens,
        pids: Vec<i32>,
    },
}

/// Start the sampler; frames arrive on `frames` until it is dropped.
pub fn spawn(frames: async_channel::Sender<Frame>, interval: Duration) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            let mut sampler = Sampler::new();
            let mut interval = interval;
            sampler.sample();
            // The first frame soon, so the window has something to show.
            let mut next = Instant::now() + Duration::from_millis(350);
            loop {
                match receiver.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(Command::Interval(new)) => {
                        interval = new;
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
                if frames.send_blocking(frame).is_err() {
                    break;
                }
                next += interval;
                let now = Instant::now();
                if next < now {
                    next = now + interval;
                }
            }
        })
        .expect("the sampler thread starts");
    commands
}

struct Previous {
    start: u64,
    time: u64,
    voluntary: u64,
    io: Option<(u64, u64)>,
}

struct Cached {
    start: u64,
    name: String,
    role: String,
    command: String,
    exe: String,
    comm: String,
    cgroup: String,
    named: u64,
    grouped: u64,
}

struct Drm {
    start: u64,
    fds: Vec<u32>,
    scanned: u64,
}

struct ClientBefore {
    engines: HashMap<String, u64>,
    cycles: HashMap<String, (u64, u64)>,
    at: Instant,
}

pub struct Sampler {
    ticks: f64,
    uid: u32,
    tick: u64,
    pub lens: Lens,
    pub focus: HashSet<i32>,
    users: HashMap<u32, String>,
    desktop: apps::Desktop,
    cpu_name: String,
    last: Option<Instant>,
    stat: Option<ProcStat>,
    previous: HashMap<i32, Previous>,
    cache: HashMap<i32, Cached>,
    smaps: HashMap<i32, (u64, Smaps)>,
    drm: HashMap<i32, Drm>,
    clients: HashMap<(String, u64), ClientBefore>,
    devices: Vec<gpu::Device>,
    nvml: Option<Nvml>,
    rapl: Vec<PathBuf>,
    rapl_before: Option<Vec<Counter>>,
    gpu_energy_before: Vec<Option<f64>>,
    cpu_floor: Floor,
    gpu_floors: Vec<Floor>,
    /// Joules and seconds measured, by process or group key.
    energy: HashMap<String, (f64, f64)>,
    total_energy: (f64, f64),
    disks_before: HashMap<String, procfs::DiskCounters>,
    disk_totals: (u64, u64),
    temperature: Option<PathBuf>,
    buffer: String,
}

/// The processor temperature sensor: k10temp's Tctl, coretemp's package,
/// or a thermal zone for the package.
fn find_temperature() -> Option<PathBuf> {
    let mut found = None;
    for entry in std::fs::read_dir("/sys/class/hwmon")
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let name = procfs::read(path.join("name")).unwrap_or_default();
        match name.trim() {
            "k10temp" | "zenpower" | "cpu_thermal" => return Some(path.join("temp1_input")),
            "coretemp" => {
                for index in 1..=4 {
                    let label = procfs::read(path.join(format!("temp{index}_label")));
                    if label.is_some_and(|label| label.starts_with("Package")) {
                        found = Some(path.join(format!("temp{index}_input")));
                    }
                }
            }
            _ => {}
        }
    }
    found.or_else(|| {
        std::fs::read_dir("/sys/class/thermal")
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .find(|zone| {
                procfs::read(zone.join("type")).is_some_and(|kind| kind.trim() == "x86_pkg_temp")
            })
            .map(|zone| zone.join("temp"))
    })
}

impl Sampler {
    pub fn new() -> Sampler {
        let devices = gpu::discover();
        let nvml = devices
            .iter()
            .any(|device| device.vendor == "NVIDIA")
            .then(Nvml::open)
            .flatten();
        let count = devices.len();
        Sampler {
            ticks: procfs::clock_ticks(),
            // SAFETY: getuid has no preconditions.
            uid: unsafe { libc::getuid() },
            tick: 0,
            lens: Lens::Cpu,
            focus: HashSet::new(),
            users: procfs::read("/etc/passwd")
                .map(|text| procfs::parse_passwd(&text))
                .unwrap_or_default(),
            desktop: apps::Desktop::load(),
            cpu_name: procfs::read("/proc/cpuinfo")
                .map(|text| procfs::parse_cpu_name(&text))
                .unwrap_or_default(),
            last: None,
            stat: None,
            previous: HashMap::new(),
            cache: HashMap::new(),
            smaps: HashMap::new(),
            drm: HashMap::new(),
            clients: HashMap::new(),
            devices,
            nvml,
            rapl: power::rapl_domains(),
            rapl_before: None,
            gpu_energy_before: vec![None; count],
            cpu_floor: Floor::default(),
            gpu_floors: vec![Floor::default(); count],
            energy: HashMap::new(),
            total_energy: (0.0, 0.0),
            disks_before: HashMap::new(),
            disk_totals: (0, 0),
            temperature: find_temperature(),
            buffer: String::new(),
        }
    }

    fn user(&self, uid: u32) -> String {
        self.users
            .get(&uid)
            .cloned()
            .unwrap_or_else(|| uid.to_string())
    }

    pub fn sample(&mut self) -> Frame {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |last| (now - last).as_secs_f64());
        self.last = Some(now);
        self.tick += 1;
        let tick = self.tick;
        if tick.is_multiple_of(30) {
            self.desktop = apps::Desktop::load();
        }
        if tick.is_multiple_of(60)
            && let Some(text) = procfs::read("/etc/passwd")
        {
            self.users = procfs::parse_passwd(&text);
        }

        let stat = procfs::read("/proc/stat")
            .map(|text| procfs::parse_proc_stat(&text))
            .unwrap_or_default();
        let cores = stat.cores.len().max(1);
        let mut system = self.system(&stat, dt);
        self.stat = Some(stat.clone());

        // Each process.
        let mut processes = Vec::new();
        let mut clients: Vec<(i32, Client)> = Vec::new();
        let mut seen = HashSet::new();
        let pids: Vec<i32> = std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .collect();
        for pid in pids {
            if let Some(process) = self.process(pid, dt, &stat, &mut clients) {
                seen.insert(pid);
                processes.push(process);
            }
        }
        self.previous.retain(|pid, _| seen.contains(pid));
        self.cache.retain(|pid, _| seen.contains(pid));
        self.smaps.retain(|pid, _| seen.contains(pid));
        self.drm.retain(|pid, _| seen.contains(pid));
        system.processes = processes.len();
        system.threads = processes.iter().map(|p| p.usage.threads).sum();

        // Graphics.
        let mut gpus: Vec<model::Gpu> = Vec::new();
        let mut gpu_power: Vec<Option<f64>> = Vec::new();
        let nvidia = self.nvml.as_mut().map(Nvml::read).unwrap_or_default();
        for (index, device) in self.devices.iter().enumerate() {
            let sysfs = device.read();
            let mut gpu = model::Gpu {
                name: device.name.clone(),
                vendor: device.vendor,
                driver: device.driver.clone(),
                pci: device.pci.clone(),
                integrated: device.integrated,
                busy: sysfs.busy,
                video: sysfs.video,
                vram_used: sysfs.vram_used,
                vram_total: sysfs.vram_total,
                power: sysfs.power,
                temperature: sysfs.temperature,
                clock: sysfs.clock,
            };
            let mut counter = sysfs.energy;
            if let Some(reading) = nvidia.iter().find(|reading| reading.pci == device.pci) {
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
                counter = reading.energy;
                for &(pid, busy, video, vram) in &reading.processes {
                    if let Some(process) = processes.iter_mut().find(|p| p.pid == pid) {
                        process.gpus.push(GpuUse {
                            device: index,
                            busy,
                            video,
                            vram,
                            shared: 0,
                        });
                    }
                }
            }
            // A counter gives the interval's average; a reading, now.
            let before = std::mem::replace(&mut self.gpu_energy_before[index], counter);
            let watts = match (before, counter) {
                (Some(before), Some(after)) if dt > 0.0 && after >= before => {
                    Some((after - before) / dt)
                }
                _ => gpu.power,
            };
            gpu.power = watts;
            gpu_power.push(watts);
            gpus.push(gpu);
        }
        self.drm_use(&mut processes, clients, now);
        for process in &mut processes {
            for gpu in &process.gpus {
                process.usage.gpu += gpu.busy;
                process.usage.video += gpu.video;
                process.usage.vram += gpu.vram;
                process.usage.gpu_shared += gpu.shared;
            }
        }
        // Without its own counter, a card's busy share is its processes'.
        for (index, gpu) in gpus.iter_mut().enumerate() {
            if gpu.busy.is_none() {
                let used: f64 = processes
                    .iter()
                    .flat_map(|p| &p.gpus)
                    .filter(|g| g.device == index)
                    .map(|g| g.busy)
                    .sum();
                gpu.busy = Some(used.min(100.0));
            }
        }

        let power = self.power(&mut processes, &gpus, &gpu_power, cores, dt);

        // Applications.
        let nodes: Vec<apps::Node> = processes
            .iter()
            .map(|p| {
                let cached = self.cache.get(&p.pid);
                apps::Node {
                    pid: p.pid,
                    ppid: p.ppid,
                    started: p.started,
                    uid: p.uid,
                    kernel: p.kernel,
                    name: p.name.clone(),
                    comm: cached.map(|c| c.comm.clone()).unwrap_or_default(),
                    exe: p.exe.clone(),
                    cgroup: p.cgroup.clone(),
                }
            })
            .collect();
        let windows = apps::windows();
        let mut groups = Vec::new();
        for members in apps::group(&nodes) {
            let identity = apps::identify(&nodes, &members, &windows, &self.desktop, self.uid);
            let key = format!("g{}", processes[members[0]].key);
            let mut usage = Usage::default();
            for &i in &members {
                usage.add(&processes[i].usage);
                processes[i].group = key.clone();
            }
            usage.energy = 0.0;
            usage.measured = 0.0;
            let entry = self.energy.entry(key.clone()).or_default();
            if let Some(watts) = usage.power {
                entry.0 += watts * dt;
                entry.1 += dt;
            }
            (usage.energy, usage.measured) = *entry;
            groups.push(model::Group {
                key,
                kind: Some(identity.kind),
                name: identity.name,
                icon: identity.icon,
                detail: identity.detail,
                windows: identity.windows,
                members: members.iter().map(|&i| processes[i].pid).collect(),
                usage,
            });
        }
        let live: HashSet<&str> = processes
            .iter()
            .map(|p| p.key.as_str())
            .chain(groups.iter().map(|g| g.key.as_str()))
            .collect();
        self.energy.retain(|key, _| live.contains(key.as_str()));

        Frame {
            system,
            gpus,
            power,
            processes,
            groups,
            uid: self.uid,
        }
    }

    fn system(&mut self, stat: &ProcStat, dt: f64) -> model::System {
        let mut system = model::System {
            cpu_name: self.cpu_name.clone(),
            ..Default::default()
        };
        if let Some(before) = &self.stat {
            let share = |after: &procfs::CpuTimes, before: &procfs::CpuTimes| {
                let total = after.total().saturating_sub(before.total()).max(1) as f64;
                (
                    after.busy().saturating_sub(before.busy()) as f64 / total * 100.0,
                    after.user.saturating_sub(before.user) as f64 / total * 100.0,
                    (after.system + after.other).saturating_sub(before.system + before.other)
                        as f64
                        / total
                        * 100.0,
                    after.iowait.saturating_sub(before.iowait) as f64 / total * 100.0,
                )
            };
            (system.cpu, system.user, system.kernel, system.iowait) =
                share(&stat.total, &before.total);
            system.cores = stat
                .cores
                .iter()
                .zip(&before.cores)
                .map(|(after, before)| share(after, before).0)
                .collect();
        } else {
            system.cores = vec![0.0; stat.cores.len()];
        }
        system.frequencies = (0..stat.cores.len())
            .filter_map(|cpu| {
                procfs::number(format!(
                    "/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq"
                ))
                .map(|khz| khz as f64 / 1000.0)
            })
            .collect();
        if let Some(text) = procfs::read("/proc/loadavg") {
            let loads: Vec<f64> = text
                .split_whitespace()
                .take(3)
                .filter_map(|f| f.parse().ok())
                .collect();
            if loads.len() == 3 {
                system.load = [loads[0], loads[1], loads[2]];
            }
        }
        system.uptime = procfs::read("/proc/uptime")
            .and_then(|text| text.split_whitespace().next()?.parse().ok())
            .unwrap_or(0.0);
        system.temperature = self
            .temperature
            .as_ref()
            .and_then(procfs::number)
            .map(|millidegrees| millidegrees as f64 / 1000.0);
        if let Some(text) = procfs::read("/proc/meminfo") {
            system.memory = procfs::parse_meminfo(&text);
        }
        // zram: the data it holds, and what that takes compressed.
        let mut compressed = (0, 0);
        for entry in std::fs::read_dir("/sys/block")
            .into_iter()
            .flatten()
            .flatten()
        {
            if !entry.file_name().to_string_lossy().starts_with("zram") {
                continue;
            }
            if let Some(text) = procfs::read(entry.path().join("mm_stat")) {
                let fields: Vec<u64> = text
                    .split_whitespace()
                    .filter_map(|f| f.parse().ok())
                    .collect();
                if fields.len() >= 3 {
                    compressed.0 += fields[0];
                    compressed.1 += fields[2];
                }
            }
        }
        if compressed.0 > 0 {
            system.memory.compressed = Some(compressed);
        }
        let pressure = |kind: &str| {
            procfs::read(format!("/proc/pressure/{kind}"))
                .map(|text| procfs::parse_pressure(&text))
                .unwrap_or_default()
        };
        let (cpu, _) = pressure("cpu");
        let (memory_some, memory_full) = pressure("memory");
        let (io_some, io_full) = pressure("io");
        system.pressure = model::Pressure {
            cpu,
            memory_some,
            memory_full,
            io_some,
            io_full,
        };
        if let Some(text) = procfs::read("/proc/diskstats") {
            let disks = procfs::parse_diskstats(&text, |name| {
                std::path::Path::new("/sys/block").join(name).exists()
            });
            let mut rates = Vec::new();
            for disk in disks {
                if let Some(before) = self.disks_before.get(&disk.name)
                    && dt > 0.0
                {
                    let read = disk.sectors_read.saturating_sub(before.sectors_read) * 512;
                    let written = disk.sectors_written.saturating_sub(before.sectors_written) * 512;
                    self.disk_totals.0 += read;
                    self.disk_totals.1 += written;
                    rates.push(model::Disk {
                        name: disk.name.clone(),
                        read_rate: read as f64 / dt,
                        write_rate: written as f64 / dt,
                        busy: (disk.busy_ms.saturating_sub(before.busy_ms) as f64 / (dt * 10.0))
                            .min(100.0),
                    });
                } else {
                    rates.push(model::Disk {
                        name: disk.name.clone(),
                        ..Default::default()
                    });
                }
                self.disks_before.insert(disk.name.clone(), disk);
            }
            system.disks = rates;
        }
        (system.read_total, system.write_total) = self.disk_totals;
        system
    }

    fn process(
        &mut self,
        pid: i32,
        dt: f64,
        stat: &ProcStat,
        clients: &mut Vec<(i32, Client)>,
    ) -> Option<Process> {
        let tick = self.tick;
        let dir = format!("/proc/{pid}");
        if !procfs::read_into(format!("{dir}/stat"), &mut self.buffer) {
            return None;
        }
        let ps = procfs::parse_stat(&self.buffer)?;
        // Kernel threads, often half the list, have no memory or files of
        // their own: their processor time is all there is to read.
        let status = if ps.kernel {
            procfs::Status::default()
        } else if procfs::read_into(format!("{dir}/status"), &mut self.buffer) {
            procfs::parse_status(&self.buffer)
        } else {
            procfs::Status::default()
        };
        let io = if ps.kernel {
            None
        } else if procfs::read_into(format!("{dir}/io"), &mut self.buffer) {
            procfs::parse_io(&self.buffer)
        } else {
            None
        };
        let readable = io.is_some();
        let previous = self.previous.get(&pid).filter(|p| p.start == ps.start);
        let rate = |after: u64, before: u64| {
            if dt > 0.0 {
                after.saturating_sub(before) as f64 / dt
            } else {
                0.0
            }
        };
        let cpu = previous.map_or(0.0, |p| rate(ps.time, p.time) / self.ticks * 100.0);
        let wakeups = previous.map_or(0.0, |p| rate(status.voluntary, p.voluntary));
        let (read_rate, write_rate) = match (io, previous.and_then(|p| p.io)) {
            (Some(after), Some(before)) => {
                (Some(rate(after.0, before.0)), Some(rate(after.1, before.1)))
            }
            (Some(_), None) => (Some(0.0), Some(0.0)),
            _ => (None, None),
        };
        self.previous.insert(
            pid,
            Previous {
                start: ps.start,
                time: ps.time,
                voluntary: status.voluntary,
                io,
            },
        );

        // The memory maps: read for the Memory view and the selected
        // process every time, otherwise every fifth reading.
        let focused = self.focus.contains(&pid);
        let stale = self
            .smaps
            .get(&pid)
            .is_none_or(|(start, _)| *start != ps.start);
        let due =
            self.lens == Lens::Memory || focused || stale || (tick + pid as u64).is_multiple_of(5);
        if readable
            && due
            && procfs::read_into(format!("{dir}/smaps_rollup"), &mut self.buffer)
            && let Some(smaps) = procfs::parse_smaps(&self.buffer)
        {
            self.smaps.insert(pid, (ps.start, smaps));
        }
        let smaps = self
            .smaps
            .get(&pid)
            .filter(|(start, _)| *start == ps.start)
            .map(|(_, smaps)| smaps.clone());

        // Names, arguments and the control group change seldom.
        let refresh = self.cache.get(&pid).is_none_or(|c| c.start != ps.start);
        if refresh {
            let args = std::fs::read(format!("{dir}/cmdline"))
                .map(|bytes| procfs::parse_cmdline(&bytes))
                .unwrap_or_default();
            let exe = std::fs::read_link(format!("{dir}/exe"))
                .map(|path| {
                    path.to_string_lossy()
                        .trim_end_matches(" (deleted)")
                        .to_owned()
                })
                .unwrap_or_default();
            let (name, role) = apps::name_and_role(&ps.comm, &args, &exe);
            let cgroup = procfs::read(format!("{dir}/cgroup"))
                .map(|text| procfs::parse_cgroup(&text))
                .unwrap_or_default();
            self.cache.insert(
                pid,
                Cached {
                    start: ps.start,
                    name,
                    role,
                    command: if args.is_empty() {
                        format!("[{}]", ps.comm)
                    } else {
                        args.join(" ")
                    },
                    exe,
                    comm: ps.comm.clone(),
                    cgroup,
                    named: tick,
                    grouped: tick,
                },
            );
        } else if let Some(cached) = self.cache.get_mut(&pid) {
            // Chromium retitles its helpers, and applications move into
            // scopes of their own after they start.
            if tick - cached.named >= 15 && !ps.kernel {
                let args = std::fs::read(format!("{dir}/cmdline"))
                    .map(|bytes| procfs::parse_cmdline(&bytes))
                    .unwrap_or_default();
                if !args.is_empty() {
                    (cached.name, cached.role) = apps::name_and_role(&ps.comm, &args, &cached.exe);
                    cached.command = args.join(" ");
                }
                cached.named = tick;
            }
            if !ps.kernel && (tick + pid as u64).is_multiple_of(5) && tick != cached.grouped {
                cached.cgroup = procfs::read(format!("{dir}/cgroup"))
                    .map(|text| procfs::parse_cgroup(&text))
                    .unwrap_or_default();
                cached.grouped = tick;
            }
        }
        let cached = self.cache.get(&pid)?;
        let cwd = if focused {
            std::fs::read_link(format!("{dir}/cwd"))
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Graphics devices it has open.
        if readable {
            let drm = self.drm.entry(pid).or_insert(Drm {
                start: ps.start,
                fds: Vec::new(),
                scanned: 0,
            });
            if drm.start != ps.start || drm.scanned == 0 || (tick + pid as u64).is_multiple_of(10) {
                drm.fds = gpu::drm_fds(pid);
                drm.start = ps.start;
                drm.scanned = tick;
            }
            for fd in &drm.fds {
                if procfs::read_into(format!("{dir}/fdinfo/{fd}"), &mut self.buffer)
                    && let Some(client) = gpu::parse_fdinfo(&self.buffer)
                {
                    clients.push((pid, client));
                }
            }
        }

        let started = stat.boot + (ps.start as f64 / self.ticks) as u64;
        Some(Process {
            pid,
            ppid: ps.ppid,
            started,
            key: format!("{pid}:{}", ps.start),
            name: cached.name.clone(),
            role: cached.role.clone(),
            command: cached.command.clone(),
            exe: cached.exe.clone(),
            cwd,
            user: self.user(status.uid),
            uid: status.uid,
            state: ps.state,
            nice: ps.nice,
            unit: procfs::unescape_unit(cached.cgroup.rsplit('/').next().unwrap_or("")),
            cgroup: cached.cgroup.clone(),
            kernel: ps.kernel,
            readable,
            usage: Usage {
                cpu,
                cpu_time: ps.time as f64 / self.ticks,
                threads: ps.threads,
                wakeups,
                pss: smaps.as_ref().map(|s| s.pss),
                private: smaps.as_ref().map(|s| s.private),
                resident: status.resident,
                shared: smaps.as_ref().map(|s| s.shared),
                swap: smaps
                    .as_ref()
                    .map_or(status.swap, |s| s.swap.max(status.swap)),
                virt: ps.vsize,
                anon: status.anon,
                file: status.file,
                shmem: status.shmem,
                read_rate,
                write_rate,
                read_total: io.map(|io| io.0),
                write_total: io.map(|io| io.1),
                ..Default::default()
            },
            gpus: Vec::new(),
            group: String::new(),
        })
    }

    /// Each process's use of the graphics processors whose drivers write
    /// DRM usage statistics. A client (an open device) shared between
    /// processes counts once, for the first.
    fn drm_use(
        &mut self,
        processes: &mut [Process],
        mut clients: Vec<(i32, Client)>,
        now: Instant,
    ) {
        clients.sort_by_key(|(pid, _)| *pid);
        let mut counted = HashSet::new();
        let mut uses: HashMap<(i32, usize), GpuUse> = HashMap::new();
        let mut current = HashMap::new();
        for (pid, client) in clients {
            let key = (client.pdev.clone(), client.id);
            if !counted.insert(key.clone()) {
                continue;
            }
            let Some(device) = self.devices.iter().position(|d| d.pci == client.pdev) else {
                continue;
            };
            let (mut busy, mut video) = (0.0f64, 0.0f64);
            if let Some(before) = self.clients.get(&key) {
                let wall = now.duration_since(before.at).as_nanos() as f64;
                for (engine, &ns) in &client.engines {
                    let Some(is_video) = gpu::engine_class(engine) else {
                        continue;
                    };
                    let earlier = before.engines.get(engine).copied().unwrap_or(ns);
                    let capacity = client.capacity.get(engine).copied().unwrap_or(1).max(1) as f64;
                    let share = if wall > 0.0 {
                        (ns.saturating_sub(earlier) as f64 / wall / capacity * 100.0).min(100.0)
                    } else {
                        0.0
                    };
                    if is_video {
                        video = video.max(share);
                    } else {
                        busy = busy.max(share);
                    }
                }
                for (engine, &(cycles, total)) in &client.cycles {
                    let Some(is_video) = gpu::engine_class(engine) else {
                        continue;
                    };
                    let (earlier, earlier_total) = before
                        .cycles
                        .get(engine)
                        .copied()
                        .unwrap_or((cycles, total));
                    let span = total.saturating_sub(earlier_total);
                    let share = if span > 0 {
                        (cycles.saturating_sub(earlier) as f64 / span as f64 * 100.0).min(100.0)
                    } else {
                        0.0
                    };
                    if is_video {
                        video = video.max(share);
                    } else {
                        busy = busy.max(share);
                    }
                }
            }
            let entry = uses.entry((pid, device)).or_insert(GpuUse {
                device,
                ..Default::default()
            });
            entry.busy = (entry.busy + busy).min(100.0);
            entry.video = (entry.video + video).min(100.0);
            entry.vram += client.vram;
            entry.shared += client.shared;
            current.insert(
                key,
                ClientBefore {
                    engines: client.engines,
                    cycles: client.cycles,
                    at: now,
                },
            );
        }
        self.clients = current;
        for ((pid, _), gpu_use) in uses {
            if let Some(process) = processes.iter_mut().find(|p| p.pid == pid) {
                process.gpus.push(gpu_use);
            }
        }
    }

    /// The meters, and each process's share of what they measured.
    fn power(
        &mut self,
        processes: &mut [Process],
        gpus: &[model::Gpu],
        gpu_power: &[Option<f64>],
        cores: usize,
        dt: f64,
    ) -> model::Power {
        let mut power = model::Power::default();
        // The processor package, directly if this user may read it, else
        // through Tatami's power meter service.
        let (counters, meter) = match power::read_direct(&self.rapl) {
            Some(counters) => (Some(counters), Meter::Direct),
            None => match power::read_service() {
                Some(counters) => (Some(counters), Meter::Service),
                None => (None, Meter::None),
            },
        };
        power.meter = meter;
        let package = match (&self.rapl_before, &counters) {
            (Some(before), Some(after)) if dt > 0.0 && before.len() == after.len() => Some(
                before
                    .iter()
                    .zip(after)
                    .map(|(before, after)| power::joules(before, after))
                    .sum::<f64>()
                    / dt,
            ),
            _ => None,
        };
        if counters.is_some() {
            self.rapl_before = counters;
        }
        // Integrated graphics draw from the same package.
        let integrated: f64 = gpus
            .iter()
            .zip(gpu_power)
            .filter(|(gpu, _)| gpu.integrated)
            .filter_map(|(_, watts)| *watts)
            .sum();
        power.cpu = package.map(|watts| (watts - integrated).max(0.0));
        power.gpus = gpu_power.to_vec();
        (power.battery, power.on_battery) = power::battery();

        let mut attributed = vec![0.0; processes.len()];
        let mut measured = false;
        if let Some(cpu) = power.cpu {
            measured = true;
            let floor = self.cpu_floor.update(cpu);
            let shares: Vec<f64> = processes
                .iter()
                .map(|p| p.usage.cpu / 100.0 / cores as f64)
                .collect();
            for (part, watts) in attributed.iter_mut().zip(power::split(cpu, floor, &shares)) {
                *part += watts;
            }
        }
        for (index, watts) in gpu_power.iter().enumerate() {
            let Some(watts) = *watts else {
                continue;
            };
            measured = true;
            let floor = self.gpu_floors[index].update(watts);
            let shares: Vec<f64> = processes
                .iter()
                .map(|p| {
                    p.gpus
                        .iter()
                        .filter(|g| g.device == index)
                        .map(|g| g.busy / 100.0)
                        .sum::<f64>()
                })
                .collect();
            for (part, watts) in attributed
                .iter_mut()
                .zip(power::split(watts, floor, &shares))
            {
                *part += watts;
            }
        }
        let discrete: f64 = gpus
            .iter()
            .zip(gpu_power)
            .filter(|(gpu, _)| !gpu.integrated)
            .filter_map(|(_, watts)| *watts)
            .sum();
        power.total = if measured {
            Some(package.unwrap_or(integrated) + discrete)
        } else {
            None
        };
        for (process, watts) in processes.iter_mut().zip(attributed) {
            let entry = self.energy.entry(process.key.clone()).or_default();
            if measured {
                process.usage.power = Some(watts);
                entry.0 += watts * dt;
                entry.1 += dt;
            }
            (process.usage.energy, process.usage.measured) = *entry;
            power.attributed += process.usage.power.unwrap_or(0.0);
        }
        if let Some(total) = power.total {
            self.total_energy.0 += total * dt;
            self.total_energy.1 += dt;
        }
        (power.energy, power.measured) = self.total_energy;
        power
    }
}
