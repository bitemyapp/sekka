//! The kernel's radio switches: `/dev/rfkill` events (read to follow them,
//! written to block or unblock every radio at once, which is airplane mode)
//! and `/sys/class/rfkill` (read when the device can't be opened).
//!
//! On a systemd system the seat's user may read and write `/dev/rfkill`
//! (udev's `uaccess` tag), so airplane mode needs no polkit prompt.

use super::model::{Radio, RadioKind};
use crate::error::{Error, Result};
use crate::service::Stop;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

pub(crate) const DEVICE: &str = "/dev/rfkill";
pub(crate) const SYSFS: &str = "/sys/class/rfkill";

const OP_ADD: u8 = 0;
const OP_DEL: u8 = 1;
const OP_CHANGE: u8 = 2;
const OP_CHANGE_ALL: u8 = 3;
const TYPE_ALL: u8 = 0;
/// `struct rfkill_event`'s first version; later ones only add bytes.
const EVENT_SIZE: usize = 8;

/// One `struct rfkill_event`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Event {
    pub index: u32,
    pub kind: u8,
    pub op: u8,
    pub soft: bool,
    pub hard: bool,
}

pub(crate) fn parse_event(bytes: &[u8]) -> Option<Event> {
    let index = u32::from_ne_bytes(bytes.get(0..4)?.try_into().ok()?);
    Some(Event {
        index,
        kind: *bytes.get(4)?,
        op: *bytes.get(5)?,
        soft: *bytes.get(6)? != 0,
        hard: *bytes.get(7)? != 0,
    })
}

/// The event that blocks (or unblocks) every radio.
pub(crate) fn block_all_event(blocked: bool) -> [u8; EVENT_SIZE] {
    let mut event = [0u8; EVENT_SIZE];
    event[4] = TYPE_ALL;
    event[5] = OP_CHANGE_ALL;
    event[6] = u8::from(blocked);
    event
}

/// Apply an event to the radios known so far.
pub(crate) fn apply(radios: &mut BTreeMap<u32, Radio>, event: Event) {
    match event.op {
        OP_ADD | OP_CHANGE => {
            radios.insert(
                event.index,
                Radio {
                    index: event.index,
                    kind: RadioKind::from_u8(event.kind),
                    soft_blocked: event.soft,
                    hard_blocked: event.hard,
                },
            );
        }
        OP_DEL => {
            radios.remove(&event.index);
        }
        _ => {}
    }
}

/// Block or unblock every radio.
pub(crate) fn block_all(device: &Path, blocked: bool) -> Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).open(device)?;
    file.write_all(&block_all_event(blocked))?;
    Ok(())
}

/// The rfkill type of Bluetooth radios.
pub(crate) const TYPE_BLUETOOTH: u8 = 2;

/// Block or unblock every radio of one type (`TYPE_BLUETOOTH`).
pub(crate) fn block_type(device: &Path, kind: u8, blocked: bool) -> Result<()> {
    let mut event = block_all_event(blocked);
    event[4] = kind;
    let mut file = std::fs::OpenOptions::new().write(true).open(device)?;
    file.write_all(&event)?;
    Ok(())
}

/// The radios, from sysfs (`rfkillN/{index,type,soft,hard}`).
pub(crate) fn read_sysfs(root: &Path) -> Vec<Radio> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let read = |dir: &Path, name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    let mut radios: Vec<Radio> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let index = read(&dir, "index").trim().parse().ok()?;
            let kind = match read(&dir, "type").trim() {
                "wlan" => RadioKind::Wlan,
                "bluetooth" => RadioKind::Bluetooth,
                "ultrawideband" => RadioKind::Uwb,
                "wimax" => RadioKind::Wimax,
                "wwan" => RadioKind::Wwan,
                "gps" => RadioKind::Gps,
                "fm" => RadioKind::Fm,
                "nfc" => RadioKind::Nfc,
                _ => RadioKind::Other,
            };
            Some(Radio {
                index,
                kind,
                soft_blocked: read(&dir, "soft").trim() == "1",
                hard_blocked: read(&dir, "hard").trim() == "1",
            })
        })
        .collect();
    radios.sort_by_key(|r| r.index);
    radios
}

/// Follow `/dev/rfkill` on a thread until `stop`, calling `changed` with
/// all radios after each burst of events. Returns false (and starts
/// nothing) when the device can't be opened.
pub(crate) fn watch(device: PathBuf, stop: Stop, changed: impl Fn(Vec<Radio>) + Send + 'static) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    let Ok(mut file) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&device) else {
        return false;
    };
    let spawned = std::thread::Builder::new().name("aizome-sys rfkill".into()).spawn(move || {
        let mut radios = BTreeMap::new();
        let mut buf = [0u8; 64];
        while !stop.stopped() {
            let mut fds = libc::pollfd { fd: file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: one valid pollfd for the duration of the call.
            let ready = unsafe { libc::poll(&mut fds, 1, 500) };
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            if ready == 0 {
                continue;
            }
            let mut any = false;
            // Each read returns one event (of the kernel's size, which may
            // be larger than the first version's 8 bytes).
            loop {
                match file.read(&mut buf) {
                    Ok(n) if n >= EVENT_SIZE => {
                        if let Some(event) = parse_event(&buf[..n]) {
                            apply(&mut radios, event);
                            any = true;
                        }
                    }
                    Ok(0) => return,
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return,
                }
            }
            if any {
                changed(radios.values().cloned().collect());
            }
        }
    });
    spawned.is_ok()
}

/// An error for an rfkill write the person isn't allowed to make.
pub(crate) fn is_permission(error: &Error) -> bool {
    matches!(error, Error::NotAuthorized(_) | Error::NotFound(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip() {
        let bytes = block_all_event(true);
        let event = parse_event(&bytes).unwrap();
        assert_eq!((event.kind, event.op, event.soft, event.hard), (TYPE_ALL, OP_CHANGE_ALL, true, false));
        assert_eq!(parse_event(&bytes[..5]), None);
    }

    #[test]
    fn events_track_radios() {
        let mut radios = BTreeMap::new();
        let event = |index, op, soft| Event { index, kind: 1, op, soft, hard: false };
        apply(&mut radios, event(2, OP_ADD, false));
        apply(&mut radios, event(3, OP_ADD, false));
        apply(&mut radios, event(2, OP_CHANGE, true));
        apply(&mut radios, event(3, OP_DEL, false));
        let radios: Vec<Radio> = radios.into_values().collect();
        assert_eq!(radios.len(), 1);
        assert!(radios[0].soft_blocked);
        assert_eq!(radios[0].kind, RadioKind::Wlan);
    }

    #[test]
    fn sysfs_radios() {
        let root = std::env::temp_dir().join(format!("aizome-sys-rfkill-{}", std::process::id()));
        let dir = root.join("rfkill0");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, value) in [("index", "0\n"), ("type", "bluetooth\n"), ("soft", "1\n"), ("hard", "0\n")] {
            std::fs::write(dir.join(name), value).unwrap();
        }
        let radios = read_sysfs(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(radios, [Radio { index: 0, kind: RadioKind::Bluetooth, soft_blocked: true, hard_blocked: false }]);
    }

    #[test]
    fn blocking_writes_one_event() {
        let path = std::env::temp_dir().join(format!("aizome-sys-rfkill-dev-{}", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        block_all(&path, true).unwrap();
        let written = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(written, block_all_event(true));
    }
}
