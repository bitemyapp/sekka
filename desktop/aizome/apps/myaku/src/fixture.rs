//! Readings made up for tests and pictures: a desktop's applications,
//! services and system processes, as many as asked for, their figures
//! moving from one reading to the next. Deterministic.

use std::time::Duration;

use aizome_sys::sysinfo::power::Floor;

use crate::model::{Frame, Gpu, GpuUsage, GpuUse, Group, Kind, Memory, Meter, Power, Process, System, Usage};

/// A small, steady generator of numbers in `[0, 1)`.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

const APPS: [(&str, &str, &str, usize, f64, u64); 10] = [
    ("Firefox", "firefox", "org.mozilla.firefox", 14, 18.0, 2_300_000_000),
    ("Sumi", "aizome-sumi", "org.aizome.Sumi", 1, 0.4, 82_000_000),
    ("foot", "foot", "foot", 3, 1.2, 46_000_000),
    ("Kura", "aizome-kura", "org.aizome.Kura", 1, 0.1, 64_000_000),
    ("Signal", "signal-desktop", "signal", 6, 2.5, 610_000_000),
    ("Visual Studio Code", "code", "code", 11, 7.5, 1_400_000_000),
    ("Tansu", "aizome-tansu", "org.aizome.Tansu", 1, 0.0, 38_000_000),
    ("Spotify", "spotify", "spotify", 5, 4.2, 520_000_000),
    ("Settei", "aizome-settei", "org.aizome.Settei", 1, 0.0, 54_000_000),
    ("Myaku", "aizome-myaku", "org.aizome.Myaku", 1, 0.9, 24_000_000),
];

const SERVICES: [&str; 12] = [
    "pipewire",
    "wireplumber",
    "aizome-shell",
    "xdg-desktop-portal",
    "dbus-broker",
    "gvfsd",
    "syncthing",
    "ssh-agent",
    "mako",
    "at-spi-bus-launcher",
    "gnome-keyring-daemon",
    "xwayland-satellite",
];

const SYSTEM: [&str; 10] = [
    "systemd",
    "systemd-journald",
    "systemd-udevd",
    "NetworkManager",
    "udisksd",
    "upowerd",
    "polkitd",
    "nix-daemon",
    "sshd",
    "cupsd",
];

