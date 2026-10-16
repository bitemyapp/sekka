// SPDX-License-Identifier: MIT OR Apache-2.0
//! Graphics processors: each card the kernel's DRM knows, what its driver
//! reports in sysfs (AMD's busy percentage and video memory, a hwmon power
//! meter and temperature), and each process's use from the DRM usage
//! statistics in /proc/PID/fdinfo (Documentation/gpu/drm-usage-stats.rst),
//! which amdgpu, i915, xe, nouveau and others write for every open device.
//! NVIDIA's driver writes none; nvml.rs reads its cards instead.
use crate::procfs;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Device {
    pub pci: String,
    pub driver: String,
    pub vendor: &'static str,
    pub name: String,
    pub integrated: bool,
    sys: PathBuf,
    hwmon: Option<PathBuf>,
}

fn vendor(id: &str) -> &'static str {
    match id {
        "0x1002" => "AMD",
        "0x10de" => "NVIDIA",
        "0x8086" => "Intel",
        _ => "",
    }
}

/// The cards in /sys/class/drm, in the kernel's order, each once.
pub fn discover() -> Vec<Device> {
    let mut cards: Vec<(u32, PathBuf)> = fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let number = name.strip_prefix("card")?.parse().ok()?;
            Some((number, entry.path()))
        })
        .collect();
    cards.sort();
    let mut devices: Vec<Device> = Vec::new();
    for (_, card) in cards {
        let Ok(sys) = fs::canonicalize(card.join("device")) else {
            continue;
        };
        let pci = sys
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if devices.iter().any(|device| device.pci == pci) {
            continue;
        }
        let driver = fs::read_link(sys.join("driver"))
            .ok()
            .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let vendor = vendor(procfs::read(sys.join("vendor")).unwrap_or_default().trim());
        let hwmon = fs::read_dir(sys.join("hwmon"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .next();
        // AMD's discrete cards say whose memory chips they carry; an APU's
        // "video memory" is carved from system memory. Intel's integrated
        // graphics always sit at 00:02.0.
        let integrated = match vendor {
            "AMD" => !sys.join("mem_info_vram_vendor").exists(),
            "Intel" => pci.ends_with(":00:02.0"),
            _ => false,
        };
        let name = procfs::read(sys.join("product_name"))
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| match vendor {
                "AMD" => "AMD Radeon Graphics".into(),
                "Intel" if integrated => "Intel Graphics".into(),
                "Intel" => "Intel Arc Graphics".into(),
                "NVIDIA" => "NVIDIA Graphics".into(),
                _ if !driver.is_empty() => format!("Graphics ({driver})"),
                _ => "Graphics".into(),
            });
        devices.push(Device {
            pci,
            driver,
            vendor,
            name,
            integrated,
            sys,
            hwmon,
        });
    }
    devices
}

/// What a card's driver reports through sysfs now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sysfs {
    pub busy: Option<f64>,
    pub video: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    /// Watts now, from a power reading.
    pub power: Option<f64>,
    /// Joules counted so far, from an energy counter.
    pub energy: Option<f64>,
    pub temperature: Option<f64>,
    pub clock: Option<f64>,
}

impl Device {
    pub fn read(&self) -> Sysfs {
        let at = |name: &str| procfs::number(self.sys.join(name));
        let mut sysfs = Sysfs {
            busy: at("gpu_busy_percent").map(|p| p as f64),
            video: at("vcn_busy_percent").map(|p| p as f64),
            vram_used: at("mem_info_vram_used"),
            vram_total: at("mem_info_vram_total"),
            ..Default::default()
        };
        if let Some(hwmon) = &self.hwmon {
            let at = |name: &str| procfs::number(hwmon.join(name));
            // Microwatts, averaged where the driver averages.
            sysfs.power = at("power1_average")
                .or_else(|| at("power1_input"))
                .map(|uw| uw as f64 / 1e6);
            // Microjoules (i915's and xe's meters).
            sysfs.energy = at("energy1_input").map(|uj| uj as f64 / 1e6);
            sysfs.temperature = at("temp1_input").map(|mc| mc as f64 / 1000.0);
            sysfs.clock = at("freq1_input").map(|hz| hz as f64 / 1e6);
        }
        sysfs
    }
}

/// One DRM client (an open device) from /proc/PID/fdinfo/FD.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Client {
    pub pdev: String,
    pub id: u64,
    /// Busy time per engine, in nanoseconds.
    pub engines: HashMap<String, u64>,
    /// Engines that count cycles instead (xe): cycles busy, and total.
    pub cycles: HashMap<String, (u64, u64)>,
    /// Instances of an engine, where there are several.
    pub capacity: HashMap<String, u64>,
    pub vram: u64,
    pub shared: u64,
}

fn kib(value: &str) -> u64 {
    let mut parts = value.split_whitespace();
    let number: u64 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    match parts.next() {
        Some("KiB") => number * 1024,
        Some("MiB") => number * 1024 * 1024,
        Some("GiB") => number * 1024 * 1024 * 1024,
        _ => number,
    }
}

