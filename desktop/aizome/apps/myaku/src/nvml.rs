// SPDX-License-Identifier: MIT OR Apache-2.0
//! NVIDIA graphics through NVML, the library nvidia-smi uses, opened at run
//! time so the window needs no NVIDIA driver to start. NixOS puts it in
//! /run/opengl-driver/lib. NVIDIA's driver reports no per-process figures
//! in DRM's fdinfo, so NVML gives each process's share of the
//! graphics processor and its video memory, and the card's power meter.
//! Ported from Sekka's desktop/tatami/activity/src/nvml.rs.
use crate::model::GpuUsage;
use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_ulonglong, c_void},
    ptr,
};

type Return = c_int;
type Device = *mut c_void;

const SUCCESS: Return = 0;
const INSUFFICIENT_SIZE: Return = 7;
const NOT_FOUND: Return = 6;
const TEMPERATURE_GPU: c_uint = 0;
const CLOCK_GRAPHICS: c_uint = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MemoryInfo {
    total: c_ulonglong,
    free: c_ulonglong,
    used: c_ulonglong,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MemoryInfoV2 {
    version: c_uint,
    total: c_ulonglong,
    reserved: c_ulonglong,
    free: c_ulonglong,
    used: c_ulonglong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PciInfo {
    bus_id_legacy: [c_char; 16],
    domain: c_uint,
    bus: c_uint,
    device: c_uint,
    pci_device_id: c_uint,
    pci_subsystem_id: c_uint,
    bus_id: [c_char; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessSample {
    pid: c_uint,
    time_stamp: c_ulonglong,
    sm_util: c_uint,
    mem_util: c_uint,
    enc_util: c_uint,
    dec_util: c_uint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessInfo {
    pid: c_uint,
    used_gpu_memory: c_ulonglong,
    gpu_instance_id: c_uint,
    compute_instance_id: c_uint,
}

struct Api {
    _library: Library,
    init: unsafe extern "C" fn() -> Return,
    shutdown: unsafe extern "C" fn() -> Return,
    count: unsafe extern "C" fn(*mut c_uint) -> Return,
    handle: unsafe extern "C" fn(c_uint, *mut Device) -> Return,
    name: unsafe extern "C" fn(Device, *mut c_char, c_uint) -> Return,
    pci: unsafe extern "C" fn(Device, *mut PciInfo) -> Return,
    utilization: unsafe extern "C" fn(Device, *mut Utilization) -> Return,
    memory: unsafe extern "C" fn(Device, *mut MemoryInfo) -> Return,
    memory_v2: Option<unsafe extern "C" fn(Device, *mut MemoryInfoV2) -> Return>,
    power: unsafe extern "C" fn(Device, *mut c_uint) -> Return,
    energy: Option<unsafe extern "C" fn(Device, *mut c_ulonglong) -> Return>,
    temperature: unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> Return,
    clock: unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> Return,
    encoder: Option<unsafe extern "C" fn(Device, *mut c_uint, *mut c_uint) -> Return>,
    decoder: Option<unsafe extern "C" fn(Device, *mut c_uint, *mut c_uint) -> Return>,
    samples: Option<unsafe extern "C" fn(Device, *mut ProcessSample, *mut c_uint, c_ulonglong) -> Return>,
    graphics: unsafe extern "C" fn(Device, *mut c_uint, *mut ProcessInfo) -> Return,
    compute: unsafe extern "C" fn(Device, *mut c_uint, *mut ProcessInfo) -> Return,
}

struct Library(*mut c_void);

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: this owns one successful dlopen, including on a missing symbol.
        unsafe {
            libc::dlclose(self.0);
        }
    }
}

// SAFETY: NVML is thread-safe; the sampler alone calls it, from one thread.
unsafe impl Send for Api {}

unsafe fn symbol<T: Copy>(library: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: the caller names a function of the type T.
    unsafe {
        let pointer = libc::dlsym(library, name.as_ptr());
        (!pointer.is_null()).then(|| std::mem::transmute_copy(&pointer))
    }
}

impl Api {
    fn open() -> Option<Api> {
        let names: [&CStr; 2] = [c"libnvidia-ml.so.1", c"/run/opengl-driver/lib/libnvidia-ml.so.1"];
        // SAFETY: dlopen with a valid C string; the handle is kept open.
        let library = Library(
            names
                .iter()
                .map(|name| unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) })
                .find(|handle| !handle.is_null())?,
        );
        // SAFETY: the symbols are NVML's, with these signatures (nvml.h).
        unsafe {
            let handle = library.0;
            macro_rules! required {
                ($name:literal) => {
                    symbol(handle, $name)?
                };
            }
            Some(Api {
                init: required!(c"nvmlInit_v2"),
                shutdown: required!(c"nvmlShutdown"),
                count: required!(c"nvmlDeviceGetCount_v2"),
                handle: required!(c"nvmlDeviceGetHandleByIndex_v2"),
                name: required!(c"nvmlDeviceGetName"),
                pci: required!(c"nvmlDeviceGetPciInfo_v3"),
                utilization: required!(c"nvmlDeviceGetUtilizationRates"),
                memory: required!(c"nvmlDeviceGetMemoryInfo"),
                memory_v2: symbol(handle, c"nvmlDeviceGetMemoryInfo_v2"),
                power: required!(c"nvmlDeviceGetPowerUsage"),
                energy: symbol(handle, c"nvmlDeviceGetTotalEnergyConsumption"),
                temperature: required!(c"nvmlDeviceGetTemperature"),
                clock: required!(c"nvmlDeviceGetClockInfo"),
                encoder: symbol(handle, c"nvmlDeviceGetEncoderUtilization"),
                decoder: symbol(handle, c"nvmlDeviceGetDecoderUtilization"),
                samples: symbol(handle, c"nvmlDeviceGetProcessUtilization"),
                graphics: symbol(handle, c"nvmlDeviceGetGraphicsRunningProcesses_v3")
                    .or_else(|| symbol(handle, c"nvmlDeviceGetGraphicsRunningProcesses_v2"))?,
                compute: symbol(handle, c"nvmlDeviceGetComputeRunningProcesses_v3")
                    .or_else(|| symbol(handle, c"nvmlDeviceGetComputeRunningProcesses_v2"))?,
                _library: library,
            })
        }
    }
}

/// One NVIDIA graphics processor as read now.
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub name: String,
    /// PCI address as the kernel writes it: "0000:01:00.0".
    pub pci: String,
    pub busy: Option<f64>,
    pub video: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    /// Watts now.
    pub power: Option<f64>,
    /// Joules counted since the driver loaded (Volta and newer): two
    /// readings give the power between them.
    pub energy: Option<f64>,
    pub temperature: Option<f64>,
    pub clock: Option<f64>,
    /// Each process's share: PID, percent of the 3D/compute engines,
    /// percent of video, video memory.
    pub processes: Vec<(u32, GpuUsage)>,
    pub unlisted_usage: GpuUsage,
}

