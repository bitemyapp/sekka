#!/usr/bin/env -S rust-script
//! flash-usb: write a disk image, such as an installer ISO, to a USB drive
//! picked from a list, on macOS or Linux, then read every byte back.
//!
//! ```cargo
//! [package]
//! edition = "2024"
//!
//! [dependencies]
//! anyhow = "1"
//! libc = "0.2"
//! plist = "1"
//! serde_json = "1"
//! sha2 = "0.10"
//! ```
//!
//! Usage: `scripts/flash_usb.rs [IMAGE] [--sha256 HEX] [--all] [--list]`
//!
//! Without IMAGE it offers the `*.iso` files in the current directory and
//! under `artifacts/`, most recently built or copied first. It lists the
//! drives that look like USB flash drives (with `--all`, every external
//! drive) with their size, device, vendor, model and serial, and erases only
//! the one you pick and confirm by typing its device name. `--list` shows
//! every drive, and why each one left out is left out, then stops.
//!
//! The image's SHA-256 must match `--sha256`, or a `<image>.sha256` or
//! `SHA256SUMS` file beside it, whichever exists. Writing needs
//! administrator rights, so only that step runs through `sudo`. It checks
//! that the drive is still the one you picked, unmounts it, writes the
//! image, clears the drive's last MiB (where a stale backup partition table
//! would confuse firmware), reads every byte back and compares it, writes a
//! report beside the image and ejects the drive.
//!
//! Internal drives, virtual and read-only drives, the drive holding the
//! running system and the drive holding the image are never offered.
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

const CHUNK: usize = 8 * 1024 * 1024;
/// Cleared at the end of the drive: a previous GPT's backup header lives in
/// the last sectors.
const TAIL: u64 = 1024 * 1024;
/// Larger than this is probably an external disk, not a flash drive.
const FLASH_DRIVE_MAX: u64 = 2_000_000_000_000;

/// A whole drive as the operating system describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Drive {
    /// `disk4` on macOS, `sdb` on Linux.
    name: String,
    bytes: u64,
    vendor: String,
    model: String,
    serial: String,
    /// `USB`, `Secure Digital`, `sata`, ...
    bus: String,
    usb_or_removable: bool,
    internal: bool,
    virtual_drive: bool,
    read_only: bool,
    /// Holds a filesystem the running system needs.
    system: bool,
    mounts: Vec<String>,
}

impl Drive {
    fn label(&self) -> String {
        let name = format!("{} {}", self.vendor.trim(), self.model.trim());
        let name = name.trim();
        if name.is_empty() { "unnamed drive".to_owned() } else { name.to_owned() }
    }

    fn serial_hint(&self) -> String {
        let serial = self.serial.trim();
        match serial.len() {
            0 => "no serial".to_owned(),
            1..=12 => format!("serial {serial}"),
            n => format!("serial …{}", &serial[n - 8..]),
        }
    }

    fn summary(&self) -> String {
        format!(
            "{:<8} {:>9}  {}  {}  {}{}",
            self.name,
            human(self.bytes),
            self.label(),
            if self.bus.is_empty() { "?" } else { self.bus.as_str() },
            self.serial_hint(),
            if self.mounts.is_empty() { String::new() } else { format!("  mounted: {}", self.mounts.join(", ")) }
        )
    }

    /// Why this drive is not offered, if it isn't.
    fn excluded(&self, image_drive: Option<&str>, all: bool) -> Option<&'static str> {
        if self.internal {
            Some("internal")
        } else if self.virtual_drive {
            Some("virtual (a disk image or synthesized volume)")
        } else if self.read_only {
            Some("read-only")
        } else if self.system {
            Some("holds the running system")
        } else if image_drive == Some(self.name.as_str()) {
            Some("holds the image")
        } else if self.bytes == 0 {
            Some("no media")
        } else if !all && !self.usb_or_removable {
            Some("not USB or removable (use --all)")
        } else if !all && self.bytes > FLASH_DRIVE_MAX {
            Some("larger than 2 TB, probably not a flash drive (use --all)")
        } else {
            None
        }
    }

    /// The same physical drive: compared before writing, so a drive that
    /// was unplugged and replaced in the meantime is not erased.
    fn same(&self, other: &Drive) -> bool {
        (&self.name, self.bytes, &self.vendor, &self.model, &self.serial, &self.bus)
            == (&other.name, other.bytes, &other.vendor, &other.model, &other.serial, &other.bus)
    }
}

/// Sizes as drive makers count them (powers of 1000).
fn human(bytes: u64) -> String {
    let units = ["B", "kB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", units[unit]) }
}

