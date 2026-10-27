//! Koyomi, the program: its state, its messages, how they change it,
//! and its menus.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

use crate::edit::{self, OccRef, Repeat, Scope};
use crate::format;
use crate::locale::Weeks;
use crate::model::{Calendar, Dye, Event, When};
use crate::occur::{self, Index, Occurrence};
use crate::settings::{self, Clock, FirstDay, Settings, State};
use crate::store::{self, Store};
use crate::tz::Tz;
use crate::ui::editor::{Editor, Field};
use crate::ui::mini::{Mini, MiniAction, YearData};
use crate::ui::month::MonthData;
use crate::ui::week::{self, Bar, Block, GridAction, GridData, HeaderData, StripData};
use crate::ui::{self, KeyAction};

pub const APP_ID: &str = "org.aizome.Koyomi";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    Day,
    Week,
    #[default]
    Month,
    Year,
}

impl View {
    fn key(self) -> &'static str {
        match self {
            View::Day => "day",
            View::Week => "week",
            View::Month => "month",
            View::Year => "year",
        }
    }

    fn from_key(s: &str) -> Option<View> {
        [View::Day, View::Week, View::Month, View::Year].into_iter().find(|v| v.key() == s)
    }
}

/// How Koyomi is started: where the calendars are, and (for tests) a
/// clock, a zone and a locale of its own.
#[derive(Clone)]
pub struct Options {
    pub dir: PathBuf,
    pub now: Option<Timestamp>,
    pub zone: Option<TimeZone>,
    pub weeks: Option<Weeks>,
    pub settings: Option<Settings>,
    pub state: Option<State>,
    /// Keep settings and state in their files.
    pub persist: bool,
    /// Write calendars on a thread of their own (the app), or at once
    /// (tests).
    pub background_saves: bool,
}

impl Options {
    /// The app as people run it.
    pub fn real() -> Options {
        Options {
            dir: settings::data_dir(),
            now: None,
            zone: None,
            weeks: None,
            settings: None,
            state: None,
            persist: true,
            background_saves: true,
        }
    }

    /// For tests: `dir`, a fixed `now` in `zone`, nothing written but the
    /// calendars.
    pub fn test(dir: impl Into<PathBuf>, now: DateTime, zone: &str) -> Options {
        let tz = TimeZone::get(zone).expect("a zone");
        let ts = now.to_zoned(tz.clone()).expect("a time").timestamp();
        Options {
            dir: dir.into(),
            now: Some(ts),
            zone: Some(tz),
            weeks: Some(Weeks::default()),
            settings: Some(Settings { clock: Clock::H24, ..Settings::default() }),
            state: Some(State::default()),
            persist: false,
            background_saves: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    SetView(View),
    Prev,
    Next,
    Today,
    GoTo(Date),
    Grid(GridAction),
    Mini(MiniAction),
    Key(KeyAction),
    // Calendars.
    ToggleCalendar(String),
    Solo(String),
    SetColor(String, Dye),
    MakeDefault(String),
    NewCalendar,
    RenameCalendar(String),
    NameText(String),
    NameDone,
    NameCancel,
    DeleteCalendar(String),
    DeleteCalendarConfirmed(String),
    Export(String),
    ExportTo(String, Option<PathBuf>),
    Import,
    ImportInto(String),
    ImportFiles(Option<Vec<PathBuf>>),
    ImportTarget(usize),
    ImportDone,
    ImportCancel,
    // Events.
    NewEvent,
    Edit,
    EditField(Field),
    EditSave,
    EditCancel,
    EditDelete,
    Delete,
    Scope(Scope),
    ScopeCancel,
    Duplicate,
    Undo,
    Redo,
    // Settings.
    ShowSettings,
    SettingsDone,
    FirstDay(FirstDay),
    WeekNumbers(bool),
    ClockFormat(Clock),
    DefaultAlert(usize),
    DefaultAllDayAlert(usize),
    Snooze(f64),
    DayStarts(f64),
    ToggleSidebar,
    SidebarWidth(f64),
    // Housekeeping.
    Tick,
    Poll,
    Flush(u64),
    Saved(String, store::Stamp, Option<String>),
    Persist(u64),
    ClearNote(u64),
    Instance(Vec<String>),
    About,
    Nothing,
}

/// A change waiting for "this event or all?".
#[derive(Clone, Debug)]
enum Pending {
    Move { r: OccRef, start: DateTime, end: DateTime, all_day: bool },
    Delete(OccRef),
    Save,
}

/// Something done that can be undone: how to put things back.
#[derive(Clone, Debug)]
enum Change {
    /// A calendar's events with a UID, as they were.
    Events { cal: String, uid: String, events: Vec<Event> },
    /// A calendar taken away.
    Removed(Calendar),
    /// A calendar made.
    Made(String),
    /// A calendar's name and colour.
    Props { cal: String, name: String, color: Dye },
}

#[derive(Clone, Debug)]
struct Step {
    label: String,
    changes: Vec<Change>,
}

/// The import sheet.
pub struct ImportSheet {
    pub sheet: WindowKey,
    pub files: Vec<PathBuf>,
    pub parsed: Calendar,
    pub skipped: usize,
    /// Index into the calendars, or the end: a new calendar.
    pub target: usize,
}

/// The name sheet: a new calendar or a rename.
pub struct NameSheet {
    pub sheet: WindowKey,
    pub cal: Option<String>,
    pub text: String,
}

/// What the views are built from, kept until what they show changes.
#[derive(Default)]
struct Models {
    key: u64,
    month: Option<Rc<MonthData>>,
    header: Option<HeaderData>,
    strip: Option<Rc<StripData>>,
    grid: Option<Rc<GridData>>,
    year: Option<YearData>,
    mini: Option<Mini>,
    up_next: Vec<(Dye, String, String)>,
}

pub struct Koyomi {
    pub store: Store,
    pub settings: Settings,
    pub state: State,
    pub tz: TimeZone,
    pub here: Tz,
    pub locale: Weeks,
    pub view: View,
    /// The day selected (the day view's, the week's highlighted day).
    pub cursor: Date,
    /// The day the view is built around (the month shown, the week).
    pub anchor: Date,
    /// The mini-month's month.
    pub mini: (i16, i8),
    pub selected: Option<OccRef>,
    pub now: DateTime,
    pub today: Date,
    pub show_sidebar: bool,
    pub editor: Option<Editor>,
    pub import: Option<ImportSheet>,
    pub name: Option<NameSheet>,
    pub settings_sheet: Option<WindowKey>,
    pub note: Option<(String, u64)>,
    /// The occurrences the main view shows.
    pub occ: Rc<Vec<Occurrence>>,
    opts: Options,
    index: Index,
    /// Bumped whenever calendars change.
    generation: u64,
    models: std::cell::RefCell<Models>,
    window: WindowKey,
    pending: Option<Pending>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    dirty: BTreeSet<String>,
    writing: HashMap<String, u32>,
    flush_seq: u64,
    persist_seq: u64,
    note_seq: u64,
    scroll: (u64, i32),
    desktop_24h: Option<bool>,
    pub instance: Option<std::os::unix::net::UnixListener>,
    /// Files handed over before the window was up.
    waiting: Vec<PathBuf>,
    /// `--new` before the window was up.
    new_waiting: bool,
    /// The command line, taken once the window is up.
    pub args: Option<Vec<String>>,
}

impl Koyomi {
    pub fn new(opts: Options) -> Koyomi {
        let mut store = Store::open(&opts.dir);
        if let Err(e) = store.ensure_some() {
            log::warn!("{}: {e}", opts.dir.display());
        }
        let settings = opts.settings.clone().unwrap_or_else(Settings::load);
        let state = opts.state.clone().unwrap_or_else(State::load);
        let here = crate::tz::named(opts.zone.clone().unwrap_or_else(TimeZone::system));
        let tz = here.zone.clone().unwrap_or(TimeZone::UTC);
        let locale = opts.weeks.unwrap_or_else(Weeks::from_env);
        let now_ts = opts.now.unwrap_or_else(Timestamp::now);
        let now = now_ts.to_zoned(tz.clone()).datetime();
        let index = Index::new(&store.calendars, &tz);
        let view = state.view.as_deref().and_then(View::from_key).unwrap_or_default();
        let desktop_24h = if opts.persist { settings::desktop_24h() } else { None };
        let scroll = (1, (settings.day_starts.min(20) as i32) * 60 - 20);
        let mut k = Koyomi {
            store,
            settings,
            show_sidebar: true,
            state,
            tz,
            here,
            locale,
            view,
            cursor: now.date(),
            anchor: now.date(),
            mini: (now.date().year(), now.date().month()),
            selected: None,
            now,
            today: now.date(),
            editor: None,
            import: None,
            name: None,
            settings_sheet: None,
            note: None,
            occ: Rc::new(Vec::new()),
            opts,
            index,
            generation: 0,
            models: Default::default(),
            window: WindowKey::NONE,
            pending: None,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: BTreeSet::new(),
            writing: HashMap::new(),
            flush_seq: 0,
            persist_seq: 0,
            note_seq: 0,
            scroll,
            desktop_24h,
            instance: None,
            waiting: Vec::new(),
            new_waiting: false,
            args: None,
        };
        k.refresh();
        k
    }

    // Reading the state.

    pub fn h24(&self) -> bool {
        self.settings.uses_24h(self.desktop_24h)
    }

    pub fn weeks(&self) -> Weeks {
        self.settings.weeks(&self.locale)
    }

    fn shown(&self) -> Vec<bool> {
        self.store.calendars.iter().map(|c| !self.state.hidden.contains(&c.id)).collect()
    }

    /// The days the main view covers: `[first, last]`.
    pub fn range(&self) -> (Date, Date) {
        let w = self.weeks();
        let c = self.anchor;
        match self.view {
            View::Day => (c, c),
            View::Week => {
                let s = w.week_start(c);
                (s, crate::rrule::add_days(s, 6).unwrap_or(s))
            }
            View::Month => {
                let first = c.first_of_month();
                let s = w.week_start(first);
                let last = c.last_of_month();
                let e = crate::rrule::add_days(w.week_start(last), 6).unwrap_or(last);
                (s, e)
            }
            View::Year => (Date::new(c.year(), 1, 1).unwrap_or(c), Date::new(c.year(), 12, 31).unwrap_or(c)),
        }
    }

    /// The toolbar's title, and what's beside it.
    pub fn title(&self) -> (String, Option<String>) {
        let c = self.anchor;
        match self.view {
            View::Day => (format::long_date(c), None),
            View::Week => {
                let (a, z) = self.range();
                let t = if a.month() == z.month() {
                    format!("{} {}", format::month_name(a.month()), a.year())
                } else if a.year() == z.year() {
                    format!("{} – {} {}", format::month_short(a.month()), format::month_short(z.month()), z.year())
                } else {
                    format!(
                        "{} {} – {} {}",
                        format::month_short(a.month()),
                        a.year(),
                        format::month_short(z.month()),
                        z.year()
                    )
                };
                (t, None)
            }
            View::Month => (format!("{} {}", format::month_name(c.month()), c.year()), None),
            View::Year => (c.year().to_string(), None),
        }
    }

    pub fn default_calendar(&self) -> Option<String> {
        let shown = self.shown();
        self.settings
            .default_calendar
            .clone()
            .filter(|id| self.store.index_of(id).is_some())
            .or_else(|| self.store.calendars.iter().zip(&shown).find(|(_, s)| **s).map(|(c, _)| c.id.clone()))
            .or_else(|| self.store.calendars.first().map(|c| c.id.clone()))
    }

    fn occ_key(&self, key: u64) -> Option<&Occurrence> {
        self.occ.iter().find(|o| o.key(&self.store.calendars) == key)
    }

    /// The selected occurrence, its calendar and its event.
    pub fn selected_event(&self) -> Option<(&Occurrence, &Calendar, &Event)> {
        let r = self.selected.as_ref()?;
        let o = self.occ.iter().find(|o| r.is(o, &self.store.calendars))?;
        let cal = &self.store.calendars[o.cal];
        Some((o, cal, &cal.events[o.event]))
    }

    /// How the occurrence's series repeats, said.
    pub fn master_rule(&self, o: &Occurrence) -> Option<String> {
        let cal = &self.store.calendars[o.cal];
        let e = &cal.events[o.event];
        let m = cal.master(&e.uid).map(|i| &cal.events[i])?;
        let rule = m.rrule.as_ref()?;
        Some(match Repeat::from_rule(Some(rule), m.start.civil().date()) {
            Some(r) => r.describe(m.start.civil().date()),
            None => format!("Repeats: {}", rule.to_ical()),
        })
    }

    pub fn up_next(&self) -> Vec<(Dye, String, String)> {
        self.models.borrow().up_next.clone()
    }

    fn occ_for(&self, a: Date, z: Date) -> Vec<Occurrence> {
        let from = a.to_datetime(Time::midnight());
        let to = z.tomorrow().unwrap_or(z).to_datetime(Time::midnight());
        occur::occurrences(&self.store.calendars, &self.index, &self.shown(), &self.tz, from, to)
    }

    /// Bring the occurrences up to date with the range and the data.
    fn refresh(&mut self) {
        let (a, z) = self.range();
        self.occ = Rc::new(self.occ_for(a, z));
        self.models.borrow_mut().key = 0;
    }

    fn model_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            self.view,
            self.cursor,
            self.mini,
            self.generation,
            &self.state.hidden,
            self.today,
            self.now.hour(),
            self.now.minute(),
        )
            .hash(&mut h);
        (
            self.weeks(),
            self.h24(),
            self.settings.week_numbers,
            &self.selected.as_ref().map(|s| (&s.cal, &s.uid, s.rid.as_ref().map(|r| r.civil()))),
        )
            .hash(&mut h);
        self.scroll.hash(&mut h);
        Rc::as_ptr(&self.occ).hash(&mut h);
        h.finish()
    }

