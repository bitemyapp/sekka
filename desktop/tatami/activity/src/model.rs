// SPDX-License-Identifier: MIT OR Apache-2.0
//! What one reading of the system holds, as the window shows it: each
//! process and each application it belongs to with its use of the
//! processor, memory, energy, graphics and disks, and the system as a
//! whole. The sampler (sampler.rs) makes a `Frame` every few seconds on its
//! own thread; the window only shows them. Plain data, so it can be tested
//! and sent between threads.

/// The five ways of looking at the processes, as Activity Monitor's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lens {
    Cpu,
    Memory,
    Energy,
    Gpu,
    Disk,
}

impl Lens {
    pub const ALL: [Lens; 5] = [Lens::Cpu, Lens::Memory, Lens::Energy, Lens::Gpu, Lens::Disk];

    pub fn id(self) -> &'static str {
        match self {
            Lens::Cpu => "cpu",
            Lens::Memory => "memory",
            Lens::Energy => "energy",
            Lens::Gpu => "gpu",
            Lens::Disk => "disk",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Lens::Cpu => "CPU",
            Lens::Memory => "Memory",
            Lens::Energy => "Energy",
            Lens::Gpu => "GPU",
            Lens::Disk => "Disk",
        }
    }

    pub fn from_id(id: &str) -> Option<Lens> {
        Lens::ALL.into_iter().find(|lens| lens.id() == id)
    }
}

/// What a group of processes is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// An application: it has a window, or a launcher entry.
    App,
    /// Your own background programs and services.
    Service,
    /// Programs of other users, root's services among them.
    System,
    /// The kernel's own threads.
    Kernel,
}

/// An icon to show: a themed icon's name, or an image file.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Icon {
    #[default]
    None,
    Named(String),
    File(String),
}

/// One process's use of one graphics processor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuUse {
    pub device: usize,
    /// Busiest 3D or compute engine, in percent of its time.
    pub busy: f64,
    /// Busiest video (encode or decode) engine, in percent.
    pub video: f64,
    /// Video memory in use, in bytes.
    pub vram: u64,
    /// System memory mapped for the graphics processor (GTT), in bytes.
    pub shared: u64,
}

/// The use of the system's resources by a process, or the sum of a
/// group's. Optional values could not be read (another user's process).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    /// Percent of one processor core; a busy group can pass 100.
    pub cpu: f64,
    /// Processor time since it started, in seconds.
    pub cpu_time: f64,
    pub threads: u64,
    /// Times a second it gave up the processor to wait for something:
    /// voluntary context switches, the wake-ups that keep a processor from
    /// resting.
    pub wakeups: f64,
    /// Proportional set size: private memory plus its share of memory it
    /// shares with other processes. Sums to what the processes use.
    pub pss: Option<u64>,
    /// Unique set size: memory no other process shares.
    pub private: Option<u64>,
    /// Resident set size: all memory in RAM it can reach, shared included.
    pub resident: u64,
    pub shared: Option<u64>,
    pub swap: u64,
    pub virt: u64,
    pub anon: u64,
    pub file: u64,
    pub shmem: u64,
    /// Estimated power in watts, from the meters (power.rs).
    pub power: Option<f64>,
    /// Estimated energy used while the window has been open, in joules.
    pub energy: f64,
    /// Seconds measured for `energy`, for its average.
    pub measured: f64,
    pub gpu: f64,
    pub video: f64,
    pub vram: u64,
    pub gpu_shared: u64,
    /// Storage read and written, in bytes a second and since it started.
    pub read_rate: Option<f64>,
    pub write_rate: Option<f64>,
    pub read_total: Option<u64>,
    pub write_total: Option<u64>,
}

impl Usage {
    /// The memory a process or group is held to use: its proportional
    /// size when it could be read, otherwise its resident size.
    pub fn memory(&self) -> u64 {
        self.pss.unwrap_or(self.resident)
    }

    pub fn average_power(&self) -> Option<f64> {
        (self.measured > 0.0).then(|| self.energy / self.measured)
    }

