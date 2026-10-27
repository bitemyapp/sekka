use crate::config::Audio;
use anyhow::{Context, Result, bail, ensure};
use std::{io::Cursor, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    sync::{mpsc, oneshot},
};

pub const RATE: usize = 16_000;
#[derive(Debug)]
pub struct Meter {
    pub samples: usize,
    pub level: f32,
}

/// Retains a split sample across pipe reads; PipeWire's `s16` is native-endian.
#[derive(Default)]
pub struct PcmDecoder {
    pending: Option<u8>,
}
impl PcmDecoder {
    pub fn decode(&mut self, bytes: &[u8]) -> Vec<f32> {
        let mut samples = Vec::with_capacity(bytes.len() / 2 + 1);
        let mut bytes = bytes;
        if let Some(first) = self.pending.take() {
            if let Some((&second, rest)) = bytes.split_first() {
                samples.push(i16::from_ne_bytes([first, second]) as f32 / 32768.0);
                bytes = rest;
            } else {
                self.pending = Some(first);
                return samples;
            }
        }
        let (chunks, remainder) = bytes.as_chunks::<2>();
        samples.extend(chunks.iter().map(|b| i16::from_ne_bytes(*b) as f32 / 32768.0));
        self.pending = remainder.first().copied();
        samples
    }
}
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt() as f32
}

/// Require 100 ms of audible input, measured in 20 ms windows. Averaging
/// the entire recording lets quiet lead-in/trailing pauses erase real speech.
/// This is an amplitude gate, not a speech classifier.
pub fn has_signal(samples: &[f32], threshold: f32) -> bool {
    samples.chunks(RATE / 50).filter(|chunk| rms(chunk) >= threshold).map(<[f32]>::len).sum::<usize>() >= RATE / 10
}

pub struct Capture {
    pub stop: oneshot::Sender<()>,
    pub task: tokio::task::JoinHandle<Result<Vec<f32>>>,
}
impl Capture {
    pub fn start(config: Audio, remaining: usize, meter: mpsc::Sender<Meter>) -> Result<Self> {
        let mut command = Command::new("pw-record");
        command
            .args(["--raw", "--rate=16000", "--channels=1", "--format=s16", "--media-role=Communication"])
            .arg("--properties=application.name=Koe node.description=Koe")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(target) = &config.target {
            command.arg(format!("--target={target}"));
        }
        let mut child = command.arg("-").spawn().context("cannot start pw-record; install PipeWire's tools")?;
        let mut stdout = child.stdout.take().context("pw-record has no stdout")?;
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut samples = Vec::new();
            let mut decoder = PcmDecoder::default();
            let mut buffer = [0_u8; 3200];
            let mut quiet_samples = 0;
            let mut heard_speech = false;
            let mut meter_tick = tokio::time::interval(Duration::from_millis(100));
            let mut current_level = 0.0;
            let mut last_audio = tokio::time::Instant::now();
            let mut watchdog = tokio::time::interval(Duration::from_secs(1));
            let result = loop {
                tokio::select! {
                    _ = &mut stopped => break Ok(samples),
                    _ = meter_tick.tick() => {
                        let level = if last_audio.elapsed() < Duration::from_millis(200) { current_level } else { 0.0 };
                        let _ = meter.try_send(Meter { samples: samples.len(), level });
                    }
                    _ = watchdog.tick() => {
                        if last_audio.elapsed() > Duration::from_secs(10) { break Err(anyhow::anyhow!("microphone produced no audio for 10 seconds; check PipeWire and the selected source")); }
                    }
                    n = stdout.read(&mut buffer) => {
                        let n = match n { Ok(n) => n, Err(e) => break Err(e.into()) };
                        if n == 0 { break Err(anyhow::anyhow!("pw-record stopped unexpectedly; check PipeWire and audio.target")); }
                        last_audio = tokio::time::Instant::now();
                        let chunk = decoder.decode(&buffer[..n]);
                        let level = rms(&chunk);
                        current_level = level;
                        if level >= config.silence_threshold { heard_speech = true; quiet_samples = 0; }
                        else { quiet_samples += chunk.len(); }
                        let count = chunk.len().min(remaining.saturating_sub(samples.len()));
                        samples.extend_from_slice(&chunk[..count]);
                        if samples.len() >= remaining || (heard_speech && config.silence_seconds > 0.0 && quiet_samples as f32 >= config.silence_seconds * RATE as f32) { break Ok(samples); }
                    }
                }
            };
            // Reap the capture process on every exit, including EOF, error and cancellation.
            let _ = child.kill().await;
            let _ = child.wait().await;
            result
        });
        Ok(Self { stop, task })
    }
    pub async fn finish(self) -> Result<Vec<f32>> {
        let _ = self.stop.send(());
        self.task.await.context("capture task failed")?
    }
}

pub fn read_wav(path: &Path, max_seconds: u64) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).context("cannot read WAV file")?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 1 && spec.sample_rate == RATE as u32,
        "input must be mono 16 kHz WAV; convert with ffmpeg -i INPUT -ar 16000 -ac 1 OUTPUT.wav"
    );
    ensure!(u64::from(reader.duration()) <= max_seconds * RATE as u64, "WAV exceeds audio.max_seconds");
    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.0))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (hound::SampleFormat::Float, 32) => reader.samples::<f32>().collect::<std::result::Result<Vec<_>, _>>()?,
        _ => bail!("input must contain signed 16-bit PCM or 32-bit float samples"),
    };
    ensure!(
        !samples.is_empty() && samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
        "WAV contains empty, non-finite or out-of-range audio"
    );
    Ok(samples)
}
pub fn wav_bytes(samples: &[f32]) -> Result<Vec<u8>> {
    let mut output = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut output,
        hound::WavSpec {
            channels: 1,
            sample_rate: RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in samples {
        writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16)?;
    }
    writer.finalize()?;
    Ok(output.into_inner())
}

/// Play only a user-requested teaching preview; no audio file is created.
pub async fn play(samples: &[f32]) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut child = Command::new("pw-play")
        .args(["--raw", "--rate=16000", "--channels=1", "--format=s16", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("cannot start pw-play")?;
    let bytes: Vec<u8> =
        samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_ne_bytes()).collect();
    tokio::time::timeout(Duration::from_secs(35), async {
        let mut stdin = child.stdin.take().context("pw-play has no stdin")?;
        stdin.write_all(&bytes).await?;
        drop(stdin);
        ensure!(child.wait().await?.success(), "audio playback failed");
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("audio playback timed out")?
}
