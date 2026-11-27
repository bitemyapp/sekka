//! Settings › Display: each display's size of things (its scale), its
//! resolution and its refresh rate, through the compositor's `outputs` and
//! `output-config`, as Settings' Displays has them on the desktop.
//!
//! The size of things applies at once. A new resolution or refresh rate
//! asks to be kept and goes back by itself after [`KEEP_FOR`] unanswered:
//! a television that can't show it is dark, and can't be answered. Going
//! back comes first in the question, so presses on a dark screen go back
//! too. What's kept is written to Aizome's settings (`[[outputs]]`, by the
//! display's make, model and serial, as Settings writes it), so the
//! compositor starts with it next time; the TV account keeps its own
//! choice until a grown-up's account sets the displays up anew
//! (`aizome-tv-account follow`).

use std::time::{Duration, Instant};

use aizome_config::OutputConfig;
use aizome_ipc::{Mode, OutputInfo, Request, Response};
use aizome_kit::prelude::*;

use super::{Chanoma, MenuKind, Modal, Msg};

/// A new resolution or refresh rate unanswered this long goes back.
pub const KEEP_FOR: Duration = Duration::from_secs(15);
/// The sizes of things offered, as scales.
const SCALES: [f64; 6] = [1.0, 1.5, 2.0, 2.5, 3.0, 4.0];
/// No scale is offered that leaves fewer lines than this to lay out in.
const FEWEST_LINES: f64 = 540.0;

/// What the display settings know.
#[derive(Debug, Default)]
pub struct Displays {
    /// The compositor's displays, as it said last; `None` until it has.
    pub listed: Option<Result<Vec<OutputInfo>, String>>,
    /// A change asked for and not answered yet.
    pub busy: bool,
    /// A new resolution or refresh rate, to be kept or gone back from.
    pub keep: Option<Keep>,
    /// Without a compositor (`Config::compositor` off, in tests): what it
    /// would have been asked, taken as it would take it.
    pub asked: Vec<Request>,
}

/// A display as it was before a change that asks to be kept.
#[derive(Clone, Debug)]
pub struct Keep {
    pub name: String,
    pub mode: Mode,
    pub scale: f64,
    /// Since when it's been asked about (once the change is made).
    pub since: Instant,
}

/// Which of a display's settings a row steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Size,
    Resolution,
    Refresh,
}

/// What a change sent to the compositor is, for what follows its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The size of things: kept at once.
    Size,
    /// A resolution or a refresh rate: asked to be kept.
    Asks,
    /// Back as it was.
    Back,
}

/// A display's name to show: its model, or its make, or its connector.
pub fn title(o: &OutputInfo) -> String {
    let known = |s: &str| !s.is_empty() && !s.eq_ignore_ascii_case("unknown") && !s.starts_with("0x");
    [&o.model, &o.make].into_iter().find(|s| known(s)).unwrap_or(&o.name).clone()
}

/// The display's resolutions, smallest first.
pub fn sizes(o: &OutputInfo) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    for m in &o.modes {
        if !out.contains(&(m.width, m.height)) {
            out.push((m.width, m.height));
        }
    }
    out.sort_by_key(|&(w, h)| (i64::from(w) * i64::from(h), w));
    out
}

