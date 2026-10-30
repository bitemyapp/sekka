//! Dormant, bounded native timeline transport. No threads, timers or SDK runtime.
//! All timestamps are Linux CLOCK_MONOTONIC nanoseconds (suspend excluded).
use serde::{Deserialize, Serialize};
use std::io;
use std::os::unix::{fs::MetadataExt, net::UnixDatagram};
use std::path::Path;
use std::sync::{
    RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub session: u64,
    pub socket: String,
    pub deadline_ns: u64,
}

pub fn now() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // CLOCK_MONOTONIC is available on every supported Linux target.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

impl Config {
    pub fn validate(&self) -> io::Result<()> {
        let remaining = self.deadline_ns.saturating_sub(now());
        if self.session == 0 || remaining == 0 || remaining > 61_000_000_000 {
            return Err(io::Error::other("profiling sessions must expire within 60 seconds"));
        }
        let path = Path::new(&self.socket);
        let parent = path.parent().ok_or_else(|| io::Error::other("missing capture directory"))?;
        let m = parent.symlink_metadata()?;
        if !path.is_absolute() || !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err(io::Error::other("capture socket needs a private directory owned by this user"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Phase {
    Started = 1,
    Stopped,
    AppEvent,
    Update,
    ViewLayout,
    PaintRecording,
    InputDispatch,
    SurfaceCommit,
    Render,
    FrameQueued,
    PresentationEstimate,
    DrmPresentation,
}
impl Phase {
    pub fn from_id(id: u64) -> Option<Self> {
        Some(match id {
            1 => Self::Started,
            2 => Self::Stopped,
            3 => Self::AppEvent,
            4 => Self::Update,
            5 => Self::ViewLayout,
            6 => Self::PaintRecording,
            7 => Self::InputDispatch,
            8 => Self::SurfaceCommit,
            9 => Self::Render,
            10 => Self::FrameQueued,
            11 => Self::PresentationEstimate,
            12 => Self::DrmPresentation,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Started => "capture started",
            Self::Stopped => "capture stopped",
            Self::AppEvent => "app event dispatch",
            Self::Update => "app update",
            Self::ViewLayout => "view and layout",
            Self::PaintRecording => "paint recording (before raster)",
            Self::InputDispatch => "compositor input dispatch",
            Self::SurfaceCommit => "surface commit (correlation only)",
            Self::Render => "compositor render",
            Self::FrameQueued => "frame queued",
            Self::PresentationEstimate => "presentation estimate (no hardware timestamp)",
            Self::DrmPresentation => "DRM presentation",
        }
    }
}

/// Fixed-size, versioned packet; credentials on the datagram supply the PID.
pub const PACKET_SIZE: usize = 80;
#[derive(Debug)]
pub struct Record {
    pub session: u64,
    pub phase: Phase,
    pub start: u64,
    pub duration: u64,
    pub subject: u64,
    pub aux: u64,
    pub dropped: u64,
    pub tid: u64,
    pub sequence: u64,
}
impl Record {
    pub fn encode(&self) -> [u8; PACKET_SIZE] {
        let mut out = [0; PACKET_SIZE];
        for (chunk, value) in out.chunks_exact_mut(8).zip([
            0x415a545200000001,
            self.session,
            self.phase as u64,
            self.start,
            self.duration,
            self.subject,
            self.aux,
            self.dropped,
            self.tid,
            self.sequence,
        ]) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        out
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PACKET_SIZE {
            return None;
        }
        let v: Vec<u64> = bytes.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
        if v[0] != 0x415a545200000001 {
            return None;
        }
        Some(Self {
            session: v[1],
            phase: Phase::from_id(v[2])?,
            start: v[3],
            duration: v[4],
            subject: v[5],
            aux: v[6],
            dropped: v[7],
            tid: v[8],
            sequence: v[9],
        })
    }
}
struct Active {
    config: Config,
    socket: UnixDatagram,
    lost: AtomicU64,
    sequence: AtomicU64,
}
static ENABLED: AtomicBool = AtomicBool::new(false);
static ACTIVE: RwLock<Option<Active>> = RwLock::new(None);
thread_local! { static TID:u64 = unsafe { libc::syscall(libc::SYS_gettid) as u64 }; }

#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
pub fn session() -> Option<u64> {
    ACTIVE.read().ok()?.as_ref().filter(|a| a.config.deadline_ns > now() && enabled()).map(|a| a.config.session)
}
pub fn start(config: Config) -> io::Result<()> {
    config.validate()?;
    let mut a = ACTIVE.write().map_err(|_| io::Error::other("trace lock poisoned"))?;
    if let Some(previous) = a.as_ref()
        && previous.config.deadline_ns > now()
        && enabled()
    {
        // An idle app may not have emitted since its collector crashed. A
        // connect-only probe distinguishes that stale session without a timer.
        match UnixDatagram::unbound()?.connect(&previous.config.socket) {
            Ok(()) => return Err(io::Error::other("a capture is already active")),
            Err(e) if matches!(e.kind(), io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound) => {}
            Err(e) => return Err(e),
        }
    }
    let socket = UnixDatagram::unbound()?;
    socket.set_nonblocking(true)?;
    socket.connect(&config.socket)?;
    *a = Some(Active { config, socket, lost: AtomicU64::new(0), sequence: AtomicU64::new(0) });
    ENABLED.store(true, Ordering::Release);
    drop(a);
    instant(Phase::Started, 0, 0);
    Ok(())
}
pub fn stop(id: u64) {
    let mut a = ACTIVE.write().unwrap_or_else(|e| e.into_inner());
    if a.as_ref().is_some_and(|a| a.config.session == id) {
        if let Some(active) = a.as_ref() {
            send(active, Phase::Stopped, now(), 0, 0, 0);
        }
        ENABLED.store(false, Ordering::Release);
        *a = None;
    }
}
pub fn stop_all() {
    let mut active = ACTIVE.write().unwrap_or_else(|e| e.into_inner());
    ENABLED.store(false, Ordering::Release);
    *active = None;
}

fn retire_disabled() {
    if !enabled()
        && let Ok(mut active) = ACTIVE.try_write()
        && !enabled()
    {
        *active = None;
    }
}
fn send(a: &Active, phase: Phase, start: u64, duration: u64, subject: u64, aux: u64) {
    let r = Record {
        session: a.config.session,
        phase,
        start,
        duration,
        subject,
        aux,
        dropped: a.lost.load(Ordering::Relaxed),
        tid: TID.with(|t| *t),
        sequence: a.sequence.fetch_add(1, Ordering::Relaxed),
    };
    if let Err(e) = a.socket.send(&r.encode()) {
        a.lost.fetch_add(1, Ordering::Relaxed);
        if !matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) {
            ENABLED.store(false, Ordering::Release);
        }
    }
}
pub fn instant(phase: Phase, subject: u64, aux: u64) {
    if !enabled() {
        return;
    }
    if let Ok(a) = ACTIVE.try_read()
        && let Some(a) = a.as_ref()
    {
        let time = now();
        if time >= a.config.deadline_ns {
            ENABLED.store(false, Ordering::Release);
        } else {
            send(a, phase, time, 0, subject, aux);
        }
    }
    retire_disabled();
}
pub struct Span {
    start: u64,
    session: u64,
    phase: Phase,
    subject: u64,
    aux: u64,
}
impl Span {
    #[inline]
    pub fn new(phase: Phase, subject: u64, aux: u64) -> Self {
        let mut s = Self { start: 0, session: 0, phase, subject, aux };
        if !enabled() {
            return s;
        }
        if let Ok(a) = ACTIVE.try_read()
            && let Some(a) = a.as_ref()
        {
            let time = now();
            if time < a.config.deadline_ns {
                s.start = time;
                s.session = a.config.session;
            } else {
                ENABLED.store(false, Ordering::Release);
            }
        }
        retire_disabled();
        s
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if self.start == 0 || !enabled() {
            return;
        }
        if let Ok(a) = ACTIVE.try_read()
            && let Some(a) = a.as_ref()
            && a.config.session == self.session
        {
            let end = now();
            if end < a.config.deadline_ns {
                send(a, self.phase, self.start, end - self.start, self.subject, self.aux);
            }
        }
        retire_disabled();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_and_stale_span() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aztrace-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("s");
        let sock = UnixDatagram::bind(&path).unwrap();
        sock.set_nonblocking(true).unwrap();
        let config =
            |session| Config { session, socket: path.to_string_lossy().into(), deadline_ns: now() + 1_000_000_000 };
        assert!(!enabled());
        let mut bytes = [0; PACKET_SIZE];
        instant(Phase::Update, 0, 0);
        assert!(sock.recv(&mut bytes).is_err());
        start(config(1)).unwrap();
        assert!(start(config(2)).is_err());
        let stale = Span::new(Phase::Update, 0, 0);
        stop(1);
        start(config(2)).unwrap();
        stop(1);
        assert_eq!(session(), Some(2));
        drop(stale);
        stop(2);
        let mut phases = Vec::new();
        while let Ok(n) = sock.recv(&mut bytes) {
            phases.push(Record::decode(&bytes[..n]).unwrap().phase);
        }
        assert_eq!(phases, vec![Phase::Started, Phase::Stopped, Phase::Started, Phase::Stopped]);
        assert!(!enabled());
        assert!(Record::decode(&[0; 79]).is_none());
        let mut invalid = config(3);
        invalid.deadline_ns = now() + 62_000_000_000;
        assert!(start(invalid).is_err());
        start(config(4)).unwrap();
        let before = std::time::Instant::now();
        for _ in 0..1000 {
            instant(Phase::Update, 0, 0);
        }
        assert!(
            before.elapsed() < std::time::Duration::from_millis(500),
            "a full collector queue must not block the app"
        );
        while sock.recv(&mut bytes).is_ok() {}
        instant(Phase::Update, 0, 0);
        let n = sock.recv(&mut bytes).unwrap();
        assert!(Record::decode(&bytes[..n]).unwrap().dropped > 0);
        stop(4);
        let mut expires = config(5);
        expires.deadline_ns = now() + 5_000_000;
        start(expires).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        instant(Phase::Update, 0, 0);
        assert!(!enabled());
        stop(5);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
