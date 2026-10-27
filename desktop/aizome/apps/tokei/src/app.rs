//! Tokei, the program: world clocks, alarms, a timer and a stopwatch.

use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key as KeyName, KeyEvent, NamedKey};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::alarms::{self, Alarm, Scheduler};
use crate::cities::{self, City};
use crate::clock::Clock;
use crate::sound::Player;
use crate::timing::{Stopwatch, Timer};

pub const APP_ID: &str = "org.aizome.Tokei";

/// The four parts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Page {
    #[default]
    World,
    Alarms,
    Timer,
    Stopwatch,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::World, Page::Alarms, Page::Timer, Page::Stopwatch];

    pub fn name(self) -> &'static str {
        match self {
            Page::World => "World Clock",
            Page::Alarms => "Alarms",
            Page::Timer => "Timer",
            Page::Stopwatch => "Stopwatch",
        }
    }

    pub fn index(self) -> usize {
        Page::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// A world clock: the city's name and zone (it's found in the table by
/// them, for its place on the globe).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    pub zone: String,
}

impl Place {
    pub fn of(c: &City) -> Place {
        Place { name: c.name.clone(), zone: c.zone.clone() }
    }

    /// Where it is: the table's coordinates (or the zone's main city's).
    pub fn coords(&self) -> (f64, f64) {
        cities::find(&self.name, &self.zone)
            .or_else(|| cities::all().iter().find(|c| c.zone == self.zone))
            .map_or((0.0, 0.0), |c| (c.lat, c.lon))
    }

    pub fn country(&self) -> String {
        cities::find(&self.name, &self.zone).map(|c| c.country.clone()).unwrap_or_default()
    }
}

/// What's kept between runs: `$XDG_STATE_HOME/aizome/tokei/state.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub page: Page,
    pub places: Vec<Place>,
    /// The timer's last length, in seconds.
    pub timer_secs: u64,
}

impl Default for Saved {
    fn default() -> Saved {
        let place = |name: &str, zone: &str| Place { name: name.into(), zone: zone.into() };
        Saved {
            page: Page::World,
            places: vec![
                place("New York", "America/New_York"),
                place("London", "Europe/London"),
                place("Tokyo", "Asia/Tokyo"),
            ],
            timer_secs: 300,
        }
    }
}

pub fn state_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("aizome/tokei/state.json")
}

/// The timer's presets, in minutes.
pub const PRESETS: [u64; 7] = [1, 3, 5, 10, 15, 30, 60];

#[derive(Clone, Debug)]
pub enum Msg {
    /// The clock's second ticked.
    Tick,
    /// A stopwatch frame.
    Frame,
    Page(Page),
    // World clocks.
    SelectPlace(Vec<usize>),
    RemovePlace(usize),
    MovePlace(usize, i32),
    AddCity,
    CityQuery(String),
    CityPick(Vec<usize>),
    CityAdd(Option<usize>),
    // Alarms.
    NewAlarm,
    EditAlarm(usize),
    SelectAlarm(Vec<usize>),
    ToggleAlarm(usize, bool),
    DeleteAlarm(usize),
    UndoDelete,
    DraftTime(String),
    DraftLabel(String),
    DraftDay(u8),
    DraftSound(usize),
    DraftWake(bool),
    DraftSnooze(f64),
    PreviewSound,
    SaveAlarm,
    CloseSheet,
    // Timer.
    TimerPreset(u64),
    TimerEntry(String),
    TimerSet,
    TimerStart,
    TimerCancel,
    TimerCheck,
    // Stopwatch.
    WatchToggle,
    WatchLap,
    WatchReset,
    Persist(u64),
    Raise,
}