pub fn parse_fdinfo(text: &str) -> Option<Client> {
    let mut client = Client::default();
    let mut has_id = false;
    let mut total_cycles: HashMap<String, u64> = HashMap::new();
    let mut vram = (None, None);
    let mut shared = (None, None);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if key == "drm-pdev" {
            client.pdev = value.to_owned();
        } else if key == "drm-client-id" {
            client.id = value.parse().ok()?;
            has_id = true;
        } else if let Some(engine) = key.strip_prefix("drm-engine-capacity-") {
            client
                .capacity
                .insert(engine.to_owned(), value.parse().unwrap_or(1));
        } else if let Some(engine) = key.strip_prefix("drm-engine-") {
            let ns = value.split_whitespace().next().and_then(|n| n.parse().ok());
            client.engines.insert(engine.to_owned(), ns.unwrap_or(0));
        } else if let Some(engine) = key.strip_prefix("drm-total-cycles-") {
            total_cycles.insert(engine.to_owned(), value.parse().unwrap_or(0));
        } else if let Some(engine) = key.strip_prefix("drm-cycles-") {
            let busy = value.parse().unwrap_or(0);
            client.cycles.entry(engine.to_owned()).or_default().0 = busy;
        } else if key == "drm-memory-vram" {
            vram.0 = Some(kib(value));
        } else if key.starts_with("drm-total-vram") {
            vram.1 = Some(vram.1.unwrap_or(0) + kib(value));
        } else if key == "drm-memory-gtt" {
            shared.0 = Some(kib(value));
        } else if key == "drm-total-gtt" || key == "drm-total-system" {
            shared.1 = Some(shared.1.unwrap_or(0) + kib(value));
        }
    }
    for (engine, total) in total_cycles {
        client.cycles.entry(engine).or_default().1 = total;
    }
    // Older amdgpu writes drm-memory-*; newer drivers drm-total-*.
    client.vram = vram.0.or(vram.1).unwrap_or(0);
    client.shared = shared.0.or(shared.1).unwrap_or(0);
    has_id.then_some(client)
}

/// Engines that draw and compute, and engines that encode and decode
/// video, by the names drivers give them.
pub fn engine_class(engine: &str) -> Option<bool> {
    let base = engine.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
    match base {
        "gfx" | "compute" | "render" | "rcs" | "ccs" | "3d" | "graphics" | "cs" | "gpu"
        | "fragment" | "vertex-tiler" => Some(false),
        "dec" | "enc" | "jpeg" | "video" | "video-enhance" | "vcs" | "vecs" | "vcn" | "nvdec"
        | "nvenc" | "media" => Some(true),
        _ => None,
    }
}

/// The DRM devices a process has open: file descriptors pointing into
/// /dev/dri.
pub fn drm_fds(pid: i32) -> Vec<u32> {
    let dir = format!("/proc/{pid}/fd");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let fd: u32 = entry.file_name().to_str()?.parse().ok()?;
            let target = fs::read_link(entry.path()).ok()?;
            target.starts_with(Path::new("/dev/dri")).then_some(fd)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amdgpu_fdinfo() {
        let text = "pos:\t0\nflags:\t0100002\ndrm-driver:\tamdgpu\ndrm-client-id:\t287\n\
                    drm-pdev:\t0000:78:00.0\npasid:\t93\ndrm-total-vram:\t12 KiB\n\
                    drm-memory-vram:\t12 KiB\ndrm-memory-gtt: \t2048 KiB\n\
                    drm-engine-gfx:\t1500000 ns\ndrm-engine-dec:\t300 ns\n";
        let client = parse_fdinfo(text).unwrap();
        assert_eq!(client.pdev, "0000:78:00.0");
        assert_eq!(client.id, 287);
        assert_eq!(client.vram, 12 * 1024);
        assert_eq!(client.shared, 2048 * 1024);
        assert_eq!(client.engines["gfx"], 1_500_000);
        assert_eq!(engine_class("gfx"), Some(false));
        assert_eq!(engine_class("enc_1"), Some(true));
        assert_eq!(engine_class("dma"), None);
        // NVIDIA's DRM file descriptors carry no statistics.
        assert_eq!(
            parse_fdinfo("pos:\t0\nflags:\t0100002\nmnt_id:\t541\n"),
            None
        );
    }

    #[test]
    fn xe_cycles_and_i915_capacity() {
        let text = "drm-driver:\txe\ndrm-client-id:\t9\ndrm-pdev:\t0000:00:02.0\n\
                    drm-cycles-rcs:\t28257900\ndrm-total-cycles-rcs:\t7655183225\n\
                    drm-total-vram0:\t4 MiB\ndrm-total-system:\t16 MiB\n";
        let client = parse_fdinfo(text).unwrap();
        assert_eq!(client.cycles["rcs"], (28257900, 7655183225));
        assert_eq!(client.vram, 4 * 1024 * 1024);
        assert_eq!(client.shared, 16 * 1024 * 1024);
        let text = "drm-driver:\ti915\ndrm-client-id:\t3\ndrm-pdev:\t0000:00:02.0\n\
                    drm-engine-render:\t25662044495 ns\ndrm-engine-video:\t0 ns\n\
                    drm-engine-capacity-video:\t2\n";
        let client = parse_fdinfo(text).unwrap();
        assert_eq!(client.engines["render"], 25662044495);
        assert_eq!(client.capacity["video"], 2);
    }
}
