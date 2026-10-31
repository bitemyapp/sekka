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

/// Graphics measurements. An unavailable counter stays unknown when summed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuUsage {
    pub busy: Option<f64>,
    pub video: Option<f64>,
    pub vram: Option<u64>,
    pub shared: Option<u64>,
}

impl Default for GpuUsage {
    fn default() -> Self {
        Self { busy: Some(0.0), video: Some(0.0), vram: Some(0), shared: Some(0) }
    }
}

impl GpuUsage {
    pub const UNKNOWN: Self = Self { busy: None, video: None, vram: None, shared: None };

    /// Add another's use of the same card, as an application sums its
    /// processes.
    pub fn add(&mut self, other: &Self) {
        self.busy = self.busy.zip(other.busy).map(|(a, b)| a + b);
        self.video = self.video.zip(other.video).map(|(a, b)| a + b);
        self.vram = self.vram.zip(other.vram).map(|(a, b)| a + b);
        self.shared = self.shared.zip(other.shared).map(|(a, b)| a + b);
    }

    /// Add the use of another card. Busy figures are shares of each card's
    /// own engines, so they don't add: the busiest card stands for them, as
    /// % GPU says. Memory adds, but shared memory only from a card that
    /// reports it (`shared`).
    fn beside(&mut self, other: &Self, shared: bool) {
        let busiest = |a: Option<f64>, b: Option<f64>| a.zip(b).map(|(a, b)| a.max(b));
        self.busy = busiest(self.busy, other.busy);
        self.video = busiest(self.video, other.video);
        self.vram = self.vram.zip(other.vram).map(|(a, b)| a + b);
        if shared {
            self.shared = self.shared.zip(other.shared).map(|(a, b)| a + b);
        }
    }
}

/// What `members` use of one card (`Some(device)`), or of every card
/// together: on one card, their use adds up; across cards, as
/// [`GpuUsage::beside`] has it.
pub fn graphics_of<'a>(
    members: impl Iterator<Item = &'a Process> + Clone,
    gpus: &[Gpu],
    device: Option<usize>,
) -> GpuUsage {
    let on = |d: usize| {
        let mut u = GpuUsage::default();
        for p in members.clone() {
            u.add(&p.graphics_on(d, &gpus[d]));
        }
        u
    };
    match device {
        Some(d) => on(d),
        None => {
            let mut all = GpuUsage::default();
            for (d, gpu) in gpus.iter().enumerate() {
                all.beside(&on(d), gpu.reports_shared());
            }
            all
        }
    }
}

/// One process's use of one graphics card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuUse {
    pub device: usize,
    pub usage: GpuUsage,
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
    pub graphics: GpuUsage,
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
        self.graphics.add(&o.graphics);
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
    /// Absence from a successfully read process list means zero; absence from
    /// an unavailable list does not establish that the process uses no GPU.
    pub fn graphics_on(&self, device: usize, gpu: &Gpu) -> GpuUsage {
        let mut uses = self.gpus.iter().filter(|g| g.device == device);
        let Some(first) = uses.next() else { return gpu.unlisted_usage };
        let mut usage = first.usage;
        for other in uses {
            usage.add(&other.usage);
        }
        usage
    }

    /// Whether it is one of the card's clients, busy or not.
    pub fn uses(&self, device: usize) -> bool {
        self.gpus.iter().any(|g| g.device == device)
    }

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
    /// A plain-language explanation of how the sampler determined membership.
    /// This describes the group, independently of each process's parent.
    pub attribution: String,
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
    /// Whether its power meter is the processor package's: an AMD APU's
    /// Radeon, not a Ryzen desktop processor's, which meters itself alone
    /// (`aizome_sys::sysinfo::power::meters_package`).
    pub package_meter: bool,
    pub busy: Option<f64>,
    pub video: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub power: Option<f64>,
    pub temperature: Option<f64>,
    pub clock: Option<f64>,
    /// What can be inferred for a process absent from this card's client lists.
    pub unlisted_usage: GpuUsage,
}

impl Gpu {
    /// Whether it reports the system memory mapped for it: the DRM drivers
    /// do; NVIDIA's NVML has no such figure.
    pub fn reports_shared(&self) -> bool {
        self.driver != "nvidia"
    }

    /// Whether its clients' figures can't be read: absent from its lists
    /// says nothing.
    pub fn clients_unknown(&self) -> bool {
        self.unlisted_usage.busy.is_none() && self.unlisted_usage.vram.is_none()
    }
}

/// A card's name without its maker or codename, as labels show it: "AMD
/// Radeon Graphics (Granite Ridge)" is "Radeon Graphics", "NVIDIA GeForce
/// RTX 5090" "GeForce RTX 5090". One that would say only "Graphics" keeps
/// its maker.
pub fn short_name(name: &str) -> String {
    let mut name = name.trim();
    if name.ends_with(')')
        && let Some(i) = name.rfind(" (")
    {
        name = &name[..i];
    }
    for maker in ["NVIDIA ", "AMD ", "Intel "] {
        if let Some(rest) = name.strip_prefix(maker)
            && !rest.to_lowercase().starts_with("graphics")
        {
            name = rest;
        }
    }
    name.to_owned()
}