/// Its refresh rates at a resolution, slowest first.
pub fn rates(o: &OutputInfo, size: (i32, i32)) -> Vec<i32> {
    let mut out: Vec<i32> = o.modes.iter().filter(|m| (m.width, m.height) == size).map(|m| m.refresh).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The sizes of things offered at a mode: no fewer than [`FEWEST_LINES`]
/// lines left.
pub fn scales(m: &Mode) -> Vec<f64> {
    let mut out: Vec<f64> = SCALES.into_iter().filter(|s| f64::from(m.height) / s >= FEWEST_LINES).collect();
    if out.is_empty() {
        out.push(1.0);
    }
    out
}

/// A refresh rate as said: "60 Hz", "59.94 Hz".
pub fn hz(refresh: i32) -> String {
    let hz = f64::from(refresh) / 1000.0;
    if (hz - hz.round()).abs() < 0.005 { format!("{} Hz", hz.round()) } else { format!("{hz:.2} Hz") }
}

/// A mode as `output-config` and the settings take it (as Settings writes
/// it): `3840x2160@60`, `3840x2160@59.940`.
pub fn mode_setting(m: &Mode) -> String {
    let hz = f64::from(m.refresh) / 1000.0;
    if m.refresh <= 0 {
        format!("{}x{}", m.width, m.height)
    } else if (hz - hz.round()).abs() < 0.0005 {
        format!("{}x{}@{}", m.width, m.height, hz.round())
    } else {
        format!("{}x{}@{hz:.3}", m.width, m.height)
    }
}

/// The resolution as said, and whether it's the display's own.
pub fn size_words(o: &OutputInfo, m: &Mode) -> String {
    let own = o.modes.iter().any(|p| p.preferred && (p.width, p.height) == (m.width, m.height));
    let words = format!("{} × {}", m.width, m.height);
    if own { format!("{words} · its own") } else { words }
}

/// The size of things as said: the scale, and the room it leaves.
pub fn scale_words(scale: f64, m: &Mode) -> String {
    let room = |px: i32| (f64::from(px) / scale).round();
    format!("{}× · as {} × {}", scale, room(m.width), room(m.height))
}

/// The one after `now` in `list` (before it, going back), at its ends none.
fn step<T: Copy + PartialEq>(list: &[T], now: T, forward: bool) -> Option<T> {
    let i = list.iter().position(|x| *x == now)?;
    let j = if forward { i + 1 } else { i.checked_sub(1)? };
    list.get(j).copied()
}

/// The scale offered nearest `scale`.
fn nearest(list: &[f64], scale: f64) -> f64 {
    list.iter().copied().min_by(|a, b| (a - scale).abs().total_cmp(&(b - scale).abs())).unwrap_or(1.0)
}

/// What changing `what` a step takes the display to: its mode (if that
/// changes) and its scale (if that does).
pub fn stepped(o: &OutputInfo, what: Setting, forward: bool) -> Option<(Option<Mode>, Option<f64>)> {
    let m = o.current_mode.as_ref()?;
    match what {
        // The next offered beyond the scale now, which may be one set
        // elsewhere.
        Setting::Size => {
            let list = scales(m);
            let next = if forward {
                list.iter().copied().find(|s| *s > o.scale + 1e-3)
            } else {
                list.iter().rev().copied().find(|s| *s < o.scale - 1e-3)
            };
            Some((None, Some(next?)))
        }
        Setting::Resolution => {
            let size = step(&sizes(o), (m.width, m.height), forward)?;
            // Its refresh rate nearest the one now, and things as big
            // as they were.
            let mode = o
                .modes
                .iter()
                .filter(|x| (x.width, x.height) == size)
                .min_by_key(|x| ((x.refresh - m.refresh).abs(), !x.preferred))?
                .clone();
            let scale = nearest(&scales(&mode), o.scale * f64::from(mode.height) / f64::from(m.height));
            Some((Some(mode), Some(scale)))
        }
        Setting::Refresh => {
            let refresh = step(&rates(o, (m.width, m.height)), m.refresh, forward)?;
            let mode = o.modes.iter().find(|x| (x.width, x.height, x.refresh) == (m.width, m.height, refresh))?;
            Some((Some(mode.clone()), None))
        }
    }
}

/// The request for a change.
fn request(name: &str, mode: Option<&Mode>, scale: Option<f64>) -> Request {
    Request::OutputConfig {
        name: name.to_owned(),
        mode: mode.map(mode_setting),
        scale,
        position: None,
        transform: None,
        enabled: None,
    }
}

/// The compositor's displays.
fn outputs(c: &mut aizome_ipc::Client) -> Result<Vec<OutputInfo>, String> {
    match c.call(&Request::Outputs).map_err(|e| e.to_string())? {
        Response::Outputs { outputs } => Ok(outputs),
        other => Err(format!("the compositor answered {other:?}")),
    }
}

fn connect() -> Result<aizome_ipc::Client, String> {
    let c = aizome_ipc::Client::connect().map_err(|e| format!("the compositor: {e}"))?;
    let _ = c.stream().set_read_timeout(Some(Duration::from_secs(10)));
    Ok(c)
}

/// Take a request as the compositor would, for the displays without one.
fn take(list: &mut [OutputInfo], req: &Request) -> Result<(), String> {
    let Request::OutputConfig { name, mode, scale, .. } = req else { return Err("not a display change".into()) };
    let o = list.iter_mut().find(|o| o.name == *name).ok_or_else(|| format!("no display {name}"))?;
    if let Some(m) = mode {
        let (w, h, r) = aizome_config::parse_mode(m).ok_or_else(|| format!("no mode {m}"))?;
        let found =
            o.modes.iter().find(|x| (x.width, x.height) == (w, h) && r.is_none_or(|r| (x.refresh - r).abs() < 10));
        o.current_mode = Some(found.ok_or_else(|| format!("{name} has no mode {m}"))?.clone());
    }
    if let Some(s) = scale {
        o.scale = *s;
    }
    Ok(())
}

/// The display's entry in the settings' `[[outputs]]`, made if there's
/// none, as Settings finds and makes it: by make, model and serial, or
/// by its connector for a display that doesn't say what it is.
fn entry<'a>(entries: &'a mut Vec<OutputConfig>, o: &OutputInfo) -> &'a mut OutputConfig {
    let known = |s: &str| !s.is_empty() && !s.eq_ignore_ascii_case("unknown");
    let identified = known(&o.make) && known(&o.model);
    let owns = |e: &OutputConfig| {
        if identified {
            e.name.is_none()
                && e.make.as_deref() == Some(o.make.as_str())
                && e.model.as_deref() == Some(o.model.as_str())
                && e.serial.as_deref().unwrap_or("") == o.serial
        } else {
            e.name.as_deref() == Some(o.name.as_str()) && e.make.is_none() && e.model.is_none()
        }
    };
    let i = match entries.iter().position(owns) {
        Some(i) => i,
        None if identified => {
            entries.push(OutputConfig {
                make: Some(o.make.clone()),
                model: Some(o.model.clone()),
                serial: Some(o.serial.clone()).filter(|s| !s.is_empty()),
                ..OutputConfig::default()
            });
            entries.len() - 1
        }
        // Ahead of entries by make and model, which would match it first.
        None => {
            entries.insert(0, OutputConfig { name: Some(o.name.clone()), ..OutputConfig::default() });
            0
        }
    };
    &mut entries[i]
}

