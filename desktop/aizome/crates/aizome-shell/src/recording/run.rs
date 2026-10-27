//! A recorder's life, on a thread of its own: once it's known what can
//! record here (`probe`), start the first recorder that keeps running
//! (one that fails, exiting at once or without writing a frame, makes way
//! for the next), stop it cleanly when asked (SIGINT, so the file gets
//! its index; then SIGTERM and SIGKILL if it won't), and say how it
//! ended.
//!
//! Each recorder is watched through a pidfd: nothing polls, and a signal
//! can never reach another process that took a dead recorder's pid. A
//! recording also outlives the shell: the recorder runs in its own
//! process group, and a note in `$XDG_RUNTIME_DIR/aizome/recording.json`
//! lets the shell that's started next take it back ([`adopt`]).

use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A command line to try.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub recorder: &'static str,
    pub argv: Vec<String>,
}

/// How long things may take.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// A recorder that exits sooner than this after starting couldn't
    /// record here: the next one is tried.
    pub settle: Duration,
    /// How long a recorder has to finish its file after SIGINT.
    pub finish: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing { settle: Duration::from_millis(1500), finish: Duration::from_secs(10) }
    }
}

/// What the thread says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A recorder is running (again, after a fallback: the clock restarts).
    Started {
        recorder: String,
        pid: i32,
    },
    /// A recorder couldn't record (the next is tried).
    GaveUp {
        recorder: String,
    },
    Ended(Ending),
}

/// How a recording ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Stopped as asked, the file finished.
    Saved(PathBuf),
    /// Stopped as asked, but the recorder had to be killed: the file may
    /// not play.
    Incomplete(PathBuf),
    /// No recorder could start; the last one's last words.
    Failed(String),
    /// The recorder exited by itself while recording.
    Crashed { path: PathBuf, error: String },
    /// Stopped before any recorder got going.
    Cancelled,
}

enum Cmd {
    Stop,
    /// The watched process exited.
    Exited,
}

/// The shell's hold on a recording.
#[derive(Clone, Debug)]
pub struct Handle {
    tx: Sender<Cmd>,
}

impl Handle {
    /// Stop recording (the thread says `Ended` once the file's done).
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

/// What's noted about a running recording for the next shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub pid: i32,
    /// The process's start time (`/proc/PID/stat` field 22), so a reused
    /// pid isn't taken for it.
    pub start_ticks: u64,
    pub recorder: String,
    pub path: PathBuf,
    /// When it started, seconds since the epoch.
    pub since: u64,
}

/// `$XDG_RUNTIME_DIR/aizome/recording.json`.
pub fn note_path() -> PathBuf {
    aizome_ipc::runtime_dir().join("aizome").join("recording.json")
}

/// Where recorders' messages go: `$XDG_RUNTIME_DIR/aizome/recorder.log`.
pub fn log_path() -> PathBuf {
    aizome_ipc::runtime_dir().join("aizome").join("recorder.log")
}

/// Starts recording into `path`, trying in turn what `attempts` gives
/// (asked until it knows: `None` while the recorders are being probed);
/// `sink` hears what happens (from the recording's thread).
pub fn start(
    mut attempts: impl FnMut() -> Option<Vec<Attempt>> + Send + 'static,
    path: PathBuf,
    note: Option<PathBuf>,
    log: PathBuf,
    timing: Timing,
    sink: impl Fn(Event) + Send + 'static,
) -> Handle {
    let (tx, rx) = channel();
    let handle = Handle { tx: tx.clone() };
    std::thread::Builder::new()
        .name("recording".into())
        .spawn(move || {
            let ending = loop {
                if let Some(attempts) = attempts() {
                    break supervise(&attempts, &path, note.as_deref(), &log, timing, &tx, &rx, &sink);
                }
                if matches!(rx.recv_timeout(Duration::from_millis(50)), Ok(Cmd::Stop)) {
                    break Ending::Cancelled;
                }
            };
            if let Some(n) = &note {
                let _ = std::fs::remove_file(n);
            }
            sink(Event::Ended(ending));
        })
        .expect("a thread");
    handle
}

