//! Running CUPS's command-line tools: where they are ([`Tools`]), how each
//! one runs (environment, session, timeout), how their failures become
//! [`Error`]s, and the queries and actions built from them.

use super::parse::{self, ptype};
use super::{Device, Driver, DriverFilter, Job, Printer, PrinterState, PrintersState};
use crate::error::{Error, Result};
use crate::service::Stop;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a query (`lpstat`, `lpoptions`, `lpq`) may take.
pub(crate) const QUERY_TIMEOUT: Duration = Duration::from_secs(10);
/// How long discovery (`lpinfo -v`, `lpinfo -m`, `driverless`) may take.
pub(crate) const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long an action (`lpadmin`, `lp`, `cancel`, `cupsenable`…) may take.
pub(crate) const ACTION_TIMEOUT: Duration = Duration::from_secs(60);
/// How long adding a printer may take: with `-m everywhere`, `lpadmin`
/// asks the printer itself for its capabilities.
pub(crate) const ADD_TIMEOUT: Duration = Duration::from_secs(120);
/// What `lpinfo -v` is told to give its backends (`--timeout`).
const BACKEND_TIMEOUT: &str = "30";
/// The most output kept from one command.
const MAX_OUTPUT: u64 = 64 << 20;
/// How long to wait for a finished command's output pipes to close (they
/// stay open while something it started in the background lives on).
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// Where CUPS's command-line tools are, and the files this module reads
/// besides their output.
///
/// [`Tools::default`] names each tool without a directory, so it is looked
/// up in `PATH` when it runs (on NixOS, `/run/current-system/sw/bin` once
/// `services.printing.enable` is set). Tests point the tools at scripts.
#[derive(Clone, Debug)]
pub struct Tools {
    /// `lpstat`: printers, their state, jobs, the default.
    pub lpstat: PathBuf,
    /// `lpoptions`: a printer's attributes; the user's default printer.
    pub lpoptions: PathBuf,
    /// `lpinfo`: devices and drivers for adding a printer.
    pub lpinfo: PathBuf,
    /// `lpadmin`: adding, removing, sharing, the system default.
    pub lpadmin: PathBuf,
    /// `lp`: printing a file or the test page.
    pub lp: PathBuf,
    /// `lpq`: job titles.
    pub lpq: PathBuf,
    /// `cancel`: cancelling jobs.
    pub cancel: PathBuf,
    /// `cupsenable`: resuming a printer.
    pub cupsenable: PathBuf,
    /// `cupsdisable`: pausing a printer.
    pub cupsdisable: PathBuf,
    /// `cupsaccept`: accepting jobs.
    pub cupsaccept: PathBuf,
    /// `cupsreject`: rejecting jobs.
    pub cupsreject: PathBuf,
    /// cups-filters' `driverless`: IPP Everywhere and AirPrint printers on
    /// the network. Optional.
    pub driverless: PathBuf,
    /// `cups-config`: where CUPS keeps its data (the test page). Optional,
    /// and usually not installed outside development packages.
    pub cups_config: PathBuf,
    /// The user's lpoptions file, `~/.cups/lpoptions`, where `lpoptions -d`
    /// records their own default printer; `None` without a home directory.
    pub user_lpoptions: Option<PathBuf>,
    /// The user's default printer from the environment (`LPDEST`, else
    /// `PRINTER` unless it is `lp`), which CUPS prefers to the lpoptions
    /// file.
    pub env_default: Option<String>,
    /// CUPS's data directory from the environment (`CUPS_DATADIR`, which
    /// NixOS sets), where the test page is looked for after `cups-config`.
    pub cups_datadir: Option<PathBuf>,
    /// CUPS's test page, if known; `None` looks for it (see
    /// [`Tools::find_test_page`]).
    pub test_page: Option<PathBuf>,
}

impl Default for Tools {
    fn default() -> Tools {
        Tools::from_env()
    }
}

/// Where NixOS's CUPS keeps the programs it runs, `driverless` among them
/// (cups-filters isn't on `PATH` there).
const NIXOS_CUPS_BIN: &str = "/var/lib/cups/path/bin";

