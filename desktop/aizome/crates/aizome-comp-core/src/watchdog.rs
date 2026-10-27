//! When the compositor's main loop stops, nothing in it can draw or
//! answer, so two things watch it from outside.
//!
//! - **systemd's watchdog.** Under `aizome.service` (`WatchdogSec=`), a
//!   timer on the main loop sends `WATCHDOG=1`; if the loop stalls, the
//!   timer doesn't fire and systemd kills the compositor (SIGABRT, so a
//!   core dump is kept) rather than leaving a frozen screen.
//!   `aizome-session` then starts it again. Apps lose their connection and
//!   close, which can't be helped.
//! - **A stall watcher thread.** The loop beats on every iteration. When it
//!   hasn't for [`STALL_LOG`], the thread logs that the loop is stalled;
//!   before systemd's deadline it captures where the main thread is (its
//!   backtrace, taken by a signal handler on that thread, and the kernel's
//!   `wchan` and `syscall` for it), logs it, and keeps a record in
//!   `$XDG_STATE_HOME/aizome/stall.log`. A marker beside it says a stall
//!   was in progress; if the loop comes back the marker goes, and if the
//!   compositor was killed instead, the next one finds it and the
//!   recovery overlay tells the person what happened
//!   ([`previous_stall`]).

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use tracing::{error, info, warn};

use crate::state::State;

/// A loop that hasn't come round for this long is stalled.
pub const STALL_LOG: Duration = Duration::from_secs(2);
/// Without systemd's watchdog: when to capture where it is stuck.
const CAPTURE_WITHOUT_SYSTEMD: Duration = Duration::from_secs(6);
/// A marker older than this is from another session, not a restart.
const NOTICE_WITHIN: Duration = Duration::from_secs(600);

/// Milliseconds since `EPOCH` at the loop's last beat.
static BEAT: AtomicU64 = AtomicU64::new(0);
static EPOCH: OnceLock<Instant> = OnceLock::new();
static CAPTURED: AtomicBool = AtomicBool::new(false);
static BACKTRACE: Mutex<Option<std::backtrace::Backtrace>> = Mutex::new(None);

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The main loop came round.
pub fn beat() {
    BEAT.store(now_ms(), Ordering::Relaxed);
}

/// The watchdog interval systemd asked for, if it is watching us.
fn systemd_interval() -> Option<Duration> {
    let usec: u64 = std::env::var("WATCHDOG_USEC").ok()?.parse().ok()?;
    if let Ok(pid) = std::env::var("WATCHDOG_PID")
        && pid.parse::<u32>().ok() != Some(std::process::id())
    {
        return None;
    }
    (usec > 0).then(|| Duration::from_micros(usec))
}

/// Starts the keepalive timer (under systemd) and the stall watcher.
pub fn start(state: &mut State) {
    beat();
    let deadline = systemd_interval();
    if let Some(interval) = deadline {
        crate::session::sd_notify("WATCHDOG=1");
        let every = interval / 3;
        info!(?interval, "systemd's watchdog is on");
        let _ = state.az.lh.insert_source(Timer::from_duration(every), move |_, _, _| {
            crate::session::sd_notify("WATCHDOG=1");
            TimeoutAction::ToDuration(every)
        });
    }
    // Capture before systemd's deadline, leaving it time to finish.
    let capture_at = deadline.map(|d| d.mul_f64(0.6).max(STALL_LOG)).unwrap_or(CAPTURE_WITHOUT_SYSTEMD);
    install_handler();
    let _ = std::thread::Builder::new()
        .name("aizome-watchdog".into())
        .stack_size(512 * 1024)
        .spawn(move || watch(capture_at, deadline));
}

fn watch(capture_at: Duration, deadline: Option<Duration>) {
    let mut stalled_since: Option<u64> = None;
    let mut captured = false;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let beat = BEAT.load(Ordering::Relaxed);
        let now = now_ms();
        let quiet = Duration::from_millis(now.saturating_sub(beat));
        match stalled_since {
            None if quiet >= STALL_LOG => {
                stalled_since = Some(beat);
                captured = false;
                warn!(for_ = ?quiet, "the main loop is stalled");
            }
            Some(since) if beat != since => {
                let lasted = Duration::from_millis(beat.saturating_sub(since));
                warn!(?lasted, "the main loop is running again");
                if captured {
                    record(&format!("recovered after {:.1} s\n", lasted.as_secs_f64()));
                    let _ = std::fs::remove_file(marker_path());
                }
                stalled_since = None;
            }
            Some(_) if !captured && quiet >= capture_at => {
                captured = true;
                let report = capture(quiet, deadline);
                error!("{report}");
                record(&report);
                mark();
            }
            _ => {}
        }
    }
}

