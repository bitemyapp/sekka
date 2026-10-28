//! User-owned, finite captures. Call `record` on a worker, never a GUI thread.
use aizome_ipc::{Client, Request, Response};
use aizome_trace::{Config, Phase, Record};
use serde::{Deserialize, Serialize};
use serde_json::json;

mod client;
pub use client::record_process;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Message {
    Progress { message: String },
    Finished { result: Result<Outcome, Failure> },
}
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::{
    fd::AsRawFd,
    unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        net::UnixDatagram,
        process::CommandExt,
    },
};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub const FILE_LIMIT: u64 = 256 * 1024 * 1024;
const TIMELINE_LIMIT: u64 = 32 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub pid: u32,
    pub start_ticks: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Mode {
    Timeline,
    Cpu,
}
#[derive(Clone, Debug)]
pub struct Options {
    pub targets: Vec<Target>,
    pub seconds: u64,
    pub mode: Mode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub path: PathBuf,
    pub native_participants: usize,
    pub reported_drops: u64,
    pub stop_reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    pub message: String,
    pub path: Option<PathBuf>,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(path) = &self.path {
            write!(f, " (capture: {})", path.display())?;
        }
        Ok(())
    }
}

/// Dropping the owner requests cancellation, including while setup is underway.
pub struct Capture {
    stop: Arc<AtomicBool>,
}
impl Capture {
    pub fn new() -> Self {
        Self { stop: Arc::new(AtomicBool::new(false)) }
    }
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn identity(pid: u32) -> io::Result<Target> {
    let dir = PathBuf::from(format!("/proc/{pid}"));
    if pid == 0 || fs::metadata(&dir)?.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("select processes owned by your user"));
    }
    let stat = fs::read_to_string(dir.join("stat"))?;
    let tail = stat.rsplit_once(") ").ok_or_else(|| io::Error::other("invalid process stat"))?.1;
    let start_ticks = tail
        .split_whitespace()
        .nth(19)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::other("missing process start time"))?;
    Ok(Target { pid, start_ticks })
}
fn validate(targets: &[Target]) -> io::Result<()> {
    if targets.is_empty() || targets.len() > 64 {
        return Err(io::Error::other("select between 1 and 64 processes"));
    }
    for t in targets {
        if identity(t.pid)? != *t {
            return Err(io::Error::other("a selected process exited or its PID was reused; select it again"));
        }
    }
    Ok(())
}
fn client() -> io::Result<Client> {
    let c = Client::connect()?;
    c.stream().set_read_timeout(Some(Duration::from_secs(2)))?;
    c.stream().set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(c)
}
fn private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}
fn private_dir(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}
fn state_dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .ok_or_else(|| io::Error::other("no state directory"))?;
    let dir = base.join("aizome/profiles");
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
    Ok(dir)
}
struct Session {
    id: u64,
    pids: Vec<u32>,
    socket_dir: PathBuf,
    comp: bool,
    apps: bool,
    perf: Option<Child>,
    log: Option<(ChildStderr, File, usize)>,
}
impl Session {
    fn drain_log(&mut self) {
        if let Some((pipe, file, written)) = &mut self.log {
            let mut buf = [0; 4096];
            // Bound work per iteration too, even for a misbehaving perf executable.
            for _ in 0..16 {
                let Ok(n) = pipe.read(&mut buf) else {
                    break;
                };
                if n == 0 {
                    break;
                }
                let keep = n.min((64 * 1024usize).saturating_sub(*written));
                let _ = file.write_all(&buf[..keep]);
                *written += keep;
            }
        }
    }
    fn disarm(&mut self) {
        if self.apps {
            if let Some(path) = aizome_shell_proto::socket_path() {
                let _ = aizome_shell_proto::request(
                    path,
                    aizome_shell_proto::ShellRequest::ProfileStop { pids: self.pids.clone(), session: self.id },
                );
            }
            self.apps = false;
        }
        if self.comp {
            if let Ok(mut c) = client() {
                let _ = c.call(&Request::ProfileStop { session: self.id });
            }
            self.comp = false;
        }
        if let Some(mut child) = self.perf.take() {
            // try_wait may already have reaped a failed sampler. Never signal
            // its numeric PID after that: the kernel can reuse it immediately.
            if matches!(child.try_wait(), Ok(None)) {
                unsafe {
                    libc::kill(child.id() as i32, libc::SIGINT);
                }
            }
            let until = Instant::now() + Duration::from_secs(2);
            loop {
                self.drain_log();
                if !matches!(child.try_wait(), Ok(None)) {
                    break;
                }
                if Instant::now() >= until {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            self.drain_log();
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.disarm();
        let _ = fs::remove_file(self.socket_dir.join("events"));
        let _ = fs::remove_dir(&self.socket_dir);
    }
}

fn start_perf(options: &Options, dir: &Path) -> io::Result<Child> {
    let program = std::env::var_os("AIZOME_PERF").unwrap_or_else(|| "perf".into());
    let mut cmd = Command::new(program);
    cmd.args([
        "record",
        "--clockid",
        "mono",
        "-e",
        "cpu-clock:u",
        "-F",
        "99",
        "--call-graph",
        "dwarf,8192",
        "--mmap-pages",
        "64",
        "--no-inherit",
        "-p",
    ])
    .arg(options.targets.iter().map(|t| t.pid.to_string()).collect::<Vec<_>>().join(","))
    .arg("-o")
    .arg(dir.join("perf.data"))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        cmd.pre_exec(move || {
            libc::umask(0o077);
            // A killed GUI/CLI must not leave perf running. No privileges added.
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::other("capture owner exited"));
            }
            let limit = libc::rlimit {
                rlim_cur: FILE_LIMIT - TIMELINE_LIMIT - 4 * 1024 * 1024,
                rlim_max: FILE_LIMIT - TIMELINE_LIMIT - 4 * 1024 * 1024,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()
        .map_err(|e| io::Error::other(format!("CPU sampler could not start: {e}. Install perf or set AIZOME_PERF.")))
}

/// Receives kernel-provided credentials, never trusts a packet's process ID.
fn receive(socket: &UnixDatagram) -> io::Result<Option<(u32, Record)>> {
    let mut bytes = [0u8; aizome_trace::PACKET_SIZE];
    // u64 gives the ancillary buffer cmsghdr alignment on supported targets.
    let mut control = [0u64; 8];
    let mut iov = libc::iovec { iov_base: bytes.as_mut_ptr().cast(), iov_len: bytes.len() };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control);
    let n = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut msg, 0) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    if msg.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0 {
        return Ok(None);
    }
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&msg);
        if header.is_null() || (*header).cmsg_level != libc::SOL_SOCKET || (*header).cmsg_type != libc::SCM_CREDENTIALS
        {
            return Ok(None);
        }
        let cred = std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<libc::ucred>());
        if cred.uid != libc::geteuid() || cred.pid <= 0 {
            return Ok(None);
        }
        Ok(Record::decode(&bytes[..n as usize]).map(|r| (cred.pid as u32, r)))
    }
}