/// A sheet over the window.
pub enum Sheet {
    AddCity { key: WindowKey, query: String, results: Vec<&'static City>, picked: usize },
    Alarm { key: WindowKey, draft: Alarm, index: Option<usize>, time: String, error: Option<String> },
}

impl Sheet {
    pub fn key(&self) -> WindowKey {
        match self {
            Sheet::AddCity { key, .. } | Sheet::Alarm { key, .. } => *key,
        }
    }
}

pub struct Tokei {
    pub page: Page,
    pub clock: Clock,
    pub local: TimeZone,
    pub h24: bool,
    /// The time as of the last tick.
    pub now: Timestamp,
    pub mono: Duration,
    pub places: Vec<Place>,
    pub selected_place: Option<usize>,
    pub alarms: Vec<Alarm>,
    pub alarms_path: Option<PathBuf>,
    pub scheduler: Box<dyn Scheduler>,
    pub player: Box<dyn Player>,
    pub selected_alarm: Option<usize>,
    pub deleted: Option<(usize, Alarm)>,
    /// What scheduling said last, if it failed.
    pub schedule_error: Option<String>,
    pub timer: Timer,
    pub timer_entry: String,
    timer_end: Option<TimerHandle>,
    /// The next tick.
    ticker: Option<TimerHandle>,
    pub stopwatch: Stopwatch,
    frames: Option<TimerHandle>,
    pub sheet: Option<Sheet>,
    pub window: WindowKey,
    state_path: Option<PathBuf>,
    persist_seq: u64,
    pub instance: Option<std::os::unix::net::UnixListener>,
    /// Notifications posted (the timer's end), for tests.
    pub notified: Vec<String>,
    /// Post nothing to the desktop (tests).
    pub quiet: bool,
}

impl Tokei {
    /// Tokei with its state and alarms from these files (none: afresh,
    /// keeping nothing), scheduling through `scheduler`.
    pub fn new(
        state: Option<PathBuf>,
        alarms_path: Option<PathBuf>,
        scheduler: Box<dyn Scheduler>,
        player: Box<dyn Player>,
    ) -> Tokei {
        let saved: Saved = state
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let alarms = alarms_path.as_deref().map(alarms::load).unwrap_or_default();
        let clock = Clock::default();
        let h24 =
            aizome_config::Settings::load_or_default().menubar_settings().clock == aizome_config::ClockFormat::H24;
        let timer = Timer::new(Duration::from_secs(saved.timer_secs.clamp(1, 99 * 3600)));
        Tokei {
            page: saved.page,
            now: clock.now(),
            mono: clock.mono(),
            clock,
            local: crate::clock::local_zone(),
            h24,
            places: saved.places,
            selected_place: None,
            alarms,
            alarms_path,
            scheduler,
            player,
            selected_alarm: None,
            deleted: None,
            schedule_error: None,
            timer_entry: crate::timing::timer_text(timer.length),
            timer,
            timer_end: None,
            ticker: None,
            stopwatch: Stopwatch::default(),
            frames: None,
            sheet: None,
            window: WindowKey::NONE,
            state_path: state,
            persist_seq: 0,
            instance: None,
            notified: Vec::new(),
            quiet: false,
        }
    }

    fn saved(&self) -> Saved {
        Saved { page: self.page, places: self.places.clone(), timer_secs: self.timer.length.as_secs() }
    }

