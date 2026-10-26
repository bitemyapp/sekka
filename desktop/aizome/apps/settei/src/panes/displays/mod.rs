//! Displays: the arrangement (drag displays beside each other; their edges
//! snap and they never overlap), and the selected display's settings: what
//! it's used as (the main display, extending the desktop, mirroring
//! another, or off), its resolution as scaled sizes (whole scales first,
//! fractional ones when asked for) or every resolution, its refresh rate,
//! rotation, and the built-in panel's brightness.
//!
//! Changes apply at once through the compositor's `output-config`. Those
//! that can leave a screen blank (resolution, refresh rate, rotation,
//! mirroring, turning one on or off) ask "Keep these display settings?" and
//! go back after 15 seconds unanswered, since a blank screen can't answer.
//! Kept settings are written to `settings.toml`'s `[[outputs]]` by make,
//! model and serial, so they follow the display to another port.
//!
//! Night Light (warmer colours at night) shows when the compositor says a
//! display can show it (its CRTC takes a gamma table); otherwise the pane
//! says why there's none rather than offering a switch that does nothing.
//!
//! Ported from Tatami's Displays (`desktop/tatami/displays`: model.rs,
//! arrangement.rs, window.rs) with Hyprland's monitor rules replaced by
//! Aizome's compositor IPC. A mirror is marked `mirror = "…"` in its
//! `[[outputs]]` entry and the main display `primary = true`; the
//! compositor mirrors and places windows from those.

pub mod arrangement;
pub mod model;
pub mod system;
mod view;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use aizome_ipc::{OutputInfo, Request};
use aizome_kit::TimerHandle;
use aizome_kit::prelude::*;
use aizome_sys::power::Backlight;
use aizome_sys::{Error, Task};

use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use model::{Display, Settings, Use};
use system::Compositor;

/// How long "Keep these display settings?" waits before going back.
pub const KEEP_SECONDS: u32 = 15;

/// A risky change waiting to be kept.
#[derive(Clone, Debug)]
pub struct Keep {
    pub before: Vec<Settings>,
    pub before_main: Option<String>,
    pub names: Vec<String>,
    pub left: u32,
    timer: Option<TimerHandle>,
}

/// The built-in panel's brightness: what's shown, and setting it without
/// a flood of requests while a slider is dragged.
#[derive(Default)]
struct Brightness {
    live: Option<Backlight>,
    /// The sample's level.
    sample: f32,
    /// A level set while another is on its way.
    pending: Option<f32>,
    in_flight: bool,
    /// The level last asked for, shown until the backlight reports it.
    shown: Option<f32>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Loaded(u64, Result<Vec<OutputInfo>, String>),
    Outputs(Vec<OutputInfo>),
    Select(String),
    Moved((String, i32, i32)),
    UseAs(String, Use),
    Resolution(String, (i32, i32)),
    Refresh(String, i32),
    Scale(String, f64),
    Rotation(String, usize),
    ShowAll(bool),
    Fractional(bool),
    Brightness(f32),
    BrightnessDone(Result<f32, Error>),
    Applied {
        result: Result<Vec<OutputInfo>, String>,
        after: Vec<Settings>,
        main: Option<String>,
        names: Vec<String>,
        /// The settings before, to go back to unless kept.
        confirm: Option<(Vec<Settings>, Option<String>)>,
        persist: bool,
    },
    Tick,
    Keep,
    Revert,
    /// What the compositor says of Night Light.
    Night(Option<aizome_ipc::NightLightState>),
    NightLight(bool),
}

pub struct Displays {
    comp: Option<Compositor>,
    displays: Vec<Display>,
    main: Option<String>,
    selected: String,
    loaded: bool,
    error: Option<String>,
    applying: bool,
    revision: u64,
    keep: Option<Keep>,
    show_all: bool,
    fractional: bool,
    watching: Option<Arc<AtomicBool>>,
    /// The compositor's list as last read, to read again with the settings
    /// when they change on disk.
    outputs: Vec<OutputInfo>,
    brightness: Brightness,
    /// Night Light as the compositor last told it (none without one).
    night: Option<aizome_ipc::NightLightState>,
}

