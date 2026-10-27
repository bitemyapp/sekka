//! Processes, from `/proc/PID/*`.
//!
//! [`ProcessSampler::sample`] lists every process it can see (all of them,
//! unless `/proc` is mounted with `hidepid=`). For each process it reads:
//!
//! - **every sample**, `/proc/PID/stat`: parent, name (`comm`), state, CPU
//!   time, nice, thread count, start time and resident memory (`rss` pages
//!   × the page size, the same number as `statm`'s and `status`'s
//!   `VmRSS`). That one small read per process keeps a sample of ~500
//!   processes to a few milliseconds.
//! - **once per process**, and cached: `cmdline`, `exe`, `status` (for the
//!   real uid) and `cgroup`, plus the user name for the uid (from
//!   `getpwuid_r`, cached per uid for the sampler's lifetime).
//!
//! The cache is keyed by pid and valid only while the process's start time
//! (clock ticks since boot) and `comm` stay the same: a reused pid belongs
//! to a process with a later start time, and a process that `exec`s keeps
//! its pid and start time but gets the new program's `comm`. Processes are
//! also re-read on every sample while they are younger than five seconds,
//! because new processes typically `exec` and get moved into their own
//! systemd scope by their launcher just after they start. A process that
//! later changes its uid, rewrites its command line or moves to another
//! cgroup keeps showing the earlier values.
//!
//! # CPU use
//!
//! [`Process::cpu_percent`] follows `top`: CPU time used over the interval
//! as a percentage of *one* CPU, so a process busy on four cores shows 400.
//! [`Process::machine_cpu_percent`] divides that by the number of online
//! CPUs, giving a 0–100 share of the whole machine. The first sample after
//! [`ProcessSampler::new`] has no interval and reports 0 for every process;
//! after that, a process that started since the previous sample counts all
//! the CPU time it has used, over the whole interval (so the processes'
//! shares add up to the machine's use).
//!
//! # systemd units
//!
//! The cgroup path is the `0::` line of `/proc/PID/cgroup` (cgroup v2; on
//! hybrid systems the `name=systemd` line). The unit is the last component
//! of that path ending in `.service` or `.scope`, such as
//! `app-aizome-foot-1234.scope` or `pipewire.service`, and it belongs to a
//! user's manager when it sits below `user@UID.service`.

use super::signal::{Signal, send_checked};
use super::{clock_ticks, online_cpus, page_size, read_into, ticks_to_duration};
use crate::Result;
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// `PF_KTHREAD` in `/proc/PID/stat`'s flags.
const PF_KTHREAD: u64 = 0x0020_0000;

/// How long after it starts a process's command line, executable, uid and
/// cgroup are re-read on every sample.
const FRESH_FOR: Duration = Duration::from_secs(5);

/// What a process is doing: the state letter in `/proc/PID/stat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessState {
    /// `R`: running or ready to run.
    Running,
    /// `S`: sleeping until something happens (most processes, most of the
    /// time).
    Sleeping,
    /// `D`: waiting uninterruptibly, usually for disk or network I/O.
    DiskSleep,
    /// `T`: stopped by a signal (`SIGSTOP`, Ctrl+Z).
    Stopped,
    /// `t`: stopped by a debugger.
    TracingStop,
    /// `Z`: exited, waiting for its parent to collect it.
    Zombie,
    /// `X`: dead, about to disappear.
    Dead,
    /// `I`: an idle kernel thread.
    Idle,
    /// `P`: a parked kernel thread.
    Parked,
    /// Any other letter.
    Unknown(char),
}

impl ProcessState {
    /// The state for a `/proc/PID/stat` state letter.
    pub fn from_char(letter: char) -> ProcessState {
        match letter {
            'R' => ProcessState::Running,
            'S' => ProcessState::Sleeping,
            'D' => ProcessState::DiskSleep,
            'T' => ProcessState::Stopped,
            't' => ProcessState::TracingStop,
            'Z' => ProcessState::Zombie,
            'X' | 'x' => ProcessState::Dead,
            'I' => ProcessState::Idle,
            'P' => ProcessState::Parked,
            other => ProcessState::Unknown(other),
        }
    }

    /// The state letter, as `ps` shows it.
    pub fn as_char(self) -> char {
        match self {
            ProcessState::Running => 'R',
            ProcessState::Sleeping => 'S',
            ProcessState::DiskSleep => 'D',
            ProcessState::Stopped => 'T',
            ProcessState::TracingStop => 't',
            ProcessState::Zombie => 'Z',
            ProcessState::Dead => 'X',
            ProcessState::Idle => 'I',
            ProcessState::Parked => 'P',
            ProcessState::Unknown(letter) => letter,
        }
    }

    /// A word or two for the UI ("Running", "Waiting for I/O").
    pub fn describe(self) -> &'static str {
        match self {
            ProcessState::Running => "Running",
            ProcessState::Sleeping => "Sleeping",
            ProcessState::DiskSleep => "Waiting for I/O",
            ProcessState::Stopped => "Stopped",
            ProcessState::TracingStop => "Stopped by debugger",
            ProcessState::Zombie => "Zombie",
            ProcessState::Dead => "Dead",
            ProcessState::Idle => "Idle",
            ProcessState::Parked => "Parked",
            ProcessState::Unknown(_) => "Unknown",
        }
    }
}

/// Which systemd instance manages a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnitManager {
    /// The system manager (PID 1).
    System,
    /// The user manager (`systemd --user`, in `user@UID.service`) of this
    /// uid.
    User(u32),
}

/// The systemd unit a process runs in, from its cgroup path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SystemdUnit {
    /// The unit name, such as `pipewire.service` or
    /// `app-aizome-foot-1234.scope`, as it appears in the cgroup path
    /// (systemd's escapes, like `\x2d`, left in).
    pub name: String,
    /// Whose manager runs it.
    pub manager: UnitManager,
}