fn run(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new(program).args(args).output().with_context(|| format!("could not run {program}"))?;
    ensure!(
        output.status.success(),
        "{program} {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use plist::Value;
    use std::collections::HashMap;

    fn plist(args: &[&str]) -> Result<Value> {
        let out = run(args[0], &args[1..])?;
        Ok(Value::from_reader(std::io::Cursor::new(out))?)
    }

    fn text(dict: &plist::Dictionary, key: &str) -> String {
        dict.get(key).and_then(Value::as_string).unwrap_or("").trim().to_owned()
    }

    fn flag(dict: &plist::Dictionary, key: &str) -> bool {
        dict.get(key).and_then(Value::as_boolean).unwrap_or(false)
    }

    /// USB vendor, product and serial for each whole disk, from the I/O
    /// Registry: the nearest USB device above each disk's media.
    fn usb_details() -> HashMap<String, (String, String, String)> {
        let mut found = HashMap::new();
        let Ok(tree) = plist(&["/usr/sbin/ioreg", "-p", "IOService", "-l", "-a"]) else {
            return found;
        };
        fn walk(node: &Value, usb: Option<(String, String, String)>, found: &mut HashMap<String, (String, String, String)>) {
            let Some(dict) = node.as_dictionary() else {
                if let Some(list) = node.as_array() {
                    for child in list {
                        walk(child, usb.clone(), found);
                    }
                }
                return;
            };
            let pick = |keys: &[&str]| keys.iter().map(|k| text(dict, k)).find(|v| !v.is_empty()).unwrap_or_default();
            let serial = pick(&["USB Serial Number", "kUSBSerialNumberString"]);
            let usb = if serial.is_empty() && !dict.contains_key("idVendor") {
                usb
            } else {
                Some((pick(&["USB Vendor Name", "kUSBVendorString"]), pick(&["USB Product Name", "kUSBProductString"]), serial))
            };
            if flag(dict, "Whole")
                && let (Some(name), Some(usb)) = (dict.get("BSD Name").and_then(Value::as_string), &usb)
            {
                found.insert(name.to_owned(), usb.clone());
            }
            if let Some(children) = dict.get("IORegistryEntryChildren").and_then(Value::as_array) {
                for child in children {
                    walk(child, usb.clone(), found);
                }
            }
        }
        walk(&tree, None, &mut found);
        found
    }

    pub fn drives() -> Result<Vec<Drive>> {
        let list = plist(&["/usr/sbin/diskutil", "list", "-plist"])?;
        let list = list.as_dictionary().context("diskutil list: not a dictionary")?;
        let wholes: Vec<String> = list
            .get("WholeDisks")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_string).map(str::to_owned).collect())
            .unwrap_or_default();
        // Mount points of each whole disk's partitions and volumes.
        let mut mounts: HashMap<String, Vec<String>> = HashMap::new();
        for entry in list.get("AllDisksAndPartitions").and_then(Value::as_array).into_iter().flatten() {
            let Some(entry) = entry.as_dictionary() else { continue };
            let name = text(entry, "DeviceIdentifier");
            let mut points: Vec<String> = Vec::new();
            let mut add = |d: &plist::Dictionary| {
                let point = text(d, "MountPoint");
                if !point.is_empty() {
                    points.push(point);
                }
            };
            add(entry);
            for key in ["Partitions", "APFSVolumes"] {
                for part in entry.get(key).and_then(Value::as_array).into_iter().flatten() {
                    if let Some(part) = part.as_dictionary() {
                        add(part);
                    }
                }
            }
            mounts.insert(name, points);
        }
        let usb = usb_details();
        let mut drives = Vec::new();
        for name in wholes {
            let info = plist(&["/usr/sbin/diskutil", "info", "-plist", &name])?;
            let info = info.as_dictionary().context("diskutil info: not a dictionary")?;
            let bytes = ["Size", "TotalSize"]
                .iter()
                .find_map(|k| info.get(k).and_then(Value::as_unsigned_integer))
                .unwrap_or(0);
            let bus = text(info, "BusProtocol");
            let points = mounts.remove(&name).unwrap_or_default();
            let (vendor, product, serial) = usb.get(&name).cloned().unwrap_or_default();
            drives.push(Drive {
                bytes,
                vendor,
                model: if product.is_empty() { text(info, "MediaName") } else { product },
                serial,
                usb_or_removable: bus == "USB"
                    || bus == "Secure Digital"
                    || flag(info, "Removable")
                    || flag(info, "RemovableMedia"),
                internal: flag(info, "Internal"),
                virtual_drive: text(info, "VirtualOrPhysical") == "Virtual" || bus == "Disk Image",
                read_only: !flag(info, "WritableMedia"),
                system: points.iter().any(|p| p == "/" || p.starts_with("/System/Volumes")),
                mounts: points,
                bus,
                name,
            });
        }
        Ok(drives)
    }

    /// The whole drive holding a device node such as `/dev/disk3s5`.
    pub fn parent(device: &str) -> Option<String> {
        let info = plist(&["/usr/sbin/diskutil", "info", "-plist", device]).ok()?;
        Some(text(info.as_dictionary()?, "ParentWholeDisk")).filter(|s| !s.is_empty())
    }

    pub fn unmount(drive: &Drive) -> Result<()> {
        run("/usr/sbin/diskutil", &["unmountDisk", &format!("/dev/{}", drive.name)])?;
        Ok(())
    }

    /// The raw device, locked exclusively and uncached, and its size.
    pub fn open(drive: &Drive) -> Result<(File, u64)> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
        use std::os::unix::io::AsRawFd;
        let path = format!("/dev/r{}", drive.name);
        let before = fs::symlink_metadata(&path)?;
        ensure!(before.file_type().is_char_device(), "{path} is not a raw disk device");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_EXLOCK | libc::O_NOFOLLOW)
            .open(&path)
            .with_context(|| format!("could not open {path} exclusively"))?;
        ensure!(file.metadata()?.rdev() == before.rdev(), "{path} changed while opening it");
        let fd = file.as_raw_fd();
        let (mut block, mut count) = (0u32, 0u64);
        // DKIOCGETBLOCKSIZE and DKIOCGETBLOCKCOUNT.
        // SAFETY: each ioctl writes one integer of the size passed.
        unsafe {
            ensure!(libc::ioctl(fd, 0x40046418, &mut block) == 0, "could not read the block size");
            ensure!(libc::ioctl(fd, 0x40086419, &mut count) == 0, "could not read the block count");
            libc::fcntl(fd, libc::F_NOCACHE, 1);
        }
        ensure!(block == 512 || block == 4096, "unexpected block size {block}");
        Ok((file, block as u64 * count))
    }

    /// The raw device is uncached already.
    pub fn drop_cache(_file: &File) {}

    pub fn eject(drive: &Drive) -> Result<()> {
        run("/usr/sbin/diskutil", &["eject", &format!("/dev/{}", drive.name)])?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use serde_json::Value;

    /// Mount points the running system needs.
    const SYSTEM: [&str; 9] = ["/", "/boot", "/boot/efi", "/efi", "/home", "/nix", "/nix/store", "/usr", "/var"];

    fn flag(v: &Value) -> bool {
        v.as_bool().unwrap_or_else(|| matches!(v.as_str(), Some("1") | Some("true")))
    }

    fn number(v: &Value) -> u64 {
        v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
    }

    fn text(v: &Value) -> String {
        v.as_str().unwrap_or("").trim().to_owned()
    }

    /// Mount points of a device and everything on it.
    fn mounts(dev: &Value, into: &mut Vec<String>) {
        for key in ["mountpoints", "mountpoint"] {
            match &dev[key] {
                Value::Array(points) => into.extend(points.iter().filter_map(Value::as_str).map(str::to_owned)),
                Value::String(point) => into.push(point.clone()),
                _ => {}
            }
        }
        for child in dev["children"].as_array().into_iter().flatten() {
            mounts(child, into);
        }
    }

    pub fn parse(json: &[u8]) -> Result<Vec<Drive>> {
        let tree: Value = serde_json::from_slice(json)?;
        let mut drives = Vec::new();
        for dev in tree["blockdevices"].as_array().into_iter().flatten() {
            if text(&dev["type"]) != "disk" {
                continue;
            }
            let name = text(&dev["name"]);
            let tran = text(&dev["tran"]);
            let mut points = Vec::new();
            mounts(dev, &mut points);
            points.retain(|p| !p.is_empty());
            let removable = flag(&dev["rm"]);
            let hotplug = flag(&dev["hotplug"]);
            drives.push(Drive {
                bytes: number(&dev["size"]),
                vendor: text(&dev["vendor"]),
                model: text(&dev["model"]),
                serial: text(&dev["serial"]),
                usb_or_removable: tran == "usb" || removable,
                internal: !(tran == "usb" || removable || hotplug),
                virtual_drive: ["loop", "zram", "ram", "nbd", "dm-", "md"].iter().any(|p| name.starts_with(p)),
                read_only: flag(&dev["ro"]),
                system: points.iter().any(|p| SYSTEM.contains(&p.as_str()) || p == "[SWAP]"),
                mounts: points,
                bus: if tran.is_empty() { String::new() } else { tran.to_uppercase() },
                name,
            });
        }
        Ok(drives)
    }

    pub fn drives() -> Result<Vec<Drive>> {
        let columns = "NAME,SIZE,TYPE,TRAN,RM,HOTPLUG,RO,VENDOR,MODEL,SERIAL";
        // MOUNTPOINTS lists every mount (util-linux 2.37); MOUNTPOINT, older,
        // only one.
        let out = run("lsblk", &["-J", "-b", "-o", &format!("{columns},MOUNTPOINTS")])
            .or_else(|_| run("lsblk", &["-J", "-b", "-o", &format!("{columns},MOUNTPOINT")]))?;
        parse(&out)
    }

    pub fn parent(device: &str) -> Option<String> {
        let out = run("lsblk", &["-no", "PKNAME", device]).ok()?;
        let name = String::from_utf8_lossy(&out).lines().next().unwrap_or("").trim().to_owned();
        if name.is_empty() { Path::new(device).file_name().map(|n| n.to_string_lossy().into_owned()) } else { Some(name) }
    }

    pub fn unmount(drive: &Drive) -> Result<()> {
        for point in &drive.mounts {
            run("umount", &[point]).with_context(|| format!("could not unmount {point}"))?;
        }
        Ok(())
    }

    /// The block device, opened exclusively (which fails while anything on
    /// it is mounted), and its size.
    pub fn open(drive: &Drive) -> Result<(File, u64)> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
        use std::os::unix::io::AsRawFd;
        let path = format!("/dev/{}", drive.name);
        let before = fs::symlink_metadata(&path)?;
        ensure!(before.file_type().is_block_device(), "{path} is not a block device");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_EXCL | libc::O_NOFOLLOW)
            .open(&path)
            .with_context(|| format!("could not open {path} exclusively (is something on it still in use?)"))?;
        ensure!(file.metadata()?.rdev() == before.rdev(), "{path} changed while opening it");
        let mut bytes = 0u64;
        // BLKGETSIZE64.
        // SAFETY: the ioctl writes one u64.
        ensure!(unsafe { libc::ioctl(file.as_raw_fd(), 0x8008_1272_u64 as _, &mut bytes) } == 0, "could not read the size");
        Ok((file, bytes))
    }

    /// Forget cached blocks, so reading back reads the drive.
    pub fn drop_cache(file: &File) {
        use std::os::unix::io::AsRawFd;
        // BLKFLSBUF, then advise the kernel the cache is not needed.
        // SAFETY: neither call touches memory.
        unsafe {
            libc::ioctl(file.as_raw_fd(), 0x1261_u64 as _, 0);
            libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
        }
    }

    pub fn eject(drive: &Drive) -> Result<()> {
        let device = format!("/dev/{}", drive.name);
        if run("udisksctl", &["power-off", "--no-user-interaction", "-b", &device]).is_ok()
            || run("eject", &[&device]).is_ok()
        {
            Ok(())
        } else {
            bail!("could not eject {device}; it is written and synced, so you can unplug it")
        }
    }
}

