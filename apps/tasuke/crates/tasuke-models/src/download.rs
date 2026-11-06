//! Downloading a model's files from Hugging Face, resumably and verified.
//!
//! Each file goes to `<file>.partial` with HTTP Range resume, hashed while it
//! streams; a resumed download first re-hashes the bytes already there. Once
//! the size and sha256 match the catalog the file is synced and renamed to its
//! final name. A mismatch deletes the partial file and fails: the next attempt
//! starts that file again.
//!
//! The download runs on its own thread at idle IO priority, one at a time per
//! store (a lock file), and retries network errors with exponential backoff.
//! Progress, pauses and the outcome come over a channel; pause and cancel are
//! honoured within one read. A cancelled download keeps its partial files, so
//! it resumes where it stopped.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256 as Hasher};

use crate::catalog::{Entry, ModelFile};
use crate::store::{Installed, Store};
use crate::units::{Bytes, BytesPerSecond, ModelId, Sha256};

pub const DEFAULT_ENDPOINT: &str = "https://huggingface.co";
/// Kept free beyond what the files need.
pub const SPACE_MARGIN: Bytes = Bytes::mib(256);
const READ_BUFFER: usize = 1 << 20;
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// Called after each event, so a window can redraw.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct DownloadOptions {
    /// `HF_ENDPOINT`, else huggingface.co.
    pub endpoint: String,
    /// `HF_TOKEN`, sent to the endpoint (not to the CDN it redirects to).
    pub token: Option<String>,
    /// Attempts per file before giving up on network errors.
    pub max_attempts: u32,
    /// The first retry's delay; it doubles each time.
    pub backoff: Duration,
    /// A read that waits this long counts as a network error (and is retried).
    pub read_timeout: Duration,
    /// Run at idle IO priority.
    pub idle_io: bool,
    pub waker: Option<Waker>,
}

impl std::fmt::Debug for DownloadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadOptions")
            .field("endpoint", &self.endpoint)
            .field("token", &self.token.as_ref().map(|_| "…"))
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl Default for DownloadOptions {
    fn default() -> DownloadOptions {
        DownloadOptions {
            endpoint: DEFAULT_ENDPOINT.to_owned(),
            token: None,
            max_attempts: 6,
            backoff: Duration::from_secs(1),
            read_timeout: Duration::from_secs(60),
            idle_io: true,
            waker: None,
        }
    }
}

impl DownloadOptions {
    /// The defaults, with `HF_ENDPOINT` and `HF_TOKEN` from the environment.
    pub fn from_env() -> DownloadOptions {
        let env = |k| std::env::var(k).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        DownloadOptions {
            endpoint: env("HF_ENDPOINT")
                .map(|e| e.trim_end_matches('/').to_owned())
                .unwrap_or(DEFAULT_ENDPOINT.to_owned()),
            token: env("HF_TOKEN"),
            ..DownloadOptions::default()
        }
    }

    pub fn url(&self, file: &ModelFile) -> String {
        let path: String = file.path.split('/').map(percent_encode).collect::<Vec<_>>().join("/");
        format!("{}/{}/resolve/{}/{}", self.endpoint, file.repo, file.revision, path)
    }
}

fn percent_encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Progress {
    /// The file being downloaded.
    pub file: String,
    /// Bytes of all the model's files that are here.
    pub done: Bytes,
    pub total: Bytes,
    /// Smoothed over the last few seconds.
    pub rate: BytesPerSecond,
    #[serde(serialize_with = "secs")]
    pub eta: Option<Duration>,
}

