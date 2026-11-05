//! What the machine has: GPUs and their memory, system RAM and the CPU.
//!
//! Detection reads sysfs, procfs and `nvidia-smi` (with a timeout), and never
//! fails: whatever it can't learn is left out and explained in `notes`. Every
//! source can be pointed at a fixture tree ([`Probe`]), which is how the tests
//! describe machines that aren't this one.
//!
//! # Integrated AMD GPUs
//!
//! amdgpu doesn't say "integrated" in sysfs, so this is a heuristic. An AMD
//! GPU is taken as integrated (an APU) when sysfs names no VRAM vendor
//! (`mem_info_vram_vendor` is shown only when the VBIOS describes memory chips,
//! which an APU has none of) and its VRAM is a firmware carve-out of at most
//! 4 GiB (typically 512 MiB–4 GiB; discrete cards that are useful for models
//! have more). Its memory is then unified: the budget is system RAM, capped by
//! the GTT size (how much the driver lets the GPU map). An APU set up with a
//! large carve-out (a Strix Halo can reserve up to 96 GiB) is described by its
//! VRAM instead, which is also what llama.cpp sees.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::units::{Bytes, BytesPerSecond};

/// The carve-out size up to which an AMD GPU without named VRAM chips is
/// taken as integrated (see the module documentation).
pub const APU_CARVEOUT_MAX: Bytes = Bytes::gib(4);

/// How long `nvidia-smi` may take before detection gives up on it.
pub const NVIDIA_SMI_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hardware {
    pub gpus: Vec<Gpu>,
    pub ram: Ram,
    pub cpu: Cpu,
    /// What detection couldn't find out, in words ("nvidia-smi timed out").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A GPU's position in [`Hardware::gpus`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GpuIndex(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GpuVendor {
    fn from_pci(id: u32) -> GpuVendor {
        match id {
            0x10de => GpuVendor::Nvidia,
            0x1002 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            _ => GpuVendor::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GpuVendor::Nvidia => "NVIDIA",
            GpuVendor::Amd => "AMD",
            GpuVendor::Intel => "Intel",
            GpuVendor::Other => "Other",
        }
    }
}

/// How a GPU's memory relates to the system's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GpuMemory {
    /// Its own VRAM.
    Dedicated { total: Bytes, free: Bytes },
    /// It shares system RAM (an APU, a GB10, an integrated Intel GPU).
    /// `limit` is how much the driver lets it map (amdgpu's GTT), if known.
    Unified { limit: Option<Bytes> },
    /// The driver didn't say (no nvidia-smi, an Intel discrete card).
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    pub vendor: GpuVendor,
    pub name: String,
    pub memory: GpuMemory,
    /// A connected display is plugged into this card, so the desktop uses
    /// some of its memory.
    pub drives_display: bool,
    pub integrated: bool,
    /// Memory bandwidth, when the model is in the built-in table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<BytesPerSecond>,
    /// PCI address, `0000:01:00.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pci: Option<String>,
    /// The vendor runtime's device number (`nvidia-smi`'s index for NVIDIA).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_index: Option<u32>,
    /// NVIDIA compute capability, `12.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_capability: Option<String>,
}

impl Gpu {
    pub fn dedicated_total(&self) -> Option<Bytes> {
        match self.memory {
            GpuMemory::Dedicated { total, .. } => Some(total),
            _ => None,
        }
    }

    pub fn dedicated_free(&self) -> Option<Bytes> {
        match self.memory {
            GpuMemory::Dedicated { free, .. } => Some(free),
            _ => None,
        }
    }