/// The cards' labels, by index: short names, and where two are alike, each
/// with its bus ("GeForce RTX 4090 · 41:00").
pub fn gpu_names(gpus: &[Gpu]) -> Vec<String> {
    let short: Vec<String> = gpus.iter().map(|g| short_name(&g.name)).collect();
    short
        .iter()
        .zip(gpus)
        .map(|(name, g)| {
            if short.iter().filter(|n| *n == name).count() > 1 {
                let bus = g.pci.strip_prefix("0000:").unwrap_or(&g.pci);
                format!("{name} · {}", bus.strip_suffix(".0").unwrap_or(bus))
            } else {
                name.clone()
            }
        })
        .collect()
}

/// The cards in the order the window lists them: discrete ones first, as
/// they do the heavy work, then as the kernel numbers them.
pub fn gpu_order(gpus: &[Gpu]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..gpus.len()).collect();
    order.sort_by_key(|&i| (gpus[i].integrated, i));
    order
}

/// Where the power figures come from. The discrete graphics cards' own
/// meters add to any of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Meter {
    /// The processor's RAPL counters: read directly where Myaku may (as
    /// root), else through Aizome's power meter service.
    Rapl,
    /// The processor package's meter as its integrated graphics report it:
    /// an AMD APU's covers the processor too, and anyone may read it.
    Package,
    /// The battery, while it discharges.
    Battery,
    /// The graphics cards' meters alone: what uses no card isn't measured.
    Graphics,
    /// Nothing measures power here.
    #[default]
    None,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Power {
    pub meter: Meter,
    /// The processor package, from RAPL or the integrated graphics' meter.
    pub cpu: Option<f64>,
    /// The discrete graphics cards' meters together.
    pub graphics: Option<f64>,
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

impl Frame {
    /// Each process's and application's use of the cards together, from its
    /// use of each.
    pub fn total_graphics(&mut self) {
        let Frame { gpus, processes, groups, .. } = self;
        for p in processes.iter_mut() {
            p.usage.graphics = graphics_of(std::iter::once(&*p), gpus, None);
        }
        for g in groups.iter_mut() {
            g.usage.graphics = graphics_of(g.members.iter().map(|&m| &processes[m]), gpus, None);
        }
    }

    /// The reading as one card sees it: each process's and application's
    /// graphics are its use of that card alone. The rest is as read, so
    /// selection, grouping and sorting keep the same rows.
    pub fn for_gpu(&self, device: usize) -> Frame {
        let mut frame = self.clone();
        let Frame { gpus, processes, groups, .. } = &mut frame;
        for p in processes.iter_mut() {
            p.usage.graphics = p.graphics_on(device, &gpus[device]);
        }
        for g in groups.iter_mut() {
            g.usage.graphics = graphics_of(g.members.iter().map(|&m| &processes[m]), gpus, Some(device));
        }
        frame
    }
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

    #[test]
    fn cards_get_short_distinct_names() {
        assert_eq!(short_name("AMD Radeon Graphics (Granite Ridge)"), "Radeon Graphics");
        assert_eq!(short_name("NVIDIA GeForce RTX 5090"), "GeForce RTX 5090");
        assert_eq!(short_name("Intel Graphics (Panther Lake)"), "Intel Graphics");
        assert_eq!(short_name("Intel graphics (8086:a7a0)"), "Intel graphics");
        let twin = |pci: &str| Gpu { name: "NVIDIA GeForce RTX 4090".into(), pci: pci.into(), ..Default::default() };
        let igpu = Gpu { name: "AMD Radeon Graphics".into(), integrated: true, ..Default::default() };
        let gpus = [igpu, twin("0000:01:00.0"), twin("0000:41:00.0")];
        assert_eq!(gpu_names(&gpus), ["Radeon Graphics", "GeForce RTX 4090 · 01:00", "GeForce RTX 4090 · 41:00"]);
        assert_eq!(gpu_order(&gpus), [1, 2, 0], "discrete cards first");
    }

    #[test]
    fn cards_busy_figures_are_not_added_together() {
        let amd = Gpu { driver: "amdgpu".into(), ..Default::default() };
        let nvidia = Gpu {
            driver: "nvidia".into(),
            unlisted_usage: GpuUsage { shared: None, ..Default::default() },
            ..Default::default()
        };
        let on = |device, busy, mib: u64, shared| GpuUse {
            device,
            usage: GpuUsage { busy: Some(busy), vram: Some(mib << 20), shared, ..Default::default() },
        };
        let renderer =
            Process { gpus: vec![on(0, 18.0, 6, Some(4 << 20)), on(1, 2.0, 876, None)], ..Default::default() };
        let helper = Process { gpus: vec![on(1, 5.0, 10, None)], ..Default::default() };
        let gpus = [amd, nvidia];
        let both = [renderer, helper];
        let all = graphics_of(both.iter(), &gpus, None);
        // The busiest card: the helper and renderer share the NVIDIA card's 7%.
        assert_eq!(all.busy, Some(18.0));
        assert_eq!(all.vram, Some(892 << 20));
        assert_eq!(all.shared, Some(4 << 20), "NVML reports no shared memory, which leaves the AMD figure");
        assert_eq!(graphics_of(both.iter(), &gpus, Some(1)).busy, Some(7.0));
    }
}
