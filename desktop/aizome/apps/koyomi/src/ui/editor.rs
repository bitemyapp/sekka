//! The event editor, as a sheet: title, location, all-day, start and
//! end, repeat, alert, calendar and notes.
//!
//! It edits a draft; Save turns the draft into the event (asking, for an
//! event of a series, whether the change is for this one or all).

use aizome_kit::prelude::*;
use aizome_kit::{EditorAction, EditorContent, column, row, text_editor};
use jiff::civil::{Date, DateTime, Time, Weekday};

use crate::edit::{OccRef, Repeat, RepeatEnd};
use crate::format;
use crate::model::{Alarm, Event, Trigger};
use crate::rrule::Freq;

/// Alerts offered for timed events: minutes before.
pub const TIMED_ALERTS: &[(i64, &str)] = &[
    (0, "At time of event"),
    (5, "5 minutes before"),
    (10, "10 minutes before"),
    (15, "15 minutes before"),
    (30, "30 minutes before"),
    (60, "1 hour before"),
    (120, "2 hours before"),
    (1440, "1 day before"),
    (2880, "2 days before"),
    (10080, "1 week before"),
];

/// … and for all-day events: minutes before the day's midnight.
pub const ALL_DAY_ALERTS: &[(i64, &str)] = &[
    (-540, "On the day, at 09:00"),
    (900, "The day before, at 09:00"),
    (2340, "2 days before, at 09:00"),
    (9540, "1 week before, at 09:00"),
];

/// The alert a draft has.
#[derive(Clone, Debug, PartialEq)]
pub enum AlertChoice {
    None,
    Before(i64),
    /// One the editor doesn't offer, kept as it was.
    Kept,
}

/// What the editor holds while it's open.
pub struct Editor {
    pub sheet: WindowKey,
    /// The occurrence edited (none: a new event).
    pub target: Option<OccRef>,
    /// Whether it belongs to a series.
    pub series: bool,
    pub calendar: String,
    pub title: String,
    pub location: String,
    pub notes: EditorContent,
    pub all_day: bool,
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    /// None: a rule the editor can't show (kept unless changed).
    pub repeat: Option<Repeat>,
    pub custom_repeat: String,
    pub until_text: String,
    pub alert: AlertChoice,
    /// The length kept when the start moves (minutes, or days for all-day).
    length: i64,
    pub error: Option<String>,
    /// Whether anything was changed.
    pub changed: bool,
    today: Date,
    h24: bool,
}

#[derive(Clone, Debug)]
pub enum Field {
    Title(String),
    Location(String),
    Notes(EditorAction),
    AllDay(bool),
    StartDate(String),
    StartTime(String),
    EndDate(String),
    EndTime(String),
    Freq(usize),
    Interval(f64),
    Weekday(Weekday),
    ByWeekday(bool),
    EndKind(usize),
    Count(f64),
    Until(String),
    Alert(usize),
    Calendar(usize),
}

fn dt_text(d: DateTime, h24: bool) -> (String, String) {
    (format::short_date(d.date()).to_string() + &format!(" {}", d.year()), format::time(d.time(), h24))
}