    /// Add another's use to this one's, as a group sums its processes.
    pub fn add(&mut self, other: &Usage) {
        fn sum<T: std::ops::Add<Output = T> + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a + b),
                (a, None) => a,
                (None, b) => b,
            }
        }
        self.cpu += other.cpu;
        self.cpu_time += other.cpu_time;
        self.threads += other.threads;
        self.wakeups += other.wakeups;
        self.pss = sum(self.pss, other.pss.or(Some(other.resident)));
        self.private = sum(self.private, other.private);
        self.resident += other.resident;
        self.shared = sum(self.shared, other.shared);
        self.swap += other.swap;
        self.virt += other.virt;
        self.anon += other.anon;
        self.file += other.file;
        self.shmem += other.shmem;
        self.power = sum(self.power, other.power);
        self.gpu += other.gpu;
        self.video += other.video;
        self.vram += other.vram;
        self.gpu_shared += other.gpu_shared;
        self.read_rate = sum(self.read_rate, other.read_rate);
        self.write_rate = sum(self.write_rate, other.write_rate);
        self.read_total = sum(self.read_total, other.read_total);
        self.write_total = sum(self.write_total, other.write_total);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Process {
    pub pid: i32,
    pub ppid: i32,
    /// Seconds since the epoch.
    pub started: u64,
    /// Identifies the process across readings: its PID and start.
    pub key: String,
    pub name: String,
    /// What it does for its application ("Renderer", a script's name).
    pub role: String,
    pub command: String,
    pub exe: String,
    pub cwd: String,
    pub user: String,
    pub uid: u32,
    pub state: char,
    pub nice: i32,
    /// Its systemd unit, the last part of its control group.
    pub unit: String,
    pub cgroup: String,
    pub kernel: bool,
    /// Its memory and storage figures could be read: it is yours.
    pub readable: bool,
    pub usage: Usage,
    pub gpus: Vec<GpuUse>,
    /// The group it belongs to (`Group::key`).
    pub group: String,
}

impl Process {
    pub fn state_text(&self) -> &'static str {
        match self.state {
            'R' => "Running",
            'S' => "Sleeping",
            'D' => "Waiting for the disk",
            'T' => "Paused",
            't' => "Being debugged",
            'Z' => "Ended, waiting for its parent",
            'I' => "Idle",
            'X' => "Ended",
            _ => "Unknown",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// Identifies the group across readings: its first process's key.
    pub key: String,
    pub kind: Option<Kind>,
    pub name: String,
    pub icon: Icon,
    /// A window's title, or the service's unit.
    pub detail: String,
    pub windows: usize,
    /// PIDs, the first process first.
    pub members: Vec<i32>,
    pub usage: Usage,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub free: u64,
    /// Applications' own memory (anonymous pages).
    pub apps: u64,
    /// Files kept in memory, which is given back when needed.
    pub cached: u64,
    pub buffers: u64,
    pub shmem: u64,
    /// The kernel's own memory: unreclaimable slab, stacks, page tables.
    pub kernel: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// zram: data stored, and the memory it takes compressed.
    pub compressed: Option<(u64, u64)>,
}

impl Memory {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    pub fn swap_used(&self) -> u64 {
        self.swap_total.saturating_sub(self.swap_free)
    }
}

/// Pressure stall information: the share of the last ten seconds some or
/// all tasks waited for a resource, in percent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pressure {
    pub cpu: f64,
    pub memory_some: f64,
    pub memory_full: f64,
    pub io_some: f64,
    pub io_full: f64,
}

/// How hard pressed memory is, in words, from its pressure stall figures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disk {
    pub name: String,
    pub read_rate: f64,
    pub write_rate: f64,
    /// Percent of the interval it was busy.
    pub busy: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct System {
    /// Percent of all processors, 0 to 100.
    pub cpu: f64,
    pub user: f64,
    pub kernel: f64,
    pub iowait: f64,
    /// Each processor's use, in percent.
    pub cores: Vec<f64>,
    /// Each processor's frequency, in MHz.
    pub frequencies: Vec<f64>,
    pub load: [f64; 3],
    pub processes: usize,
    pub threads: u64,
    pub uptime: f64,
    pub temperature: Option<f64>,
    pub cpu_name: String,
    pub memory: Memory,
    pub pressure: Pressure,
    pub disks: Vec<Disk>,
    pub read_total: u64,
    pub write_total: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vendor: &'static str,
    pub driver: String,
    pub pci: String,
    pub integrated: bool,
    pub busy: Option<f64>,
    pub video: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub power: Option<f64>,
    pub temperature: Option<f64>,
    /// Graphics clock, in MHz.
    pub clock: Option<f64>,
}

/// Where the processor's power reading comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Meter {
    /// Read from the kernel's RAPL counters directly.
    Direct,
    /// Through Tatami's power meter service.
    Service,
    #[default]
    None,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Power {
    pub meter: Meter,
    /// The processor package, graphics excluded when they share it.
    pub cpu: Option<f64>,
    /// Each graphics processor's own meter, as `Frame::gpus`.
    pub gpus: Vec<Option<f64>>,
    /// The battery, while it discharges.
    pub battery: Option<f64>,
    pub on_battery: bool,
    /// What the meters measure together.
    pub total: Option<f64>,
    /// The part of `total` the processes are held to use.
    pub attributed: f64,
    /// Energy measured since the window opened, in joules.
    pub energy: f64,
    pub measured: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub system: System,
    pub gpus: Vec<Gpu>,
    pub power: Power,
    pub processes: Vec<Process>,
    pub groups: Vec<Group>,
    /// Your user ID: other users' processes can't be quit from here.
    pub uid: u32,
}