impl Tools {
    /// The tools from `PATH`, the lpoptions file under `$HOME`, the default
    /// printer from `LPDEST`/`PRINTER` and the data directory from
    /// `CUPS_DATADIR`.
    pub fn from_env() -> Tools {
        let mut tools = Tools::named(|name| PathBuf::from(name));
        if which(OsStr::new("driverless")).is_none() {
            let nixos = Path::new(NIXOS_CUPS_BIN).join("driverless");
            if nixos.is_file() {
                tools.driverless = nixos;
            }
        }
        tools.user_lpoptions = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".cups/lpoptions"));
        let lpdest = std::env::var("LPDEST").ok();
        let printer = std::env::var("PRINTER").ok().filter(|p| p != "lp");
        tools.env_default = lpdest.or(printer).filter(|name| !name.is_empty());
        tools.cups_datadir = std::env::var_os("CUPS_DATADIR").filter(|dir| !dir.is_empty()).map(PathBuf::from);
        tools
    }

    /// Every tool in `dir` (named as CUPS names them), with no user
    /// lpoptions file and no default from the environment.
    pub fn in_dir(dir: impl AsRef<Path>) -> Tools {
        let dir = dir.as_ref();
        Tools::named(|name| dir.join(name))
    }

    fn named(path: impl Fn(&str) -> PathBuf) -> Tools {
        Tools {
            lpstat: path("lpstat"),
            lpoptions: path("lpoptions"),
            lpinfo: path("lpinfo"),
            lpadmin: path("lpadmin"),
            lp: path("lp"),
            lpq: path("lpq"),
            cancel: path("cancel"),
            cupsenable: path("cupsenable"),
            cupsdisable: path("cupsdisable"),
            cupsaccept: path("cupsaccept"),
            cupsreject: path("cupsreject"),
            driverless: path("driverless"),
            cups_config: path("cups-config"),
            user_lpoptions: None,
            env_default: None,
            cups_datadir: None,
            test_page: None,
        }
    }

    /// CUPS's test page, `testprint` in its data directory: from
    /// [`test_page`](Tools::test_page) if set, else `cups-config --datadir`,
    /// [`cups_datadir`](Tools::cups_datadir), the `share/cups` beside the `lp` or `lpstat`
    /// binary (following links to its store path), the `DataDir` of
    /// `cups-files.conf` (where NixOS points it into the CUPS environment),
    /// and finally `/usr/share/cups` and `/usr/local/share/cups`. Blocks
    /// while `cups-config` runs.
    pub fn find_test_page(&self) -> Option<PathBuf> {
        if let Some(page) = &self.test_page {
            return page.is_file().then(|| page.clone());
        }
        const PAGE: &str = "data/testprint";
        let page_in = |dir: PathBuf| Some(dir.join(PAGE)).filter(|page| page.is_file());
        let from_config = || {
            let output = run(&self.cups_config, &["--datadir"], QUERY_TIMEOUT, None).ok()?;
            let dir = output.stdout_text().trim().to_owned();
            (output.success() && dir.starts_with('/')).then(|| PathBuf::from(dir))
        };
        let beside = |tool: &Path| {
            let binary = resolve(tool)?.canonicalize().ok()?;
            Some(binary.parent()?.parent()?.join("share/cups"))
        };
        let from_conf = || {
            let roots = std::env::var_os("CUPS_SERVERROOT")
                .map(PathBuf::from)
                .into_iter()
                .chain(["/etc/cups", "/var/lib/cups"].into_iter().map(PathBuf::from));
            roots
                .filter_map(|root| std::fs::read_to_string(root.join("cups-files.conf")).ok())
                .filter_map(|conf| parse::conf_path(&conf, "DataDir"))
                .find_map(page_in)
        };
        from_config()
            .and_then(page_in)
            .or_else(|| self.cups_datadir.clone().and_then(page_in))
            .or_else(|| beside(&self.lp).and_then(page_in))
            .or_else(|| beside(&self.lpstat).and_then(page_in))
            .or_else(from_conf)
            .or_else(|| page_in(PathBuf::from("/usr/share/cups")))
            .or_else(|| page_in(PathBuf::from("/usr/local/share/cups")))
    }
}