impl Editor {
    /// A draft of `e` (shown at `start`–`end`, all day or not).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sheet: WindowKey,
        target: Option<OccRef>,
        series: bool,
        calendar: String,
        e: &Event,
        start: DateTime,
        end: DateTime,
        all_day: bool,
        today: Date,
        h24: bool,
    ) -> Editor {
        let (sd, st) = dt_text(start, h24);
        // An all-day event's end shows as its last day.
        let shown_end = if all_day && end.time() == Time::midnight() && end > start {
            end.date().yesterday().unwrap_or(end.date()).to_datetime(Time::midnight())
        } else {
            end
        };
        let (ed, et) = dt_text(shown_end, h24);
        let repeat = Repeat::from_rule(e.rrule.as_ref(), e.start.civil().date());
        let alert = match e.alarms.iter().find(|a| a.is_active() && matches!(a.action.as_str(), "DISPLAY" | "AUDIO")) {
            None => AlertChoice::None,
            Some(a) => match a.minutes_before() {
                Some(m) if (if all_day { ALL_DAY_ALERTS } else { TIMED_ALERTS }).iter().any(|(x, _)| *x == m) => {
                    AlertChoice::Before(m)
                }
                _ => AlertChoice::Kept,
            },
        };
        let until_text = match repeat.as_ref().map(|r| &r.end) {
            Some(RepeatEnd::On(d)) => format!("{} {}", format::short_date(*d), d.year()),
            _ => String::new(),
        };
        Editor {
            sheet,
            target,
            series,
            calendar,
            title: e.summary.clone(),
            location: e.location.clone(),
            notes: EditorContent::new(&e.description),
            all_day,
            start_date: sd,
            start_time: st,
            end_date: ed,
            end_time: et,
            custom_repeat: e.rrule.as_ref().map(|r| r.to_ical()).unwrap_or_default(),
            repeat,
            until_text,
            alert,
            length: if all_day {
                crate::model::days_between(start.date(), end.date()).max(1)
            } else {
                end.duration_since(start).as_mins().max(0)
            },
            error: None,
            changed: false,
            today,
            h24,
        }
    }

    fn date(&self, s: &str) -> Option<Date> {
        // "Sat 17 Oct 2026": the weekday is a courtesy.
        let s = s.trim();
        let s = match s.split_once(' ') {
            Some((w, rest))
                if w.len() == 3
                    && w.chars().all(|c| c.is_ascii_alphabetic())
                    && format::MONTHS.iter().all(|m| !m.starts_with(w)) =>
            {
                rest
            }
            _ => s,
        };
        format::parse_date(s, self.today)
    }

    /// The draft's start and end, or what's wrong with them.
    pub fn times(&self) -> Result<(DateTime, DateTime), String> {
        let sd = self.date(&self.start_date).ok_or("The start date isn't one Koyomi can read.")?;
        let ed = self.date(&self.end_date).ok_or("The end date isn't one Koyomi can read.")?;
        if self.all_day {
            if ed < sd {
                return Err("It ends before it starts.".into());
            }
            let end = ed.tomorrow().unwrap_or(ed);
            return Ok((sd.to_datetime(Time::midnight()), end.to_datetime(Time::midnight())));
        }
        let st = format::parse_time(&self.start_time).ok_or("The start time isn't one Koyomi can read.")?;
        let et = format::parse_time(&self.end_time).ok_or("The end time isn't one Koyomi can read.")?;
        let (s, e) = (sd.to_datetime(st), ed.to_datetime(et));
        if e < s {
            return Err("It ends before it starts.".into());
        }
        Ok((s, e))
    }

    fn start_moved(&mut self) {
        // Keep the length: the end follows the start.
        let Some(sd) = self.date(&self.start_date) else { return };
        if self.all_day {
            let last = crate::rrule::add_days(sd, self.length.max(1) - 1).unwrap_or(sd);
            self.end_date = format!("{} {}", format::short_date(last), last.year());
            return;
        }
        let Some(st) = format::parse_time(&self.start_time) else { return };
        let end =
            sd.to_datetime(st).checked_add(jiff::SignedDuration::from_mins(self.length)).unwrap_or(sd.to_datetime(st));
        let (ed, et) = dt_text(end, self.h24);
        self.end_date = ed;
        self.end_time = et;
    }

    fn end_moved(&mut self) {
        if let Ok((s, e)) = self.times() {
            self.length = if self.all_day {
                crate::model::days_between(s.date(), e.date()).max(1)
            } else {
                e.duration_since(s).as_mins()
            };
        }
    }

    pub fn update(&mut self, f: Field, calendars: &[String]) {
        self.changed = true;
        self.error = None;
        match f {
            Field::Title(s) => self.title = s,
            Field::Location(s) => self.location = s,
            Field::Notes(a) => {
                self.notes.perform(a);
            }
            Field::AllDay(on) => {
                if on != self.all_day {
                    self.all_day = on;
                    if on {
                        self.length = 1;
                        self.start_moved();
                        self.alert = match self.alert {
                            AlertChoice::Before(_) => AlertChoice::Before(900),
                            ref a => a.clone(),
                        };
                    } else {
                        self.start_time = format::time(Time::constant(9, 0, 0, 0), self.h24);
                        self.length = 60;
                        self.start_moved();
                        self.alert = match self.alert {
                            AlertChoice::Before(_) => AlertChoice::Before(10),
                            ref a => a.clone(),
                        };
                    }
                }
            }
            Field::StartDate(s) => {
                self.start_date = s;
                self.start_moved();
            }
            Field::StartTime(s) => {
                self.start_time = s;
                self.start_moved();
            }
            Field::EndDate(s) => {
                self.end_date = s;
                self.end_moved();
            }
            Field::EndTime(s) => {
                self.end_time = s;
                self.end_moved();
            }
            Field::Freq(i) => {
                let freq = [None, Some(Freq::Daily), Some(Freq::Weekly), Some(Freq::Monthly), Some(Freq::Yearly)]
                    .get(i)
                    .copied()
                    .flatten();
                if i == 5 {
                    // "Custom" chosen again: nothing changes.
                    return;
                }
                let mut r = self.repeat.clone().unwrap_or(Repeat::NEVER);
                if r.freq != freq {
                    r.freq = freq;
                    r.weekdays.clear();
                    r.by_weekday = false;
                }
                self.repeat = Some(r);
            }
            Field::Interval(n) => {
                if let Some(r) = self.repeat.as_mut() {
                    r.interval = n.round().clamp(1.0, 999.0) as u32;
                }
            }
            Field::Weekday(w) => {
                let start = self.date(&self.start_date).map(|d| d.weekday());
                if let Some(r) = self.repeat.as_mut() {
                    if r.weekdays.is_empty()
                        && let Some(s) = start
                    {
                        r.weekdays.push(s);
                    }
                    match r.weekdays.iter().position(|x| *x == w) {
                        Some(i) if r.weekdays.len() > 1 => {
                            r.weekdays.remove(i);
                        }
                        Some(_) => {}
                        None => r.weekdays.push(w),
                    }
                    r.weekdays.sort_by_key(|x| x.to_monday_zero_offset());
                }
            }
            Field::ByWeekday(on) => {
                if let Some(r) = self.repeat.as_mut() {
                    r.by_weekday = on;
                }
            }
            Field::EndKind(i) => {
                let until = self
                    .date(&self.until_text)
                    .or_else(|| {
                        self.date(&self.start_date).and_then(|d| d.checked_add(jiff::Span::new().months(3)).ok())
                    })
                    .unwrap_or(self.today);
                if i == 2 {
                    self.until_text = format!("{} {}", format::short_date(until), until.year());
                }
                if let Some(r) = self.repeat.as_mut() {
                    r.end = match i {
                        1 => RepeatEnd::After(match r.end {
                            RepeatEnd::After(n) => n,
                            _ => 10,
                        }),
                        2 => RepeatEnd::On(until),
                        _ => RepeatEnd::Never,
                    };
                }
            }
            Field::Count(n) => {
                if let Some(r) = self.repeat.as_mut() {
                    r.end = RepeatEnd::After(n.round().clamp(1.0, 9999.0) as u32);
                }
            }
            Field::Until(s) => {
                let d = self.date(&s);
                self.until_text = s;
                if let (Some(r), Some(d)) = (self.repeat.as_mut(), d) {
                    r.end = RepeatEnd::On(d);
                }
            }
            Field::Alert(i) => {
                let list = if self.all_day { ALL_DAY_ALERTS } else { TIMED_ALERTS };
                self.alert = match i {
                    0 => AlertChoice::None,
                    i if i <= list.len() => AlertChoice::Before(list[i - 1].0),
                    _ => AlertChoice::Kept,
                };
            }
            Field::Calendar(i) => {
                if let Some(id) = calendars.get(i) {
                    self.calendar.clone_from(id);
                }
            }
        }
    }

    /// Put the draft's alert into `alarms`: the first display alarm is
    /// replaced (or removed); others (e-mail, extra ones) are kept.
    pub fn apply_alert(&self, alarms: &mut Vec<Alarm>) {
        let first = alarms.iter().position(|a| a.is_active() && matches!(a.action.as_str(), "DISPLAY" | "AUDIO"));
        match (&self.alert, first) {
            (AlertChoice::Kept, _) => {}
            (AlertChoice::None, Some(i)) => {
                alarms.remove(i);
            }
            (AlertChoice::None, None) => {}
            (AlertChoice::Before(m), Some(i)) => {
                alarms[i].trigger =
                    Trigger::Relative { offset: crate::ics::value::Duration::minutes(-m), from_end: false };
            }
            (AlertChoice::Before(m), None) => alarms.insert(0, Alarm::before(*m)),
        }
    }

    /// The sheet.
    pub fn view<Msg: Clone + Send + 'static>(
        &self,
        calendars: &[(String, String)],
        edit: impl Fn(Field) -> Msg + Clone + 'static,
        save: Msg,
        cancel: Msg,
        delete: Option<Msg>,
    ) -> Element<Msg> {
        let e = edit.clone();
        let title = text_field(&self.title)
            .placeholder("New Event")
            .on_change(move |s| e(Field::Title(s)))
            .on_submit(save.clone())
            .id("edit-title");
        let e = edit.clone();
        let location = text_field(&self.location)
            .placeholder("Location")
            .on_change(move |s| e(Field::Location(s)))
            .on_submit(save.clone())
            .id("edit-location");
        let mut rows: Vec<Element<Msg>> = Vec::new();
        let e = edit.clone();
        rows.push(form_row("All-day", toggle(self.all_day).on_toggle(move |on| e(Field::AllDay(on)))).into());
        let when = |label: &'static str,
                    date: &str,
                    time: &str,
                    fd: fn(String) -> Field,
                    ft: fn(String) -> Field,
                    id: &'static str|
         -> Element<Msg> {
            let e1 = edit.clone();
            let e2 = edit.clone();
            let mut r =
                row![text_field(date).mono().on_change(move |s| e1(fd(s))).width(132.0).id(format!("{id}-date"))]
                    .spacing(6.0);
            if !self.all_day {
                r = r.push(text_field(time).mono().on_change(move |s| e2(ft(s))).width(88.0).id(format!("{id}-time")));
            }
            form_row(label, r).into()
        };
        rows.push(when("Starts", &self.start_date, &self.start_time, Field::StartDate, Field::StartTime, "start"));
        rows.push(when("Ends", &self.end_date, &self.end_time, Field::EndDate, Field::EndTime, "end"));
        // Repeat.
        let start = self.date(&self.start_date).unwrap_or(self.today);
        let mut freqs: Vec<String> =
            ["Never", "Every Day", "Every Week", "Every Month", "Every Year"].iter().map(|s| s.to_string()).collect();
        let sel = match &self.repeat {
            Some(r) => match r.freq {
                None => 0,
                Some(Freq::Daily) => 1,
                Some(Freq::Weekly) => 2,
                Some(Freq::Monthly) => 3,
                _ => 4,
            },
            None => {
                freqs.push("Custom".into());
                5
            }
        };
        let e = edit.clone();
        rows.push(
            form_row("Repeat", dropdown(freqs, Some(sel)).on_select(move |i| e(Field::Freq(i))).id("edit-repeat"))
                .into(),
        );
        match &self.repeat {
            None => rows.push(
                form_row("Rule", text(self.custom_repeat.clone()).mono().size(11.0).role(Role::Text3))
                    .description("Kept as it is unless Repeat is changed.")
                    .into(),
            ),
            Some(r) if r.freq.is_some() => {
                let unit = match r.freq {
                    Some(Freq::Daily) => "days",
                    Some(Freq::Weekly) => "weeks",
                    Some(Freq::Monthly) => "months",
                    _ => "years",
                };
                let e = edit.clone();
                rows.push(
                    form_row(
                        "Every",
                        stepper(r.interval as f64)
                            .range(1.0..=99.0)
                            .step(1.0)
                            .suffix(format!(" {}", if r.interval == 1 { unit.trim_end_matches('s') } else { unit }))
                            .on_change(move |v| e(Field::Interval(v))),
                    )
                    .into(),
                );
                if r.freq == Some(Freq::Weekly) {
                    let days = crate::locale::Weeks::default().days();
                    let mut buttons: Vec<Element<Msg>> = Vec::new();
                    for w in days {
                        let on = if r.weekdays.is_empty() { w == start.weekday() } else { r.weekdays.contains(&w) };
                        let e = edit.clone();
                        buttons.push(
                            button(&format::weekday_short(w)[..2])
                                .compact()
                                .selected(on)
                                .on_press(e(Field::Weekday(w)))
                                .width(34.0)
                                .label(format::weekday_name(w))
                                .into(),
                        );
                    }
                    rows.push(form_row("On", aizome_kit::widgets::layout::row(buttons).spacing(2.0)).into());
                }
                if r.freq == Some(Freq::Monthly) {
                    let nth = (start.day() - 1) / 7 + 1;
                    let ord = ["first", "second", "third", "fourth", "last"][(nth as usize - 1).min(4)];
                    let opts = [
                        format!("On the {}", crate::edit::ordinal(start.day())),
                        format!("On the {ord} {}", format::weekday_short(start.weekday())),
                    ];
                    let e = edit.clone();
                    rows.push(
                        form_row(
                            "On",
                            segmented(opts, if r.by_weekday { 1 } else { 0 })
                                .on_select(move |i| e(Field::ByWeekday(i == 1))),
                        )
                        .into(),
                    );
                }
                let (kind, extra): (usize, Option<Element<Msg>>) = match &r.end {
                    RepeatEnd::Never => (0, None),
                    RepeatEnd::After(n) => {
                        let e = edit.clone();
                        (
                            1,
                            Some(
                                stepper(*n as f64)
                                    .range(1.0..=999.0)
                                    .step(1.0)
                                    .suffix(" times")
                                    .on_change(move |v| e(Field::Count(v)))
                                    .into(),
                            ),
                        )
                    }
                    RepeatEnd::On(_) => {
                        let e = edit.clone();
                        (
                            2,
                            Some(
                                text_field(&self.until_text)
                                    .mono()
                                    .on_change(move |s| e(Field::Until(s)))
                                    .width(132.0)
                                    .id("edit-until")
                                    .into(),
                            ),
                        )
                    }
                };
                let e = edit.clone();
                let mut end_row =
                    row![dropdown(["Never", "After", "On Date"], Some(kind)).on_select(move |i| e(Field::EndKind(i)))]
                        .spacing(6.0);
                if let Some(x) = extra {
                    end_row = end_row.push(x);
                }
                rows.push(form_row("End Repeat", end_row).into());
                if let Some(rr) = &self.repeat {
                    rows.push(form_row("", text(rr.describe(start)).size(11.0).role(Role::Text3)).into());
                }
            }
            _ => {}
        }
        // Alert.
        let list = if self.all_day { ALL_DAY_ALERTS } else { TIMED_ALERTS };
        let mut alerts: Vec<String> = vec!["None".into()];
        alerts.extend(list.iter().map(|(_, s)| s.to_string()));
        let sel = match &self.alert {
            AlertChoice::None => 0,
            AlertChoice::Before(m) => list.iter().position(|(x, _)| x == m).map(|i| i + 1).unwrap_or(0),
            AlertChoice::Kept => {
                alerts.push("Custom".into());
                alerts.len() - 1
            }
        };
        let e = edit.clone();
        rows.push(
            form_row("Alert", dropdown(alerts, Some(sel)).on_select(move |i| e(Field::Alert(i))).id("edit-alert"))
                .into(),
        );
        let names: Vec<String> = calendars.iter().map(|(_, n)| n.clone()).collect();
        let sel = calendars.iter().position(|(id, _)| *id == self.calendar);
        let e = edit.clone();
        rows.push(
            form_row("Calendar", dropdown(names, sel).on_select(move |i| e(Field::Calendar(i))).id("edit-calendar"))
                .into(),
        );

        let e = edit.clone();
        let notes = text_editor(&self.notes)
            .on_action(move |a| e(Field::Notes(a)))
            .mono(false)
            .current_line(false)
            .placeholder("Notes")
            .padding(6.0)
            .height(76.0)
            .id("edit-notes");
        let mut buttons: Vec<Element<Msg>> = Vec::new();
        if let Some(d) = delete {
            buttons.push(button("Delete…").destructive().on_press(d).into());
        }
        buttons.push(spacer().into());
        buttons.push(button("Cancel").cancel().on_press(cancel).into());
        buttons.push(
            button(if self.target.is_some() { "Save" } else { "Add Event" })
                .default()
                .on_press(save)
                .id("edit-save")
                .into(),
        );
        let mut col = column![
            text(if self.target.is_some() {
                if self.series { "Edit Repeating Event" } else { "Edit Event" }
            } else {
                "New Event"
            })
            .semibold()
            .size(15.0),
            title,
            location,
            form_section("", rows),
            container(notes).border(Edge::Line2).width(Length::Fill),
        ]
        .spacing(10.0);
        if let Some(err) = &self.error {
            col = col.push(text(err.clone()).role(Role::Danger).size(12.0));
        }
        col.push(aizome_kit::widgets::layout::row(buttons).spacing(8.0).width(Length::Fill))
            .padding(18.0)
            .width(470.0)
            .into()
    }
}
