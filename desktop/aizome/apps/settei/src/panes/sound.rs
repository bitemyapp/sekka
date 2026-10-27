//! Sound: the output and input devices (choose one; its volume, balance,
//! mute and port), a level meter for the input, each application's volume
//! and device, and the alert sound and its volume. Through PipeWire's
//! pipewire-pulse (`aizome-sys::audio`); the meter is
//! `aizome-sys::meter`, running only while the pane shows.
//!
//! The alert settings live in `settings.toml`'s `[sound]` table, which
//! this pane owns: `alert_sound` (a sound theme name such as `"bell"`),
//! `alert_volume` (0.0–1.0) and `volume_feedback` (a sound at the new
//! volume when the volume keys or the Sound panel change it). The shell
//! plays them through PipeWire: the alert for the system bell, the alert
//! volume for notifications' sounds too.

use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{TimerHandle, column, row};
use aizome_sys::audio::{Audio, AudioDevice, AudioState, AudioStream, DeviceState, MAX_VOLUME};
use aizome_sys::meter::{Meter, loudness};
use aizome_sys::{Error, Task};

use crate::form::{self, Row};
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::panes::hw::level_meter;
use crate::system::Cell;
use crate::widgets::{PressStyle, press, tile};

/// How often the input meter redraws.
const METER_EVERY: Duration = Duration::from_millis(80);
/// Segments of the input meter.
const SEGMENTS: usize = 24;

/// The sound server, live or the sample.
pub enum Snd {
    None,
    Live(Audio),
    Sample(Cell<AudioState>),
}

impl Snd {
    pub fn snapshot(&self) -> Arc<AudioState> {
        match self {
            Snd::None => Arc::new(AudioState::default()),
            Snd::Live(a) => a.snapshot(),
            Snd::Sample(c) => c.get(),
        }
    }

    fn sample(&self, f: impl FnOnce(&mut AudioState)) -> Task<()> {
        match self {
            Snd::Sample(c) => c.update(f),
            _ => Task::ready(Err(Error::Unavailable("the sound server".into()))),
        }
    }
}

/// Which list a device is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Io {
    Output,
    Input,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Default(Io, String),
    Volume(Io, u32, f32),
    Mute(Io, u32, bool),
    Balance(Io, u32, f32),
    Port(Io, u32, String),
    AppVolume(bool, u32, f32),
    AppMute(bool, u32, bool),
    AppDevice(bool, u32, u32),
    AlertSound(usize),
    AlertVolume(f32),
    /// The alert volume's slider let go: hear it.
    AlertVolumeSet,
    Feedback(bool),
    Meter,
    Done(String, Result<(), Error>),
}

pub struct Sound {
    snd: Snd,
    meter: Option<(String, Meter)>,
    meter_timer: Option<TimerHandle>,
    level: f32,
    peak: f32,
    /// The sample meter's clock.
    ticks: u32,
    sounds: Vec<(String, String)>,
}

impl Default for Sound {
    fn default() -> Sound {
        Sound { snd: Snd::None, meter: None, meter_timer: None, level: 0.0, peak: 0.0, ticks: 0, sounds: Vec::new() }
    }
}

/// The freedesktop sound theme's sounds found on this system, as (name,
/// file), with the usual ones first.
pub fn alert_sounds() -> Vec<(String, String)> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(std::path::PathBuf::from(home).join("sounds"));
    }
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/run/current-system/sw/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|d| !d.is_empty()).map(|d| std::path::PathBuf::from(d).join("sounds")));
    let mut found: Vec<(String, String)> = Vec::new();
    for d in dirs {
        let stereo = d.join("freedesktop").join("stereo");
        let Ok(entries) = std::fs::read_dir(&stereo) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
            if !found.iter().any(|(n, _)| n == stem) {
                found.push((stem.to_owned(), p.display().to_string()));
            }
        }
    }
    let order = ["bell", "message", "complete", "dialog-information", "dialog-warning", "message-new-instant"];
    found.sort_by_key(|(n, _)| (order.iter().position(|o| o == n).unwrap_or(order.len()), n.clone()));
    found
}

