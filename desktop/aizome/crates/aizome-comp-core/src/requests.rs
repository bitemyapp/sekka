//! Answers to IPC requests.

use std::path::PathBuf;

use aizome_ipc::{LogicStats, ReloadPart, ReloadReport, Request, Response, Stats, Synthetic};
use smithay::input::keyboard::{Keycode, keysyms, xkb};
use smithay::utils::Point;

use crate::Rect;
use crate::input::MODIFIER_KEYS;
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
            Request::CatchKey { .. } => return Err("the connection that waits for the key asks".into()),
            Request::IdleInhibitors => Response::IdleInhibitors { inhibitors: self.idle_inhibitors() },
            Request::KeepAwake => return Err("the connection that holds as Keep Awake asks".into()),
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
                let codes = self.chord_codes(&keys)?;
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
            Synthetic::Idle { seconds } => self.synthesize_idle(std::time::Duration::from_secs(seconds.into())),
        }
        Ok(())
    }

    /// A chord's keys as evdev codes, read against the keymap in force (the
    /// last keyboard's, which the chord goes through): see
    /// [`chord_codes_in`].
    fn chord_codes(&mut self, keys: &str) -> Result<Vec<u32>, String> {
        let Some(kb) = self.az.seat.get_keyboard() else {
            return keys.split('+').map(|k| key_code(k.trim())).collect();
        };
        kb.with_xkb_state(self, |c| {
            let xkb = c.xkb().lock().unwrap();
            // SAFETY: the keymap is borrowed for these lookups, under the
            // lock, on the seat's thread; the states made from it to try
            // its keys are dropped before `chord_codes_in` returns.
            let keymap = unsafe { xkb.keymap() };
            chord_codes_in(keymap, xkb.active_layout().0, keys)
        })
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
        _ => return Err(format!("unknown key {name}")),
    };
    Ok(code)
}

/// The modifiers a chord names, as xkb names them.
const CHORD_MODIFIERS: [&str; 4] = [xkb::MOD_NAME_SHIFT, xkb::MOD_NAME_CTRL, xkb::MOD_NAME_ALT, xkb::MOD_NAME_LOGO];

/// What a key's name asks for, whichever key does it.
enum Named {
    /// One of [`CHORD_MODIFIERS`], held.
    Modifier(&'static str),
    /// What the key types alone (a keysym).
    Sym(u32),
}

/// What `name` means, apart from where a US keyboard has it. Nothing for
/// an evdev code, which is that key whatever it does, nor for the keys
/// with no meaning a layout could move (the volume keys, power).
fn named(name: &str) -> Option<Named> {
    use keysyms::*;
    let lower = name.to_lowercase();
    // A letter, a digit or a punctuation mark: itself.
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(Named::Sym(xkb::utf32_to_keysym(c as u32).raw()));
    }
    // F1 to F12: their keysyms follow one another as they do.
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=12).contains(&n)
    {
        return Some(Named::Sym(KEY_F1 + n - 1));
    }
    Some(Named::Sym(match lower.as_str() {
        "super" | "logo" | "cmd" | "leftmeta" => return Some(Named::Modifier(xkb::MOD_NAME_LOGO)),
        "ctrl" | "control" | "leftctrl" => return Some(Named::Modifier(xkb::MOD_NAME_CTRL)),
        "alt" | "option" | "leftalt" => return Some(Named::Modifier(xkb::MOD_NAME_ALT)),
        "shift" | "leftshift" | "rightshift" => return Some(Named::Modifier(xkb::MOD_NAME_SHIFT)),
        "esc" | "escape" => KEY_Escape,
        "minus" => KEY_minus,
        "equal" => KEY_equal,
        "backspace" => KEY_BackSpace,
        "tab" => KEY_Tab,
        "leftbrace" => KEY_bracketleft,
        "rightbrace" => KEY_bracketright,
        "return" | "enter" => KEY_Return,
        "semicolon" => KEY_semicolon,
        "apostrophe" => KEY_apostrophe,
        "grave" => KEY_grave,
        "backslash" => KEY_backslash,
        "comma" => KEY_comma,
        "dot" | "period" => KEY_period,
        "slash" => KEY_slash,
        "space" => KEY_space,
        "print" | "sysrq" => KEY_Print,
        "home" => KEY_Home,
        "up" => KEY_Up,
        "pageup" => KEY_Prior,
        "left" => KEY_Left,
        "right" => KEY_Right,
        "end" => KEY_End,
        "down" => KEY_Down,
        "pagedown" => KEY_Next,
        "delete" => KEY_Delete,
        _ => return None,
    }))
}

