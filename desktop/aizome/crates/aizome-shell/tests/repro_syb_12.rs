//! SYB-12 (regression): logind's sleep delay and the lock before sleep,
//! across a sleep that came before the lock was confirmed. The real shell
//! (headless) and the real `aizome_sys::power::Logind` follower, against a
//! fake logind on a private dbus-daemon whose `Inhibit` hands out pipes (a
//! write end closed = the inhibitor released). Nothing suspends:
//! "sleeping" is the fake emitting PrepareForSleep(true), then (false).

mod common;

use std::io::{BufRead, BufReader};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aizome_shell::lock::Phase;
use aizome_shell::shell::Msg;
use aizome_sys::Wake;
use aizome_sys::power::Logind;
use common::*;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

/// A private dbus-daemon that allows everything and activates nothing.
struct TestBus {
    child: Child,
    address: String,
    dir: PathBuf,
}

impl TestBus {
    fn start() -> Option<TestBus> {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("repro-syb-12-bus-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                dir.display()
            ),
        )
        .ok()?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        let address = line.trim().to_owned();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(TestBus { child, address, dir })
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Default)]
struct Log {
    /// The read ends of the inhibitor pipes handed out.
    inhibitors: Vec<OwnedFd>,
}

struct LoginManager {
    log: Arc<Mutex<Log>>,
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl LoginManager {
    fn get_session(&self, id: String) -> fdo::Result<OwnedObjectPath> {
        if id == "7" {
            Ok(OwnedObjectPath::try_from("/org/freedesktop/login1/session/_37").unwrap())
        } else {
            Err(fdo::Error::Failed("no session".into()))
        }
    }
    fn inhibit(&self, what: String, _who: String, _why: String, mode: String) -> zbus::zvariant::OwnedFd {
        assert_eq!((what.as_str(), mode.as_str()), ("sleep", "delay"));
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors we then own.
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) }, 0);
        // SAFETY: fresh descriptors.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        self.log.lock().unwrap().inhibitors.push(read);
        zbus::zvariant::OwnedFd::from(write)
    }
    #[zbus(signal)]
    async fn prepare_for_sleep(emitter: &SignalEmitter<'_>, start: bool) -> zbus::Result<()>;
}

struct LoginSession;

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl LoginSession {
    #[zbus(property)]
    fn id(&self) -> String {
        "7".into()
    }
    #[zbus(property)]
    fn active(&self) -> bool {
        true
    }
    fn set_locked_hint(&self, _locked: bool) {}
}

