//! What one reading of the system holds, as the window shows it: each
//! process and each application it belongs to, with its use of the
//! processor, memory, energy, graphics and storage, and the system as a
//! whole. The sampler makes a [`Frame`] every second on a thread of its
//! own; the window only shows them. Plain data.
//!
//! Ported from Tatami's Activity (Sekka, MIT OR Apache-2.0, the same
//! author).

/// The ways of looking at the processes, as Activity Monitor's tabs.
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
        Lens::ALL.into_iter().find(|l| l.id() == id)
    }

    pub fn index(self) -> usize {
        Lens::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }
}

/// What a group of processes is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// An application: it has a window, or a launcher entry.
    App,
    /// Your own background programs and services.
    Service,
    /// Other users' programs, root's services among them.
    System,
    /// The kernel's own threads.
    Kernel,
}

impl Kind {
    pub fn words(self) -> &'static str {
        match self {
            Kind::App => "Application",
            Kind::Service => "Your background program",
            Kind::System => "System",
            Kind::Kernel => "Kernel",
        }
    }
}

/// One process's use of one graphics card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuUse {
    pub device: usize,
    pub busy: f64,
    pub video: f64,
    pub vram: u64,
    pub shared: u64,
}

/// The use of the system's resources by a process, or the sum of a
/// group's. Optional values couldn't be read (another user's process) or
/// weren't read this time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    /// Percent of one core; a busy group can pass 100.
    pub cpu: f64,
    /// Processor time since it started, in seconds.
    pub cpu_time: f64,
    pub threads: u64,
    /// Voluntary context switches a second: the wake-ups that keep a
    /// processor from resting.
    pub wakeups: f64,
    /// Proportional set size: private memory plus its share of what it
    /// shares. Sums to what processes use.
    pub pss: Option<u64>,
    /// Memory no other process shares.
    pub private: Option<u64>,
    /// All memory in RAM it can reach, shared included.
    pub resident: u64,
    pub shared: Option<u64>,
    pub swap: u64,
    pub virt: u64,
    /// Estimated power, in watts, from the meters.
    pub power: Option<f64>,
    /// Estimated energy used while Myaku has been open, in joules, and the
    /// seconds measured.
    pub energy: f64,
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
    /// The memory a process or group is held to use: its proportional size
    /// when known, otherwise its resident size.
    pub fn memory(&self) -> u64 {
        self.pss.unwrap_or(self.resident)
    }

    pub fn average_power(&self) -> Option<f64> {
        (self.measured > 0.0).then(|| self.energy / self.measured)
    }

    /// Add another's use, as a group sums its processes.
    pub fn add(&mut self, o: &Usage) {
        fn sum<T: std::ops::Add<Output = T> + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a + b),
                (a, None) => a,
                (None, b) => b,
            }
        }
        self.cpu += o.cpu;
        self.cpu_time += o.cpu_time;
        self.threads += o.threads;
        self.wakeups += o.wakeups;
        self.pss = sum(self.pss, o.pss.or(Some(o.resident)));
        self.private = sum(self.private, o.private);
        self.resident += o.resident;
        self.shared = sum(self.shared, o.shared);
        self.swap += o.swap;
        self.virt += o.virt;
        self.power = sum(self.power, o.power);
        self.energy += o.energy;
        self.measured = self.measured.max(o.measured);
        self.gpu += o.gpu;
        self.video += o.video;
        self.vram += o.vram;
        self.gpu_shared += o.gpu_shared;
        self.read_rate = sum(self.read_rate, o.read_rate);
        self.write_rate = sum(self.write_rate, o.write_rate);
        self.read_total = sum(self.read_total, o.read_total);
        self.write_total = sum(self.write_total, o.write_total);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    /// Clock ticks after boot: with the pid, it names the process across
    /// readings (and keeps a signal from a reused pid).
    pub start_ticks: u64,
    /// Seconds since the epoch.
    pub started: u64,
    pub name: String,
    /// What it does for its application ("Renderer", a script's name).
    pub role: String,
    pub command: String,
    pub exe: String,
    pub user: String,
    pub uid: u32,
    pub state: char,
    pub nice: i32,
    pub unit: String,
    pub cgroup: String,
    pub kernel: bool,
    /// Its memory maps and storage counters could be read: it is yours.
    pub readable: bool,
    pub usage: Usage,
    pub gpus: Vec<GpuUse>,
    /// Its group's index in [`Frame::groups`].
    pub group: usize,
}

impl Process {
    pub fn key(&self) -> (u32, u64) {
        (self.pid, self.start_ticks)
    }

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

