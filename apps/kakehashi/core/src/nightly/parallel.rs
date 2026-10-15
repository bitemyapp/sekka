//! Bounded, adaptive range downloads. Only complete, contiguous ranges are
//! appended to the .part file; interruption never leaves holes to resume over.

use super::{Build, Fetch, agent};
use crate::image::Cancelled;
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const CHUNK: u64 = 16 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 32;
const SAMPLE: Duration = Duration::from_secs(3);
const SETTLE: Duration = Duration::from_secs(1);

/// Try enough connections to get past startup noise, then require a 10% gain
/// over the best measured rate. Two flat samples confirm diminishing returns.
struct Adaptive {
    limit: usize,
    best_limit: usize,
    best_rate: f64,
    flat: usize,
    stopped: bool,
    since: Instant,
    bytes: u64,
    settling: bool,
}

impl Adaptive {
    fn new(now: Instant, bytes: u64) -> Self {
        Self { limit: 2, best_limit: 2, best_rate: 0., flat: 0, stopped: false, since: now, bytes, settling: true }
    }

    fn observe(&mut self, now: Instant, bytes: u64) {
        if self.stopped {
            return;
        }
        let elapsed = now.duration_since(self.since);
        if self.settling {
            if elapsed >= SETTLE {
                self.since = now;
                self.bytes = bytes;
                self.settling = false;
            }
            return;
        }
        if elapsed < SAMPLE {
            return;
        }
        let rate = bytes.saturating_sub(self.bytes) as f64 / elapsed.as_secs_f64();
        self.since = now;
        self.bytes = bytes;
        if rate > self.best_rate * 1.10 {
            self.best_rate = rate;
            self.best_limit = self.limit;
            self.flat = 0;
        } else {
            self.flat += 1;
            if self.flat < 2 {
                return;
            }
            if self.limit >= 8 {
                self.limit = self.best_limit;
                self.stopped = true;
                return;
            }
        }
        if self.limit == MAX_CONNECTIONS {
            self.stopped = true;
        } else {
            self.limit = (self.limit * 2).min(MAX_CONNECTIONS);
            self.flat = 0;
            self.settling = true;
        }
    }
}

type Response = ureq::http::Response<ureq::Body>;

fn request(agent: &ureq::Agent, url: &str, start: u64, len: u64) -> Result<Response> {
    Ok(agent
        .get(url)
        .header("Range", &format!("bytes={start}-{}", start + len - 1))
        .header("Accept-Encoding", "identity")
        .header("Cache-Control", "no-cache")
        .config()
        // Bound shutdown even if a server stops sending bytes. This applies to
        // a small range, not the complete ISO or the sequential fallback.
        .timeout_per_call(Some(Duration::from_secs(30)))
        .build()
        .call()?)
}

fn validate(response: &Response, start: u64, len: u64, total: u64) -> Result<()> {
    ensure!(response.status().as_u16() == 206, "the server stopped honoring download ranges; try again to resume");
    let expected = format!("bytes {start}-{}/{total}", start + len - 1);
    ensure!(
        response.headers().get("content-range").and_then(|v| v.to_str().ok()) == Some(expected.as_str()),
        "the server returned the wrong download range"
    );
    if let Some(length) = response.headers().get("content-length") {
        ensure!(length.to_str()?.parse::<u64>()? == len, "the server returned the wrong range length");
    }
    if let Some(encoding) = response.headers().get("content-encoding") {
        ensure!(encoding == "identity", "the server encoded a download range");
    }
    Ok(())
}

/// A server that ignores Range supplies a full response for the existing
/// sequential path. Never interpret that response as a piece of the ISO.
pub(super) fn probe(url: &str, have: u64, total: u64) -> Result<Option<Response>> {
    let mut response = request(&agent(), url, have, 1)?;
    if response.status().as_u16() == 200 {
        // Do not retain the probe's 30-second deadline for a full ISO download.
        drop(response);
        return Ok(Some(agent().get(url).header("Accept-Encoding", "identity").call()?));
    }
    validate(&response, have, 1, total)?;
    let mut reader = response.body_mut().as_reader();
    reader.read_exact(&mut [0])?;
    ensure!(reader.read(&mut [0])? == 0, "invalid range probe length");
    Ok(None)
}

struct Pending {
    received: Arc<AtomicU64>,
    data: Option<Vec<u8>>,
}