impl Default for Displays {
    fn default() -> Displays {
        Displays {
            comp: None,
            displays: Vec::new(),
            main: None,
            selected: String::new(),
            loaded: false,
            error: None,
            applying: false,
            revision: 0,
            keep: None,
            show_all: false,
            fractional: false,
            watching: None,
            outputs: Vec::new(),
            brightness: Brightness { sample: 0.72, ..Brightness::default() },
            night: None,
        }
    }
}

impl Displays {
    fn settings(&self) -> Vec<Settings> {
        self.displays.iter().map(|d| d.settings.clone()).collect()
    }

    fn start(&mut self, cx: &mut PaneCx<Msg>) {
        if self.comp.is_none() {
            self.comp = Some(if cx.sample() {
                Compositor::Sample(Arc::new(std::sync::Mutex::new(system::sample_outputs())))
            } else {
                Compositor::Live
            });
            if !cx.sample() {
                self.brightness.live = Some(Backlight::new(cx.wake()));
            }
        }
    }

    fn read(&mut self, cx: &mut PaneCx<Msg>) {
        let Some(comp) = self.comp.clone() else { return };
        let revision = self.revision;
        cx.spawn(move || Msg::Loaded(revision, comp.outputs()));
    }

    /// The displays from the compositor's list, with what the settings
    /// file knows (mirrors, the main display, displays that are off).
    fn take(&mut self, outputs: Vec<OutputInfo>, cx: &PaneCx<Msg>) {
        self.outputs = outputs.clone();
        let entries = cx.settings().outputs.clone();
        let entry_for = |o: &OutputInfo| entries.iter().find(|e| e.matches(&o.name, &o.make, &o.model, &o.serial));
        let mut ds: Vec<Display> = outputs.iter().map(|o| Display::from_output(o, entry_for(o))).collect();
        // Displays turned off: the compositor doesn't list them.
        for d in &self.displays {
            if !ds.iter().any(|x| x.name == d.name) && !d.settings.enabled {
                ds.push(d.clone());
            }
        }
        let known = model::known_off(&entries, &ds);
        ds.extend(known);
        model::infer_mirrors(&mut ds);
        // A stable order: built-in first, then by connector.
        ds.sort_by(|a, b| b.is_builtin().cmp(&a.is_builtin()).then_with(|| a.name.cmp(&b.name)));
        let kept = entries
            .iter()
            .find(|e| model::is_primary(e))
            .and_then(|e| ds.iter().find(|d| d.config_key().matches(e)).map(|d| d.name.clone()));
        let settings: Vec<Settings> = ds.iter().map(|d| d.settings.clone()).collect();
        // The file's main display (it may have changed elsewhere), else the
        // compositor's, else the one shown.
        let live = outputs.iter().find(|o| o.primary).map(|o| o.name.clone());
        self.main = model::main_display(&settings, kept.as_deref().or(live.as_deref()).or(self.main.as_deref()));
        if !ds.iter().any(|d| d.name == self.selected) {
            self.selected = self.main.clone().or_else(|| ds.first().map(|d| d.name.clone())).unwrap_or_default();
        }
        if ds.iter().any(|d| !model::is_integer(d.settings.scale)) {
            self.fractional = true;
        }
        self.displays = ds;
        self.loaded = true;
    }

    /// Change the settings with `edit`; `confirm` asks to keep risky ones.
    fn change(
        &mut self,
        cx: &mut PaneCx<Msg>,
        confirm: bool,
        edit: impl FnOnce(&mut Vec<Settings>, &mut Option<String>),
    ) {
        if self.applying || self.keep.is_some() {
            cx.say("A display change is still in progress");
            return;
        }
        let before = self.settings();
        let before_main = self.main.clone();
        let mut after = before.clone();
        let mut main = before_main.clone();
        edit(&mut after, &mut main);
        self.commit(before, before_main, after, main, confirm, true, cx);
    }