#[allow(clippy::too_many_arguments)] // the thread's whole world
fn supervise(
    attempts: &[Attempt],
    path: &Path,
    note: Option<&Path>,
    log: &Path,
    timing: Timing,
    tx: &Sender<Cmd>,
    rx: &Receiver<Cmd>,
    sink: &impl Fn(Event),
) -> Ending {
    let mut last_error = String::from("no recorder is installed (gpu-screen-recorder or wf-recorder)");
    for attempt in attempts {
        // A stop that came while the last one failed.
        if matches!(rx.try_recv(), Ok(Cmd::Stop)) {
            return Ending::Cancelled;
        }
        let _ = std::fs::remove_file(path);
        let mut child = match spawn(&attempt.argv, log) {
            Ok(c) => c,
            Err(e) => {
                last_error = format!("{}: {e}", attempt.recorder);
                continue;
            }
        };
        let pid = child.id() as i32;
        let proc = Proc::watch(pid, tx.clone());
        if let Some(n) = note {
            write_note(n, pid, attempt.recorder, path);
        }
        sink(Event::Started { recorder: attempt.recorder.to_owned(), pid });
        match rx.recv_timeout(timing.settle) {
            Ok(Cmd::Exited) => {
                let status = child.wait().ok();
                last_error = format!("{} exited ({}): {}", attempt.recorder, describe(status), log_tail(log));
                log::info!("recording: {last_error}");
                sink(Event::GaveUp { recorder: attempt.recorder.to_owned() });
                continue;
            }
            Ok(Cmd::Stop) => return stop(&mut child, &proc, path, log, timing, rx),
            Err(_) => {}
        }
        // Recording.
        match rx.recv() {
            Ok(Cmd::Stop) | Err(_) => return stop(&mut child, &proc, path, log, timing, rx),
            Ok(Cmd::Exited) => {
                let status = child.wait().ok();
                let error = format!("{} exited ({}): {}", attempt.recorder, describe(status), log_tail(log));
                // One that wrote nothing never got going, however long it
                // took to give up (gpu-screen-recorder finding only
                // software OpenGL takes seconds in a VM): the next one.
                if !recorded(path) {
                    log::info!("recording: {error}");
                    last_error = error;
                    sink(Event::GaveUp { recorder: attempt.recorder.to_owned() });
                    continue;
                }
                log::warn!("recording: {error}");
                return Ending::Crashed {
                    path: path.to_owned(),
                    error: format!("{} quit unexpectedly ({}).", attempt.recorder, describe(status)),
                };
            }
        }
    }
    Ending::Failed(last_error)
}

/// Whether a recorder wrote frames, not just a container's header (its
/// muxer writes in 32 KiB blocks).
fn recorded(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() >= 16 * 1024)
}

/// SIGINT, so the recorder finishes the file; SIGTERM, then SIGKILL, if it
/// won't.
fn stop(child: &mut Child, proc: &Proc, path: &Path, log: &Path, timing: Timing, rx: &Receiver<Cmd>) -> Ending {
    proc.signal(libc::SIGINT);
    if !wait_exit(rx, timing.finish) {
        log::warn!("recording: the recorder didn't finish in {:?}; terminating it", timing.finish);
        proc.signal(libc::SIGTERM);
        if !wait_exit(rx, Duration::from_secs(2)) {
            proc.signal(libc::SIGKILL);
            wait_exit(rx, Duration::from_secs(2));
        }
        let _ = child.wait();
        return Ending::Incomplete(path.to_owned());
    }
    let status = child.wait().ok();
    finished(path, status.is_some_and(|s| s.success()), log)
}

/// What a recorder that exited when asked left.
fn finished(path: &Path, clean: bool, log: &Path) -> Ending {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        return Ending::Failed(format!("nothing was recorded: {}", log_tail(log)));
    }
    if clean { Ending::Saved(path.to_owned()) } else { Ending::Incomplete(path.to_owned()) }
}