/// The whole drive holding `image`, so it is never offered.
fn image_drive(image: &Path) -> Option<String> {
    let out = run("df", &["-P", &image.to_string_lossy()]).ok()?;
    let line = String::from_utf8_lossy(&out).lines().nth(1)?.to_owned();
    let device = line.split_whitespace().next()?;
    device.starts_with("/dev/").then(|| platform::parent(device)).flatten()
}

/// The checksum the image must have, and where it came from.
fn expected_sha256(image: &Path, given: Option<&str>) -> Result<Option<(String, String)>> {
    let valid = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if let Some(hex) = given {
        ensure!(valid(hex), "--sha256 needs 64 hexadecimal digits");
        return Ok(Some((hex.to_ascii_lowercase(), "--sha256".to_owned())));
    }
    let file_name = image.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let sidecar = PathBuf::from(format!("{}.sha256", image.display()));
    if let Ok(text) = fs::read_to_string(&sidecar) {
        let hex = text.split_whitespace().next().unwrap_or("");
        ensure!(valid(hex), "{} doesn't start with a SHA-256", sidecar.display());
        return Ok(Some((hex.to_ascii_lowercase(), sidecar.display().to_string())));
    }
    let sums = image.with_file_name("SHA256SUMS");
    if let Ok(text) = fs::read_to_string(&sums) {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(hex), Some(name)) = (parts.next(), parts.next())
                && name.trim_start_matches('*') == file_name
                && valid(hex)
            {
                return Ok(Some((hex.to_ascii_lowercase(), sums.display().to_string())));
            }
        }
    }
    Ok(None)
}

