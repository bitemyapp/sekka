//! What graphics cards are doing: each card's own figures from sysfs (how
//! busy, video memory, power, temperature, clock), and each process's use
//! from the DRM usage statistics drivers write in `/proc/PID/fdinfo` for
//! every open device (Documentation/gpu/drm-usage-stats.rst: amdgpu, i915,
//! xe, nouveau, msm, panfrost and others; NVIDIA's driver writes none).
//!
//! Busy time comes as nanoseconds per engine (`drm-engine-render: 123 ns`)
//! or, with xe, as cycles busy and cycles counted (`drm-cycles-rcs`,
//! `drm-total-cycles-rcs`); [`ClientUse::between`] turns two readings into
//! percentages.

use super::{read_attr, read_u64};
use std::path::{Path, PathBuf};

/// One open DRM device, from `/proc/PID/fdinfo/FD`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrmClient {
    /// The driver that wrote the figures (`amdgpu`, `i915`).
    pub driver: String,
    /// The card's PCI address (`0000:00:02.0`).
    pub pdev: String,
    /// Identifies the client: several descriptors may share one.
    pub id: u64,
    /// Busy nanoseconds by engine.
    pub engines: Vec<(String, u64)>,
    /// Cycles busy and cycles counted, by engine (xe).
    pub cycles: Vec<(String, u64, u64)>,
    /// How many of an engine there are, where several.
    pub capacity: Vec<(String, u64)>,
    /// Video memory and system memory mapped for the GPU, in bytes.
    pub vram: u64,
    pub shared: u64,
}

fn size(value: &str) -> u64 {
    let mut parts = value.split_ascii_whitespace();
    let n: u64 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    match parts.next() {
        Some("KiB") => n << 10,
        Some("MiB") => n << 20,
        Some("GiB") => n << 30,
        _ => n,
    }
}

/// `None` when the descriptor isn't a DRM client.
pub fn parse_fdinfo(text: &str) -> Option<DrmClient> {
    let mut c = DrmClient::default();
    let mut has_id = false;
    let mut busy_cycles: Vec<(String, u64)> = Vec::new();
    let mut total_cycles: Vec<(String, u64)> = Vec::new();
    let (mut vram, mut vram_total, mut gtt, mut gtt_total) = (None, None::<u64>, None, None::<u64>);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if key == "drm-driver" {
            c.driver = value.to_owned();
        } else if key == "drm-pdev" {
            c.pdev = value.to_owned();
        } else if key == "drm-client-id" {
            c.id = value.parse().ok()?;
            has_id = true;
        } else if let Some(e) = key.strip_prefix("drm-engine-capacity-") {
            c.capacity.push((e.to_owned(), value.parse().unwrap_or(1)));
        } else if let Some(e) = key.strip_prefix("drm-engine-") {
            let ns = value.split_ascii_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
            c.engines.push((e.to_owned(), ns));
        } else if let Some(e) = key.strip_prefix("drm-total-cycles-") {
            total_cycles.push((e.to_owned(), value.parse().unwrap_or(0)));
        } else if let Some(e) = key.strip_prefix("drm-cycles-") {
            busy_cycles.push((e.to_owned(), value.parse().unwrap_or(0)));
        } else if key == "drm-memory-vram" {
            vram = Some(size(value));
        } else if key.starts_with("drm-total-vram") {
            vram_total = Some(vram_total.unwrap_or(0) + size(value));
        } else if key == "drm-memory-gtt" {
            gtt = Some(size(value));
        } else if key == "drm-total-gtt" || key == "drm-total-system" {
            gtt_total = Some(gtt_total.unwrap_or(0) + size(value));
        }
    }
    if !has_id {
        return None;
    }
    for (e, busy) in busy_cycles {
        let total = total_cycles.iter().find(|(t, _)| *t == e).map_or(0, |(_, n)| *n);
        c.cycles.push((e, busy, total));
    }
    c.vram = vram.or(vram_total).unwrap_or(0);
    c.shared = gtt.or(gtt_total).unwrap_or(0);
    Some(c)
}

