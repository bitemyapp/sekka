//! Launching applications from their desktop entries.
//!
//! **What runs**: the entry's (or the chosen action's) `Exec`, expanded
//! with the targets (see [`Exec`]); several processes when it takes one
//! file or URL (`%f`/`%u`) and several were given. `Terminal=true` puts
//! [`LaunchOptions::terminal`] in front (default `aizome-hotaru --`; an index's
//! [`terminal_command`](super::AppIndex::terminal_command) gives the
//! person's choice). The working directory is the override, else the
//! entry's `Path=`, else `$HOME`. The program must exist (in `PATH`, or as
//! a path) before anything starts.
//!
//! **How**: through the systemd user manager, so each app gets its own
//! scope as the XDG standardization of applications in cgroups asks
//! (`app-<launcher>-<ApplicationID>-<RANDOM>.scope`, here
//! `app-aizome-<escaped ID>-<16 hex digits>.scope`, the ID escaped as
//! `systemd-escape` does, `-` becoming `\x2d`):
//!
//! ```text
//! systemd-run --user --scope --unit=… --description=<Name> --collect \
//!     [--expand-environment=no] -- /bin/sh -c 'exec 2>&1; "$@" & wait "$!"' aizome-launch argv…
//! ```
//!
//! With `--scope`, systemd-run asks the manager for a scope holding its own
//! process, prints `Running as unit: <unit>` on stderr and then execs the
//! command, so the waiting shell is our child and `systemd-run` never
//! "returns". [`launch`] therefore only waits for that line (milliseconds),
//! then hands the child to a small thread that reaps it when it exits. The
//! program's output goes to the journal (a journald stream tagged with the
//! app's ID, or the observer's unique launch ID; `/dev/null` without journald):
//! that is its stdout, and the `sh` wrapper points its stderr there too,
//! closing the pipe the line came through; without that the app would hold
//! a pipe to the launcher and die of `SIGPIPE` when the launcher went away.
//! systemd 254+ expands `$VAR` in scope command lines unless told not to,
//! hence `--expand-environment=no` (when the version has it).
//! The waiting shell stays until the app exits, including on the plain
//! route. It gives sandboxes such as Bubblewrap with `--die-with-parent`
//! a parent that survives restarting Aizome's shell.
//!
//! **Fallback**: a plain spawn when `systemd-run` isn't in `PATH`, the user
//! manager isn't reachable (`$XDG_RUNTIME_DIR/systemd/private` missing), or
//! systemd-run fails (exits without starting the scope, or says nothing
//! for 5 seconds, in which case it is killed).
//!
//! **Either way** the process starts in its own session (`setsid`, so a
//! terminal's Ctrl-C or the launcher's process group don't reach it), with
//! stdin on `/dev/null` and output in the journal, and a thread reaps it, so no
//! zombies are left. `XDG_ACTIVATION_TOKEN` and `DESKTOP_STARTUP_ID` are
//! never inherited from the launcher; with an activation token both are
//! set to it.
//!
//! **D-Bus activation**: a `DBusActivatable=true` entry is started through
//! `org.freedesktop.Application` on the session bus ([`activate_dbus`]:
//! `Activate`, `Open` with the targets as URIs, or `ActivateAction`, the
//! activation token in the platform data), as the specification asks;
//! when that fails its `Exec` runs as above (one without `Exec` is then
//! refused with [`Error::Unavailable`]).
//!
//! Not handled here: `Type=Link` entries (open their `URL` with the default
//! browser).

use aizome_apps::{DesktopEntry, EntryKind, Exec, ExecContext, find_program, random_hex};
use crate::{Error, Result, Task, Wake};
use std::collections::HashMap;
use std::io::BufRead;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long systemd-run may take to start a scope before it is killed and
/// the program spawned directly.
const SYSTEMD_TIMEOUT: Duration = Duration::from_secs(5);

/// The launcher name in unit names.
const LAUNCHER: &str = "aizome";

/// What to launch an entry with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchOptions {
    /// Files and URIs to open: absolute paths (relative ones are taken
    /// from the current directory) or URIs.
    pub targets: Vec<String>,
    /// An action ID from the entry's `Actions` to run instead of its main
    /// `Exec`.
    pub action: Option<String>,
    /// An `xdg-activation-v1` token for the new app's first window; passed
    /// as `XDG_ACTIVATION_TOKEN` and `DESKTOP_STARTUP_ID`.
    pub activation_token: Option<String>,
    /// The working directory, instead of the entry's `Path=` or `$HOME`.
    pub working_dir: Option<PathBuf>,
    /// The command a `Terminal=true` program is appended to.
    pub terminal: Vec<String>,
    /// Run through `systemd-run --user --scope` when the user manager is
    /// there (default); `false` always spawns directly.
    pub use_systemd: bool,
    /// Start `DBusActivatable=true` apps through D-Bus activation on the
    /// session bus (default), as the spec asks; their `Exec` is the
    /// fallback.
    pub use_dbus_activation: bool,
}

impl Default for LaunchOptions {
    fn default() -> LaunchOptions {
        LaunchOptions {
            targets: Vec::new(),
            action: None,
            activation_token: None,
            working_dir: None,
            terminal: vec!["aizome-hotaru".to_owned(), "--".to_owned()],
            use_systemd: true,
            use_dbus_activation: true,
        }
    }
}

/// A started process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launched {
    /// The waiting shell's process ID. The application is its child.
    pub pid: u32,
    /// The systemd scope it runs in, if launched through systemd.
    pub unit: Option<String>,
    /// What ran.
    pub argv: Vec<String>,
}

/// Process creation and exit are distinct from a window becoming ready.
#[derive(Clone, Debug)]
pub enum LaunchEvent {
    Started(Launched),
    Exited { pid: u32, status: std::result::Result<std::process::ExitStatus, String> },
    Activated,
    Message(String),
}