/// `tool` itself if it names a directory, else the first match in `PATH`.
fn resolve(tool: &Path) -> Option<PathBuf> {
    if tool.components().count() > 1 {
        return Some(tool.to_owned());
    }
    which(tool.as_os_str())
}

fn which(name: &OsStr) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|candidate| candidate.is_file())
}

/// What a command printed, and how it ended.
#[derive(Clone, Debug, Default)]
pub(crate) struct Output {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: String,
    /// The exit code; `None` when a signal ended it.
    pub(crate) status: Option<i32>,
}

impl Output {
    pub(crate) fn success(&self) -> bool {
        self.status == Some(0)
    }

    /// Standard output as text (invalid UTF-8 replaced).
    pub(crate) fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// The name a tool reports itself by (its file name), for messages.
pub(crate) fn program_name(program: &Path) -> String {
    program.file_name().unwrap_or(program.as_os_str()).to_string_lossy().into_owned()
}

/// Run `program` with `args` and return what it printed, whatever its exit
/// status.
///
/// It runs with `LC_ALL=C` and `LANG=C` (untranslated messages, C-locale
/// dates; CUPS still passes UTF-8 names and descriptions through), stdin
/// from `/dev/null`, and in a session of its own: with no controlling
/// terminal, a tool that wants a password (CUPS's `cupsGetPassword` reads
/// `/dev/tty`) fails at once instead of waiting on the terminal the app was
/// started from, and its whole process group can be killed. It is killed
/// after `timeout` ([`Error::Timeout`]), or as soon as `stop` is set
/// ([`Error::Cancelled`]).
pub(crate) fn run<A: AsRef<OsStr>>(
    program: &Path,
    args: &[A],
    timeout: Duration,
    stop: Option<&Stop>,
) -> Result<Output> {
    if stop.is_some_and(|stop| stop.stopped()) {
        return Err(Error::Cancelled);
    }
    let name = program_name(program);
    let mut command = Command::new(program);
    command
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env_remove("LANGUAGE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid is async-signal-safe and touches no memory of this
    // process; its only failure (already a group leader) is harmless.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = spawn(&mut command).map_err(|error| spawn_error(&name, &error))?;

    let (sender, receiver) = mpsc::channel::<(bool, Vec<u8>)>();
    let pipes: [(bool, Option<Box<dyn Read + Send>>); 2] = [
        (true, child.stdout.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>)),
        (false, child.stderr.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>)),
    ];
    let mut readers = 0;
    for (is_stdout, pipe) in pipes {
        let Some(pipe) = pipe else { continue };
        let sender = sender.clone();
        let spawned = std::thread::Builder::new().name("aizome-sys printers pipe".into()).spawn(move || {
            let mut buffer = Vec::new();
            // A read error keeps what came before it.
            let _ = pipe.take(MAX_OUTPUT).read_to_end(&mut buffer);
            let _ = sender.send((is_stdout, buffer));
        });
        if let Err(error) = spawned {
            kill(&mut child);
            return Err(Error::Failed(format!("could not start a thread for {name}: {error}")));
        }
        readers += 1;
    }
    drop(sender);

    let started = Instant::now();
    let mut pause = Duration::from_millis(1);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                kill(&mut child);
                return Err(Error::Io(format!("{name}: {error}")));
            }
        }
        if stop.is_some_and(|stop| stop.stopped()) {
            kill(&mut child);
            return Err(Error::Cancelled);
        }
        if started.elapsed() >= timeout {
            kill(&mut child);
            log::debug!("aizome-sys: {name} did not finish in {timeout:?}");
            return Err(Error::Timeout);
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(Duration::from_millis(20));
    };

    let mut output = Output { status: status.code(), ..Output::default() };
    let deadline = Instant::now() + PIPE_GRACE;
    for _ in 0..readers {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok((true, bytes)) => output.stdout = bytes,
            Ok((false, bytes)) => output.stderr = String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => break,
        }
    }
    Ok(output)
}

