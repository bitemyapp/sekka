//! What the recovery overlay knows about apps' processes, and how Force
//! Quit kills them.
//!
//! Everything here touches `/proc` and the cgroup tree, which can be slow
//! or block outright (a process stuck in the kernel, a machine swapping),
//! so it runs on a thread of its own: the compositor sends [`Request`]s
//! down a channel and gets [`Report`]s back through the event loop. The
//! main loop never reads `/proc` itself.
//!
//! An app's processes are its windows' clients (Wayland credentials). When
//! a process lives in an app's own cgroup (a systemd scope such as
//! `app-aizome-firefox-….scope`, or an `app-*.service`), the whole cgroup
//! is the app: its CPU and memory are the cgroup's (`cpu.stat`,
//! `memory.current`) and Force Quit kills all of it (`cgroup.kill`).
//! Otherwise, or when the cgroup also holds the compositor (programs it
//! started itself live in `aizome.service`), only that process counts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::channel;

use crate::state::State;

/// One app: a key (its app ID) and the processes behind its windows.
#[derive(Clone, Debug)]
pub struct AppProcs {
    pub key: String,
    pub pids: Vec<i32>,
    /// xwayland-satellite, when it runs: its process (the owner of every
    /// X11 app's windows) and its X display.
    pub x11: Option<X11>,
}

#[derive(Clone, Debug)]
pub struct X11 {
    pub satellite: i32,
    pub display: String,
}

impl AppProcs {
    /// Its processes, with satellite's replaced by the X clients' own
    /// when the X server names them (else satellite's stays: the fallback
    /// that stops every X11 app).
    fn resolved(&self) -> AppProcs {
        let mut pids = self.pids.clone();
        if let Some(x) = &self.x11
            && pids.contains(&x.satellite)
        {
            let clients = super::x11::pids_for(&x.display, &self.key);
            if !clients.is_empty() {
                pids.retain(|p| *p != x.satellite);
                for c in clients {
                    if !pids.contains(&c) {
                        pids.push(c);
                    }
                }
            }
        }
        AppProcs { key: self.key.clone(), pids, x11: None }
    }
}

pub enum Request {
    /// Measure these apps (CPU over the time since the last sample).
    Sample(Vec<AppProcs>),
    /// Kill an app: its cgroups where it has its own, else its processes.
    Kill(AppProcs),
}

/// Where an app's processes live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unit {
    /// The app's own cgroup (a scope or an app service).
    Cgroup(PathBuf),
    /// A process on its own (no cgroup of the app's own).
    Pid(i32),
}