impl SystemdUnit {
    /// The unit for a cgroup path such as
    /// `/user.slice/user-1000.slice/user@1000.service/session.slice/pipewire.service`:
    /// the last component ending in `.service` or `.scope`. `None` for paths
    /// with no unit, such as `/` (kernel threads) or a bare slice.
    pub fn from_cgroup(path: &str) -> Option<SystemdUnit> {
        let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        let position = components.iter().rposition(|c| c.ends_with(".service") || c.ends_with(".scope"))?;
        let name = components.get(position)?;
        let manager = components
            .get(..position)
            .unwrap_or_default()
            .iter()
            .rev()
            .find_map(|c| c.strip_prefix("user@")?.strip_suffix(".service")?.parse().ok())
            .map_or(UnitManager::System, UnitManager::User);
        Some(SystemdUnit { name: (*name).to_owned(), manager })
    }

    /// Whether a user's manager runs it, rather than the system's.
    pub fn is_user(&self) -> bool {
        matches!(self.manager, UnitManager::User(_))
    }

    /// Whether it is a `.service` (started by systemd).
    pub fn is_service(&self) -> bool {
        self.name.ends_with(".service")
    }

    /// Whether it is a `.scope` (processes started elsewhere and grouped,
    /// such as an app launched from the shell).
    pub fn is_scope(&self) -> bool {
        self.name.ends_with(".scope")
    }

    /// The application ID of an app unit named per systemd's desktop
    /// integration convention, `app[-LAUNCHER]-APPID[@RANDOM].service` or
    /// `app[-LAUNCHER]-APPID-RANDOM.scope`: `foot` for
    /// `app-aizome-foot-1234.scope`, `com.anthropic.Claude` for
    /// `app-com.anthropic.Claude-4006.scope`. `None` for other units.
    pub fn app_id(&self) -> Option<String> {
        let rest = self.name.strip_prefix("app-")?;
        let rest = if let Some(scope) = rest.strip_suffix(".scope") {
            scope.rsplit_once('-').map_or(scope, |(name, _random)| name)
        } else {
            let service = rest.strip_suffix(".service")?;
            service.split_once('@').map_or(service, |(name, _random)| name)
        };
        // Unescaped dashes separate the launcher from the app ID (dashes in
        // the ID itself are escaped as \x2d).
        let id = rest.split_once('-').map_or(rest, |(_launcher, id)| id);
        let id = unescape_unit(id);
        (!id.is_empty()).then_some(id)
    }
}

/// Undo systemd's `\xNN` escapes in a unit name component.
fn unescape_unit(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'\\'
            && let Some(hex) = tail.get(1..3).filter(|_| tail.first() == Some(&b'x'))
            && let Some(value) = std::str::from_utf8(hex).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            bytes.push(value);
            rest = tail.get(3..).unwrap_or_default();
            continue;
        }
        bytes.push(byte);
        rest = tail;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// One process, as of the sample that listed it.
#[derive(Clone, Debug, PartialEq)]
pub struct Process {
    /// Its process ID.
    pub pid: u32,
    /// Its parent's process ID (0 for `init` and `kthreadd`).
    pub ppid: u32,
    /// Its name as the kernel keeps it (`comm`): the executable's file
    /// name, cut to 15 bytes, unless the process renamed itself. See
    /// [`display_name`](Self::display_name) for the untruncated one.
    pub name: String,
    /// Its command line, one string per argument. Empty for kernel threads
    /// and zombies. A process that rewrote its command line (`sshd:
    /// user@pts/0`) may have it all in one string.
    pub args: Vec<String>,
    /// The executable's path, when readable (only the caller's own
    /// processes, without privileges). The kernel appends ` (deleted)` when
    /// the file was replaced, as after an upgrade.
    pub exe: Option<PathBuf>,
    /// The real uid it runs as (the first `Uid:` field of `status`).
    pub uid: u32,
    /// The user name for [`uid`](Self::uid), or the uid in decimal if it
    /// has no name.
    pub user: String,
    /// CPU use since the previous sample as a percentage of one CPU, as in
    /// `top`: a process keeping two cores busy shows 200. 0 in the first
    /// sample.
    pub cpu_percent: f32,
    /// [`cpu_percent`](Self::cpu_percent) divided by the number of online
    /// CPUs: its share of the whole machine, 0–100.
    pub machine_cpu_percent: f32,
    /// Total CPU time used since it started (user + system).
    pub cpu_time: Duration,
    /// Resident memory in bytes (RSS: its pages in RAM, including shared
    /// libraries and shared memory it has touched).
    pub memory: u64,
    /// Number of threads.
    pub threads: u32,
    /// What it is doing.
    pub state: ProcessState,
    /// Its nice value, from -20 (most favoured) to 19.
    pub nice: i32,
    /// When it started, to the clock tick (the boot time plus
    /// [`start_ticks`](Self::start_ticks)).
    pub start_time: SystemTime,
    /// When it started, in clock ticks since boot. With the pid, this
    /// identifies the process: a reused pid has a later start.
    pub start_ticks: u64,
    /// Its cgroup path, such as `/user.slice/user-1000.slice/session-3.scope`.
    pub cgroup: Option<String>,
    /// The systemd unit it runs in, from [`cgroup`](Self::cgroup).
    pub unit: Option<SystemdUnit>,
    /// Whether it is a kernel thread (`kthreadd` and its children), which
    /// has no command line, executable or memory of its own.
    pub kernel_thread: bool,
}

impl Process {
    /// The name to show: [`name`](Self::name), or the file name of the first
    /// argument when `comm` is that name cut to 15 bytes
    /// (`gnome-shell-calendar-server` rather than `gnome-shell-cal`).
    pub fn display_name(&self) -> &str {
        if self.name.len() >= 15
            && let Some(first) = self.args.first()
        {
            let program = first.split(' ').next().unwrap_or(first);
            let base = program.rsplit('/').next().unwrap_or(program);
            if base.len() > self.name.len() && base.starts_with(self.name.as_str()) {
                return base;
            }
        }
        &self.name
    }