/// A reading with about `processes` processes, at reading `tick`.
pub fn frame(processes: usize, tick: u64) -> Frame {
    let mut noise = Noise(0x5eed ^ tick.wrapping_mul(0x9e3779b97f4a7c15));
    let mut f = Frame { tick, uid: 1000, cost: Duration::from_millis(9), ..Default::default() };
    let mut procs: Vec<Process> = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut pid = 1u32;
    let mut add = |procs: &mut Vec<Process>,
                   name: &str,
                   role: &str,
                   uid: u32,
                   cpu: f64,
                   mem: u64,
                   noise: &mut Noise,
                   kernel: bool|
     -> usize {
        pid += 1;
        let jitter = 0.6 + noise.next() * 0.8;
        let own = uid == 1000;
        let usage = Usage {
            cpu: cpu * jitter,
            cpu_time: f64::from(pid) * 3.7 + tick as f64 * cpu / 100.0,
            threads: 1 + u64::from(pid % 23),
            wakeups: (cpu * 12.0 * jitter).round(),
            pss: own.then_some((mem as f64 * 0.8) as u64),
            private: own.then_some((mem as f64 * 0.6) as u64),
            resident: mem,
            shared: own.then_some((mem as f64 * 0.3) as u64),
            swap: if pid.is_multiple_of(7) { 12 << 20 } else { 0 },
            virt: mem * 6,
            read_rate: own.then_some((noise.next() * 200_000.0 * (cpu / 10.0)).floor()),
            write_rate: own.then_some((noise.next() * 80_000.0 * (cpu / 10.0)).floor()),
            read_total: own.then_some(u64::from(pid) * 9_100_000),
            write_total: own.then_some(u64::from(pid) * 3_700_000),
            graphics: GpuUsage {
                busy: Some(if name == "firefox" && role == "GPU process" { 8.0 * jitter } else { 0.0 }),
                vram: Some(if role == "GPU process" { 180 << 20 } else { 0 }),
                ..Default::default()
            },
            ..Default::default()
        };
        procs.push(Process {
            pid,
            ppid: 1,
            start_ticks: u64::from(pid) * 101,
            started: 1_792_000_000 + u64::from(pid) * 37,
            name: name.into(),
            role: role.into(),
            command: if kernel { String::new() } else { format!("/run/current-system/sw/bin/{name} --flag {pid}") },
            exe: if kernel { String::new() } else { format!("/nix/store/0000-{name}/bin/{name}") },
            user: if uid == 0 { "root".into() } else { "callen".into() },
            uid,
            state: if cpu > 5.0 { 'R' } else { 'S' },
            nice: 0,
            unit: String::new(),
            cgroup: String::new(),
            kernel,
            readable: own,
            gpus: if role == "GPU process" { vec![GpuUse { device: 0, usage: usage.graphics }] } else { Vec::new() },
            usage,
            group: 0,
        });
        procs.len() - 1
    };
    let push_group = |groups: &mut Vec<Group>,
                      procs: &mut [Process],
                      members: Vec<usize>,
                      name: &str,
                      kind: Kind,
                      app: Option<&str>,
                      detail: &str| {
        let mut usage = Usage::default();
        for &m in &members {
            usage.add(&procs[m].usage);
            procs[m].group = groups.len();
        }
        groups.push(Group {
            key: procs[members[0]].key(),
            kind: Some(kind),
            name: name.into(),
            app_id: app.map(str::to_owned),
            detail: detail.into(),
            attribution: match kind {
                Kind::App => format!(
                    "The {name} process tree includes its helpers and commands. Members need not be direct children."
                ),
                Kind::Service => "This background program is shown separately from applications.".into(),
                Kind::System => "Processes belonging to this system service are grouped together.".into(),
                Kind::Kernel => "Kernel threads are grouped together separately from user programs.".into(),
            },
            windows: usize::from(kind == Kind::App),
            focused: name == "Sumi",
            own: kind != Kind::System && kind != Kind::Kernel,
            members,
            usage,
        });
    };
    for (name, prog, id, n, cpu, mem) in APPS {
        let mut members = Vec::new();
        for k in 0..n {
            let role = match (prog, k) {
                (_, 0) => "",
                ("firefox", 1) => "GPU process",
                ("firefox" | "code" | "signal-desktop" | "spotify", _) => "Renderer",
                _ => "",
            };
            members.push(add(&mut procs, prog, role, 1000, cpu / n as f64, mem / n as u64, &mut noise, false));
        }
        let detail = match name {
            "Firefox" => "Aizome — Sekka's desktop",
            "Sumi" => "architecture.md",
            "foot" => "~/work/aizome",
            _ => "",
        };
        push_group(&mut groups, &mut procs, members, name, Kind::App, Some(id), detail);
    }
    let mut s = 0;
    while procs.len() + 40 < processes {
        let name = SERVICES[s % SERVICES.len()];
        let m = add(&mut procs, name, "", 1000, 0.3, 9_000_000 + (s as u64 % 9) * 3_000_000, &mut noise, false);
        push_group(
            &mut groups,
            &mut procs,
            vec![m],
            name,
            Kind::Service,
            None,
            if s < SERVICES.len() { "" } else { "user service" },
        );
        s += 1;
        if procs.len() + 40 >= processes {
            break;
        }
        let name = SYSTEM[s % SYSTEM.len()];
        let m = add(&mut procs, name, "", 0, 0.2, 7_000_000, &mut noise, false);
        push_group(&mut groups, &mut procs, vec![m], name, Kind::System, None, &format!("{name}.service"));
    }
    let mut kernel = Vec::new();
    while procs.len() < processes.max(procs.len() + 1) && kernel.len() < 40 {
        let m = add(&mut procs, &format!("kworker/{}:1", kernel.len()), "", 0, 0.05, 0, &mut noise, true);
        kernel.push(m);
    }
    push_group(&mut groups, &mut procs, kernel, "Kernel", Kind::Kernel, None, "The kernel's own threads");

    let mut wave = |phase: f64| ((tick as f64 * 0.21 + phase).sin() * 0.5 + 0.5) * noise.next().mul_add(0.3, 0.7);
    let user = 6.0 + 28.0 * wave(0.0);
    let kernel_cpu = 2.0 + 7.0 * wave(1.3);
    let total: u64 = 32 << 30;
    let used = (11.5 * (1u64 << 30) as f64 * (0.95 + wave(2.0) * 0.1)) as u64;
    f.system = System {
        cpu: user + kernel_cpu,
        user,
        kernel: kernel_cpu,
        iowait: 0.4,
        cores: (0..16).map(|c| (user + kernel_cpu) * (0.4 + (c as f64 * 0.37).sin().abs())).collect(),
        frequencies: (0..16).map(|c| 2400.0 + f64::from(c) * 41.0).collect(),
        load: [1.42, 1.18, 0.97],
        processes: procs.len(),
        threads: procs.iter().map(|p| p.usage.threads).sum(),
        uptime: 3.0 * 86400.0 + 4.0 * 3600.0 + 300.0,
        temperature: Some(52.0 + 6.0 * wave(0.5)),
        cpu_name: "Intel Core Ultra 7 155H".into(),
        memory: Memory {
            total,
            available: total - used,
            free: 4 << 30,
            cached: 9 << 30,
            buffers: 300 << 20,
            shared: 900 << 20,
            swap_total: 31 << 30,
            swap_used: 420 << 20,
        },
        pressure: Some(aizome_sys::sysinfo::detail::Pressure {
            cpu_some: 2.1,
            memory_some: 0.4,
            memory_full: 0.0,
            io_some: 0.6,
            io_full: 0.1,
        }),
        disks: Vec::new(),
        disk_read: 18_000_000.0 * wave(3.0).powi(3),
        disk_write: 6_000_000.0 * wave(4.1).powi(2),
        net_rx: 2_400_000.0 * wave(0.9).powi(2),
        net_tx: 300_000.0 * wave(2.2),
    };
    f.gpus = vec![Gpu {
        name: "Intel Arc Graphics (Meteor Lake)".into(),
        driver: "xe".into(),
        pci: "0000:00:02.0".into(),
        integrated: true,
        busy: Some(9.0 * wave(1.0)),
        ..Default::default()
    }];
    f.power = Power { meter: Meter::None, ..Default::default() };
    f.processes = procs;
    f.groups = groups;
    f.total_graphics();
    f
}