/// Spawn, retrying briefly when the program is a script another thread
/// has just written ("text file busy").
fn spawn(command: &mut Command) -> std::io::Result<Child> {
    let mut tries = 0;
    loop {
        match command.spawn() {
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) && tries < 20 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(5));
            }
            result => return result,
        }
    }
}

/// Kill the child's process group (and the child, should it not have
/// reached `setsid`), and reap it.
fn kill(child: &mut Child) {
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: the child is not reaped yet, so its pid (and the process
        // group it leads) can't belong to anyone else.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_error(name: &str, error: &std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            Error::Unavailable(format!("{name} is not installed (CUPS's command-line tools)"))
        }
        std::io::ErrorKind::PermissionDenied => Error::Unavailable(format!("{name} can't be run: {error}")),
        _ => Error::Io(format!("{name}: {error}")),
    }
}

/// Run `program`, and turn a failure exit into an [`Error`].
pub(crate) fn checked<A: AsRef<OsStr>>(
    program: &Path,
    args: &[A],
    timeout: Duration,
    stop: Option<&Stop>,
) -> Result<Output> {
    let output = run(program, args, timeout, stop)?;
    if output.success() { Ok(output) } else { Err(failure(&program_name(program), &output)) }
}

/// The error a failed command reported, sorted by what it says.
pub(crate) fn failure(program: &str, output: &Output) -> Error {
    let stdout = output.stdout_text();
    let said = [output.stderr.trim(), stdout.trim()].into_iter().find(|text| !text.is_empty());
    let message = match said {
        Some(text) => text
            .lines()
            .map(|line| line.trim().strip_prefix(program).and_then(|l| l.strip_prefix(':')).unwrap_or(line))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        None => match output.status {
            Some(code) => format!("exited with status {code}"),
            None => "was killed by a signal".to_owned(),
        },
    };
    classify(program, message)
}

/// Sort a tool's error message into an [`Error`]: refusals into
/// [`Error::NotAuthorized`], a scheduler that isn't there into
/// [`Error::Unavailable`], unknown printers, jobs and files into
/// [`Error::NotFound`], the rest into [`Error::Command`].
pub(crate) fn classify(program: &str, message: String) -> Error {
    let lower = message.to_lowercase();
    let says = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    if says(&["forbidden", "not authorized", "unauthorized", "not-authorized", "authentication"]) {
        Error::NotAuthorized(message)
    } else if says(&[
        "unable to connect to server",
        "unable to contact server",
        "scheduler is not running",
        "connection refused",
    ]) {
        Error::Unavailable(message)
    } else if says(&[
        "does not exist",
        "unknown destination",
        "invalid destination",
        "unknown printer",
        "no such printer",
        "no such job",
        "not found",
        "unable to access",
        "unable to open ppd",
    ]) {
        Error::NotFound(message)
    } else if says(&["no such file"]) {
        Error::Unavailable(message)
    } else {
        Error::Command { program: program.to_owned(), message }
    }
}

/// `lpstat ARGS`'s output; "No destinations added" (with no printers) is
/// an empty list, not a failure.
fn lpstat(tools: &Tools, args: &[&str], stop: Option<&Stop>) -> Result<String> {
    let output = run(&tools.lpstat, args, QUERY_TIMEOUT, stop)?;
    let text = output.stdout_text();
    if output.success() {
        Ok(text)
    } else if output.stderr.contains("No destinations added") || text.contains("No destinations added") {
        Ok(String::new())
    } else {
        Err(failure(&program_name(&tools.lpstat), &output))
    }
}