    /// Build what the views show, if it changed.
    fn models(&self) -> std::cell::RefMut<'_, Models> {
        let key = self.model_key();
        let mut m = self.models.borrow_mut();
        if m.key != key {
            *m = self.build_models();
            m.key = key;
        }
        m
    }

    fn chip_of(&self, o: &Occurrence) -> (u64, Arc<str>, Dye, bool, Arc<str>) {
        let cal = &self.store.calendars[o.cal];
        let e = &cal.events[o.event];
        let title: Arc<str> = if e.summary.trim().is_empty() { "New Event".into() } else { e.summary.as_str().into() };
        let selected = self.selected.as_ref().is_some_and(|s| s.is(o, &self.store.calendars));
        (o.key(&self.store.calendars), title, cal.color, selected, format::time(o.start.time(), self.h24()).into())
    }

    fn build_models(&self) -> Models {
        let mut m = Models::default();
        let w = self.weeks();
        let (a, z) = self.range();
        let weekend = |d: Date| matches!(d.weekday(), jiff::civil::Weekday::Saturday | jiff::civil::Weekday::Sunday);
        match self.view {
            View::Month => {
                let mut weeks: Vec<Vec<Date>> = Vec::new();
                let mut d = a;
                while d <= z {
                    weeks.push((0..7).filter_map(|i| crate::rrule::add_days(d, i)).collect());
                    d = crate::rrule::add_days(d, 7).unwrap_or(z.tomorrow().unwrap_or(z));
                }
                let cells = crate::ui::month::build(
                    &weeks,
                    &self.occ,
                    |o| self.chip_of(o),
                    self.anchor.month(),
                    self.today,
                    self.cursor,
                    weekend,
                );
                let names = w.days().iter().map(|d| format::weekday_short(*d).to_uppercase()).collect();
                m.month = Some(Rc::new(MonthData {
                    weeks: cells
                        .into_iter()
                        .zip(&weeks)
                        .map(|(c, days)| (w.number(days[0]), c.into_iter().map(Rc::new).collect()))
                        .collect(),
                    names,
                    week_numbers: self.settings.week_numbers,
                }));
            }
            View::Week | View::Day => {
                let days: Vec<Date> =
                    (0..=crate::model::days_between(a, z)).filter_map(|i| crate::rrule::add_days(a, i)).collect();
                let mut blocks = Vec::new();
                let mut bars = Vec::new();
                for o in self.occ.iter() {
                    let (key, title, dye, selected, _) = self.chip_of(o);
                    let e = &self.store.calendars[o.cal].events[o.event];
                    if o.spans_days() {
                        let first = days.iter().position(|d| *d >= o.first_day()).unwrap_or(0);
                        let last = days.iter().rposition(|d| *d <= o.last_day()).unwrap_or(days.len() - 1);
                        bars.push(Bar {
                            key,
                            first,
                            last,
                            lane: 0,
                            before: o.first_day() < a,
                            after: o.last_day() > z,
                            title,
                            dye,
                            selected,
                            cancelled: o.cancelled,
                            start: o.first_day(),
                            days: crate::model::days_between(o.first_day(), o.last_day()) + 1,
                        });
                        continue;
                    }
                    let Some(day) = days.iter().position(|d| *d == o.start.date()) else { continue };
                    let s = o.start.hour() as i32 * 60 + o.start.minute() as i32;
                    let e_min = if o.end.date() > o.start.date() {
                        1440
                    } else {
                        o.end.hour() as i32 * 60 + o.end.minute() as i32
                    };
                    blocks.push(Block {
                        key,
                        day,
                        start: s,
                        end: e_min.max(s),
                        col: 0,
                        cols: 1,
                        title,
                        time: format::time_range(o.start.time(), o.end.time(), self.h24()).into(),
                        location: e.location.lines().next().unwrap_or("").into(),
                        dye,
                        selected,
                        cancelled: o.cancelled,
                        real_start: o.start,
                        real_end: o.end,
                    });
                }
                week::pack(&mut blocks);
                // Bars in lanes.
                bars.sort_by(|x, y| x.first.cmp(&y.first).then(y.last.cmp(&x.last)));
                let mut lanes: Vec<Vec<bool>> = Vec::new();
                for b in bars.iter_mut() {
                    let lane = (0..)
                        .find(|l: &usize| lanes.get(*l).is_none_or(|row| (b.first..=b.last).all(|i| !row[i])))
                        .unwrap_or(0);
                    if lanes.len() <= lane {
                        lanes.resize(lane + 1, vec![false; days.len()]);
                    }
                    for slot in &mut lanes[lane][b.first..=b.last] {
                        *slot = true;
                    }
                    b.lane = lane;
                }
                let max_lanes = if self.view == View::Day { 6 } else { 4 };
                let total = lanes.len();
                let shown_lanes = if total > max_lanes { max_lanes } else { total.max(1) };
                let mut more = vec![0usize; days.len()];
                if total > max_lanes {
                    for b in bars.iter().filter(|b| b.lane >= max_lanes - 1) {
                        for slot in more.iter_mut().take(b.last + 1).skip(b.first) {
                            *slot += 1;
                        }
                    }
                    for b in bars.iter_mut() {
                        if b.lane >= max_lanes - 1 {
                            b.lane = usize::MAX;
                        }
                    }
                }
                let weekend_cols = days.iter().map(|d| weekend(*d)).collect();
                m.header = Some(HeaderData {
                    days: days.clone(),
                    today: self.today,
                    cursor: self.cursor,
                    week_number: self.settings.week_numbers.then(|| w.number(a)),
                });
                m.strip =
                    Some(Rc::new(StripData { days: days.clone(), bars, lanes: shown_lanes, more, today: self.today }));
                m.grid = Some(Rc::new(GridData {
                    days,
                    today: self.today,
                    now: self.now,
                    h24: self.h24(),
                    blocks,
                    weekend: weekend_cols,
                    scroll_to: self.scroll,
                }));
            }
            View::Year => {
                let y = self.anchor.year();
                let first = Date::new(y, 1, 1).unwrap_or(self.cursor);
                let last = Date::new(y, 12, 31).unwrap_or(self.cursor);
                let start = w.week_start(first);
                let end = crate::rrule::add_days(w.week_start(last), 13).unwrap_or(last);
                let busy = self.busy_days(start, end);
                let months = (1..=12)
                    .map(|mo| {
                        let mut mm = Mini {
                            year: y,
                            month: mo,
                            today: self.today,
                            cursor: self.cursor,
                            range: None,
                            first_day: w.first_day,
                            busy: 0,
                        };
                        mm.busy = bits(&busy, mm.first_shown());
                        mm
                    })
                    .collect();
                m.year = Some(YearData { months });
            }
        }
        // The mini-month, with the main view's days marked.
        let (my, mm) = self.mini;
        let mut mini = Mini {
            year: my,
            month: mm,
            today: self.today,
            cursor: self.cursor,
            range: matches!(self.view, View::Week | View::Day).then_some((a, z)),
            first_day: w.first_day,
            busy: 0,
        };
        let f = mini.first_shown();
        let busy = self.busy_days(f, crate::rrule::add_days(f, 41).unwrap_or(f));
        mini.busy = bits(&busy, f);
        m.mini = Some(mini);
        // Up next: the next few events from now.
        let soon = self.occ_for(self.today, crate::rrule::add_days(self.today, 6).unwrap_or(self.today));
        m.up_next = soon
            .iter()
            .filter(|o| !o.cancelled && o.end > self.now)
            .take(4)
            .map(|o| {
                let cal = &self.store.calendars[o.cal];
                let e = &cal.events[o.event];
                let day = if o.start.date() == self.today {
                    "Today".to_owned()
                } else if self.today.tomorrow().ok() == Some(o.start.date()) {
                    "Tomorrow".to_owned()
                } else {
                    format::short_date(o.start.date())
                };
                let when = if o.spans_days() {
                    format!("{day}, all day")
                } else {
                    format!("{day}, {}", format::time(o.start.time(), self.h24()))
                };
                (cal.color, when, if e.summary.is_empty() { "New Event".into() } else { e.summary.clone() })
            })
            .collect();
        m
    }

    /// Which days in `[a, b]` have events (shown calendars).
    fn busy_days(&self, a: Date, b: Date) -> Vec<Date> {
        let mut out: Vec<Date> = Vec::new();
        for o in self.occ_for(a, b) {
            let mut d = o.first_day().max(a);
            let last = o.last_day().min(b);
            while d <= last {
                out.push(d);
                d = d.tomorrow().unwrap_or(last.tomorrow().unwrap_or(last));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    pub fn month_model(&self) -> Rc<MonthData> {
        self.models()
            .month
            .clone()
            .unwrap_or_else(|| Rc::new(MonthData { weeks: Vec::new(), names: Vec::new(), week_numbers: false }))
    }

    pub fn week_models(&self) -> (HeaderData, Rc<StripData>, Rc<GridData>) {
        let m = self.models();
        (
            m.header.clone().expect("week models"),
            m.strip.clone().expect("week models"),
            m.grid.clone().expect("week models"),
        )
    }

    pub fn year_model(&self) -> YearData {
        self.models().year.clone().unwrap_or(YearData { months: Vec::new() })
    }

    pub fn mini_model(&self) -> Mini {
        self.models().mini.clone().expect("mini model")
    }

    // Changing the state.

    fn note(&mut self, s: impl Into<String>, cx: &mut Cx<Msg>) {
        self.note_seq += 1;
        self.note = Some((s.into(), self.note_seq));
        cx.after(Duration::from_secs(6), Msg::ClearNote(self.note_seq));
    }

    fn go(&mut self, d: Date) {
        self.cursor = d;
        self.anchor = d;
        self.mini = (d.year(), d.month());
        self.refresh();
    }

    fn step(&mut self, n: i64) {
        let c = self.anchor;
        let d = match self.view {
            View::Day => c.checked_add(n.days()),
            View::Week => c.checked_add((7 * n).days()),
            View::Month => c.checked_add(n.months()),
            View::Year => c.checked_add(n.years()),
        }
        .unwrap_or(c);
        self.go(d);
    }

    fn set_view(&mut self, v: View, cx: &mut Cx<Msg>) {
        if v != self.view {
            self.view = v;
            self.state.view = Some(v.key().into());
            self.refresh();
            self.persist_soon(cx);
        }
    }

    fn persist_soon(&mut self, cx: &mut Cx<Msg>) {
        if !self.opts.persist {
            return;
        }
        self.persist_seq += 1;
        cx.after(Duration::from_millis(600), Msg::Persist(self.persist_seq));
    }

    /// Calendar `id` changed: show it, and write it soon.
    fn changed(&mut self, id: &str, cx: &mut Cx<Msg>) {
        self.generation += 1;
        if let Some(ci) = self.store.index_of(id) {
            self.index.refresh(ci, &self.store.calendars[ci], &self.tz);
        } else {
            self.index = Index::new(&self.store.calendars, &self.tz);
        }
        self.dirty.insert(id.to_owned());
        self.refresh();
        if self.opts.background_saves {
            self.flush_seq += 1;
            cx.after(Duration::from_millis(300), Msg::Flush(self.flush_seq));
        } else {
            self.flush(cx);
        }
    }

    fn reindex(&mut self) {
        self.generation += 1;
        self.index = Index::new(&self.store.calendars, &self.tz);
        self.refresh();
    }

    /// Write the calendars that changed.
    fn flush(&mut self, cx: &mut Cx<Msg>) {
        for id in std::mem::take(&mut self.dirty) {
            let Some(i) = self.store.index_of(&id) else { continue };
            if self.opts.background_saves {
                let (path, cal) = self.store.snapshot(i);
                *self.writing.entry(id.clone()).or_default() += 1;
                cx.spawn(move || {
                    let r = path
                        .parent()
                        .map(std::fs::create_dir_all)
                        .transpose()
                        .and_then(|_| settings::write_atomic(&path, cal.to_ics().as_bytes()));
                    Msg::Saved(id, store::stamp(&path), r.err().map(|e| e.to_string()))
                });
            } else if let Err(e) = self.store.save(i) {
                self.note(format!("“{}” couldn't be saved: {e}", self.store.calendars[i].name), cx);
            }
        }
    }

    fn snapshot(&self, cal: &str, uid: &str) -> Change {
        let events = self
            .store
            .index_of(cal)
            .map(|i| self.store.calendars[i].events.iter().filter(|e| e.uid == uid).cloned().collect())
            .unwrap_or_default();
        Change::Events { cal: cal.to_owned(), uid: uid.to_owned(), events }
    }

    fn record(&mut self, label: &str, changes: Vec<Change>) {
        self.undo.push(Step { label: label.to_owned(), changes });
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Put back what `changes` say; what's there now comes back as the
    /// step to redo.
    fn apply(&mut self, changes: Vec<Change>, cx: &mut Cx<Msg>) -> Vec<Change> {
        let mut inverse = Vec::new();
        for c in changes.into_iter().rev() {
            match c {
                Change::Events { cal, uid, events } => {
                    inverse.push(self.snapshot(&cal, &uid));
                    if let Some(i) = self.store.index_of(&cal) {
                        let evs = &mut self.store.calendars[i].events;
                        // Keep the series where it was in the file.
                        let at = evs.iter().position(|e| e.uid == uid).unwrap_or(evs.len());
                        evs.retain(|e| e.uid != uid);
                        let at = at.min(evs.len());
                        for (n, e) in events.into_iter().enumerate() {
                            evs.insert(at + n, e);
                        }
                        self.changed(&cal, cx);
                    }
                }
                Change::Removed(calendar) => {
                    let id = calendar.id.clone();
                    match self.store.restore(calendar) {
                        Ok(_) => inverse.push(Change::Made(id)),
                        Err(e) => self.note(format!("The calendar couldn't be put back: {e}"), cx),
                    }
                    self.reindex();
                }
                Change::Made(id) => {
                    if let Some(i) = self.store.index_of(&id) {
                        match self.store.remove(i) {
                            Ok(cal) => inverse.push(Change::Removed(cal)),
                            Err(e) => self.note(format!("The calendar couldn't be removed: {e}"), cx),
                        }
                    }
                    self.reindex();
                }
                Change::Props { cal, name, color } => {
                    if let Some(i) = self.store.index_of(&cal) {
                        let c = &mut self.store.calendars[i];
                        inverse.push(Change::Props { cal: cal.clone(), name: c.name.clone(), color: c.color });
                        c.name = name;
                        c.color = color;
                        self.changed(&cal, cx);
                    }
                }
            }
        }
        inverse
    }

    fn select_key(&mut self, key: Option<u64>) {
        self.selected = key.and_then(|k| self.occ_key(k)).map(|o| OccRef::of(o, &self.store.calendars));
        if let Some(r) = &self.selected
            && !(self.range().0..=self.range().1).contains(&r.start.date())
        {
            // Selecting doesn't move the view.
        }
    }

    // Events.

    fn calendar_index(&self, id: &str) -> Option<usize> {
        self.store.index_of(id)
    }

    /// Move (or resize) an occurrence, asking first if it's in a series.
    fn reschedule(&mut self, r: OccRef, start: DateTime, end: DateTime, all_day: bool, cx: &mut Cx<Msg>) {
        let Some(ci) = self.calendar_index(&r.cal) else { return };
        if edit::in_series(&self.store.calendars[ci], &r) {
            self.pending = Some(Pending::Move { r, start, end, all_day });
            cx.alert(
                self.window,
                Alert::new(
                    "Change this event, or all of them?",
                    "It's one of a series. Changing all of them moves every event in it the same way.",
                )
                .default_button("Only This Event", Msg::Scope(Scope::This))
                .button("All Events", Msg::Scope(Scope::All))
                .cancel_with("Cancel", Msg::ScopeCancel),
            );
            return;
        }
        self.do_reschedule(r, start, end, all_day, Scope::This, cx);
    }

    fn do_reschedule(
        &mut self,
        r: OccRef,
        start: DateTime,
        end: DateTime,
        all_day: bool,
        scope: Scope,
        cx: &mut Cx<Msg>,
    ) {
        let Some(ci) = self.calendar_index(&r.cal) else { return };
        let before = self.snapshot(&r.cal, &r.uid);
        let tz = self.tz.clone();
        let here = self.here.clone();
        if edit::reschedule(&mut self.store.calendars[ci], &r, start, end, all_day, scope, &tz, &here) {
            let title = self.store.calendars[ci]
                .events
                .iter()
                .find(|e| e.uid == r.uid)
                .map(|e| e.summary.clone())
                .unwrap_or_default();
            self.record("Move Event", vec![before]);
            let cal = r.cal.clone();
            self.changed(&cal, cx);
            // The moved occurrence stays selected.
            let moved = OccRef { start, end, all_day, ..r };
            let found = self
                .occ
                .iter()
                .find(|o| moved.is(o, &self.store.calendars))
                .map(|o| OccRef::of(o, &self.store.calendars));
            self.selected = found.or(Some(moved));
            let when = format::span(start, end, all_day, self.h24());
            self.note(format!("Moved “{title}” to {when} · ❖Z to undo"), cx);
        }
    }

    fn delete(&mut self, r: OccRef, scope: Scope, cx: &mut Cx<Msg>) {
        let Some(ci) = self.calendar_index(&r.cal) else { return };
        let before = self.snapshot(&r.cal, &r.uid);
        let title = self.store.calendars[ci]
            .events
            .iter()
            .find(|e| e.uid == r.uid)
            .map(|e| e.summary.clone())
            .unwrap_or_default();
        if edit::delete(&mut self.store.calendars[ci], &r, scope) {
            self.record("Delete Event", vec![before]);
            let cal = r.cal.clone();
            self.selected = None;
            self.changed(&cal, cx);
            self.note(format!("Deleted “{title}” · ❖Z to undo"), cx);
        }
    }

    fn ask_delete(&mut self, r: OccRef, cx: &mut Cx<Msg>) {
        let Some(ci) = self.calendar_index(&r.cal) else { return };
        if edit::in_series(&self.store.calendars[ci], &r) {
            self.pending = Some(Pending::Delete(r));
            cx.alert(
                self.window,
                Alert::new(
                    "Delete this event, or more of the series?",
                    "It's one of a series. Deleted events can be had back with Undo.",
                )
                .destructive_button("Only This Event", Msg::Scope(Scope::This))
                .button("This and Later Events", Msg::Scope(Scope::Future))
                .button("All Events", Msg::Scope(Scope::All))
                .cancel_with("Cancel", Msg::ScopeCancel),
            );
        } else {
            self.delete(r, Scope::This, cx);
        }
    }

    /// Open the editor for a new event over `[start, end)`.
    fn new_event(&mut self, start: DateTime, end: DateTime, all_day: bool, cx: &mut Cx<Msg>) {
        let Some(cal) = self.default_calendar() else { return };
        let (s, e) = if all_day {
            (When::Date(start.date()), When::Date(end.date()))
        } else {
            (edit::local_when(start, &self.here), edit::local_when(end, &self.here))
        };
        let mut ev = Event::new("", s, e);
        let alert = if all_day { self.settings.default_all_day_alert } else { self.settings.default_alert };
        if let Some(m) = alert {
            ev.alarms.push(crate::model::Alarm::before(m));
        }
        let sheet = cx.show_sheet(self.window);
        self.editor = Some(Editor::new(sheet, None, false, cal, &ev, start, end, all_day, self.today, self.h24()));
        cx.focus("edit-title");
    }

    fn edit_selected(&mut self, cx: &mut Cx<Msg>) {
        let Some((o, cal, e)) = self.selected_event() else { return };
        let r = OccRef::of(o, &self.store.calendars);
        let series = edit::in_series(cal, &r);
        // A series is edited from its master's rule.
        let master = cal.master(&e.uid).map(|i| cal.events[i].clone());
        let mut shown = e.clone();
        if let Some(m) = &master {
            shown.rrule = m.rrule.clone();
        }
        let (start, end, all_day, id) = (o.start, o.end, o.all_day, cal.id.clone());
        let sheet = cx.show_sheet(self.window);
        self.editor =
            Some(Editor::new(sheet, Some(r), series, id, &shown, start, end, all_day, self.today, self.h24()));
        cx.focus("edit-title");
    }

    fn save_edit(&mut self, scope: Option<Scope>, cx: &mut Cx<Msg>) {
        let Some(ed) = self.editor.as_mut() else { return };
        let (start, end) = match ed.times() {
            Ok(t) => t,
            Err(e) => {
                ed.error = Some(e);
                return;
            }
        };
        let target = ed.target.clone();
        if target.is_some() && ed.series && scope.is_none() {
            self.pending = Some(Pending::Save);
            cx.alert(
                self.window,
                Alert::new("Save for this event, or all of them?", "It's one of a series.")
                    .default_button("Only This Event", Msg::Scope(Scope::This))
                    .button("All Events", Msg::Scope(Scope::All))
                    .cancel_with("Cancel", Msg::ScopeCancel),
            );
            return;
        }
        let Some(ed) = self.editor.take() else { return };
        cx.close_window(ed.sheet);
        let tz = self.tz.clone();
        let here = self.here.clone();
        let title = if ed.title.trim().is_empty() { "New Event".to_owned() } else { ed.title.trim().to_owned() };
        match target {
            None => {
                let Some(ci) = self.calendar_index(&ed.calendar) else { return };
                let (s, e) = if ed.all_day {
                    (When::Date(start.date()), When::Date(end.date()))
                } else {
                    (edit::local_when(start, &here), edit::local_when(end, &here))
                };
                let mut ev = Event::new(title.clone(), s, e);
                ev.location = ed.location.trim().to_owned();
                ev.description = ed.notes.text().trim_end().to_owned();
                ed.apply_alert(&mut ev.alarms);
                if let Some(r) = &ed.repeat {
                    ev.rrule = r.to_rule(&ev.start, &tz);
                }
                let uid = ev.uid.clone();
                let before = self.snapshot(&ed.calendar, &uid);
                self.store.calendars[ci].events.push(ev);
                self.record("New Event", vec![before]);
                self.changed(&ed.calendar, cx);
                let r = OccRef { cal: ed.calendar.clone(), uid, rid: None, start, end, all_day: ed.all_day };
                self.selected = Some(r);
                if !(self.range().0..=self.range().1).contains(&start.date()) {
                    self.go(start.date());
                }
                self.note(format!("Added “{title}”"), cx);
            }
            Some(r) => {
                let Some(ci) = self.calendar_index(&r.cal) else { return };
                let mut changes = vec![self.snapshot(&r.cal, &r.uid)];
                let scope = scope.unwrap_or(Scope::This);
                let times_changed = start != r.start || end != r.end || ed.all_day != r.all_day;
                let cal = &mut self.store.calendars[ci];
                if times_changed {
                    edit::reschedule(cal, &r, start, end, ed.all_day, scope, &tz, &here);
                }
                // The event the fields go in: the instance's override, or
                // the master (or the event itself).
                let instance = cal.events.iter().position(|e| {
                    e.uid == r.uid
                        && e.recurrence_id.as_ref().zip(r.rid.as_ref()).is_some_and(|(a, b)| a.civil() == b.civil())
                });
                let target_i = match (scope, r.rid.is_some()) {
                    (Scope::This, true) => {
                        if instance.is_none() {
                            // Make the override, unmoved.
                            edit::reschedule(cal, &r, r.start, r.end, r.all_day, Scope::This, &tz, &here);
                        }
                        cal.events.iter().position(|e| {
                            e.uid == r.uid
                                && e.recurrence_id
                                    .as_ref()
                                    .zip(r.rid.as_ref())
                                    .is_some_and(|(a, b)| a.civil() == b.civil())
                        })
                    }
                    _ => cal.master(&r.uid).or(instance),
                };
                if let Some(i) = target_i {
                    let e = &mut cal.events[i];
                    e.summary = title.clone();
                    e.location = ed.location.trim().to_owned();
                    e.description = ed.notes.text().trim_end().to_owned();
                    ed.apply_alert(&mut e.alarms);
                    if e.recurrence_id.is_none()
                        && let Some(rep) = &ed.repeat
                    {
                        let rule = rep.to_rule(&e.start, &tz);
                        let had = e.rrule.clone();
                        if rule != had && Repeat::from_rule(had.as_ref(), e.start.civil().date()).as_ref() != Some(rep)
                        {
                            e.rrule = rule;
                        }
                    }
                    e.touch();
                }
                let mut touched = vec![r.cal.clone()];
                // Another calendar: the whole series goes there.
                if ed.calendar != r.cal
                    && let Some(cj) = self.calendar_index(&ed.calendar)
                {
                    changes.push(self.snapshot(&ed.calendar, &r.uid));
                    let moving: Vec<Event> =
                        self.store.calendars[ci].events.iter().filter(|e| e.uid == r.uid).cloned().collect();
                    self.store.calendars[ci].events.retain(|e| e.uid != r.uid);
                    self.store.calendars[cj].events.extend(moving);
                    touched.push(ed.calendar.clone());
                }
                self.record("Edit Event", changes);
                for c in touched {
                    self.changed(&c, cx);
                }
                let moved = OccRef { cal: ed.calendar.clone(), start, end, all_day: ed.all_day, ..r };
                self.selected = self
                    .occ
                    .iter()
                    .find(|o| moved.is(o, &self.store.calendars))
                    .map(|o| OccRef::of(o, &self.store.calendars))
                    .or(Some(moved));
                self.note(format!("Saved “{title}”"), cx);
            }
        }
        cx.focus("calendar");
    }

    fn duplicate(&mut self, cx: &mut Cx<Msg>) {
        let Some((_, cal, e)) = self.selected_event() else { return };
        let id = cal.id.clone();
        let mut copy = cal.master(&e.uid).map(|i| cal.events[i].clone()).unwrap_or_else(|| e.clone());
        copy.uid = crate::model::new_uid();
        copy.recurrence_id = None;
        copy.touch();
        copy.sequence = 0;
        let uid = copy.uid.clone();
        let before = self.snapshot(&id, &uid);
        if let Some(ci) = self.calendar_index(&id) {
            self.store.calendars[ci].events.push(copy);
            self.record("Duplicate Event", vec![before]);
            self.changed(&id, cx);
            self.note("Duplicated", cx);
        }
    }

    // Calendars.

    fn open_import(&mut self, files: Vec<PathBuf>, into: Option<String>, cx: &mut Cx<Msg>) {
        let mut parsed = Calendar::new("import", "", Dye::Hanada);
        let mut skipped = 0;
        let mut names = Vec::new();
        for f in &files {
            match std::fs::read(f) {
                Ok(b) => {
                    let (c, s) = Calendar::parse("import", &String::from_utf8_lossy(&b));
                    skipped += s;
                    if !c.name.is_empty() && c.name != "import" {
                        names.push(c.name.clone());
                    }
                    if parsed.name.is_empty() {
                        parsed.color = c.color;
                    }
                    store::merge(&mut parsed, &c);
                }
                Err(e) => {
                    self.note(format!("{}: {e}", f.display()), cx);
                    return;
                }
            }
        }
        parsed.name = names.first().cloned().unwrap_or_else(|| {
            files
                .first()
                .and_then(|f| f.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Imported".into())
        });
        if parsed.events.is_empty() {
            self.note("There are no events in that file.", cx);
            return;
        }
        let target = into
            .and_then(|id| self.store.index_of(&id))
            .or_else(|| self.default_calendar().and_then(|id| self.store.index_of(&id)))
            .unwrap_or(0);
        let sheet = cx.show_sheet(self.window);
        self.import = Some(ImportSheet { sheet, files, parsed, skipped, target });
    }

    fn finish_import(&mut self, cx: &mut Cx<Msg>) {
        let Some(imp) = self.import.take() else { return };
        cx.close_window(imp.sheet);
        let new = imp.target >= self.store.calendars.len();
        let id = if new {
            match self.store.create(&imp.parsed.name, imp.parsed.color) {
                Ok(i) => {
                    let id = self.store.calendars[i].id.clone();
                    self.record("Import", vec![Change::Made(id.clone())]);
                    self.reindex();
                    id
                }
                Err(e) => {
                    self.note(format!("The calendar couldn't be made: {e}"), cx);
                    return;
                }
            }
        } else {
            self.store.calendars[imp.target].id.clone()
        };
        let Some(ci) = self.store.index_of(&id) else { return };
        let mut changes: Vec<Change> = Vec::new();
        if !new {
            let uids: BTreeSet<String> = imp.parsed.events.iter().map(|e| e.uid.clone()).collect();
            for u in uids {
                changes.push(self.snapshot(&id, &u));
            }
        }
        let r = store::merge(&mut self.store.calendars[ci], &imp.parsed);
        if !new {
            self.record("Import", changes);
        }
        self.changed(&id, cx);
        let name = self.store.calendars[ci].name.clone();
        let mut s = format!("Imported into “{name}”: {} added", r.added);
        if r.updated > 0 {
            s.push_str(&format!(", {} updated", r.updated));
        }
        if r.unchanged > 0 {
            s.push_str(&format!(", {} already there", r.unchanged));
        }
        if imp.skipped > 0 {
            s.push_str(&format!(", {} without a date skipped", imp.skipped));
        }
        self.note(s, cx);
        // Show where the events are.
        if let Some(first) = imp
            .parsed
            .events
            .iter()
            .map(|e| e.start.instant(&self.tz))
            .filter(|t| t.to_zoned(self.tz.clone()).date() >= self.today)
            .min()
        {
            self.go(first.to_zoned(self.tz.clone()).date());
        }
    }

    /// Take arguments (from the command line or a later launch): files
    /// to import, `--show DATE`, a bare date.
    fn take_args(&mut self, args: Vec<String>, cx: &mut Cx<Msg>) {
        let mut files = Vec::new();
        let mut it = args.into_iter().peekable();
        while let Some(a) = it.next() {
            if a == "--new" {
                if self.window == WindowKey::NONE {
                    self.new_waiting = true;
                } else {
                    self.update(Msg::NewEvent, cx);
                }
                continue;
            }
            if a == "--show" {
                let today = self.today;
                if let Some(d) = it.next().and_then(|d| format::parse_date(&d, today)) {
                    self.go(d);
                    if self.view == View::Year || self.view == View::Month {
                        self.set_view(View::Day, cx);
                    }
                }
                continue;
            }
            if a.starts_with('-') {
                continue;
            }
            let p = PathBuf::from(&a.strip_prefix("file://").unwrap_or(&a));
            if p.is_file() {
                files.push(p);
            } else if let Ok(d) = a.parse::<Date>() {
                self.go(d);
            }
        }
        if !files.is_empty() {
            if self.window == WindowKey::NONE {
                self.waiting.extend(files);
            } else {
                self.open_import(files, None, cx);
            }
        }
        cx.activate(self.window);
    }

    /// Now: the clock, or the fixed time tests give.
    fn clock(&self) -> Timestamp {
        self.opts.now.unwrap_or_else(Timestamp::now)
    }
}

/// The days of `busy` among the 42 a small month shows from `first`, as bits.
fn bits(busy: &[Date], first: Date) -> u64 {
    let mut b = 0u64;
    for i in 0..42 {
        if let Some(d) = crate::rrule::add_days(first, i)
            && busy.binary_search(&d).is_ok()
        {
            b |= 1 << i;
        }
    }
    b
}

impl Program for Koyomi {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Koyomi", "Calendar").icon(Icon::CalendarDays).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let (w, h) = self.state.window.unwrap_or((1120.0, 740.0));
        self.window = cx.open_window(WindowSpec::new("Koyomi").size(w.max(760.0), h.max(520.0)).min_size(720.0, 480.0));
        cx.focus("calendar");
        cx.every(Duration::from_secs(20), Msg::Tick);
        cx.every(Duration::from_secs(3), Msg::Poll);
        if let Some(l) = self.instance.take() {
            crate::instance::serve(l, cx.stream(), Msg::Instance);
        }
        if let Some(args) = self.args.clone() {
            self.take_args(args, cx);
        }
        let waiting = std::mem::take(&mut self.waiting);
        if !waiting.is_empty() {
            self.open_import(waiting, None, cx);
        } else if std::mem::take(&mut self.new_waiting) {
            self.update(Msg::NewEvent, cx);
        }
    }

    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        // What the command line gave is taken whole (flags too) after the
        // window opens; the kit's copy (paths only) is for programs run
        // without `main`.
        if self.args.is_some() {
            return;
        }
        let args = paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
        self.take_args(args, cx);
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::SetView(v) => self.set_view(v, cx),
            Msg::Prev => self.step(-1),
            Msg::Next => self.step(1),
            Msg::Today => {
                let t = self.today;
                self.go(t);
                self.scroll = (self.scroll.0 + 1, ((self.now.hour() as i32 - 2).max(0)) * 60);
            }
            Msg::GoTo(d) => {
                if matches!(self.view, View::Week | View::Day) && self.cursor == d && self.view == View::Week {
                    self.set_view(View::Day, cx);
                }
                self.go(d);
            }
            Msg::Mini(MiniAction::Pick(d)) => self.go(d),
            Msg::Mini(MiniAction::Browse(n)) => {
                let (y, m) = self.mini;
                let total = y as i32 * 12 + (m as i32 - 1) + n;
                self.mini = (total.div_euclid(12) as i16, (total.rem_euclid(12) + 1) as i8);
            }
            Msg::Grid(a) => match a {
                GridAction::Select(k) => self.select_key(k),
                GridAction::Open(k) => {
                    self.select_key(Some(k));
                    self.edit_selected(cx);
                }
                GridAction::Day(d) => {
                    self.selected = None;
                    if d != self.cursor {
                        let (a, z) = self.range();
                        if d < a || d > z {
                            self.go(d);
                        } else {
                            self.cursor = d;
                        }
                    }
                }
                GridAction::Create { start, end, all_day } => self.new_event(start, end, all_day, cx),
                GridAction::Move { key, start, end, all_day } => {
                    if let Some(o) = self.occ_key(key) {
                        let r = OccRef::of(o, &self.store.calendars);
                        self.reschedule(r, start, end, all_day, cx);
                    }
                }
                GridAction::Resize { key, end } => {
                    if let Some(o) = self.occ_key(key) {
                        let r = OccRef::of(o, &self.store.calendars);
                        let (s, a) = (r.start, r.all_day);
                        self.reschedule(r, s, end, a, cx);
                    }
                }
                GridAction::ShowDay(d) => {
                    self.go(d);
                    self.set_view(View::Day, cx);
                }
                GridAction::ShowWeek(d) => {
                    self.go(d);
                    self.set_view(View::Week, cx);
                }
                GridAction::ShowMonth(d) => {
                    self.go(d);
                    self.set_view(View::Month, cx);
                }
            },
            Msg::Key(k) => match k {
                KeyAction::Prev => self.step(-1),
                KeyAction::Next => self.step(1),
                KeyAction::Today => self.update(Msg::Today, cx),
                KeyAction::Delete => self.update(Msg::Delete, cx),
                KeyAction::Open => self.edit_selected(cx),
                KeyAction::Deselect => self.selected = None,
            },
            Msg::ToggleCalendar(id) => {
                if let Some(i) = self.state.hidden.iter().position(|h| *h == id) {
                    self.state.hidden.remove(i);
                } else {
                    self.state.hidden.push(id);
                }
                self.generation += 1;
                self.refresh();
                self.persist_soon(cx);
            }
            Msg::Solo(id) => {
                self.state.hidden = self.store.calendars.iter().map(|c| c.id.clone()).filter(|c| *c != id).collect();
                self.generation += 1;
                self.refresh();
                self.persist_soon(cx);
            }
            Msg::SetColor(id, d) => {
                if let Some(i) = self.store.index_of(&id) {
                    let c = &self.store.calendars[i];
                    let before = Change::Props { cal: id.clone(), name: c.name.clone(), color: c.color };
                    self.store.calendars[i].color = d;
                    self.record("Change Colour", vec![before]);
                    self.changed(&id, cx);
                }
            }
            Msg::MakeDefault(id) => {
                self.settings.default_calendar = Some(id);
                if self.opts.persist {
                    let _ = self.settings.save();
                }
            }
            Msg::NewCalendar => {
                let sheet = cx.show_sheet(self.window);
                self.name = Some(NameSheet { sheet, cal: None, text: String::new() });
                cx.focus("name-field");
            }
            Msg::RenameCalendar(id) => {
                let Some(i) = self.store.index_of(&id) else { return };
                let sheet = cx.show_sheet(self.window);
                self.name = Some(NameSheet { sheet, cal: Some(id), text: self.store.calendars[i].name.clone() });
                cx.focus("name-field");
            }
            Msg::NameText(s) => {
                if let Some(n) = self.name.as_mut() {
                    n.text = s;
                }
            }
            Msg::NameDone => {
                let Some(n) = self.name.take() else { return };
                let name = n.text.trim().to_owned();
                cx.close_window(n.sheet);
                if name.is_empty() {
                    return;
                }
                match n.cal {
                    None => {
                        // The next dye not in use.
                        let used: Vec<Dye> = self.store.calendars.iter().map(|c| c.color).collect();
                        let dye = Dye::ALL.into_iter().find(|d| !used.contains(d)).unwrap_or(Dye::Hanada);
                        match self.store.create(&name, dye) {
                            Ok(i) => {
                                let id = self.store.calendars[i].id.clone();
                                self.record("New Calendar", vec![Change::Made(id)]);
                                self.reindex();
                                self.note(format!("Made “{name}”"), cx);
                            }
                            Err(e) => self.note(format!("The calendar couldn't be made: {e}"), cx),
                        }
                    }
                    Some(id) => {
                        if let Some(i) = self.store.index_of(&id) {
                            let c = &self.store.calendars[i];
                            let before = Change::Props { cal: id.clone(), name: c.name.clone(), color: c.color };
                            self.store.calendars[i].name = name;
                            self.record("Rename Calendar", vec![before]);
                            self.changed(&id, cx);
                        }
                    }
                }
                cx.focus("calendar");
            }
            Msg::NameCancel => {
                if let Some(n) = self.name.take() {
                    cx.close_window(n.sheet);
                }
            }
            Msg::DeleteCalendar(id) => {
                let Some(i) = self.store.index_of(&id) else { return };
                let c = &self.store.calendars[i];
                let n = c.events.len();
                cx.alert(
                    self.window,
                    Alert::new(
                        format!("Delete “{}”?", c.name),
                        format!("Its {n} event{} go with it. Undo brings it back; its file is kept in the calendars’ .trash folder.", if n == 1 { "" } else { "s" }),
                    )
                    .destructive_button("Delete Calendar", Msg::DeleteCalendarConfirmed(id))
                    .cancel_button("Cancel"),
                );
            }
            Msg::DeleteCalendarConfirmed(id) => {
                let Some(i) = self.store.index_of(&id) else { return };
                match self.store.remove(i) {
                    Ok(cal) => {
                        let name = cal.name.clone();
                        self.record("Delete Calendar", vec![Change::Removed(cal)]);
                        self.selected = None;
                        self.reindex();
                        self.note(format!("Deleted “{name}” · ❖Z to undo"), cx);
                    }
                    Err(e) => self.note(format!("The calendar couldn't be deleted: {e}"), cx),
                }
            }
            Msg::Export(id) => {
                let Some(i) = self.store.index_of(&id) else { return };
                let name = format!("{}.ics", self.store.calendars[i].name);
                let dialog = FileDialog::new().title("Export Calendar").file_name(name).filter("iCalendar", &["ics"]);
                cx.save_panel(self.window, dialog, move |p| Msg::ExportTo(id.clone(), p));
            }
            Msg::ExportTo(id, Some(path)) => {
                let Some(i) = self.store.index_of(&id) else { return };
                match settings::write_atomic(&path, self.store.calendars[i].to_ics().as_bytes()) {
                    Ok(()) => self.note(format!("Exported to {}", path.display()), cx),
                    Err(e) => cx.alert(
                        self.window,
                        Alert::new("The calendar couldn't be exported", format!("{}: {e}", path.display())),
                    ),
                }
            }
            Msg::ExportTo(_, None) => {}
            Msg::Import => {
                let dialog = FileDialog::new()
                    .title("Import Events")
                    .filter("iCalendar", &["ics", "ical", "ifb", "icalendar"])
                    .multiple();
                cx.open_panel(self.window, dialog, Msg::ImportFiles);
            }
            Msg::ImportInto(id) => {
                self.settings.default_calendar.get_or_insert(id.clone());
                let dialog = FileDialog::new()
                    .title(format!(
                        "Import Events into “{}”",
                        self.store.index_of(&id).map(|i| self.store.calendars[i].name.clone()).unwrap_or_default()
                    ))
                    .filter("iCalendar", &["ics", "ical"])
                    .multiple();
                cx.open_panel(self.window, dialog, move |p| {
                    Msg::ImportFiles(p.map(|mut v| {
                        v.push(PathBuf::from(format!("koyomi-into:{id}")));
                        v
                    }))
                });
            }
            Msg::ImportFiles(Some(mut files)) => {
                let into = files
                    .iter()
                    .position(|f| f.to_string_lossy().starts_with("koyomi-into:"))
                    .map(|i| files.remove(i).to_string_lossy().trim_start_matches("koyomi-into:").to_owned());
                if !files.is_empty() {
                    self.open_import(files, into, cx);
                }
            }
            Msg::ImportFiles(None) => {}
            Msg::ImportTarget(i) => {
                if let Some(imp) = self.import.as_mut() {
                    imp.target = i;
                }
            }
            Msg::ImportDone => self.finish_import(cx),
            Msg::ImportCancel => {
                if let Some(imp) = self.import.take() {
                    cx.close_window(imp.sheet);
                }
            }
            Msg::NewEvent => {
                // The next whole hour on the day in view (or 09:00 on another day).
                let d = self.cursor;
                let start = if d == self.today {
                    let h = (self.now.hour() + 1).min(23);
                    d.to_datetime(Time::constant(h, 0, 0, 0))
                } else {
                    d.to_datetime(Time::constant(9, 0, 0, 0))
                };
                let end = start.checked_add(jiff::SignedDuration::from_hours(1)).unwrap_or(start);
                self.new_event(start, end, false, cx);
            }
            Msg::Edit => self.edit_selected(cx),
            Msg::EditField(f) => {
                let ids: Vec<String> = self.store.calendars.iter().map(|c| c.id.clone()).collect();
                if let Some(ed) = self.editor.as_mut() {
                    ed.update(f, &ids);
                }
            }
            Msg::EditSave => self.save_edit(None, cx),
            Msg::EditCancel => {
                if let Some(ed) = self.editor.take() {
                    cx.close_window(ed.sheet);
                }
                cx.focus("calendar");
            }
            Msg::EditDelete => {
                if let Some(ed) = self.editor.take() {
                    cx.close_window(ed.sheet);
                    if let Some(r) = ed.target {
                        self.ask_delete(r, cx);
                    }
                }
            }
            Msg::Delete => {
                if let Some(r) = self.selected.clone() {
                    self.ask_delete(r, cx);
                }
            }
            Msg::Scope(s) => match self.pending.take() {
                Some(Pending::Move { r, start, end, all_day }) => self.do_reschedule(r, start, end, all_day, s, cx),
                Some(Pending::Delete(r)) => self.delete(r, s, cx),
                Some(Pending::Save) => self.save_edit(Some(if s == Scope::Future { Scope::All } else { s }), cx),
                None => {}
            },
            Msg::ScopeCancel => self.pending = None,
            Msg::Duplicate => self.duplicate(cx),
            Msg::Undo => {
                if let Some(step) = self.undo.pop() {
                    let label = step.label.clone();
                    let inverse = self.apply(step.changes, cx);
                    self.redo.push(Step { label: label.clone(), changes: inverse });
                    self.selected = None;
                    self.note(format!("Undid {}", label.to_lowercase()), cx);
                }
            }
            Msg::Redo => {
                if let Some(step) = self.redo.pop() {
                    let label = step.label.clone();
                    let inverse = self.apply(step.changes, cx);
                    self.undo.push(Step { label: label.clone(), changes: inverse });
                    self.note(format!("Redid {}", label.to_lowercase()), cx);
                }
            }
            Msg::ShowSettings => {
                if self.settings_sheet.is_none() {
                    self.settings_sheet = Some(cx.show_sheet(self.window));
                }
            }
            Msg::SettingsDone => {
                if let Some(s) = self.settings_sheet.take() {
                    cx.close_window(s);
                }
                if self.opts.persist {
                    let _ = self.settings.save();
                }
            }
            Msg::FirstDay(f) => {
                self.settings.first_day = f;
                self.refresh();
            }
            Msg::WeekNumbers(on) => self.settings.week_numbers = on,
            Msg::ClockFormat(c) => self.settings.clock = c,
            Msg::DefaultAlert(i) => {
                self.settings.default_alert =
                    if i == 0 { None } else { crate::ui::editor::TIMED_ALERTS.get(i - 1).map(|(m, _)| *m) };
            }
            Msg::DefaultAllDayAlert(i) => {
                self.settings.default_all_day_alert =
                    if i == 0 { None } else { crate::ui::editor::ALL_DAY_ALERTS.get(i - 1).map(|(m, _)| *m) };
            }
            Msg::Snooze(v) => self.settings.snooze_minutes = v.round().clamp(1.0, 120.0) as i64,
            Msg::DayStarts(v) => {
                self.settings.day_starts = v.round().clamp(0.0, 20.0) as u8;
                self.scroll = (self.scroll.0 + 1, self.settings.day_starts as i32 * 60 - 20);
            }
            Msg::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Msg::SidebarWidth(w) => {
                self.state.sidebar = Some(w.round());
                self.persist_soon(cx);
            }
            Msg::Tick => {
                let now = self.clock().to_zoned(self.tz.clone()).datetime();
                if now.date() != self.today {
                    // A new day: today moves (the view stays put).
                    self.today = now.date();
                    self.refresh();
                }
                self.now = now;
            }
            Msg::Poll => {
                if self.dirty.is_empty() && self.writing.values().all(|n| *n == 0) && self.store.changed_on_disk() {
                    let sel = self.selected.clone();
                    self.store.reload();
                    self.reindex();
                    self.selected = sel.filter(|s| self.occ.iter().any(|o| s.is(o, &self.store.calendars)));
                    // The undo history refers to what was there before.
                    self.undo.clear();
                    self.redo.clear();
                }
            }
            Msg::Flush(seq) => {
                if seq == self.flush_seq {
                    self.flush(cx);
                }
            }
            Msg::Saved(id, stamp, err) => {
                if let Some(n) = self.writing.get_mut(&id) {
                    *n = n.saturating_sub(1);
                }
                if self.writing.get(&id).copied().unwrap_or(0) == 0 {
                    self.store.saved(&id, stamp);
                }
                if let Some(e) = err {
                    self.note(format!("“{id}” couldn't be saved: {e}"), cx);
                }
            }
            Msg::Persist(seq) => {
                if seq == self.persist_seq && self.opts.persist {
                    let _ = self.state.save();
                }
            }
            Msg::ClearNote(seq) => {
                if self.note.as_ref().is_some_and(|(_, s)| *s == seq) {
                    self.note = None;
                }
            }
            Msg::Instance(args) => self.take_args(args, cx),
            Msg::About | Msg::Nothing => {}
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if let Some(ed) = self.editor.as_ref().filter(|e| e.sheet == window) {
            let cals: Vec<(String, String)> =
                self.store.calendars.iter().map(|c| (c.id.clone(), c.name.clone())).collect();
            return ed.view(
                &cals,
                Msg::EditField,
                Msg::EditSave,
                Msg::EditCancel,
                ed.target.as_ref().map(|_| Msg::EditDelete),
            );
        }
        if let Some(n) = self.name.as_ref().filter(|n| n.sheet == window) {
            let (title, ok) =
                if n.cal.is_some() { ("Rename Calendar", "Rename") } else { ("New Calendar", "Make Calendar") };
            return ui::name_sheet(title, &n.text, ok, Msg::NameText, Msg::NameDone, Msg::NameCancel);
        }
        if let Some(imp) = self.import.as_ref().filter(|i| i.sheet == window) {
            return crate::ui::sheets::import(self, imp);
        }
        if self.settings_sheet == Some(window) {
            return crate::ui::sheets::settings(self);
        }
        ui::view(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        crate::ui::sheets::menus(self)
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if window == self.window {
            self.flush(cx);
            if self.opts.persist {
                let _ = self.state.save();
            }
        }
        if self.editor.as_ref().is_some_and(|e| e.sheet == window) {
            self.editor = None;
        }
        if self.name.as_ref().is_some_and(|n| n.sheet == window) {
            self.name = None;
        }
        if self.import.as_ref().is_some_and(|n| n.sheet == window) {
            self.import = None;
        }
        if self.settings_sheet == Some(window) {
            self.settings_sheet = None;
        }
        true
    }
}

/// Run the calendar (one instance: a second launch hands its arguments
/// over).
pub fn main(args: Vec<String>) {
    let mut app = Koyomi::new(Options::real());
    app.args = Some(args.clone());
    match crate::instance::claim(&args) {
        crate::instance::Claim::Handed => return,
        crate::instance::Claim::First(l) => app.instance = Some(l),
        crate::instance::Claim::Alone => {}
    }
    aizome_kit::run(app);
}