    /// The command line as one string, arguments separated by spaces; for
    /// kernel threads and zombies, the name in brackets, as `ps` shows them.
    pub fn command_line(&self) -> String {
        if self.args.is_empty() { format!("[{}]", self.name) } else { self.args.join(" ") }
    }

    /// Whether the caller may signal it under [`signal_process`]'s rule:
    /// its real uid is the caller's.
    ///
    /// [`signal_process`]: super::signal_process
    pub fn is_own(&self) -> bool {
        // SAFETY: getuid has no preconditions and cannot fail.
        self.uid == unsafe { libc::getuid() }
    }

    /// Send `signal` to this process, as [`signal_process`] does, but only
    /// if its pid still belongs to this process: if it has exited and the
    /// pid was reused since this sample, [`Error::NotFound`].
    ///
    /// [`signal_process`]: super::signal_process
    /// [`Error::NotFound`]: crate::Error::NotFound
    pub fn signal(&self, signal: Signal) -> Result<()> {
        send_checked(self.pid, signal, Some(self.start_ticks))
    }
}

/// What a process list can be sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortKey {
    /// Process ID.
    Pid,
    /// [`Process::display_name`], ignoring case.
    Name,
    /// User name, ignoring case.
    User,
    /// [`Process::cpu_percent`].
    Cpu,
    /// Resident memory.
    Memory,
    /// Thread count.
    Threads,
    /// Start time.
    StartTime,
}

/// Sort `processes` by `key`, largest first when `descending`. Ties are
/// broken by pid, ascending either way, so the order is stable from one
/// sample to the next.
pub fn sort_processes(processes: &mut [Process], key: SortKey, descending: bool) {
    fn caseless<'a>(text: &'a str) -> impl Iterator<Item = char> + 'a {
        text.chars().flat_map(char::to_lowercase)
    }
    processes.sort_by(|a, b| {
        let order = match key {
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::Name => caseless(a.display_name()).cmp(caseless(b.display_name())),
            SortKey::User => caseless(&a.user).cmp(caseless(&b.user)),
            SortKey::Cpu => a.cpu_percent.total_cmp(&b.cpu_percent),
            SortKey::Memory => a.memory.cmp(&b.memory),
            SortKey::Threads => a.threads.cmp(&b.threads),
            SortKey::StartTime => a.start_ticks.cmp(&b.start_ticks),
        };
        let order = if descending { order.reverse() } else { order };
        order.then(a.pid.cmp(&b.pid))
    });
}

/// The fields of `/proc/PID/stat` the sampler uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stat {
    pub pid: u32,
    pub comm: String,
    pub state: char,
    pub ppid: u32,
    pub flags: u64,
    /// User and system CPU time, in clock ticks.
    pub utime: u64,
    pub stime: u64,
    pub nice: i32,
    pub threads: u32,
    /// Start time in clock ticks since boot.
    pub start_ticks: u64,
    pub rss_pages: u64,
}

/// Parse `/proc/PID/stat`. The name sits between the first `(` and the
/// *last* `)`, since it may itself contain spaces and parentheses.
pub(crate) fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let pid = text.get(..open)?.trim().parse().ok()?;
    let comm = text.get(open + 1..close)?.to_owned();
    let mut fields = text.get(close + 1..)?.split_ascii_whitespace();
    // Field numbers below are proc(5)'s, counting the pid as 1.
    let state = fields.next()?.chars().next()?; // 3
    let ppid = fields.next()?.parse().ok()?; // 4
    let flags = fields.nth(4)?.parse().ok()?; // 9
    let utime = fields.nth(4)?.parse().ok()?; // 14
    let stime = fields.next()?.parse().ok()?; // 15
    let nice = fields.nth(3)?.parse().ok()?; // 19
    let threads: i64 = fields.next()?.parse().ok()?; // 20
    let start_ticks = fields.nth(1)?.parse().ok()?; // 22
    let rss: i64 = fields.nth(1)?.parse().ok()?; // 24
    Some(Stat {
        pid,
        comm,
        state,
        ppid,
        flags,
        utime,
        stime,
        nice,
        threads: u32::try_from(threads).unwrap_or(0),
        start_ticks,
        rss_pages: u64::try_from(rss).unwrap_or(0),
    })
}

/// Split `/proc/PID/cmdline` (NUL-separated, NUL-terminated) into
/// arguments. A command line rewritten without NULs stays one argument.
pub(crate) fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_suffix(b"\0").unwrap_or(bytes);
    if bytes.is_empty() {
        return Vec::new();
    }
    bytes.split(|&b| b == 0).map(|arg| String::from_utf8_lossy(arg).into_owned()).collect()
}

/// The real uid from `/proc/PID/status` (`Uid:\treal\teffective\tsaved\tfs`).
pub(crate) fn parse_status_uid(text: &str) -> Option<u32> {
    text.lines().find_map(|line| line.strip_prefix("Uid:"))?.split_ascii_whitespace().next()?.parse().ok()
}