/// A reading of a desktop with two graphics cards: the processor's
/// integrated Radeon, and an RTX 5090 that draws the screen and runs a
/// language model in bursts, so that most of the graphics work and memory
/// is the NVIDIA card's. Its clients are listed by NVML, which reports no
/// shared memory.
pub fn workstation(processes: usize, tick: u64) -> Frame {
    let mut f = frame(processes, tick);
    let t = tick as f64;
    let wave = |phase: f64, speed: f64| (t * speed + phase).sin() * 0.5 + 0.5;
    // The model answers now and then: the card's busy time comes in bursts.
    let infer = wave(0.0, 0.17).powi(4);
    f.gpus = vec![
        Gpu {
            name: "AMD Radeon Graphics (Granite Ridge)".into(),
            driver: "amdgpu".into(),
            pci: "0000:78:00.0".into(),
            integrated: true,
            busy: Some((2.0 + 4.0 * wave(1.0, 0.31)).round()),
            video: Some(0.0),
            vram_used: Some(22 << 20),
            vram_total: Some(2 << 30),
            power: Some(9.2),
            temperature: Some(41.0),
            ..Default::default()
        },
        Gpu {
            name: "NVIDIA GeForce RTX 5090".into(),
            driver: "nvidia".into(),
            pci: "0000:01:00.0".into(),
            busy: Some((4.0 + 88.0 * infer).round()),
            video: Some(0.0),
            vram_used: Some(((11.9 + 1.4 * infer) * (1u64 << 30) as f64) as u64),
            vram_total: Some(32607 << 20),
            power: Some(31.0 + 430.0 * infer),
            temperature: Some((38.0 + 24.0 * infer).round()),
            clock: Some(if infer > 0.05 { 2610.0 } else { 397.0 }),
            unlisted_usage: GpuUsage { shared: None, ..Default::default() },
            ..Default::default()
        },
    ];
    for p in &mut f.processes {
        p.gpus.clear();
    }
    let (amd, nvidia) = (0, 1);
    let on = |device, busy: f64, vram: u64, shared: Option<u64>| GpuUse {
        device,
        usage: GpuUsage { busy: Some(busy), video: Some(0.0), vram: Some(vram), shared },
    };
    let mut give = |name: &str, role: &str, uses: Vec<GpuUse>| {
        if let Some(p) = f.processes.iter_mut().find(|p| p.name == name && p.role == role && p.gpus.is_empty()) {
            p.gpus = uses;
        }
    };
    give("firefox", "GPU process", vec![on(nvidia, (3.0 + 4.0 * wave(2.0, 0.4)).round(), 412 << 20, None)]);
    // The compositor's helper and the browser keep idle contexts on the Radeon.
    give("firefox", "", vec![on(amd, 0.0, 12 << 10, Some(2 << 20))]);
    give("aizome-shell", "", vec![on(nvidia, 1.0, 96 << 20, None), on(amd, 1.0, 6 << 20, Some(4 << 20))]);
    give("code", "", vec![on(nvidia, 1.0, 180 << 20, None)]);
    give("signal-desktop", "", vec![on(nvidia, 0.0, 248 << 20, None)]);
    give("spotify", "", vec![on(amd, (1.0 + 2.0 * wave(0.7, 0.5)).round(), 64 << 20, Some(18 << 20))]);
    // The model's runner: a service of the user's, with nearly all the card's memory.
    let pid = f.processes.iter().map(|p| p.pid).max().unwrap_or(0) + 1;
    let runner = Process {
        pid,
        ppid: 1,
        start_ticks: u64::from(pid) * 101,
        started: 1_792_000_000,
        name: "ollama".into(),
        role: "runner".into(),
        command: "/run/current-system/sw/bin/ollama runner --model qwen3-14b".into(),
        exe: "/nix/store/0000-ollama/bin/ollama".into(),
        user: "callen".into(),
        uid: 1000,
        state: if infer > 0.05 { 'R' } else { 'S' },
        readable: true,
        usage: Usage { cpu: 2.0 + 40.0 * infer, threads: 38, resident: 1_900 << 20, ..Default::default() },
        gpus: vec![on(nvidia, (86.0 * infer).round(), (9.6 * (1u64 << 30) as f64) as u64, None)],
        group: f.groups.len(),
        ..Default::default()
    };
    f.groups.push(Group {
        key: runner.key(),
        kind: Some(Kind::Service),
        name: "ollama".into(),
        detail: "ollama.service".into(),
        own: true,
        members: vec![f.processes.len()],
        usage: runner.usage.clone(),
        ..Default::default()
    });
    f.processes.push(runner);
    f.system.processes = f.processes.len();
    f.total_graphics();
    measure(&mut f);
    f
}

