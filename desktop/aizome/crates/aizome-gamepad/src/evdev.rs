//! Linux's event devices, as much of them as controllers need: what a
//! device has, its events, an exclusive grab and rumble.

use std::ffi::CStr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const EV_FF: u16 = 0x15;
pub const SYN_REPORT: u16 = 0;
pub const SYN_DROPPED: u16 = 3;

pub const KEY_MAX: u16 = 0x2ff;
pub const ABS_MAX: u16 = 0x3f;
pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_Z: u16 = 0x02;
pub const ABS_RX: u16 = 0x03;
pub const ABS_RY: u16 = 0x04;
pub const ABS_RZ: u16 = 0x05;
pub const ABS_GAS: u16 = 0x09;
pub const ABS_BRAKE: u16 = 0x0a;
pub const ABS_HAT0X: u16 = 0x10;
pub const ABS_HAT0Y: u16 = 0x11;
pub const ABS_HAT3Y: u16 = 0x17;

pub const BTN_MISC: u16 = 0x100;
pub const BTN_JOYSTICK: u16 = 0x120;
pub const BTN_SOUTH: u16 = 0x130;
pub const BTN_EAST: u16 = 0x131;
pub const BTN_C: u16 = 0x132;
pub const BTN_NORTH: u16 = 0x133;
pub const BTN_WEST: u16 = 0x134;
pub const BTN_TL: u16 = 0x136;
pub const BTN_TR: u16 = 0x137;
pub const BTN_TL2: u16 = 0x138;
pub const BTN_TR2: u16 = 0x139;
pub const BTN_SELECT: u16 = 0x13a;
pub const BTN_START: u16 = 0x13b;
pub const BTN_MODE: u16 = 0x13c;
pub const BTN_THUMBL: u16 = 0x13d;
pub const BTN_THUMBR: u16 = 0x13e;
pub const BTN_TOUCH: u16 = 0x14a;
pub const BTN_TOOL_FINGER: u16 = 0x145;
pub const BTN_DPAD_UP: u16 = 0x220;
pub const BTN_DPAD_DOWN: u16 = 0x221;
pub const BTN_DPAD_LEFT: u16 = 0x222;
pub const BTN_DPAD_RIGHT: u16 = 0x223;
/// The letter keys' first: devices that have it are keyboards.
pub const KEY_Q: u16 = 16;

pub const INPUT_PROP_POINTER: u16 = 0x00;
pub const INPUT_PROP_ACCELEROMETER: u16 = 0x06;

pub const FF_RUMBLE: u16 = 0x50;

/// `struct input_event` on a 64-bit system.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputEvent {
    pub sec: i64,
    pub usec: i64,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputId {
    pub bustype: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbsInfo {
    pub value: i32,
    pub minimum: i32,
    pub maximum: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

/// `struct ff_effect` with a rumble in its union (48 bytes on 64-bit).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FfRumbleEffect {
    kind: u16,
    id: i16,
    direction: u16,
    trigger_button: u16,
    trigger_interval: u16,
    replay_length: u16,
    replay_delay: u16,
    _pad: u16,
    strong_magnitude: u16,
    weak_magnitude: u16,
    _union_rest: [u8; 28],
}

pub const BUS_USB: u16 = 0x03;
pub const BUS_BLUETOOTH: u16 = 0x05;

const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;

const fn ioc(dir: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | ((b'E' as u64) << 8) | nr
}

const EVIOCGID: u64 = ioc(IOC_READ, 0x02, std::mem::size_of::<InputId>() as u64);
const EVIOCGRAB: u64 = ioc(IOC_WRITE, 0x90, std::mem::size_of::<libc::c_int>() as u64);
const EVIOCSFF: u64 = ioc(IOC_WRITE, 0x80, std::mem::size_of::<FfRumbleEffect>() as u64);
const EVIOCRMFF: u64 = ioc(IOC_WRITE, 0x81, std::mem::size_of::<libc::c_int>() as u64);

const fn eviocgname(len: u64) -> u64 {
    ioc(IOC_READ, 0x06, len)
}
const fn eviocguniq(len: u64) -> u64 {
    ioc(IOC_READ, 0x08, len)
}
const fn eviocgprop(len: u64) -> u64 {
    ioc(IOC_READ, 0x09, len)
}
const fn eviocgkey(len: u64) -> u64 {
    ioc(IOC_READ, 0x18, len)
}
const fn eviocgbit(ev: u64, len: u64) -> u64 {
    ioc(IOC_READ, 0x20 + ev, len)
}
const fn eviocgabs(abs: u64) -> u64 {
    ioc(IOC_READ, 0x40 + abs, std::mem::size_of::<AbsInfo>() as u64)
}

/// A set of event codes, as the kernel's bit arrays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bits(pub Vec<u8>);

impl Bits {
    pub fn has(&self, code: u16) -> bool {
        self.0.get(usize::from(code / 8)).is_some_and(|b| b & (1 << (code % 8)) != 0)
    }

    pub fn with(codes: &[u16]) -> Bits {
        let mut b = Bits(vec![0; 0x300 / 8]);
        for &c in codes {
            b.0[usize::from(c / 8)] |= 1 << (c % 8);
        }
        b
    }

    pub fn codes(&self) -> impl Iterator<Item = u16> + '_ {
        (0..(self.0.len() * 8) as u16).filter(|c| self.has(*c))
    }
}