    /// Change one display.
    fn edit(&mut self, cx: &mut PaneCx<Msg>, name: &str, confirm: bool, edit: impl FnOnce(&mut Settings)) {
        let name = name.to_owned();
        self.change(cx, confirm, move |settings, _| {
            let Some(i) = settings.iter().position(|s| s.name == name) else { return };
            let old = settings[i].logical_size();
            edit(&mut settings[i]);
            if settings[i].logical_size() != old {
                model::resized(settings, &name, old);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn commit(
        &mut self,
        before: Vec<Settings>,
        before_main: Option<String>,
        after: Vec<Settings>,
        main: Option<String>,
        confirm: bool,
        persist: bool,
        cx: &mut PaneCx<Msg>,
    ) {
        let names = model::changed(&before, &after);
        if names.is_empty() {
            if main != before_main {
                self.main = main;
                if persist {
                    self.persist(&[], cx);
                }
            }
            return;
        }
        let confirm = confirm && model::risky(&before, &after);
        let requests: Vec<Request> = model::requests(&before, &after, &names);
        let Some(comp) = self.comp.clone() else { return };
        self.applying = true;
        self.revision += 1;
        // Shown as asked at once, so nothing jumps back while it applies.
        self.assume(&after);
        self.main = main.clone();
        let confirm = confirm.then_some((before, before_main));
        cx.spawn(move || Msg::Applied { result: comp.apply(&requests), after, main, names, confirm, persist });
    }

    fn assume(&mut self, after: &[Settings]) {
        for s in after {
            if let Some(d) = self.displays.iter_mut().find(|d| d.name == s.name) {
                d.settings = s.clone();
            }
        }
    }

    /// Write the displays named (and the main display) to `[[outputs]]`.
    fn persist(&mut self, names: &[String], cx: &mut PaneCx<Msg>) {
        // The sample's displays are made up: they don't go in the file.
        if cx.sample() {
            return;
        }
        let keys = model::config_keys(&self.displays);
        let entries: Vec<(model::ConfigKey, Settings)> = self
            .displays
            .iter()
            .zip(keys)
            .filter(|(d, _)| names.contains(&d.name) || self.main.as_deref() == Some(d.name.as_str()))
            .map(|(d, k)| (k, d.settings.clone()))
            .collect();
        let main = self.main.clone();
        cx.change(move |s| model::write_entries(&mut s.outputs, &entries, main.as_deref()));
    }

    /// What the compositor didn't do of what it was asked.
    fn refused(&self, after: &[Settings], names: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for s in after.iter().filter(|s| names.contains(&s.name)) {
            let Some(d) = self.displays.iter().find(|d| d.name == s.name) else { continue };
            let now = &d.settings;
            if s.enabled != now.enabled {
                out.push(format!("turning {} {}", d.title(), if s.enabled { "on" } else { "off" }));
            } else if s.enabled {
                if s.mode != now.mode {
                    out.push(format!("{}'s resolution", d.title()));
                }
                if (s.scale - now.scale).abs() > 1e-3 {
                    out.push(format!("{}'s scale", d.title()));
                }
                if s.transform != now.transform {
                    out.push(format!("{}'s rotation", d.title()));
                }
            }
        }
        out
    }

    fn end_keep(&mut self, cx: &mut PaneCx<Msg>) -> Option<Keep> {
        let mut keep = self.keep.take()?;
        if let Some(t) = keep.timer.take() {
            cx.cancel(t);
        }
        cx.close_sheet();
        Some(keep)
    }

    fn revert(&mut self, cx: &mut PaneCx<Msg>) {
        let Some(keep) = self.end_keep(cx) else { return };
        let current = self.settings();
        let mut back = current.clone();
        for prev in &keep.before {
            if keep.names.contains(&prev.name)
                && let Some(slot) = back.iter_mut().find(|s| s.name == prev.name)
            {
                *slot = prev.clone();
            }
        }
        let main = self.main.clone();
        // The file still has what was there before: nothing to write.
        self.commit(current, main, back, keep.before_main, false, false, cx);
    }

    fn set_brightness(&mut self, level: f32, cx: &mut PaneCx<Msg>) {
        let level = level.clamp(0.01, 1.0);
        self.brightness.shown = Some(level);
        let Some(b) = &self.brightness.live else {
            self.brightness.sample = level;
            return;
        };
        if self.brightness.in_flight {
            self.brightness.pending = Some(level);
            return;
        }
        let Some(name) = b.snapshot().primary().map(|d| d.name.clone()) else { return };
        self.brightness.in_flight = true;
        let task: Task<f32> = b.set_fraction(&name, level);
        cx.wait(task, Msg::BrightnessDone);
    }

    /// The built-in panel's brightness, if it has a backlight.
    pub(crate) fn brightness_now(&self) -> Option<f32> {
        match &self.brightness.live {
            Some(b) => {
                let s = b.snapshot();
                let d = s.primary()?;
                Some(self.brightness.shown.unwrap_or_else(|| d.fraction()))
            }
            None if self.comp.as_ref().is_some_and(|c| matches!(c, Compositor::Sample(_))) => {
                Some(self.brightness.sample)
            }
            None => None,
        }
    }
}

impl Pane for Displays {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "displays"
    }

    fn title(&self) -> &'static str {
        "Displays"
    }

    fn icon(&self) -> Icon {
        Icon::Monitor
    }

    fn tile(&self) -> Tile {
        Tile::Ai
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["monitor", "screen", "resolution", "scale", "hidpi", "arrangement", "rotate", "mirror", "refresh rate", "hz"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("arrangement", "Arrangement")
                .keywords(&["arrange", "position", "layout", "drag", "left", "right"]),
            SettingEntry::new("use-as", "Use as").keywords(&[
                "main display",
                "primary",
                "extend",
                "mirror",
                "turn off",
                "disable",
            ]),
            SettingEntry::new("resolution", "Resolution").keywords(&[
                "scale",
                "scaling",
                "larger text",
                "more space",
                "hidpi",
                "retina",
            ]),
            SettingEntry::new("fractional", "Fractional scaling").keywords(&["1.5", "125%", "150%"]),
            SettingEntry::new("refresh-rate", "Refresh rate").keywords(&["hz", "120", "144", "frequency"]),
            SettingEntry::new("rotation", "Rotation").keywords(&[
                "rotate",
                "portrait",
                "landscape",
                "flip",
                "orientation",
            ]),
            SettingEntry::new("brightness", "Brightness").keywords(&["backlight", "dim"]),
            SettingEntry::new("night-light", "Night Light").keywords(&[
                "night shift",
                "warm",
                "blue light",
                "colour temperature",
                "color temperature",
            ]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        self.start(cx);
        self.read(cx);
        if !cx.sample() {
            cx.compositor(Request::NightLight { enabled: None, temperature: None }, night_state);
        }
        if self.watching.is_none()
            && let Some(comp) = &self.comp
        {
            let stop = Arc::new(AtomicBool::new(false));
            let tx = cx.stream();
            comp.watch(stop.clone(), move |outputs| tx.send(Msg::Outputs(outputs)));
            self.watching = Some(stop);
        }
    }

    fn hidden(&mut self, _cx: &mut PaneCx<Msg>) {
        if let Some(stop) = self.watching.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    fn settings_changed(&mut self, cx: &mut PaneCx<Msg>) {
        // `[[outputs]]` changed elsewhere (the main display, mirrors,
        // displays off): what shows is read again with them, unless a
        // change of the pane's own is on its way or waiting to be kept.
        if self.loaded && !self.applying && self.keep.is_none() && !self.outputs.is_empty() {
            let outputs = self.outputs.clone();
            self.take(outputs, cx);
        }
    }

    fn focus_entry(&mut self, entry: &str, _cx: &mut PaneCx<Msg>) {
        if entry == "fractional" {
            self.fractional = true;
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Night(state) => self.night = state,
            Msg::NightLight(on) => {
                // Kept for the next login, and on now.
                cx.change(|s| s.night_light.enabled = on);
                if !cx.sample() {
                    cx.compositor(Request::NightLight { enabled: Some(on), temperature: None }, night_state);
                }
            }
            Msg::Loaded(revision, result) => {
                // A reading from before a change would undo what it shows.
                if self.applying || revision != self.revision {
                    return;
                }
                match result {
                    Ok(outputs) => {
                        self.error = None;
                        self.take(outputs, cx);
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.loaded = true;
                    }
                }
            }
            Msg::Outputs(outputs) => {
                if !self.applying && self.keep.is_none() {
                    self.take(outputs, cx);
                }
            }
            Msg::Select(name) => self.selected = name,
            Msg::Moved((name, x, y)) => {
                self.selected = name.clone();
                self.change(cx, false, move |settings, _| model::place(settings, &name, x, y));
            }
            Msg::UseAs(name, used) => {
                self.change(cx, true, move |settings, main| model::use_as(settings, main, &name, used))
            }
            Msg::Resolution(name, (width, height)) => {
                let modes = self.displays.iter().find(|d| d.name == name).map(|d| d.modes.clone()).unwrap_or_default();
                self.edit(cx, &name, true, move |s| {
                    let rates = model::refresh_rates(&modes, width, height);
                    let refresh = model::nearest(&rates, s.mode.refresh).map(|i| rates[i]).unwrap_or(s.mode.refresh);
                    s.mode = model::Mode::new(width, height, refresh);
                    if !model::is_integer(s.scale) {
                        s.scale = model::clean_scale(s.scale, width, height);
                    }
                });
            }
            Msg::Refresh(name, refresh) => self.edit(cx, &name, true, move |s| s.mode.refresh = refresh),
            Msg::Scale(name, scale) => self.edit(cx, &name, false, move |s| s.scale = scale),
            Msg::Rotation(name, i) => {
                let t = model::ROTATIONS.get(i).map(|r| r.0).unwrap_or_default();
                self.edit(cx, &name, true, move |s| s.transform = t);
            }
            Msg::ShowAll(on) => self.show_all = on,
            Msg::Fractional(on) => self.fractional = on,
            Msg::Brightness(level) => self.set_brightness(level, cx),
            Msg::BrightnessDone(result) => {
                self.brightness.in_flight = false;
                if let Err(e) = result {
                    cx.failed("Couldn't change the brightness", e);
                }
                if let Some(next) = self.brightness.pending.take() {
                    self.set_brightness(next, cx);
                } else {
                    self.brightness.shown = None;
                }
            }
            Msg::Applied { result, after, main, names, confirm, persist } => {
                self.applying = false;
                match result {
                    Ok(outputs) => {
                        self.take(outputs, cx);
                        // A display turned off isn't listed: keep it as asked.
                        for s in after.iter().filter(|s| !s.enabled) {
                            if let Some(d) = self.displays.iter_mut().find(|d| d.name == s.name) {
                                d.settings = s.clone();
                            }
                        }
                        // Mirrors and the main display are the pane's own.
                        for s in &after {
                            if let Some(d) = self.displays.iter_mut().find(|d| d.name == s.name) {
                                d.settings.mirror = s.mirror.clone();
                            }
                        }
                        self.main = main.or(self.main.take());
                        let refused = self.refused(&after, &names);
                        if !refused.is_empty() {
                            cx.failed("The compositor didn't apply", refused.join(", "));
                        }
                        match confirm {
                            Some((before, before_main)) => {
                                let timer = cx.every(Duration::from_secs(1), Msg::Tick);
                                self.keep =
                                    Some(Keep { before, before_main, names, left: KEEP_SECONDS, timer: Some(timer) });
                                cx.open_sheet();
                            }
                            None if persist => self.persist(&names, cx),
                            None => {}
                        }
                    }
                    Err(e) => {
                        cx.failed("Couldn't change the displays", e);
                        self.revision += 1;
                        self.read(cx);
                    }
                }
            }
            Msg::Tick => {
                let done = match &mut self.keep {
                    Some(k) => {
                        k.left = k.left.saturating_sub(1);
                        k.left == 0
                    }
                    None => false,
                };
                if done {
                    self.revert(cx);
                    cx.say("Went back to the display settings before");
                }
            }
            Msg::Keep => {
                if let Some(keep) = self.end_keep(cx) {
                    self.persist(&keep.names, cx);
                }
            }
            Msg::Revert => self.revert(cx),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        view::page(self, vcx)
    }

    fn sheet(&self, _vcx: &ViewCx) -> Option<Element<Msg>> {
        self.keep.as_ref().map(view::keep_sheet)
    }

    fn back(&mut self, cx: &mut PaneCx<Msg>) -> bool {
        // Escape over the question means "go back to how it was".
        if self.keep.is_some() {
            self.revert(cx);
            return true;
        }
        false
    }
}

/// What views read.
impl Displays {
    pub(crate) fn displays_now(&self) -> &[Display] {
        &self.displays
    }

    pub(crate) fn main_now(&self) -> Option<&str> {
        self.main.as_deref()
    }

    pub(crate) fn selected_now(&self) -> &str {
        &self.selected
    }

    pub(crate) fn error_now(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub(crate) fn is_applying(&self) -> bool {
        self.applying
    }

    pub(crate) fn show_all_now(&self) -> bool {
        self.show_all
    }

    pub(crate) fn fractional_now(&self) -> bool {
        self.fractional
    }
}

/// The compositor's answer to `night-light`, as the pane's message.
fn night_state(r: std::io::Result<aizome_ipc::Response>) -> Msg {
    match r {
        Ok(aizome_ipc::Response::NightLight { state }) => Msg::Night(Some(state)),
        _ => Msg::Night(None),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aizome_kit::testing::Harness;

    use super::Msg;
    use crate::app::{AppMsg, Settei, Start};

    fn open(name: &str) -> (Harness<Settei>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("settei-night-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        let mut h = Harness::new(Settei::sample(path.clone(), Start::parse("displays")));
        loaded(&mut h);
        (h, path)
    }

    /// The sample's displays read (on a thread of their own: real time
    /// passes too, or a busy machine doesn't get to it).
    pub(super) fn loaded(h: &mut Harness<Settei>) {
        let w = h.window();
        for _ in 0..400 {
            if h.texts(w).iter().any(|t| t == "Use as") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.advance(std::time::Duration::from_millis(10));
        }
        panic!("the sample's displays never showed: {:?}", h.texts(w));
    }

    fn night(outputs: &[&str]) -> AppMsg {
        let state = aizome_ipc::NightLightState {
            temperature: 4000,
            schedule: "sunset".into(),
            current: 6500,
            outputs: outputs.iter().map(|o| (*o).to_owned()).collect(),
            from: Some("18:57".into()),
            to: Some("08:15".into()),
            place: Some("Europe/Paris".into()),
            ..Default::default()
        };
        AppMsg::Pane("displays", Arc::new(Msg::Night(Some(state))))
    }

    #[test]
    fn displays_without_gamma_say_why_theres_no_night_light() {
        let (mut h, _) = open("none");
        let w = h.window();
        h.send(night(&[]));
        let texts = h.texts(w);
        assert!(texts.iter().any(|t| t == "Not on these displays"), "{texts:?}");
        assert!(h.find_id(w, "entry:displays.night-light").is_some());
    }

    #[test]
    fn a_display_with_gamma_gets_the_switch_and_it_is_kept() {
        let (mut h, path) = open("gamma");
        let w = h.window();
        h.send(night(&["eDP-1"]));
        assert!(!h.texts(w).iter().any(|t| t == "Not on these displays"));
        assert!(
            h.texts(w).iter().any(|t| t
                == "From sunset (18:57) to sunrise (08:15) in Paris, warmer colours, easier on the eyes at night.")
        );
        h.send(AppMsg::Pane("displays", Arc::new(Msg::NightLight(true))));
        h.program().shared.flush();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("[night_light]") && saved.contains("enabled = true"), "{saved}");
    }
}

#[cfg(test)]
mod scroll_tests {
    use aizome_kit::layout::{Point, Vec2};
    use aizome_kit::testing::Harness;

    use crate::app::{AppMsg, Settei, Start};

    #[test]
    fn night_light_arriving_late_can_be_scrolled_to() {
        let dir = std::env::temp_dir().join(format!("settei-scroll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = Harness::new(Settei::sample(dir.join("settings.toml"), Start::parse("displays")));
        let w = h.window();
        super::tests::loaded(&mut h);
        let state = aizome_ipc::NightLightState::default();
        h.send(AppMsg::Pane("displays", std::sync::Arc::new(super::Msg::Night(Some(state)))));
        let size = h.size(w);
        for _ in 0..12 {
            h.scroll(w, Point::new(size.width - 100.0, size.height / 2.0), Vec2::new(0.0, 120.0));
        }
        let row = h.inspected(w).into_iter().find(|e| e["name"] == "entry:displays.night-light").expect("the row");
        assert_eq!(row["visible"], true, "{row}");
    }
}