pub struct Nvml {
    api: Api,
    devices: Vec<Device>,
    /// The newest sample each device has given, so the next asks for newer.
    last_seen: Vec<c_ulonglong>,
}

// SAFETY: device handles are plain identifiers, used from the sampler's
// thread only.
unsafe impl Send for Nvml {}

impl Nvml {
    pub fn open() -> Option<Nvml> {
        let api = Api::open()?;
        // SAFETY: NVML's documented initialization; then plain queries
        // into storage of the documented size.
        unsafe {
            if (api.init)() != SUCCESS {
                return None;
            }
            let mut count = 0;
            if (api.count)(&mut count) != SUCCESS || count == 0 {
                (api.shutdown)();
                return None;
            }
            let devices: Vec<Device> = (0..count)
                .filter_map(|index| {
                    let mut device = ptr::null_mut();
                    ((api.handle)(index, &mut device) == SUCCESS).then_some(device)
                })
                .collect();
            let last_seen = vec![0; devices.len()];
            Some(Nvml { api, devices, last_seen })
        }
    }

    pub fn read(&mut self) -> Vec<Reading> {
        let api = &self.api;
        let mut readings = Vec::new();
        for (index, &device) in self.devices.iter().enumerate() {
            let mut reading = Reading::default();
            // SAFETY: NVML queries into storage of the documented size;
            // failures leave the figure out.
            unsafe {
                let mut name = [0 as c_char; 96];
                if (api.name)(device, name.as_mut_ptr(), name.len() as c_uint) == SUCCESS {
                    reading.name = CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned();
                }
                let mut pci: PciInfo = std::mem::zeroed();
                if (api.pci)(device, &mut pci) == SUCCESS {
                    reading.pci = format!("{:04x}:{:02x}:{:02x}.0", pci.domain, pci.bus, pci.device);
                }
                let mut utilization = Utilization::default();
                if (api.utilization)(device, &mut utilization) == SUCCESS {
                    reading.busy = Some(utilization.gpu as f64);
                }
                // v2 separates firmware/driver reservations from allocations,
                // matching nvidia-smi's Used column. v1 includes reservations.
                let mut memory_v2 = MemoryInfoV2 {
                    version: (std::mem::size_of::<MemoryInfoV2>() as c_uint) | (2 << 24),
                    ..Default::default()
                };
                if api.memory_v2.is_some_and(|query| query(device, &mut memory_v2) == SUCCESS) {
                    reading.vram_used = Some(memory_v2.used);
                    reading.vram_total = Some(memory_v2.total);
                } else {
                    let mut memory = MemoryInfo::default();
                    if (api.memory)(device, &mut memory) == SUCCESS {
                        reading.vram_used = Some(memory.used);
                        reading.vram_total = Some(memory.total);
                    }
                }
                let mut milliwatts = 0;
                if (api.power)(device, &mut milliwatts) == SUCCESS {
                    reading.power = Some(milliwatts as f64 / 1000.0);
                }
                let mut millijoules = 0;
                if api.energy.is_some_and(|query| query(device, &mut millijoules) == SUCCESS) {
                    reading.energy = Some(millijoules as f64 / 1000.0);
                }
                let mut celsius = 0;
                if (api.temperature)(device, TEMPERATURE_GPU, &mut celsius) == SUCCESS {
                    reading.temperature = Some(celsius as f64);
                }
                let mut mhz = 0;
                if (api.clock)(device, CLOCK_GRAPHICS, &mut mhz) == SUCCESS {
                    reading.clock = Some(mhz as f64);
                }
                let mut video: Option<f64> = None;
                for query in [api.encoder, api.decoder].into_iter().flatten() {
                    let (mut percent, mut period) = (0, 0);
                    if query(device, &mut percent, &mut period) == SUCCESS {
                        video = Some(video.unwrap_or(0.0).max(percent as f64));
                    }
                }
                reading.video = video;
                let (processes, newest, unlisted_usage) = processes(api, device, self.last_seen[index]);
                reading.processes = processes;
                reading.unlisted_usage = unlisted_usage;
                self.last_seen[index] = newest;
            }
            readings.push(reading);
        }
        readings
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: balances this instance's successful initialization, before dlclose.
        unsafe {
            (self.api.shutdown)();
        }
    }
}