/// What a device says it is and has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    pub name: String,
    /// Bluetooth devices' address, USB devices' serial.
    pub uniq: String,
    pub id: InputId,
    pub keys: Bits,
    pub abs: Bits,
    /// Each axis's range, by code.
    pub absinfo: Vec<(u16, AbsInfo)>,
    pub props: Bits,
    pub rumble: bool,
    /// The kernel driver: `xpad`, `playstation`, `nintendo`, `hid-generic`…
    pub driver: String,
}

impl Caps {
    /// Whether it's a game controller: buttons in the joystick or gamepad
    /// ranges, a stick or a pad of arrows; not a keyboard, touchpad or
    /// motion sensor.
    pub fn is_controller(&self) -> bool {
        let buttons = (BTN_JOYSTICK..=BTN_THUMBR).any(|c| self.keys.has(c));
        let steers =
            (self.abs.has(ABS_X) && self.abs.has(ABS_Y)) || self.abs.has(ABS_HAT0X) || self.keys.has(BTN_DPAD_UP);
        let keyboard = self.keys.has(KEY_Q);
        let touch = self.keys.has(BTN_TOUCH) || self.keys.has(BTN_TOOL_FINGER) || self.props.has(INPUT_PROP_POINTER);
        let motion = self.props.has(INPUT_PROP_ACCELEROMETER);
        buttons && steers && !keyboard && !touch && !motion
    }

    pub fn range(&self, code: u16) -> Option<AbsInfo> {
        self.absinfo.iter().find(|(c, _)| *c == code).map(|(_, a)| *a)
    }

    pub fn bluetooth(&self) -> bool {
        self.id.bustype == BUS_BLUETOOTH
    }
}

/// An open event device.
pub struct Device {
    pub fd: OwnedFd,
    pub path: PathBuf,
    pub caps: Caps,
    rumble_effect: Option<i16>,
}

