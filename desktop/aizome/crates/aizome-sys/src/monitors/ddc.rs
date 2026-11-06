//! DDC/CI: MCCS VCP feature 0x10 (luminance) over a connector's I2C bus.
//!
//! The kernel gives every connector's DDC channel (HDMI's DDC pins, or
//! DisplayPort's AUX channel) as an I2C adapter, `/sys/class/drm/cardN-
//! <connector>/ddc`; the `i2c-dev` module makes it `/dev/i2c-N`. The
//! display answers DDC/CI at I2C address 0x37 (VESA DDC/CI 1.1):
//!
//! - Get VCP Feature: the host writes `51 82 01 <code> <check>`, waits at
//!   least 40 ms, then reads `6E 88 02 <result> <code> <type> <max hi>
//!   <max lo> <current hi> <current lo> <check>`. The result is 0 when the
//!   display has the feature, 1 when it doesn't.
//! - Set VCP Feature: `51 84 03 <code> <hi> <lo> <check>`, with no reply;
//!   the next message waits 50 ms.
//! - The host's checksums XOR every byte with the display's write address
//!   0x6E first; the display's XOR its reply with 0x50 first. A display
//!   that's busy, or has nothing to say, answers the null message
//!   `6E 80 BE`.
//!
//! The protocol is what ddcutil (GPL-2.0) and ddcci-driver-linux speak;
//! this is an implementation of the VESA messages, not a copy of either.
//! Like ddcutil (since 2.0), the bus is locked with `flock` for the length
//! of an exchange, so ddcutil, Settings and the shell don't interleave
//! their messages on one bus.

use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

/// MCCS's luminance: the monitor's brightness.
pub const LUMINANCE: u8 = 0x10;
/// The display's DDC/CI address.
const ADDRESS: u64 = 0x37;
/// `I2C_SLAVE`, from <linux/i2c-dev.h>.
const I2C_SLAVE: u64 = 0x0703;
/// The host's source address: the first byte it sends.
const HOST: u8 = 0x51;
/// The display's address as written on the bus, which seeds the host's
/// checksums.
const DISPLAY_WRITE: u8 = 0x6e;
/// The virtual host address, which seeds the display's checksums.
const HOST_VIRTUAL: u8 = 0x50;
/// How long to give the display to answer: DDC/CI asks for 40 ms; ddcutil
/// waits 50.
pub const REPLY_DELAY: Duration = Duration::from_millis(50);
/// The least time from a Set VCP Feature to the next message.
pub const SET_DELAY: Duration = Duration::from_millis(50);
/// Tries for a reading before the display is taken to have no DDC/CI.
pub const TRIES: usize = 3;

fn checksum(seed: u8, bytes: &[u8]) -> u8 {
    bytes.iter().fold(seed, |a, b| a ^ b)
}

/// Get VCP Feature for `code`.
pub fn get_request(code: u8) -> [u8; 5] {
    let mut m = [HOST, 0x82, 0x01, code, 0];
    m[4] = checksum(DISPLAY_WRITE, &m[..4]);
    m
}

/// Set VCP Feature `code` to `value`.
pub fn set_request(code: u8, value: u16) -> [u8; 7] {
    let [hi, lo] = value.to_be_bytes();
    let mut m = [HOST, 0x84, 0x03, code, hi, lo, 0];
    m[6] = checksum(DISPLAY_WRITE, &m[..6]);
    m
}

/// What a display answered to Get VCP Feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Value {
        current: u16,
        max: u16,
    },
    /// It said it hasn't the feature.
    Unsupported,
    /// The null message: busy, or nothing to say.
    Null,
}

/// Read a reply to Get VCP Feature `code`; `None` when it's garbled (a
/// bad checksum, a reply to something else, the all-ones of no answer).
pub fn parse_reply(code: u8, r: &[u8]) -> Option<Reply> {
    if r.len() < 3 || r[0] != DISPLAY_WRITE || r[1] & 0x80 == 0 {
        return None;
    }
    let len = (r[1] & 0x7f) as usize;
    let body = r.get(..2 + len)?;
    if checksum(HOST_VIRTUAL, body) != *r.get(2 + len)? {
        return None;
    }
    if len == 0 {
        return Some(Reply::Null);
    }
    if len != 8 || r[2] != 0x02 || r[4] != code {
        return None;
    }
    match r[3] {
        0 => Some(Reply::Value { max: u16::from_be_bytes([r[6], r[7]]), current: u16::from_be_bytes([r[8], r[9]]) }),
        1 => Some(Reply::Unsupported),
        _ => None,
    }
}

