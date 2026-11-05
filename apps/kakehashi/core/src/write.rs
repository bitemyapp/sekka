//! The write, as the privileged helper does it: check the plan against the
//! drive as it is now, write the image, clear the drive's end, read every
//! byte back and compare, and eject.

use crate::choice::ConfirmedWrite;
use crate::drive::{Drive, human};
use crate::image::{CHUNK, Cancelled, hex};
use crate::platform;
use aizome_typestate::{Confirmed, Reviewed};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Cleared at the end of the drive: a previous GPT's backup header lives in
/// the last sectors, and would confuse firmware.
pub const TAIL: u64 = 1024 * 1024;

/// What the window asks the helper to do, as the window writes it to
/// `plan.json`. The helper reads the file as a [`Requested`].
///
/// The window makes one only from the drive and image the open dialog
/// described, found again when OK was pressed ([`ConfirmedWrite`]).
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Plan {
    image: PathBuf,
    /// The image's SHA-256 as the window computed it: what is written and
    /// what is read back must both have it.
    sha256: String,
    drive: Drive,
}

impl Plan {
    /// The write the person confirmed: that image onto that drive.
    ///
    /// A plan for the drive chosen in the list as it is now, rather than
    /// the one the confirmation dialog named, doesn't compile (KTL-04): the
    /// list keeps refreshing under the dialog, and the next stick plugged
    /// in takes the same name.
    ///
    /// ```compile_fail,E0308
    /// use kakehashi_core::{Drive, choice::DriveList, write::Plan};
    /// fn ok_pressed(list: &DriveList) -> Option<Plan> {
    ///     let chosen_now: Drive = list.chosen(None)?.clone();
    ///     Some(Plan::new(chosen_now))
    /// }
    /// ```
    ///
    /// Nor does reading one from JSON naming any drive:
    ///
    /// ```compile_fail,E0277
    /// use kakehashi_core::{Drive, write::Plan};
    /// fn ok_pressed(chosen_now: &Drive) -> Plan {
    ///     let json = serde_json::json!({ "image": "sekka.iso", "sha256": "0", "drive": chosen_now });
    ///     serde_json::from_value::<Plan>(json).unwrap()
    /// }
    /// ```
    ///
    /// The open dialog's drive and image, found again when OK is pressed:
    ///
    /// ```
    /// use kakehashi_core::choice::{DriveList, ImageId, ReviewTicket};
    /// use kakehashi_core::write::Plan;
    /// fn ok_pressed(list: &mut DriveList, review: ReviewTicket, image: ImageId) -> Option<Plan> {
    ///     Some(Plan::new(list.confirm(review, None, Some(image)).ok()?))
    /// }
    /// ```
    pub fn new(write: ConfirmedWrite) -> Plan {
        let (drive, image) = write.into_target().into_parts();
        Plan { image: image.path, sha256: image.sha256, drive }
    }

    /// The same write from a copy of the image (on macOS, one the helper
    /// may read). The checksum stays: the copy must have it.
    pub fn with_image(self, image: PathBuf) -> Plan {
        Plan { image, ..self }
    }

    pub fn image(&self) -> &Path {
        &self.image
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// The drive as the person confirmed it.
    pub fn drive(&self) -> &Drive {
        &self.drive
    }
}

/// A [`Plan`] as the helper reads it from the window's file. Anything may
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

/// What the helper reports, one JSON object per line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Progress {
    Preparing,
    Writing {
        done: u64,
        total: u64,
    },
    Verifying {
        done: u64,
        total: u64,
    },
    Finished {
        ejected: bool,
        note: Option<String>,
    },
    /// `touched`: the drive may have been changed before it stopped.
    Failed {
        message: String,
        touched: bool,
    },
}

/// The brand of the helper's re-check. Only this module makes one, in
/// [`still_there`], so the rest of the core can't unmount or open a drive
/// without the check.
#[derive(Debug)]
pub struct Recheck(());

/// The requested drive, found again as it is now.
pub type StillThere = Confirmed<Drive, Recheck>;

/// The drive named in the request, as it is now, if it is still the same
/// one ([`Drive::same`]) and still one that may be written. Unmounting and
/// opening a drive take the proof.
pub fn still_there(plan: &Requested) -> Result<StillThere> {
    // The request carries the drive as the person confirmed it in the window.
    let reviewed = Reviewed::new(plan.drive.clone(), Recheck(()));
    let now = platform::drives()?
        .into_iter()
        .find(|d| d.name == plan.drive.name)
        .with_context(|| format!("{} is gone; was it unplugged?", plan.drive.label()))?;
    let Ok(now) = reviewed.confirm(now) else {
        bail!("{} is no longer the drive you picked; nothing was written", plan.drive.name);
    };
    if let Some(why) = now.target().excluded(None, true) {
        bail!("{} may not be written: it is {}", now.target().label(), why.reason());
    }
    Ok(now)
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

    #[test]
    fn progress_is_one_json_object_per_line() {
        let line = serde_json::to_string(&Progress::Writing { done: 1, total: 2 }).unwrap();
        assert_eq!(line, r#"{"stage":"writing","done":1,"total":2}"#);
        let back: Progress = serde_json::from_str(&line).unwrap();
        assert_eq!(back, Progress::Writing { done: 1, total: 2 });
    }

    fn requested(drive: &Drive) -> Requested {
        use crate::choice::{DriveList, DriveListing, ImageId};
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
