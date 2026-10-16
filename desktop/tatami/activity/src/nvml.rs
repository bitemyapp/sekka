// SPDX-License-Identifier: MIT OR Apache-2.0
//! NVIDIA graphics through NVML, the library nvidia-smi uses, opened at run
//! time so the window needs no NVIDIA driver to start. NixOS puts it in
//! /run/opengl-driver/lib. NVIDIA's driver reports no per-process figures
//! in DRM's fdinfo (gpu.rs), so NVML gives each process's share of the
//! graphics processor and its video memory, and the card's power meter.
use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_ulonglong, c_void},
    ptr,
};

type Return = c_int;
type Device = *mut c_void;

const SUCCESS: Return = 0;
const INSUFFICIENT_SIZE: Return = 7;
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
    _library: *mut c_void,
    init: unsafe extern "C" fn() -> Return,
    count: unsafe extern "C" fn(*mut c_uint) -> Return,
    handle: unsafe extern "C" fn(c_uint, *mut Device) -> Return,
    name: unsafe extern "C" fn(Device, *mut c_char, c_uint) -> Return,
    pci: unsafe extern "C" fn(Device, *mut PciInfo) -> Return,
    utilization: unsafe extern "C" fn(Device, *mut Utilization) -> Return,
    memory: unsafe extern "C" fn(Device, *mut MemoryInfo) -> Return,
    power: unsafe extern "C" fn(Device, *mut c_uint) -> Return,
    energy: Option<unsafe extern "C" fn(Device, *mut c_ulonglong) -> Return>,
    temperature: unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> Return,
    clock: unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> Return,
    encoder: Option<unsafe extern "C" fn(Device, *mut c_uint, *mut c_uint) -> Return>,
    decoder: Option<unsafe extern "C" fn(Device, *mut c_uint, *mut c_uint) -> Return>,
    samples: unsafe extern "C" fn(Device, *mut ProcessSample, *mut c_uint, c_ulonglong) -> Return,
    graphics: unsafe extern "C" fn(Device, *mut c_uint, *mut ProcessInfo) -> Return,
    compute: unsafe extern "C" fn(Device, *mut c_uint, *mut ProcessInfo) -> Return,
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
        let names: [&CStr; 2] = [
            c"libnvidia-ml.so.1",
            c"/run/opengl-driver/lib/libnvidia-ml.so.1",
        ];
        // SAFETY: dlopen with a valid C string; the handle is kept open.
        let library = names
            .iter()
            .map(|name| unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) })
            .find(|handle| !handle.is_null())?;
        // SAFETY: the symbols are NVML's, with these signatures (nvml.h).
        unsafe {
            Some(Api {
                _library: library,
                init: symbol(library, c"nvmlInit_v2")?,
                count: symbol(library, c"nvmlDeviceGetCount_v2")?,
                handle: symbol(library, c"nvmlDeviceGetHandleByIndex_v2")?,
                name: symbol(library, c"nvmlDeviceGetName")?,
                pci: symbol(library, c"nvmlDeviceGetPciInfo_v3")?,
                utilization: symbol(library, c"nvmlDeviceGetUtilizationRates")?,
                memory: symbol(library, c"nvmlDeviceGetMemoryInfo")?,
                power: symbol(library, c"nvmlDeviceGetPowerUsage")?,
                energy: symbol(library, c"nvmlDeviceGetTotalEnergyConsumption"),
                temperature: symbol(library, c"nvmlDeviceGetTemperature")?,
                clock: symbol(library, c"nvmlDeviceGetClockInfo")?,
                encoder: symbol(library, c"nvmlDeviceGetEncoderUtilization"),
                decoder: symbol(library, c"nvmlDeviceGetDecoderUtilization"),
                samples: symbol(library, c"nvmlDeviceGetProcessUtilization")?,
                graphics: symbol(library, c"nvmlDeviceGetGraphicsRunningProcesses_v3")?,
                compute: symbol(library, c"nvmlDeviceGetComputeRunningProcesses_v3")?,
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
    /// Joules since the driver loaded, where the card counts them.
    pub energy: Option<f64>,
    pub temperature: Option<f64>,
    pub clock: Option<f64>,
    /// Each process's share: PID, percent of the 3D/compute engines,
    /// percent of video, video memory.
    pub processes: Vec<(i32, f64, f64, u64)>,
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
                return None;
            }
            let devices: Vec<Device> = (0..count)
                .filter_map(|index| {
                    let mut device = ptr::null_mut();
                    ((api.handle)(index, &mut device) == SUCCESS).then_some(device)
                })
                .collect();
            let last_seen = vec![0; devices.len()];
            Some(Nvml {
                api,
                devices,
                last_seen,
            })
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
                    reading.pci =
                        format!("{:04x}:{:02x}:{:02x}.0", pci.domain, pci.bus, pci.device);
                }
                let mut utilization = Utilization::default();
                if (api.utilization)(device, &mut utilization) == SUCCESS {
                    reading.busy = Some(utilization.gpu as f64);
                }
                let mut memory = MemoryInfo::default();
                if (api.memory)(device, &mut memory) == SUCCESS {
                    reading.vram_used = Some(memory.used);
                    reading.vram_total = Some(memory.total);
                }
                let mut milliwatts = 0;
                if (api.power)(device, &mut milliwatts) == SUCCESS {
                    reading.power = Some(milliwatts as f64 / 1000.0);
                }
                let mut millijoules = 0;
                if let Some(energy) = api.energy
                    && energy(device, &mut millijoules) == SUCCESS
                {
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
                let (processes, newest) = processes(api, device, self.last_seen[index]);
                reading.processes = processes;
                self.last_seen[index] = newest;
            }
            readings.push(reading);
        }
        readings
    }
}