impl Device {
    /// Open `path` (`/dev/input/eventN`) without blocking: write access
    /// too where allowed, for rumble.
    pub fn open(path: &Path) -> std::io::Result<Device> {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(std::io::Error::other)?;
        let flags = libc::O_NONBLOCK | libc::O_CLOEXEC;
        // SAFETY: a valid C string; the descriptor is owned at once.
        let mut fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | flags) };
        let mut writable = true;
        if fd < 0 {
            // SAFETY: as above.
            fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | flags) };
            writable = false;
        }
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: just opened, owned here alone.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut caps = caps_of(fd.as_raw_fd())?;
        caps.rumble &= writable;
        caps.driver = driver_of(path);
        Ok(Device { fd, path: path.to_owned(), caps, rumble_effect: None })
    }

    /// Read what's waiting, into `out`: false once the device is gone.
    pub fn read(&self, out: &mut Vec<InputEvent>) -> bool {
        let mut buf = [InputEvent::default(); 64];
        loop {
            let size = std::mem::size_of_val(&buf);
            // SAFETY: reading into a buffer of `size` bytes.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), size) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                return matches!(e.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EINTR));
            }
            if n == 0 {
                return false;
            }
            let count = n as usize / std::mem::size_of::<InputEvent>();
            out.extend_from_slice(&buf[..count]);
            if (n as usize) < size {
                return true;
            }
        }
    }

    /// Take (or give back) its events from everyone else: browsers'
    /// Gamepad API included.
    pub fn grab(&self, on: bool) -> bool {
        // SAFETY: EVIOCGRAB takes an int by value.
        unsafe { libc::ioctl(self.fd.as_raw_fd(), EVIOCGRAB as _, libc::c_int::from(on)) == 0 }
    }

    /// The buttons held now, after a SYN_DROPPED.
    pub fn key_state(&self) -> Bits {
        let mut b = vec![0u8; usize::from(KEY_MAX / 8 + 1)];
        // SAFETY: the buffer is the size the request says.
        unsafe { libc::ioctl(self.fd.as_raw_fd(), eviocgkey(b.len() as u64) as _, b.as_mut_ptr()) };
        Bits(b)
    }

    /// Each axis now.
    pub fn abs_now(&self, code: u16) -> Option<AbsInfo> {
        abs_info(self.fd.as_raw_fd(), code)
    }

    /// Rumble at `strong` and `weak` (0–65535) for `ms`, replacing what
    /// rumbled before; zero both to stop.
    pub fn rumble(&mut self, strong: u16, weak: u16, ms: u16) {
        if !self.caps.rumble {
            return;
        }
        let fd = self.fd.as_raw_fd();
        if strong == 0 && weak == 0 {
            if let Some(id) = self.rumble_effect.take() {
                // SAFETY: EVIOCRMFF takes the effect's id by value.
                unsafe { libc::ioctl(fd, EVIOCRMFF as _, libc::c_int::from(id)) };
            }
            return;
        }
        let mut effect = FfRumbleEffect {
            kind: FF_RUMBLE,
            id: self.rumble_effect.unwrap_or(-1),
            replay_length: ms,
            strong_magnitude: strong,
            weak_magnitude: weak,
            ..FfRumbleEffect::default()
        };
        // SAFETY: EVIOCSFF reads and writes back (the id) the struct.
        if unsafe { libc::ioctl(fd, EVIOCSFF as _, &mut effect) } != 0 {
            return;
        }
        self.rumble_effect = Some(effect.id);
        let play = InputEvent { kind: EV_FF, code: effect.id as u16, value: 1, ..InputEvent::default() };
        // SAFETY: writing one event struct.
        unsafe { libc::write(fd, (&play as *const InputEvent).cast(), std::mem::size_of::<InputEvent>()) };
    }
}

fn ioctl_string(fd: RawFd, request: u64) -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the request's length is the buffer's.
    let n = unsafe { libc::ioctl(fd, request as _, buf.as_mut_ptr()) };
    if n <= 0 {
        return String::new();
    }
    CStr::from_bytes_until_nul(&buf).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()
}

fn bits(fd: RawFd, ev: u16, len: usize) -> Bits {
    let mut b = vec![0u8; len];
    // SAFETY: the request's length is the buffer's.
    let n = unsafe { libc::ioctl(fd, eviocgbit(u64::from(ev), len as u64) as _, b.as_mut_ptr()) };
    if n < 0 {
        b.fill(0);
    }
    Bits(b)
}

fn abs_info(fd: RawFd, code: u16) -> Option<AbsInfo> {
    let mut info = AbsInfo::default();
    // SAFETY: EVIOCGABS fills an absinfo.
    (unsafe { libc::ioctl(fd, eviocgabs(u64::from(code)) as _, &mut info) } == 0).then_some(info)
}