fn sha256_file(path: &Path, done: &AtomicU64) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One line of progress, rewritten in place.
fn progress(stage: &str, done: u64, total: u64, start: Instant) {
    let seconds = start.elapsed().as_secs_f64().max(0.001);
    let rate = done as f64 / seconds;
    let left = if rate > 0.0 { (total.saturating_sub(done)) as f64 / rate } else { 0.0 };
    eprint!(
        "\r{stage} {} of {} ({:.0}%), {}/s, about {}:{:02} left   ",
        human(done),
        human(total),
        done as f64 * 100.0 / total.max(1) as f64,
        human(rate as u64),
        left as u64 / 60,
        left as u64 % 60
    );
    let _ = std::io::stderr().flush();
}

/// Copy `len` bytes from `src` to `dst`, returning the SHA-256 of what was
/// read. Writes are whole chunks, so a raw device always sees a multiple of
/// its block size.
fn copy(src: &mut impl Read, dst: &mut impl Write, len: u64, report: &mut dyn FnMut(u64)) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut done = 0u64;
    while done < len {
        let n = CHUNK.min((len - done) as usize);
        src.read_exact(&mut buffer[..n])?;
        hasher.update(&buffer[..n]);
        dst.write_all(&buffer[..n])?;
        done += n as u64;
        report(done);
    }
    Ok(hex(&hasher.finalize()))
}