/// Waits for `Exited`, ignoring further stops.
fn wait_exit(rx: &Receiver<Cmd>, within: Duration) -> bool {
    let deadline = std::time::Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(Cmd::Exited) => return true,
            Ok(Cmd::Stop) => {}
            Err(RecvTimeoutError::Timeout) => return false,
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn spawn(argv: &[String], log: &Path) -> std::io::Result<Child> {
    let (prog, args) = argv.split_first().ok_or_else(|| std::io::Error::other("an empty command"))?;
    if let Some(d) = log.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let stderr = File::create(log).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
    Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        // Its own process group: a ^C meant for whatever started the
        // shell isn't for it, and it outlives a shell that crashes.
        .process_group(0)
        .spawn()
}

/// A process watched through its pidfd (or, without pidfds, a thread
/// waiting on it without reaping it).
struct Proc {
    pid: i32,
    pidfd: Option<OwnedFd>,
}

impl Proc {
    /// Watches `pid`, sending `Exited` when it ends.
    fn watch(pid: i32, tx: Sender<Cmd>) -> Proc {
        let pidfd = pidfd_open(pid);
        let watch_fd = pidfd.as_ref().and_then(|f| f.try_clone().ok());
        std::thread::Builder::new()
            .name("recorder-watch".into())
            .spawn(move || {
                match watch_fd {
                    // Readable once the process has exited.
                    Some(fd) => {
                        let mut p = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                        // SAFETY: one valid pollfd, for as long as `fd` lives.
                        while unsafe { libc::poll(&mut p, 1, -1) } < 0 {}
                    }
                    None => {
                        // Waits without reaping: the supervisor reaps.
                        // SAFETY: plain syscall on our child's pid.
                        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                        unsafe {
                            libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT)
                        };
                    }
                }
                let _ = tx.send(Cmd::Exited);
            })
            .expect("a thread");
        Proc { pid, pidfd }
    }

    fn signal(&self, sig: i32) {
        match &self.pidfd {
            // SAFETY: pidfd_send_signal on our own pidfd.
            Some(fd) => unsafe {
                libc::syscall(libc::SYS_pidfd_send_signal, fd.as_raw_fd(), sig, std::ptr::null::<libc::siginfo_t>(), 0);
            },
            // SAFETY: the pid is our unreaped child (the supervisor reaps
            // only after it exits), so it can't be anyone else's.
            None => unsafe {
                libc::kill(self.pid, sig);
            },
        }
    }
}

fn pidfd_open(pid: i32) -> Option<OwnedFd> {
    // SAFETY: pidfd_open returns a new descriptor we then own, or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as RawFd;
    // SAFETY: a fresh descriptor, owned by nothing else.
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
}

fn describe(status: Option<std::process::ExitStatus>) -> String {
    use std::os::unix::process::ExitStatusExt;
    match status {
        Some(s) => match (s.code(), s.signal()) {
            (Some(c), _) => format!("status {c}"),
            (_, Some(sig)) => format!("signal {sig}"),
            _ => "?".into(),
        },
        None => "?".into(),
    }
}

/// The last few lines the recorder wrote, for the notification.
pub fn log_tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tail = &lines[lines.len().saturating_sub(3)..];
    let s = tail.join(" · ");
    if s.chars().count() > 300 { s.chars().take(300).collect::<String>() + "…" } else { s }
}

