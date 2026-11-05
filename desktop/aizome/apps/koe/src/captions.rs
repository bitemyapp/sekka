//! Local, memory-only captions. File decoding is backpressured; a live
//! monitor instead evicts old chunks so inference cannot stall audio capture.
use crate::{
    audio,
    config::Config,
    engine::{CaptionSegment, Recognizer},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Write},
    os::fd::{AsRawFd, FromRawFd},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, unix::AsyncFd},
    process::{Child, Command},
    sync::{Mutex, Notify},
};

const CHUNK_SAMPLES: usize = audio::RATE * 6;
// Local media can be decoded ahead, so use Whisper's complete context rather
// than splitting words every six seconds as a low-latency live source must.
const FILE_CHUNK_SAMPLES: usize = audio::RATE * 30;
const QUEUE_CHUNKS: usize = 2;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub input: Option<PathBuf>,
    pub start: f64,
    pub audio_track: usize,
    pub diarize: bool,
}
impl Options {
    fn validate(&self) -> Result<()> {
        ensure!(self.start.is_finite() && self.start >= 0.0, "--start must be finite and nonnegative");
        ensure!(
            self.input.is_some() || (self.start == 0.0 && self.audio_track == 0),
            "--start and --audio-track require --input"
        );
        if let Some(path) = &self.input {
            ensure!(path.is_file(), "caption input is not a local file: {}", path.display());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Ready,
    Cue {
        start: f64,
        end: f64,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        speaker: Option<String>,
    },
    Gap {
        message: String,
    },
    Error {
        message: String,
    },
}

/// Nonblocking pipe writes let cancellation interrupt a slow/paused consumer.
/// Regular-file redirection does not support epoll and uses bounded writes.
enum Output {
    Pipe(AsyncFd<File>),
    File(File),
}
impl Output {
    fn stdout() -> Result<Self> {
        // SAFETY: dup returns a new owned descriptor; it is wrapped once.
        let fd = unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
        ensure!(fd >= 0, "cannot duplicate caption stdout: {}", io::Error::last_os_error());
        let file = unsafe { File::from_raw_fd(fd) };
        if file.metadata()?.is_file() {
            return Ok(Self::File(file));
        }
        // stdout belongs exclusively to the NDJSON caption session.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        ensure!(
            flags >= 0 && unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
            "cannot make caption stdout nonblocking"
        );
        // SAFETY: this File exclusively owns the duplicated descriptor and
        // stays inside AsyncFd until registration is dropped or rejected.
        match unsafe { AsyncFd::register(file) } {
            Ok(file) => Ok(Self::Pipe(file)),
            Err(error) => {
                let (file, error) = error.into_parts();
                if error.raw_os_error() == Some(libc::EPERM) {
                    // /dev/null and other non-pollable file redirections.
                    Ok(Self::File(file))
                } else {
                    Err(error.into())
                }
            }
        }
    }
    async fn emit(&mut self, event: Event) -> Result<()> {
        let mut bytes = serde_json::to_vec(&event)?;
        bytes.push(b'\n');
        match self {
            Self::File(file) => file.write_all(&bytes)?,
            Self::Pipe(file) => {
                let mut remaining = bytes.as_slice();
                while !remaining.is_empty() {
                    let mut ready = file.writable().await?;
                    match ready.try_io(|fd| (&mut fd.get_ref()).write(remaining)) {
                        Ok(Ok(0)) => return Err(io::Error::from(io::ErrorKind::WriteZero).into()),
                        Ok(Ok(n)) => remaining = &remaining[n..],
                        Ok(Err(error)) => return Err(error.into()),
                        Err(_) => {}
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Cancellation {
    flag: Arc<AtomicBool>,
    changed: Notify,
}
impl Cancellation {
    fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
        self.changed.notify_waiters();
    }
    async fn cancelled(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.flag.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }
}
// Also abort native inference if the caller drops the run future.
struct CancelOnDrop(Arc<Cancellation>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[derive(Debug)]
struct Chunk {
    start_sample: u64,
    samples: Vec<f32>,
}
#[derive(Default)]
struct QueueState {
    chunks: VecDeque<Chunk>,
    done: Option<Result<()>>,
}
#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    ready: Notify,
    space: Notify,
}
impl Queue {
    async fn push(&self, chunk: Chunk, live: bool) {
        loop {
            let space = self.space.notified();
            {
                let mut state = self.state.lock().await;
                if state.chunks.len() < QUEUE_CHUNKS || live {
                    if state.chunks.len() == QUEUE_CHUNKS {
                        state.chunks.pop_front();
                    }
                    state.chunks.push_back(chunk);
                    self.ready.notify_one();
                    return;
                }
            }
            space.await;
        }
    }
    async fn finish(&self, result: Result<()>) {
        self.state.lock().await.done = Some(result);
        self.ready.notify_one();
    }
    async fn next(&self) -> Result<Option<Chunk>> {
        loop {
            let ready = self.ready.notified();
            {
                let mut state = self.state.lock().await;
                if let Some(chunk) = state.chunks.pop_front() {
                    self.space.notify_one();
                    return Ok(Some(chunk));
                }
                if let Some(done) = state.done.take() {
                    done?;
                    return Ok(None);
                }
            }
            ready.await;
        }
    }
}

fn source_command(options: &Options) -> Command {
    if let Some(input) = &options.input {
        let mut command = Command::new("ffmpeg");
        command
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-protocol_whitelist", "file,pipe", "-ss"])
            .arg(options.start.to_string())
            .arg("-i")
            .arg(input)
            .arg("-map")
            .arg(format!("0:a:{}", options.audio_track))
            .args([
                "-vn",
                "-sn",
                "-dn",
                "-af",
                "aresample=16000:async=1:first_pts=0",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-f",
                "s16le",
                "pipe:1",
            ]);
        command
    } else {
        let mut command = Command::new("pw-record");
        // No explicit target: WirePlumber chooses its default Audio/Sink
        // because capture.sink is true. PulseAudio's @DEFAULT_AUDIO_SINK@
        // is not a pw-record node name and would prevent linking.
        command.args(["--raw", "--rate=16000", "--channels=1", "--format=s16",
            "--properties=application.name=Koe-Captions node.description=Koe-Captions stream.capture.sink=true node.autoconnect=true node.dont-fallback=true node.dont-move=true",
            "-"]);
        command
    }
}

fn spawn_source(options: &Options) -> Result<Child> {
    let mut command = source_command(options);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    // A window may SIGKILL the CLI. Linux must then stop its capture/decoder
    // too, even though Rust destructors and async cleanup cannot run.
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "caption parent exited"));
            }
            Ok(())
        });
    }
    command.spawn().with_context(|| {
        if options.input.is_some() {
            "cannot start ffmpeg; install it to caption local media"
        } else {
            "cannot start pw-record; install PipeWire tools to caption system output"
        }
    })
}

