//! Apple Studio Displays' brightness, over USB HID.
//!
//! The Studio Display has no DDC/CI. Its USB interface 7 is a HID monitor
//! control (USB HID Usage Tables, Monitor page 0x80, usage 0x01) whose
//! feature report 1 carries VESA Virtual Controls brightness (page 0x82,
//! usage 0x10, the MCCS luminance code as a HID usage): 32 bits from 400
//! to 60000, in hundredths of a candela per square metre, so 4 to 600
//! nits on the 2022 model. A 16-bit vendor field follows it, which writes
//! leave at 0. The kernel reports that descriptor in sysfs
//! (`report_descriptor`, readable by anyone), and the level is read and
//! set as feature reports through `/dev/hidrawN` (`HIDIOCGFEATURE`,
//! `HIDIOCSFEATURE`).
//!
//! Sources: juliuszint/asdbctl (MIT; its README decodes the descriptor
//! above, and `src/main.rs` at 4f0e081, 2026-06-26, lists the product IDs
//! 0x1114 for the 2022 Studio Display, 0x1116 for the Studio Display XDR
//! and 0x1118 for the 2026 Studio Display, interface 7, report 1 of 7
//! bytes, little-endian); nikosdion/asdcontrol (the same range through
//! hiddev); and the proposed kernel driver "backlight: hid_bl: Add VESA
//! VCP HID backlight driver" (LKML, 2023-08, v3), which finds the field by
//! those usages. This module finds the field by its usages too, and takes
//! the range from the descriptor, rather than assuming asdbctl's numbers
//! for every model.

use std::io;
use std::os::fd::AsRawFd;

/// Apple's USB vendor ID.
pub const APPLE: u16 = 0x05ac;

/// The Studio Displays whose HID brightness is known (asdbctl's list).
pub const STUDIO_DISPLAYS: [(u16, &str); 3] =
    [(0x1114, "Studio Display (2022)"), (0x1116, "Studio Display XDR"), (0x1118, "Studio Display (2026)")];

/// The USB interface that carries the brightness control on every model
/// asdbctl knows. Discovery reads every interface's descriptor anyway;
/// the udev rule grants access to this one only.
pub const INTERFACE: u8 = 7;

const MONITOR_CONTROL: u32 = 0x0080_0001;
const VESA_BRIGHTNESS: u32 = 0x0082_0010;

/// Where brightness is in a feature report, and its range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub report_id: u8,
    /// Whether the device numbers its reports (the first byte is the ID).
    pub numbered: bool,
    /// The field's first bit after the report ID.
    pub bit_offset: u32,
    pub bits: u32,
    pub min: i64,
    pub max: i64,
    /// The whole feature report, in bytes, with its ID.
    pub report_len: usize,
}

impl Field {
    /// The value in a feature report read from the device.
    pub fn read(&self, report: &[u8]) -> Option<i64> {
        let payload = report.get(usize::from(self.numbered)..)?;
        let mut value: u64 = 0;
        for b in 0..self.bits {
            let at = self.bit_offset + b;
            let byte = *payload.get((at / 8) as usize)?;
            value |= u64::from((byte >> (at % 8)) & 1) << b;
        }
        if self.min < 0 && self.bits < 64 && value & (1 << (self.bits - 1)) != 0 {
            return Some(value as i64 - (1i64 << self.bits));
        }
        Some(value as i64)
    }

    /// A feature report setting the field to `value` (clamped to the
    /// range), its other fields 0.
    pub fn report(&self, value: i64) -> Vec<u8> {
        let value = value.clamp(self.min, self.max) as u64;
        let mut report = vec![0u8; self.report_len];
        if self.numbered {
            report[0] = self.report_id;
        }
        let start = usize::from(self.numbered);
        for b in 0..self.bits {
            if (value >> b) & 1 == 1 {
                let at = self.bit_offset + b;
                report[start + (at / 8) as usize] |= 1 << (at % 8);
            }
        }
        report
    }
}