/// Whether a key (an xkb code, as `held`), pressed with the chord's
/// earlier keys held, holds this one of [`CHORD_MODIFIERS`] as well and
/// changes nothing else: no other of them, nor the layout (Shift, then the
/// Alt that switches layouts with it), nor what is locked (a Caps Lock
/// that is Shift Lock). A keymap says what a key types; what it holds,
/// only pressing it says, so the chord so far is pressed in a keyboard
/// state of its own.
fn holds(keymap: &xkb::Keymap, layout: xkb::LayoutIndex, held: &[u32], key: u32, modifier: &str) -> bool {
    let mut state = xkb::State::new(keymap);
    state.update_mask(0, 0, 0, 0, 0, layout);
    let mut press = |key: u32| {
        state.update_key(Keycode::new(key), xkb::KeyDirection::Down);
        let mods = CHORD_MODIFIERS.map(|m| state.mod_name_is_active(m, xkb::STATE_MODS_DEPRESSED));
        // And what the key must leave as it was.
        let kept = (state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE), state.serialize_mods(xkb::STATE_MODS_LOCKED));
        (mods, kept)
    };
    let (before, kept) = held.iter().map(|k| press(*k)).last().unwrap_or(([false; 4], (layout, 0)));
    let (after, kept_after) = press(key);
    kept_after == kept && (0..4).all(|i| after[i] == (before[i] || CHORD_MODIFIERS[i] == modifier))
}

/// The key (an xkb code) that does, under `keymap` with `layout` in use
/// and the chord's earlier keys `held`, what `name` says.
fn key_in(keymap: &xkb::Keymap, layout: xkb::LayoutIndex, held: &[u32], name: &str) -> Result<u32, String> {
    let usual = key_code(name)? + 8;
    let Some(named) = named(name) else { return Ok(usual) };
    let every = keymap.min_keycode().raw()..=keymap.max_keycode().raw();
    let found = match named {
        // After the usual key, the others a keyboard has for modifiers
        // (the ones sticky keys know), then any.
        Named::Modifier(m) => std::iter::once(usual)
            .chain(MODIFIER_KEYS.map(|c| c + 8))
            .chain(every)
            .find(|c| holds(keymap, layout, held, *c, m)),
        // In the layout in use, then in the keymap's others, where
        // bindings too read the Latin letter of a key that types none.
        Named::Sym(sym) => {
            std::iter::once(layout).chain((0..keymap.num_layouts()).filter(|l| *l != layout)).find_map(|l| {
                let types =
                    |c: &u32| keymap.key_get_syms_by_level(Keycode::new(*c), l, 0).iter().any(|s| s.raw() == sym);
                std::iter::once(usual).chain(every.clone()).find(types)
            })
        }
    };
    Ok(found.unwrap_or(usual))
}

