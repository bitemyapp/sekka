//! Session startup and the processes the compositor keeps alive.
//!
//! Once the Wayland socket and IPC are up, the compositor exports its
//! environment to the systemd user manager and D-Bus activation, tells
//! systemd it is ready (when started as `aizome.service`), and starts
//! `aizome-session.target`. It runs `aizome-shell` and
//! `xwayland-satellite` (when installed) and restarts them, backing off,
//! if they die.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use smithay::desktop::WindowSurfaceType;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use tracing::{info, warn};

use crate::state::{Aizome, State};

/// A process the compositor keeps running.
pub struct Supervised {
    pub name: String,
    pub argv: Vec<String>,
    child: Option<Child>,
    started: Instant,
    backoff: Duration,
    restart_at: Option<Instant>,
    /// The previous instance, while a new one takes over (a shell
    /// restart): it is stopped once the new one has replaced its surfaces.
    retiring: Option<Child>,
    /// Asked to restart: start again at once once it has gone.
    restart_now: bool,
}

impl Supervised {
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }
}

#[derive(Default)]
pub struct Children {
    pub supervised: Vec<Supervised>,
    /// The environment every child gets on top of ours.
    pub env: Vec<(String, String)>,
    /// A shell restart in progress.
    pub handover: Option<Handover>,
}

/// A shell restart: the new shell runs beside the old one, and each of its
/// layer surfaces, once drawn, replaces the old shell's surface of the
/// same namespace on the same output in the same frame, so the bar, the
/// Deskbar and the desktop never leave the screen. Windows aren't refitted
/// meanwhile (both shells' exclusive zones count until the swap). The old
/// shell is stopped once it has no surfaces left, or after a while.
pub struct Handover {
    pub old: u32,
    pub new: u32,
    pub started: Instant,
}

impl Children {
    /// The process of a supervised helper, while it runs.
    pub fn pid_of(&self, name: &str) -> Option<u32> {
        self.supervised.iter().find(|s| s.name == name)?.child.as_ref().map(|c| c.id())
    }
}

/// The variables the session shares with everything it starts.
pub fn session_env(az: &Aizome) -> Vec<(String, String)> {
    let mut env = vec![
        ("WAYLAND_DISPLAY".to_string(), az.socket_name.to_string_lossy().into_owned()),
        ("XDG_CURRENT_DESKTOP".into(), "Aizome".into()),
        ("XDG_SESSION_DESKTOP".into(), "aizome".into()),
        ("XDG_SESSION_TYPE".into(), "wayland".into()),
    ];
    if let Some(p) = &az.ipc.path {
        env.push(("AIZOME_SOCKET".into(), p.to_string_lossy().into_owned()));
    }
    let shell = aizome_ipc::runtime_dir().join(format!("aizome-shell.{}.sock", az.socket_name.to_string_lossy()));
    env.push(("AIZOME_SHELL_SOCKET".into(), shell.to_string_lossy().into_owned()));
    env.extend(az.children.env.iter().cloned());
    env
}

/// Runs a program detached from the compositor, with the session's
/// environment, optionally in a folder.
pub fn spawn(az: &Aizome, argv: &[String], cwd: Option<PathBuf>) -> Result<u32, String> {
    spawn_with(session_env(az), argv, cwd)
}