/// A process's start time in clock ticks (`/proc/PID/stat` field 22).
fn start_ticks(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command can hold spaces and parentheses: fields follow the last ')'.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

fn write_note(path: &Path, pid: i32, recorder: &str, file: &Path) {
    let Some(start_ticks) = start_ticks(pid) else { return };
    let note = Note {
        pid,
        start_ticks,
        recorder: recorder.to_owned(),
        path: file.to_owned(),
        since: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(json) = serde_json::to_string(&note) {
        let _ = std::fs::write(path, json);
    }
}

/// A recording an earlier shell started, if it's still going: the note,
/// checked against the process (the same start time, so not a reused
/// pid).
pub fn running(note: &Path) -> Option<Note> {
    let n: Note = serde_json::from_str(&std::fs::read_to_string(note).ok()?).ok()?;
    if start_ticks(n.pid) == Some(n.start_ticks) {
        Some(n)
    } else {
        let _ = std::fs::remove_file(note);
        None
    }
}

/// Takes back a recording an earlier shell started: stops it when asked
/// and says how it ended, as [`start`] does (its exit status is its
/// parent's, so only the file tells).
pub fn adopt(n: Note, note: PathBuf, log: PathBuf, timing: Timing, sink: impl Fn(Event) + Send + 'static) -> Handle {
    let (tx, rx) = channel();
    let handle = Handle { tx: tx.clone() };
    std::thread::Builder::new()
        .name("recording".into())
        .spawn(move || {
            let ending = match pidfd_open(n.pid) {
                None => Ending::Crashed { path: n.path.clone(), error: "The recorder is gone.".into() },
                Some(fd) => {
                    // The pidfd pins the process: check it's still the one noted.
                    if start_ticks(n.pid) != Some(n.start_ticks) {
                        Ending::Crashed { path: n.path.clone(), error: "The recorder is gone.".into() }
                    } else {
                        let proc = Proc::watch_fd(n.pid, fd, tx.clone());
                        match rx.recv() {
                            Ok(Cmd::Stop) | Err(_) => {
                                proc.signal(libc::SIGINT);
                                if !wait_exit(&rx, timing.finish) {
                                    proc.signal(libc::SIGTERM);
                                    if !wait_exit(&rx, Duration::from_secs(2)) {
                                        proc.signal(libc::SIGKILL);
                                    }
                                    Ending::Incomplete(n.path.clone())
                                } else {
                                    finished(&n.path, true, &log)
                                }
                            }
                            Ok(Cmd::Exited) => Ending::Crashed {
                                path: n.path.clone(),
                                error: format!("{} quit unexpectedly.", n.recorder),
                            },
                        }
                    }
                }
            };
            let _ = std::fs::remove_file(&note);
            sink(Event::Ended(ending));
        })
        .expect("a thread");
    handle
}

impl Proc {
    fn watch_fd(pid: i32, fd: OwnedFd, tx: Sender<Cmd>) -> Proc {
        let watch = fd.try_clone().ok();
        std::thread::Builder::new()
            .name("recorder-watch".into())
            .spawn(move || {
                if let Some(w) = watch {
                    let mut p = libc::pollfd { fd: w.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                    // SAFETY: one valid pollfd, for as long as `w` lives.
                    while unsafe { libc::poll(&mut p, 1, -1) } < 0 {}
                }
                let _ = tx.send(Cmd::Exited);
            })
            .expect("a thread");
        Proc { pid, pidfd: Some(fd) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aizome-rec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A pretend recorder: writes to its file until SIGINT or SIGTERM, then
    /// exits 0 (as gpu-screen-recorder and wf-recorder do).
    fn recorder(path: &Path) -> Attempt {
        let script = format!(
            "trap 'echo done >> {p}; exit 0' INT TERM; echo start > {p}; while :; do sleep 0.02; done",
            p = path.display()
        );
        Attempt { recorder: "pretend", argv: vec!["sh".into(), "-c".into(), script] }
    }

    fn failing() -> Attempt {
        Attempt { recorder: "failing", argv: vec!["sh".into(), "-c".into(), "echo no GPU here >&2; exit 3".into()] }
    }

    const FAST: Timing = Timing { settle: Duration::from_millis(300), finish: Duration::from_millis(800) };

    fn run(attempts: Vec<Attempt>, d: &Path, path: &Path) -> (Handle, Receiver<Event>) {
        let (tx, rx) = channel();
        let h = start(
            move || Some(attempts.clone()),
            path.to_owned(),
            Some(d.join("note.json")),
            d.join("log"),
            FAST,
            move |e| {
                let _ = tx.send(e);
            },
        );
        (h, rx)
    }

    fn next(rx: &Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(10)).expect("an event")
    }

    #[test]
    fn a_recorder_that_fails_makes_way_and_a_stop_saves_the_file() {
        let d = dir("fallback");
        let path = d.join("a.mp4");
        let (h, rx) = run(vec![failing(), recorder(&path)], &d, &path);
        assert!(matches!(next(&rx), Event::Started { recorder, .. } if recorder == "failing"));
        assert_eq!(next(&rx), Event::GaveUp { recorder: "failing".into() });
        assert!(matches!(next(&rx), Event::Started { recorder, .. } if recorder == "pretend"));
        std::thread::sleep(Duration::from_millis(500));
        // The next shell would find it.
        let n = running(&d.join("note.json")).expect("noted");
        assert_eq!(n.path, path);
        h.stop();
        assert_eq!(next(&rx), Event::Ended(Ending::Saved(path.clone())));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "start\ndone\n");
        assert!(!d.join("note.json").exists());
    }

    #[test]
    fn nothing_that_can_record_fails_with_its_words() {
        let d = dir("failed");
        let path = d.join("a.mp4");
        let (_h, rx) = run(vec![failing()], &d, &path);
        next(&rx);
        assert_eq!(next(&rx), Event::GaveUp { recorder: "failing".into() });
        match next(&rx) {
            Event::Ended(Ending::Failed(e)) => assert!(e.contains("no GPU here"), "{e}"),
            e => panic!("{e:?}"),
        }
        let (_h, rx) = run(vec![], &d, &path);
        assert!(matches!(next(&rx), Event::Ended(Ending::Failed(_))));
    }

    #[test]
    fn a_recorder_that_dies_while_recording_is_a_crash() {
        let d = dir("crash");
        let path = d.join("a.mp4");
        let dies = Attempt {
            recorder: "dies",
            argv: vec![
                "sh".into(),
                "-c".into(),
                format!("head -c 20000 /dev/zero > {}; sleep 0.6; exit 9", path.display()),
            ],
        };
        let (_h, rx) = run(vec![dies], &d, &path);
        next(&rx);
        match next(&rx) {
            Event::Ended(Ending::Crashed { path: p, error }) => {
                assert_eq!(p, path);
                assert!(error.contains("dies quit unexpectedly (status 9)"), "{error}");
            }
            e => panic!("{e:?}"),
        }
    }

    /// gpu-screen-recorder in a VM takes seconds to find it has only
    /// software OpenGL: past the settle time, but it wrote nothing.
    #[test]
    fn one_that_gives_up_slowly_without_writing_makes_way_too() {
        let d = dir("slow");
        let path = d.join("a.mp4");
        let slow = Attempt {
            recorder: "slow",
            argv: vec!["sh".into(), "-c".into(), format!("echo hdr > {}; sleep 0.6; exit 1", path.display())],
        };
        let (h, rx) = run(vec![slow, recorder(&path)], &d, &path);
        assert!(matches!(next(&rx), Event::Started { recorder, .. } if recorder == "slow"));
        assert_eq!(next(&rx), Event::GaveUp { recorder: "slow".into() });
        assert!(matches!(next(&rx), Event::Started { recorder, .. } if recorder == "pretend"));
        std::thread::sleep(Duration::from_millis(400));
        h.stop();
        assert_eq!(next(&rx), Event::Ended(Ending::Saved(path)));
    }

    /// Nothing starts until the probe has said what can record, and a
    /// stop while it hasn't yet cancels.
    #[test]
    fn a_recording_waits_for_the_probe() {
        let d = dir("probe");
        let path = d.join("a.mp4");
        let known = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let attempts = {
            let (known, path) = (known.clone(), path.clone());
            move || known.load(std::sync::atomic::Ordering::SeqCst).then(|| vec![recorder(&path)])
        };
        let (tx, rx) = channel();
        let h = start(attempts, path.clone(), None, d.join("log"), FAST, move |e| {
            let _ = tx.send(e);
        });
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "started before the probe");
        known.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(next(&rx), Event::Started { recorder, .. } if recorder == "pretend"));
        std::thread::sleep(Duration::from_millis(400));
        h.stop();
        assert_eq!(next(&rx), Event::Ended(Ending::Saved(path.clone())));
        let (tx, rx) = channel();
        let h = start(
            || None,
            path,
            None,
            d.join("log"),
            FAST,
            move |e| {
                let _ = tx.send(e);
            },
        );
        h.stop();
        assert_eq!(next(&rx), Event::Ended(Ending::Cancelled));
    }

    #[test]
    fn a_recorder_that_wont_stop_is_killed() {
        let d = dir("stubborn");
        let path = d.join("a.mp4");
        let stubborn = Attempt {
            recorder: "stubborn",
            argv: vec![
                "sh".into(),
                "-c".into(),
                format!("trap '' INT TERM; echo x > {}; while :; do sleep 0.02; done", path.display()),
            ],
        };
        let (h, rx) = run(vec![stubborn], &d, &path);
        let pid = match next(&rx) {
            Event::Started { pid, .. } => pid,
            e => panic!("{e:?}"),
        };
        std::thread::sleep(Duration::from_millis(400));
        h.stop();
        assert_eq!(next(&rx), Event::Ended(Ending::Incomplete(path)));
        assert!(start_ticks(pid).is_none(), "the recorder is gone");
    }

    #[test]
    fn a_stop_while_starting_still_saves() {
        let d = dir("early");
        let path = d.join("a.mp4");
        let (h, rx) = run(vec![recorder(&path)], &d, &path);
        next(&rx);
        std::thread::sleep(Duration::from_millis(50));
        h.stop();
        assert_eq!(next(&rx), Event::Ended(Ending::Saved(path)));
    }

    #[test]
    fn a_later_shell_takes_a_recording_back() {
        let d = dir("adopt");
        let path = d.join("a.mp4");
        // Started by "the first shell": its thread is forgotten.
        let (_first, rx) = run(vec![recorder(&path)], &d, &path);
        next(&rx);
        std::thread::sleep(Duration::from_millis(400));
        let n = running(&d.join("note.json")).expect("noted");
        let (tx, rx2) = channel();
        let h = adopt(n, d.join("note.json"), d.join("log"), FAST, move |e| {
            let _ = tx.send(e);
        });
        h.stop();
        assert_eq!(rx2.recv_timeout(Duration::from_secs(10)).unwrap(), Event::Ended(Ending::Saved(path.clone())));
        // A note about a process that's gone is dropped.
        let stale = Note { pid: i32::MAX - 1, start_ticks: 1, recorder: "x".into(), path, since: 0 };
        std::fs::write(d.join("stale.json"), serde_json::to_string(&stale).unwrap()).unwrap();
        assert!(running(&d.join("stale.json")).is_none());
        assert!(!d.join("stale.json").exists());
    }
}