/// Where the main thread is: its backtrace (from a signal handler on it)
/// and what the kernel says it waits on.
fn capture(quiet: Duration, deadline: Option<Duration>) -> String {
    let pid = std::process::id() as i32;
    // The main thread's id is the process's.
    let tid = pid;
    let wchan = std::fs::read_to_string(format!("/proc/self/task/{tid}/wchan")).unwrap_or_default();
    let syscall = std::fs::read_to_string(format!("/proc/self/task/{tid}/syscall")).unwrap_or_default();
    let stat = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).unwrap_or_default();
    let state = stat.rsplit(')').next().and_then(|s| s.split_whitespace().next()).unwrap_or("?").to_string();
    CAPTURED.store(false, Ordering::SeqCst);
    // Safety: tgkill to our own main thread with a signal we handle.
    unsafe {
        libc::syscall(libc::SYS_tgkill, pid, tid, signal());
    }
    let waited = Instant::now();
    while !CAPTURED.load(Ordering::SeqCst) && waited.elapsed() < Duration::from_millis(800) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let bt = BACKTRACE.lock().ok().and_then(|mut b| b.take());
    let trace = match bt {
        Some(bt) => format!("{bt}"),
        None => "(the main thread didn't answer the signal; it may be stuck in the kernel)\n".to_string(),
    };
    let killed = match deadline {
        Some(d) => format!("systemd stops Aizome at {:.0} s and starts it again", d.as_secs_f64()),
        None => "no watchdog is watching (not under systemd)".to_string(),
    };
    format!(
        "Aizome's main loop has been stalled for {:.1} s ({killed}).\n\
         main thread: state {state}, waiting in {}, syscall {}\n\
         backtrace:\n{trace}",
        quiet.as_secs_f64(),
        if wchan.trim().is_empty() { "?" } else { wchan.trim() },
        syscall.trim(),
    )
}

fn signal() -> libc::c_int {
    libc::SIGRTMIN() + 3
}

extern "C" fn on_signal(_: libc::c_int) {
    // On the main thread, interrupted wherever it is. Taking a backtrace
    // allocates, which isn't strictly safe here; the thread is stuck and
    // about to be killed, and the watcher only waits a moment for it.
    if let Ok(mut slot) = BACKTRACE.try_lock() {
        *slot = Some(std::backtrace::Backtrace::force_capture());
    }
    CAPTURED.store(true, Ordering::SeqCst);
}

fn install_handler() {
    // Safety: a zeroed sigaction with a handler of the right signature.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_signal as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(signal(), &sa, std::ptr::null_mut());
    }
}

fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| aizome_config::home_dir().join(".local/state"));
    base.join("aizome")
}

pub fn log_path() -> PathBuf {
    state_dir().join("stall.log")
}

fn marker_path() -> PathBuf {
    state_dir().join("stalled")
}

fn record(text: &str) {
    let _ = std::fs::create_dir_all(state_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = writeln!(f, "---- {} (pid {})\n{text}", stamp(SystemTime::now()), std::process::id());
    }
}

/// A stall in progress, in case it is the last thing this process does.
fn mark() {
    let _ = std::fs::create_dir_all(state_dir());
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = std::fs::write(marker_path(), format!("{secs}\n"));
}

/// If the last compositor was stopped mid-stall (recently), what to tell
/// the person; the marker is used up.
pub fn previous_stall() -> Option<String> {
    let path = marker_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let secs: u64 = text.trim().parse().ok()?;
    let when = UNIX_EPOCH + Duration::from_secs(secs);
    if SystemTime::now().duration_since(when).unwrap_or(Duration::MAX) > NOTICE_WITHIN {
        return None;
    }
    let log = log_path();
    let home = aizome_config::home_dir();
    let shown = match log.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => log.display().to_string(),
    };
    Some(format!(
        "Aizome stopped responding at {} and was restarted; apps that were open had to close.\nWhat it was doing: {shown}",
        hh_mm(when),
    ))
}

fn local(t: SystemTime) -> libc::tm {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
    // Safety: localtime_r writes only into `tm`.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    }
}

fn hh_mm(t: SystemTime) -> String {
    let tm = local(t);
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

fn stamp(t: SystemTime) -> String {
    let tm = local(t);
    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_signal_captures_this_threads_backtrace() {
        super::install_handler();
        // Safety: raising a handled signal on this thread.
        unsafe {
            libc::raise(super::signal());
        }
        assert!(super::CAPTURED.load(std::sync::atomic::Ordering::SeqCst));
        let bt = super::BACKTRACE.lock().unwrap().take().expect("a backtrace");
        assert!(format!("{bt}").contains("a_signal_captures_this_threads_backtrace"), "{bt}");
    }
}