    fn persist_now(&self) {
        let Some(path) = &self.state_path else { return };
        let Ok(json) = serde_json::to_string_pretty(&self.saved()) else { return };
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = path.with_extension("json.new");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    fn persist_soon(&mut self, cx: &mut Cx<Msg>) {
        if self.state_path.is_some() {
            self.persist_seq += 1;
            cx.after(Duration::from_millis(600), Msg::Persist(self.persist_seq));
        }
    }

    /// The local zone's city: `Berlin` for Europe/Berlin.
    pub fn local_name(&self) -> String {
        crate::clock::zone_city(&self.local).unwrap_or_else(|| "Here".into())
    }

    /// Read the clock, and ask for the next tick on the next second.
    /// Read the clock, and ask for the next tick: on the next second of
    /// the wall clock, or of a running timer when the timer shows (so its
    /// digits turn as its seconds do).
    fn tick(&mut self, cx: &mut Cx<Msg>) {
        self.now = self.clock.now();
        self.mono = self.clock.mono();
        if let Some(h) = self.ticker.take() {
            cx.cancel(h);
        }
        const SECOND: u64 = 1_000_000_000;
        let to_next = |nanos_into: u64| Duration::from_nanos(SECOND - nanos_into.min(SECOND - 1));
        let wait = if self.page == Page::Timer && self.timer.running() {
            let left = self.timer.left(self.mono);
            match u64::from(left.subsec_nanos()) {
                0 => Duration::from_secs(1),
                n => Duration::from_nanos(n),
            }
        } else {
            to_next(self.now.subsec_nanosecond().max(0) as u64)
        };
        self.ticker = Some(cx.after(wait.max(Duration::from_millis(5)), Msg::Tick));
    }

    /// Run the stopwatch's frames while it runs and shows.
    fn frames(&mut self, cx: &mut Cx<Msg>) {
        let want = self.stopwatch.running() && self.page == Page::Stopwatch;
        match (want, self.frames) {
            (true, None) => self.frames = Some(cx.every(Duration::from_millis(40), Msg::Frame)),
            (false, Some(h)) => {
                cx.cancel(h);
                self.frames = None;
            }
            _ => {}
        }
    }

    // Alarms.

    fn save_alarms(&mut self) {
        if let Some(p) = &self.alarms_path
            && let Err(e) = alarms::save(p, &self.alarms)
        {
            self.schedule_error = Some(format!("Couldn't save the alarms: {e}"));
        }
    }

    /// Schedule (or unschedule) alarm `i` as it now is.
    fn reschedule(&mut self, i: usize) {
        let Some(a) = self.alarms.get(i).cloned() else { return };
        self.schedule_error = None;
        if a.enabled {
            if let Err(e) = self.scheduler.schedule(&a, self.now, &self.local) {
                self.schedule_error = Some(format!("Couldn't schedule the alarm: {e}"));
            }
        } else {
            self.scheduler.cancel(&a.id);
        }
    }

    /// Make the system's timers match the alarms (at launch).
    pub fn sync_alarms(&mut self) {
        let errors = alarms::sync(self.scheduler.as_mut(), &self.alarms, self.now, &self.local);
        self.schedule_error = errors.first().map(|e| format!("Couldn't schedule an alarm: {e}"));
    }

    fn open_alarm_sheet(&mut self, index: Option<usize>, cx: &mut Cx<Msg>) {
        if self.sheet.is_some() {
            return;
        }
        let draft = match index.and_then(|i| self.alarms.get(i)) {
            Some(a) => a.clone(),
            None => {
                let seed = self.now.as_nanosecond() as u64;
                let mut a = Alarm { id: alarms::new_id(&self.alarms, seed), ..Alarm::default() };
                // The next whole hour, as a start.
                let here = self.now.to_zoned(self.local.clone());
                a.hour = ((here.hour() + 1) % 24) as u8;
                a.minute = 0;
                a
            }
        };
        let time = format!("{:02}:{:02}", draft.hour, draft.minute);
        let key = cx.show_sheet(self.window);
        self.sheet = Some(Sheet::Alarm { key, draft, index, time, error: None });
        cx.focus("alarm-time");
    }

    fn close_sheet(&mut self, cx: &mut Cx<Msg>) {
        if let Some(s) = self.sheet.take() {
            cx.close_window(s.key());
        }
        cx.focus("tokei-keys");
    }

    /// The next alarm to ring, and when.
    pub fn next_alarm(&self) -> Option<(Timestamp, &Alarm)> {
        self.alarms
            .iter()
            .filter(|a| a.enabled)
            .filter_map(|a| a.next(self.now, &self.local).map(|t| (t, a)))
            .min_by_key(|(t, _)| *t)
    }

    // The timer.

    fn timer_start(&mut self, cx: &mut Cx<Msg>) {
        let now = self.clock.mono();
        if let Some(ends) = self.timer.start(now) {
            if let Some(h) = self.timer_end.take() {
                cx.cancel(h);
            }
            self.timer_end = Some(cx.after(ends.saturating_sub(now), Msg::TimerCheck));
        }
    }

    fn timer_stop_waiting(&mut self, cx: &mut Cx<Msg>) {
        if let Some(h) = self.timer_end.take() {
            cx.cancel(h);
        }
    }

    fn set_timer(&mut self, length: Duration, cx: &mut Cx<Msg>) {
        self.timer_stop_waiting(cx);
        self.timer = Timer::new(length);
        self.timer_entry = crate::timing::timer_text(length);
        self.persist_soon(cx);
    }
}

/// The keys the window answers itself (nothing focused, or keys a
/// focused control left alone).
pub fn key_message(page: Page, k: &KeyEvent, here: bool) -> Option<Msg> {
    let m = k.modifiers;
    if m.super_key() || m.control() || m.alt() {
        return None;
    }
    let ch = match &k.key {
        KeyName::Character(s) => s.chars().next().map(|c| c.to_ascii_lowercase()),
        _ => None,
    };
    match (page, &k.key, ch) {
        (Page::World, KeyName::Named(NamedKey::Delete | NamedKey::Backspace), _) => {
            None.or(Some(Msg::RemovePlace(usize::MAX)))
        }
        (Page::Alarms, KeyName::Named(NamedKey::Delete | NamedKey::Backspace), _) => Some(Msg::DeleteAlarm(usize::MAX)),
        (Page::Stopwatch, _, Some(' ')) if here => Some(Msg::WatchToggle),
        (Page::Stopwatch, _, Some('l')) => Some(Msg::WatchLap),
        (Page::Stopwatch, _, Some('r')) => Some(Msg::WatchReset),
        (Page::Timer, _, Some(' ')) if here => Some(Msg::TimerStart),
        (Page::Timer, KeyName::Named(NamedKey::Escape), _) => Some(Msg::TimerCancel),
        _ => None,
    }
}

impl Program for Tokei {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Tokei", "Clock").icon(Icon::AlarmClock).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(WindowSpec::new("Tokei").size(560.0, 620.0).min_size(460.0, 520.0));
        cx.focus("tokei-keys");
        self.tick(cx);
        self.sync_alarms();
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, cx.stream(), || Msg::Raise);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        // The timer's changes move the ticks to its own seconds (or back).
        let rephase =
            matches!(msg, Msg::TimerPreset(_) | Msg::TimerSet | Msg::TimerStart | Msg::TimerCancel | Msg::TimerCheck);
        match msg {
            Msg::Tick => self.tick(cx),
            Msg::Frame => self.mono = self.clock.mono(),
            Msg::Page(p) => {
                if p != self.page {
                    self.page = p;
                    self.tick(cx);
                    self.frames(cx);
                    self.persist_soon(cx);
                    cx.focus("tokei-keys");
                }
            }
            Msg::SelectPlace(sel) => self.selected_place = sel.last().copied(),
            Msg::RemovePlace(i) => {
                let i = if i == usize::MAX { self.selected_place.unwrap_or(usize::MAX) } else { i };
                if i < self.places.len() {
                    self.places.remove(i);
                    self.selected_place = (!self.places.is_empty()).then(|| i.min(self.places.len() - 1));
                    self.persist_soon(cx);
                }
            }
            Msg::MovePlace(i, by) => {
                let j = i as i64 + i64::from(by);
                if i < self.places.len() && (0..self.places.len() as i64).contains(&j) {
                    self.places.swap(i, j as usize);
                    self.selected_place = Some(j as usize);
                    self.persist_soon(cx);
                }
            }
            Msg::AddCity => {
                if self.sheet.is_none() {
                    let key = cx.show_sheet(self.window);
                    self.sheet = Some(Sheet::AddCity { key, query: String::new(), results: Vec::new(), picked: 0 });
                    cx.focus("city-search");
                }
            }
            Msg::CityQuery(q) => {
                if let Some(Sheet::AddCity { query, results, picked, .. }) = &mut self.sheet {
                    *results = cities::search(&q, 60);
                    *query = q;
                    *picked = 0;
                }
            }
            Msg::CityPick(sel) => {
                if let Some(Sheet::AddCity { picked, .. }) = &mut self.sheet
                    && let Some(i) = sel.last()
                {
                    *picked = *i;
                }
            }
            Msg::CityAdd(i) => {
                let city = match &self.sheet {
                    Some(Sheet::AddCity { results, picked, .. }) => results.get(i.unwrap_or(*picked)).copied(),
                    _ => None,
                };
                if let Some(c) = city {
                    let place = Place::of(c);
                    if !self.places.contains(&place) {
                        self.places.push(place);
                    }
                    self.selected_place = self.places.iter().position(|p| p.name == c.name && p.zone == c.zone);
                    self.close_sheet(cx);
                    self.persist_soon(cx);
                }
            }
            Msg::NewAlarm => self.open_alarm_sheet(None, cx),
            Msg::EditAlarm(i) => self.open_alarm_sheet(Some(i), cx),
            Msg::SelectAlarm(sel) => self.selected_alarm = sel.last().copied(),
            Msg::ToggleAlarm(i, on) => {
                if let Some(a) = self.alarms.get_mut(i) {
                    a.enabled = on;
                    self.save_alarms();
                    self.reschedule(i);
                }
            }
            Msg::DeleteAlarm(i) => {
                let i = if i == usize::MAX { self.selected_alarm.unwrap_or(usize::MAX) } else { i };
                if i < self.alarms.len() {
                    let a = self.alarms.remove(i);
                    self.scheduler.cancel(&a.id);
                    self.deleted = Some((i, a));
                    self.selected_alarm = (!self.alarms.is_empty()).then(|| i.min(self.alarms.len() - 1));
                    self.save_alarms();
                    if matches!(&self.sheet, Some(Sheet::Alarm { .. })) {
                        self.close_sheet(cx);
                    }
                }
            }
            Msg::UndoDelete => {
                if let Some((i, a)) = self.deleted.take() {
                    let i = i.min(self.alarms.len());
                    self.alarms.insert(i, a);
                    self.save_alarms();
                    self.reschedule(i);
                    self.selected_alarm = Some(i);
                }
            }
            Msg::DraftTime(t) => {
                if let Some(Sheet::Alarm { draft, time, error, .. }) = &mut self.sheet {
                    match parse_time(&t) {
                        Some((h, m)) => {
                            draft.hour = h;
                            draft.minute = m;
                            *error = None;
                        }
                        None => *error = (!t.trim().is_empty()).then(|| "A time such as 7:30 or 19:30".to_owned()),
                    }
                    *time = t;
                }
            }
            Msg::DraftLabel(l) => {
                if let Some(Sheet::Alarm { draft, .. }) = &mut self.sheet {
                    draft.label = l;
                }
            }
            Msg::DraftDay(d) => {
                if let Some(Sheet::Alarm { draft, .. }) = &mut self.sheet {
                    match draft.days.iter().position(|x| *x == d) {
                        Some(p) => {
                            draft.days.remove(p);
                        }
                        None => {
                            draft.days.push(d);
                            draft.days.sort_unstable();
                        }
                    }
                }
            }
            Msg::DraftSound(i) => {
                if let Some(Sheet::Alarm { draft, .. }) = &mut self.sheet
                    && let Some((id, _)) = crate::sound::SOUNDS.get(i)
                {
                    draft.sound = (*id).to_owned();
                    let id = draft.sound.clone();
                    self.player.start(&id);
                }
            }
            Msg::PreviewSound => {
                if let Some(Sheet::Alarm { draft, .. }) = &self.sheet {
                    let id = draft.sound.clone();
                    if self.player.playing() {
                        self.player.stop();
                    } else {
                        self.player.start(&id);
                    }
                }
            }
            Msg::DraftWake(on) => {
                if let Some(Sheet::Alarm { draft, .. }) = &mut self.sheet {
                    draft.wake = on;
                }
            }
            Msg::DraftSnooze(v) => {
                if let Some(Sheet::Alarm { draft, .. }) = &mut self.sheet {
                    draft.snooze_minutes = v.round().clamp(1.0, 60.0) as u32;
                }
            }
            Msg::SaveAlarm => {
                let Some(Sheet::Alarm { draft, index, error, time, .. }) = &self.sheet else { return };
                if error.is_some() || parse_time(time).is_none() {
                    return;
                }
                let mut a = draft.clone();
                a.label = a.label.trim().to_owned();
                let i = match *index {
                    Some(i) if i < self.alarms.len() => {
                        self.alarms[i] = a;
                        i
                    }
                    _ => {
                        a.enabled = true;
                        self.alarms.push(a);
                        self.alarms.len() - 1
                    }
                };
                // Keep them in the order of the day.
                let id = self.alarms[i].id.clone();
                self.alarms.sort_by_key(|a| (a.hour, a.minute));
                let i = self.alarms.iter().position(|a| a.id == id).unwrap_or(i);
                self.player.stop();
                self.save_alarms();
                self.reschedule(i);
                self.selected_alarm = Some(i);
                self.close_sheet(cx);
            }
            Msg::CloseSheet => {
                self.player.stop();
                self.close_sheet(cx);
            }
            Msg::TimerPreset(min) => {
                self.set_timer(Duration::from_secs(min * 60), cx);
                self.timer_start(cx);
            }
            Msg::TimerEntry(s) => self.timer_entry = s,
            Msg::TimerSet => {
                if let Some(d) = crate::timing::parse_length(&self.timer_entry) {
                    self.set_timer(d, cx);
                    self.timer_start(cx);
                }
            }
            Msg::TimerStart => {
                let now = self.clock.mono();
                if self.timer.running() {
                    self.timer.pause(now);
                    self.timer_stop_waiting(cx);
                } else {
                    if let Some(d) = crate::timing::parse_length(&self.timer_entry)
                        && d != self.timer.length
                        && !matches!(self.timer.state, crate::timing::TimerState::Paused { .. })
                    {
                        self.set_timer(d, cx);
                    }
                    self.timer_start(cx);
                }
                self.mono = now;
            }
            Msg::TimerCancel => {
                self.timer_stop_waiting(cx);
                self.timer.reset();
                self.player.stop();
            }
            Msg::TimerCheck => {
                self.timer_end = None;
                let now = self.clock.mono();
                self.mono = now;
                if self.timer.check(now) {
                    let len = crate::timing::timer_text(self.timer.length);
                    if !self.quiet {
                        cx.notify("Timer done", &format!("{len} is up."));
                    }
                    self.notified.push(format!("Timer done: {len}"));
                    self.player.start("complete");
                } else if self.timer.running() {
                    // Woken early: wait for the rest.
                    let left = self.timer.left(now);
                    self.timer_end = Some(cx.after(left, Msg::TimerCheck));
                }
            }
            Msg::WatchToggle => {
                let now = self.clock.mono();
                if self.stopwatch.running() {
                    self.stopwatch.stop(now);
                } else {
                    self.stopwatch.start(now);
                }
                self.mono = now;
                self.frames(cx);
            }
            Msg::WatchLap => {
                let now = self.clock.mono();
                self.stopwatch.lap(now);
                self.mono = now;
            }
            Msg::WatchReset => {
                if !self.stopwatch.running() {
                    self.stopwatch.reset();
                    self.mono = self.clock.mono();
                }
            }
            Msg::Persist(seq) => {
                if seq == self.persist_seq {
                    self.persist_now();
                }
            }
            Msg::Raise => cx.activate(self.window),
        }
        if rephase {
            self.tick(cx);
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if let Some(s) = self.sheet.as_ref().filter(|s| s.key() == window) {
            return crate::views::sheet(self, s);
        }
        crate::views::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let file = Menu::new("File")
            .item(
                MenuItem::new("Add City…", Msg::AddCity).shortcut(Shortcut::cmd('N')).enabled(self.page == Page::World),
            )
            .item(
                MenuItem::new("New Alarm…", Msg::NewAlarm)
                    .shortcut(Shortcut::cmd('N'))
                    .enabled(self.page == Page::Alarms),
            )
            .separator()
            .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
        let mut view = Menu::new("View");
        for (i, p) in Page::ALL.iter().enumerate() {
            let digit = char::from(b'1' + i as u8);
            view = view
                .item(MenuItem::new(p.name(), Msg::Page(*p)).shortcut(Shortcut::cmd(digit)).checked(self.page == *p));
        }
        let mut clock = Menu::new("Clock");
        match self.page {
            Page::World => {
                let sel = self.selected_place;
                clock = clock
                    .item(MenuItem::new("Add City…", Msg::AddCity))
                    .item(
                        MenuItem::new("Remove City", Msg::RemovePlace(sel.unwrap_or(usize::MAX)))
                            .shortcut(Shortcut::cmd_key(ShortcutKey::Backspace))
                            .enabled(sel.is_some()),
                    )
                    .separator()
                    .item(
                        MenuItem::new("Move Up", Msg::MovePlace(sel.unwrap_or(0), -1))
                            .shortcut(Shortcut::cmd_key(ShortcutKey::Up).shift())
                            .enabled(sel.is_some_and(|i| i > 0)),
                    )
                    .item(
                        MenuItem::new("Move Down", Msg::MovePlace(sel.unwrap_or(0), 1))
                            .shortcut(Shortcut::cmd_key(ShortcutKey::Down).shift())
                            .enabled(sel.is_some_and(|i| i + 1 < self.places.len())),
                    );
            }
            Page::Alarms => {
                let sel = self.selected_alarm;
                let on = sel.and_then(|i| self.alarms.get(i)).is_some_and(|a| a.enabled);
                clock = clock
                    .item(MenuItem::new("New Alarm…", Msg::NewAlarm))
                    .item(
                        MenuItem::new("Edit Alarm…", Msg::EditAlarm(sel.unwrap_or(0)))
                            .shortcut(Shortcut::cmd('E'))
                            .enabled(sel.is_some()),
                    )
                    .item(
                        MenuItem::new(if on { "Turn Off" } else { "Turn On" }, Msg::ToggleAlarm(sel.unwrap_or(0), !on))
                            .shortcut(Shortcut::cmd('T'))
                            .enabled(sel.is_some()),
                    )
                    .item(
                        MenuItem::new("Delete Alarm", Msg::DeleteAlarm(sel.unwrap_or(usize::MAX)))
                            .shortcut(Shortcut::cmd_key(ShortcutKey::Backspace))
                            .enabled(sel.is_some()),
                    );
            }
            Page::Timer => {
                let label = if self.timer.running() { "Pause" } else { "Start" };
                clock = clock
                    .item(MenuItem::new(label, Msg::TimerStart).shortcut(Shortcut::key(ShortcutKey::Space)))
                    .item(MenuItem::new("Cancel", Msg::TimerCancel).shortcut(Shortcut::key(ShortcutKey::Escape)))
                    .separator();
                for m in PRESETS {
                    clock = clock.item(MenuItem::new(preset_words(m), Msg::TimerPreset(m)));
                }
            }
            Page::Stopwatch => {
                let label = if self.stopwatch.running() { "Stop" } else { "Start" };
                clock = clock
                    .item(MenuItem::new(label, Msg::WatchToggle).shortcut(Shortcut::key(ShortcutKey::Space)))
                    .item(
                        MenuItem::new("Lap", Msg::WatchLap)
                            .shortcut(Shortcut::key(ShortcutKey::Char('L')))
                            .enabled(self.stopwatch.running()),
                    )
                    .item(
                        MenuItem::new("Reset", Msg::WatchReset)
                            .shortcut(Shortcut::key(ShortcutKey::Char('R')))
                            .enabled(!self.stopwatch.running() && self.stopwatch.started()),
                    );
            }
        }
        let bar = MenuBar::standard(&self.info()).with_file_menu(file).with_menu(view).with_menu(clock);
        match &self.deleted {
            Some((_, a)) => {
                let what = if a.label.is_empty() { format!("{:02}:{:02}", a.hour, a.minute) } else { a.label.clone() };
                bar.replace(
                    "Edit",
                    Menu::new("Edit")
                        .item(
                            MenuItem::new(format!("Undo Delete “{what}”"), Msg::UndoDelete)
                                .shortcut(Shortcut::cmd('Z')),
                        )
                        .separator()
                        .item(MenuItem::role_item("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
                        .item(MenuItem::role_item("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('C')))
                        .item(MenuItem::role_item("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
                        .item(MenuItem::role_item("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A'))),
                )
            }
            None => bar,
        }
    }

    fn close_requested(&mut self, _window: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        self.persist_now();
        self.player.stop();
        true
    }
}

/// "5 Minutes", "1 Hour".
pub fn preset_words(min: u64) -> String {
    match min {
        60 => "1 Hour".into(),
        1 => "1 Minute".into(),
        m => format!("{m} Minutes"),
    }
}

/// A time of day typed: `7:30`, `07:30`, `730`, `7.30`, `7`, `7:30 pm`.
pub fn parse_time(s: &str) -> Option<(u8, u8)> {
    let s = s.trim().to_lowercase();
    let (body, pm, am) = if let Some(b) = s.strip_suffix("pm").or_else(|| s.strip_suffix("p")) {
        (b.trim().to_owned(), true, false)
    } else if let Some(b) = s.strip_suffix("am").or_else(|| s.strip_suffix("a")) {
        (b.trim().to_owned(), false, true)
    } else {
        (s.clone(), false, false)
    };
    let (h, m): (u32, u32) = if let Some((h, m)) = body.split_once([':', '.', 'h']) {
        (h.trim().parse().ok()?, if m.trim().is_empty() { 0 } else { m.trim().parse().ok()? })
    } else if body.chars().all(|c| c.is_ascii_digit()) && !body.is_empty() {
        match body.len() {
            1 | 2 => (body.parse().ok()?, 0),
            3 | 4 => {
                let (h, m) = body.split_at(body.len() - 2);
                (h.parse().ok()?, m.parse().ok()?)
            }
            _ => return None,
        }
    } else {
        return None;
    };
    let h = if pm || am {
        if !(1..=12).contains(&h) {
            return None;
        }
        (h % 12) + if pm { 12 } else { 0 }
    } else {
        h
    };
    (h < 24 && m < 60).then_some((h as u8, m as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_parse_many_ways() {
        assert_eq!(parse_time("7:30"), Some((7, 30)));
        assert_eq!(parse_time("07:05"), Some((7, 5)));
        assert_eq!(parse_time("730"), Some((7, 30)));
        assert_eq!(parse_time("1930"), Some((19, 30)));
        assert_eq!(parse_time("7"), Some((7, 0)));
        assert_eq!(parse_time("7.45"), Some((7, 45)));
        assert_eq!(parse_time("7:30 pm"), Some((19, 30)));
        assert_eq!(parse_time("12am"), Some((0, 0)));
        assert_eq!(parse_time("12:15 PM"), Some((12, 15)));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("13pm"), None);
        assert_eq!(parse_time("7:60"), None);
        assert_eq!(parse_time("soon"), None);
    }
}