    pub fn is_unified(&self) -> bool {
        matches!(self.memory, GpuMemory::Unified { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ram {
    pub total: Bytes,
    pub available: Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cpu {
    pub model: String,
    pub physical_cores: u32,
    pub logical_cores: u32,
    pub flags: CpuFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CpuFlags {
    pub avx2: bool,
    pub avx512: bool,
    pub avx512_bf16: bool,
    pub avx_vnni: bool,
    pub amx: bool,
    /// AArch64 SVE.
    pub sve: bool,
    /// AArch64 int8 matrix multiply.
    pub i8mm: bool,
}

impl Cpu {
    /// llama.cpp's `--threads` for work it does on the CPU: the physical
    /// cores, less two for the desktop, at least one.
    pub fn llama_threads(&self) -> u32 {
        self.physical_cores.saturating_sub(2).max(1)
    }
}

/// Where detection reads from. [`Probe::system`] is this machine; tests build
/// one over fixture trees.
#[derive(Clone, Debug)]
pub struct Probe {
    /// Stands for `/sys`.
    pub sys: PathBuf,
    /// Stands for `/proc`.
    pub proc: PathBuf,
    pub nvidia_smi: NvidiaSmi,
}

#[derive(Clone, Debug)]
pub enum NvidiaSmi {
    /// Run this program, killing it after `timeout`.
    Run { program: PathBuf, timeout: Duration },
    /// Use this text as its CSV output (fixtures).
    Output(String),
    /// There is no nvidia-smi.
    Absent,
}

impl Probe {
    pub fn system() -> Probe {
        Probe {
            sys: PathBuf::from("/sys"),
            proc: PathBuf::from("/proc"),
            nvidia_smi: NvidiaSmi::Run { program: PathBuf::from("nvidia-smi"), timeout: NVIDIA_SMI_TIMEOUT },
        }
    }

    /// A fixture machine rooted at `root` (`root/sys`, `root/proc`, and
    /// `root/nvidia-smi.csv` if it has NVIDIA GPUs).
    pub fn fixture(root: &Path) -> Probe {
        let csv = root.join("nvidia-smi.csv");
        Probe {
            sys: root.join("sys"),
            proc: root.join("proc"),
            nvidia_smi: match fs::read_to_string(&csv) {
                Ok(text) => NvidiaSmi::Output(text),
                Err(_) => NvidiaSmi::Absent,
            },
        }
    }
}

impl Hardware {
    /// This machine. Fast (one nvidia-smi run), never hangs, never fails.
    pub fn detect() -> Hardware {
        Hardware::detect_with(&Probe::system())
    }

    pub fn detect_with(probe: &Probe) -> Hardware {
        let mut notes = Vec::new();
        let ram = read_meminfo(&probe.proc.join("meminfo")).unwrap_or_else(|| {
            notes.push("couldn't read /proc/meminfo".to_owned());
            Ram::default()
        });
        let cpu = read_cpuinfo(&probe.proc.join("cpuinfo")).unwrap_or_else(|| {
            notes.push("couldn't read /proc/cpuinfo".to_owned());
            Cpu::default()
        });
        let cards = drm_cards(&probe.sys);
        let nvidia = nvidia_gpus(probe, &mut notes);
        let mut gpus = Vec::new();
        let mut nvidia_used = vec![false; nvidia.len()];
        for card in &cards {
            match card.vendor {
                GpuVendor::Nvidia => {
                    // Match nvidia-smi's rows to DRM cards by PCI address.
                    let pos = nvidia.iter().position(|g| g.pci.as_deref() == card.pci.as_deref());
                    match pos {
                        Some(i) => {
                            nvidia_used[i] = true;
                            let mut g = nvidia[i].clone();
                            g.drives_display = card.drives_display;
                            gpus.push(g);
                        }
                        None => gpus.push(Gpu {
                            vendor: GpuVendor::Nvidia,
                            name: nvidia_proc_name(&probe.proc, card.pci.as_deref())
                                .unwrap_or_else(|| "NVIDIA GPU".to_owned()),
                            memory: GpuMemory::Unknown,
                            drives_display: card.drives_display,
                            integrated: false,
                            bandwidth: None,
                            pci: card.pci.clone(),
                            device_index: None,
                            compute_capability: None,
                        }),
                    }
                }
                GpuVendor::Amd => gpus.push(amd_gpu(card, &ram)),
                GpuVendor::Intel => gpus.push(intel_gpu(card)),
                GpuVendor::Other => {}
            }
        }
        // NVIDIA GPUs without a DRM card (nvidia-drm not loaded; a GB10 in a
        // headless box) still count.
        for (g, used) in nvidia.into_iter().zip(nvidia_used) {
            if !used {
                gpus.push(g);
            }
        }
        for g in &mut gpus {
            if g.bandwidth.is_none() {
                g.bandwidth = known_bandwidth(&g.name);
            }
        }
        Hardware { gpus, ram, cpu, notes }
    }

    /// The GPUs llama.cpp can run on with a known budget, largest first.
    pub fn gpu(&self, index: GpuIndex) -> Option<&Gpu> {
        self.gpus.get(index.0)
    }
}

/// A DRM card from sysfs.
#[derive(Debug)]
struct DrmCard {
    dir: PathBuf,
    vendor: GpuVendor,
    pci: Option<String>,
    drives_display: bool,
}

fn drm_cards(sys: &Path) -> Vec<DrmCard> {
    let drm = sys.join("class/drm");
    let Ok(entries) = fs::read_dir(&drm) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.strip_prefix("card").is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())))
        .collect();
    names.sort_by_key(|n| n[4..].parse::<u32>().unwrap_or(u32::MAX));
    let connectors: Vec<String> = fs::read_dir(&drm)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names
        .into_iter()
        .filter_map(|name| {
            let dir = drm.join(&name);
            let device = dir.join("device");
            let vendor = read_hex(&device.join("vendor"))?;
            // Display controllers only (class 0x03xxxx).
            if read_hex(&device.join("class")).is_some_and(|c| c >> 16 != 0x03) {
                return None;
            }
            let pci = pci_address(&device);
            let prefix = format!("{name}-");
            let drives_display = connectors.iter().filter(|c| c.starts_with(&prefix)).any(|c| {
                fs::read_to_string(drm.join(c).join("status")).is_ok_and(|s| s.trim() == "connected")
            });
            Some(DrmCard { dir, vendor: GpuVendor::from_pci(vendor as u32), pci, drives_display })
        })
        .collect()
}

/// The PCI address of a sysfs device: the name its `device` link points to,
/// or the `PCI_SLOT_NAME` in its uevent (fixtures have no links).
fn pci_address(device: &Path) -> Option<String> {
    if let Ok(target) = fs::read_link(device)
        && let Some(name) = target.file_name()
    {
        let name = name.to_string_lossy();
        if looks_like_pci(&name) {
            return Some(name.to_ascii_lowercase());
        }
    }
    let uevent = fs::read_to_string(device.join("uevent")).ok()?;
    uevent.lines().find_map(|l| l.strip_prefix("PCI_SLOT_NAME=")).map(|s| s.trim().to_ascii_lowercase())
}

fn looks_like_pci(s: &str) -> bool {
    // dddd:bb:dd.f
    s.len() == 12 && s.as_bytes()[4] == b':' && s.as_bytes()[7] == b':' && s.as_bytes()[10] == b'.'
}

/// nvidia-smi's `00000000:01:00.0` as sysfs writes it, `0000:01:00.0`.
fn normalize_nvidia_pci(s: &str) -> Option<String> {
    let (domain, rest) = s.trim().split_once(':')?;
    let domain = u32::from_str_radix(domain, 16).ok()?;
    Some(format!("{domain:04x}:{}", rest.to_ascii_lowercase()))
}

fn read_hex(path: &Path) -> Option<u64> {
    let s = fs::read_to_string(path).ok()?;
    let s = s.trim();
    u64::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn amd_gpu(card: &DrmCard, ram: &Ram) -> Gpu {
    let device = card.dir.join("device");
    let vram_total = read_u64(&device.join("mem_info_vram_total")).map(Bytes);
    let vram_used = read_u64(&device.join("mem_info_vram_used")).map(Bytes);
    let gtt_total = read_u64(&device.join("mem_info_gtt_total")).map(Bytes);
    let names_vram_chips = device.join("mem_info_vram_vendor").exists();
    let integrated = !names_vram_chips && vram_total.is_some_and(|v| v <= APU_CARVEOUT_MAX);
    let memory = match (integrated, vram_total) {
        (true, _) => GpuMemory::Unified { limit: gtt_total.map(|g| g.min(ram.total)).or(Some(ram.total)) },
        (false, Some(total)) => GpuMemory::Dedicated { total, free: total - vram_used.unwrap_or_default() },
        (false, None) => GpuMemory::Unknown,
    };
    let name = amd_name(&device).unwrap_or_else(|| if integrated { "AMD Radeon (integrated)" } else { "AMD Radeon" }.to_owned());
    Gpu {
        vendor: GpuVendor::Amd,
        name,
        memory,
        drives_display: card.drives_display,
        integrated,
        bandwidth: None,
        pci: card.pci.clone(),
        device_index: None,
        compute_capability: None,
    }
}

/// amdgpu has `product_name` on some boards; otherwise the PCI device ID.
fn amd_name(device: &Path) -> Option<String> {
    if let Ok(s) = fs::read_to_string(device.join("product_name")) {
        let s = s.trim();
        if !s.is_empty() {
            return Some(s.to_owned());
        }
    }
    let id = read_hex(&device.join("device"))?;
    Some(format!("AMD Radeon [1002:{id:04x}]"))
}

fn intel_gpu(card: &DrmCard) -> Gpu {
    let device = card.dir.join("device");
    // xe reports discrete VRAM per tile; i915 has lmem_total_bytes on the card.
    let vram = read_u64(&device.join("tile0/physical_vram_size_bytes"))
        .or_else(|| read_u64(&card.dir.join("lmem_total_bytes")))
        .filter(|&v| v > 0)
        .map(Bytes);
    let id = read_hex(&device.join("device")).unwrap_or(0);
    let discrete_id = intel_is_discrete(id as u32);
    let (memory, integrated) = match vram {
        Some(total) => (GpuMemory::Dedicated { total, free: total }, false),
        None if discrete_id => (GpuMemory::Unknown, false),
        None => (GpuMemory::Unified { limit: None }, true),
    };
    Gpu {
        vendor: GpuVendor::Intel,
        name: format!("Intel {} [8086:{id:04x}]", if integrated { "Graphics" } else { "Arc" }),
        memory,
        drives_display: card.drives_display,
        integrated,
        bandwidth: None,
        pci: card.pci.clone(),
        device_index: None,
        compute_capability: None,
    }
}

/// Intel's discrete Arc device IDs (Alchemist 0x56xx, Battlemage 0xe20x–0xe21x).
fn intel_is_discrete(id: u32) -> bool {
    (0x5690..=0x56ff).contains(&id) || (0xe200..=0xe21f).contains(&id)
}

fn nvidia_gpus(probe: &Probe, notes: &mut Vec<String>) -> Vec<Gpu> {
    let text = match &probe.nvidia_smi {
        NvidiaSmi::Absent => return Vec::new(),
        NvidiaSmi::Output(text) => text.clone(),
        NvidiaSmi::Run { program, timeout } => match run_nvidia_smi(program, *timeout) {
            Ok(Some(text)) => text,
            Ok(None) => return Vec::new(), // not installed
            Err(e) => {
                notes.push(e);
                return Vec::new();
            }
        },
    };
    parse_nvidia_smi(&text)
}

const NVIDIA_QUERY: &str = "--query-gpu=index,name,memory.total,memory.used,memory.free,pci.bus_id,compute_cap";

/// `Ok(None)` when nvidia-smi isn't installed.
fn run_nvidia_smi(program: &Path, timeout: Duration) -> Result<Option<String>, String> {
    let mut child = match Command::new(program)
        .args([NVIDIA_QUERY, "--format=csv,noheader,nounits"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("couldn't run nvidia-smi: {e}")),
    };
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("nvidia-smi didn't answer within {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(format!("nvidia-smi: {e}")),
        }
    };
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("nvidia-smi failed ({status}); is the NVIDIA driver loaded?"));
    }
    Ok(Some(out))
}

/// Rows of `index, name, memory.total, memory.used, memory.free, pci.bus_id,
/// compute_cap` in MiB. Memory `[N/A]` means the GPU has none of its own.
pub(crate) fn parse_nvidia_smi(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() < 7 {
                return None;
            }
            let mib = |s: &str| s.parse::<u64>().ok().map(Bytes::mib);
            let memory = match (mib(f[2]), mib(f[4])) {
                (Some(total), Some(free)) if total > Bytes::ZERO => GpuMemory::Dedicated { total, free },
                _ => GpuMemory::Unified { limit: None },
            };
            Some(Gpu {
                vendor: GpuVendor::Nvidia,
                name: f[1].to_owned(),
                integrated: matches!(memory, GpuMemory::Unified { .. }),
                memory,
                drives_display: false,
                bandwidth: None,
                pci: normalize_nvidia_pci(f[5]),
                device_index: f[0].parse().ok(),
                compute_capability: Some(f[6].to_owned()).filter(|s| !s.starts_with('[')),
            })
        })
        .collect()
}