/// "Going back by itself in 15 seconds".
pub fn back_in(left: Duration) -> String {
    match left.as_secs_f64().ceil() as u64 {
        1 => "Going back by itself in a second".into(),
        s => format!("Going back by itself in {s} seconds"),
    }
}

impl Chanoma {
    /// Ask the compositor for its displays (as the settings open).
    pub(super) fn list_displays(&mut self, cx: &mut Cx<Msg>) {
        if !self.config.compositor {
            return;
        }
        cx.spawn(|| Msg::Displays(connect().and_then(|mut c| outputs(&mut c))));
    }

    pub fn displays_listed(&mut self, result: Result<Vec<OutputInfo>, String>) {
        if let Err(e) = &result {
            log::warn!("chanoma: the displays: {e}");
        }
        // A list asked for before a change mustn't undo the change's.
        if !self.displays.busy {
            self.displays.listed = Some(result);
        }
    }

    /// The display named, as the compositor said last.
    pub fn display(&self, name: &str) -> Option<&OutputInfo> {
        match &self.displays.listed {
            Some(Ok(list)) => list.iter().find(|o| o.name == name),
            _ => None,
        }
    }

    /// Step a display's size of things, resolution or refresh rate.
    pub(super) fn step_display(&mut self, name: &str, what: Setting, forward: bool, cx: &mut Cx<Msg>) {
        if self.displays.busy || self.displays.keep.is_some() {
            return self.toast("A display change is still going", cx);
        }
        let Some(o) = self.display(name).cloned() else { return };
        let (Some(now), Some((mode, scale))) = (o.current_mode.clone(), stepped(&o, what, forward)) else { return };
        let change = if what == Setting::Size {
            Change::Size
        } else {
            self.displays.keep = Some(Keep { name: name.to_owned(), mode: now, scale: o.scale, since: Instant::now() });
            Change::Asks
        };
        self.send_display(request(name, mode.as_ref(), scale), change, cx);
    }