fn caps_of(fd: RawFd) -> std::io::Result<Caps> {
    let mut id = InputId::default();
    // SAFETY: EVIOCGID fills an input_id.
    if unsafe { libc::ioctl(fd, EVIOCGID as _, &mut id) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let types = bits(fd, 0, 4);
    let keys = if types.has(EV_KEY) { bits(fd, EV_KEY, usize::from(KEY_MAX / 8 + 1)) } else { Bits::default() };
    let abs = if types.has(EV_ABS) { bits(fd, EV_ABS, usize::from(ABS_MAX / 8 + 1)) } else { Bits::default() };
    let ff = if types.has(EV_FF) { bits(fd, EV_FF, 16) } else { Bits::default() };
    let mut props = vec![0u8; 4];
    // SAFETY: the request's length is the buffer's.
    unsafe { libc::ioctl(fd, eviocgprop(4) as _, props.as_mut_ptr()) };
    let absinfo = abs.codes().filter_map(|c| abs_info(fd, c).map(|a| (c, a))).collect();
    Ok(Caps {
        name: ioctl_string(fd, eviocgname(256)),
        uniq: ioctl_string(fd, eviocguniq(256)),
        id,
        keys,
        abs,
        absinfo,
        props: Bits(props),
        rumble: ff.has(FF_RUMBLE),
        driver: String::new(),
    })
}

/// The driver of the device under an event node, from sysfs.
fn driver_of(path: &Path) -> String {
    let Some(name) = path.file_name() else { return String::new() };
    let link = Path::new("/sys/class/input").join(name).join("device/device/driver");
    std::fs::read_link(link)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// The battery of the device under an event node: (percent, charging),
/// from the power supply its HID device registers.
pub fn battery_of(path: &Path) -> Option<(u8, bool)> {
    let name = path.file_name()?;
    let dir = Path::new("/sys/class/input").join(name).join("device/device/power_supply");
    let supply = std::fs::read_dir(dir).ok()?.flatten().next()?.path();
    let read = |f: &str| std::fs::read_to_string(supply.join(f)).ok().map(|s| s.trim().to_owned());
    let percent = read("capacity").and_then(|s| s.parse::<u8>().ok()).or_else(|| {
        // Some drivers say only a level.
        Some(match read("capacity_level")?.as_str() {
            "Full" => 100,
            "High" => 80,
            "Normal" => 55,
            "Low" => 20,
            "Critical" => 5,
            _ => return None,
        })
    })?;
    let charging = read("status").is_some_and(|s| s == "Charging" || s == "Full");
    Some((percent.min(100), charging))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_have_the_kernels_numbers() {
        // From <linux/input.h> on x86-64.
        assert_eq!(EVIOCGID, 0x8008_4502);
        assert_eq!(EVIOCGRAB, 0x4004_4590);
        assert_eq!(EVIOCSFF, 0x4030_4580);
        assert_eq!(eviocgname(256), 0x8100_4506);
        assert_eq!(eviocgbit(u64::from(EV_KEY), 96), 0x8060_4521);
        assert_eq!(eviocgabs(u64::from(ABS_Y)), 0x8018_4541);
        assert_eq!(std::mem::size_of::<InputEvent>(), 24);
        assert_eq!(std::mem::size_of::<FfRumbleEffect>(), 48);
    }

    #[test]
    fn a_touchpad_or_keyboard_is_no_controller() {
        let pad = Caps {
            keys: Bits::with(&[BTN_SOUTH, BTN_EAST, BTN_START]),
            abs: Bits::with(&[ABS_X, ABS_Y]),
            ..Caps::default()
        };
        assert!(pad.is_controller());
        let touch = Caps { keys: Bits::with(&[BTN_SOUTH, BTN_TOUCH]), ..pad.clone() };
        assert!(!touch.is_controller());
        let keyboard = Caps { keys: Bits::with(&[BTN_SOUTH, KEY_Q]), ..pad.clone() };
        assert!(!keyboard.is_controller());
        let sensors = Caps { keys: Bits::default(), ..pad.clone() };
        assert!(!sensors.is_controller());
    }
}