/// Progress is a short user-facing status. The returned directory is private.
pub fn record(options: Options, stop: Arc<AtomicBool>, mut progress: impl FnMut(String)) -> Result<Outcome, Failure> {
    let mut path = None;
    record_inner(options, stop, &mut progress, &mut path).map_err(|e| Failure { message: e.to_string(), path })
}
fn record_inner(
    options: Options,
    stop: Arc<AtomicBool>,
    progress: &mut dyn FnMut(String),
    saved: &mut Option<PathBuf>,
) -> io::Result<Outcome> {
    validate(&options.targets)?;
    if ![10, 30, 60].contains(&options.seconds) {
        return Err(io::Error::other("choose 10, 30 or 60 seconds"));
    }
    if stop.load(Ordering::Acquire) {
        return Err(io::Error::other("capture cancelled"));
    }
    let mut ipc = client()?;
    let (comp_pid, backend) = match ipc.call(&Request::ProfileState)? {
        Response::Profile { pid, locked: false, session: None, backend } => (pid, backend),
        _ => return Err(io::Error::other("the desktop is locked or another capture is active")),
    };
    let id = aizome_trace::now();
    let dir = state_dir()?.join(format!("capture-{}-{id}", std::process::id()));
    private_dir(&dir)?;
    *saved = Some(dir.clone());
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| io::Error::other("no user runtime directory"))?;
    let socket_dir = PathBuf::from(runtime).join(format!("azprofile-{}-{id}", std::process::id()));
    private_dir(&socket_dir)?;
    let mut session = Session {
        id,
        pids: options.targets.iter().map(|t| t.pid).collect(),
        socket_dir,
        comp: false,
        apps: false,
        perf: None,
        log: None,
    };
    let socket_path = session.socket_dir.join("events");
    let socket = UnixDatagram::bind(&socket_path)?;
    let on: libc::c_int = 1;
    if unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            (&on as *const libc::c_int).cast(),
            std::mem::size_of_val(&on) as _,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    let config = Config {
        session: id,
        socket: socket_path.to_string_lossy().into(),
        deadline_ns: aizome_trace::now() + options.seconds * 1_000_000_000,
    };
    let mut manifest = json!({"version":1,"session":id,"mode":options.mode,"targets":options.targets,"clock":"CLOCK_MONOTONIC nanoseconds; trace timestamps in microseconds","backend":backend,"status":"starting","symbols":[],"limits":{"timeline_bytes":TIMELINE_LIMIT,"total_file_budget_bytes":FILE_LIMIT,"duration_seconds":options.seconds},"coverage":"App phases and compositor events are time-correlated; no causal input/render token, raster, async task or GPU execution tracks. Existing processes only; no late worker discovery. DRM ts/aux use the hardware monotonic presentation timestamp; callback_receipt_ns records delivery. Headless/winit presentation is estimated. Compositor render/input tracks include desktop work outside the selection; surface commits are filtered to selected PIDs."});
    let manifest_path = dir.join("manifest.json");
    let mut manifest_file = private_file(&manifest_path)?;
    serde_json::to_writer_pretty(&mut manifest_file, &manifest)?;
    let result = (|| -> io::Result<()> {
        if options.mode == Mode::Cpu {
            session.perf = Some(start_perf(&options, &dir)?);
            let stderr = session.perf.as_mut().unwrap().stderr.take().unwrap();
            if unsafe { libc::fcntl(stderr.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
                return Err(io::Error::last_os_error());
            }
            session.log = Some((stderr, private_file(&dir.join("perf.log"))?, 0));
            std::thread::sleep(Duration::from_millis(150));
            if let Some(status) = session.perf.as_mut().unwrap().try_wait()? {
                return Err(io::Error::other(format!(
                    "CPU sampling unavailable ({status}). Open Folder and check perf.log for permissions or tool availability."
                )));
            }
            validate(&options.targets)?;
        }
        ipc.call(&Request::ProfileStart { config: config.clone() })?;
        session.comp = true;
        let routed = if let Some(path) = aizome_shell_proto::socket_path() {
            let (ok, message) = aizome_shell_proto::request(
                path,
                aizome_shell_proto::ShellRequest::Profile { pids: session.pids.clone(), config: config.clone() },
            )?;
            if !ok {
                return Err(io::Error::other(message));
            }
            session.apps = true;
            serde_json::from_str::<Vec<u32>>(&message).unwrap_or_default()
        } else {
            Vec::new()
        };
        manifest["native_routed"] = json!(routed);
        // Symbol reads occur here, off all app/compositor threads.
        let mut symbol_pids = session.pids.clone();
        symbol_pids.push(comp_pid);
        manifest["symbols"] = json!(modules(&symbol_pids));
        let mut out = BufWriter::with_capacity(64 * 1024, private_file(&dir.join("trace.json"))?);
        out.write_all(b"{\"displayTimeUnit\":\"ms\",\"traceEvents\":[\n")?;
        let mut bytes = 55;
        let mut count = 0u64;
        let mut observed = BTreeSet::new();
        let mut loss = BTreeMap::new();
        let mut last_check = Instant::now();
        let mut reason = "duration reached";
        progress(format!(
            "Recording {} seconds — {} registered app(s); native participation is verified in the saved capture",
            options.seconds,
            routed.len()
        ));
        while aizome_trace::now() < config.deadline_ns {
            session.drain_log();
            if stop.load(Ordering::Acquire) {
                reason = "stopped by user";
                break;
            }
            if last_check.elapsed() >= Duration::from_millis(500) {
                if validate(&options.targets).is_err() {
                    reason = "a selected process exited or changed identity";
                    break;
                }
                match ipc.call(&Request::ProfileState)? {
                    Response::Profile { locked: false, session: Some(s), .. } if s == id => {}
                    _ => {
                        reason = "desktop locked or capture revoked";
                        break;
                    }
                }
                if let Some(child) = session.perf.as_mut()
                    && let Some(status) = child.try_wait()?
                {
                    if !status.success() {
                        return Err(io::Error::other(format!("CPU sampler failed ({status}); inspect perf.log")));
                    }
                    reason = "CPU sampler exited";
                    break;
                }
                last_check = Instant::now();
            }
            match receive(&socket) {
                Ok(Some((pid, r))) if r.session == id && (pid == comp_pid || session.pids.contains(&pid)) => {
                    observed.insert(pid);
                    let dropped = loss.entry(pid).or_insert(0);
                    *dropped = (*dropped).max(r.dropped);
                    // Only selected clients' commits; global compositor work remains
                    // visible and explicitly cannot be attributed to one application.
                    if r.phase == Phase::SurfaceCommit && !session.pids.contains(&(r.aux as u32)) {
                        continue;
                    }
                    let mut event = json!({"name":r.phase.name(),"cat":"aizome","pid":pid,"tid":r.tid,"ts":r.start as f64/1000.0,"args":{"subject":r.subject,"aux":r.aux,"producer_sequence":r.sequence,"dropped_before":r.dropped}});
                    if r.phase == Phase::DrmPresentation && r.aux != 0 {
                        event["ts"] = json!(r.aux as f64 / 1000.0);
                        event["args"]["callback_receipt_ns"] = json!(r.start);
                    }
                    if r.duration > 0 {
                        event["ph"] = json!("X");
                        event["dur"] = json!(r.duration as f64 / 1000.0);
                    } else {
                        event["ph"] = json!("i");
                        event["s"] = json!("t");
                    }
                    let encoded = serde_json::to_vec(&event)?;
                    if bytes + encoded.len() as u64 + 4 > TIMELINE_LIMIT {
                        reason = "timeline file limit reached";
                        break;
                    }
                    if count > 0 {
                        out.write_all(b",\n")?;
                    }
                    out.write_all(&encoded)?;
                    bytes += encoded.len() as u64 + 2;
                    count += 1;
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e),
            }
        }
        session.disarm();
        out.write_all(b"\n]}\n")?;
        out.flush()?;
        manifest["stop_reason"] = json!(reason);
        manifest["events"] = json!(count);
        manifest["native_observed"] = json!(observed.into_iter().filter(|p| *p != comp_pid).collect::<Vec<_>>());
        manifest["producer_loss_lower_bound"] = json!(loss);
        manifest["loss_note"] =
            json!("Counts are lower bounds: final dropped packets and producer death can hide tail loss.");
        manifest["symbols_after"] = json!(modules(&symbol_pids));
        Ok(())
    })();
    session.disarm();
    manifest["status"] = json!(if result.is_ok() { "finished" } else { "failed" });
    if let Err(e) = &result {
        manifest["error"] = json!(e.to_string());
    }
    let encoded = serde_json::to_vec_pretty(&manifest)?;
    if encoded.len() > 3 * 1024 * 1024 {
        return Err(io::Error::other("symbol manifest exceeded its file budget"));
    }
    manifest_file.set_len(0)?;
    manifest_file.seek(SeekFrom::Start(0))?;
    manifest_file.write_all(&encoded)?;
    manifest_file.flush()?;
    result?;
    Ok(Outcome {
        path: dir,
        native_participants: manifest["native_observed"].as_array().map_or(0, Vec::len),
        reported_drops: manifest["producer_loss_lower_bound"]
            .as_object()
            .map_or(0, |m| m.values().filter_map(|v| v.as_u64()).sum()),
        stop_reason: manifest["stop_reason"].as_str().unwrap_or("finished").into(),
    })
}