/// Sizes in binary units: "512 KiB", "3.4 GiB".
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if value < 1024 {
        return format!("{value} B");
    }
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if size >= 99.95 {
        format!("{size:.0} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

pub fn rate(value: f64) -> String {
    format!("{}/s", bytes(value.max(0.0).round() as u64))
}

/// A percentage as Activity Monitor shows it: "12.3", "0.0".
pub fn percent(value: f64) -> String {
    if value >= 999.5 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

pub fn watts(value: f64) -> String {
    let value = value.max(0.0);
    if value >= 99.5 {
        format!("{value:.0} W")
    } else if value >= 1.0 {
        format!("{value:.1} W")
    } else if value >= 0.0005 {
        format!("{:.0} mW", value * 1000.0)
    } else {
        "0 mW".into()
    }
}

/// Energy in watt-hours, from joules.
pub fn energy(joules: f64) -> String {
    let wh = joules.max(0.0) / 3600.0;
    if wh >= 99.95 {
        format!("{wh:.0} Wh")
    } else if wh >= 1.0 {
        format!("{wh:.1} Wh")
    } else if wh >= 0.0005 {
        format!("{:.0} mWh", wh * 1000.0)
    } else {
        "0 mWh".into()
    }
}

/// Processor time: "0:04.21", "12:00.50", "3:02:01".
pub fn cpu_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let whole = seconds as u64;
    let (hours, minutes, secs) = (whole / 3600, whole / 60 % 60, whole % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        let hundredths = ((seconds - whole as f64) * 100.0).floor() as u64;
        format!("{minutes}:{secs:02}.{hundredths:02}")
    }
}

/// How long ago, in words: "just now", "5 minutes ago", "3 days ago".
pub fn ago(seconds: u64) -> String {
    let plural = |n: u64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    match seconds {
        0..60 => "just now".into(),
        60..3600 => plural(seconds / 60, "minute"),
        3600..86400 => plural(seconds / 3600, "hour"),
        _ => plural(seconds / 86400, "day"),
    }
}

pub fn temperature(celsius: f64) -> String {
    format!("{celsius:.0} °C")
}

pub fn frequency(mhz: f64) -> String {
    if mhz >= 1000.0 {
        format!("{:.2} GHz", mhz / 1000.0)
    } else {
        format!("{mhz:.0} MHz")
    }
}

pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Thousands separated with thin spaces: "12 345".
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024 + 400 * 1024 * 1024), "3.4 GiB");
        assert_eq!(bytes(200 * 1024 * 1024), "200 MiB");
        assert_eq!(rate(2048.0), "2.0 KiB/s");
        assert_eq!(rate(-5.0), "0 B/s");
    }

    #[test]
    fn power_and_energy_read_naturally() {
        assert_eq!(watts(0.0), "0 mW");
        assert_eq!(watts(0.063), "63 mW");
        assert_eq!(watts(3.44), "3.4 W");
        assert_eq!(watts(145.2), "145 W");
        assert_eq!(energy(3600.0), "1.0 Wh");
        assert_eq!(energy(36.0), "10 mWh");
        assert_eq!(energy(0.0), "0 mWh");
    }

    #[test]
    fn processor_time_and_ages() {
        assert_eq!(cpu_time(4.215), "0:04.21");
        assert_eq!(cpu_time(720.5), "12:00.50");
        assert_eq!(cpu_time(10921.0), "3:02:01");
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(7300), "2 hours ago");
        assert_eq!(ago(3 * 86400), "3 days ago");
        assert_eq!(percent(12.345), "12.3");
        assert_eq!(percent(1234.0), "1234");
        assert_eq!(frequency(4712.0), "4.71 GHz");
        assert_eq!(grouped(1234567), "1\u{2009}234\u{2009}567");
        assert_eq!(grouped(999), "999");
    }

    #[test]
    fn groups_sum_what_could_be_read() {
        let mut total = Usage::default();
        total.add(&Usage {
            cpu: 50.0,
            pss: Some(100),
            resident: 300,
            private: Some(80),
            power: Some(2.0),
            ..Default::default()
        });
        // Another user's process: its resident size stands in for its
        // proportional one.
        total.add(&Usage {
            cpu: 25.0,
            resident: 200,
            ..Default::default()
        });
        assert_eq!(total.cpu, 75.0);
        assert_eq!(total.pss, Some(300));
        assert_eq!(total.private, Some(80));
        assert_eq!(total.resident, 500);
        assert_eq!(total.power, Some(2.0));
        assert_eq!(total.memory(), 300);
        assert_eq!(Usage::default().average_power(), None);
    }

    #[test]
    fn memory_strain_from_pressure() {
        let calm = Pressure::default();
        assert_eq!(calm.memory_strain(), Strain::Low);
        let busy = Pressure {
            memory_some: 8.0,
            ..Default::default()
        };
        assert_eq!(busy.memory_strain(), Strain::Moderate);
        let thrashing = Pressure {
            memory_full: 12.0,
            ..Default::default()
        };
        assert_eq!(thrashing.memory_strain(), Strain::High);
    }
}
