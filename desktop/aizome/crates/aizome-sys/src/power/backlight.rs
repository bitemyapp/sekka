//! Screen brightness: backlights from `/sys/class/backlight`, read from
//! sysfs and set through logind's `Session.SetBrightness`, which needs no
//! root and no polkit prompt for the active session's user.
//!
//! A thread re-reads the levels when the kernel flags a change (sysfs
//! notifies `actual_brightness` when a hotkey changes it) and at least once
//! a second, so changes made elsewhere show too.

use super::logind::{IFACE_SESSION, LOGIN1, session_path};
use crate::bus::{Bus, Opts};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, lock, service_handle};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SYSFS: &str = "/sys/class/backlight";
/// Brightness keys never go below this fraction: an unlit panel looks
/// like a dead one.
pub const MIN_STEP_FRACTION: f32 = 0.01;

/// The backlights.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BacklightState {
    pub devices: Vec<BacklightDevice>,
}

impl BacklightState {
    /// The backlight to control: firmware interfaces first, then platform,
    /// then raw (the kernel's recommendation).
    pub fn primary(&self) -> Option<&BacklightDevice> {
        self.devices.iter().min_by_key(|d| match d.kind.as_str() {
            "firmware" => 0,
            "platform" => 1,
            _ => 2,
        })
    }
}

/// One backlight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BacklightDevice {
    /// Its sysfs name (`intel_backlight`, `amdgpu_bl0`, `acpi_video0`).
    pub name: String,
    /// `firmware`, `platform` or `raw`.
    pub kind: String,
    pub brightness: u32,
    pub max_brightness: u32,
}

impl BacklightDevice {
    /// The level as a fraction, 0.0–1.0.
    pub fn fraction(&self) -> f32 {
        if self.max_brightness == 0 { 0.0 } else { self.brightness as f32 / self.max_brightness as f32 }
    }

    /// The raw level for a fraction.
    pub fn level_for(&self, fraction: f32) -> u32 {
        let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
        (fraction * self.max_brightness as f32).round() as u32
    }
}

/// Read every backlight under `root`.
pub(crate) fn read(root: &Path) -> Vec<BacklightDevice> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let number =
        |dir: &Path, name: &str| -> Option<u32> { std::fs::read_to_string(dir.join(name)).ok()?.trim().parse().ok() };
    let mut devices: Vec<BacklightDevice> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let max_brightness = number(&dir, "max_brightness")?;
            // What the hardware shows, but a panel that's off (blanked, the
            // lid shut) reads 0 there: then the level set, which it comes
            // back at and which the keys step from.
            let brightness = match (number(&dir, "actual_brightness"), number(&dir, "brightness")) {
                (Some(0), Some(set)) => set,
                (Some(actual), _) => actual,
                (None, set) => set?,
            };
            Some(BacklightDevice {
                name: entry.file_name().to_string_lossy().into_owned(),
                kind: std::fs::read_to_string(dir.join("type"))
                    .map(|t| t.trim().to_owned())
                    .unwrap_or_else(|_| "raw".into()),
                brightness: brightness.min(max_brightness),
                max_brightness,
            })
        })
        .filter(|d| d.max_brightness > 0)
        .collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

/// The next level for a brightness key: a step of `step` (a fraction of
/// the maximum), landing on the step grid, never below
/// [`MIN_STEP_FRACTION`].
pub fn stepped(device: &BacklightDevice, step: f32) -> u32 {
    if device.max_brightness == 0 || !step.is_finite() || step == 0.0 {
        return device.brightness;
    }
    let size = step.abs();
    let steps = device.fraction() / size;
    let near = steps.round();
    let base = if (steps - near).abs() < 0.02 {
        near
    } else if step > 0.0 {
        steps.floor()
    } else {
        steps.ceil()
    };
    let fraction = ((base + step.signum()) * size).clamp(MIN_STEP_FRACTION, 1.0);
    device.level_for(fraction).max(1)
}

/// Screen backlights.
pub struct Backlight {
    bus: Option<Bus>,
    session: Arc<Mutex<Option<String>>>,
    root: PathBuf,
    model: Arc<Model<BacklightState>>,
    stop: Stop,
}

service_handle!(Backlight, BacklightState);

impl Backlight {
    /// The machine's backlights, set through this process's logind session.
    pub fn new(wake: Wake) -> Backlight {
        Backlight::with(Bus::system().ok(), PathBuf::from(SYSFS), None, wake)
    }

    /// Backlights under `root` (a `/sys/class/backlight`), set through the
    /// logind session at `session` (an object path; found when `None`).
    pub fn with(bus: Option<Bus>, root: PathBuf, session: Option<String>, wake: Wake) -> Backlight {
        let model = Arc::new(Model::new(BacklightState { devices: read(&root) }, wake));
        let stop = Stop::default();
        let session = Arc::new(Mutex::new(session));
        {
            let model = model.clone();
            let root = root.clone();
            let stop = stop.clone();
            let _ = std::thread::Builder::new()
                .name("aizome-sys backlight".into())
                .spawn(move || watch(&root, &model, &stop));
        }
        Backlight { bus, session, root, model, stop }
    }

    /// Whether there is a backlight to control.
    pub fn available(&self) -> bool {
        !self.model.get().devices.is_empty()
    }

    /// Re-read the levels now.
    pub fn refresh(&self) {
        self.model.set_if_changed(BacklightState { devices: read(&self.root) });
    }

    fn session(&self, bus: &Bus) -> Result<String> {
        let mut cached = lock(&self.session);
        if let Some(path) = cached.as_ref() {
            return Ok(path.clone());
        }
        let path = session_path(bus).ok_or_else(|| Error::NotFound("no login session".into()))?;
        *cached = Some(path.clone());
        Ok(path)
    }