/// Observe a launch without keeping its output connected to the desktop
/// shell. Output goes straight to journald under this attempt's unique ID,
/// so replacing the shell cannot break a running application's stdout.
#[derive(Clone)]
pub struct LaunchObserver {
    journal_id: String,
    send: Arc<dyn Fn(LaunchEvent) + Send + Sync>,
}

impl LaunchObserver {
    pub fn new(send: impl Fn(LaunchEvent) + Send + Sync + 'static) -> Self {
        Self { journal_id: format!("aizome-launch-{}", random_hex(16)), send: Arc::new(send) }
    }

    pub fn journal_id(&self) -> &str {
        &self.journal_id
    }

    fn emit(&self, event: LaunchEvent) {
        (self.send)(event);
    }
}

/// How to start processes; tests point `Systemd` at fakes.
#[derive(Clone, Debug)]
enum Backend {
    Plain,
    Systemd { command: Vec<String>, timeout: Duration },
}

/// Launch `entry`: one [`Launched`] per process started (usually one).
/// Returns once each process has started, which through systemd means a
/// few milliseconds; use [`launch_in_background`] to not wait at all.
///
/// A `DBusActivatable=true` app is activated over the session bus
/// instead ([`activate_dbus`]), and the result is then empty: the bus
/// starts the process, or the running instance opens a window.
pub fn launch(entry: &DesktopEntry, options: &LaunchOptions) -> Result<Vec<Launched>> {
    launch_observed(entry, options, None)
}

/// Like [`launch`], with asynchronous lifecycle events and isolated journal
/// output. An accepted D-Bus request is reported as such, not as a window.
pub fn launch_observed(
    entry: &DesktopEntry,
    options: &LaunchOptions,
    observer: Option<&LaunchObserver>,
) -> Result<Vec<Launched>> {
    if entry.dbus_activatable && options.use_dbus_activation && entry.kind == EntryKind::Application {
        match crate::Bus::session().and_then(|bus| activate_dbus(entry, options, &bus)) {
            Ok(()) => {
                if let Some(o) = observer {
                    o.emit(LaunchEvent::Activated);
                }
                return Ok(Vec::new());
            }
            Err(error) => {
                let message = format!("D-Bus activation of {} failed ({error}); running its Exec", entry.id);
                log::info!("apps: {message}");
                if let Some(o) = observer {
                    o.emit(LaunchEvent::Message(message));
                }
            }
        }
    }
    let backend = if options.use_systemd { systemd_backend() } else { None };
    launch_with_observer(entry, options, &backend.unwrap_or(Backend::Plain), observer)
}

/// The object path `org.freedesktop.Application` lives at for an app ID,
/// per the Desktop Entry spec: `/` for `.` and `_` for `-`.
pub fn dbus_object_path(app_id: &str) -> String {
    format!("/{}", app_id.replace('.', "/").replace('-', "_"))
}

/// Activate a `DBusActivatable=true` app through `org.freedesktop.Application`
/// on `bus` (its well-known name is its app ID): `ActivateAction` for an
/// action, `Open` with targets (as URIs), else `Activate`; the activation
/// token goes in the platform data. The call waits for the app (up to the
/// action timeout), which the bus starts if it isn't running.
pub fn activate_dbus(entry: &DesktopEntry, options: &LaunchOptions, bus: &crate::Bus) -> Result<()> {
    use zbus::zvariant::Value;
    let name = entry.app_id();
    if zbus::names::WellKnownName::try_from(name).is_err() {
        return Err(Error::InvalidArgument(format!("{name} is not a D-Bus name")));
    }
    let path = dbus_object_path(name);
    let mut platform: HashMap<&str, Value> = HashMap::new();
    if let Some(token) = options.activation_token.as_deref().filter(|t| !t.is_empty()) {
        platform.insert("activation-token", Value::from(token));
        platform.insert("desktop-startup-id", Value::from(token));
    }
    let opts = crate::bus::Opts::ACTION;
    const IFACE: &str = "org.freedesktop.Application";
    match (&options.action, options.targets.is_empty()) {
        (Some(action), _) => {
            let parameter: Vec<Value> = Vec::new();
            bus.call_with(opts, name, &path, IFACE, "ActivateAction", &(action.as_str(), parameter, platform))
        }
        (None, false) => {
            let uris: Vec<String> = options.targets.iter().map(|t| target_uri(t)).collect();
            bus.call_with(opts, name, &path, IFACE, "Open", &(uris, platform))
        }
        (None, true) => bus.call_with(opts, name, &path, IFACE, "Activate", &(platform,)),
    }
}

/// A target as a URI: URIs as they are, paths as `file://` URIs.
fn target_uri(target: &str) -> String {
    if target.contains("://") || target.starts_with("mailto:") {
        return target.to_owned();
    }
    let path = Path::new(target);
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_owned())
    };
    aizome_apps::path_to_file_uri(&absolute)
}

/// [`launch`] on a new thread; `wake` is called when the result is ready.
pub fn launch_in_background(entry: DesktopEntry, options: LaunchOptions, wake: &Wake) -> Task<Vec<Launched>> {
    Task::spawn("launch", wake, move || launch(&entry, &options))
}