/// The model name from `/proc/driver/nvidia/gpus/<pci>/information`.
fn nvidia_proc_name(proc: &Path, pci: Option<&str>) -> Option<String> {
    let info = fs::read_to_string(proc.join("driver/nvidia/gpus").join(pci?).join("information")).ok()?;
    info.lines().find_map(|l| l.strip_prefix("Model:")).map(|s| s.trim().to_owned())
}

fn read_meminfo(path: &Path) -> Option<Ram> {
    let text = fs::read_to_string(path).ok()?;
    let field = |name: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(name)?.strip_prefix(':')?;
            rest.split_whitespace().next()?.parse::<u64>().ok().map(Bytes::kib)
        })
    };
    let total = field("MemTotal")?;
    Some(Ram { total, available: field("MemAvailable").unwrap_or(total) })
}

fn read_cpuinfo(path: &Path) -> Option<Cpu> {
    let text = fs::read_to_string(path).ok()?;
    let mut logical = 0u32;
    let mut cores = std::collections::BTreeSet::new();
    let mut model = String::new();
    let mut flags = CpuFlags::default();
    let mut physical_id = String::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "processor" => logical += 1,
            "model name" | "Model" if model.is_empty() => model = value.to_owned(),
            "physical id" => physical_id = value.to_owned(),
            "core id" => {
                cores.insert((physical_id.clone(), value.to_owned()));
            }
            "flags" | "Features" => {
                for f in value.split_whitespace() {
                    match f {
                        "avx2" => flags.avx2 = true,
                        "avx512f" => flags.avx512 = true,
                        "avx512_bf16" => flags.avx512_bf16 = true,
                        "avx_vnni" => flags.avx_vnni = true,
                        "amx_tile" => flags.amx = true,
                        "sve" => flags.sve = true,
                        "i8mm" => flags.i8mm = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if logical == 0 {
        return None;
    }
    // ARM's cpuinfo has no core ids: every processor is a core.
    let physical = if cores.is_empty() { logical } else { cores.len() as u32 };
    Some(Cpu { model, physical_cores: physical, logical_cores: logical, flags })
}

/// Memory bandwidth of GPUs people run models on, from their specifications,
/// for speed estimates. Matched as a case-insensitive substring of the name,
/// most specific first.
const BANDWIDTH_GB_S: &[(&str, f64)] = &[
    ("RTX PRO 6000", 1792.0),
    ("RTX 5090", 1792.0),
    ("RTX 5080", 960.0),
    ("RTX 5070 Ti", 896.0),
    ("RTX 5070", 672.0),
    ("RTX 5060 Ti", 448.0),
    ("RTX 5060", 448.0),
    ("RTX 4090", 1008.0),
    ("RTX 4080 SUPER", 736.0),
    ("RTX 4080", 717.0),
    ("RTX 4070 Ti SUPER", 672.0),
    ("RTX 4070 Ti", 504.0),
    ("RTX 4070 SUPER", 504.0),
    ("RTX 4070", 504.0),
    ("RTX 4060 Ti", 288.0),
    ("RTX 4060", 272.0),
    ("RTX 3090 Ti", 1008.0),
    ("RTX 3090", 936.0),
    ("RTX 3080 Ti", 912.0),
    ("RTX 3080", 760.0),
    ("RTX 3070", 448.0),
    ("RTX 3060", 360.0),
    ("RTX A6000", 768.0),
    ("GB10", 273.0),
    ("RX 9070 XT", 640.0),
    ("RX 9070", 640.0),
    ("RX 7900 XTX", 960.0),
    ("RX 7900 XT", 800.0),
    ("RX 7800 XT", 624.0),
    ("RX 7600", 288.0),
    ("8060S", 256.0),
];

pub fn known_bandwidth(name: &str) -> Option<BytesPerSecond> {
    let lower = name.to_ascii_lowercase();
    BANDWIDTH_GB_S
        .iter()
        .find(|(model, _)| lower.contains(&model.to_ascii_lowercase()))
        .map(|&(_, gb)| BytesPerSecond::gb_per_s(gb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_smi_rows() {
        let gpus = parse_nvidia_smi(
            "0, NVIDIA GeForce RTX 5090, 32607, 1885, 30284, 00000000:01:00.0, 12.0\n\
             1, NVIDIA GB10, [N/A], [N/A], [N/A], 0000000F:01:00.0, 12.1\n",
        );
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].memory, GpuMemory::Dedicated { total: Bytes::mib(32607), free: Bytes::mib(30284) });
        assert_eq!(gpus[0].pci.as_deref(), Some("0000:01:00.0"));
        assert_eq!(gpus[0].device_index, Some(0));
        assert!(gpus[1].is_unified());
        assert_eq!(gpus[1].pci.as_deref(), Some("000f:01:00.0"));
    }

    #[test]
    fn bandwidth_prefers_the_specific_name() {
        assert_eq!(known_bandwidth("NVIDIA GeForce RTX 4070 Ti SUPER"), Some(BytesPerSecond::gb_per_s(672.0)));
        assert_eq!(known_bandwidth("NVIDIA GeForce RTX 4070"), Some(BytesPerSecond::gb_per_s(504.0)));
        assert_eq!(known_bandwidth("Mystery GPU"), None);
    }

    #[test]
    fn threads_leave_two_cores() {
        let cpu = Cpu { physical_cores: 8, logical_cores: 16, ..Cpu::default() };
        assert_eq!(cpu.llama_threads(), 6);
        assert_eq!(Cpu { physical_cores: 2, ..Cpu::default() }.llama_threads(), 1);
    }
}