    fn send_display(&mut self, req: Request, change: Change, cx: &mut Cx<Msg>) {
        let Request::OutputConfig { name, .. } = &req else { return };
        let name = name.clone();
        self.displays.busy = true;
        if !self.config.compositor {
            self.displays.asked.push(req.clone());
            let mut list = match &self.displays.listed {
                Some(Ok(list)) => list.clone(),
                _ => Vec::new(),
            };
            let result = take(&mut list, &req).map(|()| list);
            return self.display_changed(change, name, result, cx);
        }
        cx.spawn(move || {
            let result = connect().and_then(|mut c| {
                c.call(&req).map_err(|e| e.to_string())?;
                outputs(&mut c)
            });
            Msg::DisplayChanged(change, name, result)
        });
    }

    /// The compositor answered a change.
    pub fn display_changed(
        &mut self,
        change: Change,
        name: String,
        result: Result<Vec<OutputInfo>, String>,
        cx: &mut Cx<Msg>,
    ) {
        self.displays.busy = false;
        match result {
            Err(e) => {
                log::warn!("chanoma: changing {name}: {e}");
                if change == Change::Asks {
                    self.displays.keep = None;
                }
                self.toast("Couldn't change the display", cx);
                self.list_displays(cx);
            }
            Ok(list) => {
                self.displays.listed = Some(Ok(list));
                match change {
                    Change::Size => self.save_display(&name),
                    Change::Asks => {
                        if let Some(k) = &mut self.displays.keep {
                            k.since = Instant::now();
                        }
                        self.open_menu(MenuKind::KeepDisplay, cx);
                    }
                    Change::Back => {}
                }
            }
        }
    }

    /// Kept: in Aizome's settings, for the sessions to come.
    pub(super) fn keep_display(&mut self) {
        if let Some(k) = self.displays.keep.take() {
            self.save_display(&k.name);
        }
    }

    /// Back as the display was (the settings still say so).
    pub(super) fn display_back(&mut self, cx: &mut Cx<Msg>) {
        let Some(k) = self.displays.keep.take() else { return };
        if matches!(self.modal, Some(Modal::Menu { kind: MenuKind::KeepDisplay, .. })) {
            self.close_modal(cx);
        }
        self.send_display(request(&k.name, Some(&k.mode), Some(k.scale)), Change::Back, cx);
    }

    /// Each second: the time left to keep a change, and back when it's up.
    pub(super) fn display_tick(&mut self, cx: &mut Cx<Msg>) {
        let Some(k) = &self.displays.keep else { return };
        if self.displays.busy {
            return;
        }
        let left = KEEP_FOR.saturating_sub(k.since.elapsed());
        if left.is_zero() {
            return self.display_back(cx);
        }
        if let Some(Modal::Menu { menu, kind: MenuKind::KeepDisplay }) = &mut self.modal {
            menu.subtitle = back_in(left);
        }
    }