/// The argument vectors [`launch`] would run for `entry` (one per
/// process), terminal prefix included, without checking the programs
/// exist.
pub fn command_lines(entry: &DesktopEntry, options: &LaunchOptions) -> Result<Vec<Vec<String>>> {
    if entry.kind != EntryKind::Application {
        return Err(Error::InvalidArgument(format!("{} is not an application", entry.id)));
    }
    let (exec, icon) = match &options.action {
        Some(id) => {
            let action = entry.action(id).ok_or_else(|| Error::NotFound(format!("{} has no action {id}", entry.id)))?;
            (action.exec.as_deref(), action.icon.as_deref().or(entry.icon.as_deref()))
        }
        None => (entry.exec.as_deref(), entry.icon.as_deref()),
    };
    let Some(exec) = exec else {
        return Err(if entry.dbus_activatable {
            Error::Unavailable(format!("{} can only be started through D-Bus activation", entry.id))
        } else {
            Error::InvalidArgument(format!("{} has no Exec", entry.id))
        });
    };
    let exec = Exec::parse(exec)?;
    let context = ExecContext { name: &entry.name, icon, location: entry.path.as_deref() };
    let mut argvs = exec.expand_each(&options.targets, &context);
    if entry.terminal {
        if options.terminal.is_empty() {
            return Err(Error::InvalidArgument("no terminal to run it in".into()));
        }
        for argv in &mut argvs {
            argv.splice(0..0, options.terminal.iter().cloned());
        }
    }
    Ok(argvs)
}

#[cfg(test)]
fn launch_with(entry: &DesktopEntry, options: &LaunchOptions, backend: &Backend) -> Result<Vec<Launched>> {
    launch_with_observer(entry, options, backend, None)
}

fn launch_with_observer(
    entry: &DesktopEntry,
    options: &LaunchOptions,
    backend: &Backend,
    observer: Option<&LaunchObserver>,
) -> Result<Vec<Launched>> {
    let argvs = command_lines(entry, options)?;
    launch_commands(entry, argvs, options, backend, observer)
}

/// Launch an already parsed command, without passing it through a shell's
/// parser or desktop-entry field-code expansion. `app_id` is the app the
/// caller knows the command runs, if any; its scope is named for that app,
/// else for the program, as an app's own launch would be.
pub fn launch_command_observed(
    argv: &[String],
    app_id: Option<&str>,
    options: &LaunchOptions,
    observer: &LaunchObserver,
) -> Result<Vec<Launched>> {
    let entry = command_entry(argv, app_id)?;
    let backend = if options.use_systemd { systemd_backend() } else { None };
    launch_commands(&entry, vec![argv.to_vec()], options, &backend.unwrap_or(Backend::Plain), Some(observer))
}

/// The stand-in entry a command launches as: only its ID (which names the
/// scope) and its name come from the command; nothing else of an app's
/// entry (its folder, its terminal) applies.
fn command_entry(argv: &[String], app_id: Option<&str>) -> Result<DesktopEntry> {
    let program = argv.first().map(String::as_str).unwrap_or("");
    let program = program.rsplit('/').next().unwrap_or(program);
    let id = match app_id.filter(|id| !id.is_empty()) {
        Some(id) => format!("{id}.desktop"),
        None if !program.is_empty() => program.to_owned(),
        None => "command".to_owned(),
    };
    let mut entry = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=Command\nExec=true\n",
        &id,
        &aizome_apps::Locale::parse("C"),
    )?;
    entry.name = argv.first().cloned().unwrap_or_else(|| "Command".into());
    Ok(entry)
}

fn launch_commands(
    entry: &DesktopEntry,
    argvs: Vec<Vec<String>>,
    options: &LaunchOptions,
    backend: &Backend,
    observer: Option<&LaunchObserver>,
) -> Result<Vec<Launched>> {
    for argv in &argvs {
        let program = argv.first().map(String::as_str).unwrap_or("");
        let resolved =
            find_program(program).ok_or_else(|| Error::NotFound(format!("program {program} (to run {})", entry.id)))?;
        if let Some(o) = observer {
            let target = std::fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
            o.emit(LaunchEvent::Message(format!(
                "Executable: {} (resolved to {})",
                resolved.display(),
                target.display()
            )));
        }
    }
    let dir = working_dir(entry, options)?;
    let mut launched = Vec::with_capacity(argvs.len());
    for argv in argvs {
        let started = match backend {
            Backend::Plain => spawn_plain(&argv, &dir, options, observer),
            Backend::Systemd { command, timeout } => {
                match spawn_scope(command, *timeout, entry, &argv, &dir, options, observer) {
                    Ok(started) => Ok(started),
                    Err(Fallback(why)) => {
                        log::warn!("apps: systemd-run failed for {} ({why}); starting it directly", entry.id);
                        if let Some(o) = observer {
                            o.emit(LaunchEvent::Message(format!(
                                "systemd scope unavailable: {why}; starting directly"
                            )));
                        }
                        spawn_plain(&argv, &dir, options, observer)
                    }
                }
            }
        };
        match started {
            Ok(started) => launched.push(started),
            // Report the first failure; earlier processes keep running.
            Err(error) if launched.is_empty() => return Err(error),
            Err(error) => log::warn!("apps: launching {}: {error}", entry.id),
        }
    }
    Ok(launched)
}

/// The override (which must be a directory), else `Path=` if it is a
/// directory, else `$HOME`, else `/`.
fn working_dir(entry: &DesktopEntry, options: &LaunchOptions) -> Result<PathBuf> {
    if let Some(dir) = &options.working_dir {
        if !dir.is_dir() {
            return Err(Error::NotFound(format!("working directory {}", dir.display())));
        }
        return Ok(dir.clone());
    }
    if let Some(dir) = &entry.working_dir {
        if dir.is_dir() {
            return Ok(dir.clone());
        }
        log::warn!("apps: {}: Path={} is not a directory", entry.id, dir.display());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.is_dir());
    Ok(home.unwrap_or_else(|| PathBuf::from("/")))
}

/// A stream into the journal for an app's output (as systemd gives
/// services), tagged with `identifier`; `None` without journald.
fn journal_stream(identifier: &str) -> Option<std::os::unix::net::UnixStream> {
    use std::io::Write;
    let mut stream = std::os::unix::net::UnixStream::connect("/run/systemd/journal/stdout").ok()?;
    stream.shutdown(std::net::Shutdown::Read).ok()?;
    let identifier: String = identifier.chars().filter(|c| !c.is_control()).collect();
    // Identifier, unit, priority (info), level prefix, and no forwarding
    // to syslog, the kernel log or the console.
    stream.write_all(format!("{identifier}\n\n6\n0\n0\n0\n0\n").as_bytes()).ok()?;
    Some(stream)
}