/// [`spawn`] with the session's environment taken already, for a thread
/// that can't reach the compositor's state.
pub fn spawn_with(env: Vec<(String, String)>, argv: &[String], cwd: Option<PathBuf>) -> Result<u32, String> {
    let Some((prog, args)) = argv.split_first() else { return Err("nothing to run".into()) };
    // Keybindings and IPC launches must leave the compositor's service,
    // just as desktop-entry launches already do. In a nested/headless
    // session without a user manager, keep the direct-spawn behaviour.
    let manager = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .is_some_and(|dir| dir.join("systemd/private").exists());
    let mut cmd = if let Some(systemd_run) = which("systemd-run").filter(|_| manager) {
        let mut cmd = Command::new(systemd_run);
        cmd.args(["--user", "--scope", "--collect", "--quiet", "--slice=app.slice",
            "--expand-environment=no", "--"]);
        cmd.arg(prog);
        cmd
    } else {
        Command::new(prog)
    };
    cmd.args(args).stdin(Stdio::null()).envs(env);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    } else {
        cmd.current_dir(aizome_config::home_dir());
    }
    // Safety: setsid is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("{prog}: {e}"))?;
    let pid = child.id();
    // Reap it when it exits.
    std::thread::Builder::new()
        .name("aizome-reap".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .ok();
    info!(pid, "spawned {prog}");
    Ok(pid)
}

/// Finds a program on PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| is_executable(p))
}

pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// A free X display number.
fn free_x_display() -> Option<u32> {
    (1..64).find(|n| {
        !Path::new(&format!("/tmp/.X11-unix/X{n}")).exists() && !Path::new(&format!("/tmp/.X{n}-lock")).exists()
    })
}

/// Whether this is someone's login session, on a seat and known to
/// systemd: only then does the shell serve polkit, hold logind's sleep and
/// lock for logind (`AIZOME_SESSION=1`). A headless or nested run (tests,
/// screenshots, trying Aizome inside another desktop) runs as the same user
/// inside that user's real login session, and must leave all of that alone.
fn real_session(opts: &crate::cli::Options) -> bool {
    opts.backend == crate::cli::BackendKind::Udev && !opts.no_session
}

/// Starts the session's helpers and talks to systemd.
pub fn start(state: &mut State) {
    let opts = state.az.options.clone();

    let real = real_session(&opts);
    state.az.children.env.push(("AIZOME_SESSION".into(), if real { "1" } else { "0" }.into()));
    // Only someone's own session moves their terminal to Hotaru: a test or
    // nested run must not rewrite the person's lists.
    if real {
        crate::terminal::move_to_hotaru_once(state.az.settings.keys.legacy_terminal.clone());
    }

    if !opts.no_xwayland {
        if let Some(bin) = which("xwayland-satellite") {
            if let Some(n) = free_x_display() {
                let display = format!(":{n}");
                state.az.children.env.push(("DISPLAY".into(), display.clone()));
                state
                    .az
                    .children
                    .supervised
                    .push(Supervised::new("xwayland-satellite", vec![bin.to_string_lossy().into_owned(), display]));
            }
        } else {
            info!("xwayland-satellite isn't installed; X11 apps won't run");
        }
    }
    if !opts.no_shell {
        if let Some(bin) = shell_binary() {
            state
                .az
                .children
                .supervised
                .push(Supervised::new("aizome-shell", vec![bin.to_string_lossy().into_owned()]));
        } else {
            info!("aizome-shell isn't installed; running without a shell");
        }
    }
    let env = session_env(&state.az);
    for s in state.az.children.supervised.iter_mut() {
        start_child(s, &env);
    }
    // Supervision runs off the same timer that reaps (more often while a
    // shell restart is under way).
    let _ = state.az.lh.insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, state: &mut State| {
        supervise(state);
        let next = if state.az.children.handover.is_some() { 100 } else { 1000 };
        TimeoutAction::ToDuration(Duration::from_millis(next))
    });

    if !opts.no_session {
        let env = session_env(&state.az);
        let names: Vec<String> = env.iter().map(|(k, _)| k.clone()).collect();
        let under_systemd = std::env::var_os("NOTIFY_SOCKET").is_some() || std::env::var_os("INVOCATION_ID").is_some();
        if under_systemd {
            run_quietly(
                Command::new("systemctl").arg("--user").arg("import-environment").args(&names).envs(env.clone()),
            );
        }
        let mut dbus = Command::new("dbus-update-activation-environment");
        if under_systemd {
            dbus.arg("--systemd");
        }
        run_quietly(dbus.args(&names).envs(env.clone()));
        notify_ready();
        if under_systemd {
            run_quietly(Command::new("systemctl").args(["--user", "--no-block", "start", "aizome-session.target"]));
        }
    }

    if !opts.command.is_empty() {
        let argv = opts.command.clone();
        if let Err(e) = spawn(&state.az, &argv, None) {
            warn!("{e}");
        }
    }
}