/// Whether every write end of the pipe whose read end is `read` is closed.
fn released(read: &OwnedFd) -> bool {
    let mut byte = 0u8;
    // SAFETY: reading one byte into a valid buffer from a pipe we own.
    let n = unsafe { libc::read(read.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
    n == 0
}

/// One test at a time: they set the environment, and share the harness's
/// process-wide state.
fn serially() -> std::sync::MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    static SESSION: std::sync::Once = std::sync::Once::new();
    let guard = ONE.lock().unwrap_or_else(|e| e.into_inner());
    // The shell follows logind's session events only in an Aizome
    // session. SAFETY: no other test thread runs past `ONE` meanwhile.
    SESSION.call_once(|| unsafe { std::env::set_var("AIZOME_SESSION", "1") });
    guard
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_sleep_delay_is_held_for_the_next_sleep() {
    let _one = serially();
    let Some(bus) = TestBus::start() else {
        eprintln!("skipping: no dbus-daemon");
        return;
    };
    let log = Arc::new(Mutex::new(Log::default()));
    let fake = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.freedesktop.login1")
        .unwrap()
        .serve_at("/org/freedesktop/login1", LoginManager { log: log.clone() })
        .unwrap()
        .serve_at("/org/freedesktop/login1/session/_37", LoginSession)
        .unwrap()
        .build()
        .unwrap();
    let emit = |start: bool| {
        fake.emit_signal(
            None::<&str>,
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
            &(start,),
        )
        .unwrap();
    };
    let logind = Logind::with_bus(aizome_sys::Bus::at(&bus.address).unwrap(), Wake::none(), Some("7".into()));
    until("logind available", || logind.available());

    let mut h = scene(false);
    assert!(h.program().lock_cfg.before_sleep);
    // What start_lock does in a session: hold the delay.
    logind.take_sleep_delay().unwrap();
    h.program_mut().sys.logind = Some(logind);
    let inhibitors = |log: &Arc<Mutex<Log>>| log.lock().unwrap().inhibitors.len();
    assert_eq!(inhibitors(&log), 1);

    // The lid closes: logind says it's about to sleep; the shell locks.
    emit(true);
    until("the shell locking for sleep", || {
        h.send(Msg::Sys);
        h.program().lock.holds_sleep() && h.program().lock.phase() == Phase::Locking
    });
    assert!(!released(&log.lock().unwrap().inhibitors[0]), "sleep waits for the lock");

    // The compositor doesn't confirm the lock within logind's delay limit
    // (InhibitDelayMaxSec): the machine sleeps anyway, and wakes.
    let preparing = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        h.program().sys.logind.as_ref().unwrap().snapshot().preparing_for_sleep
    };
    emit(false);
    until("the shell hearing the resume", || {
        h.send(Msg::Sys);
        !preparing(&h) && !h.program().lock.holds_sleep()
    });
    // Now the compositor confirms the lock.
    h.confirm_lock();
    assert_eq!(h.program().lock.phase(), Phase::Locked);
    std::thread::sleep(Duration::from_millis(100));
    h.send(Msg::Sys);

    let log = log.lock().unwrap();
    let held: Vec<bool> = log.inhibitors.iter().map(|r| !released(r)).collect();
    let model_held = h.program().sys.logind.as_ref().unwrap().snapshot().sleep_delay_held;
    assert!(
        held.iter().any(|h| *h) && model_held,
        "no sleep delay is held for the next sleep: inhibitors taken {}, held now {held:?}, \
         LogindState.sleep_delay_held {model_held}, a sleep waiting for the lock {}",
        held.len(),
        h.program().lock.holds_sleep()
    );
}

/// The other order: logind resumes the machine from a sleep the lock
/// let go (it was confirmed in time), and the next sleep waits for its
/// own lock.
#[test]
fn a_confirmed_lock_lets_its_sleep_go_and_the_next_one_waits() {
    let _one = serially();
    let Some(bus) = TestBus::start() else {
        eprintln!("skipping: no dbus-daemon");
        return;
    };
    let log = Arc::new(Mutex::new(Log::default()));
    let fake = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.freedesktop.login1")
        .unwrap()
        .serve_at("/org/freedesktop/login1", LoginManager { log: log.clone() })
        .unwrap()
        .serve_at("/org/freedesktop/login1/session/_37", LoginSession)
        .unwrap()
        .build()
        .unwrap();
    let emit = |start: bool| {
        fake.emit_signal(
            None::<&str>,
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
            &(start,),
        )
        .unwrap();
    };
    let logind = Logind::with_bus(aizome_sys::Bus::at(&bus.address).unwrap(), Wake::none(), Some("7".into()));
    until("logind available", || logind.available());
    let mut h = scene(false);
    logind.take_sleep_delay().unwrap();
    h.program_mut().sys.logind = Some(logind);
    let held = |log: &Arc<Mutex<Log>>, i: usize| !released(&log.lock().unwrap().inhibitors[i]);

    emit(true);
    until("the shell locking for sleep", || {
        h.send(Msg::Sys);
        h.program().lock.holds_sleep()
    });
    assert!(held(&log, 0), "sleep waits for the lock");
    h.confirm_lock();
    until("the sleep let go", || !held(&log, 0));
    emit(false);
    until("the delay taken again", || {
        h.send(Msg::Sys);
        log.lock().unwrap().inhibitors.len() == 2
    });
    // Unlocked, then the lid closes again: that sleep waits for the next lock.
    h.program_mut().authenticator = Arc::new(aizome_shell::pam::Fixed("hunter2".into()));
    h.send(Msg::Lock(aizome_shell::lock::LockMsg::Password("hunter2".into())));
    h.send(Msg::Lock(aizome_shell::lock::LockMsg::Submit));
    until("unlocked", || {
        h.send(Msg::Nothing);
        h.program().lock.phase() == Phase::Unlocked
    });
    emit(true);
    until("the shell locking for the next sleep", || {
        h.send(Msg::Sys);
        h.program().lock.holds_sleep()
    });
    assert!(held(&log, 1), "the next sleep waits for its lock");
    h.confirm_lock();
    until("that sleep let go", || !held(&log, 1));
}