/// Compare `len` bytes of `written` with `src`, returning the SHA-256 of
/// what was read back.
fn verify(src: &mut impl Read, written: &mut impl Read, len: u64, report: &mut dyn FnMut(u64)) -> Result<String> {
    let mut hasher = Sha256::new();
    let (mut a, mut b) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    let mut done = 0u64;
    while done < len {
        let n = CHUNK.min((len - done) as usize);
        src.read_exact(&mut a[..n])?;
        written.read_exact(&mut b[..n])?;
        if a[..n] != b[..n] {
            let at = a[..n].iter().zip(&b[..n]).position(|(x, y)| x != y).unwrap_or(0);
            bail!("the drive differs from the image at byte {}", done + at as u64);
        }
        hasher.update(&b[..n]);
        done += n as u64;
        report(done);
    }
    Ok(hex(&hasher.finalize()))
}

/// What the privileged step is asked to do.
struct Plan {
    image: PathBuf,
    drive: Drive,
    sha256: String,
}

impl Plan {
    fn encode(&self) -> String {
        serde_json::json!({
            "image": self.image, "sha256": self.sha256, "name": self.drive.name, "bytes": self.drive.bytes,
            "vendor": self.drive.vendor, "model": self.drive.model, "serial": self.drive.serial, "bus": self.drive.bus,
        })
        .to_string()
    }

    fn decode(text: &str) -> Result<Plan> {
        let v: serde_json::Value = serde_json::from_str(text)?;
        let s = |k: &str| v[k].as_str().map(str::to_owned).with_context(|| format!("plan lacks {k}"));
        Ok(Plan {
            image: PathBuf::from(s("image")?),
            sha256: s("sha256")?,
            drive: Drive {
                name: s("name")?,
                bytes: v["bytes"].as_u64().context("plan lacks bytes")?,
                vendor: s("vendor")?,
                model: s("model")?,
                serial: s("serial")?,
                bus: s("bus")?,
                ..Drive::default()
            },
        })
    }
}

/// The drive named in the plan, as it is now, if it is still the same one.
fn still_there(plan: &Plan) -> Result<Drive> {
    let now = platform::drives()?
        .into_iter()
        .find(|d| d.name == plan.drive.name)
        .with_context(|| format!("{} is gone; was it unplugged?", plan.drive.name))?;
    ensure!(now.same(&plan.drive), "{} is no longer the drive you picked; nothing was written", plan.drive.name);
    ensure!(
        now.excluded(None, true).is_none(),
        "{} may not be written: {}",
        now.name,
        now.excluded(None, true).unwrap_or("")
    );
    Ok(now)
}