    /// Set a backlight's raw level (0–its maximum); the result is the new
    /// fraction.
    pub fn set_brightness(&self, name: &str, level: u32) -> Task<f32> {
        let Some(device) = self.model.get().devices.iter().find(|d| d.name == name).cloned() else {
            return Task::ready(Err(Error::NotFound(name.to_owned())));
        };
        let level = level.min(device.max_brightness);
        self.apply(device, level)
    }

    /// Set a backlight to a fraction of its maximum.
    pub fn set_fraction(&self, name: &str, fraction: f32) -> Task<f32> {
        let Some(device) = self.model.get().devices.iter().find(|d| d.name == name).cloned() else {
            return Task::ready(Err(Error::NotFound(name.to_owned())));
        };
        let level = device.level_for(fraction);
        self.apply(device, level)
    }

    /// The brightness keys: step the primary backlight by `step` (`0.05`,
    /// `-0.05`). The snapshot shows the new level at once; the result is
    /// the new fraction.
    pub fn step(&self, step: f32) -> Task<f32> {
        let Some(device) = self.model.get().primary().cloned() else {
            return Task::ready(Err(Error::NotFound("no backlight".into())));
        };
        let level = stepped(&device, step);
        self.apply(device, level)
    }

    /// Show `level` at once, then ask logind to set it.
    fn apply(&self, device: BacklightDevice, level: u32) -> Task<f32> {
        let Some(bus) = self.bus.clone() else {
            return Task::ready(Err(Error::Unavailable("logind".into())));
        };
        let session = match self.session(&bus) {
            Ok(session) => session,
            Err(error) => return Task::ready(Err(error)),
        };
        self.model.update(|state| {
            if let Some(d) = state.devices.iter_mut().find(|d| d.name == device.name) {
                d.brightness = level;
            }
        });
        let fraction = BacklightDevice { brightness: level, ..device.clone() }.fraction();
        Task::spawn("backlight", &self.model.wake(), move || {
            bus.call_with::<_, ()>(
                Opts::QUERY,
                LOGIN1,
                &session,
                IFACE_SESSION,
                "SetBrightness",
                &("backlight", device.name.as_str(), level),
            )?;
            Ok(fraction)
        })
    }
}

impl Drop for Backlight {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// Re-read on each kernel notification, and once a second.
fn watch(root: &Path, model: &Model<BacklightState>, stop: &Stop) {
    use std::io::{Read, Seek};
    while !stop.stopped() {
        let mut files: Vec<std::fs::File> = std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| std::fs::File::open(entry.path().join("actual_brightness")).ok())
            .collect();
        // Reading arms sysfs's notification.
        let mut buf = String::new();
        for file in &mut files {
            buf.clear();
            let _ = file.rewind();
            let _ = file.read_to_string(&mut buf);
        }
        let mut fds: Vec<libc::pollfd> = files
            .iter()
            .map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLPRI | libc::POLLERR, revents: 0 })
            .collect();
        if fds.is_empty() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        } else {
            // SAFETY: valid pollfds for the duration of the call.
            unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 1000) };
        }
        model.set_if_changed(BacklightState { devices: read(root) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(brightness: u32, max: u32) -> BacklightDevice {
        BacklightDevice { name: "b".into(), kind: "raw".into(), brightness, max_brightness: max }
    }

    #[test]
    fn keys_step_on_the_grid_and_never_go_dark() {
        assert_eq!(stepped(&device(50, 100), 0.05), 55);
        assert_eq!(stepped(&device(48, 100), 0.05), 50);
        assert_eq!(stepped(&device(48, 100), -0.05), 45);
        assert_eq!(stepped(&device(3, 100), -0.05), 1);
        assert_eq!(stepped(&device(100, 100), 0.05), 100);
        assert_eq!(stepped(&device(25, 496), 0.05), 50);
    }

    #[test]
    fn sysfs_backlights_and_the_primary_one() {
        let root = std::env::temp_dir().join(format!("aizome-sys-backlight-{}", std::process::id()));
        for (name, kind, now) in [("intel_backlight", "raw", "25"), ("acpi_video0", "firmware", "7")] {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("max_brightness"), if kind == "raw" { "496\n" } else { "15\n" }).unwrap();
            std::fs::write(dir.join("actual_brightness"), format!("{now}\n")).unwrap();
            std::fs::write(dir.join("type"), format!("{kind}\n")).unwrap();
        }
        std::fs::create_dir_all(root.join("broken")).unwrap();
        let state = BacklightState { devices: read(&root) };
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(state.devices.len(), 2, "a device without levels is skipped");
        assert_eq!(state.primary().unwrap().name, "acpi_video0");
        let raw = state.devices.iter().find(|d| d.kind == "raw").unwrap();
        assert!((raw.fraction() - 25.0 / 496.0).abs() < 1e-6);
    }

    #[test]
    fn a_panel_that_is_off_reads_the_level_set() {
        // Blanked or with the lid shut, intel_backlight reads 0 actual:
        // the keys step from the level it comes back at, not from dark.
        let root = std::env::temp_dir().join(format!("aizome-sys-backlight-off-{}", std::process::id()));
        let dir = root.join("intel_backlight");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("max_brightness"), "496\n").unwrap();
        std::fs::write(dir.join("brightness"), "124\n").unwrap();
        std::fs::write(dir.join("actual_brightness"), "0\n").unwrap();
        let devices = read(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(devices[0].brightness, 124);
        assert_eq!(stepped(&devices[0], 0.05), 149);
    }
}