/// The cgroup path from `/proc/PID/cgroup`: the cgroup v2 line (`0::PATH`),
/// or on a v1/hybrid system without one, systemd's `name=systemd` line.
pub(crate) fn parse_cgroup(text: &str) -> Option<String> {
    let mut v1 = None;
    for line in text.lines() {
        let mut parts = line.splitn(3, ':');
        let (Some(id), Some(controllers), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if id == "0" && controllers.is_empty() {
            return Some(path.to_owned());
        }
        if controllers.split(',').any(|c| c == "name=systemd") {
            v1 = Some(path.to_owned());
        }
    }
    v1
}

/// The name of user `uid`, from the password database (`getpwuid_r`, so
/// NSS sources such as systemd-userdb count).
pub(crate) fn lookup_user_name(uid: u32) -> Option<String> {
    // SAFETY: sysconf has no preconditions.
    let suggested = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let mut size = usize::try_from(suggested).ok().filter(|&n| n > 0).unwrap_or(1024);
    loop {
        let mut buffer = vec![0 as libc::c_char; size];
        // SAFETY: passwd is plain data (integers and pointers), for which
        // all-zero is valid.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: every pointer is valid for the duration of the call and
        // `buffer.len()` is its true size.
        let status = unsafe { libc::getpwuid_r(uid, &mut entry, buffer.as_mut_ptr(), buffer.len(), &mut found) };
        if status == libc::ERANGE && size < 1 << 20 {
            size *= 2;
            continue;
        }
        if status != 0 || found.is_null() || entry.pw_name.is_null() {
            return None;
        }
        // SAFETY: on success pw_name points at a NUL-terminated string in
        // `buffer`, which is still alive.
        let name = unsafe { CStr::from_ptr(entry.pw_name) };
        return Some(name.to_string_lossy().into_owned());
    }
}

/// What the sampler remembers about a process between samples.
#[derive(Debug)]
struct Known {
    start_ticks: u64,
    comm: String,
    args: Vec<String>,
    exe: Option<PathBuf>,
    uid: u32,
    user: String,
    cgroup: Option<String>,
    unit: Option<SystemdUnit>,
    /// utime + stime at the last sample.
    cpu_ticks: u64,
    /// The sample that last saw it, to forget processes that exited.
    seen: u64,
}

/// Lists processes and measures their CPU use between calls to
/// [`sample`](Self::sample). See the module documentation for what it reads
/// and caches.
#[derive(Debug)]
pub struct ProcessSampler {
    proc_root: PathBuf,
    known: HashMap<u32, Known>,
    users: HashMap<u32, String>,
    /// When the previous sample ran, and the uptime then in clock ticks.
    previous: Option<(Instant, Option<u64>)>,
    generation: u64,
    boot_time: Option<SystemTime>,
    buffer: Vec<u8>,
}

impl Default for ProcessSampler {
    fn default() -> ProcessSampler {
        ProcessSampler::new()
    }
}

impl ProcessSampler {
    /// A sampler with nothing cached: its first sample reads everything and
    /// reports 0% CPU for every process.
    pub fn new() -> ProcessSampler {
        ProcessSampler::with_root("/proc")
    }

    /// A sampler reading a `/proc`-like tree at `root`.
    pub(crate) fn with_root(root: impl Into<PathBuf>) -> ProcessSampler {
        ProcessSampler {
            proc_root: root.into(),
            known: HashMap::new(),
            users: HashMap::new(),
            previous: None,
            generation: 0,
            boot_time: None,
            buffer: Vec::with_capacity(1024),
        }
    }

    /// List the processes, with CPU use since the previous call, in no
    /// particular order (see [`sort_processes`]). Processes that exit while
    /// being read are left out.
    pub fn sample(&mut self) -> Vec<Process> {
        let now = Instant::now();
        let uptime = super::read_attr(self.proc_root.join("uptime")).and_then(|text| super::load::parse_uptime(&text));
        self.sample_at(now, uptime)
    }

    /// [`sample`](Self::sample) with the clock and uptime given.
    pub(crate) fn sample_at(&mut self, now: Instant, uptime: Option<Duration>) -> Vec<Process> {
        let ticks_per_second = clock_ticks();
        let uptime_ticks = uptime.map(|up| {
            let ticks = up.as_secs_f64() * ticks_per_second as f64;
            ticks as u64
        });
        let interval = self.previous.and_then(|(then, then_ticks)| {
            let seconds = now.saturating_duration_since(then).as_secs_f64();
            (seconds > 0.0).then_some(Interval { seconds, since_ticks: then_ticks })
        });
        self.previous = Some((now, uptime_ticks));
        self.generation += 1;
        let boot_time = self.boot_time(uptime);
        let context = Context {
            interval,
            uptime_ticks,
            ticks_per_second,
            cpus: online_cpus() as f32,
            page_size: page_size(),
            boot_time,
            fresh_ticks: FRESH_FOR.as_secs().saturating_mul(ticks_per_second),
        };

        let entries = match std::fs::read_dir(&self.proc_root) {
            Ok(entries) => entries,
            Err(error) => {
                log::warn!("{}: {error}", self.proc_root.display());
                return Vec::new();
            }
        };
        let mut processes = Vec::with_capacity(self.known.len() + 32);
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            if let Some(process) = self.read_process(pid, &context) {
                processes.push(process);
            }
        }
        let generation = self.generation;
        self.known.retain(|_, known| known.seen == generation);
        processes
    }

    /// The boot time: `btime` from `stat`, read once; failing that, now
    /// less the uptime.
    fn boot_time(&mut self, uptime: Option<Duration>) -> SystemTime {
        if let Some(boot) = self.boot_time {
            return boot;
        }
        let from_stat = super::read_text(self.proc_root.join("stat"))
            .ok()
            .and_then(|text| super::cpu::parse_proc_stat(&text).boot_time)
            .and_then(|s| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(s)));
        match from_stat {
            Some(boot) => {
                self.boot_time = Some(boot);
                boot
            }
            None => uptime.and_then(|up| SystemTime::now().checked_sub(up)).unwrap_or(SystemTime::UNIX_EPOCH),
        }
    }

    fn read_process(&mut self, pid: u32, context: &Context) -> Option<Process> {
        let dir = self.proc_root.join(pid.to_string());
        read_into(&dir.join("stat"), &mut self.buffer).ok()?;
        let stat = parse_stat(&String::from_utf8_lossy(&self.buffer))?;
        let ticks = stat.utime.saturating_add(stat.stime);

        let same = self.known.get(&pid).filter(|known| known.start_ticks == stat.start_ticks);
        let previous_ticks = same.map(|known| known.cpu_ticks);
        let young = context.uptime_ticks.is_some_and(|up| up.saturating_sub(stat.start_ticks) < context.fresh_ticks);
        if same.is_none_or(|known| known.comm != stat.comm || young) {
            let known = self.read_details(&dir, &stat, previous_ticks)?;
            self.known.insert(pid, known);
        }
        let known = self.known.get_mut(&pid)?;

        let cpu_percent = match (context.interval, previous_ticks) {
            (Some(interval), Some(previous)) => {
                ticks.saturating_sub(previous) as f64 / context.ticks_per_second as f64 / interval.seconds * 100.0
            }
            // New since the previous sample: everything it used, it used
            // within the interval.
            (Some(interval), None) if interval.since_ticks.is_some_and(|then| stat.start_ticks >= then) => {
                ticks as f64 / context.ticks_per_second as f64 / interval.seconds * 100.0
            }
            _ => 0.0,
        };
        let cpu_percent = (cpu_percent.max(0.0) as f32).min(100.0 * context.cpus);
        known.cpu_ticks = ticks;
        known.seen = self.generation;

        let since_boot = ticks_to_duration(stat.start_ticks, context.ticks_per_second);
        Some(Process {
            pid,
            ppid: stat.ppid,
            name: stat.comm,
            args: known.args.clone(),
            exe: known.exe.clone(),
            uid: known.uid,
            user: known.user.clone(),
            cpu_percent,
            machine_cpu_percent: (cpu_percent / context.cpus).clamp(0.0, 100.0),
            cpu_time: ticks_to_duration(ticks, context.ticks_per_second),
            memory: stat.rss_pages.saturating_mul(context.page_size),
            threads: stat.threads,
            state: ProcessState::from_char(stat.state),
            nice: stat.nice,
            start_time: context.boot_time.checked_add(since_boot).unwrap_or(context.boot_time),
            start_ticks: stat.start_ticks,
            cgroup: known.cgroup.clone(),
            unit: known.unit.clone(),
            kernel_thread: stat.flags & PF_KTHREAD != 0,
        })
    }

    /// Read what rarely changes. `None` if the process has gone.
    fn read_details(&mut self, dir: &Path, stat: &Stat, cpu_ticks: Option<u64>) -> Option<Known> {
        read_into(&dir.join("status"), &mut self.buffer).ok()?;
        let uid = parse_status_uid(&String::from_utf8_lossy(&self.buffer))?;
        let kernel_thread = stat.flags & PF_KTHREAD != 0;
        let (args, exe) = if kernel_thread {
            (Vec::new(), None)
        } else {
            let args = match read_into(&dir.join("cmdline"), &mut self.buffer) {
                Ok(()) => parse_cmdline(&self.buffer),
                Err(_) => Vec::new(),
            };
            (args, std::fs::read_link(dir.join("exe")).ok())
        };
        let cgroup = match read_into(&dir.join("cgroup"), &mut self.buffer) {
            Ok(()) => parse_cgroup(&String::from_utf8_lossy(&self.buffer)),
            Err(_) => None,
        };
        let unit = cgroup.as_deref().and_then(SystemdUnit::from_cgroup);
        let user =
            self.users.entry(uid).or_insert_with(|| lookup_user_name(uid).unwrap_or_else(|| uid.to_string())).clone();
        Some(Known {
            start_ticks: stat.start_ticks,
            comm: stat.comm.clone(),
            args,
            exe,
            uid,
            user,
            cgroup,
            unit,
            cpu_ticks: cpu_ticks.unwrap_or(0),
            seen: 0,
        })
    }
}

