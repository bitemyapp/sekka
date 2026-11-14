//! The write, as the privileged helper does it: check the request against
//! the drive as it is now, write the image, clear the drive's end, read
//! every byte back and compare, and eject.
//!
//! Only the helper depends on this crate. The window can't read a request
//! for any drive and write it in its own process: it builds
//! [`Plan`](kakehashi_core::write::Plan)s, which only the dialog's
//! confirmation makes, and starts the helper.

mod platform;
mod recheck;

pub use recheck::{Recheck, StillThere, still_there};

use anyhow::{Context, Result, bail, ensure};
use kakehashi_core::drive::{Drive, human};
use kakehashi_core::image::{CHUNK, Cancelled, hex};
use kakehashi_core::write::{Progress, TAIL};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A [`Plan`](kakehashi_core::write::Plan) as the helper reads it from the window's file. Anything may
/// have written that file, so it is only a request: the helper checks its
/// drive against the drive as it is ([`still_there`]) before it unmounts
/// or opens anything.
#[derive(Debug, Deserialize)]
pub struct Requested {
    image: PathBuf,
    sha256: String,
    drive: Drive,
}

impl Requested {
    pub fn image(&self) -> &Path {
        &self.image
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn drive(&self) -> &Drive {
        &self.drive
    }
}

/// A buffer aligned for unbuffered I/O, which Windows requires.
struct Aligned {
    ptr: *mut u8,
    layout: Layout,
}

impl Aligned {
    fn new(len: usize) -> Aligned {
        let layout = Layout::from_size_align(len.max(1), 4096).expect("buffer layout");
        // SAFETY: the layout has a nonzero size.
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(!ptr.is_null(), "out of memory");
        Aligned { ptr, layout }
    }

    fn slice(&mut self, len: usize) -> &mut [u8] {
        assert!(len <= self.layout.size());
        // SAFETY: `ptr` is valid for `layout.size()` bytes and owned here.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, len) }
    }
}

impl Drop for Aligned {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with this layout.
        unsafe { dealloc(self.ptr, self.layout) }
    }
}

fn round_up(n: u64, to: u64) -> u64 {
    n.div_ceil(to) * to
}