/// Each process's share of one device: its engine samples since the last
/// reading, averaged, and its video memory; and the newest sample's time.
unsafe fn processes(
    api: &Api,
    device: Device,
    last_seen: c_ulonglong,
) -> (Vec<(i32, f64, f64, u64)>, c_ulonglong) {
    let mut shares: std::collections::BTreeMap<i32, (f64, f64, u32, u64)> = Default::default();
    let mut newest = last_seen;
    // SAFETY: the NVML calls fill buffers of the size they are given.
    unsafe {
        {
            let mut count: c_uint = 0;
            let first = (api.samples)(device, ptr::null_mut(), &mut count, last_seen);
            if (first == INSUFFICIENT_SIZE || first == SUCCESS) && count > 0 {
                let mut samples = vec![ProcessSample::default(); count as usize];
                if (api.samples)(device, samples.as_mut_ptr(), &mut count, last_seen) == SUCCESS {
                    samples.truncate(count as usize);
                    newest = samples
                        .iter()
                        .map(|s| s.time_stamp)
                        .max()
                        .unwrap_or(last_seen);
                    for sample in samples {
                        let entry = shares.entry(sample.pid as i32).or_default();
                        entry.0 += sample.sm_util as f64;
                        entry.1 += sample.enc_util.max(sample.dec_util) as f64;
                        entry.2 += 1;
                    }
                }
            }
            for query in [api.graphics, api.compute] {
                let mut count: c_uint = 0;
                let first = query(device, &mut count, ptr::null_mut());
                if first != INSUFFICIENT_SIZE && first != SUCCESS {
                    continue;
                }
                // Room for processes that start between the two calls.
                count += 8;
                let mut infos = vec![ProcessInfo::default(); count as usize];
                if query(device, &mut count, infos.as_mut_ptr()) != SUCCESS {
                    continue;
                }
                for info in infos.into_iter().take(count as usize) {
                    let memory = info.used_gpu_memory;
                    // NVML_VALUE_NOT_AVAILABLE is all ones.
                    let memory = if memory == c_ulonglong::MAX {
                        0
                    } else {
                        memory
                    };
                    let entry = shares.entry(info.pid as i32).or_default();
                    entry.3 = entry.3.max(memory);
                }
            }
        }
    }
    let shares = shares
        .into_iter()
        .map(|(pid, (sm, video, samples, memory))| {
            let n = samples.max(1) as f64;
            (pid, (sm / n).min(100.0), (video / n).min(100.0), memory)
        })
        .collect();
    (shares, newest)
}