/// Where an app's output goes: the journal, else nowhere.
fn output(identifier: &str) -> (Stdio, Stdio) {
    match journal_stream(identifier).and_then(|s| Some((s.try_clone().ok()?, s))) {
        Some((out, err)) => {
            (Stdio::from(std::os::fd::OwnedFd::from(out)), Stdio::from(std::os::fd::OwnedFd::from(err)))
        }
        None => (Stdio::null(), Stdio::null()),
    }
}

/// Settings common to both ways of starting: directory, environment,
/// standard streams (output to the journal), a new session.
fn prepare(command: &mut Command, dir: &Path, options: &LaunchOptions, identifier: &str) {
    let (stdout, stderr) = output(identifier);
    command.current_dir(dir).stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    set_activation_env(command, options.activation_token.as_deref());
    // SAFETY: setsid is async-signal-safe and touches no memory; it is
    // the only thing done between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn set_activation_env(command: &mut Command, token: Option<&str>) {
    command.env_remove("XDG_ACTIVATION_TOKEN").env_remove("DESKTOP_STARTUP_ID");
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        command.env("XDG_ACTIVATION_TOKEN", token).env("DESKTOP_STARTUP_ID", token);
    }
}

fn spawn_plain(
    argv: &[String],
    dir: &Path,
    options: &LaunchOptions,
    observer: Option<&LaunchObserver>,
) -> Result<Launched> {
    let Some(program) = argv.first() else {
        return Err(Error::InvalidArgument("empty command line".into()));
    };
    let mut command = Command::new(app_shell());
    command.args(["-c", APP_WAIT, "aizome-launch"]).args(argv);
    let identifier = Path::new(program).file_name().map_or(program.clone(), |n| n.to_string_lossy().into_owned());
    prepare(&mut command, dir, options, observer.map_or(identifier.as_str(), LaunchObserver::journal_id));
    let child =
        command.spawn().map_err(|error| Error::Command { program: program.clone(), message: error.to_string() })?;
    let pid = child.id();
    let launched = Launched { pid, unit: None, argv: argv.to_vec() };
    observe_child(child, &launched, observer);
    Ok(launched)
}

/// Why the systemd route didn't work; the caller falls back.
struct Fallback(String);

// Keep the sandbox's parent alive when the desktop shell is replaced.
// Forward stderr before launching, closing systemd-run's handshake pipe.
const APP_WAIT: &str = "exec 2>&1; \"$@\" & wait \"$!\"";

fn app_shell() -> &'static str {
    if Path::new("/bin/sh").exists() { "/bin/sh" } else { "sh" }
}

fn spawn_scope(
    systemd_run: &[String],
    timeout: Duration,
    entry: &DesktopEntry,
    argv: &[String],
    dir: &Path,
    options: &LaunchOptions,
    observer: Option<&LaunchObserver>,
) -> std::result::Result<Launched, Fallback> {
    let Some((program, prefix)) = systemd_run.split_first() else {
        return Err(Fallback("no systemd-run".into()));
    };
    let unit = unit_name(entry.app_id(), &random_hex(8));
    let mut command = Command::new(program);
    command.args(prefix).args(["--user", "--scope", "--collect"]);
    command.arg(format!("--unit={unit}"));
    command.arg(format!("--description={}", entry.name));
    if systemd_version(systemd_run).is_some_and(|version| version >= 254) {
        command.arg("--expand-environment=no");
    }
    command.args(["--", app_shell(), "-c", APP_WAIT, "aizome-launch"]).args(argv);
    prepare(&mut command, dir, options, observer.map_or(entry.app_id(), LaunchObserver::journal_id));
    command.stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| Fallback(format!("{program}: {error}")))?;
    let pid = child.id();
    let launched = || Launched { pid, unit: Some(unit.clone()), argv: argv.to_vec() };

    // Read stderr on a thread until systemd-run names the unit (it then
    // execs the wrapper, which closes stderr) or exits.
    let Some(stderr) = child.stderr.take() else {
        kill(child);
        return Err(Fallback("no stderr pipe".into()));
    };
    let (sender, receiver) = mpsc::channel();
    let wanted = unit.clone();
    let reader = std::thread::Builder::new().name("aizome-sys launch".into()).stack_size(64 * 1024).spawn(move || {
        let mut reader = std::io::BufReader::new(stderr);
        let mut said = String::new();
        let mut line = Vec::new();
        let mut started = false;
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&line);
            if !started && text.contains(&wanted) {
                started = true;
                let _ = sender.send(None);
            } else if !started {
                said.push_str(text.trim_end());
                said.push(' ');
            }
        }
        if !started {
            let _ = sender.send(Some(said.trim_end().to_owned()));
        }
    });
    if let Err(error) = reader {
        kill(child);
        return Err(Fallback(format!("no thread to read systemd-run: {error}")));
    }
    match receiver.recv_timeout(timeout) {
        Ok(None) => {
            observe_child(child, &launched(), observer);
            Ok(launched())
        }
        Ok(Some(said)) => {
            // Stderr closed without the line: systemd-run failed and is
            // exiting, or (if it stopped printing the line) exec'd the
            // program.
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => {
                        if let Some(o) = observer {
                            o.emit(LaunchEvent::Started(launched()));
                            o.emit(LaunchEvent::Exited { pid, status: Ok(status) });
                        }
                        return Ok(launched());
                    }
                    Ok(Some(status)) => return Err(Fallback(format!("{status}: {said}"))),
                    Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                    Ok(None) => {
                        observe_child(child, &launched(), observer);
                        return Ok(launched());
                    }
                    Err(error) => {
                        kill(child);
                        return Err(Fallback(error.to_string()));
                    }
                }
            }
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            kill(child);
            Err(Fallback(format!("no answer in {timeout:?}")))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            kill(child);
            Err(Fallback("the reader thread died".into()))
        }
    }
}

