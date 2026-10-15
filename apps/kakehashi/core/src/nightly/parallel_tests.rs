use super::*;
use crate::image::hex;
use crate::nightly::download;
use std::cell::Cell;
use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

#[derive(Clone, Copy)]
enum Mode {
    Ranges,
    Ignore,
    WrongRange,
    WrongTotal,
    Truncated,
    Corrupt,
    IgnoreLater,
    IgnoreOnce,
    FailOnce,
    TruncateOnce,
    DelayFirst,
}

struct Server {
    url: String,
    data: Arc<Vec<u8>>,
    requests: Arc<Mutex<Vec<(u64, u64)>>>,
    peak: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn new(bytes: usize, mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let data = Arc::new((0..bytes).map(|i| (i % 251) as u8).collect::<Vec<_>>());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let peak = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (d, r, p, s) = (data.clone(), requests.clone(), peak.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            let mut handlers = Vec::new();
            while !s.load(Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (data, requests, peak, active) = (d.clone(), r.clone(), p.clone(), active.clone());
                        handlers.push(std::thread::spawn(move || {
                            let n = active.fetch_add(1, Relaxed) + 1;
                            peak.fetch_max(n, Relaxed);
                            let _ = respond(stream, &data, &requests, mode);
                            active.fetch_sub(1, Relaxed);
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("test server: {e}"),
                }
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        Self { url, data, requests, peak, stop, thread: Some(thread) }
    }

    fn build(&self) -> Build {
        Build {
            id: "fixture".into(),
            revision: "test".into(),
            uploaded: "now".into(),
            iso: "image.iso".into(),
            bytes: self.data.len() as u64,
            sha256: hex(&Sha256::digest(&*self.data)),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn respond(mut stream: TcpStream, data: &[u8], requests: &Mutex<Vec<(u64, u64)>>, mode: Mode) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut requested = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
            let (a, b) = value.trim().split_once('-').unwrap();
            requested = Some((a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap()));
        }
    }
    let (a, b) = requested.unwrap_or((0, data.len() as u64 - 1));
    let count = {
        let mut requests = requests.lock().unwrap();
        requests.push((a, b));
        requests.iter().filter(|r| **r == (a, b)).count()
    };
    let probe = a == b;
    if matches!(mode, Mode::FailOnce) && !probe && count == 1 {
        return stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    }
    let ignored = matches!(mode, Mode::Ignore)
        || (matches!(mode, Mode::IgnoreLater) && !probe)
        || (matches!(mode, Mode::IgnoreOnce) && !probe && count == 1);
    let (start, end) = if ignored { (0, data.len() - 1) } else { (a as usize, b as usize) };
    if matches!(mode, Mode::DelayFirst) && a == 0 && !probe {
        std::thread::sleep(Duration::from_millis(200));
    }
    if ignored || requested.is_none() {
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())?;
    } else {
        let header_start = if matches!(mode, Mode::WrongRange) && !probe { start + 1 } else { start };
        let total = data.len() + usize::from(matches!(mode, Mode::WrongTotal) && !probe);
        write!(
            stream,
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {header_start}-{end}/{total}\r\nConnection: close\r\n\r\n",
            end - start + 1
        )?;
    }
    let mut body = data[start..=end].to_vec();
    if !probe {
        match mode {
            Mode::TruncateOnce if count == 1 => {
                body.pop();
            }
            Mode::Truncated => {
                body.pop();
            }
            Mode::Corrupt => {
                body[0] ^= 1;
            }
            _ => {}
        }
    }
    stream.write_all(&body)
}

#[test]
fn parallel_ranges_commit_in_order_and_match_the_golden_checksum() {
    let server = Server::new(1024 * 1024 + 37, Mode::DelayFirst);
    let build = server.build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("part");
    let mut out = File::create(&path).unwrap();
    let mut hasher = Sha256::new();
    let mut previous = 0;
    transfer_with_chunk(
        &build,
        &build.url(&server.url),
        0,
        &mut out,
        &mut hasher,
        &mut |p| {
            if let Fetch::Downloading { done, total } = p {
                assert!(done >= previous && done <= total);
                previous = done;
            }
        },
        &|| false,
        64 * 1024,
    )
    .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *server.data);
    assert_eq!(hex(&hasher.finalize()), build.sha256);
    assert!(server.peak.load(Relaxed) >= 2);
}

#[test]
fn cancellation_keeps_a_contiguous_prefix_and_resume_finishes_it() {
    let server = Server::new(2 * CHUNK as usize + 19, Mode::DelayFirst);
    let build = server.build();
    let dir = tempfile::tempdir().unwrap();
    let seen = Cell::new(0);
    let result = download(
        &build,
        &server.url,
        dir.path(),
        &mut |p| {
            if let Fetch::Downloading { done, .. } = p {
                seen.set(done);
            }
        },
        &|| seen.get() >= CHUNK,
    );
    let err = result.unwrap_err();
    assert!(err.is::<Cancelled>(), "{err:#}");
    let part = dir.path().join("image.iso.part");
    let prefix = std::fs::read(&part).unwrap();
    assert_eq!(prefix, server.data[..prefix.len()]);
    assert!(!dir.path().join("image.iso").exists());
    let path = download(&build, &server.url, dir.path(), &mut |_| {}, &|| false).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *server.data);
    assert!(!part.exists());
}

#[test]
fn legacy_partial_file_resumes_from_its_exact_length() {
    let server = Server::new(256 * 1024 + 37, Mode::Ranges);
    let build = server.build();
    let dir = tempfile::tempdir().unwrap();
    let have = 12345;
    std::fs::write(dir.path().join("image.iso.part"), &server.data[..have]).unwrap();
    let path = download(&build, &server.url, dir.path(), &mut |_| {}, &|| false).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *server.data);
    assert!(server.requests.lock().unwrap().iter().all(|(start, _)| *start >= have as u64));
}