fn range(
    agent: &ureq::Agent,
    url: &str,
    start: u64,
    len: u64,
    total: u64,
    received: &AtomicU64,
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    for attempt in 0..3 {
        if stop.load(Relaxed) {
            bail!(Cancelled);
        }
        match range_once(agent, url, start, len, total, received, stop) {
            Ok(data) => return Ok(data),
            Err(e) if e.is::<Cancelled>() || attempt == 2 => return Err(e),
            Err(_) => {
                // Retry only this uncommitted range. Poll during backoff so Stop
                // does not start another connection after a transient failure.
                for _ in 0..(5 << attempt) {
                    if stop.load(Relaxed) {
                        bail!(Cancelled);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
    unreachable!("the last attempt returns its error")
}

fn range_once(
    agent: &ureq::Agent,
    url: &str,
    start: u64,
    len: u64,
    total: u64,
    received: &AtomicU64,
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    if stop.load(Relaxed) {
        bail!(Cancelled);
    }
    let mut response = request(agent, url, start, len)?;
    validate(&response, start, len, total)?;
    let mut body = response.body_mut().as_reader();
    let mut data = vec![0; len as usize];
    let mut done = 0;
    while done < data.len() {
        if stop.load(Relaxed) {
            bail!(Cancelled);
        }
        let end = (done + 256 * 1024).min(data.len());
        let n = body.read(&mut data[done..end])?;
        ensure!(n > 0, "a download range ended early; try again to resume");
        done += n;
        received.fetch_max(done as u64, Relaxed);
    }
    ensure!(body.read(&mut [0])? == 0, "a download range was longer than requested");
    Ok(data)
}

pub(super) fn transfer(
    build: &Build,
    url: &str,
    have: u64,
    out: &mut File,
    hasher: &mut Sha256,
    report: &mut dyn FnMut(Fetch),
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    transfer_with_chunk(build, url, have, out, hasher, report, cancelled, CHUNK)
}

fn transfer_with_chunk(
    build: &Build,
    url: &str,
    have: u64,
    out: &mut File,
    hasher: &mut Sha256,
    report: &mut dyn FnMut(Fetch),
    cancelled: &dyn Fn() -> bool,
    chunk: u64,
) -> Result<()> {
    let client = agent();
    let stop = AtomicBool::new(false);
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let result = (|| {
            let mut controller = Adaptive::new(Instant::now(), have);
            let trace = std::env::var_os("KAKEHASHI_DOWNLOAD_TRACE").is_some();
            let mut pending = BTreeMap::<u64, Pending>::new();
            let (mut committed, mut next) = (have, have);
            while committed < build.bytes {
                if cancelled() {
                    bail!(Cancelled);
                }
                let received = committed + pending.values().map(|p| p.received.load(Relaxed)).sum::<u64>();
                report(Fetch::Downloading { done: received, total: build.bytes });
                // Don't let the final, draining batch look like a throughput loss.
                if next < build.bytes {
                    let previous = controller.limit;
                    controller.observe(Instant::now(), received);
                    if trace && controller.limit != previous {
                        eprintln!(
                            "download connections: {previous} -> {}, best {:.1} MiB/s{}",
                            controller.limit,
                            controller.best_rate / 1048576.,
                            if controller.stopped { " (settled)" } else { "" }
                        );
                    }
                }
                while pending.len() < controller.limit && next < build.bytes {
                    let start = next;
                    let len = chunk.min(build.bytes - start);
                    let received = Arc::new(AtomicU64::new(0));
                    pending.insert(start, Pending { received: received.clone(), data: None });
                    let tx = tx.clone();
                    let (client, stop) = (&client, &stop);
                    std::thread::Builder::new().name("image-download".into()).spawn_scoped(scope, move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            range(client, url, start, len, build.bytes, &received, stop)
                        }))
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("a download worker panicked")));
                        let _ = tx.send((start, result));
                    })?;
                    next += len;
                }
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok((start, result)) => {
                        pending.get_mut(&start).context("unexpected download range")?.data = Some(result?);
                        while pending.get(&committed).is_some_and(|p| p.data.is_some()) {
                            let data = pending.remove(&committed).unwrap().data.unwrap();
                            out.write_all(&data)?;
                            hasher.update(&data);
                            committed += data.len() as u64;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => bail!("download workers stopped"),
                }
            }
            report(Fetch::Downloading { done: build.bytes, total: build.bytes });
            Ok(())
        })();
        // In-flight workers stop between reads; scope joins them before any
        // caller can restart the download or remove its partial file.
        stop.store(true, Relaxed);
        result
    })
}

#[cfg(test)]
#[path = "parallel_tests.rs"]
mod tests;