/// Wait for `child` on a small thread so it doesn't stay a zombie.
fn observe_child(mut child: Child, launched: &Launched, observer: Option<&LaunchObserver>) {
    let observer = observer.cloned();
    let pid = child.id();
    if let Some(o) = &observer {
        o.emit(LaunchEvent::Started(launched.clone()));
    }
    let spawned = std::thread::Builder::new().name("aizome-sys reaper".into()).stack_size(64 * 1024).spawn(move || {
        let status = child.wait().map_err(|e| e.to_string());
        if let Some(o) = observer {
            o.emit(LaunchEvent::Exited { pid, status });
        }
    });
    if let Err(error) = spawned {
        log::warn!("apps: no thread to reap a launched process: {error}");
    }
}

fn kill(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// systemd-run, if it and the user manager are there.
fn systemd_backend() -> Option<Backend> {
    let systemd_run = find_program("systemd-run")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        // SAFETY: getuid has no preconditions.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    if !runtime.join("systemd/private").exists() {
        log::debug!("apps: no systemd user manager at {}", runtime.display());
        return None;
    }
    Some(Backend::Systemd { command: vec![systemd_run.to_string_lossy().into_owned()], timeout: SYSTEMD_TIMEOUT })
}

/// The systemd version of a `systemd-run` command (`systemd 261 (261.3)`),
/// asked once per command.
fn systemd_version(systemd_run: &[String]) -> Option<u32> {
    use std::sync::Mutex;
    static KNOWN: Mutex<Vec<(Vec<String>, Option<u32>)>> = Mutex::new(Vec::new());
    let mut known = crate::service::lock(&KNOWN);
    if let Some((_, version)) = known.iter().find(|(command, _)| command == systemd_run) {
        return *version;
    }
    let (program, prefix) = systemd_run.split_first()?;
    let version = Command::new(program)
        .args(prefix)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            text.split_whitespace().nth(1).and_then(|word| word.parse().ok())
        });
    known.push((systemd_run.to_vec(), version));
    version
}

/// A transient scope's name for an application ID:
/// `app-aizome-<escaped ID>-<random>.scope`, shortened to systemd's 255
/// characters if need be.
pub fn unit_name(app_id: &str, random: &str) -> String {
    let fixed = format!("app-{LAUNCHER}--{random}.scope").len();
    let escaped = systemd_escape(app_id);
    let room = 255usize.saturating_sub(fixed);
    let mut id = String::new();
    let mut rest = escaped.as_str();
    while !rest.is_empty() {
        // Keep `\xNN` escapes whole.
        let take = if rest.starts_with('\\') { 4.min(rest.len()) } else { 1 };
        let Some(piece) = rest.get(..take) else {
            break;
        };
        if id.len() + piece.len() > room {
            break;
        }
        id.push_str(piece);
        rest = &rest[take..];
    }
    format!("app-{LAUNCHER}-{id}-{random}.scope")
}