fn secs<S: serde::Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
    match d {
        Some(d) => s.serialize_some(&d.as_secs()),
        None => s.serialize_none(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DownloadEvent {
    /// Another download holds the store; this one starts when it ends.
    Waiting,
    /// `have` of `total` is already here (resumed or complete files).
    Started {
        have: Bytes,
        total: Bytes,
    },
    /// Re-hashing a partial file before resuming it.
    Rehashing {
        file: String,
        done: Bytes,
        total: Bytes,
    },
    Progress(Progress),
    /// A file's size and sha256 matched; it has its final name.
    Verified {
        file: String,
    },
    Retrying {
        attempt: u32,
        #[serde(serialize_with = "secs_plain")]
        delay: Duration,
        error: String,
    },
    Paused,
    Resumed,
    /// Every file is here and verified.
    Finished {
        id: ModelId,
    },
    Failed {
        error: String,
    },
    /// Stopped by [`DownloadHandle::cancel`]; partial files are kept.
    Cancelled,
}

fn secs_plain<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64())
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("cancelled")]
    Cancelled,
    #[error("not enough disk space: {need} needed, {free} free in {dir}")]
    NoSpace { need: Bytes, free: Bytes, dir: PathBuf },
    #[error("{file} is corrupt: expected sha256 {expected}, got {got}; the partial file was deleted")]
    Corrupt { file: String, expected: Sha256, got: Sha256 },
    #[error("{file}: expected {expected} bytes, the server sent {got}")]
    WrongSize { file: String, expected: Bytes, got: Bytes },
    #[error("{url}: HTTP {status}{hint}")]
    Http { url: String, status: u16, hint: &'static str },
    #[error("network error: {0}")]
    Network(String),
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

impl DownloadError {
    fn retryable(&self) -> bool {
        match self {
            DownloadError::Network(_) => true,
            DownloadError::Http { status, .. } => *status == 408 || *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[derive(Default)]
struct ControlState {
    paused: bool,
    cancelled: bool,
}

#[derive(Default)]
struct Control {
    state: Mutex<ControlState>,
    changed: Condvar,
}

impl Control {
    fn cancelled(&self) -> bool {
        self.state.lock().unwrap().cancelled
    }

    fn paused(&self) -> bool {
        self.state.lock().unwrap().paused
    }

    /// Block while paused; `true` if cancelled.
    fn wait_while_paused(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        while s.paused && !s.cancelled {
            s = self.changed.wait(s).unwrap();
        }
        s.cancelled
    }

    /// Sleep up to `d`, waking early on cancel; `true` if cancelled.
    fn sleep(&self, d: Duration) -> bool {
        let s = self.state.lock().unwrap();
        let (s, _) = self.changed.wait_timeout_while(s, d, |s| !s.cancelled).unwrap();
        s.cancelled
    }

    fn set(&self, f: impl FnOnce(&mut ControlState)) {
        f(&mut self.state.lock().unwrap());
        self.changed.notify_all();
    }
}

/// A download running on its own thread.
pub struct DownloadHandle {
    pub events: Receiver<DownloadEvent>,
    control: Arc<Control>,
    thread: Option<JoinHandle<Result<Installed, DownloadError>>>,
}

impl DownloadHandle {
    pub fn pause(&self) {
        self.control.set(|s| s.paused = true);
    }

    pub fn resume(&self) {
        self.control.set(|s| s.paused = false);
    }

    /// Stop; partial files are kept for a later resume.
    pub fn cancel(&self) {
        self.control.set(|s| s.cancelled = true);
    }

    /// A handle that can cancel from elsewhere (a signal handler's thread).
    pub fn canceller(&self) -> Canceller {
        Canceller(self.control.clone())
    }

    /// Wait for the outcome.
    pub fn wait(mut self) -> Result<Installed, DownloadError> {
        self.thread
            .take()
            .expect("joined once")
            .join()
            .unwrap_or_else(|_| Err(DownloadError::Network("the download thread panicked".into())))
    }
}

#[derive(Clone)]
pub struct Canceller(Arc<Control>);

impl Canceller {
    pub fn cancel(&self) {
        self.0.set(|s| s.cancelled = true);
    }
}

pub struct Download;

impl Download {
    /// Download `entry` into `store` with [`DownloadOptions::from_env`].
    pub fn start(entry: &Entry, store: &Store) -> DownloadHandle {
        Download::start_with(entry, store, DownloadOptions::from_env())
    }

    pub fn start_with(entry: &Entry, store: &Store, opts: DownloadOptions) -> DownloadHandle {
        let (tx, rx) = mpsc::channel();
        let control = Arc::new(Control::default());
        let job = Job { entry: entry.clone(), store: store.clone(), opts, control: control.clone(), tx };
        let thread = std::thread::Builder::new()
            .name(format!("download {}", entry.id))
            .spawn(move || job.run())
            .expect("spawn the download thread");
        DownloadHandle { events: rx, control, thread: Some(thread) }
    }
}

struct Job {
    entry: Entry,
    store: Store,
    opts: DownloadOptions,
    control: Arc<Control>,
    tx: Sender<DownloadEvent>,
}

/// Bytes counted towards progress, across files.
struct Totals {
    done: Bytes,
    total: Bytes,
    rate: RateMeter,
    last_sent: Instant,
}

impl Job {
    fn send(&self, e: DownloadEvent) {
        let _ = self.tx.send(e);
        if let Some(w) = &self.opts.waker {
            w();
        }
    }

    fn run(self) -> Result<Installed, DownloadError> {
        if self.opts.idle_io {
            set_idle_io_priority();
        }
        let result = self.run_inner();
        match &result {
            Ok(i) => self.send(DownloadEvent::Finished { id: i.id().clone() }),
            Err(DownloadError::Cancelled) => self.send(DownloadEvent::Cancelled),
            Err(e) => self.send(DownloadEvent::Failed { error: e.to_string() }),
        }
        result
    }

    fn run_inner(&self) -> Result<Installed, DownloadError> {
        let io_err = |path: &Path| {
            let path = path.to_owned();
            move |source| DownloadError::Io { path, source }
        };
        let dir = self.store.model_dir(&self.entry.id);
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        let _lock = self.lock()?;

        let mut have = Bytes::ZERO;
        let mut need = Bytes::ZERO;
        for f in &self.entry.files {
            if self.store.has_file(&self.entry.id, f) {
                have += f.size;
            } else {
                let partial = fs::metadata(self.store.partial_path(&self.entry.id, f)).map(|m| m.len()).unwrap_or(0);
                let partial = Bytes(partial.min(f.size.0));
                have += partial;
                need += f.size - partial;
            }
        }
        let total = self.entry.download_size();
        if need > Bytes::ZERO {
            let free = crate::store::free_space(&dir).map_err(io_err(&dir))?;
            if free < need + SPACE_MARGIN {
                return Err(DownloadError::NoSpace { need: need + SPACE_MARGIN, free, dir });
            }
        }
        self.send(DownloadEvent::Started { have, total });
        let mut totals = Totals { done: have, total, rate: RateMeter::new(), last_sent: Instant::now() };
        for f in &self.entry.files {
            if !self.store.has_file(&self.entry.id, f) {
                self.file(f, &mut totals)?;
            }
        }
        self.store.installed(&self.entry).ok_or_else(|| DownloadError::Io {
            path: dir.clone(),
            source: io::Error::other("a verified file disappeared from the store"),
        })
    }

    /// The store's download lock; waits (cancellably) while another holds it.
    fn lock(&self) -> Result<File, DownloadError> {
        let path = self.store.root().join(".download.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| DownloadError::Io { path: path.clone(), source })?;
        use std::os::fd::AsRawFd;
        let mut waiting = false;
        loop {
            // SAFETY: a valid open descriptor.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(file);
            }
            if !waiting {
                waiting = true;
                self.send(DownloadEvent::Waiting);
            }
            if self.control.sleep(Duration::from_millis(250)) {
                return Err(DownloadError::Cancelled);
            }
        }
    }

    /// Download one file, retrying network errors.
    fn file(&self, f: &ModelFile, totals: &mut Totals) -> Result<(), DownloadError> {
        let final_path = self.store.file_path(&self.entry.id, f);
        let partial_path = self.store.partial_path(&self.entry.id, f);
        let io_err = |source| DownloadError::Io { path: partial_path.clone(), source };
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&partial_path)
            .map_err(io_err)?;
        let mut len = file.metadata().map_err(io_err)?.len();
        if len > f.size.0 {
            // Longer than the file can be: start again.
            totals.done = totals.done - Bytes(len.min(f.size.0));
            file.set_len(0).map_err(io_err)?;
            len = 0;
        }
        let mut hasher = self.rehash(&mut file, f, Bytes(len))?;
        let mut attempt = 0;
        while len < f.size.0 {
            if self.control.paused() {
                self.send(DownloadEvent::Paused);
                if self.control.wait_while_paused() {
                    return Err(DownloadError::Cancelled);
                }
                self.send(DownloadEvent::Resumed);
            }
            if self.control.cancelled() {
                return Err(DownloadError::Cancelled);
            }
            let before = len;
            match self.fetch(f, &mut file, &mut len, &mut hasher, totals) {
                Ok(()) => {}
                Err(e) if e.retryable() => {
                    if len > before {
                        attempt = 0; // progress was made: a fresh budget
                    }
                    attempt += 1;
                    if attempt >= self.opts.max_attempts {
                        return Err(e);
                    }
                    let delay = self.opts.backoff * 2u32.saturating_pow(attempt - 1);
                    self.send(DownloadEvent::Retrying { attempt, delay, error: e.to_string() });
                    if self.control.sleep(delay) {
                        return Err(DownloadError::Cancelled);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        file.flush().map_err(io_err)?;
        let got = Sha256::from_digest(&hasher.finalize());
        if got != f.sha256 {
            drop(file);
            let _ = fs::remove_file(&partial_path);
            totals.done = totals.done - f.size;
            return Err(DownloadError::Corrupt { file: f.file_name().to_owned(), expected: f.sha256.clone(), got });
        }
        file.sync_all().map_err(io_err)?;
        drop(file);
        fs::rename(&partial_path, &final_path)
            .map_err(|source| DownloadError::Io { path: final_path.clone(), source })?;
        if let Some(dir) = final_path.parent()
            && let Ok(d) = File::open(dir)
        {
            let _ = d.sync_all();
        }
        self.send(DownloadEvent::Verified { file: f.file_name().to_owned() });
        Ok(())
    }

    /// Hash the `len` bytes already in a partial file.
    fn rehash(&self, file: &mut File, f: &ModelFile, len: Bytes) -> Result<Hasher, DownloadError> {
        let mut hasher = Hasher::new();
        if len == Bytes::ZERO {
            return Ok(hasher);
        }
        let io_err = |source| DownloadError::Io { path: self.store.partial_path(&self.entry.id, f), source };
        file.seek(SeekFrom::Start(0)).map_err(io_err)?;
        let mut buf = vec![0u8; READ_BUFFER];
        let mut done = 0u64;
        let mut last = Instant::now();
        let name = f.file_name().to_owned();
        while done < len.0 {
            if self.control.cancelled() {
                return Err(DownloadError::Cancelled);
            }
            let want = buf.len().min((len.0 - done) as usize);
            let n = file.read(&mut buf[..want]).map_err(io_err)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            done += n as u64;
            if last.elapsed() >= PROGRESS_EVERY {
                last = Instant::now();
                self.send(DownloadEvent::Rehashing { file: name.clone(), done: Bytes(done), total: len });
            }
        }
        self.send(DownloadEvent::Rehashing { file: name, done: Bytes(done), total: len });
        file.seek(SeekFrom::Start(done)).map_err(io_err)?;
        file.set_len(done).map_err(io_err)?;
        Ok(hasher)
    }

    /// One request from `len` onwards, appending to `file`. Returns when the
    /// file is complete, on a pause or cancel, or with the error.
    fn fetch(
        &self,
        f: &ModelFile,
        file: &mut File,
        len: &mut u64,
        hasher: &mut Hasher,
        totals: &mut Totals,
    ) -> Result<(), DownloadError> {
        let url = self.opts.url(f);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_per_read(Some(self.opts.read_timeout))
            .user_agent(concat!("tasuke/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let mut req = agent.get(&url);
        if *len > 0 {
            req = req.header("Range", format!("bytes={}-", *len));
        }
        if let Some(token) = &self.opts.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        let mut resp = req.call().map_err(|e| DownloadError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let io_err = |source| DownloadError::Io { path: self.store.partial_path(&self.entry.id, f), source };
        match status {
            206 => {
                let start = resp
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes "))
                    .and_then(|v| v.split('-').next())
                    .and_then(|v| v.parse::<u64>().ok());
                if start != Some(*len) {
                    return Err(DownloadError::Network(format!("the server resumed at {start:?}, not {}", *len)));
                }
            }
            200 if *len > 0 => {
                // The server ignored the range: start the file again.
                totals.done = totals.done - Bytes(*len);
                file.set_len(0).map_err(io_err)?;
                file.seek(SeekFrom::Start(0)).map_err(io_err)?;
                *len = 0;
                *hasher = Hasher::new();
            }
            200 => {}
            416 => {
                return Err(DownloadError::WrongSize {
                    file: f.file_name().to_owned(),
                    expected: f.size,
                    got: Bytes(*len),
                });
            }
            _ => {
                let hint = match status {
                    401 | 403 => ": this repository may need a Hugging Face token (HF_TOKEN) or accepting its licence",
                    404 => ": the file isn't at this revision",
                    _ => "",
                };
                return Err(DownloadError::Http { url, status, hint });
            }
        }
        let mut body = resp.body_mut().as_reader();
        let mut buf = vec![0u8; READ_BUFFER];
        let name = f.file_name().to_owned();
        loop {
            if self.control.cancelled() {
                return Err(DownloadError::Cancelled);
            }
            if self.control.paused() {
                return Ok(()); // the caller waits, then resumes with a new range
            }
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(DownloadError::Network(e.to_string())),
            };
            if *len + n as u64 > f.size.0 {
                return Err(DownloadError::WrongSize { file: name, expected: f.size, got: Bytes(*len + n as u64) });
            }
            file.write_all(&buf[..n]).map_err(io_err)?;
            hasher.update(&buf[..n]);
            *len += n as u64;
            totals.done += Bytes(n as u64);
            totals.rate.add(n as u64);
            if totals.last_sent.elapsed() >= PROGRESS_EVERY || *len == f.size.0 {
                totals.last_sent = Instant::now();
                let rate = totals.rate.rate();
                self.send(DownloadEvent::Progress(Progress {
                    file: name.clone(),
                    done: totals.done,
                    total: totals.total,
                    rate,
                    eta: rate.time_for(totals.total - totals.done),
                }));
            }
        }
        if *len < f.size.0 {
            return Err(DownloadError::Network(format!("the connection ended at {} of {}", Bytes(*len), f.size)));
        }
        Ok(())
    }
}

/// Bytes per second over a sliding window of about five seconds.
struct RateMeter {
    samples: std::collections::VecDeque<(Instant, u64)>,
}

impl RateMeter {
    const WINDOW: Duration = Duration::from_secs(5);

    fn new() -> RateMeter {
        RateMeter { samples: std::collections::VecDeque::new() }
    }

    fn add(&mut self, n: u64) {
        let now = Instant::now();
        self.samples.push_back((now, n));
        while self.samples.front().is_some_and(|&(t, _)| now.duration_since(t) > Self::WINDOW) {
            self.samples.pop_front();
        }
    }

    fn rate(&self) -> BytesPerSecond {
        let (Some(&(first, _)), Some(&(last, _))) = (self.samples.front(), self.samples.back()) else {
            return BytesPerSecond(0.0);
        };
        let span = last.duration_since(first).as_secs_f64();
        if span < 0.2 {
            return BytesPerSecond(0.0);
        }
        // The first sample's bytes arrived before the span began.
        let bytes: u64 = self.samples.iter().skip(1).map(|&(_, n)| n).sum();
        BytesPerSecond(bytes as f64 / span)
    }
}

/// `ioprio_set(IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_IDLE)`: with `who` 0 it
/// applies to the calling thread only.
pub fn set_idle_io_priority() -> bool {
    const IOPRIO_WHO_PROCESS: libc::c_long = 1;
    const IOPRIO_CLASS_IDLE: libc::c_long = 3;
    const IOPRIO_CLASS_SHIFT: libc::c_long = 13;
    // SAFETY: a plain syscall with integer arguments.
    unsafe { libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT) == 0 }
}

/// The calling thread's IO priority class (for tests): 3 is idle.
pub fn io_priority_class() -> Option<i64> {
    const IOPRIO_WHO_PROCESS: libc::c_long = 1;
    // SAFETY: as above.
    let r = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, 0) };
    (r >= 0).then_some(r >> 13)
}