#[derive(Clone, Copy, Default)]
struct Globals {
    usage_page: u32,
    logical_min: i64,
    /// Logical maximum as signed and as unsigned: which applies depends on
    /// the minimum's sign, as the kernel reads it.
    logical_max: (i64, i64),
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

/// The brightness feature in a HID report descriptor: a feature field
/// with the VESA brightness usage inside a Monitor Control application
/// collection.
pub fn brightness_field(desc: &[u8]) -> Option<Field> {
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    let mut usages: Vec<u32> = Vec::new();
    let mut range: (Option<u32>, Option<u32>) = (None, None);
    // Each open collection: whether it is an application, and its usage.
    let mut collections: Vec<(bool, u32)> = Vec::new();
    // Bits of each feature report so far.
    let mut feature_bits: Vec<(u8, u32)> = Vec::new();
    let mut numbered = false;
    let mut found: Option<Field> = None;
    let mut i = 0;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == 0xfe {
            // A long item: size, tag, data.
            i += 3 + *desc.get(i + 1)? as usize;
            continue;
        }
        let size = [0, 1, 2, 4][(prefix & 3) as usize];
        let data = desc.get(i + 1..i + 1 + size)?;
        i += 1 + size;
        let unsigned = data.iter().rev().fold(0u32, |a, b| (a << 8) | u32::from(*b));
        let signed = match size {
            1 => i64::from(unsigned as u8 as i8),
            2 => i64::from(unsigned as u16 as i16),
            4 => i64::from(unsigned as i32),
            _ => 0,
        };
        let page = g.usage_page;
        let usage = |u: u32| if size == 4 { u } else { (page << 16) | u };
        match ((prefix >> 2) & 3, prefix >> 4) {
            // Main items.
            (0, 0xa) => {
                let first = usages.first().copied().or(range.0).unwrap_or(0);
                collections.push((unsigned == 1, first));
                usages.clear();
                range = (None, None);
            }
            (0, 0xc) => {
                collections.pop();
                usages.clear();
                range = (None, None);
            }
            (0, tag @ (0x8 | 0x9 | 0xb)) => {
                if tag == 0xb {
                    let bits = g.report_size.saturating_mul(g.report_count);
                    let slot = match feature_bits.iter().position(|(id, _)| *id == g.report_id) {
                        Some(s) => s,
                        None => {
                            feature_bits.push((g.report_id, 0));
                            feature_bits.len() - 1
                        }
                    };
                    let offset = feature_bits[slot].1;
                    let constant = unsigned & 1 == 1;
                    let in_monitor = collections.iter().any(|(app, u)| *app && *u == MONITOR_CONTROL);
                    if found.is_none() && in_monitor && !constant {
                        for k in 0..g.report_count {
                            let u = match (usages.get(k as usize).or(usages.last()), range) {
                                (Some(u), _) => *u,
                                (None, (Some(lo), Some(hi))) => (lo + k).min(hi),
                                _ => 0,
                            };
                            if u == VESA_BRIGHTNESS {
                                let max = if g.logical_min < 0 { g.logical_max.0 } else { g.logical_max.1 };
                                found = Some(Field {
                                    report_id: g.report_id,
                                    numbered: false,
                                    bit_offset: offset + k * g.report_size,
                                    bits: g.report_size,
                                    min: g.logical_min,
                                    max,
                                    report_len: 0,
                                });
                                break;
                            }
                        }
                    }
                    feature_bits[slot].1 = offset.saturating_add(bits);
                }
                usages.clear();
                range = (None, None);
            }
            // Global items.
            (1, 0x0) => g.usage_page = unsigned,
            (1, 0x1) => g.logical_min = signed,
            (1, 0x2) => g.logical_max = (signed, i64::from(unsigned)),
            (1, 0x7) => g.report_size = unsigned,
            (1, 0x8) => {
                g.report_id = unsigned as u8;
                numbered = true;
            }
            (1, 0x9) => g.report_count = unsigned,
            (1, 0xa) => stack.push(g),
            (1, 0xb) => g = stack.pop().unwrap_or_default(),
            // Local items.
            (2, 0x0) => usages.push(usage(unsigned)),
            (2, 0x1) => range.0 = Some(usage(unsigned)),
            (2, 0x2) => range.1 = Some(usage(unsigned)),
            _ => {}
        }
    }
    let mut field = found?;
    if field.bits == 0 || field.bits > 32 || field.max <= field.min {
        return None;
    }
    let bits = feature_bits.iter().find(|(id, _)| *id == field.report_id).map_or(0, |(_, b)| *b);
    field.numbered = numbered;
    field.report_len = usize::from(numbered) + bits.div_ceil(8) as usize;
    Some(field)
}