/// Bounded ELF64 little-endian PT_NOTE reader: no full-file mapping or debug download.
pub fn build_id(path: &Path) -> io::Result<Option<String>> {
    let mut f = File::open(path)?;
    let mut h = [0u8; 64];
    f.read_exact(&mut h)?;
    if &h[..6] != b"\x7fELF\x02\x01" {
        return Ok(None);
    }
    let u16at = |i| u16::from_le_bytes(h[i..i + 2].try_into().unwrap()) as usize;
    let offset = u64::from_le_bytes(h[32..40].try_into().unwrap());
    let size = u16at(54);
    let count = u16at(56);
    if size < 56 || size > 1024 || count > 1024 {
        return Ok(None);
    }
    for i in 0..count {
        f.seek(SeekFrom::Start(offset + i as u64 * size as u64))?;
        let mut p = [0; 56];
        f.read_exact(&mut p)?;
        if u32::from_le_bytes(p[0..4].try_into().unwrap()) != 4 {
            continue;
        }
        let pos = u64::from_le_bytes(p[8..16].try_into().unwrap());
        let len = u64::from_le_bytes(p[32..40].try_into().unwrap());
        if len > 1024 * 1024 {
            continue;
        }
        f.seek(SeekFrom::Start(pos))?;
        let mut data = vec![0; len as usize];
        f.read_exact(&mut data)?;
        let mut at = 0;
        while at + 12 <= data.len() {
            let n = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            let d = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap()) as usize;
            let kind = u32::from_le_bytes(data[at + 8..at + 12].try_into().unwrap());
            let name = at + 12;
            let desc = name + ((n + 3) & !3);
            let end = desc + ((d + 3) & !3);
            if end > data.len() {
                break;
            }
            if kind == 3 && n == 4 && &data[name..name + 4] == b"GNU\0" && d <= 64 {
                return Ok(Some(data[desc..desc + d].iter().map(|b| format!("{b:02x}")).collect()));
            }
            at = end;
        }
    }
    Ok(None)
}
fn modules(pids: &[u32]) -> Vec<serde_json::Value> {
    let mut result = Vec::new();
    let mut cache = BTreeMap::new();
    let mut inventory_bytes = 0;
    for pid in pids {
        let mut maps = String::new();
        if let Ok(f) = File::open(format!("/proc/{pid}/maps")) {
            let _ = f.take(1024 * 1024).read_to_string(&mut maps);
        }
        let mut seen = BTreeSet::new();
        for line in maps.lines().take(8192) {
            if result.len() >= 4096 {
                return result;
            }
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 6 || !fields[1].contains('x') || !fields[5].starts_with('/') {
                continue;
            }
            let path = fields[5..].join(" ");
            if !seen.insert(path.clone()) {
                continue;
            }
            // Bound inventory memory even with thousands of very long mapped
            // paths. The manifest also has an independent encoded-size limit.
            inventory_bytes += path.len() + 512;
            if inventory_bytes > 512 * 1024 {
                return result;
            }
            let matches = fs::metadata(&path).is_ok_and(|m| {
                let dev = format!("{:02x}:{:02x}", libc::major(m.dev()), libc::minor(m.dev()));
                dev == fields[3] && m.ino().to_string() == fields[4]
            });
            let key = (path.clone(), fields[3].to_owned(), fields[4].to_owned());
            let id = cache
                .entry(key)
                .or_insert_with(|| if matches { build_id(Path::new(&path)).ok().flatten() } else { None });
            result.push(json!({"pid":pid,"path":path,"build_id":id,"mapped_device":fields[3],"mapped_inode":fields[4],"lookup":"build-ID; missing/deleted modules are explicit null"}));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_reused_identity_and_bad_bounds() {
        let mut t = identity(std::process::id()).unwrap();
        validate(&[t]).unwrap();
        t.start_ticks += 1;
        assert!(validate(&[t]).is_err());
        assert!(validate(&[]).is_err());
        let capture = Capture::new();
        let stop = capture.cancellation();
        drop(capture);
        assert!(stop.load(Ordering::Acquire));
    }
    #[test]
    fn reads_build_id_and_reports_missing_notes() {
        let path = std::env::temp_dir().join(format!("azprofile-elf-{}", std::process::id()));
        let mut elf = vec![0u8; 144];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        elf[32..40].copy_from_slice(&64u64.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1u16.to_le_bytes());
        elf[64..68].copy_from_slice(&4u32.to_le_bytes());
        elf[72..80].copy_from_slice(&120u64.to_le_bytes());
        elf[96..104].copy_from_slice(&24u64.to_le_bytes());
        elf[120..124].copy_from_slice(&4u32.to_le_bytes());
        elf[124..128].copy_from_slice(&8u32.to_le_bytes());
        elf[128..132].copy_from_slice(&3u32.to_le_bytes());
        elf[132..136].copy_from_slice(b"GNU\0");
        elf[136..144].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        fs::write(&path, &elf).unwrap();
        assert_eq!(build_id(&path).unwrap().as_deref(), Some("0102030405060708"));
        elf[128..132].copy_from_slice(&1u32.to_le_bytes());
        fs::write(&path, &elf).unwrap();
        assert_eq!(build_id(&path).unwrap(), None);
        fs::remove_file(path).unwrap();
    }
}