fn run_quietly(cmd: &mut Command) {
    match cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(s) if s.success() => {}
        Ok(s) => warn!("{:?} exited with {s}", cmd.get_program()),
        Err(e) => warn!("{:?}: {e}", cmd.get_program()),
    }
}

/// `READY=1` on `$NOTIFY_SOCKET` (no libsystemd needed).
pub fn notify_ready() {
    sd_notify("READY=1\nSTATUS=Aizome is running");
}

pub fn notify_stopping() {
    sd_notify("STOPPING=1");
}

pub fn sd_notify(msg: &str) {
    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else { return };
    let path = path.to_string_lossy().into_owned();
    let Ok(sock) = std::os::unix::net::UnixDatagram::unbound() else { return };
    let res = if let Some(abstract_name) = path.strip_prefix('@') {
        use std::os::linux::net::SocketAddrExt;
        std::os::unix::net::SocketAddr::from_abstract_name(abstract_name.as_bytes())
            .and_then(|addr| sock.send_to_addr(msg.as_bytes(), &addr))
    } else {
        sock.send_to(msg.as_bytes(), &path)
    };
    if let Err(e) = res {
        warn!("sd_notify: {e}");
    }
}

impl Supervised {
    fn new(name: &str, argv: Vec<String>) -> Supervised {
        Supervised {
            name: name.into(),
            argv,
            child: None,
            started: Instant::now(),
            backoff: Duration::from_secs(1),
            restart_at: None,
            retiring: None,
            restart_now: false,
        }
    }
}

/// The shell to run: `$AIZOME_SHELL_BIN`, else `aizome-shell` on PATH (the
/// system's, after an update).
pub fn shell_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AIZOME_SHELL_BIN").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    which("aizome-shell")
}

/// Whether the shell that would start now differs from the one running
/// (`None`: no shell is running).
pub fn shell_binary_changed(az: &Aizome) -> Option<bool> {
    let s = az.children.supervised.iter().find(|s| s.name == "aizome-shell")?;
    let pid = s.pid()?;
    let running = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    let installed = shell_binary().and_then(|p| std::fs::canonicalize(p).ok());
    Some(running.is_none() || installed.is_none() || running != installed)
}

impl State {
    /// Restarts the shell beside the old one (see [`Handover`]): the apps
    /// and their windows don't notice; the shell's notifications and other
    /// state carry over (it saves them, and the new one takes over its bus
    /// name). `bin`: a shell of one's own build, else the installed one;
    /// it stays the one started again after a crash.
    pub fn restart_shell(&mut self, bin: Option<PathBuf>) -> Result<String, String> {
        if self.az.children.handover.is_some() {
            return Err("a shell restart is already under way".into());
        }
        // The shell is the lock screen: a new one couldn't take over the
        // lock its predecessor holds.
        if self.az.lock.is_locked() {
            return Err("the screen is locked; unlock first".into());
        }
        let env = session_env(&self.az);
        let i =
            self.az.children.supervised.iter().position(|s| s.name == "aizome-shell").ok_or("no shell is running")?;
        let bin = match bin {
            Some(p) if is_executable(&p) => p,
            Some(p) => return Err(format!("{} isn't a program", p.display())),
            None => shell_binary().ok_or("aizome-shell isn't installed")?,
        };
        let s = &mut self.az.children.supervised[i];
        s.argv[0] = bin.to_string_lossy().into_owned();
        let old = s.child.take();
        let old_pid = old.as_ref().map(|c| c.id());
        s.retiring = old;
        let mut env = env;
        if let Some(p) = old_pid {
            env.push(("AIZOME_SHELL_PREVIOUS_PID".into(), p.to_string()));
        }
        start_child(s, &env);
        s.backoff = Duration::from_secs(1);
        let new_pid = s.pid().ok_or("the new shell didn't start")?;
        if let Some(old) = old_pid {
            self.az.children.handover = Some(Handover { old, new: new_pid, started: Instant::now() });
        }
        info!(old = ?old_pid, new = new_pid, "restarting the shell");
        Ok(format!("{} (pid {new_pid}) replaces pid {}", bin.display(), old_pid.unwrap_or(0)))
    }