/// Everything the printers snapshot holds, read from CUPS now.
///
/// `lpstat -r` must say the scheduler is running ([`Error::Unavailable`]
/// otherwise); `lpstat -l -p`, `-a`, `-v` and `-W not-completed -l -o`
/// must work. The rest only adds detail and is skipped when it fails:
/// `lpstat -d`, `lpoptions -p` per printer, `lpq -a` for job titles.
pub(crate) fn query(tools: &Tools, stop: Option<&Stop>) -> Result<PrintersState> {
    let scheduler = run(&tools.lpstat, &["-r"], QUERY_TIMEOUT, stop)?;
    let said = format!("{}{}", scheduler.stdout_text(), scheduler.stderr);
    if parse::scheduler_running(&said) != Some(true) {
        return Err(Error::Unavailable("the CUPS scheduler is not running".into()));
    }

    let status = parse::printer_status(&lpstat(tools, &["-l", "-p"], stop)?);
    let accepting = parse::accepting(&lpstat(tools, &["-a"], stop)?);
    let devices = parse::device_uris(&lpstat(tools, &["-v"], stop)?);
    let mut jobs = parse::jobs(&lpstat(tools, &["-W", "not-completed", "-l", "-o"], stop)?);
    let effective_default = match lpstat(tools, &["-d"], stop) {
        Ok(text) => parse::default_destination(&text),
        Err(error) => {
            log::debug!("aizome-sys: lpstat -d: {error}");
            None
        }
    };

    let mut server_default = None;
    let mut printers = Vec::with_capacity(status.len());
    for entry in status {
        let options = printer_options(tools, &entry.name, stop);
        let printer = assemble(entry, &accepting, &devices, &options);
        let printer_type = options.get("printer-type").and_then(|t| t.parse::<u32>().ok());
        if printer_type.is_some_and(|t| t & ptype::DEFAULT != 0) {
            server_default = Some(printer.name.clone());
        }
        printers.push(printer);
    }

    let user_default = tools.env_default.clone().or_else(|| {
        let file = std::fs::read_to_string(tools.user_lpoptions.as_ref()?).ok()?;
        parse::lpoptions_default(&file)
    });
    // `lpstat -d` reports the user's own default when they have one; the
    // system's is then the queue CUPS marks as its default.
    let system_default = if user_default.is_none() { effective_default.or(server_default) } else { server_default };
    for printer in &mut printers {
        printer.is_system_default = same_name(system_default.as_deref(), &printer.name);
        printer.is_user_default = same_name(user_default.as_deref(), &printer.name);
    }

    let titles = if jobs.is_empty() {
        HashMap::new()
    } else {
        match checked(&tools.lpq, &["-a"], QUERY_TIMEOUT, stop) {
            Ok(output) => parse::lpq_titles(&output.stdout),
            Err(error) => {
                log::debug!("aizome-sys: lpq -a: {error}");
                HashMap::new()
            }
        }
    };
    for job in &mut jobs {
        let printing = printers.iter().any(|p| p.current_job.as_deref() == Some(job.id.as_str()));
        job.state = parse::job_state(&job.reasons, printing, false);
        job.title = titles.get(&job.number).cloned();
    }

    Ok(PrintersState { loaded: true, available: true, printers, jobs, system_default, user_default, error: None })
}

/// CUPS names are compared without regard to case.
fn same_name(name: Option<&str>, printer: &str) -> bool {
    name.is_some_and(|name| name.eq_ignore_ascii_case(printer))
}

/// What `lpoptions -p NAME` says about a printer; nothing if it fails.
fn printer_options(tools: &Tools, name: &str, stop: Option<&Stop>) -> HashMap<String, String> {
    let args = [OsStr::new("-p"), OsStr::new(name)];
    match checked(&tools.lpoptions, &args, QUERY_TIMEOUT, stop) {
        Ok(output) => parse::options(&output.stdout_text()),
        Err(error) => {
            log::debug!("aizome-sys: lpoptions -p {name}: {error}");
            HashMap::new()
        }
    }
}