/// The workstation's meters as the sampler reads them: RAPL is root's and
/// there's no battery, but the Radeon's meter is the processor package's
/// and the RTX has its own. Each process draws as it does now since the
/// first reading.
fn measure(f: &mut Frame) {
    f.power = crate::sampler::meters(None, None, false, &f.gpus);
    let cores = f.system.cores.len().max(1) as f64;
    let mut floors = vec![Floor::default(); f.gpus.len()];
    let parts = crate::sampler::attribute(&f.power, &f.gpus, &f.processes, cores, &mut Floor::default(), &mut floors);
    f.power.attributed = parts.iter().flatten().sum();
    let seconds = f.tick as f64;
    let Frame { processes, groups, .. } = f;
    for (p, part) in processes.iter_mut().zip(parts) {
        p.usage.power = part;
        if let Some(w) = part {
            p.usage.energy = w * seconds;
            p.usage.measured = seconds;
        }
    }
    for g in groups.iter_mut() {
        let members = || g.members.iter().map(|&m| &processes[m].usage);
        g.usage.power = members().filter_map(|u| u.power).reduce(|a, b| a + b);
        g.usage.energy = members().map(|u| u.energy).sum();
        g.usage.measured = members().map(|u| u.measured).fold(0.0, f64::max);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn as_many_processes_as_asked() {
        let f = super::frame(600, 3);
        assert!((590..=610).contains(&f.processes.len()), "{}", f.processes.len());
        let members: usize = f.groups.iter().map(|g| g.members.len()).sum();
        assert_eq!(members, f.processes.len());
        assert_ne!(super::frame(600, 4).system.cpu, f.system.cpu, "it moves");
    }

    #[test]
    fn a_workstation_works_its_nvidia_card() {
        let f = super::workstation(300, 40);
        let ollama = f.groups.iter().find(|g| g.name == "ollama").unwrap();
        assert!(ollama.usage.graphics.vram > Some(9 << 30));
        let firefox = f.groups.iter().find(|g| g.name == "Firefox").unwrap();
        assert!(f.processes[firefox.members[0]].uses(0) && f.processes[firefox.members[1]].uses(1));
        let members: usize = f.groups.iter().map(|g| g.members.len()).sum();
        assert_eq!(members, f.processes.len());
    }
}