impl DrmClient {
    /// Whether its figures say how busy it kept the card. Most drivers list
    /// every engine, or none when they don't count; amdgpu leaves out the
    /// engines a client hasn't used, so there none listed means idle.
    pub fn measures_busy(&self) -> bool {
        !self.engines.is_empty() || !self.cycles.is_empty() || self.driver == "amdgpu"
    }
}

/// Whether an engine draws or computes (as opposed to video).
fn is_3d(engine: &str) -> bool {
    matches!(engine, "render" | "gfx" | "compute" | "rcs" | "ccs" | "bcs" | "copy" | "cs")
}

fn is_video(engine: &str) -> bool {
    engine.starts_with("video") || matches!(engine, "vcs" | "vecs" | "dec" | "enc" | "jpeg" | "vcn" | "vcn_unified")
}

/// A process's (or a client's) use of a card between two readings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClientUse {
    /// The busiest 3D or compute engine, in percent of its time.
    pub busy: f32,
    /// The busiest video engine, in percent.
    pub video: f32,
}

impl ClientUse {
    /// From a client's readings `seconds` apart.
    pub fn between(before: &DrmClient, after: &DrmClient, seconds: f64) -> ClientUse {
        let mut out = ClientUse::default();
        let mut note = |engine: &str, percent: f64| {
            let p = percent.clamp(0.0, 100.0) as f32;
            if is_3d(engine) {
                out.busy = out.busy.max(p);
            } else if is_video(engine) {
                out.video = out.video.max(p);
            }
        };
        if seconds > 0.0 {
            for (e, ns) in &after.engines {
                let Some((_, was)) = before.engines.iter().find(|(b, _)| b == e) else { continue };
                let capacity = after.capacity.iter().find(|(c, _)| c == e).map_or(1, |(_, n)| (*n).max(1));
                let delta = ns.saturating_sub(*was) as f64;
                note(e, delta / (seconds * 1e9 * capacity as f64) * 100.0);
            }
        }
        for (e, busy, total) in &after.cycles {
            let Some((_, b0, t0)) = before.cycles.iter().find(|(b, ..)| b == e) else { continue };
            let dt = total.saturating_sub(*t0);
            if dt > 0 {
                note(e, busy.saturating_sub(*b0) as f64 / dt as f64 * 100.0);
            }
        }
        out
    }
}

/// What a card's driver reports now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuStats {
    /// Percent busy (amdgpu's `gpu_busy_percent`), and its video engine's.
    pub busy: Option<f32>,
    pub video: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    /// Watts now, from the card's hwmon meter.
    pub power: Option<f64>,
    /// Joules counted so far, from an energy counter (i915, xe).
    pub energy: Option<f64>,
    pub temperature: Option<f32>,
    /// Clock, in MHz.
    pub clock: Option<f32>,
}

/// The figures of `card` (`card0`).
pub fn gpu_stats(card: &str) -> GpuStats {
    gpu_stats_at(&Path::new("/sys/class/drm").join(card).join("device"))
}

pub(crate) fn gpu_stats_at(device: &Path) -> GpuStats {
    let at = |name: &str| read_u64(device.join(name));
    let mut s = GpuStats {
        busy: at("gpu_busy_percent").map(|p| p as f32),
        video: at("vcn_busy_percent").map(|p| p as f32),
        vram_used: at("mem_info_vram_used"),
        vram_total: at("mem_info_vram_total"),
        ..Default::default()
    };
    let hwmon: Option<PathBuf> =
        std::fs::read_dir(device.join("hwmon")).ok().and_then(|mut d| d.next()).and_then(|e| e.ok()).map(|e| e.path());
    if let Some(h) = hwmon {
        let at = |name: &str| read_u64(h.join(name));
        s.power = at("power1_average").or_else(|| at("power1_input")).map(|uw| uw as f64 / 1e6);
        s.energy = at("energy1_input").map(|uj| uj as f64 / 1e6);
        s.temperature = at("temp1_input").map(|mc| mc as f32 / 1000.0);
        s.clock = at("freq1_input").map(|hz| hz as f32 / 1e6);
    }
    s
}