/// A [`Printer`] from `lpstat`'s view of it and `lpoptions`'.
fn assemble(
    entry: parse::StatusEntry,
    accepting: &HashMap<String, bool>,
    devices: &HashMap<String, String>,
    options: &HashMap<String, String>,
) -> Printer {
    let option = |key: &str| options.get(key).map(String::as_str).filter(|value| !value.is_empty());
    let flag = |key: &str| match option(key) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let printer_type = option("printer-type").and_then(|t| t.parse::<u32>().ok());
    let has_bit = |bit: u32| printer_type.is_some_and(|t| t & bit != 0);

    let make_and_model = option("printer-make-and-model").map(str::to_owned);
    let device_uri = devices.get(&entry.name).cloned().or_else(|| option("device-uri").map(str::to_owned));
    let temporary = flag("printer-is-temporary").unwrap_or(false) || has_bit(ptype::DISCOVERED);
    let reasons = if entry.reasons.is_empty() {
        option("printer-state-reasons").map(parse::words).unwrap_or_default()
    } else {
        entry.reasons
    };
    let driverless = is_driverless(make_and_model.as_deref(), device_uri.as_deref(), temporary);
    Printer {
        description: entry
            .description
            .filter(|d| !d.is_empty())
            .or_else(|| option("printer-info").map(str::to_owned))
            .unwrap_or_default(),
        location: entry
            .location
            .filter(|l| !l.is_empty())
            .or_else(|| option("printer-location").map(str::to_owned))
            .unwrap_or_default(),
        make_and_model,
        device_uri,
        state: entry.state,
        state_since: entry.since,
        state_message: entry.message,
        state_reasons: reasons,
        current_job: entry.current_job,
        accepting: accepting.get(&entry.name).copied().or_else(|| flag("printer-is-accepting-jobs")).unwrap_or(true),
        enabled: entry.state != PrinterState::Stopped,
        shared: flag("printer-is-shared").or_else(|| printer_type.map(|t| t & ptype::NOT_SHARED == 0)),
        is_class: has_bit(ptype::CLASS),
        remote: entry.remote || has_bit(ptype::REMOTE),
        temporary,
        driverless,
        is_system_default: false,
        is_user_default: false,
        name: entry.name,
    }
}

/// Whether a queue prints without a vendor driver: CUPS's IPP Everywhere
/// driver says so in the make and model (`… - IPP Everywhere`), as do
/// cups-filters' (`…, driverless, cups-filters 2.0`); cups-browsed's
/// `implicitclass://` queues and CUPS's temporary queues are driverless
/// too. `None` when nothing says either way.
fn is_driverless(make_and_model: Option<&str>, device_uri: Option<&str>, temporary: bool) -> Option<bool> {
    let model = make_and_model.map(str::to_lowercase);
    if temporary
        || device_uri.is_some_and(|uri| uri.starts_with("implicitclass:"))
        || model.as_deref().is_some_and(|m| m.contains("ipp everywhere") || m.contains("driverless"))
    {
        Some(true)
    } else if model.is_some() {
        Some(false)
    } else {
        None
    }
}

/// Jobs CUPS still remembers that are done: completed, cancelled or
/// aborted (`lpstat -W completed -l -o`).
pub(crate) fn completed_jobs(tools: &Tools) -> Result<Vec<Job>> {
    let text = lpstat(tools, &["-W", "completed", "-l", "-o"], None)?;
    let mut jobs = parse::jobs(&text);
    for job in &mut jobs {
        job.state = parse::job_state(&job.reasons, false, true);
    }
    Ok(jobs)
}

/// Devices to add a printer on: `lpinfo -l -v` (every backend's devices,
/// with `--timeout` for the slow ones) and, alongside, `driverless list`
/// (IPP Everywhere and AirPrint printers). A printer both list is one
/// device, marked driverless. Fails only when neither works.
pub(crate) fn discover(tools: &Tools) -> Result<Vec<Device>> {
    let driverless_tool = tools.driverless.clone();
    let driverless = std::thread::Builder::new()
        .name("aizome-sys printers driverless".into())
        .spawn(move || checked(&driverless_tool, &["list"], DISCOVERY_TIMEOUT, None));
    let lpinfo = checked(&tools.lpinfo, &["-l", "--timeout", BACKEND_TIMEOUT, "-v"], DISCOVERY_TIMEOUT, None)
        .map(|output| parse::devices(&output.stdout_text()));
    let driverless = match driverless {
        Ok(thread) => thread.join().unwrap_or_else(|_| Err(Error::Failed("driverless panicked".into()))),
        Err(error) => Err(Error::Failed(format!("could not start a thread: {error}"))),
    }
    .map(|output| parse::driverless(&output.stdout_text()));
    if let Err(error) = &driverless {
        log::debug!("aizome-sys: driverless: {error}");
    }
    match (lpinfo, driverless) {
        (Ok(devices), Ok(found)) => Ok(merge_driverless(devices, found)),
        (Ok(devices), Err(_)) => Ok(merge_driverless(devices, Vec::new())),
        (Err(_), Ok(found)) if !found.is_empty() => Ok(found),
        (Err(error), _) => Err(error),
    }
}