/// Where DDC/CI messages go: an I2C bus, or a test's display.
pub trait Bus {
    fn write(&mut self, message: &[u8]) -> io::Result<()>;
    fn read(&mut self, buf: &mut [u8]) -> io::Result<()>;
}

/// `/dev/i2c-N`, talking to the display's address, locked for this
/// process until dropped.
pub struct I2c(std::fs::File);

impl I2c {
    pub fn open(path: &Path) -> io::Result<I2c> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_CLOEXEC).open(path)?;
        let fd = file.as_raw_fd();
        // Another process's exchange takes a few hundred milliseconds at
        // most: wait for it up to a second.
        let mut waited = Duration::ZERO;
        loop {
            // SAFETY: flock on a descriptor this function owns.
            if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EWOULDBLOCK) || waited >= Duration::from_secs(1) {
                return Err(e);
            }
            std::thread::sleep(Duration::from_millis(20));
            waited += Duration::from_millis(20);
        }
        // SAFETY: I2C_SLAVE takes the address as its argument, not a
        // pointer. It fails with EBUSY when a kernel driver (ddcci) holds
        // the address, which is left to that driver.
        if unsafe { libc::ioctl(fd, I2C_SLAVE as _, ADDRESS as libc::c_ulong) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(I2c(file))
    }
}

impl Bus for I2c {
    fn write(&mut self, message: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.0.write_all(message)
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<()> {
        use std::io::Read;
        self.0.read_exact(buf)
    }
}

/// Ask for `code`, with up to [`TRIES`] tries for a clear answer. An
/// error is the bus's (no display answering at 0x37 is `ENXIO` or `EIO`,
/// depending on the driver); `Reply::Null` means no clear answer came.
pub fn get(bus: &mut dyn Bus, code: u8, sleep: &dyn Fn(Duration)) -> io::Result<Reply> {
    let mut last_error = None;
    for attempt in 0..TRIES {
        if attempt > 0 {
            sleep(SET_DELAY);
        }
        if let Err(e) = bus.write(&get_request(code)) {
            last_error = Some(e);
            continue;
        }
        sleep(REPLY_DELAY);
        let mut reply = [0u8; 11];
        if let Err(e) = bus.read(&mut reply) {
            last_error = Some(e);
            continue;
        }
        last_error = None;
        match parse_reply(code, &reply) {
            Some(Reply::Null) | None => continue,
            Some(r) => return Ok(r),
        }
    }
    match last_error {
        Some(e) => Err(e),
        None => Ok(Reply::Null),
    }
}

/// Set `code` to `value`. The caller waits [`SET_DELAY`] before the next
/// message on this bus.
pub fn set(bus: &mut dyn Bus, code: u8, value: u16) -> io::Result<()> {
    bus.write(&set_request(code, value))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A display at the other end of the bus: answers Get VCP Feature for
    /// luminance (or says it hasn't it), takes Set, and can garble its
    /// first answers.
    pub(crate) struct Display {
        pub(crate) current: u16,
        pub(crate) max: u16,
        pub(crate) has_luminance: bool,
        pub(crate) garbled: usize,
        pub(crate) written: Vec<Vec<u8>>,
        asked: Option<u8>,
    }

    impl Display {
        pub(crate) fn new(current: u16, max: u16) -> Display {
            Display { current, max, has_luminance: true, garbled: 0, written: Vec::new(), asked: None }
        }
    }

    pub(crate) fn reply(code: u8, result: u8, max: u16, current: u16) -> [u8; 11] {
        let [mh, ml] = max.to_be_bytes();
        let [ch, cl] = current.to_be_bytes();
        let mut r = [DISPLAY_WRITE, 0x88, 0x02, result, code, 0x00, mh, ml, ch, cl, 0];
        r[10] = checksum(HOST_VIRTUAL, &r[..10]);
        r
    }

    impl Bus for Display {
        fn write(&mut self, m: &[u8]) -> io::Result<()> {
            self.written.push(m.to_vec());
            assert_eq!(m[0], HOST);
            assert_eq!(checksum(DISPLAY_WRITE, &m[..m.len() - 1]), m[m.len() - 1], "the host's checksum");
            match m[2] {
                0x01 => self.asked = Some(m[3]),
                0x03 if m[3] == LUMINANCE => self.current = u16::from_be_bytes([m[4], m[5]]).min(self.max),
                _ => {}
            }
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8]) -> io::Result<()> {
            let code = self.asked.take().unwrap_or(0);
            let r = if self.garbled > 0 {
                self.garbled -= 1;
                [0xff; 11]
            } else if code == LUMINANCE && self.has_luminance {
                reply(code, 0, self.max, self.current)
            } else {
                reply(code, 1, 0, 0)
            };
            buf.copy_from_slice(&r[..buf.len()]);
            Ok(())
        }
    }