/// The privileged step: check, write, read back, report, eject.
fn write(plan: &Plan) -> Result<()> {
    let drive = still_there(plan)?;
    platform::unmount(&drive)?;
    let (mut device, bytes) = platform::open(&drive)?;
    ensure!(bytes == drive.bytes, "{} reports {bytes} bytes, not {}", drive.name, drive.bytes);
    // Opened: check once more that it is still the drive that was picked.
    still_there(plan)?;
    let mut image = File::open(&plan.image)?;
    let len = image.metadata()?.len();
    ensure!(len % 512 == 0, "the image is not a whole number of 512-byte sectors");
    ensure!(len + TAIL <= bytes, "the image ({}) doesn't fit on {} ({})", human(len), drive.name, human(bytes));

    let start = Instant::now();
    let written = copy(&mut image, &mut device, len, &mut |done| progress("Writing", done, len, start))?;
    eprintln!();
    ensure!(written == plan.sha256, "the image changed while it was being written (SHA-256 {written})");
    device.seek(SeekFrom::Start(bytes - TAIL))?;
    device.write_all(&vec![0u8; TAIL as usize])?;
    device.sync_all()?;
    platform::drop_cache(&device);

    image.seek(SeekFrom::Start(0))?;
    device.seek(SeekFrom::Start(0))?;
    let start = Instant::now();
    let read = verify(&mut image, &mut device, len, &mut |done| progress("Reading back", done, len, start))?;
    eprintln!();
    ensure!(read == plan.sha256, "what was read back has SHA-256 {read}");
    device.seek(SeekFrom::Start(bytes - TAIL))?;
    let mut tail = vec![0u8; TAIL as usize];
    device.read_exact(&mut tail)?;
    ensure!(tail.iter().all(|&b| b == 0), "the end of the drive was not cleared");
    drop(device);

    let report = serde_json::json!({
        "passed": true,
        "image": plan.image.file_name().map(|n| n.to_string_lossy().into_owned()),
        "image_bytes": len,
        "sha256": plan.sha256,
        "device": drive.name,
        "drive": drive.label(),
        "serial": drive.serial,
        "drive_bytes": bytes,
        "tail_cleared_bytes": TAIL,
        "written_at": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    });
    let report_path = PathBuf::from(format!("{}.flash.json", plan.image.display()));
    fs::write(&report_path, serde_json::to_string_pretty(&report)? + "\n")?;
    // The report belongs to whoever ran sudo, not to root.
    if let (Ok(uid), Ok(gid)) = (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
        && let (Ok(uid), Ok(gid)) = (uid.parse(), gid.parse())
    {
        let path = std::ffi::CString::new(report_path.to_string_lossy().as_bytes())?;
        // SAFETY: a valid C string; chown has no other preconditions.
        unsafe { libc::chown(path.as_ptr(), uid, gid) };
    }
    match platform::eject(&drive) {
        Ok(()) => println!("PASS: {} written, every byte read back and compared, drive ejected.", drive.name),
        Err(e) => println!("PASS: {} written and every byte read back and compared. {e}", drive.name),
    }
    println!("Report: {}", report_path.display());
    Ok(())
}

fn ask(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    ensure!(std::io::stdin().lock().read_line(&mut line)? > 0, "no answer");
    Ok(line.trim().to_owned())
}

/// A number from 1 to `count`; `q` quits without writing anything.
fn pick(prompt: &str, count: usize) -> Result<usize> {
    loop {
        let answer = ask(prompt)?;
        if answer.eq_ignore_ascii_case("q") {
            println!("Nothing was written.");
            std::process::exit(0);
        }
        match answer.parse::<usize>() {
            Ok(n) if (1..=count).contains(&n) => return Ok(n - 1),
            _ => println!("Type a number from 1 to {count}, or q."),
        }
    }
}

fn age(time: SystemTime) -> String {
    let seconds = SystemTime::now().duration_since(time).map(|d| d.as_secs()).unwrap_or(0);
    match seconds {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86399 => format!("{} h ago", seconds / 3600),
        86400..=172799 => "1 day ago".to_owned(),
        _ => format!("{} days ago", seconds / 86400),
    }
}

/// When a file arrived where it is: the later of when its contents and its
/// metadata last changed. An image copied with its times preserved (the Nix
/// store dates everything 1970) keeps its old modification time, but the
/// copy changed its status time.
fn arrived(meta: &fs::Metadata) -> SystemTime {
    use std::os::unix::fs::MetadataExt;
    let changed = SystemTime::UNIX_EPOCH
        + std::time::Duration::new(meta.ctime().max(0) as u64, meta.ctime_nsec().clamp(0, 999_999_999) as u32);
    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH).max(changed)
}

/// `*.iso` files in `dir` and below, to `depth` levels.
fn find_images(dir: &Path, depth: usize, into: &mut Vec<(PathBuf, SystemTime, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() && depth > 0 && !entry.file_name().to_string_lossy().starts_with('.') {
            find_images(&path, depth - 1, into);
        } else if meta.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("iso")) {
            into.push((path, arrived(&meta), meta.len()));
        }
    }
}

/// The images in each `(directory, depth)`, each once, most recent first.
fn newest_images(roots: &[(&Path, usize)]) -> Vec<(PathBuf, SystemTime, u64)> {
    let mut images = Vec::new();
    for (dir, depth) in roots {
        find_images(dir, *depth, &mut images);
    }
    let mut seen = std::collections::HashSet::new();
    images.retain(|(path, _, _)| seen.insert(fs::canonicalize(path).unwrap_or_else(|_| path.clone())));
    images.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    images
}