async fn capture(mut child: Child, live: bool, queue: Arc<Queue>, cancel: Arc<Cancellation>) {
    let result = async {
        let mut stdout = child.stdout.take().context("caption audio source has no stdout")?;
        let window = if live { CHUNK_SAMPLES } else { FILE_CHUNK_SAMPLES };
        let mut bytes = vec![0_u8; window * 2];
        let mut filled = 0;
        let mut start_sample = 0;
        loop {
            let read = stdout.read(&mut bytes[filled..]);
            let n = if live {
                tokio::time::timeout(Duration::from_secs(10), read).await.context(
                    "system-output monitor produced no audio for 10 seconds; check the default output and PipeWire",
                )??
            } else {
                read.await?
            };
            filled += n;
            if filled == bytes.len() || n == 0 {
                ensure!(filled % 2 == 0, "audio decoder ended with an incomplete PCM sample");
                if filled > 0 {
                    let samples: Vec<f32> = bytes[..filled]
                        .chunks_exact(2)
                        .map(|b| {
                            let sample =
                                if live { i16::from_ne_bytes([b[0], b[1]]) } else { i16::from_le_bytes([b[0], b[1]]) };
                            sample as f32 / 32768.0
                        })
                        .collect();
                    let count = samples.len() as u64;
                    queue.push(Chunk { start_sample, samples }, live).await;
                    start_sample += count;
                    filled = 0;
                }
            }
            if n == 0 {
                let status = child.wait().await?;
                ensure!(
                    !live,
                    "system-output monitor stopped; check the default output and PipeWire (microphones are never used)"
                );
                ensure!(
                    status.success(),
                    "ffmpeg could not decode the selected audio track; check the input and --audio-track"
                );
                break;
            }
        }
        Ok(())
    };
    let result = tokio::select! {
        _ = cancel.cancelled() => Ok(()),
        result = result => result,
    };
    // Explicitly reap on EOF, read failure, queue cancellation and signal.
    let _ = child.kill().await;
    let _ = child.wait().await;
    queue.finish(result).await;
}