#[test]
fn a_server_ignoring_ranges_restarts_without_appending_duplicate_bytes() {
    let server = Server::new(256 * 1024 + 37, Mode::Ignore);
    let build = server.build();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("image.iso.part"), &server.data[..123]).unwrap();
    let path = download(&build, &server.url, dir.path(), &mut |_| {}, &|| false).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *server.data);
}

#[test]
fn invalid_or_truncated_ranges_never_become_an_image() {
    for mode in [Mode::WrongRange, Mode::WrongTotal, Mode::Truncated, Mode::IgnoreLater] {
        let server = Server::new(256 * 1024 + 37, mode);
        let dir = tempfile::tempdir().unwrap();
        assert!(download(&server.build(), &server.url, dir.path(), &mut |_| {}, &|| false).is_err());
        assert!(!dir.path().join("image.iso").exists());
        assert!(!dir.path().join("image.iso.sha256").exists());
        assert_eq!(std::fs::metadata(dir.path().join("image.iso.part")).unwrap().len(), 0);
    }
}

#[test]
fn a_checksum_mismatch_deletes_the_invalid_partial_download() {
    let server = Server::new(256 * 1024 + 37, Mode::Corrupt);
    let dir = tempfile::tempdir().unwrap();
    let err = download(&server.build(), &server.url, dir.path(), &mut |_| {}, &|| false).unwrap_err();
    assert!(err.to_string().contains("SHA-256"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn a_complete_partial_file_is_verified_without_a_range_past_eof() {
    let server = Server::new(1024, Mode::Ranges);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("image.iso.part"), &*server.data).unwrap();
    download(&server.build(), "http://127.0.0.1:9", dir.path(), &mut |_| {}, &|| false).unwrap();
    assert!(dir.path().join("image.iso").exists());
}

fn sample(controller: &mut Adaptive, rate: u64) {
    if controller.settling {
        controller.observe(controller.since + SETTLE, controller.bytes);
    }
    controller.observe(controller.since + SAMPLE, controller.bytes + rate * SAMPLE.as_secs());
}

#[test]
fn concurrency_explores_then_backs_off_when_more_connections_stop_helping() {
    let mut c = Adaptive::new(Instant::now(), 0);
    sample(&mut c, 50);
    assert_eq!(c.limit, 4);
    sample(&mut c, 55);
    sample(&mut c, 55);
    assert_eq!(c.limit, 8);
    sample(&mut c, 150);
    assert_eq!(c.limit, 16);
    sample(&mut c, 152);
    assert_eq!(c.limit, 16);
    sample(&mut c, 151);
    assert_eq!(c.limit, 8);
    assert!(c.stopped);
    sample(&mut c, 300);
    assert_eq!(c.limit, 8);
}

#[test]
fn concurrency_has_a_hard_ceiling_even_when_every_probe_improves() {
    let mut c = Adaptive::new(Instant::now(), 0);
    for rate in [50, 100, 200, 400, 800, 1600] {
        sample(&mut c, rate);
        assert!(c.limit <= MAX_CONNECTIONS);
    }
    assert_eq!(c.limit, MAX_CONNECTIONS);
    assert!(c.stopped);
}

#[test]
fn a_transient_full_response_is_retried_without_corrupting_the_range() {
    let server = Server::new(256 * 1024 + 37, Mode::IgnoreOnce);
    let dir = tempfile::tempdir().unwrap();
    let path = download(&server.build(), &server.url, dir.path(), &mut |_| {}, &|| false).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *server.data);
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}

#[test]
fn transient_http_errors_and_interrupted_bodies_retry_only_the_uncommitted_range() {
    for mode in [Mode::FailOnce, Mode::TruncateOnce] {
        let server = Server::new(256 * 1024 + 37, mode);
        let dir = tempfile::tempdir().unwrap();
        let mut previous = 0;
        let path = download(
            &server.build(),
            &server.url,
            dir.path(),
            &mut |p| {
                if let Fetch::Downloading { done, .. } = p {
                    assert!(done >= previous);
                    previous = done;
                }
            },
            &|| false,
        )
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), *server.data);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
    }
}