/// The time between two samples.
#[derive(Clone, Copy, Debug)]
struct Interval {
    seconds: f64,
    /// The uptime at the previous sample, in clock ticks.
    since_ticks: Option<u64>,
}

/// What every process in one sample shares.
struct Context {
    interval: Option<Interval>,
    uptime_ticks: Option<u64>,
    ticks_per_second: u64,
    cpus: f32,
    page_size: u64,
    boot_time: SystemTime,
    fresh_ticks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    // This machine's `cat` (trimmed of nothing: all 52 fields).
    const STAT_CAT: &str = "545842 (cat) R 545841 545842 545841 0 -1 4194304 142 0 0 0 0 0 0 0 20 0 1 0 21130570 10702848 725 18446744073709551615 100170682748928 100170683841697 140720702784064 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0 100170684194600 100170684247044 100171210719232 140720702792882 140720702792902 140720702792902 140720702803947 0\n";
    const STAT_KTHREADD: &str = "2 (kthreadd) S 0 0 0 0 -1 2129984 0 0 0 0 0 6 0 0 20 0 1 0 9 0 0 18446744073709551615 0 0 0 0 0 0 0 2147483647 0 0 0 0 0 7 0 0 0 0 0 0 0 0 0 0 0 0 0\n";

    #[test]
    fn stat_parses() {
        let stat = parse_stat(STAT_CAT).unwrap();
        assert_eq!(stat.pid, 545842);
        assert_eq!(stat.comm, "cat");
        assert_eq!(stat.state, 'R');
        assert_eq!(stat.ppid, 545841);
        assert_eq!(stat.flags, 4194304);
        assert_eq!((stat.utime, stat.stime), (0, 0));
        assert_eq!(stat.nice, 0);
        assert_eq!(stat.threads, 1);
        assert_eq!(stat.start_ticks, 21130570);
        assert_eq!(stat.rss_pages, 725);

        let kthreadd = parse_stat(STAT_KTHREADD).unwrap();
        assert_eq!(kthreadd.stime, 6);
        assert_eq!(kthreadd.ppid, 0);
        assert_ne!(kthreadd.flags & PF_KTHREAD, 0);
    }

    #[test]
    fn stat_names_may_hold_spaces_and_parentheses() {
        let stat =
            parse_stat("4242 (Web Content (x)) S 1 4242 4242 0 -1 0 0 0 0 0 1500 250 0 0 20 -5 31 0 777 0 52000 0\n")
                .unwrap();
        assert_eq!(stat.comm, "Web Content (x)");
        assert_eq!(stat.utime, 1500);
        assert_eq!(stat.stime, 250);
        assert_eq!(stat.nice, -5);
        assert_eq!(stat.threads, 31);
        assert_eq!(stat.start_ticks, 777);
        assert_eq!(stat.rss_pages, 52000);
    }