/// Each process's share of one device: its engine samples since the last
/// reading, averaged, and its video memory; and the newest sample's time.
unsafe fn processes(
    api: &Api,
    device: Device,
    last_seen: c_ulonglong,
) -> (Vec<(u32, GpuUsage)>, c_ulonglong, GpuUsage) {
    let mut samples = Vec::new();
    let mut infos = Vec::new();
    let mut utilization_known = false;
    let mut memory_lists = 0;
    let mut newest = last_seen;
    // SAFETY: the NVML calls fill buffers of the size they are given.
    unsafe {
        if let Some(query) = api.samples {
            let mut count: c_uint = 0;
            let first = query(device, ptr::null_mut(), &mut count, last_seen);
            utilization_known = first == NOT_FOUND || first == SUCCESS;
            if (first == INSUFFICIENT_SIZE || first == SUCCESS) && count > 0 {
                for _ in 0..3 {
                    count = count.saturating_add(16);
                    let mut buffer = vec![ProcessSample::default(); count as usize];
                    let result = query(device, buffer.as_mut_ptr(), &mut count, last_seen);
                    if result == SUCCESS {
                        utilization_known = true;
                        buffer.truncate(count as usize);
                        samples = buffer;
                        break;
                    }
                    if result != INSUFFICIENT_SIZE {
                        utilization_known = result == NOT_FOUND;
                        break;
                    }
                }
            }
        }
        // Memory queries are independent of utilization support.
        for query in [api.graphics, api.compute] {
            let mut count: c_uint = 0;
            let first = query(device, &mut count, ptr::null_mut());
            if first != INSUFFICIENT_SIZE && first != SUCCESS {
                continue;
            }
            for _ in 0..3 {
                // Room for processes that start between calls; retry if it grows again.
                count = count.saturating_add(8);
                let mut buffer = vec![ProcessInfo::default(); count as usize];
                let result = query(device, &mut count, buffer.as_mut_ptr());
                if result == SUCCESS {
                    memory_lists += 1;
                    infos.extend(buffer.into_iter().take(count as usize));
                    break;
                }
                if result != INSUFFICIENT_SIZE {
                    break;
                }
            }
        }
    }
    samples.retain(|s| s.time_stamp > last_seen);
    newest = samples.iter().map(|s| s.time_stamp).max().unwrap_or(newest);
    let unlisted_usage = GpuUsage {
        busy: utilization_known.then_some(0.0),
        video: utilization_known.then_some(0.0),
        vram: (memory_lists == 2).then_some(0),
        shared: None,
    };
    (process_shares(&samples, &infos, utilization_known), newest, unlisted_usage)
}