    /// The display as it is now, in Aizome's settings.
    fn save_display(&mut self, name: &str) {
        let Some(o) = self.display(name).cloned() else { return };
        let Some(m) = &o.current_mode else { return };
        let path = &self.config.display_settings;
        let mut settings = match aizome_config::Settings::load_from(path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("chanoma: {}: {e}; the display's settings aren't kept", path.display());
                return;
            }
        };
        let e = entry(&mut settings.outputs, &o);
        e.mode = Some(mode_setting(m));
        e.scale = Some(o.scale);
        let saved = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .map_err(|e| e.to_string())
            .and_then(|()| settings.save_to(path).map_err(|e| e.to_string()));
        if let Err(e) = saved {
            log::warn!("chanoma: {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: i32, height: i32, hz: i32, preferred: bool) -> Mode {
        Mode { width, height, refresh: hz * 1000, preferred }
    }

    fn tv() -> OutputInfo {
        OutputInfo {
            name: "HDMI-A-1".into(),
            make: "GSM".into(),
            model: "LG TV SSCR2".into(),
            enabled: true,
            current_mode: Some(mode(3840, 2160, 60, true)),
            modes: vec![
                mode(3840, 2160, 60, true),
                mode(3840, 2160, 120, false),
                mode(4096, 2160, 60, false),
                mode(1920, 1080, 120, false),
                mode(1920, 1080, 60, false),
                mode(1280, 720, 60, false),
            ],
            scale: 2.0,
            ..OutputInfo::default()
        }
    }

    #[test]
    fn a_displays_choices_go_up_to_the_right() {
        let o = tv();
        assert_eq!(sizes(&o), [(1280, 720), (1920, 1080), (3840, 2160), (4096, 2160)]);
        assert_eq!(rates(&o, (3840, 2160)), [60_000, 120_000]);
        assert_eq!(scales(&mode(3840, 2160, 60, true)), SCALES);
        assert_eq!(scales(&mode(1920, 1080, 60, true)), [1.0, 1.5, 2.0]);
        assert_eq!(scales(&mode(640, 480, 60, true)), [1.0]);
        assert_eq!(title(&o), "LG TV SSCR2");
        assert_eq!(size_words(&o, &mode(3840, 2160, 120, false)), "3840 × 2160 · its own");
        assert_eq!(scale_words(2.0, &mode(3840, 2160, 60, true)), "2× · as 1920 × 1080");
        assert_eq!(scale_words(1.5, &mode(3840, 2160, 60, true)), "1.5× · as 2560 × 1440");
        assert_eq!([hz(60_000), hz(59_940), hz(119_880)], ["60 Hz", "59.94 Hz", "119.88 Hz"]);
        assert_eq!(mode_setting(&mode(3840, 2160, 60, true)), "3840x2160@60");
        assert_eq!(mode_setting(&Mode { refresh: 59_940, ..mode(1920, 1080, 0, false) }), "1920x1080@59.940");
    }

    #[test]
    fn a_step_keeps_things_as_big_and_stops_at_the_ends() {
        let o = tv();
        // Smaller: 1080p at its refresh rate nearest, things as big (1×).
        assert_eq!(stepped(&o, Setting::Resolution, false), Some((Some(mode(1920, 1080, 60, false)), Some(1.0))));
        assert_eq!(stepped(&o, Setting::Refresh, true), Some((Some(mode(3840, 2160, 120, false)), None)));
        assert_eq!(stepped(&o, Setting::Refresh, false), None, "60 Hz is its slowest");
        assert_eq!(stepped(&o, Setting::Size, true), Some((None, Some(2.5))));
        let biggest = OutputInfo { scale: 4.0, ..tv() };
        assert_eq!(stepped(&biggest, Setting::Size, true), None);
        // A scale set elsewhere steps to the next offered either side.
        let odd = OutputInfo { scale: 1.75, ..tv() };
        assert_eq!(stepped(&odd, Setting::Size, false), Some((None, Some(1.5))));
        assert_eq!(stepped(&odd, Setting::Size, true), Some((None, Some(2.0))));
    }

    #[test]
    fn the_settings_entry_is_the_displays_own() {
        let o = tv();
        let mut entries = vec![
            OutputConfig { name: Some("DP-1".into()), scale: Some(1.0), ..OutputConfig::default() },
            OutputConfig { make: Some("GSM".into()), model: Some("LG TV SSCR2".into()), ..OutputConfig::default() },
        ];
        entry(&mut entries, &o).scale = Some(2.5);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].scale, Some(2.5));
        // Another of the same model with a serial is a display of its own.
        let other = OutputInfo { serial: "123".into(), ..tv() };
        entry(&mut entries, &other).scale = Some(1.0);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].serial.as_deref(), Some("123"));
        // One that doesn't say what it is: by its connector, first.
        let unknown = OutputInfo { make: String::new(), model: String::new(), ..tv() };
        entry(&mut entries, &unknown).scale = Some(2.0);
        assert_eq!(entries[0].name.as_deref(), Some("HDMI-A-1"));
    }
}
