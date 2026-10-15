//! The write, as the privileged helper does it: check the plan against the
//! drive as it is now, write the image, clear the drive's end, read every
//! byte back and compare, and eject.

use crate::drive::{Drive, human};
use crate::image::{CHUNK, Cancelled, hex};
use crate::platform;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

/// Cleared at the end of the drive: a previous GPT's backup header lives in
/// the last sectors, and would confuse firmware.
pub const TAIL: u64 = 1024 * 1024;

/// What the window asks the helper to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub image: PathBuf,
    /// The image's SHA-256 as the window computed it: what is written and
    /// what is read back must both have it.
    pub sha256: String,
    pub drive: Drive,
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

/// The drive named in the plan, as it is now, if it is still the same one
/// and still one that may be written.
pub fn still_there(plan: &Plan) -> Result<Drive> {
    let now = platform::drives()?
        .into_iter()
        .find(|d| d.name == plan.drive.name)
        .with_context(|| format!("{} is gone; was it unplugged?", plan.drive.label()))?;
    ensure!(now.same(&plan.drive), "{} is no longer the drive you picked; nothing was written", plan.drive.name);
    if let Some(why) = now.excluded(None, true) {
        bail!("{} may not be written: it is {}", now.label(), why.reason());
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
pub fn execute(
    plan: &Plan,
    report: &mut dyn FnMut(Progress),
    cancelled: &dyn Fn() -> bool,
    touched: &mut bool,
) -> Result<Outcome> {
    report(Progress::Preparing);
    let drive = still_there(plan)?;
    let mut image = File::open(&plan.image).with_context(|| format!("could not open {}", plan.image.display()))?;
    let len = image.metadata()?.len();
    ensure!(len > 0, "the image is empty");
    platform::unmount(&drive)?;
    let mut device = platform::open(&drive, cancelled)?;
    ensure!(device.bytes == drive.bytes, "{} reports {} bytes, not {}", drive.name, device.bytes, drive.bytes);
    // Opened: check once more that it is still the drive that was picked.
    still_there(plan)?;
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
        &mut |done| report(Progress::Writing { done, total: len }),
        cancelled,
    )?;
    ensure!(written == plan.sha256, "the image changed while it was being written (SHA-256 {written})");
    device.file.seek(SeekFrom::Start(device.bytes - TAIL))?;
    let mut tail = Aligned::new(TAIL as usize);
    device.file.write_all(tail.slice(TAIL as usize))?;
    device.file.flush()?;
    device.file.sync_all()?;
    platform::drop_cache(&device);

    image.seek(SeekFrom::Start(0))?;
    device.file.seek(SeekFrom::Start(0))?;
    let read = verify(
        &mut image,
        &mut device.file,
        len,
        sector,
        &mut |done| report(Progress::Verifying { done, total: len }),
        cancelled,
    )?;
    ensure!(read == plan.sha256, "what was read back has SHA-256 {read}");
    device.file.seek(SeekFrom::Start(device.bytes - TAIL))?;
    let back = tail.slice(TAIL as usize);
    device.file.read_exact(back)?;
    ensure!(back.iter().all(|&b| b == 0), "the end of the drive was not cleared");
    drop(device);

    Ok(match platform::eject(&drive) {
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
}