/// "dialog-warning" → "Dialog warning".
pub fn sound_label(name: &str) -> String {
    let words = name.replace(['-', '_'], " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The glyph for a device, from what it says it is.
fn device_icon(d: &AudioDevice, io: Io) -> Icon {
    let f = d.form_factor.as_str();
    let i = d.icon_name.as_str();
    if io == Io::Input {
        return if f == "headset" || i.contains("headset") { Icon::Headset } else { Icon::Mic };
    }
    if f == "headphone" || i.contains("headphone") {
        Icon::Headphones
    } else if f == "headset" || i.contains("headset") {
        Icon::Headset
    } else if d.bus == "bluetooth" {
        Icon::Headphones
    } else if f == "hdmi"
        || f == "tv"
        || d.name.contains("hdmi")
        || d.description.contains("HDMI")
        || d.description.contains("DisplayPort")
    {
        Icon::Tv
    } else if d.bus == "usb" {
        Icon::Usb
    } else {
        Icon::Speaker
    }
}

/// "Bluetooth", "USB", "HDMI", "Built-in".
fn bus_words(d: &AudioDevice) -> &'static str {
    match d.bus.as_str() {
        "bluetooth" => "Bluetooth",
        "usb" => "USB",
        _ if d.form_factor == "hdmi" || d.description.contains("HDMI") => "HDMI",
        "pci" | "" if d.form_factor == "internal" || d.form_factor.is_empty() => "Built-in",
        _ => "",
    }
}

/// Whether the meter should move to another input: the default input isn't
/// the one it follows (another is the default now, or one is known only
/// now, or none is any more).
fn input_changed(default: Option<&str>, following: Option<&str>) -> bool {
    default != following
}

/// Whether a device is in use: an output playing, an input that an app
/// records from. An input runs while this pane's own level meter reads it,
/// so its state alone would always say "In use".
fn in_use(state: &AudioState, d: &AudioDevice, io: Io) -> bool {
    match io {
        Io::Output => d.state == DeviceState::Running,
        Io::Input => state.recording.iter().any(|s| s.device == d.index && !s.corked),
    }
}

fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

impl Sound {
    fn act(&mut self, cx: &mut PaneCx<Msg>, what: &str, task: Task<()>) {
        let what = what.to_owned();
        cx.wait(task, move |r| Msg::Done(what, r));
    }

    fn act_bool(&mut self, cx: &mut PaneCx<Msg>, what: &str, task: Task<bool>) {
        let what = what.to_owned();
        cx.wait(task, move |r| Msg::Done(what, r.map(drop)));
    }

    /// Follow the default input's level (live: a meter on it).
    fn follow_input(&mut self, cx: &mut PaneCx<Msg>) {
        if let Snd::Live(a) = &self.snd {
            let name = a.snapshot().default_input().map(|d| d.name.clone());
            match name {
                Some(n) if self.meter.as_ref().is_none_or(|(m, _)| *m != n) => {
                    self.meter = Some((n.clone(), Meter::start(&n, aizome_sys::Wake::none())));
                }
                None => self.meter = None,
                _ => {}
            }
        }
        if self.meter_timer.is_none() {
            self.meter_timer = Some(cx.every(METER_EVERY, Msg::Meter));
        }
    }

    fn alert(&self, cx: &mut PaneCx<Msg>) {
        let s = &cx.settings().sound;
        let name = s.get("alert_sound").and_then(|v| v.as_str()).unwrap_or("bell").to_owned();
        let volume = s.get("alert_volume").and_then(|v| v.as_float()).unwrap_or(0.7);
        let Some((_, file)) = self.sounds.iter().find(|(n, _)| *n == name) else { return };
        if cx.sample() {
            cx.say(format!("Plays “{}”", sound_label(&name)));
            return;
        }
        let spawned = std::process::Command::new("pw-play")
            .arg("--volume")
            .arg(format!("{volume:.2}"))
            .arg(file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            // Reap it when it ends.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

impl Pane for Sound {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "sound"
    }

    fn title(&self) -> &'static str {
        "Sound"
    }

    fn icon(&self) -> Icon {
        Icon::Volume2
    }

    fn tile(&self) -> Tile {
        Tile::Red
    }

    fn group(&self) -> Group {
        Group::Alerts
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["audio", "volume", "speakers", "headphones", "microphone", "mic", "output", "input", "pipewire"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("output-device", "Output device").keywords(&[
                "speakers",
                "headphones",
                "hdmi",
                "default",
            ]),
            SettingEntry::new("output-volume", "Output volume").keywords(&["loud", "mute"]),
            SettingEntry::new("balance", "Balance").keywords(&["left", "right", "stereo"]),
            SettingEntry::new("port", "Port").keywords(&["jack", "headphones", "line out"]),
            SettingEntry::new("input-device", "Input device").keywords(&["microphone", "mic"]),
            SettingEntry::new("input-volume", "Input volume").keywords(&["microphone", "gain", "mute"]),
            SettingEntry::new("input-level", "Input level").keywords(&["meter", "test microphone"]),
            SettingEntry::new("apps", "Application volume").keywords(&["per-app", "mixer", "streams"]),
            SettingEntry::new("alert-sound", "Alert sound").keywords(&["beep", "bell"]),
            SettingEntry::new("alert-volume", "Alert volume"),
            SettingEntry::new("feedback", "Play feedback when volume is changed"),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        if matches!(self.snd, Snd::None) {
            let wake = cx.wake();
            self.snd = if cx.sample() {
                Snd::Sample(Cell::new(
                    crate::panes::hw::live_snapshot(
                        || Audio::new(aizome_sys::Wake::none()),
                        |a| a.available() && !a.snapshot().sinks.is_empty(),
                        |a| a.snapshot(),
                    )
                    .unwrap_or_else(sample),
                    wake,
                ))
            } else {
                Snd::Live(Audio::new(wake))
            };
        }
        if self.sounds.is_empty() {
            // The sample's are the same everywhere (for golden images).
            self.sounds = if cx.sample() { Vec::new() } else { alert_sounds() };
            if self.sounds.is_empty() {
                // The names the shell plays even without files of its own.
                self.sounds = ["bell", "message", "complete", "dialog-information", "dialog-warning"]
                    .iter()
                    .map(|n| ((*n).to_owned(), String::new()))
                    .collect();
            }
        }
        self.follow_input(cx);
    }

    fn hidden(&mut self, cx: &mut PaneCx<Msg>) {
        self.meter = None;
        if let Some(t) = self.meter_timer.take() {
            cx.cancel(t);
        }
        self.level = 0.0;
        self.peak = 0.0;
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Default(io, name) => {
                let t = match &self.snd {
                    Snd::Live(a) if io == Io::Output => a.set_default_sink(&name),
                    Snd::Live(a) => a.set_default_source(&name),
                    _ => self.snd.sample(move |s| match io {
                        Io::Output => s.default_sink = Some(name),
                        Io::Input => s.default_source = Some(name),
                    }),
                };
                self.act(cx, "Couldn't change the device", t);
                if io == Io::Input {
                    self.meter = None;
                    self.follow_input(cx);
                }
            }
            Msg::Volume(io, index, v) => {
                let t = match &self.snd {
                    Snd::Live(a) if io == Io::Output => a.set_sink_volume(index, v),
                    Snd::Live(a) => a.set_source_volume(index, v),
                    _ => self.snd.sample(move |s| {
                        let list = if io == Io::Output { &mut s.sinks } else { &mut s.sources };
                        if let Some(d) = list.iter_mut().find(|d| d.index == index) {
                            d.volume = v;
                        }
                    }),
                };
                self.act(cx, "Couldn't change the volume", t);
            }
            Msg::Mute(io, index, on) => match &self.snd {
                Snd::Live(a) => {
                    let t = if io == Io::Output { a.set_sink_mute(index, on) } else { a.set_source_mute(index, on) };
                    self.act_bool(cx, "Couldn't mute it", t);
                }
                _ => {
                    let t = self.snd.sample(move |s| {
                        let list = if io == Io::Output { &mut s.sinks } else { &mut s.sources };
                        if let Some(d) = list.iter_mut().find(|d| d.index == index) {
                            d.muted = on;
                        }
                    });
                    self.act(cx, "Couldn't mute it", t);
                }
            },
            Msg::Balance(io, index, b) => {
                // Near the middle is the middle.
                let b = if b.abs() < 0.04 { 0.0 } else { b };
                let t = match &self.snd {
                    Snd::Live(a) if io == Io::Output => a.set_sink_balance(index, b),
                    Snd::Live(a) => a.set_source_balance(index, b),
                    _ => self.snd.sample(move |s| {
                        let list = if io == Io::Output { &mut s.sinks } else { &mut s.sources };
                        if let Some(d) = list.iter_mut().find(|d| d.index == index) {
                            d.balance = b;
                        }
                    }),
                };
                self.act(cx, "Couldn't change the balance", t);
            }
            Msg::Port(io, index, port) => {
                let t = match &self.snd {
                    Snd::Live(a) if io == Io::Output => a.set_sink_port(index, &port),
                    Snd::Live(a) => a.set_source_port(index, &port),
                    _ => self.snd.sample(move |s| {
                        let list = if io == Io::Output { &mut s.sinks } else { &mut s.sources };
                        if let Some(d) = list.iter_mut().find(|d| d.index == index) {
                            d.active_port = Some(port);
                        }
                    }),
                };
                self.act(cx, "Couldn't switch the port", t);
            }
            Msg::AppVolume(rec, index, v) => {
                let t = match &self.snd {
                    Snd::Live(a) if rec => a.set_recording_volume(index, v),
                    Snd::Live(a) => a.set_playback_volume(index, v),
                    _ => self.snd.sample(move |s| {
                        let list = if rec { &mut s.recording } else { &mut s.playback };
                        if let Some(x) = list.iter_mut().find(|x| x.index == index) {
                            x.volume = v;
                        }
                    }),
                };
                self.act(cx, "Couldn't change its volume", t);
            }
            Msg::AppMute(rec, index, on) => match &self.snd {
                Snd::Live(a) => {
                    let t = if rec { a.set_recording_mute(index, on) } else { a.set_playback_mute(index, on) };
                    self.act_bool(cx, "Couldn't mute it", t);
                }
                _ => {
                    let t = self.snd.sample(move |s| {
                        let list = if rec { &mut s.recording } else { &mut s.playback };
                        if let Some(x) = list.iter_mut().find(|x| x.index == index) {
                            x.muted = on;
                        }
                    });
                    self.act(cx, "Couldn't mute it", t);
                }
            },
            Msg::AppDevice(rec, index, device) => {
                let t = match &self.snd {
                    Snd::Live(a) if rec => a.move_recording(index, device),
                    Snd::Live(a) => a.move_playback(index, device),
                    _ => self.snd.sample(move |s| {
                        let list = if rec { &mut s.recording } else { &mut s.playback };
                        if let Some(x) = list.iter_mut().find(|x| x.index == index) {
                            x.device = device;
                        }
                    }),
                };
                self.act(cx, "Couldn't move it", t);
            }
            Msg::AlertSound(i) => {
                if let Some((name, _)) = self.sounds.get(i).cloned() {
                    cx.change(|s| {
                        s.sound.insert("alert_sound".into(), toml::Value::String(name));
                    });
                    self.alert(cx);
                }
            }
            Msg::AlertVolume(v) => {
                let v = ((v as f64) * 100.0).round() / 100.0;
                cx.change(|s| {
                    s.sound.insert("alert_volume".into(), toml::Value::Float(v));
                });
            }
            Msg::AlertVolumeSet => self.alert(cx),
            Msg::Feedback(on) => cx.change(|s| {
                s.sound.insert("volume_feedback".into(), toml::Value::Boolean(on));
            }),
            Msg::Meter => {
                // The default input changed elsewhere (a headset plugged in),
                // or is known only now (the pane showed before the sound
                // server answered): follow it.
                if let Snd::Live(a) = &self.snd
                    && input_changed(
                        a.snapshot().default_input().map(|d| d.name.as_str()),
                        self.meter.as_ref().map(|(n, _)| n.as_str()),
                    )
                {
                    self.follow_input(cx);
                }
                let raw = match (&self.meter, &self.snd) {
                    (Some((_, m)), _) => loudness(m.level()),
                    (None, Snd::Sample(c)) => {
                        // The sample talks: a level that rises and falls.
                        self.ticks = self.ticks.wrapping_add(1);
                        let muted = c.get().default_input().is_none_or(|d| d.muted);
                        let t = self.ticks as f32 * 0.37;
                        if muted { 0.0 } else { (0.52 + 0.22 * t.sin() + 0.08 * (t * 2.7).sin()).clamp(0.0, 1.0) }
                    }
                    _ => 0.0,
                };
                self.level = raw;
                self.peak = (self.peak * 0.94).max(raw);
            }
            Msg::Done(what, result) => {
                if let Err(e) = result {
                    cx.failed(&what, e);
                }
            }
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let state = self.snd.snapshot();
        if matches!(self.snd, Snd::Live(_)) && !state.available {
            return form::page(vec![
                form::heading("No sound server", "Sound goes through PipeWire, which isn't answering."),
                form::note(
                    "On NixOS it's turned on with services.pipewire.enable and services.pipewire.pulse.enable = true.",
                ),
            ]);
        }
        let mut sections: Vec<Element<Msg>> = Vec::new();
        sections.push(devices(vcx, &state, Io::Output, self.levels()));
        sections.push(devices(vcx, &state, Io::Input, self.levels()));
        if let Some(apps) = applications(vcx, &state) {
            sections.push(apps);
        }
        sections.push(alerts(self, vcx));
        form::page(sections)
    }
}

impl Sound {
    pub(crate) fn levels(&self) -> (f32, f32) {
        (self.level, self.peak)
    }
}

/// Words that say how many channels, at the end of a description.
const LAYOUTS: [&str; 8] = [
    "Analog Stereo",
    "Digital Stereo",
    "Surround 4.0",
    "Surround 5.1",
    "Surround 7.1",
    "Multichannel",
    "Stereo",
    "Mono",
];

/// A device's name as people say it: a display's own name where HDMI
/// gives one ("… Output [PG27UCDM] (Stereo)" is "PG27UCDM"), the sound
/// card's long name taken off where other devices share it ("Core Ultra
/// Processors (Series 3) HD Audio Speaker" is "Speaker"), and the channel
/// layout dropped.
pub fn device_name(d: &AudioDevice, all: &[&AudioDevice]) -> String {
    let desc = d.description.trim();
    if let (Some(a), Some(b)) = (desc.find('['), desc.rfind(']'))
        && a < b
        && !desc[a + 1..b].trim().is_empty()
    {
        return desc[a + 1..b].trim().to_owned();
    }
    let mut s = desc.to_owned();
    for l in LAYOUTS {
        for form in [format!(" ({l})"), format!(" {l}")] {
            if s.len() > form.len() && s.ends_with(&form) {
                s.truncate(s.len() - form.len());
            }
        }
    }
    // The words every device of its card begins with (its card: those
    // sharing at least two first words with it).
    let words: Vec<&str> = s.split(' ').collect();
    let common = |o: &AudioDevice| words.iter().zip(o.description.split(' ')).take_while(|(a, b)| **a == *b).count();
    let shared = all
        .iter()
        .filter(|o| o.index != d.index || o.name != d.name)
        .map(|o| common(o))
        .filter(|n| *n >= 2)
        .min()
        .unwrap_or(0);
    if shared >= 2 && shared < words.len() {
        s = words[shared..].join(" ");
    }
    for end in [" Output", " Input"] {
        if s.len() > end.len() && s.ends_with(end) {
            s.truncate(s.len() - end.len());
        }
    }
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => desc.to_owned(),
    }
}

/// A device to pick; `busy`: something plays to it, or records from it.
fn device_row(d: &AudioDevice, io: Io, all: &[&AudioDevice], busy: bool) -> Row<Msg> {
    let name = device_name(d, all);
    let detail = {
        let mut parts: Vec<String> = Vec::new();
        let bus = bus_words(d);
        if !bus.is_empty() {
            parts.push(bus.to_owned());
        }
        if let Some(p) = d.active_port.as_ref().and_then(|p| d.ports.iter().find(|x| x.name == *p))
            && p.description != name
            && !name.contains(p.description.trim_end_matches(" Output").trim_end_matches(" Input"))
        {
            parts.push(p.description.trim_end_matches(" Output").trim_end_matches(" Input").to_owned());
        }
        if busy {
            parts.push(if io == Io::Output { "Playing".into() } else { "In use".into() });
        }
        parts.join(" · ")
    };
    let line = row![
        tile(device_icon(d, io), if d.is_default { Tile::Ai } else { Tile::Slate }, 26.0),
        column![
            text(name.clone()).weight(if d.is_default { 600.0 } else { 400.0 }),
            text(detail).size(11.0).role(Role::Text3)
        ]
        .spacing(2.0)
        .width(Fill),
        if d.is_default { icon(Icon::Check).size(16.0).role(Role::Accent).into() } else { space::<Msg>(16.0) },
    ]
    .spacing(10.0)
    .align(Align::Center)
    .width(Fill);
    let msg = (!d.is_default).then(|| Msg::Default(io, d.name.clone()));
    // Selected: what assistive technology hears for the check.
    form::whole(
        press(line)
            .style(PressStyle::Plain)
            .selected(d.is_default)
            .padding([7.0, 12.0])
            .on_press_maybe(msg)
            .named(name)
            .width(Fill),
    )
}

/// A volume slider with the mute button before it and the level after.
fn volume_control(io: Io, d: &AudioDevice) -> Element<Msg> {
    let index = d.index;
    let muted = d.muted;
    let glyph = match (io, muted) {
        (Io::Output, true) => Icon::VolumeX,
        (Io::Output, false) if d.volume < 0.01 => Icon::VolumeOff,
        (Io::Output, false) if d.volume < 0.5 => Icon::Volume1,
        (Io::Output, false) => Icon::Volume2,
        (Io::Input, true) => Icon::MicOff,
        (Io::Input, false) => Icon::Mic,
    };
    let max = if d.volume > 1.0 { MAX_VOLUME as f64 } else { 1.0 };
    row![
        icon_button(glyph)
            .plain()
            .compact()
            .on_press(Msg::Mute(io, index, !muted))
            .tooltip(if muted { "Unmute" } else { "Mute" })
            .label(if muted { "Unmute" } else { "Mute" }),
        slider(d.volume as f64, 0.0..=max)
            .step(0.01)
            .on_change(move |v| Msg::Volume(io, index, v as f32))
            .enabled(!muted)
            .width(220.0),
        text(if muted { "Muted".to_owned() } else { percent(d.volume) })
            .mono()
            .size(11.0)
            .role(Role::Text3)
            .width(44.0),
    ]
    .spacing(8.0)
    .align(Align::Center)
    .into()
}

fn devices(vcx: &ViewCx, state: &AudioState, io: Io, (level, peak): (f32, f32)) -> Element<Msg> {
    let (list, title, entry, none) = match io {
        Io::Output => (&state.sinks, "Output", "output-device", "No output devices"),
        Io::Input => (&state.sources, "Input", "input-device", "No input devices"),
    };
    let mut rows: Vec<Row<Msg>> = Vec::new();
    if list.is_empty() {
        rows.push(form::row(none, space(0.0)).entry(entry));
        return vcx.section(title, rows);
    }
    let all: Vec<&AudioDevice> = state.sinks.iter().chain(&state.sources).collect();
    for (i, d) in list.iter().enumerate() {
        let r = device_row(d, io, &all, in_use(state, d, io));
        rows.push(if i == 0 { r.entry(entry) } else { r });
    }
    let default = match io {
        Io::Output => state.default_output(),
        Io::Input => state.default_input(),
    };
    let mut controls: Vec<Row<Msg>> = Vec::new();
    if let Some(d) = default {
        let (label, entry) =
            if io == Io::Output { ("Output volume", "output-volume") } else { ("Input volume", "input-volume") };
        controls.push(form::row(label, volume_control(io, d)).entry(entry));
        if d.channel_volumes.len() >= 2 {
            let index = d.index;
            controls.push(
                form::row(
                    "Balance",
                    row![
                        text("L").mono().size(11.0).role(Role::Text3),
                        slider(d.balance as f64, -1.0..=1.0)
                            .step(0.02)
                            .on_change(move |v| Msg::Balance(io, index, v as f32))
                            .width(200.0),
                        text("R").mono().size(11.0).role(Role::Text3),
                    ]
                    .spacing(8.0)
                    .align(Align::Center),
                )
                .entry("balance"),
            );
        }
        let ports: Vec<&aizome_sys::audio::Port> = d
            .ports
            .iter()
            .filter(|p| p.available != Some(false) || d.active_port.as_deref() == Some(p.name.as_str()))
            .collect();
        if ports.len() > 1 {
            let labels: Vec<&str> = ports.iter().map(|p| p.description.as_str()).collect();
            let at = ports.iter().position(|p| d.active_port.as_deref() == Some(p.name.as_str())).unwrap_or(0);
            let names: Vec<String> = ports.iter().map(|p| p.name.clone()).collect();
            let index = d.index;
            controls.push(
                form::row(
                    "Port",
                    form::choice(&labels, at, move |i| Msg::Port(io, index, names[i.min(names.len() - 1)].clone())),
                )
                .entry("port"),
            );
        }
        if io == Io::Input {
            controls.push(
                form::row("Input level", level_meter::<Msg>(level, peak, SEGMENTS).width(268.0))
                    .help("Speak to see the level move")
                    .entry("input-level"),
            );
        }
    }
    column![vcx.section(title, rows), vcx.section("", controls)].spacing(8.0).width(Fill).into()
}

fn app_row(state: &AudioState, s: &AudioStream, rec: bool) -> Row<Msg> {
    let index = s.index;
    let glyph = if rec {
        Icon::Mic
    } else {
        match s.role.as_str() {
            "music" => Icon::EighthNotes,
            "video" => Icon::Film,
            "event" => Icon::Bell,
            "phone" => Icon::Phone,
            _ => Icon::AppWindow,
        }
    };
    let devices = if rec { &state.sources } else { &state.sinks };
    let mut line = row![
        tile(glyph, if s.corked { Tile::Slate } else { Tile::Violet }, 26.0),
        column![
            text(s.application.clone()).weight(500.0),
            text(match (s.corked, s.media_name.is_empty() || s.media_name == s.application) {
                (true, true) => "Paused".to_owned(),
                (true, false) => format!("Paused · {}", s.media_name),
                (false, true) => String::new(),
                (false, false) => s.media_name.clone(),
            })
            .size(11.0)
            .role(Role::Text3),
        ]
        .spacing(2.0)
        .width(Fill),
    ]
    .spacing(10.0)
    .align(Align::Center)
    .width(Fill);
    if devices.len() > 1 {
        let all: Vec<&AudioDevice> = state.sinks.iter().chain(&state.sources).collect();
        let labels: Vec<String> = devices.iter().map(|d| device_name(d, &all)).collect();
        let at = devices.iter().position(|d| d.index == s.device).unwrap_or(0);
        let ids: Vec<u32> = devices.iter().map(|d| d.index).collect();
        line = line.push(
            dropdown(labels, at)
                .compact()
                .on_select(move |i| Msg::AppDevice(rec, index, ids[i.min(ids.len() - 1)]))
                .width(150.0),
        );
    }
    let muted = s.muted;
    line = line.push(
        icon_button(if muted { Icon::VolumeX } else { Icon::Volume2 })
            .plain()
            .compact()
            .on_press(Msg::AppMute(rec, index, !muted))
            .tooltip(if muted { "Unmute" } else { "Mute" })
            .label(if muted { "Unmute" } else { "Mute" }),
    );
    line = line.push(
        slider(s.volume as f64, 0.0..=1.0)
            .step(0.01)
            .on_change(move |v| Msg::AppVolume(rec, index, v as f32))
            .enabled(s.volume_writable && !muted)
            .width(120.0),
    );
    form::whole(container(line).padding([7.0, 12.0]).width(Fill))
}

fn applications(vcx: &ViewCx, state: &AudioState) -> Option<Element<Msg>> {
    if state.playback.is_empty() && state.recording.is_empty() {
        return None;
    }
    let mut rows: Vec<Row<Msg>> = state.playback.iter().map(|s| app_row(state, s, false)).collect();
    rows.extend(state.recording.iter().map(|s| app_row(state, s, true)));
    let first = rows.remove(0);
    rows.insert(0, first.entry("apps"));
    Some(vcx.section_with(
        "Applications",
        rows,
        Some("Each application's own volume and device, while it plays or records.".into()),
    ))
}

fn alerts(p: &Sound, vcx: &ViewCx) -> Element<Msg> {
    let s = &vcx.settings().sound;
    let name = s.get("alert_sound").and_then(|v| v.as_str()).unwrap_or("bell");
    let volume = s.get("alert_volume").and_then(|v| v.as_float()).unwrap_or(0.7);
    let feedback = s.get("volume_feedback").and_then(|v| v.as_bool()).unwrap_or(false);
    let labels: Vec<String> = p.sounds.iter().map(|(n, _)| sound_label(n)).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let at = p.sounds.iter().position(|(n, _)| n == name).unwrap_or(0);
    let mut rows = Vec::new();
    if !labels.is_empty() {
        rows.push(
            form::row("Alert sound", form::choice(&labels, at, Msg::AlertSound))
                .help("Plays when you choose it")
                .entry("alert-sound"),
        );
    }
    rows.push(
        form::row(
            "Alert volume",
            row![
                icon(Icon::Volume1).size(14.0).role(Role::Text3),
                slider(volume, 0.0..=1.0)
                    .step(0.05)
                    .on_change(|v| Msg::AlertVolume(v as f32))
                    .on_release(Msg::AlertVolumeSet)
                    .width(200.0),
                icon(Icon::Volume2).size(14.0).role(Role::Text3),
            ]
            .spacing(8.0)
            .align(Align::Center),
        )
        .entry("alert-volume"),
    );
    rows.push(
        form::row("Play feedback when volume is changed", toggle(feedback).on_toggle(Msg::Feedback)).entry("feedback"),
    );
    vcx.section("Alerts", rows)
}

/// The sample: built-in speakers, headphones over Bluetooth and a display
/// over HDMI; the built-in microphone and the headphones'; music and a
/// browser playing, and a call recording.
pub fn sample() -> AudioState {
    let port = |n: &str, d: &str, a: Option<bool>| aizome_sys::audio::Port {
        name: n.into(),
        description: d.into(),
        priority: 100,
        available: a,
    };
    let dev = |index: u32, name: &str, description: &str, volume: f32, form: &str, bus: &str| AudioDevice {
        index,
        name: name.into(),
        description: description.into(),
        volume,
        channel_volumes: vec![volume, volume],
        balance: 0.0,
        muted: false,
        is_default: false,
        icon_name: String::new(),
        form_factor: form.into(),
        bus: bus.into(),
        ports: Vec::new(),
        active_port: None,
        state: DeviceState::Idle,
        is_monitor: false,
    };
    let mut speakers =
        dev(41, "alsa_output.pci-0000_c1_00.6.analog-stereo", "Built-in Speakers", 0.62, "internal", "pci");
    speakers.ports = vec![
        port("analog-output-speaker", "Speakers", Some(true)),
        port("analog-output-headphones", "Headphones jack", Some(false)),
    ];
    speakers.active_port = Some("analog-output-speaker".into());
    speakers.is_default = true;
    speakers.state = DeviceState::Running;
    let headphones = dev(57, "bluez_output.F4_4E_FD_2A_01_25.1", "WH-1000XM5", 0.4, "headphone", "bluetooth");
    let hdmi = dev(44, "alsa_output.pci-0000_c1_00.1.hdmi-stereo", "PG27UCDM (HDMI)", 1.0, "hdmi", "pci");
    let mut mic = dev(42, "alsa_input.pci-0000_c1_00.6.analog-stereo", "Built-in Microphone", 0.8, "internal", "pci");
    mic.is_default = true;
    mic.state = DeviceState::Running;
    let mut headset = dev(58, "bluez_input.F4_4E_FD_2A_01_25.0", "WH-1000XM5", 0.7, "headset", "bluetooth");
    headset.channel_volumes = vec![0.7];
    let stream = |index: u32, app: &str, media: &str, role: &str, volume: f32, device: u32, corked: bool| AudioStream {
        index,
        application: app.into(),
        app_id: app.to_lowercase(),
        icon_name: String::new(),
        binary: app.to_lowercase(),
        pid: None,
        media_name: media.into(),
        role: role.into(),
        volume,
        muted: false,
        device,
        corked,
        volume_writable: true,
    };
    AudioState {
        available: true,
        server: "PulseAudio (on PipeWire 1.6.9) 15.0.0".into(),
        default_sink: Some(speakers.name.clone()),
        default_source: Some(mic.name.clone()),
        sinks: vec![speakers, headphones, hdmi],
        sources: vec![mic, headset],
        playback: vec![
            stream(101, "Spotify", "Kohaku — Lo-fi for Tea", "music", 0.8, 41, false),
            stream(102, "Firefox", "Tōkyō Jazz Night — YouTube", "video", 0.55, 41, true),
        ],
        recording: vec![stream(201, "Signal", "Call with Aiko", "phone", 1.0, 42, false)],
        cards: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(index: u32, description: &str) -> AudioDevice {
        AudioDevice { index, name: format!("dev{index}"), description: description.into(), ..AudioDevice::default() }
    }

    #[test]
    fn devices_go_by_the_names_people_say() {
        let all = [
            dev(1, "Core Ultra Processors (Series 3) HD Audio HDMI / DisplayPort 1 Output [PG27UCDM] (Stereo)"),
            dev(2, "Core Ultra Processors (Series 3) HD Audio HDMI / DisplayPort 2 Output (Stereo)"),
            dev(3, "Core Ultra Processors (Series 3) HD Audio Speaker"),
            dev(4, "Core Ultra Processors (Series 3) HD Audio Microphones"),
            dev(5, "WH-1000XM5"),
            dev(6, "Built-in Audio Analog Stereo"),
        ];
        let refs: Vec<&AudioDevice> = all.iter().collect();
        let names: Vec<String> = all.iter().map(|d| device_name(d, &refs)).collect();
        assert_eq!(
            names,
            ["PG27UCDM", "HDMI / DisplayPort 2", "Speaker", "Microphones", "WH-1000XM5", "Built-in Audio"]
        );
        assert_eq!(sound_label("dialog-warning"), "Dialog warning");
    }

    #[test]
    fn the_meter_starts_once_the_default_input_is_known() {
        // The pane shows before pipewire-pulse has answered: no input yet,
        // so no meter. The input that then appears must start one.
        assert!(!input_changed(None, None));
        assert!(input_changed(Some("alsa_input.mic"), None));
        assert!(!input_changed(Some("alsa_input.mic"), Some("alsa_input.mic")));
        assert!(input_changed(Some("bluez_input.headset"), Some("alsa_input.mic")));
        assert!(input_changed(None, Some("alsa_input.mic")));
    }

    #[test]
    fn an_input_is_in_use_when_an_app_records_not_when_the_meter_reads() {
        let mut state = sample();
        let mic = state.sources[0].clone();
        assert_eq!(mic.state, DeviceState::Running);
        assert!(in_use(&state, &mic, Io::Input), "Signal records from it");
        // Only the pane's own level meter reads it: the source still runs,
        // but nobody is recording.
        state.recording.clear();
        assert!(!in_use(&state, &mic, Io::Input));
        let speakers = state.sinks[0].clone();
        assert!(in_use(&state, &speakers, Io::Output));
    }
}