/// The card's PCI address, to match clients' `drm-pdev`.
pub fn card_pci(card: &str) -> Option<String> {
    let link = std::fs::canonicalize(Path::new("/sys/class/drm").join(card).join("device")).ok()?;
    Some(link.file_name()?.to_string_lossy().into_owned())
}

/// Whether a card is integrated (shares system memory).
pub fn card_integrated(card: &str) -> bool {
    let dev = Path::new("/sys/class/drm").join(card).join("device");
    let vendor = read_attr(dev.join("vendor")).unwrap_or_default();
    match vendor.as_str() {
        "0x1002" => !dev.join("mem_info_vram_vendor").exists(),
        "0x8086" => card_pci(card).is_some_and(|p| p.ends_with(":00:02.0")),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    #[test]
    fn amdgpu_and_xe_clients() {
        let amd = parse_fdinfo(
            "pos:\t0\nflags:\t02100002\ndrm-driver:\tamdgpu\ndrm-pdev:\t0000:03:00.0\ndrm-client-id:\t12\n\
             drm-memory-vram:\t1024 KiB\ndrm-memory-gtt:\t2 MiB\ndrm-engine-gfx:\t1000000000 ns\ndrm-engine-dec:\t0 ns\n",
        )
        .unwrap();
        assert_eq!((amd.id, amd.vram, amd.shared), (12, 1 << 20, 2 << 20));
        assert!(amd.measures_busy());
        // An amdgpu client that hasn't submitted work lists no engines.
        let idle = parse_fdinfo("drm-driver:\tamdgpu\ndrm-pdev:\t0000:78:00.0\ndrm-client-id:\t90\n").unwrap();
        assert!(idle.measures_busy());
        assert_eq!(ClientUse::between(&idle, &idle, 1.0), ClientUse::default());
        let silent = parse_fdinfo("drm-driver:\tnouveau\ndrm-client-id:\t4\ndrm-memory-vram:\t4 MiB\n").unwrap();
        assert!(!silent.measures_busy(), "a driver that counts nothing is unknown, not idle");
        let later =
            DrmClient { engines: vec![("gfx".into(), 1_500_000_000), ("dec".into(), 100_000_000)], ..amd.clone() };
        let u = ClientUse::between(&amd, &later, 1.0);
        assert!((u.busy - 50.0).abs() < 0.01 && (u.video - 10.0).abs() < 0.01, "{u:?}");
        let xe = parse_fdinfo(
            "drm-client-id:\t3\ndrm-cycles-rcs:\t100\ndrm-total-cycles-rcs:\t1000\ndrm-total-vram0:\t4096 KiB\n",
        )
        .unwrap();
        let xe2 = DrmClient { cycles: vec![("rcs".into(), 400, 2000)], ..xe.clone() };
        assert!((ClientUse::between(&xe, &xe2, 1.0).busy - 30.0).abs() < 0.01);
        assert_eq!(xe.vram, 4 << 20);
        assert_eq!(parse_fdinfo("pos:\t0\nflags:\t0\n"), None, "not a DRM client");
    }

    #[test]
    fn card_figures_from_sysfs() {
        let tree = TestTree::new("drm");
        tree.file("device/gpu_busy_percent", "37\n")
            .file("device/mem_info_vram_used", "1048576\n")
            .file("device/hwmon/hwmon4/power1_average", "15000000\n")
            .file("device/hwmon/hwmon4/temp1_input", "51000\n");
        let s = gpu_stats_at(&tree.path("device"));
        assert_eq!((s.busy, s.vram_used, s.power, s.temperature), (Some(37.0), Some(1 << 20), Some(15.0), Some(51.0)));
    }
}