/// Write `len` bytes of `src` to `dst` in whole sectors, the last one padded
/// with zeros, and return the SHA-256 of what was read.
pub(crate) fn copy(
    src: &mut impl Read,
    dst: &mut impl Write,
    len: u64,
    sector: u64,
    report: &mut dyn FnMut(u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = Aligned::new(CHUNK);
    let mut done = 0u64;
    while done < len {
        if cancelled() {
            bail!(Cancelled);
        }
        let n = CHUNK.min((len - done) as usize);
        let padded = round_up(n as u64, sector) as usize;
        let chunk = buffer.slice(padded);
        src.read_exact(&mut chunk[..n])?;
        chunk[n..].fill(0);
        hasher.update(&chunk[..n]);
        dst.write_all(chunk)?;
        done += n as u64;
        report(done);
    }
    Ok(hex(&hasher.finalize()))
}

/// Compare `len` bytes of `written` with `src`, reading whole sectors, and
/// return the SHA-256 of what was read back.
pub(crate) fn verify(
    src: &mut impl Read,
    written: &mut impl Read,
    len: u64,
    sector: u64,
    report: &mut dyn FnMut(u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let mut hasher = Sha256::new();
    let (mut a, mut b) = (vec![0u8; CHUNK], Aligned::new(CHUNK));
    let mut done = 0u64;
    while done < len {
        if cancelled() {
            bail!(Cancelled);
        }
        let n = CHUNK.min((len - done) as usize);
        let padded = round_up(n as u64, sector) as usize;
        src.read_exact(&mut a[..n])?;
        let back = b.slice(padded);
        written.read_exact(back)?;
        if a[..n] != back[..n] {
            let at = a[..n].iter().zip(&back[..n]).position(|(x, y)| x != y).unwrap_or(0);
            bail!("the drive differs from the image at byte {}", done + at as u64);
        }
        hasher.update(&back[..n]);
        done += n as u64;
        report(done);
    }
    Ok(hex(&hasher.finalize()))
}

/// What a finished write leaves.
pub struct Outcome {
    pub ejected: bool,
    pub note: Option<String>,
}
/// The privileged step. `touched` is set once the drive may have changed.
///
/// `report` sends a stage to the window. The first, `Preparing`, must reach
/// it: the window takes a helper that reported nothing as one that never
/// looked at the drive, so if that report fails nothing is done. Later
/// reports that fail are dropped rather than stop a write half done.
pub fn execute(
    plan: &Requested,
    report: &mut dyn FnMut(Progress) -> std::io::Result<()>,
    cancelled: &dyn Fn() -> bool,
    touched: &mut bool,
) -> Result<Outcome> {
    report(Progress::Preparing).context("could not report to the window, so nothing was done")?;
    let confirmed = still_there(plan)?;
    let mut image = File::open(&plan.image).with_context(|| format!("could not open {}", plan.image.display()))?;
    let len = image.metadata()?.len();
    ensure!(len > 0, "the image is empty");
    platform::unmount(&confirmed)?;
    let mut device = platform::open(&confirmed, cancelled)?;
    let drive = confirmed.target();
    ensure!(device.bytes == drive.bytes, "{} reports {} bytes, not {}", drive.name, device.bytes, drive.bytes);
    // Opened: check once more that it is still the drive that was picked.
    let _still = still_there(plan)?;
    let sector = device.sector;
    ensure!(
        round_up(len, sector) + TAIL <= device.bytes,
        "the image ({}) doesn't fit on {} ({})",
        human(len),
        drive.label(),
        human(device.bytes)
    );

    if cancelled() {
        bail!(Cancelled);
    }
    *touched = true;
    let written = copy(
        &mut image,
        &mut device.file,
        len,
        sector,
        &mut |done| {
            let _ = report(Progress::Writing { done, total: len });
        },
        cancelled,
    )?;
    ensure!(written == plan.sha256, "the image changed while it was being written (SHA-256 {written})");
    device.file.seek(SeekFrom::Start(device.bytes - TAIL))?;
    let mut tail = Aligned::new(TAIL as usize);
    device.file.write_all(tail.slice(TAIL as usize))?;
    device.file.flush()?;
    platform::sync(&device)?;
    platform::drop_cache(&device);

    image.seek(SeekFrom::Start(0))?;
    device.file.seek(SeekFrom::Start(0))?;
    let read = verify(
        &mut image,
        &mut device.file,
        len,
        sector,
        &mut |done| {
            let _ = report(Progress::Verifying { done, total: len });
        },
        cancelled,
    )?;
    ensure!(read == plan.sha256, "what was read back has SHA-256 {read}");
    device.file.seek(SeekFrom::Start(device.bytes - TAIL))?;
    let back = tail.slice(TAIL as usize);
    device.file.read_exact(back)?;
    ensure!(back.iter().all(|&b| b == 0), "the end of the drive was not cleared");
    drop(device);

    Ok(match platform::eject(drive) {
        Ok(()) => Outcome { ejected: true, note: None },
        Err(e) => Outcome { ejected: false, note: Some(format!("{e:#}")) },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn copies_whole_sectors_and_verifies() {
        // Not a multiple of the sector, and longer than one chunk.
        let len = CHUNK as u64 + 1000;
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let mut target = Cursor::new(Vec::new());
        let mut writes = Vec::new();
        let sha = copy(&mut Cursor::new(&data), &mut target, len, 4096, &mut |d| writes.push(d), &|| false).unwrap();
        let out = target.into_inner();
        assert_eq!(out.len() as u64, round_up(len, 4096));
        assert_eq!(&out[..len as usize], &data[..]);
        assert!(out[len as usize..].iter().all(|&b| b == 0));
        assert_eq!(writes.last(), Some(&len));
        assert_eq!(sha, hex(&Sha256::digest(&data)));

        let back = verify(&mut Cursor::new(&data), &mut Cursor::new(&out), len, 4096, &mut |_| {}, &|| false).unwrap();
        assert_eq!(back, sha);
    }

    #[test]
    fn verify_finds_the_first_difference() {
        let data = vec![7u8; 10_000];
        let mut bad = data.clone();
        bad.resize(round_up(10_000, 512) as usize, 0);
        bad[4321] = 8;
        let err =
            verify(&mut Cursor::new(&data), &mut Cursor::new(&bad), 10_000, 512, &mut |_| {}, &|| false).unwrap_err();
        assert!(err.to_string().contains("byte 4321"), "{err}");
    }

    #[test]
    fn stops_when_cancelled() {
        let data = vec![1u8; 100];
        let err =
            copy(&mut Cursor::new(&data), &mut Cursor::new(Vec::new()), 100, 512, &mut |_| {}, &|| true).unwrap_err();
        assert!(err.is::<Cancelled>());
    }

    fn requested(drive: &Drive) -> Requested {
        use kakehashi_core::choice::{DriveList, DriveListing, ImageId};
        use kakehashi_core::write::Plan;
        let mut list = DriveList::new();
        let listing = list.request(std::time::Instant::now()).unwrap();
        list.listed(DriveListing::new(listing, Ok(vec![drive.clone()])));
        list.choose(drive);
        let image = ImageId { path: "/images/sekka.iso".into(), sha256: "ab".repeat(32) };
        let (review, _) = list.open_review(None, image.clone()).unwrap();
        let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap());
        serde_json::from_str(&serde_json::to_string(&plan).unwrap()).unwrap()
    }

    #[test]
    fn the_helper_reads_the_plan_the_window_writes() {
        let drive = Drive {
            name: "sdzz".into(),
            bytes: 1 << 30,
            serial: "K1".into(),
            usb_or_removable: true,
            ..Drive::default()
        };
        let back = requested(&drive);
        assert_eq!(
            (back.image(), back.sha256(), back.drive()),
            (Path::new("/images/sekka.iso"), &*"ab".repeat(32), &drive)
        );
    }

    #[test]
    fn nothing_is_done_if_the_window_cant_be_told() {
        let drive = Drive {
            name: "sdzz".into(),
            bytes: 1 << 30,
            serial: "K1".into(),
            usb_or_removable: true,
            ..Drive::default()
        };
        let mut touched = false;
        let mut reports = 0;
        let err = execute(
            &requested(&drive),
            &mut |_| {
                reports += 1;
                Err(std::io::Error::other("no space left"))
            },
            &|| false,
            &mut touched,
        )
        .err()
        .expect("the write went on");
        // Not "sdzz is gone": it stopped before looking for the drive.
        assert!(format!("{err:#}").starts_with("could not report to the window"), "{err:#}");
        assert_eq!((reports, touched), (1, false));
    }
}
