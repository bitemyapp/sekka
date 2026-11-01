//! Answers to IPC requests.

use std::path::PathBuf;

use aizome_ipc::{LogicStats, ReloadPart, ReloadReport, Request, Response, Stats, Synthetic};
use smithay::utils::Point;

use crate::Rect;
use crate::outputs::{OutputChange, transform_from_ipc};
use crate::state::State;

impl State {
    pub fn handle_request(&mut self, req: Request) -> Response {
        if let Some(r) = crate::recovery::handle_request(self, &req) {
            return r;
        }
        match self.request(req) {
            Ok(r) => r,
            Err(e) => Response::Error { message: e },
        }
    }

    fn request(&mut self, req: Request) -> Result<Response, String> {
        Ok(match req {
            Request::Version => Response::Version {
                version: env!("CARGO_PKG_VERSION").into(),
                protocol: aizome_ipc::PROTOCOL_VERSION,
                pid: std::process::id(),
                exe: std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()),
                build: Some(crate::logic::build_info()),
            },
            Request::Outputs => Response::Outputs { outputs: crate::outputs::all_info(&self.az) },
            Request::Keyboards => Response::Keyboards { keyboards: self.az.keyboards.info() },
            Request::CatchKey { .. } | Request::CatchKeys { .. } => {
                return Err("the connection that waits for the keys asks".into());
            }
            Request::Spawn { argv, cwd, in_desk_folder } => {
                let cwd = cwd.map(|c| aizome_config::expand_home(&c)).or_else(|| {
                    if in_desk_folder { self.az.view.desk_folder.clone().filter(|p| p.is_dir()) } else { None }
                });
                let pid = crate::session::spawn(&self.az, &argv, cwd)?;
                Response::Spawned { pid }
            }
            Request::Screenshot { output, path, window, area, clipboard } => {
                use crate::screenshot::What;
                let what = if let Some(id) = window {
                    if !self.az.windows.contains_key(&id) {
                        return Err(format!("no window {id}"));
                    }
                    What::Window(id)
                } else if let Some(a) = area {
                    What::Area(Rect::new(a.x, a.y, a.w, a.h))
                } else if let Some(name) = output {
                    What::Output(
                        self.az
                            .known_output(&name)
                            .filter(|o| !self.az.disabled_outputs.contains(o))
                            .ok_or_else(|| format!("no output {name}"))?,
                    )
                } else {
                    What::Screen
                };
                let path = crate::screenshot::take(self, what, path.map(std::path::PathBuf::from), clipboard)?;
                Response::Screenshot { path: path.to_string_lossy().into_owned() }
            }
            Request::ReloadConfig => {
                self.reload_config()?;
                Response::Ok
            }
            Request::OutputConfig { name, mode, scale, position, transform, enabled } => {
                let mode = match mode {
                    Some(m) => Some(aizome_config::parse_mode(&m).ok_or_else(|| format!("bad mode {m}"))?),
                    None => None,
                };
                let change = OutputChange {
                    name,
                    mode,
                    scale,
                    position: position.map(|p| (p[0], p[1])),
                    transform: transform.map(transform_from_ipc),
                    enabled,
                };
                self.configure_output(&change)?;
                Response::Ok
            }
            Request::MirrorOutput { name, source } => {
                self.mirror_output(&name, source.as_deref())?;
                Response::Ok
            }
            Request::SetPrimaryOutput { name } => {
                self.az.output_by_name(&name).ok_or_else(|| format!("no output {name} in the layout"))?;
                self.az.primary_override = Some(name);
                crate::outputs::outputs_changed(&mut self.az);
                Response::Ok
            }
            Request::NightLight { enabled, temperature } => {
                if let Some(t) = temperature {
                    if !(1900..=6500).contains(&t) {
                        return Err(format!("{t} K is out of range (1900–6500)"));
                    }
                    self.az.night.temperature = Some(t);
                }
                if let Some(e) = enabled {
                    self.az.night.enabled = Some(e);
                }
                if enabled.is_some() || temperature.is_some() {
                    self.update_night_light(true);
                }
                Response::NightLight { state: self.night_state() }
            }
            Request::Stats { reset } => {
                let outputs = self
                    .az
                    .outputs
                    .iter()
                    .filter_map(|o| self.az.out.get(o).map(|s| s.stats.report(&o.name())))
                    .collect();
                let renderer = self.backend.renderer_name();
                let choice = self.az.settings.renderer_choice();
                let renderer_after_restart = choice != self.az.startup_renderer;
                let renderer_setting = match choice {
                    aizome_config::RendererChoice::Auto => "auto",
                    aizome_config::RendererChoice::Gpu => "gpu",
                    aizome_config::RendererChoice::Software => "software",
                };
                let stats = Stats {
                    renderer,
                    backend: self.backend.name().into(),
                    outputs,
                    renderer_setting: renderer_setting.into(),
                    renderer_after_restart,
                    logic: self.logic_stats(),
                    screencopy: self.az.screencopy.active().then(|| self.az.screencopy.stats.report("screencopy")),
                };
                if reset {
                    for o in self.az.out.values_mut() {
                        o.stats.reset();
                    }
                    self.az.screencopy.stats.reset();
                }
                Response::Stats { stats }
            }
            Request::Quit => {
                self.quit();
                Response::Ok
            }
            Request::PowerOffMonitors => {
                self.set_monitors_power(false);
                Response::Ok
            }
            Request::PowerOnMonitors => {
                self.set_monitors_power(true);
                Response::Ok
            }
            Request::Lock => {
                self.az.ipc.emit(&aizome_ipc::Event::Binding { action: "lock".into(), pointer: None, output: None });
                Response::Ok
            }
            Request::Subscribe => Response::Ok,
            Request::Shares => Response::Shares { shares: crate::protocols::image_copy_capture::shares(&self.az) },
            Request::StopShare { id } => match crate::protocols::image_copy_capture::stop(self, id) {
                0 => return Err(id.map_or("nothing is shared".into(), |id| format!("no share {id}"))),
                _ => Response::Ok,
            },
            Request::Synthesize { events } => {
                for e in events {
                    self.synthesize(e)?;
                }
                Response::Ok
            }
            Request::Action { action } if action == "reload" => {
                // As ⌃❖R does (the palette's Reload Aizome sends this): what
                // happened, in a notification too.
                let report = self.reload(&ReloadArgs::default());
                self.notify("Aizome reloaded", &reload_summary(&report));
                Response::Reload { report }
            }
            Request::Reload { logic, shell, from, shell_from, check } => {
                let args = ReloadArgs {
                    logic,
                    shell,
                    from: from.map(PathBuf::from),
                    shell_from: shell_from.map(PathBuf::from),
                    check,
                };
                Response::Reload { report: self.reload(&args) }
            }
            Request::DebugStall { seconds } => {
                tracing::warn!(seconds, "stalling the main loop, as asked");
                std::thread::sleep(std::time::Duration::from_secs(seconds.min(60) as u64));
                Response::Ok
            }
            Request::Recovery { .. }
            | Request::RecoveryState
            | Request::RecoveryAction { .. }
            | Request::InputState => {
                return Err("the recovery overlay answers this".into());
            }
            other => {
                // The window manager's: the logic answers.
                if self.lg.healthy() {
                    let answer = self.lg_call(Some(Response::error("the compositor's logic failed")), |vt, st| {
                        (vt.request)(st, &other)
                    });
                    answer.ok_or_else(|| format!("unknown request {other:?}"))?
                } else {
                    crate::fallback::request(self, &other)?
                }
            }
        })
    }

    fn synthesize(&mut self, e: Synthetic) -> Result<(), String> {
        let time = self.az.start.elapsed().as_millis() as u32;
        match e {
            Synthetic::PointerMotion { x, y } => self.pointer_motion_absolute(Point::from((x, y)), time),
            Synthetic::PointerButton { button, pressed } => self.pointer_button(button_code(&button)?, pressed, time),
            Synthetic::Click { button } => {
                let b = button_code(&button)?;
                self.pointer_button(b, true, time);
                self.pointer_button(b, false, time);
            }
            Synthetic::PointerScroll { dx, dy } => self.pointer_scroll(dx, dy, time),
            Synthetic::Key { key, pressed } => {
                self.key_from(None);
                self.keyboard_key(key_code(&key)?, pressed, time);
            }
            Synthetic::Chord { keys } => {
                let codes: Vec<u32> = keys.split('+').map(|k| key_code(k.trim())).collect::<Result<_, _>>()?;
                self.key_from(None);
                for c in &codes {
                    self.keyboard_key(*c, true, time);
                }
                for c in codes.iter().rev() {
                    self.keyboard_key(*c, false, time);
                }
            }
            Synthetic::Lid { closed } => self.lid_switched(closed),
            Synthetic::PlugKeyboard { name, id, bus } => {
                let (vendor, product) = id
                    .split_once(':')
                    .and_then(|(v, p)| Some((u32::from_str_radix(v, 16).ok()?, u32::from_str_radix(p, 16).ok()?)))
                    .ok_or_else(|| format!("{id}: say vendor:product, as 05ac:029c"))?;
                let k = crate::keyboards::Keyboard::new(&name, vendor, product, bus);
                self.keyboard_added(format!("synthetic:{name}"), k);
            }
            Synthetic::UnplugKeyboard { name } => self.keyboard_removed(&format!("synthetic:{name}")),
            Synthetic::KeyOn { keyboard, key, pressed } => {
                self.key_from(Some(&format!("synthetic:{keyboard}")));
                self.keyboard_key(key_code(&key)?, pressed, time);
            }
        }
        Ok(())
    }

    /// What the logic is and how it has fared, for `stats`.
    pub fn logic_stats(&self) -> LogicStats {
        LogicStats {
            build: self.lg.build().unwrap_or_default().to_string(),
            path: self.lg.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            loaded_at: self
                .lg
                .loaded_at()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
            healthy: self.lg.healthy(),
            reloads: self.lg.reloads,
            rollbacks: self.lg.rollbacks,
            last_error: self.lg.last_error.clone(),
            core_abi: crate::ABI.to_string(),
        }
    }

    /// Reloads what was asked for (or, asked for nothing, what changed):
    /// the logic, the shell, the settings. With `check`, says what would
    /// happen and does nothing.
    pub fn reload(&mut self, args: &ReloadArgs) -> ReloadReport {
        let everything = !args.logic && !args.shell && args.from.is_none() && args.shell_from.is_none();
        let mut report = ReloadReport::default();

        // The core can't be reloaded: the installed one (after an update)
        // may differ from the one running.
        let installed = crate::logic::installed_logic(None);
        let installed_abi = installed.as_ref().map(|p| crate::logic::read_abi(p));
        report.core = match &installed_abi {
            Some(Ok(abi)) if abi == crate::ABI => ReloadPart::new("same", ""),
            Some(Ok(_)) => ReloadPart::new(
                "needs-restart",
                "the installed compositor core differs from the running one: log out and in to use it",
            ),
            Some(Err(e)) => ReloadPart::new("skipped", e.clone()),
            None => ReloadPart::new("same", ""),
        };

        // The logic: the one asked for, else the installed one.
        let candidate = args.from.clone().or(installed);
        report.logic = if args.logic || args.from.is_some() || everything {
            match candidate {
                None => ReloadPart::new("skipped", "no logic library found"),
                Some(path) => match crate::logic::read_abi(&path) {
                    Err(e) => ReloadPart::new("refused", e),
                    Ok(abi) if abi != crate::ABI => {
                        let why = if args.from.is_none() {
                            "the installed logic was built against another compositor core; it loads at the next login"
                                .to_string()
                        } else {
                            format!(
                                "{} was built against another compositor core (its ABI {}…, the running core's {}…); \
                                 build it as the session was built (aizome-dev)",
                                path.display(),
                                &abi[..12.min(abi.len())],
                                &crate::ABI[..12]
                            )
                        };
                        ReloadPart::new("refused", why)
                    }
                    Ok(_) if everything && self.logic_is(&path) => {
                        ReloadPart::new("unchanged", format!("{} is the one running", path.display()))
                    }
                    Ok(_) if args.check => ReloadPart::new("would-reload", format!("from {}", path.display())),
                    Ok(_) => match self.load_logic(&path, true) {
                        Ok(o) => ReloadPart::new(
                            "reloaded",
                            format!("logic {} from {} in {} ms", o.build, o.path.display(), o.millis),
                        ),
                        Err(e) => ReloadPart::new("failed", e),
                    },
                },
            }
        } else {
            ReloadPart::new("skipped", "")
        };

        // The shell.
        report.shell = if args.shell || args.shell_from.is_some() || everything {
            let changed = crate::session::shell_binary_changed(&self.az);
            match changed {
                None => ReloadPart::new("skipped", "no shell is running"),
                Some(false) if everything => ReloadPart::new("unchanged", "the running shell is the installed one"),
                Some(_) if args.check => match &args.shell_from {
                    Some(p) if !crate::session::is_executable(p) => {
                        ReloadPart::new("refused", format!("{} isn't a program", p.display()))
                    }
                    Some(p) => ReloadPart::new("would-reload", format!("restart the shell as {}", p.display())),
                    None => ReloadPart::new("would-reload", "restart the shell"),
                },
                Some(_) => match self.restart_shell(args.shell_from.clone()) {
                    Ok(d) => ReloadPart::new("restarted", d),
                    Err(e) => ReloadPart::new("failed", e),
                },
            }
        } else {
            ReloadPart::new("skipped", "")
        };

        // The settings (live anyway; read again in case the watcher
        // missed a change).
        report.settings = if everything {
            if args.check {
                ReloadPart::new("would-reload", "")
            } else {
                match self.reload_config() {
                    Ok(()) => ReloadPart::new("reloaded", "settings, theme and key bindings"),
                    Err(e) => ReloadPart::new("failed", e),
                }
            }
        } else {
            ReloadPart::new("skipped", "")
        };
        if !args.check {
            self.az.ipc.emit(&aizome_ipc::Event::Reloaded { report: report.clone() });
        }
        report
    }

    /// Whether the loaded logic came from this file, unchanged.
    fn logic_is(&self, path: &std::path::Path) -> bool {
        let Some(running) = self.lg.path() else { return false };
        let same_file = std::fs::canonicalize(running).ok() == std::fs::canonicalize(path).ok();
        same_file
            && self
                .lg
                .loaded_at()
                .is_some_and(|t| std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|m| m <= t))
    }
}