impl Unit {
    pub fn describe(&self) -> String {
        match self {
            Unit::Cgroup(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            Unit::Pid(p) => format!("process {p}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub key: String,
    pub units: Vec<Unit>,
    /// Percent of one core over the last interval.
    pub cpu_percent: Option<f64>,
    pub memory: Option<u64>,
    /// Every process behind its windows is gone, a zombie, or dumping
    /// core.
    pub crashed: bool,
    /// Stopped (SIGSTOP, a debugger).
    pub stopped: bool,
}

pub enum Report {
    Samples(Vec<Sample>),
    Killed { key: String, result: Result<String, String> },
}

/// The thread's handle.
pub struct ProcInfo {
    tx: mpsc::Sender<Request>,
}

impl ProcInfo {
    /// Starts the thread; its reports go to `on_report` on the event loop.
    pub fn start(lh: &LoopHandle<'static, State>, on_report: fn(&mut State, Report)) -> Option<ProcInfo> {
        let (tx, rx) = mpsc::channel::<Request>();
        let (report_tx, report_rx) = channel::channel::<Report>();
        lh.insert_source(report_rx, move |event, _, state: &mut State| {
            if let channel::Event::Msg(r) = event {
                on_report(state, r);
            }
        })
        .ok()?;
        std::thread::Builder::new()
            .name("aizome-procinfo".into())
            .stack_size(256 * 1024)
            .spawn(move || run(rx, report_tx))
            .ok()?;
        Some(ProcInfo { tx })
    }

    pub fn send(&self, r: Request) {
        let _ = self.tx.send(r);
    }
}

fn run(rx: mpsc::Receiver<Request>, tx: channel::Sender<Report>) {
    let mut meter = Meter::new();
    while let Ok(req) = rx.recv() {
        // Only the newest sampling request matters.
        let mut req = req;
        while let Ok(next) = rx.try_recv() {
            match next {
                Request::Kill(k) => {
                    let key = k.key.clone();
                    let result = kill(&k, &meter.own_cgroup);
                    let _ = tx.send(Report::Killed { key, result });
                }
                s => req = s,
            }
        }
        let report = match req {
            Request::Sample(apps) => Report::Samples(meter.sample(&apps)),
            Request::Kill(k) => {
                let key = k.key.clone();
                let result = kill(&k, &meter.own_cgroup);
                Report::Killed { key, result }
            }
        };
        if tx.send(report).is_err() {
            return;
        }
    }
}

/// CPU accounting between samples.
struct Meter {
    own_cgroup: Option<String>,
    /// Per app: CPU time used (µs) and when it was read.
    last: HashMap<String, (u64, Instant)>,
    tick_us: u64,
}

impl Meter {
    fn new() -> Meter {
        // Safety: sysconf has no preconditions.
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        Meter {
            own_cgroup: cgroup_of(std::process::id() as i32),
            last: HashMap::new(),
            tick_us: if ticks > 0 { 1_000_000 / ticks as u64 } else { 10_000 },
        }
    }

    fn sample(&mut self, apps: &[AppProcs]) -> Vec<Sample> {
        // A first look at an app needs a second reading for its CPU use:
        // take one now and the other a moment later.
        let fresh = apps.iter().any(|a| !self.last.contains_key(&a.key));
        let mut out = self.read(apps);
        if fresh {
            std::thread::sleep(Duration::from_millis(250));
            out = self.read(apps);
        }
        self.last.retain(|k, _| apps.iter().any(|a| &a.key == k));
        out
    }

    fn read(&mut self, apps: &[AppProcs]) -> Vec<Sample> {
        let now = Instant::now();
        let mut out = Vec::new();
        for app in apps {
            let app = &app.resolved();
            let units = units_of(&app.key, &app.pids, self.own_cgroup.as_deref());
            let mut cpu_us = 0u64;
            let mut memory = 0u64;
            let mut measured = false;
            for u in &units {
                match u {
                    Unit::Cgroup(p) => {
                        if let Some(c) = cgroup_cpu_us(p) {
                            cpu_us += c;
                            measured = true;
                        }
                        memory += cgroup_memory(p).unwrap_or(0);
                    }
                    Unit::Pid(pid) => {
                        if let Some(st) = read_stat(*pid) {
                            cpu_us += (st.utime + st.stime) * self.tick_us;
                            measured = true;
                        }
                        memory += pid_rss(*pid).unwrap_or(0);
                    }
                }
            }
            let states: Vec<ProcState> = app.pids.iter().map(|p| proc_state(*p)).collect();
            let crashed =
                !states.is_empty() && states.iter().all(|s| matches!(s, ProcState::Gone | ProcState::Crashed));
            let stopped = states.iter().any(|s| matches!(s, ProcState::Stopped));
            let cpu_percent = match self.last.get(&app.key) {
                Some((prev, at)) if measured && cpu_us >= *prev => {
                    let wall = now.duration_since(*at).as_micros().max(1) as f64;
                    Some((cpu_us - prev) as f64 / wall * 100.0)
                }
                _ => None,
            };
            if measured {
                self.last.insert(app.key.clone(), (cpu_us, now));
            }
            out.push(Sample {
                key: app.key.clone(),
                units,
                cpu_percent,
                memory: measured.then_some(memory),
                crashed,
                stopped,
            });
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcState {
    Running,
    Stopped,
    /// A zombie, dead, or dumping core.
    Crashed,
    Gone,
}

fn proc_state(pid: i32) -> ProcState {
    let Some(st) = read_stat(pid) else { return ProcState::Gone };
    match st.state {
        'Z' | 'X' | 'x' => return ProcState::Crashed,
        'T' | 't' => return ProcState::Stopped,
        _ => {}
    }
    let dumping = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .is_some_and(|s| s.lines().any(|l| l.starts_with("CoreDumping:") && l.trim_end().ends_with('1')));
    if dumping { ProcState::Crashed } else { ProcState::Running }
}

struct Stat {
    state: char,
    utime: u64,
    stime: u64,
}

fn read_stat(pid: i32) -> Option<Stat> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat(&s)
}

/// `/proc/PID/stat`: the command is in parentheses and may hold spaces
/// and parentheses itself, so fields are counted from the last `)`.
fn parse_stat(s: &str) -> Option<Stat> {
    let rest = &s[s.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // After the command: state (3), … utime (14), stime (15).
    Some(Stat { state: f.first()?.chars().next()?, utime: f.get(11)?.parse().ok()?, stime: f.get(12)?.parse().ok()? })
}

fn pid_rss(pid: i32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    // Safety: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
    Some(pages * page)
}

/// A process's cgroup (v2), as a path under the cgroup root.
fn cgroup_of(pid: i32) -> Option<String> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    s.lines().find_map(|l| l.strip_prefix("0::")).map(|p| p.trim().to_string())
}

const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// Whether a cgroup is this app's own: a scope or an `app-*.service`
/// named for it (`app-aizome-org.example.App-….scope`), not the
/// compositor's (nor one holding it). A program started from a terminal
/// lives in the terminal's scope, which isn't its own: then only its
/// process counts.
pub fn is_app_cgroup(path: &str, app_id: &str, own: Option<&str>) -> bool {
    let Some(leaf) = path.rsplit('/').next() else { return false };
    if !names_app(leaf, app_id) {
        return false;
    }
    let app_unit = leaf.ends_with(".scope") || (leaf.starts_with("app-") && leaf.ends_with(".service"));
    // Never a session scope (login sessions are `session-N.scope`), nor
    // the user manager's own `init.scope`.
    let session = leaf.starts_with("session-") || leaf == "init.scope";
    let ours = own.is_some_and(|o| o == path || o.starts_with(&format!("{path}/")));
    app_unit && !session && !ours && path.contains("/user@")
}

/// A unit name names an app: its ID as systemd escapes it, or the ID's
/// last part (`org.mozilla.firefox` → `firefox`), ignoring case.
fn names_app(leaf: &str, app_id: &str) -> bool {
    if app_id.is_empty() {
        return false;
    }
    let leaf = leaf.to_lowercase();
    let id = app_id.to_lowercase();
    let escaped = id.replace('-', "\\x2d");
    let last = id.rsplit('.').next().unwrap_or(&id).replace('-', "\\x2d");
    let word = |w: &str| {
        // Bounded by `-` or `.` on both sides, as in `app-aizome-foot-1f.scope`.
        leaf.match_indices(w).any(|(i, _)| {
            let before = leaf[..i].chars().last();
            let after = leaf[i + w.len()..].chars().next();
            matches!(before, Some('-' | '.')) && matches!(after, Some('-' | '.' | '@'))
        })
    };
    word(&escaped) || (last.len() >= 3 && word(&last))
}

fn units_of(app_id: &str, pids: &[i32], own: Option<&str>) -> Vec<Unit> {
    let mut units = Vec::new();
    for pid in pids {
        let unit = match cgroup_of(*pid) {
            Some(cg) if is_app_cgroup(&cg, app_id, own) => Unit::Cgroup(PathBuf::from(&cg)),
            _ => Unit::Pid(*pid),
        };
        if !units.contains(&unit) {
            units.push(unit);
        }
    }
    units
}

fn cgroup_dir(p: &Path) -> PathBuf {
    Path::new(CGROUP_ROOT).join(p.strip_prefix("/").unwrap_or(p))
}

fn cgroup_cpu_us(p: &Path) -> Option<u64> {
    let s = std::fs::read_to_string(cgroup_dir(p).join("cpu.stat")).ok()?;
    s.lines().find_map(|l| l.strip_prefix("usage_usec ")).and_then(|v| v.trim().parse().ok())
}

fn cgroup_memory(p: &Path) -> Option<u64> {
    std::fs::read_to_string(cgroup_dir(p).join("memory.current")).ok()?.trim().parse().ok()
}

/// Processes Force Quit never signals: init, the compositor and its
/// parent (the session or the user's systemd).
fn protected(pid: i32) -> bool {
    // Safety: getppid has no preconditions.
    let parent = unsafe { libc::getppid() };
    pid <= 1 || pid == std::process::id() as i32 || pid == parent
}

/// SIGKILL to an app: each cgroup of its own at once (`cgroup.kill`, or
/// each process in it), and processes without one by PID.
fn kill(app: &AppProcs, own: &Option<String>) -> Result<String, String> {
    let app = &app.resolved();
    let units = units_of(&app.key, &app.pids, own.as_deref());
    if units.is_empty() {
        return Err(format!("{} has no processes to stop", app.key));
    }
    let mut done = Vec::new();
    let mut errors = Vec::new();
    for u in &units {
        match u {
            Unit::Cgroup(p) => match kill_cgroup(p) {
                Ok(()) => done.push(u.describe()),
                Err(e) => errors.push(format!("{}: {e}", u.describe())),
            },
            Unit::Pid(pid) => {
                if protected(*pid) {
                    errors.push(format!("process {pid} is Aizome's own"));
                    continue;
                }
                // Safety: kill with a valid signal number.
                if unsafe { libc::kill(*pid, libc::SIGKILL) } == 0 {
                    done.push(u.describe());
                } else {
                    errors.push(format!("process {pid}: {}", std::io::Error::last_os_error()));
                }
            }
        }
    }
    tracing::info!(app = app.key, killed = ?done, errors = ?errors, "force quit");
    if done.is_empty() { Err(errors.join("; ")) } else { Ok(done.join(", ")) }
}

fn kill_cgroup(p: &Path) -> Result<(), String> {
    let dir = cgroup_dir(p);
    // Linux 5.14+: the whole cgroup at once, including processes forked
    // meanwhile.
    if std::fs::write(dir.join("cgroup.kill"), "1").is_ok() {
        return Ok(());
    }
    let procs = std::fs::read_to_string(dir.join("cgroup.procs")).map_err(|e| e.to_string())?;
    let mut any = false;
    for pid in procs.lines().filter_map(|l| l.trim().parse::<i32>().ok()) {
        if protected(pid) {
            continue;
        }
        // Safety: kill with a valid signal number.
        if unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
            any = true;
        }
    }
    if any { Ok(()) } else { Err("no process in it could be stopped".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_with_odd_command_names() {
        let s = "1234 (Web Content (x)) S 1 2 3 0 -1 4194304 100 0 0 0 250 75 0 0 20 0 30 0 100 0 0";
        let st = parse_stat(s).unwrap();
        assert_eq!(st.state, 'S');
        assert_eq!((st.utime, st.stime), (250, 75));
    }

    #[test]
    fn which_cgroups_are_an_apps_own() {
        const OWN: &str = "/user.slice/user-1000.slice/user@1000.service/session.slice/aizome.service";
        let own = Some(OWN);
        let app = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-aizome-foot-0123abcd.scope";
        assert!(is_app_cgroup(app, "foot", own));
        // Firefox started from that terminal is in its scope, which isn't
        // Firefox's.
        assert!(!is_app_cgroup(app, "org.mozilla.firefox", own));
        let escaped = r"/user.slice/user-1000.slice/user@1000.service/app.slice/app-aizome-org.foo\x2dbar-ff.scope";
        assert!(is_app_cgroup(escaped, "org.foo-bar", own));
        let svc = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.example.Thing@x.service";
        assert!(is_app_cgroup(svc, "org.example.Thing", own));
        // The compositor's own service, and anything above it.
        assert!(!is_app_cgroup(OWN, "aizome", own));
        assert!(!is_app_cgroup("/user.slice/user-1000.slice/user@1000.service/session.slice", "session", own));
        // A login session and the user manager itself.
        assert!(!is_app_cgroup("/user.slice/user-1000.slice/session-3.scope", "session", own));
        assert!(!is_app_cgroup("/user.slice/user-1000.slice/user@1000.service/init.scope", "init", own));
        // Other services (pipewire, the portals) aren't apps.
        let pw = "/user.slice/user-1000.slice/user@1000.service/session.slice/pipewire.service";
        assert!(!is_app_cgroup(pw, "pipewire", own));
    }

    #[test]
    fn this_process_is_measured_and_protected() {
        let me = std::process::id() as i32;
        assert!(protected(me));
        assert!(protected(1));
        let mut m = Meter::new();
        let s = m.sample(&[AppProcs { key: "me".into(), pids: vec![me], x11: None }]);
        assert_eq!(s.len(), 1);
        assert!(s[0].memory.is_some_and(|m| m > 0));
        assert!(s[0].cpu_percent.is_some());
        assert!(!s[0].crashed);
        // A process that doesn't exist counts as crashed.
        let s = m.sample(&[AppProcs { key: "gone".into(), pids: vec![i32::MAX - 7], x11: None }]);
        assert!(s[0].crashed);
    }
}