fn process_shares(samples: &[ProcessSample], infos: &[ProcessInfo], utilization_known: bool) -> Vec<(u32, GpuUsage)> {
    let mut shares: std::collections::BTreeMap<u32, (f64, f64, u32, Option<u64>)> = Default::default();
    for sample in samples {
        let entry = shares.entry(sample.pid).or_default();
        entry.0 += sample.sm_util as f64;
        entry.1 += sample.enc_util.max(sample.dec_util) as f64;
        entry.2 += 1;
    }
    for info in infos {
        let entry = shares.entry(info.pid).or_default();
        // The same PID can appear in both graphics and compute lists. Never add
        // those copies, or turn NVML_VALUE_NOT_AVAILABLE into a huge allocation.
        if info.used_gpu_memory != c_ulonglong::MAX {
            entry.3 = Some(entry.3.unwrap_or(0).max(info.used_gpu_memory));
        }
    }
    shares
        .into_iter()
        .map(|(pid, (sm, video, samples, memory))| {
            let n = samples.max(1) as f64;
            (
                pid,
                GpuUsage {
                    busy: utilization_known.then_some((sm / n).min(100.0)),
                    video: utilization_known.then_some((video / n).min(100.0)),
                    vram: memory,
                    shared: None,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_survives_unsupported_utilization_and_duplicate_process_lists() {
        let infos = [
            ProcessInfo { pid: 42, used_gpu_memory: 355 << 20, ..Default::default() },
            ProcessInfo { pid: 42, used_gpu_memory: 355 << 20, ..Default::default() },
            ProcessInfo { pid: 43, used_gpu_memory: u64::MAX, ..Default::default() },
        ];
        let shares = process_shares(&[], &infos, false);
        assert_eq!(shares[0], (42, GpuUsage { busy: None, video: None, vram: Some(355 << 20), shared: None }));
        assert_eq!(shares[1].1.vram, None);
        let idle = process_shares(&[], &infos[..1], true);
        assert_eq!(idle[0].1.busy, Some(0.0), "a supported idle counter is distinct from unavailable");
    }

    #[test]
    fn samples_average_each_pid_and_keep_memory_separate() {
        let samples = [
            ProcessSample { pid: 7, sm_util: 20, enc_util: 10, ..Default::default() },
            ProcessSample { pid: 7, sm_util: 40, dec_util: 30, ..Default::default() },
            ProcessSample { pid: 8, sm_util: 80, ..Default::default() },
        ];
        let shares = process_shares(&samples, &[], true);
        assert_eq!(shares[0].1.busy, Some(30.0));
        assert_eq!(shares[0].1.video, Some(20.0));
        assert_eq!(shares[0].1.vram, None);
        assert_eq!(shares[1].1.busy, Some(80.0));
    }
}