    #[test]
    fn odd_stat_is_none_and_negative_counts_are_zero() {
        assert_eq!(parse_stat(""), None);
        assert_eq!(parse_stat("12 (x"), None);
        assert_eq!(parse_stat("12 )x( S 1"), None);
        assert_eq!(parse_stat("12 (x) S 1 2 3"), None);
        assert_eq!(parse_stat("x (y) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 5 0 3 0"), None);
        let stat = parse_stat("12 (x) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 -1 0 5 0 -3 0").unwrap();
        assert_eq!((stat.threads, stat.rss_pages), (0, 0));
    }

    #[test]
    fn cmdlines_split_on_nuls() {
        assert_eq!(
            parse_cmdline(b"/run/current-system/sw/bin/foot\0--server\0"),
            ["/run/current-system/sw/bin/foot", "--server"]
        );
        assert_eq!(parse_cmdline(b""), Vec::<String>::new());
        assert_eq!(parse_cmdline(b"sshd: callen@pts/0"), ["sshd: callen@pts/0"]);
        assert_eq!(parse_cmdline(b"a\0\0b\0"), ["a", "", "b"]);
        assert_eq!(parse_cmdline(b"\xff\0"), ["\u{fffd}"]);
    }

    #[test]
    fn status_gives_the_real_uid() {
        let status = "Name:\tsudo\nUmask:\t0022\nState:\tS (sleeping)\nUid:\t1000\t0\t0\t0\nGid:\t100\t100\t100\t100\n";
        assert_eq!(parse_status_uid(status), Some(1000));
        assert_eq!(parse_status_uid("Name:\tx\n"), None);
        assert_eq!(parse_status_uid("Uid:\n"), None);
    }

    #[test]
    fn cgroups_prefer_the_v2_line() {
        assert_eq!(
            parse_cgroup(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-com.anthropic.Claude-4006.scope\n"
            )
            .as_deref(),
            Some("/user.slice/user-1000.slice/user@1000.service/app.slice/app-com.anthropic.Claude-4006.scope")
        );
        let hybrid =
            "12:cpuset:/\n1:name=systemd:/system.slice/nix-daemon.service\n0::/system.slice/nix-daemon.service\n";
        assert_eq!(parse_cgroup(hybrid).as_deref(), Some("/system.slice/nix-daemon.service"));
        let v1 = "3:cpu,cpuacct:/\n1:name=systemd:/system.slice/sshd.service\n";
        assert_eq!(parse_cgroup(v1).as_deref(), Some("/system.slice/sshd.service"));
        assert_eq!(parse_cgroup("garbage\n"), None);
        assert_eq!(parse_cgroup("0::/\n").as_deref(), Some("/"));
    }

    #[test]
    fn units_come_from_the_deepest_service_or_scope() {
        let unit =
            SystemdUnit::from_cgroup("/user.slice/user-1000.slice/user@1000.service/session.slice/pipewire.service")
                .unwrap();
        assert_eq!(unit.name, "pipewire.service");
        assert_eq!(unit.manager, UnitManager::User(1000));
        assert!(unit.is_user() && unit.is_service() && !unit.is_scope());

        let system = SystemdUnit::from_cgroup("/system.slice/nix-daemon.service").unwrap();
        assert_eq!(system.manager, UnitManager::System);

        // A login session's scope is the system manager's, though it sits
        // under the user's slice.
        let session = SystemdUnit::from_cgroup("/user.slice/user-1000.slice/session-3.scope").unwrap();
        assert_eq!(session.name, "session-3.scope");
        assert_eq!(session.manager, UnitManager::System);

        // The user manager itself.
        let manager = SystemdUnit::from_cgroup("/user.slice/user-1000.slice/user@1000.service/init.scope").unwrap();
        assert_eq!(manager.name, "init.scope");
        assert_eq!(manager.manager, UnitManager::User(1000));

        // A sub-cgroup inside a service still belongs to it.
        let nested = SystemdUnit::from_cgroup("/system.slice/docker.service/payload").unwrap();
        assert_eq!(nested.name, "docker.service");

        assert_eq!(SystemdUnit::from_cgroup("/"), None);
        assert_eq!(SystemdUnit::from_cgroup(""), None);
        assert_eq!(SystemdUnit::from_cgroup("/user.slice/user-1000.slice"), None);
        assert_eq!(SystemdUnit::from_cgroup("/init.scope").unwrap().manager, UnitManager::System);
    }