/// `lpinfo`'s devices with `driverless`' merged in: a printer `driverless`
/// found under the same DNS-SD name (or URI) marks lpinfo's device as
/// driverless and fills in what it lacks; the others are added. Without
/// `driverless`, an IPP device whose device ID lists PWG or Apple raster
/// counts as driverless.
pub(crate) fn merge_driverless(mut devices: Vec<Device>, found: Vec<Device>) -> Vec<Device> {
    for device in &mut devices {
        let ipp = device.uri.starts_with("ipp") || (device.uri.starts_with("dnssd://") && device.uri.contains("._ipp"));
        if ipp && device.device_id.as_deref().is_some_and(parse::speaks_driverless) {
            device.driverless = true;
        }
    }
    for extra in found {
        let name = parse::service_name(&extra.uri);
        let mut matched = false;
        for device in &mut devices {
            let same = device.uri == extra.uri
                || name.as_ref().is_some_and(|name| {
                    parse::service_name(&device.uri).is_some_and(|other| other.eq_ignore_ascii_case(name))
                });
            if same {
                matched = true;
                device.driverless = true;
                if device.make_and_model.is_none() {
                    device.make_and_model.clone_from(&extra.make_and_model);
                }
                if device.device_id.is_none() {
                    device.device_id.clone_from(&extra.device_id);
                }
            }
        }
        if !matched {
            devices.push(extra);
        }
    }
    devices
}

/// The drivers `lpinfo -l -m` lists for `filter` (with `--device-id` and
/// `--make-and-model`, which CUPS matches itself).
pub(crate) fn drivers(tools: &Tools, filter: &DriverFilter) -> Result<Vec<Driver>> {
    let mut args = vec!["-l"];
    if let Some(device_id) = filter.device_id.as_deref() {
        args.extend(["--device-id", device_id]);
    }
    if let Some(make_and_model) = filter.make_and_model.as_deref() {
        args.extend(["--make-and-model", make_and_model]);
    }
    if let Some(language) = filter.language.as_deref() {
        args.extend(["--language", language]);
    }
    args.push("-m");
    let output = checked(&tools.lpinfo, &args, DISCOVERY_TIMEOUT, None)?;
    Ok(parse::drivers(&output.stdout_text()).collect())
}

/// Run an action's tool; a failure exit becomes an error.
pub(crate) fn act(program: &Path, args: &[OsString], timeout: Duration) -> Result<()> {
    checked(program, args, timeout, None).map(|_| ())
}

/// Print `file` on `printer` with `lp`, titled `title`; the job id `lp`
/// reports.
pub(crate) fn print(tools: &Tools, printer: &str, file: &Path, title: Option<&str>) -> Result<String> {
    let mut args = vec![OsString::from("-d"), OsString::from(printer)];
    if let Some(title) = title {
        args.extend([OsString::from("-t"), OsString::from(title)]);
    }
    args.push(file.into());
    let output = checked(&tools.lp, &args, ACTION_TIMEOUT, None)?;
    parse::request_id(&output.stdout_text())
        .or_else(|| parse::request_id(&output.stderr))
        .ok_or_else(|| Error::Failed(format!("lp printed no job id: {}", output.stdout_text().trim())))
}

/// Remove the default from the user's lpoptions file, keeping each
/// printer's saved options: CUPS's tools can set it (`lpoptions -d`) but
/// not unset it. Written to a new file renamed over the old one.
pub(crate) fn clear_user_default(tools: &Tools) -> Result<()> {
    let Some(path) = tools.user_lpoptions.as_deref() else { return Ok(()) };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let without = parse::without_default(&text);
    if without == text {
        return Ok(());
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".aizome-new");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, without)?;
    let permissions = std::fs::metadata(path).map(|m| m.permissions());
    let renamed = permissions
        .and_then(|permissions| std::fs::set_permissions(&temporary, permissions))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = renamed {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}