/// A chord's keys (`"super+shift+3"`) as the evdev codes of the keys that
/// do, under `keymap` with `layout` in use, what its names say: the
/// modifier wherever a swap of modifier keys put it, the letter wherever
/// the layout has it. A chord means what it says (❖V pastes), and the
/// keymap of the keyboard that typed last reads it: with left Alt and
/// Super swapped, the key [`key_code`] calls `super` holds Alt, and under
/// Dvorak its `v` types k.
///
/// The key [`key_code`] gives comes first, so nothing moves while that key
/// does the job (a US keyboard as it comes), and it is the answer when no
/// key does (a layout without that letter). A modifier's key is one that
/// holds it with the chord's earlier keys held ([`holds`]). An evdev code
/// is that key.
fn chord_codes_in(keymap: &xkb::Keymap, layout: xkb::LayoutIndex, keys: &str) -> Result<Vec<u32>, String> {
    let mut held = Vec::new();
    for name in keys.split('+') {
        held.push(key_in(keymap, layout, &held, name.trim())?);
    }
    Ok(held.into_iter().map(|k| k - 8).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: &str = xkb::MOD_NAME_SHIFT;
    const CTRL: &str = xkb::MOD_NAME_CTRL;
    const ALT: &str = xkb::MOD_NAME_ALT;
    const SUPER: &str = xkb::MOD_NAME_LOGO;

    /// A keymap as the compositor compiles one (`layouts` and `variants`
    /// with commas between, as the settings join them).
    fn keymap(layouts: &str, variants: &str, options: &str) -> xkb::Keymap {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            layouts,
            variants,
            Some(options.into()),
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap_or_else(|| panic!("no keymap for {layouts} ({variants}) with {options}"))
    }

    fn codes(keymap: &xkb::Keymap, layout: u32, chord: &str) -> Vec<u32> {
        chord_codes_in(keymap, layout, chord).unwrap()
    }

    /// What an app reading `keymap` makes of the chord as the compositor
    /// would press it: the modifiers held as its last key comes, and what
    /// that key types alone. The layout stays the first.
    fn read(keymap: &xkb::Keymap, chord: &str) -> (Vec<&'static str>, String) {
        let codes = codes(keymap, 0, chord);
        let (last, held) = codes.split_last().unwrap();
        let mut state = xkb::State::new(keymap);
        for c in held {
            state.update_key(Keycode::new(c + 8), xkb::KeyDirection::Down);
        }
        assert_eq!(state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE), 0, "{chord} changed the layout");
        let mods =
            CHORD_MODIFIERS.into_iter().filter(|m| state.mod_name_is_active(m, xkb::STATE_MODS_EFFECTIVE)).collect();
        let typed = keymap.key_get_syms_by_level(Keycode::new(last + 8), 0, 0);
        (mods, typed.iter().map(|s| xkb::keysym_get_name(*s)).collect::<Vec<_>>().join(" "))
    }

    #[test]
    fn a_us_keyboard_as_it_comes_keeps_every_key_where_it_is() {
        let names = "esc escape 1 2 3 4 5 6 7 8 9 0 minus - equal = backspace tab q w e r t y u i o p leftbrace [ \
                     rightbrace ] return enter ctrl control leftctrl a s d f g h j k l semicolon ; apostrophe ' grave ` \
                     shift leftshift backslash \\ z x c v b n m comma , dot period . slash / rightshift alt option \
                     leftalt space f1 f2 f3 f4 f5 f6 f7 f8 f9 f10 f11 f12 print sysrq home up pageup left right end \
                     down pagedown delete mute power volumedown volumeup super logo cmd leftmeta A V F5 Super 125 47";
        let us = keymap("us", "", "");
        for name in names.split_whitespace() {
            assert_eq!(chord_codes_in(&us, 0, name), key_code(name).map(|c| vec![c]), "{name}");
            // All but the media keys and the evdev codes were looked for,
            // and found where they are.
            let fixed = ["mute", "power", "volumedown", "volumeup", "125", "47"].contains(&name);
            assert_eq!(named(name).is_none(), fixed, "{name}");
        }
        assert_eq!(codes(&us, 0, "ctrl + Shift+super+3"), [29, 42, 125, 4]);
        assert!(chord_codes_in(&us, 0, "super+hyperspace").is_err());
        assert!(chord_codes_in(&us, 0, "é").is_err());
    }

    #[test]
    fn a_swapped_modifier_is_pressed_where_the_swap_put_it() {
        // Each arrangement Settei offers, and Genkan's for a PC keyboard.
        for options in [
            "",
            "altwin:swap_alt_win",
            "altwin:swap_lalt_lwin",
            "altwin:swap_ralt_rwin",
            "ctrl:swap_lwin_lctl,ctrl:swap_rwin_rctl",
            "ctrl:swap_lwin_lctl",
            "ctrl:swap_rwin_rctl",
            "ctrl:swap_lalt_lctl,ctrl:swap_ralt_rctl",
            "ctrl:swap_lalt_lctl",
            "ctrl:swap_ralt_rctl",
            // Caps Lock and Control changed about; layout switching, on
            // keys the chords hold.
            "ctrl:swapcaps",
            "ctrl:nocaps,grp:alt_shift_toggle,altwin:swap_lalt_lwin",
            "grp:lctrl_lshift_toggle",
        ] {
            let k = keymap("us,de", "", options);
            for (chord, mods) in [
                // Koe's pastes.
                ("super+v", vec![SUPER]),
                ("ctrl+v", vec![CTRL]),
                ("ctrl+shift+v", vec![SHIFT, CTRL]),
                ("alt+v", vec![ALT]),
                ("shift+super+v", vec![SHIFT, SUPER]),
                ("ctrl+alt+super+v", vec![CTRL, ALT, SUPER]),
            ] {
                assert_eq!(read(&k, chord), (mods, "v".to_string()), "{chord} with {options}");
            }
        }
        // Which keys those are: left Alt for Super once they are swapped
        // (Genkan's swap for a PC keyboard), left Control likewise.
        let swapped = |options: &str, chord: &str| codes(&keymap("us", "", options), 0, chord);
        assert_eq!(swapped("altwin:swap_lalt_lwin", "super+v"), [56, 47]);
        assert_eq!(swapped("altwin:swap_alt_win", "super+v"), [56, 47]);
        assert_eq!(swapped("ctrl:swap_lwin_lctl", "super+v"), [29, 47]);
        assert_eq!(swapped("ctrl:swap_lwin_lctl,ctrl:swap_rwin_rctl", "ctrl+super+v"), [125, 29, 47]);
        // The side that wasn't swapped stays the answer.
        assert_eq!(swapped("altwin:swap_ralt_rwin", "alt+super+v"), [56, 125, 47]);
        // An evdev code is that key, whatever it holds now.
        assert_eq!(swapped("altwin:swap_lalt_lwin", "125+47"), [125, 47]);
    }

    #[test]
    fn a_modifier_is_one_that_holds_with_the_chords_earlier_keys() {
        // Shift, then the Alt key that is Super now, switches layouts where
        // Alt and Shift do: the other Super holds with Shift.
        let k = keymap("us,de", "", "grp:alt_shift_toggle,altwin:swap_lalt_lwin");
        assert_eq!(codes(&k, 0, "super+shift+3"), [56, 42, 4]);
        assert_eq!(codes(&k, 0, "shift+super+3"), [42, 126, 4]);
        // Koe's paste into a terminal, where left Control and left Shift
        // switch layouts: the right Shift.
        let k = keymap("us,de", "", "grp:lctrl_lshift_toggle");
        assert_eq!(codes(&k, 0, "ctrl+shift+v"), [29, 54, 47]);
        assert_eq!(codes(&k, 0, "shift+v"), [42, 47]);
        // No Shift holds with Control where both sides switch: the usual.
        let k = keymap("us,de", "", "grp:ctrl_shift_toggle");
        assert_eq!(codes(&k, 0, "ctrl+shift+v"), [29, 42, 47]);
        // Not Caps Lock as Shift Lock either, which holds Shift while it is
        // down and leaves it locked.
        let k = keymap("us,de", "", "grp:ctrl_shift_toggle,caps:shiftlock");
        let mut state = xkb::State::new(&k);
        state.update_key(Keycode::new(58 + 8), xkb::KeyDirection::Down);
        for held in [xkb::STATE_MODS_DEPRESSED, xkb::STATE_MODS_LOCKED] {
            assert!(state.mod_name_is_active(SHIFT, held), "Caps Lock as Shift Lock");
        }
        assert_eq!(codes(&k, 0, "ctrl+shift+v"), [29, 42, 47]);
    }

    #[test]
    fn a_letter_is_pressed_where_the_layout_has_it() {
        let dvorak = keymap("us", "dvorak", "");
        assert_eq!(codes(&dvorak, 0, "super+v"), [125, 52], "Dvorak's v is where a US keyboard has the full stop");
        assert_eq!(read(&dvorak, "super+v"), (vec![SUPER], "v".to_string()));
        assert_eq!(read(&dvorak, "shift+super+]"), (vec![SHIFT, SUPER], "bracketright".to_string()));
        assert_eq!(codes(&keymap("de", "", ""), 0, "ctrl+z"), [29, 21], "QWERTZ");
        assert_eq!(codes(&keymap("fr", "", ""), 0, "super+a+q+m"), [125, 16, 30, 39], "AZERTY");
        // With a swap as well.
        assert_eq!(codes(&keymap("us", "dvorak", "altwin:swap_lalt_lwin"), 0, "super+v"), [56, 52]);
        // A key no layout moves, moved by an option.
        assert_eq!(codes(&keymap("us", "", "caps:swapescape"), 0, "escape"), [58]);
    }

    #[test]
    fn a_key_the_layout_lacks_is_the_usual_one() {
        // German types ] with AltGr and French types digits with Shift:
        // no key types them alone.
        assert_eq!(codes(&keymap("de", "", ""), 0, "super+]"), [125, 27]);
        assert_eq!(codes(&keymap("fr", "", ""), 0, "ctrl+super+2"), [29, 125, 3]);
        // Russian has no v: the key that types it in the keymap's other
        // layout, whichever of the two is in use.
        let two = keymap("us,ru", "", "");
        assert_eq!(codes(&two, 0, "super+v"), [125, 47]);
        assert_eq!(codes(&two, 1, "super+v"), [125, 47]);
        let two = keymap("ru,us", ",dvorak", "");
        assert_eq!(codes(&two, 0, "super+v"), [125, 52]);
        assert_eq!(codes(&two, 1, "super+v"), [125, 52]);
        // The layout in use comes first.
        let two = keymap("us,us", ",dvorak", "");
        assert_eq!(codes(&two, 0, "v"), [47]);
        assert_eq!(codes(&two, 1, "v"), [52]);
    }
}