// HIDIOCGFEATURE(len) and HIDIOCSFEATURE(len): _IOC(_IOC_READ |
// _IOC_WRITE, 'H', nr, len), from <linux/hidraw.h>.
fn hid_ioc(nr: u64, len: usize) -> u64 {
    (3 << 30) | ((len as u64) << 16) | (u64::from(b'H') << 8) | nr
}

/// Read a feature report: `report[0]` names it; the result is its length.
pub fn get_feature(file: &std::fs::File, report: &mut [u8]) -> io::Result<usize> {
    // SAFETY: the kernel writes at most `report.len()` bytes, the size the
    // request encodes, into the buffer, which lives through the call.
    let n = unsafe { libc::ioctl(file.as_raw_fd(), hid_ioc(0x07, report.len()) as _, report.as_mut_ptr()) };
    if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
}

/// Send a feature report (its first byte its ID).
pub fn set_feature(file: &std::fs::File, report: &[u8]) -> io::Result<()> {
    // SAFETY: the kernel reads `report.len()` bytes, the size the request
    // encodes, from the buffer, which lives through the call.
    let n = unsafe { libc::ioctl(file.as_raw_fd(), hid_ioc(0x06, report.len()) as _, report.as_ptr()) };
    if n < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// The Container ID capability in a USB BOS descriptor (sysfs
/// `bos_descriptors`): a Device Capability (type 0x10) of kind 0x04.
pub fn container_id(bos: &[u8]) -> Option<[u8; 16]> {
    if bos.len() < 5 || bos[1] != 0x0f {
        return None;
    }
    let total = (u16::from_le_bytes([bos[2], bos[3]]) as usize).min(bos.len());
    let mut at = bos[0] as usize;
    while at + 3 <= total {
        let len = bos[at] as usize;
        if len < 3 {
            return None;
        }
        if bos[at + 1] == 0x10 && bos[at + 2] == 0x04 && len >= 20 && at + 20 <= total {
            return bos[at + 4..at + 20].try_into().ok();
        }
        at += len;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The 2022 Studio Display's interface 7 descriptor, as asdbctl's
    /// README records it (79 bytes).
    pub(crate) const STUDIO_DISPLAY: [u8; 79] = [
        0x05, 0x80, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x01, 0x06, 0x82, 0x00, 0x09, 0x10, 0x16, 0x90, 0x01, 0x27, 0x60,
        0xea, 0x00, 0x00, 0x67, 0xe1, 0x00, 0x00, 0x01, 0x55, 0x0e, 0x75, 0x20, 0x95, 0x01, 0xb1, 0x42, 0x05, 0x0f,
        0x09, 0x50, 0x15, 0x00, 0x26, 0x20, 0x4e, 0x66, 0x10, 0x01, 0x55, 0x0d, 0x75, 0x10, 0xb1, 0x42, 0x06, 0x82,
        0x00, 0x09, 0x10, 0x16, 0x90, 0x01, 0x27, 0x60, 0xea, 0x00, 0x00, 0x67, 0xe1, 0x00, 0x00, 0x01, 0x55, 0x0e,
        0x75, 0x20, 0x95, 0x01, 0x81, 0x02, 0xc0,
    ];

    #[test]
    fn the_studio_displays_descriptor_gives_report_1_from_400_to_60000() {
        let f = brightness_field(&STUDIO_DISPLAY).expect("the brightness feature");
        assert_eq!(
            f,
            Field { report_id: 1, numbered: true, bit_offset: 0, bits: 32, min: 400, max: 60000, report_len: 7 }
        );
        // asdbctl's minimum: 01 90 01 00 00 00 00.
        assert_eq!(f.report(400), vec![0x01, 0x90, 0x01, 0, 0, 0, 0]);
        assert_eq!(f.report(60000), vec![0x01, 0x60, 0xea, 0, 0, 0, 0]);
        assert_eq!(f.report(1_000_000), f.report(60000), "clamped to the range");
        assert_eq!(f.read(&[0x01, 0x30, 0x75, 0, 0, 0x12, 0x34]), Some(30000));
        assert_eq!(f.read(&[0x01, 0x30]), None, "a short report");
    }

    #[test]
    fn other_hid_descriptors_have_no_brightness() {
        // A boot keyboard's descriptor (HID 1.11, appendix B).
        let keyboard = [
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01,
            0x95, 0x08, 0x81, 0x02, 0xc0,
        ];
        assert_eq!(brightness_field(&keyboard), None);
        // Brightness outside a monitor control collection doesn't count.
        let loose = [0x06, 0x82, 0x00, 0x09, 0x10, 0x15, 0x00, 0x25, 0x64, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x02];
        assert_eq!(brightness_field(&loose), None);
        assert_eq!(brightness_field(&STUDIO_DISPLAY[..20]), None, "a cut-off descriptor");
    }

    #[test]
    fn a_field_after_others_and_without_report_ids() {
        // Monitor control: contrast (8 bits), then brightness 0..100 (8
        // bits), unnumbered.
        let desc = [
            0x05, 0x80, 0x09, 0x01, 0xa1, 0x01, 0x05, 0x82, 0x09, 0x12, 0x09, 0x10, 0x15, 0x00, 0x25, 0x64, 0x75, 0x08,
            0x95, 0x02, 0xb1, 0x02, 0xc0,
        ];
        let f = brightness_field(&desc).unwrap();
        assert_eq!((f.numbered, f.bit_offset, f.bits, f.min, f.max, f.report_len), (false, 8, 8, 0, 100, 2));
        assert_eq!(f.report(42), vec![0, 42]);
        assert_eq!(f.read(&[7, 42]), Some(42));
    }

    #[test]
    fn bos_descriptors_give_the_container_id() {
        // A BOS with a USB 2.0 extension, then a Container ID.
        let mut bos = vec![0x05, 0x0f, 0, 0, 2, 0x07, 0x10, 0x02, 0x06, 0, 0, 0, 0x14, 0x10, 0x04, 0x00];
        bos.extend(super::super::edid::tests::CONTAINER);
        let total = bos.len() as u16;
        bos[2..4].copy_from_slice(&total.to_le_bytes());
        assert_eq!(container_id(&bos), Some(super::super::edid::tests::CONTAINER));
        // ganymede's camera: no Container ID.
        let camera = [
            0x05, 0x0f, 0x29, 0x00, 0x02, 0x1c, 0x10, 0x05, 0x00, 0xdf, 0x60, 0xdd, 0xd8, 0x89, 0x45, 0xc7, 0x4c, 0x9c,
            0xd2, 0x65, 0x9d, 0x9e, 0x64, 0x8a, 0x9f, 0x00, 0x00, 0x03, 0x06, 0x17, 0x06, 0x15, 0x00, 0x08, 0x10, 0x11,
            0x01, 0x03, 0x00, 0x00, 0x00,
        ];
        assert_eq!(container_id(&camera), None);
        assert_eq!(container_id(&[]), None);
    }
}