    pub fn paused(&self) -> bool {
        self.state == 'T'
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// Names the group across readings: its first process's key.
    pub key: (u32, u64),
    pub kind: Option<Kind>,
    pub name: String,
    /// The launcher entry's id, for its icon and to launch it again.
    pub app_id: Option<String>,
    /// A window's title, or the service's unit.
    pub detail: String,
    pub windows: usize,
    pub focused: bool,
    /// Indices into [`Frame::processes`], the first process first.
    pub members: Vec<usize>,
    pub usage: Usage,
    /// Every member is yours (so it can be quit from here).
    pub own: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub free: u64,
    pub cached: u64,
    pub buffers: u64,
    pub shared: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

impl Memory {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disk {
    pub name: String,
    pub read_rate: f64,
    pub write_rate: f64,
    pub busy: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct System {
    /// Percent of all processors, 0 to 100.
    pub cpu: f64,
    pub user: f64,
    pub kernel: f64,
    pub iowait: f64,
    /// Each processor's use, in percent, and frequency in MHz.
    pub cores: Vec<f64>,
    pub frequencies: Vec<f64>,
    pub load: [f64; 3],
    pub processes: usize,
    pub threads: u64,
    pub uptime: f64,
    pub temperature: Option<f64>,
    pub cpu_name: String,
    pub memory: Memory,
    pub pressure: Option<aizome_sys::sysinfo::detail::Pressure>,
    pub disks: Vec<Disk>,
    pub disk_read: f64,
    pub disk_write: f64,
    pub net_rx: f64,
    pub net_tx: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub driver: String,
    pub pci: String,
    pub integrated: bool,
    pub busy: Option<f64>,
    pub video: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub power: Option<f64>,
    pub temperature: Option<f64>,
    pub clock: Option<f64>,
}

/// Where the power figures come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Meter {
    /// The processor's RAPL counters (readable: Myaku runs as root, or the
    /// counters were opened up).
    Rapl,
    /// The battery, while it discharges.
    Battery,
    /// Nothing measures power here.
    #[default]
    None,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Power {
    pub meter: Meter,
    pub cpu: Option<f64>,
    pub battery: Option<f64>,
    pub on_battery: bool,
    /// What the meters measure together, and the part processes are held
    /// to use.
    pub total: Option<f64>,
    pub attributed: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// Which reading this is (from 1).
    pub tick: u64,
    pub system: System,
    pub gpus: Vec<Gpu>,
    pub power: Power,
    pub processes: Vec<Process>,
    pub groups: Vec<Group>,
    /// Your user ID.
    pub uid: u32,
    /// How long the reading took on the sampler's thread.
    pub cost: std::time::Duration,
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
    if size >= 99.95 { format!("{size:.0} {}", UNITS[unit]) } else { format!("{size:.1} {}", UNITS[unit]) }
}

pub fn rate(value: f64) -> String {
    format!("{}/s", bytes(value.max(0.0).round() as u64))
}

/// A percentage as Activity Monitor shows it: "12.3", "0.0".
pub fn percent(value: f64) -> String {
    if value >= 999.5 { format!("{value:.0}") } else { format!("{value:.1}") }
}

pub fn watts(value: f64) -> String {
    let v = value.max(0.0);
    if v >= 99.5 {
        format!("{v:.0} W")
    } else if v >= 1.0 {
        format!("{v:.1} W")
    } else if v >= 0.0005 {
        format!("{:.0} mW", v * 1000.0)
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
    let s = seconds.max(0.0);
    let whole = s as u64;
    let (h, m, sec) = (whole / 3600, whole / 60 % 60, whole % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}.{:02}", ((s - whole as f64) * 100.0).floor() as u64)
    }
}

/// How long ago, in words.
pub fn ago(seconds: u64) -> String {
    let plural = |n: u64, unit: &str| if n == 1 { format!("1 {unit} ago") } else { format!("{n} {unit}s ago") };
    match seconds {
        0..60 => "just now".into(),
        60..3600 => plural(seconds / 60, "minute"),
        3600..86400 => plural(seconds / 3600, "hour"),
        _ => plural(seconds / 86400, "day"),
    }
}

/// An uptime: "3 days, 4:05", "2:15".
pub fn uptime(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    match d {
        0 => format!("{h}:{m:02}"),
        1 => format!("1 day, {h}:{m:02}"),
        _ => format!("{d} days, {h}:{m:02}"),
    }
}

pub fn frequency(mhz: f64) -> String {
    if mhz >= 1000.0 { format!("{:.2} GHz", mhz / 1000.0) } else { format!("{mhz:.0} MHz") }
}

pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
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
    fn sizes_rates_and_times() {
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(200 << 20), "200 MiB");
        assert_eq!(rate(-5.0), "0 B/s");
        assert_eq!(watts(0.063), "63 mW");
        assert_eq!(energy(36.0), "10 mWh");
        assert_eq!(cpu_time(4.215), "0:04.21");
        assert_eq!(cpu_time(10921.0), "3:02:01");
        assert_eq!(ago(7300), "2 hours ago");
        assert_eq!(uptime(3.0 * 86400.0 + 4.0 * 3600.0 + 300.0), "3 days, 4:05");
        assert_eq!(percent(1234.0), "1234");
        assert_eq!(grouped(1234567), "1\u{2009}234\u{2009}567");
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
        total.add(&Usage { cpu: 25.0, resident: 200, ..Default::default() });
        assert_eq!(
            (total.cpu, total.pss, total.private, total.resident, total.power),
            (75.0, Some(300), Some(80), 500, Some(2.0))
        );
        assert_eq!(total.memory(), 300);
    }
}