fn cue(segment: CaptionSegment, offset: f64, duration: f64, speaker: Option<String>) -> Option<Event> {
    if !segment.start.is_finite() || !segment.end.is_finite() {
        return None;
    }
    let start = segment.start.clamp(0.0, duration);
    let end = segment.end.clamp(start, duration);
    let text = segment.text.trim();
    if text.is_empty() || end <= start {
        return None;
    }
    Some(Event::Cue { start: offset + start, end: offset + end, text: text.to_owned(), speaker })
}

async fn consume(
    queue: &Queue,
    engine: &Recognizer,
    options: &Options,
    threshold: f32,
    cancel: &Cancellation,
    output: &mut Output,
) -> Result<()> {
    let mut expected_sample = 0;
    while let Some(mut chunk) = queue.next().await? {
        if chunk.start_sample > expected_sample {
            output
                .emit(Event::Gap {
                    message: format!(
                        "Caption inference fell behind; skipped {:.2} seconds of system output ({:.2}–{:.2}s)",
                        (chunk.start_sample - expected_sample) as f64 / audio::RATE as f64,
                        options.start + expected_sample as f64 / audio::RATE as f64,
                        options.start + chunk.start_sample as f64 / audio::RATE as f64,
                    ),
                })
                .await?;
        }
        expected_sample = chunk.start_sample + chunk.samples.len() as u64;
        if !audio::has_signal(&chunk.samples, threshold) {
            continue;
        }
        let duration = chunk.samples.len() as f64 / audio::RATE as f64;
        let offset = options.start + chunk.start_sample as f64 / audio::RATE as f64;
        // Whisper requires enough input even when EOF leaves a short tail.
        if chunk.samples.len() < audio::RATE {
            chunk.samples.resize(audio::RATE, 0.0);
        }
        let segments = engine.captions(chunk.samples, options.diarize, cancel.flag.clone()).await?;
        let mut turn = 1;
        for segment in segments {
            let next_turn = segment.turn_next;
            let window = if options.input.is_some() { FILE_CHUNK_SAMPLES } else { CHUNK_SAMPLES };
            let speaker =
                options.diarize.then(|| format!("Chunk {}, turn {turn}", chunk.start_sample / window as u64 + 1));
            if let Some(event) = cue(segment, offset, duration, speaker) {
                output.emit(event).await?;
            }
            if next_turn {
                turn += 1;
            }
        }
    }
    Ok(())
}

async fn session(config: Config, options: Options, cancel: Arc<Cancellation>, output: &mut Output) -> Result<()> {
    options.validate()?;
    let engine = Recognizer::new(config.engine);
    if options.input.is_some() {
        // File decoding applies backpressure, so no discarded inference is needed.
        engine.prepare_captions(options.diarize, cancel.flag.clone()).await?;
    } else {
        // Warm live inference before capture: cold GPU setup can otherwise
        // overrun the bounded queue and evict the opening audio.
        engine.captions(vec![0.0; audio::RATE], options.diarize, cancel.flag.clone()).await?;
    }
    let child = spawn_source(&options)?;
    let queue = Arc::new(Queue::default());
    let capture = tokio::spawn(capture(child, options.input.is_none(), queue.clone(), cancel.clone()));
    let result = tokio::select! {
        _ = cancel.cancelled() => Ok(()),
        result = async {
            output.emit(Event::Ready).await?;
            consume(&queue, &engine, &options, config.audio.silence_threshold, &cancel, output).await
        } => result,
    };
    cancel.cancel();
    capture.await.context("caption capture worker failed")?;
    result
}

/// Stream NDJSON until EOF, consumer disconnect, SIGINT or SIGTERM. No
/// transcripts/audio are stored, and nothing is sent to an HTTP backend.
pub async fn run(config: Config, options: Options) -> Result<()> {
    let cancel = Arc::new(Cancellation::default());
    let _guard = CancelOnDrop(cancel.clone());
    let mut output = Output::stdout()?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let result = {
        let session = session(config, options, cancel.clone(), &mut output);
        tokio::pin!(session);
        tokio::select! {
            result = &mut session => result,
            _ = terminate.recv() => { cancel.cancel(); session.await },
            _ = interrupt.recv() => { cancel.cancel(); session.await },
        }
    };
    if let Err(error) = &result {
        // A disconnected or stalled consumer must not prevent child cleanup.
        let _ =
            tokio::time::timeout(Duration::from_secs(1), output.emit(Event::Error { message: format!("{error:#}") }))
                .await;
    }
    result
}

#[cfg(test)]
#[path = "captions_tests.rs"]
mod tests;