    /// While a shell restart is under way, windows keep their places
    /// (both shells' exclusive zones count until the swap).
    pub fn shell_restart_holds_layout(&self) -> bool {
        self.az.children.handover.is_some()
    }

    /// A layer surface of the new shell drew its first frame: the old
    /// shell's surface of the same namespace on this output goes, in the
    /// same frame.
    pub fn shell_handover_layer(&mut self, output: &smithay::output::Output, surface: &WlSurface) {
        let Some(h) = &self.az.children.handover else { return };
        let (old, new) = (h.old, h.new);
        let pid_of = |s: &WlSurface| s.client().and_then(|c| c.get_credentials(&self.az.dh).ok()).map(|c| c.pid as u32);
        if pid_of(surface) != Some(new) {
            return;
        }
        let mut map = smithay::desktop::layer_map_for_output(output);
        let Some(namespace) =
            map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).map(|l| l.namespace().to_string())
        else {
            return;
        };
        let stale: Vec<_> = map
            .layers()
            .filter(|l| l.namespace() == namespace && pid_of(l.wl_surface()) == Some(old))
            .cloned()
            .collect();
        for l in &stale {
            l.layer_surface().send_close();
            map.unmap_layer(l);
        }
        if !stale.is_empty() {
            map.arrange();
            tracing::debug!(namespace, "the new shell's surface replaced the old one's");
        }
    }

    /// Ends a shell restart once the old shell has no surfaces left (or it
    /// took too long, or the new one died): stops the old shell, and lets
    /// windows refit if the usable area changed.
    fn check_handover(&mut self) {
        let Some(h) = &self.az.children.handover else { return };
        let (old, new, started) = (h.old, h.new, h.started);
        let pid_of = |s: &WlSurface| s.client().and_then(|c| c.get_credentials(&self.az.dh).ok()).map(|c| c.pid as u32);
        let old_surfaces = self
            .az
            .outputs
            .iter()
            .map(|o| {
                smithay::desktop::layer_map_for_output(o)
                    .layers()
                    .filter(|l| pid_of(l.wl_surface()) == Some(old))
                    .count()
            })
            .sum::<usize>();
        let new_alive = self.az.children.supervised.iter().any(|s| s.pid() == Some(new) && s.child.is_some());
        let done = old_surfaces == 0 || started.elapsed() > Duration::from_secs(8) || !new_alive;
        if !done {
            return;
        }
        info!(old, new, old_surfaces, "shell restart finished");
        self.az.children.handover = None;
        for s in &mut self.az.children.supervised {
            if let Some(mut c) = s.retiring.take() {
                // SIGTERM: it saved its state on every change, and its last
                // word as the new shell took its bus name.
                // Safety: kill() with a valid pid and signal.
                unsafe {
                    libc::kill(c.id() as i32, libc::SIGTERM);
                }
                std::thread::Builder::new()
                    .name("aizome-reap".into())
                    .spawn(move || {
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while Instant::now() < deadline {
                            if let Ok(Some(_)) = c.try_wait() {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        let _ = c.kill();
                        let _ = c.wait();
                    })
                    .ok();
            }
        }
        // Windows refit if the new shell left them another area.
        for o in self.az.outputs.clone() {
            self.lg_call((), |vt, st| (vt.usable_area_changed)(st, &o));
        }
    }
}

fn start_child(s: &mut Supervised, env: &[(String, String)]) {
    let mut cmd = Command::new(&s.argv[0]);
    cmd.args(&s.argv[1..]).stdin(Stdio::null()).envs(env.iter().cloned()).current_dir(aizome_config::home_dir());
    match cmd.spawn() {
        Ok(child) => {
            info!(pid = child.id(), "started {}", s.name);
            s.child = Some(child);
            s.started = Instant::now();
            s.restart_at = None;
        }
        Err(e) => warn!("{}: {e}", s.name),
    }
}

fn supervise(state: &mut State) {
    state.check_handover();
    let now = Instant::now();
    let mut restart = Vec::new();
    for (i, s) in state.az.children.supervised.iter_mut().enumerate() {
        if let Some(child) = &mut s.child {
            if let Ok(Some(status)) = child.try_wait() {
                s.child = None;
                if std::mem::take(&mut s.restart_now) {
                    info!("{} stopped ({status}); starting it again", s.name);
                    s.restart_at = Some(now);
                    restart.push(i);
                    continue;
                }
                warn!("{} exited ({status}); restarting in {:?}", s.name, s.backoff);
                // A long run resets the backoff.
                if s.started.elapsed() > Duration::from_secs(60) {
                    s.backoff = Duration::from_secs(1);
                }
                s.restart_at = Some(now + s.backoff);
                s.backoff = (s.backoff * 2).min(Duration::from_secs(30));
            }
        } else if s.restart_at.is_some_and(|t| now >= t) {
            s.restart_now = false;
            restart.push(i);
        }
    }
    let env = session_env(&state.az);
    for i in restart {
        start_child(&mut state.az.children.supervised[i], &env);
    }
}

/// Restarts a supervised helper now (the recovery overlay's Restart
/// Shell): SIGKILL, since it may be hung, and a start as soon as it has
/// gone, without the usual backoff. Apps are untouched.
pub fn restart_supervised(state: &mut State, name: &str) -> Result<String, String> {
    let s = state
        .az
        .children
        .supervised
        .iter_mut()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("{name} isn't running under Aizome"))?;
    s.backoff = Duration::from_secs(1);
    s.restart_now = true;
    match &mut s.child {
        Some(child) => {
            let pid = child.id();
            // Safety: kill with a valid signal, to a child of ours.
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
            info!(pid, "restarting {name}");
        }
        None => s.restart_at = Some(Instant::now()),
    }
    // Come back for it shortly rather than at the next second (the usual
    // supervision carries on if it takes longer to go).
    let until = Instant::now() + Duration::from_secs(5);
    let _ =
        state.az.lh.insert_source(Timer::from_duration(Duration::from_millis(50)), move |_, _, state: &mut State| {
            supervise(state);
            if Instant::now() < until && state.az.children.supervised.iter().any(|s| s.restart_now) {
                TimeoutAction::ToDuration(Duration::from_millis(50))
            } else {
                TimeoutAction::Drop
            }
        });
    Ok(format!("Restarting {name}"))
}

/// Stops the helpers when the compositor quits.
pub fn stop_children(az: &mut Aizome) {
    for s in &mut az.children.supervised {
        for c in s.child.iter_mut().chain(s.retiring.iter_mut()) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_seat_known_to_systemd_is_a_real_session() {
        let real = |args: &[&str]| real_session(&crate::cli::parse(args.iter().map(|s| s.to_string())).unwrap());
        assert!(real(&["--backend", "udev"]));
        assert!(!real(&["--backend", "udev", "--no-session"]));
        assert!(!real(&["--backend", "headless"]));
        assert!(!real(&["--backend", "winit"]));
    }
}