    /// No display at 0x37: every message is refused.
    pub(crate) struct Nobody;

    impl Bus for Nobody {
        fn write(&mut self, _: &[u8]) -> io::Result<()> {
            Err(io::Error::from_raw_os_error(libc::ENXIO))
        }
        fn read(&mut self, _: &mut [u8]) -> io::Result<()> {
            Err(io::Error::from_raw_os_error(libc::ENXIO))
        }
    }

    #[test]
    fn messages_as_ddc_ci_has_them() {
        // ddcutil's Get VCP luminance and Set VCP luminance to 50.
        assert_eq!(get_request(0x10), [0x51, 0x82, 0x01, 0x10, 0xac]);
        assert_eq!(set_request(0x10, 50), [0x51, 0x84, 0x03, 0x10, 0x00, 0x32, 0x9a]);
        assert_eq!(parse_reply(0x10, &reply(0x10, 0, 100, 42)), Some(Reply::Value { current: 42, max: 100 }));
        assert_eq!(parse_reply(0x10, &reply(0x10, 1, 0, 0)), Some(Reply::Unsupported));
        assert_eq!(parse_reply(0x10, &[0x6e, 0x80, 0xbe]), Some(Reply::Null));
        let mut bad = reply(0x10, 0, 100, 42);
        bad[8] ^= 1;
        assert_eq!(parse_reply(0x10, &bad), None, "a bad checksum");
        assert_eq!(parse_reply(0x12, &reply(0x10, 0, 100, 42)), None, "a reply about another feature");
        assert_eq!(parse_reply(0x10, &[0xff; 11]), None, "nobody answered");
    }

    #[test]
    fn readings_retry_a_garbled_answer_and_settings_reach_the_display() {
        let mut d = Display::new(30, 100);
        d.garbled = 2;
        let slept = std::cell::Cell::new(Duration::ZERO);
        let sleep = |t: Duration| slept.set(slept.get() + t);
        assert_eq!(get(&mut d, LUMINANCE, &sleep).unwrap(), Reply::Value { current: 30, max: 100 });
        assert_eq!(d.written.len(), 3, "two garbled answers, then a good one");
        assert!(slept.get() >= REPLY_DELAY * 3, "it waited for each answer");
        set(&mut d, LUMINANCE, 75).unwrap();
        assert_eq!(get(&mut d, LUMINANCE, &sleep).unwrap(), Reply::Value { current: 75, max: 100 });
    }

    #[test]
    fn a_display_without_luminance_or_without_ddc() {
        let mut d = Display::new(0, 100);
        d.has_luminance = false;
        assert_eq!(get(&mut d, LUMINANCE, &|_| {}).unwrap(), Reply::Unsupported);
        let mut d = Display::new(0, 100);
        d.garbled = TRIES;
        assert_eq!(get(&mut d, LUMINANCE, &|_| {}).unwrap(), Reply::Null, "never a clear answer");
        let e = get(&mut Nobody, LUMINANCE, &|_| {}).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::ENXIO));
    }
}