    #[test]
    fn app_ids_follow_the_desktop_naming_convention() {
        let app = |name: &str| SystemdUnit { name: name.into(), manager: UnitManager::User(1000) }.app_id();
        assert_eq!(app("app-aizome-foot-1234.scope").as_deref(), Some("foot"));
        assert_eq!(app("app-com.anthropic.Claude-4006.scope").as_deref(), Some("com.anthropic.Claude"));
        assert_eq!(app("app-gnome-org.gnome.Nautilus@12345.service").as_deref(), Some("org.gnome.Nautilus"));
        assert_eq!(app("app-org.gnome.Terminal.service").as_deref(), Some("org.gnome.Terminal"));
        assert_eq!(app(r"app-aizome-foot\x2dserver-99.scope").as_deref(), Some("foot-server"));
        assert_eq!(app("pipewire.service"), None);
        assert_eq!(app("run-p3183-i8539.service"), None);
        assert_eq!(app("app-.scope"), None);
        assert_eq!(unescape_unit(r"a\x2"), r"a\x2");
        assert_eq!(unescape_unit(r"a\xzz\"), r"a\xzz\");
    }

    #[test]
    fn states_map_from_letters() {
        for letter in ['R', 'S', 'D', 'T', 't', 'Z', 'X', 'I', 'P', 'W'] {
            assert_eq!(ProcessState::from_char(letter).as_char(), letter);
        }
        assert_eq!(ProcessState::from_char('D'), ProcessState::DiskSleep);
        assert_eq!(ProcessState::from_char('W'), ProcessState::Unknown('W'));
        assert_eq!(ProcessState::from_char('x'), ProcessState::Dead);
    }

    fn process(pid: u32, name: &str, user: &str, cpu: f32, memory: u64) -> Process {
        Process {
            pid,
            ppid: 1,
            name: name.into(),
            args: Vec::new(),
            exe: None,
            uid: 1000,
            user: user.into(),
            cpu_percent: cpu,
            machine_cpu_percent: cpu / 8.0,
            cpu_time: Duration::ZERO,
            memory,
            threads: pid,
            state: ProcessState::Sleeping,
            nice: 0,
            start_time: SystemTime::UNIX_EPOCH,
            start_ticks: 1000 - u64::from(pid),
            cgroup: None,
            unit: None,
            kernel_thread: false,
        }
    }

    #[test]
    fn sorting_breaks_ties_by_pid() {
        let mut list = vec![
            process(30, "foot", "callen", 5.0, 100),
            process(10, "Bash", "root", 5.0, 300),
            process(20, "bash", "callen", 50.0, 200),
        ];
        let pids = |list: &[Process]| list.iter().map(|p| p.pid).collect::<Vec<_>>();
        sort_processes(&mut list, SortKey::Cpu, true);
        assert_eq!(pids(&list), [20, 10, 30]);
        sort_processes(&mut list, SortKey::Cpu, false);
        assert_eq!(pids(&list), [10, 30, 20]);
        sort_processes(&mut list, SortKey::Name, false);
        assert_eq!(pids(&list), [10, 20, 30]);
        sort_processes(&mut list, SortKey::User, false);
        assert_eq!(pids(&list), [20, 30, 10]);
        sort_processes(&mut list, SortKey::Memory, true);
        assert_eq!(pids(&list), [10, 20, 30]);
        sort_processes(&mut list, SortKey::Threads, true);
        assert_eq!(pids(&list), [30, 20, 10]);
        sort_processes(&mut list, SortKey::StartTime, false);
        assert_eq!(pids(&list), [30, 20, 10]);
        sort_processes(&mut list, SortKey::Pid, false);
        assert_eq!(pids(&list), [10, 20, 30]);
    }

    #[test]
    fn display_names_undo_comm_truncation() {
        let mut p = process(1, "gnome-shell-cal", "u", 0.0, 0);
        p.args = vec!["/nix/store/abc-gnome-shell/libexec/gnome-shell-calendar-server".into()];
        assert_eq!(p.display_name(), "gnome-shell-calendar-server");
        p.args = vec!["/usr/bin/something-else".into()];
        assert_eq!(p.display_name(), "gnome-shell-cal");
        let short = process(2, "foot", "u", 0.0, 0);
        assert_eq!(short.display_name(), "foot");
        assert_eq!(short.command_line(), "[foot]");
    }

    /// A fake `/proc` with `stat`, `uptime` and process directories.
    struct FakeProc(TestTree);

    impl FakeProc {
        fn new(name: &str) -> FakeProc {
            let tree = TestTree::new(name);
            tree.file("stat", "cpu 1 1 1 1\nbtime 1792056739\n")
                .file("uptime", "100.00 400.00\n")
                .file("self", "not a process directory\n");
            FakeProc(tree)
        }

        #[allow(clippy::too_many_arguments)]
        fn process(&self, pid: u32, comm: &str, uid: u32, ticks: u64, start: u64, cmdline: &str, cgroup: &str) {
            let dir = format!("{pid}");
            self.0
                .file(
                    &format!("{dir}/stat"),
                    &format!(
                        "{pid} ({comm}) S 1 {pid} {pid} 0 -1 4194560 0 0 0 0 {ticks} 0 0 0 20 0 3 0 {start} 0 256 0\n"
                    ),
                )
                .file(&format!("{dir}/status"), &format!("Name:\t{comm}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"))
                .file(&format!("{dir}/cmdline"), cmdline)
                .file(&format!("{dir}/cgroup"), &format!("0::{cgroup}\n"));
        }

        fn remove(&self, pid: u32) {
            std::fs::remove_dir_all(self.0.path(&pid.to_string())).unwrap();
        }
    }

    fn by_pid(list: Vec<Process>) -> HashMap<u32, Process> {
        list.into_iter().map(|p| (p.pid, p)).collect()
    }

    #[test]
    fn the_sampler_measures_cpu_between_samples() {
        let fake = FakeProc::new("sampler-cpu");
        let hz = clock_ticks();
        // Started at 10 s, long before "now" (uptime 100 s).
        fake.process(
            100,
            "busy",
            1000,
            0,
            10 * hz,
            "busy\0--spin\0",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-aizome-busy-7.scope",
        );
        fake.process(200, "idle", 1000, 50, 10 * hz, "idle\0", "/system.slice/idle.service");
        let mut sampler = ProcessSampler::with_root(&fake.0.0);
        sampler.users.insert(1000, "alice".into());
        let start = Instant::now();

        let first = by_pid(sampler.sample_at(start, Some(Duration::from_secs(100))));
        assert_eq!(first.len(), 2);
        let busy = &first[&100];
        assert_eq!(busy.cpu_percent, 0.0, "the first sample has no interval");
        assert_eq!(busy.name, "busy");
        assert_eq!(busy.args, ["busy", "--spin"]);
        assert_eq!(busy.user, "alice");
        assert_eq!(busy.threads, 3);
        assert_eq!(busy.memory, 256 * page_size());
        assert_eq!(busy.unit.as_ref().unwrap().app_id().as_deref(), Some("busy"));
        assert_eq!(busy.start_time, SystemTime::UNIX_EPOCH + Duration::from_secs(1792056739 + 10));
        assert!(!busy.kernel_thread);

        // Two seconds later "busy" has used 3 s of CPU: 150% of one CPU.
        fake.process(100, "busy", 1000, 3 * hz, 10 * hz, "busy\0--spin\0", "/x.scope");
        let second = by_pid(sampler.sample_at(start + Duration::from_secs(2), Some(Duration::from_secs(102))));
        let busy = &second[&100];
        assert!((busy.cpu_percent - 150.0).abs() < 0.01, "{}", busy.cpu_percent);
        let cpus = online_cpus() as f32;
        assert!((busy.machine_cpu_percent - (150.0 / cpus).min(100.0)).abs() < 0.01);
        assert_eq!(busy.cpu_time, Duration::from_secs(3));
        assert_eq!(second[&200].cpu_percent, 0.0);
        // Old processes keep their cached cgroup.
        assert_eq!(busy.unit.as_ref().unwrap().name, "app-aizome-busy-7.scope");
    }

    #[test]
    fn reused_pids_and_execs_are_read_again() {
        let fake = FakeProc::new("sampler-reuse");
        let hz = clock_ticks();
        fake.process(300, "bash", 1000, 100, 10 * hz, "bash\0", "/a.service");
        let mut sampler = ProcessSampler::with_root(&fake.0.0);
        sampler.users.insert(1000, "alice".into());
        sampler.users.insert(1001, "bob".into());
        let start = Instant::now();
        sampler.sample_at(start, Some(Duration::from_secs(100)));

        // The process execs: same pid and start, new comm.
        fake.process(300, "vim", 1000, 100, 10 * hz, "vim\0notes.txt\0", "/a.service");
        let after_exec = by_pid(sampler.sample_at(start + Duration::from_secs(1), Some(Duration::from_secs(101))));
        assert_eq!(after_exec[&300].args, ["vim", "notes.txt"]);
        assert_eq!(after_exec[&300].cpu_percent, 0.0, "CPU carries over an exec");

        // It exits and its pid goes to a new process of another user, which
        // started within the interval and used 0.5 s of CPU in it.
        fake.remove(300);
        fake.process(300, "sleep", 1001, hz / 2, 101 * hz + 50, "sleep\0", "/b.service");
        let reused = by_pid(sampler.sample_at(start + Duration::from_secs(2), Some(Duration::from_secs(102))));
        let sleep = &reused[&300];
        assert_eq!(sleep.args, ["sleep"]);
        assert_eq!(sleep.user, "bob");
        assert_eq!(sleep.unit.as_ref().unwrap().name, "b.service");
        assert!((sleep.cpu_percent - 50.0).abs() < 0.01, "{}", sleep.cpu_percent);
    }

    #[test]
    fn young_processes_are_reread_and_gone_ones_forgotten() {
        let fake = FakeProc::new("sampler-young");
        let hz = clock_ticks();
        // Started at 99 s: one second old.
        fake.process(
            400,
            "foot",
            1000,
            0,
            99 * hz,
            "foot\0",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/launcher.service",
        );
        let mut sampler = ProcessSampler::with_root(&fake.0.0);
        sampler.users.insert(1000, "alice".into());
        let start = Instant::now();
        sampler.sample_at(start, Some(Duration::from_secs(100)));
        // Its launcher moves it into its own scope.
        fake.process(
            400,
            "foot",
            1000,
            0,
            99 * hz,
            "foot\0",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-aizome-foot-400.scope",
        );
        let moved = by_pid(sampler.sample_at(start + Duration::from_secs(1), Some(Duration::from_secs(101))));
        assert_eq!(moved[&400].unit.as_ref().unwrap().name, "app-aizome-foot-400.scope");

        fake.remove(400);
        assert!(sampler.sample_at(start + Duration::from_secs(2), Some(Duration::from_secs(102))).is_empty());
        assert!(sampler.known.is_empty());
    }

    #[test]
    fn a_missing_proc_gives_an_empty_list() {
        let mut sampler = ProcessSampler::with_root("/nonexistent/proc");
        assert!(sampler.sample().is_empty());
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_processes_are_sane_and_fast() {
        let mut sampler = ProcessSampler::new();
        let started = Instant::now();
        let first = sampler.sample();
        let first_time = started.elapsed();
        std::thread::sleep(Duration::from_millis(200));
        let mut times = Vec::new();
        let mut list = Vec::new();
        for _ in 0..5 {
            let started = Instant::now();
            list = sampler.sample();
            times.push(started.elapsed());
            std::thread::sleep(Duration::from_millis(100));
        }
        times.sort();
        println!(
            "{} processes: first sample {first_time:?}, later samples median {:?} (min {:?}, max {:?})",
            list.len(),
            times[times.len() / 2],
            times[0],
            times[times.len() - 1]
        );
        assert!(first.len() > 10);
        let me = list.iter().find(|p| p.pid == std::process::id()).expect("finds itself");
        println!("{me:#?}");
        assert_eq!(me.uid, unsafe { libc::getuid() });
        assert!(me.is_own());
        assert!(me.exe.is_some());
        assert!(!me.args.is_empty());
        assert!(me.memory > 0);
        assert!(me.threads >= 1);
        assert!(me.start_time <= SystemTime::now());
        assert!(SystemTime::now().duration_since(me.start_time).unwrap() < Duration::from_secs(600));
        assert!(list.iter().any(|p| p.kernel_thread && p.args.is_empty()));
        assert!(list.iter().all(|p| p.machine_cpu_percent <= 100.0 && p.cpu_percent >= 0.0));
        sort_processes(&mut list, SortKey::Cpu, true);
        for p in list.iter().take(8) {
            println!(
                "{:>7} {:<10} {:>6.1}% {:>5.1}% {:>8} KiB {:>3} {:?} {:?} {}",
                p.pid,
                p.user,
                p.cpu_percent,
                p.machine_cpu_percent,
                p.memory / 1024,
                p.threads,
                p.state,
                p.unit.as_ref().map(|u| (&u.name, u.manager)),
                p.display_name()
            );
        }
        let release_budget = Duration::from_millis(50);
        if !cfg!(debug_assertions) {
            assert!(times[times.len() / 2] < release_budget);
        }
    }
}