fn choose_image() -> Result<PathBuf> {
    const SHOWN: usize = 9;
    let mut images = newest_images(&[(Path::new("."), 0), (Path::new("artifacts"), 2)]);
    ensure!(!images.is_empty(), "no .iso files here or under artifacts/; pass the image's path");
    let older = images.len().saturating_sub(SHOWN);
    images.truncate(SHOWN);
    println!("Images, most recent first:");
    for (i, (path, time, len)) in images.iter().enumerate() {
        println!("  {}) {}  {}  {}", i + 1, path.display(), human(*len), age(*time));
    }
    if older > 0 {
        println!("  ({older} older not shown; pass an image's path to use one of them)");
    }
    let index = pick(&format!("Pick an image (1-{}), or q to quit: ", images.len()), images.len())?;
    Ok(images.swap_remove(index).0)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut image, mut sha256, mut all, mut list) = (None, None, false, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // The privileged step, run through sudo by this same program.
            "--write" => return write(&Plan::decode(&args.next().context("--write needs a plan")?)?),
            "--sha256" => sha256 = Some(args.next().context("--sha256 needs a value")?),
            "--all" => all = true,
            "--list" => list = true,
            "-h" | "--help" => {
                println!("Usage: flash_usb.rs [IMAGE] [--sha256 HEX] [--all] [--list]");
                return Ok(());
            }
            flag if flag.starts_with('-') => bail!("unknown option {flag}"),
            path => image = Some(PathBuf::from(path)),
        }
    }

    if list {
        let image_drive = image.as_deref().and_then(image_drive);
        for drive in platform::drives()? {
            match drive.excluded(image_drive.as_deref(), all) {
                None => println!("  offered   {}", drive.summary()),
                Some(why) => println!("  left out  {}  ({why})", drive.summary()),
            }
        }
        return Ok(());
    }

    let image = match image {
        Some(path) => path,
        None => choose_image()?,
    };
    let image = fs::canonicalize(&image).with_context(|| format!("no image at {}", image.display()))?;
    let len = fs::metadata(&image)?.len();
    let expected = expected_sha256(&image, sha256.as_deref())?;
    println!("Image: {} ({})", image.display(), human(len));

    // Hash the image while the drive is being chosen.
    let hashed = Arc::new(AtomicU64::new(0));
    let hashing = {
        let (image, hashed) = (image.clone(), hashed.clone());
        std::thread::spawn(move || sha256_file(&image, &hashed))
    };

    let image_drive = image_drive(&image);
    let drives: Vec<Drive> =
        platform::drives()?.into_iter().filter(|d| d.excluded(image_drive.as_deref(), all).is_none()).collect();
    if drives.is_empty() {
        bail!(
            "no {} found. Plug one in and run this again; --list shows every drive and why it isn't offered.",
            if all { "external drives" } else { "USB flash drives" }
        );
    }
    println!("{}", if all { "External drives:" } else { "Drives that look like USB flash drives:" });
    for (i, drive) in drives.iter().enumerate() {
        println!("  {}) {}", i + 1, drive.summary());
    }
    let index = pick(&format!("Pick a drive (1-{}), or q to quit: ", drives.len()), drives.len())?;
    let drive = drives[index].clone();
    ensure!(
        len + TAIL <= drive.bytes,
        "the image ({}) doesn't fit on {} ({})",
        human(len),
        drive.name,
        human(drive.bytes)
    );

    let start = Instant::now();
    while !hashing.is_finished() {
        progress("Checking the image:", hashed.load(Ordering::Relaxed), len, start);
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let sha = hashing.join().map_err(|_| anyhow::anyhow!("hashing stopped"))??;
    eprint!("\r{:80}\r", "");
    match &expected {
        Some((want, source)) if *want == sha => println!("SHA-256 {sha}, matching {source}."),
        Some((want, source)) => bail!("the image's SHA-256 is {sha}, but {source} says {want}; nothing was written"),
        None => println!("SHA-256 {sha} (there is no checksum file beside the image to compare it with)."),
    }

    println!();
    println!("Everything on {} will be erased:", drive.name);
    println!("  {}", drive.summary());
    let typed = ask(&format!("Type {} to write the image to it: ", drive.name))?;
    ensure!(typed == drive.name, "not confirmed; nothing was written");

    let plan = Plan { image, drive, sha256: sha };
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return write(&plan);
    }
    println!("Writing needs administrator rights; sudo may ask for your password.");
    let status = Command::new("sudo").arg("--").arg(std::env::current_exe()?).arg("--write").arg(plan.encode()).status()?;
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn drive() -> Drive {
        Drive {
            name: "sdb".into(),
            bytes: 64_000_000_000,
            vendor: "SanDisk".into(),
            model: "3.2Gen1".into(),
            serial: "0401abcdef0123456789be1c".into(),
            bus: "USB".into(),
            usb_or_removable: true,
            ..Drive::default()
        }
    }

    #[test]
    fn only_external_usb_drives_are_offered() {
        let usb = drive();
        assert_eq!(usb.excluded(None, false), None);
        assert_eq!(usb.excluded(Some("sdb"), true), Some("holds the image"));
        assert!(Drive { internal: true, ..drive() }.excluded(None, true).is_some());
        assert!(Drive { system: true, ..drive() }.excluded(None, true).is_some());
        assert!(Drive { virtual_drive: true, ..drive() }.excluded(None, true).is_some());
        assert!(Drive { read_only: true, ..drive() }.excluded(None, true).is_some());
        let ssd = Drive { usb_or_removable: false, bytes: 4_000_000_000_000, ..drive() };
        assert!(ssd.excluded(None, false).is_some());
        assert_eq!(ssd.excluded(None, true), None);
        assert!(!drive().same(&Drive { serial: "other".into(), ..drive() }));
        assert!(drive().same(&Drive { mounts: vec!["/media/x".into()], ..drive() }));
    }

    #[test]
    fn plans_survive_the_trip_through_sudo() {
        let plan = Plan { image: "/tmp/a b.iso".into(), drive: drive(), sha256: "ab".repeat(32) };
        let back = Plan::decode(&plan.encode()).unwrap();
        assert!(back.drive.same(&plan.drive));
        assert_eq!((back.image, back.sha256), (plan.image, plan.sha256));
    }

    #[test]
    fn copies_and_verifies_byte_for_byte() {
        let data: Vec<u8> = (0..(CHUNK * 2 + 4096)).map(|i| (i * 7 % 251) as u8).collect();
        let mut target = Cursor::new(vec![0u8; data.len() + 1024]);
        let len = data.len() as u64;
        let written = copy(&mut Cursor::new(&data), &mut target, len, &mut |_| {}).unwrap();
        assert_eq!(written, hex(&Sha256::digest(&data)));
        target.set_position(0);
        assert_eq!(verify(&mut Cursor::new(&data), &mut target, len, &mut |_| {}).unwrap(), written);
        let mut damaged = target.into_inner();
        damaged[CHUNK + 5] ^= 1;
        let error = verify(&mut Cursor::new(&data), &mut Cursor::new(damaged), len, &mut |_| {}).unwrap_err();
        assert!(error.to_string().contains(&format!("byte {}", CHUNK + 5)), "{error}");
    }

    #[test]
    fn checksums_from_the_flag_or_files_beside_the_image() {
        let dir = std::env::temp_dir().join(format!("flash-usb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let image = dir.join("x.iso");
        fs::write(&image, b"iso").unwrap();
        assert_eq!(expected_sha256(&image, None).unwrap(), None);
        let hex = "f".repeat(64);
        fs::write(dir.join("SHA256SUMS"), format!("{} other.iso\n{hex}  x.iso\n", "0".repeat(64))).unwrap();
        assert_eq!(expected_sha256(&image, None).unwrap().unwrap().0, hex);
        fs::write(dir.join("x.iso.sha256"), format!("{}  x.iso\n", "a".repeat(64))).unwrap();
        assert_eq!(expected_sha256(&image, None).unwrap().unwrap().0, "a".repeat(64));
        assert_eq!(expected_sha256(&image, Some(&"B".repeat(64))).unwrap().unwrap().0, "b".repeat(64));
        assert!(expected_sha256(&image, Some("nope")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn images_most_recent_first() {
        let dir = std::env::temp_dir().join(format!("flash-usb-images-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/deep/deeper")).unwrap();
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
        // Copied first and second with Nix's 1970 times, then built here.
        for (name, preserved) in [("first.iso", true), ("a/second.iso", true), ("third.iso", false)] {
            let file = File::create(dir.join(name)).unwrap();
            if preserved {
                file.set_modified(old).unwrap();
            }
            drop(file);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        fs::write(dir.join("a/deep/deeper/too-deep.iso"), b"").unwrap();
        fs::write(dir.join("notes.txt"), b"").unwrap();
        let found = newest_images(&[(&dir, 2), (&dir.join("a"), 0)]);
        let _ = fs::remove_dir_all(&dir);
        let names: Vec<String> = found.iter().map(|(p, _, _)| p.file_name().unwrap().to_string_lossy().into()).collect();
        // Most recent first, each once, nothing deeper than asked or not an image.
        assert_eq!(names, ["third.iso", "second.iso", "first.iso"]);
    }

    #[test]
    fn sizes_as_drive_makers_count() {
        assert_eq!(human(123_048_296_448), "123.0 GB");
        assert_eq!(human(512), "512 B");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn lsblk_drives() {
        let json = br#"{"blockdevices":[
          {"name":"nvme0n1","size":2000398934016,"type":"disk","tran":"nvme","rm":false,"hotplug":false,"ro":false,
           "vendor":null,"model":"Samsung SSD","serial":"S1","mountpoints":[null],
           "children":[{"name":"nvme0n1p2","size":1,"type":"part","mountpoints":["/","/nix/store"]}]},
          {"name":"sdb","size":"64023257088","type":"disk","tran":"usb","rm":"1","hotplug":"1","ro":"0",
           "vendor":"SanDisk ","model":"Ultra","serial":"4C53","mountpoint":null,
           "children":[{"name":"sdb1","size":1,"type":"part","mountpoint":"/run/media/me/USB"}]},
          {"name":"zram0","size":8589934592,"type":"disk","tran":null,"rm":false,"hotplug":false,"ro":false,
           "mountpoints":["[SWAP]"]},
          {"name":"loop0","size":1,"type":"loop","mountpoints":[null]}
        ]}"#;
        let drives = platform::parse(json).unwrap();
        assert_eq!(drives.len(), 3);
        let offered: Vec<&str> =
            drives.iter().filter(|d| d.excluded(None, true).is_none()).map(|d| d.name.as_str()).collect();
        assert_eq!(offered, ["sdb"]);
        let sdb = &drives[1];
        assert_eq!((sdb.bytes, sdb.vendor.as_str(), sdb.bus.as_str()), (64023257088, "SanDisk", "USB"));
        assert_eq!(sdb.mounts, ["/run/media/me/USB"]);
        assert!(drives[0].system && drives[0].internal);
    }
}