/// What `reload` is asked to do.
#[derive(Clone, Debug, Default)]
pub struct ReloadArgs {
    pub logic: bool,
    pub shell: bool,
    pub from: Option<PathBuf>,
    pub shell_from: Option<PathBuf>,
    pub check: bool,
}

/// A reload's report in a line or two (notifications, the log).
pub fn reload_summary(r: &ReloadReport) -> String {
    let mut lines = Vec::new();
    for (what, part) in [("logic", &r.logic), ("shell", &r.shell), ("settings", &r.settings), ("core", &r.core)] {
        if part.action == "skipped" && part.detail.is_empty() {
            continue;
        }
        if part.detail.is_empty() {
            lines.push(format!("{what}: {}", part.action));
        } else {
            lines.push(format!("{what}: {} ({})", part.action, part.detail));
        }
    }
    lines.join("\n")
}

fn button_code(name: &str) -> Result<u32, String> {
    Ok(match name {
        "left" | "" => crate::input::BTN_LEFT,
        "right" => crate::input::BTN_RIGHT,
        "middle" => crate::input::BTN_MIDDLE,
        n => n.parse().map_err(|_| format!("unknown button {n}"))?,
    })
}

/// evdev key codes by name.
pub fn key_code(name: &str) -> Result<u32, String> {
    if let Ok(n) = name.parse::<u32>()
        && name.len() > 1
    {
        return Ok(n);
    }
    let lower = name.to_lowercase();
    let code = match lower.as_str() {
        "esc" | "escape" => 1,
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        "minus" | "-" => 12,
        "equal" | "=" => 13,
        "backspace" => 14,
        "tab" => 15,
        "q" => 16,
        "w" => 17,
        "e" => 18,
        "r" => 19,
        "t" => 20,
        "y" => 21,
        "u" => 22,
        "i" => 23,
        "o" => 24,
        "p" => 25,
        "leftbrace" | "[" => 26,
        "rightbrace" | "]" => 27,
        "return" | "enter" => 28,
        "ctrl" | "control" | "leftctrl" => 29,
        "a" => 30,
        "s" => 31,
        "d" => 32,
        "f" => 33,
        "g" => 34,
        "h" => 35,
        "j" => 36,
        "k" => 37,
        "l" => 38,
        "semicolon" | ";" => 39,
        "apostrophe" | "'" => 40,
        "grave" | "`" => 41,
        "shift" | "leftshift" => 42,
        "backslash" | "\\" => 43,
        "z" => 44,
        "x" => 45,
        "c" => 46,
        "v" => 47,
        "b" => 48,
        "n" => 49,
        "m" => 50,
        "comma" | "," => 51,
        "dot" | "period" | "." => 52,
        "slash" | "/" => 53,
        "rightshift" => 54,
        "alt" | "option" | "leftalt" => 56,
        "space" => 57,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "f11" => 87,
        "f12" => 88,
        "print" | "sysrq" => 99,
        "home" => 102,
        "up" => 103,
        "pageup" => 104,
        "left" => 105,
        "right" => 106,
        "end" => 107,
        "down" => 108,
        "pagedown" => 109,
        "delete" => 111,
        "mute" => 113,
        "power" => 116,
        "volumedown" => 114,
        "volumeup" => 115,
        "super" | "logo" | "cmd" | "leftmeta" => 125,
        "back" => 158,
        "homepage" => 172,
        _ => return Err(format!("unknown key {name}")),
    };
    Ok(code)
}