/// Escape a string for a unit name as `systemd-escape` does: ASCII
/// letters, digits, `:`, `_` and `.` (but not a leading `.`) stay, `/`
/// becomes `-`, every other byte `\xNN`.
pub fn systemd_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (n, byte) in s.bytes().enumerate() {
        match byte {
            b'/' => out.push('-'),
            b'.' if n == 0 => out.push_str("\\x2e"),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b':' | b'_' | b'.' => out.push(char::from(byte)),
            other => out.push_str(&format!("\\x{other:02x}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use aizome_apps::Locale;
    use aizome_apps::testutil::TempDir;
    use super::*;

    fn entry(text: &str) -> DesktopEntry {
        DesktopEntry::parse(text, "org.example.Test-App.desktop", &Locale::default()).unwrap()
    }

    #[test]
    fn unit_names_escape_like_systemd() {
        assert_eq!(systemd_escape("org.gnome.Nautilus"), "org.gnome.Nautilus");
        assert_eq!(systemd_escape("code-url-handler"), r"code\x2durl\x2dhandler");
        assert_eq!(systemd_escape("a b/c"), r"a\x20b-c");
        assert_eq!(systemd_escape(".hidden"), r"\x2ehidden");
        assert_eq!(systemd_escape("ünï"), r"\xc3\xbcn\xc3\xaf");
        assert_eq!(systemd_escape(r"back\slash"), r"back\x5cslash");
        assert_eq!(unit_name("foot", "0123abcd"), "app-aizome-foot-0123abcd.scope");
        assert_eq!(unit_name("org.foo-bar", "ff"), r"app-aizome-org.foo\x2dbar-ff.scope");
        // A command's scope is named for the app it runs, else its program.
        let settings =
            command_entry(&["aizome-settei".into(), "--pane=sound".into()], Some("org.aizome.Settei")).unwrap();
        assert_eq!(settings.app_id(), "org.aizome.Settei");
        assert_eq!(settings.name, "aizome-settei");
        let open = command_entry(&["/run/current-system/sw/bin/xdg-open".into(), "x".into()], None).unwrap();
        assert_eq!(unit_name(open.app_id(), "ff"), r"app-aizome-xdg\x2dopen-ff.scope");
        assert_eq!(command_entry(&[], None).unwrap().app_id(), "command");
        let long = unit_name(&"-".repeat(200), "0123456789abcdef");
        assert!(long.len() <= 255, "{}", long.len());
        assert!(long.ends_with(r"\x2d-0123456789abcdef.scope"));
        // Compare with the real tool, when there is one.
        if find_program("systemd-escape").is_some() {
            for id in ["org.gnome.Nautilus", "code-url-handler", "a b", ".x", "ünï", "x:y_z", "a\\b"] {
                let output = Command::new("systemd-escape").arg(id).output().unwrap();
                assert_eq!(String::from_utf8_lossy(&output.stdout).trim_end(), systemd_escape(id), "{id}");
            }
        }
    }

    #[test]
    fn command_lines_for_actions_terminals_and_targets() {
        let app = entry(
            "[Desktop Entry]\nType=Application\nName=Test App\nIcon=test-icon\nExec=test-app %i %c %F\n\
             Actions=one;\nTerminal=true\n[Desktop Action one]\nName=One\nIcon=one-icon\nExec=test-app --one %u\n",
        );
        let mut options = LaunchOptions { targets: vec!["/a".into(), "/b".into()], ..LaunchOptions::default() };
        assert_eq!(
            command_lines(&app, &options).unwrap(),
            [vec!["aizome-hotaru", "--", "test-app", "--icon", "test-icon", "Test App", "/a", "/b"]]
        );
        options.action = Some("one".into());
        options.terminal = vec!["kitty".into(), "-e".into()];
        assert_eq!(
            command_lines(&app, &options).unwrap(),
            [vec!["kitty", "-e", "test-app", "--one", "/a"], vec!["kitty", "-e", "test-app", "--one", "/b"]]
        );
        options.action = Some("two".into());
        assert!(matches!(command_lines(&app, &options), Err(Error::NotFound(_))));
        options.action = None;
        options.terminal.clear();
        assert!(matches!(command_lines(&app, &options), Err(Error::InvalidArgument(_))));
        let dbus_only = entry("[Desktop Entry]\nType=Application\nName=D\nDBusActivatable=true\n");
        assert!(matches!(command_lines(&dbus_only, &options), Err(Error::Unavailable(_))));
        let link = entry("[Desktop Entry]\nType=Link\nName=L\nURL=https://x.org\n");
        assert!(matches!(command_lines(&link, &options), Err(Error::InvalidArgument(_))));
        let bad = entry("[Desktop Entry]\nType=Application\nName=B\nExec=prog \"unterminated\n");
        assert!(matches!(command_lines(&bad, &options), Err(Error::InvalidArgument(_))));
        // Both layers of escaping, from the file to the argument vector:
        // the string escapes (`\\` is `\`), then Exec quoting (`\$`, `\\`).
        let escaped = entry(
            r#"[Desktop Entry]
Type=Application
Name=E
Exec=echo "\\$HOME" "a\\\\b" 100%% two\sspaces
"#,
        );
        assert_eq!(
            command_lines(&escaped, &options).unwrap(),
            [vec!["echo", "$HOME", r"a\b", "100%", "two", "spaces"]]
        );
    }

    #[test]
    fn missing_programs_are_refused_before_starting() {
        let app = entry("[Desktop Entry]\nType=Application\nName=X\nExec=surely-no-such-program-here %f\n");
        let mut options = LaunchOptions { use_systemd: false, ..LaunchOptions::default() };
        assert!(matches!(launch(&app, &options), Err(Error::NotFound(_))));
        // A working directory that isn't one.
        let true_app = entry("[Desktop Entry]\nType=Application\nName=T\nExec=true\n");
        options.working_dir = Some(PathBuf::from("/no/such/directory"));
        assert!(matches!(launch(&true_app, &options), Err(Error::NotFound(_))));
    }

    #[test]
    fn activation_tokens_replace_inherited_ones() {
        let get = |command: &Command, key: &str| {
            command.get_envs().find(|(k, _)| *k == key).map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        let mut command = Command::new("true");
        set_activation_env(&mut command, None);
        // Removed (explicitly unset), not inherited.
        assert_eq!(get(&command, "XDG_ACTIVATION_TOKEN"), Some(None));
        assert_eq!(get(&command, "DESKTOP_STARTUP_ID"), Some(None));
        let mut command = Command::new("true");
        set_activation_env(&mut command, Some("token-123"));
        assert_eq!(get(&command, "XDG_ACTIVATION_TOKEN"), Some(Some("token-123".into())));
        assert_eq!(get(&command, "DESKTOP_STARTUP_ID"), Some(Some("token-123".into())));
    }

    /// A desktop entry running a shell script that records what it got.
    struct Probe {
        tmp: TempDir,
    }

    impl Probe {
        fn new(name: &str) -> Probe {
            let tmp = TempDir::new(name);
            tmp.write(
                "probe.sh",
                "out=\"$(dirname \"$0\")/out-$$\"\n\
                 {\n\
                 echo \"args:$*\"\n\
                 echo \"pwd:$(pwd -P)\"\n\
                 echo \"token:${XDG_ACTIVATION_TOKEN-unset}:${DESKTOP_STARTUP_ID-unset}\"\n\
                 echo \"stat:$(cat /proc/$$/stat)\"\n\
                 echo \"cgroup:$(cat /proc/$$/cgroup)\"\n\
                 } > \"$out.tmp\"\n\
                 mv \"$out.tmp\" \"$out\"\n",
            );
            tmp.write("work/.keep", "");
            Probe { tmp }
        }

        fn entry(&self, extra: &str) -> DesktopEntry {
            let script = self.tmp.path().join("probe.sh");
            entry(&format!(
                "[Desktop Entry]\nType=Application\nName=Probe $HOME\nExec=sh \"{}\" %F\nPath={}\n{extra}",
                script.display(),
                self.tmp.path().join("work").display()
            ))
        }

        /// What the app in launch session `pid` recorded, once it has.
        fn report(&self, pid: u32) -> String {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                for file in std::fs::read_dir(self.tmp.path()).unwrap().flatten() {
                    let name = file.file_name().to_string_lossy().into_owned();
                    if name.starts_with("out-")
                        && !name.ends_with(".tmp")
                        && let Ok(text) = std::fs::read_to_string(file.path())
                        && session_of(field(&text, "stat")) == pid
                    {
                        return text;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("process {pid} wrote nothing");
        }
    }

    fn field<'a>(report: &'a str, name: &str) -> &'a str {
        report
            .lines()
            .find_map(|line| line.strip_prefix(name).and_then(|rest| rest.strip_prefix(':')))
            .unwrap_or_else(|| panic!("no {name} in {report}"))
    }

    /// The session ID from /proc/<pid>/stat (field 6).
    fn session_of(stat: &str) -> u32 {
        let after_comm = stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("");
        after_comm.split_whitespace().nth(3).and_then(|s| s.parse().ok()).unwrap()
    }

    /// Wait until `pid` is gone (reaped), not a zombie.
    fn assert_reaped(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if std::fs::metadata(format!("/proc/{pid}")).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        panic!("process {pid} is still there: {stat}");
    }

    #[test]
    fn plain_spawn() {
        let probe = Probe::new("launch-plain");
        let app = probe.entry("");
        let options = LaunchOptions {
            targets: vec!["/a b".into(), "file:///c%20d".into(), "https://skipped.example/".into()],
            activation_token: Some("tok-1".into()),
            use_systemd: false,
            ..LaunchOptions::default()
        };
        let launched = launch(&app, &options).unwrap();
        assert_eq!(launched.len(), 1);
        assert_eq!(launched[0].unit, None);
        let pid = launched[0].pid;
        let report = probe.report(pid);
        assert_eq!(field(&report, "args"), "/a b /c d");
        assert_eq!(Path::new(field(&report, "pwd")), probe.tmp.path().join("work").canonicalize().unwrap());
        assert_eq!(field(&report, "token"), "tok-1:tok-1");
        assert_eq!(session_of(field(&report, "stat")), pid, "its own session");
        assert_reaped(pid);
    }

    #[test]
    fn through_systemd() {
        let Some(backend) = systemd_backend() else {
            eprintln!("no systemd user manager here; skipping");
            return;
        };
        let probe = Probe::new("launch-systemd");
        let app = probe.entry("");
        let options = LaunchOptions {
            targets: vec!["/x$HOME".into()],
            working_dir: Some(probe.tmp.path().to_owned()),
            ..LaunchOptions::default()
        };
        let started = Instant::now();
        let launched = launch_with(&app, &options, &backend).unwrap();
        let took = started.elapsed();
        let pid = launched[0].pid;
        let unit = launched[0].unit.clone().expect("a scope");
        assert!(unit.starts_with(r"app-aizome-org.example.Test\x2dApp-") && unit.ends_with(".scope"), "{unit}");
        let report = probe.report(pid);
        println!("scope {unit} started in {took:?}");
        // `$HOME` reaches the program unexpanded.
        assert_eq!(field(&report, "args"), "/x$HOME");
        assert_eq!(Path::new(field(&report, "pwd")), probe.tmp.path().canonicalize().unwrap());
        assert_eq!(field(&report, "token"), "unset:unset");
        assert!(field(&report, "cgroup").ends_with(&format!("/{unit}")), "{report}");
        assert_eq!(session_of(field(&report, "stat")), pid);
        assert!(took < Duration::from_secs(2), "{took:?}");
        assert_reaped(pid);
    }

    #[test]
    fn failing_systemd_run_falls_back() {
        let probe = Probe::new("launch-fallback");
        // Fakes that answer --version like systemd 256 does, then fail or
        // hang when asked to start a scope.
        let version = "if [ \"$1\" = --version ]; then echo \"systemd 256 (256.1)\"; exit 0; fi\n";
        probe.tmp.write(
            "fake-systemd-run",
            &format!("{version}echo 'Failed to start transient scope unit: Access denied' >&2\nexit 1\n"),
        );
        probe.tmp.write("hanging-systemd-run", &format!("{version}exec sleep 30\n"));
        let app = probe.entry("");
        let options = LaunchOptions { targets: vec!["/t".into()], ..LaunchOptions::default() };
        for (script, timeout) in
            [("fake-systemd-run", SYSTEMD_TIMEOUT), ("hanging-systemd-run", Duration::from_millis(300))]
        {
            let backend = Backend::Systemd {
                command: vec!["sh".into(), probe.tmp.path().join(script).to_string_lossy().into_owned()],
                timeout,
            };
            let launched = launch_with(&app, &options, &backend).unwrap();
            assert_eq!(launched[0].unit, None, "{script}");
            let report = probe.report(launched[0].pid);
            assert_eq!(field(&report, "args"), "/t");
            assert_reaped(launched[0].pid);
        }
    }

    #[test]
    fn sandbox_survives_launcher_exit() {
        const TEST: &str = "apps::launch::tests::sandbox_survives_launcher_exit";
        const ENV: &str = "AIZOME_TEST_LAUNCH_LIFETIME";
        if let Some(dir) = std::env::var_os(ENV) {
            let dir = PathBuf::from(dir);
            if dir.join("launching").exists() {
                // Bubblewrap's --die-with-parent uses this same contract.
                // SAFETY: prctl changes only this disposable process.
                assert_eq!(unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) }, 0);
                std::fs::write(dir.join("ready"), "").unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while !dir.join("continue").exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(dir.join("continue").exists());
                std::fs::write(dir.join("survived"), "").unwrap();
                return;
            }
            std::fs::write(dir.join("launching"), "").unwrap();
            let argv =
                vec![std::env::current_exe().unwrap().to_string_lossy().into_owned(), "--exact".into(), TEST.into()];
            let options = LaunchOptions::default();
            let launched = if dir.join("systemd-run").exists() {
                let command = vec!["sh".into(), dir.join("systemd-run").to_string_lossy().into_owned()];
                spawn_scope(
                    &command,
                    SYSTEMD_TIMEOUT,
                    &entry("[Desktop Entry]\nType=Application\nName=Test\nExec=true\n"),
                    &argv,
                    &dir,
                    &options,
                    None,
                )
                .unwrap_or_else(|e| panic!("scope failed: {}", e.0))
            } else {
                spawn_plain(&argv, &dir, &options, None).unwrap()
            };
            std::fs::write(dir.join("supervisor"), launched.pid.to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(20));
            panic!("the test launcher was not stopped");
        }
        for scoped in [false, true] {
            let tmp = TempDir::new("launch-lifetime");
            if scoped {
                tmp.write("systemd-run", "if [ \"$1\" = --version ]; then echo 'systemd 256'; exit 0; fi\nwhile [ \"$1\" != -- ]; do\ncase $1 in --unit=*) echo \"Running as unit: ${1#--unit=}\" >&2;; esac\nshift\ndone\nshift\nexec \"$@\"\n");
            }
            let mut launcher = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST])
                .env(ENV, tmp.path())
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while (!tmp.path().join("ready").exists() || !tmp.path().join("supervisor").exists())
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            launcher.kill().unwrap();
            launcher.wait().unwrap();
            assert!(tmp.path().join("ready").exists(), "sandbox did not start");
            std::fs::write(tmp.path().join("continue"), "").unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !tmp.path().join("survived").exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(tmp.path().join("survived").exists(), "sandbox died with its launcher (scoped={scoped})");
            let pid = std::fs::read_to_string(tmp.path().join("supervisor")).unwrap().parse().unwrap();
            assert_reaped(pid);
        }
    }

    #[test]
    fn terminal_apps_run_inside_the_terminal_command() {
        let probe = Probe::new("launch-terminal");
        let app = probe.entry("Terminal=true\n");
        let script = probe.tmp.path().join("probe.sh");
        // The "terminal" is the probe itself: it records the command line.
        let options = LaunchOptions {
            terminal: vec!["sh".into(), script.to_string_lossy().into_owned(), "-e".into()],
            use_systemd: false,
            ..LaunchOptions::default()
        };
        let launched = launch(&app, &options).unwrap();
        let report = probe.report(launched[0].pid);
        assert_eq!(field(&report, "args"), format!("-e sh {}", script.display()));
    }

    #[test]
    fn background_launch_wakes() {
        let probe = Probe::new("launch-background");
        let app = probe.entry("");
        let options = LaunchOptions { use_systemd: false, ..LaunchOptions::default() };
        let task = launch_in_background(app, options, &Wake::none());
        let launched = task.wait_timeout(Duration::from_secs(10)).expect("finished").unwrap();
        probe.report(launched[0].pid);
    }

    #[test]
    fn observed_launch_reports_delayed_failure_after_process_creation() {
        let tmp = TempDir::new("launch-observed");
        let script = tmp.write("fail.sh", "sleep 0.15\necho 'koe: daemon did not become ready' >&2\nexit 23\n");
        let shim = tmp.write("systemd-run", "if [ \"$1\" = --version ]; then echo 'systemd 256'; exit 0; fi\nwhile [ \"$1\" != -- ]; do\ncase $1 in --unit=*) echo \"Running as unit: ${1#--unit=}\" >&2;; esac\nshift\ndone\nshift\nexec \"$@\"\n");
        for backend in [
            Backend::Plain,
            Backend::Systemd { command: vec!["sh".into(), shim.display().to_string()], timeout: SYSTEMD_TIMEOUT },
        ] {
            let (tx, rx) = mpsc::channel();
            let observer = LaunchObserver::new(move |event| {
                let _ = tx.send(event);
            });
            let app = entry(&format!(
                "[Desktop Entry]\nType=Application\nName=Koe failure fixture\nExec=sh {}\n",
                script.display()
            ));
            let launched = launch_with_observer(&app, &LaunchOptions::default(), &backend, Some(&observer)).unwrap();
            assert!(
                matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), LaunchEvent::Message(s) if s.contains("Executable:"))
            );
            let LaunchEvent::Started(started) = rx.recv_timeout(Duration::from_secs(2)).unwrap() else {
                panic!("missing start")
            };
            assert_eq!(started, launched[0]);
            let LaunchEvent::Exited { pid, status } = rx.recv_timeout(Duration::from_secs(5)).unwrap() else {
                panic!("missing exit")
            };
            assert_eq!(pid, started.pid);
            assert_eq!(status.unwrap().code(), Some(23));
            assert_reaped(pid);
        }
    }

    #[test]
    fn observed_commands_preserve_literal_arguments_and_successful_handoffs() {
        let (tx, rx) = mpsc::channel();
        let observer = LaunchObserver::new(move |event| {
            let _ = tx.send(event);
        });
        let argv = vec![
            "sh".into(),
            "-c".into(),
            "test \"$1\" = '%F $HOME `false`'".into(),
            "test".into(),
            "%F $HOME `false`".into(),
        ];
        let launched = launch_command_observed(
            &argv,
            None,
            &LaunchOptions { use_systemd: false, ..Default::default() },
            &observer,
        )
        .unwrap();
        assert_eq!(launched[0].argv, argv);
        assert!(
            matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), LaunchEvent::Message(s) if s.contains("Executable:"))
        );
        assert!(matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), LaunchEvent::Started(_)));
        let LaunchEvent::Exited { status, .. } = rx.recv_timeout(Duration::from_secs(2)).unwrap() else {
            panic!("no exit")
        };
        assert!(status.unwrap().success());
        let other = LaunchObserver::new(|_| {});
        assert_ne!(observer.journal_id(), other.journal_id());
        let missing = launch_command_observed(
            &["/definitely/not/an/application".into()],
            None,
            &LaunchOptions::default(),
            &other,
        );
        assert!(missing.is_err());
    }
}
