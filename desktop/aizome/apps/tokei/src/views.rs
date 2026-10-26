//! What Tokei's window and sheets show.

use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{column, row, stack};

use crate::alarms::{Alarm, DAY_NAMES};
use crate::app::{Msg, PRESETS, Page, Sheet, Tokei, key_message};
use crate::clock::{self, Daylight, hm};
use crate::sound::SOUNDS;
use crate::timing::{TimerState, stopwatch_text, timer_text};
use crate::widgets::{digits, keys, mono, ring};

/// The width of the toolbar's side slots, so the page switch sits in the
/// middle.
const SLOT: f64 = 28.0;

pub fn window(app: &Tokei) -> Element<Msg> {
    let page = app.page;
    let body = match app.page {
        Page::World => world(app),
        Page::Alarms => alarms(app),
        Page::Timer => timer(app),
        Page::Stopwatch => stopwatch(app),
    };
    keys(column![toolbar(app), body].width(Length::Fill).height(Length::Fill), move |k, here| {
        key_message(page, k, here)
    })
}

fn toolbar(app: &Tokei) -> Element<Msg> {
    let names: Vec<&str> = Page::ALL.iter().map(|p| p.name()).collect();
    let switch = segmented(names, app.page.index()).on_select(|i| Msg::Page(Page::ALL[i.min(3)])).label("Page");
    let add: Element<Msg> = match app.page {
        Page::World => icon_button(Icon::Plus)
            .toolbar()
            .on_press(Msg::AddCity)
            .tooltip("Add a city (❖N)")
            .label("Add a city")
            .width(SLOT)
            .into(),
        Page::Alarms => icon_button(Icon::Plus)
            .toolbar()
            .on_press(Msg::NewAlarm)
            .tooltip("New alarm (❖N)")
            .label("New alarm")
            .width(SLOT)
            .into(),
        _ => space(SLOT),
    };
    aizome_kit::toolbar(vec![space(SLOT), spacer().into(), switch.into(), spacer().into(), add])
}

/// A big clock's line under it, or a page's quiet hint.
fn hint(s: impl Into<String>) -> Element<Msg> {
    container(text(s.into()).role(Role::Text3).center().wrap())
        .padding(24.0)
        .center()
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// World clocks.

fn world(app: &Tokei) -> Element<Msg> {
    let here = app.now.to_zoned(app.local.clone());
    let (t, suffix) = hm(here.hour(), here.minute(), app.h24);
    let big = format!("{t}:{:02}", here.second());
    let mut clock_row = vec![digits(&big, mono(54.0, 400.0), Role::Text)];
    if let Some(s) = suffix {
        clock_row.push(text(s).font(mono(20.0, 500.0)).role(Role::Text2).into());
    }
    let date = format!("{} · {}", here.strftime("%A %-d %B %Y"), app.local_name());
    let hero = column![aizome_kit::row(clock_row).spacing(8.0).align(Align::End), text(date).role(Role::Text2),]
        .spacing(4.0)
        .align(Align::Center)
        .padding([22.0, 16.0, 18.0, 16.0])
        .width(Length::Fill);
    let body: Element<Msg> = if app.places.is_empty() {
        hint("No cities yet. Add one with + or ❖N.")
    } else {
        let rows: Vec<Element<Msg>> = app.places.iter().map(|p| place_row(app, p)).collect();
        let n = app.places.len();
        list(rows)
            .row_height(60.0)
            .selected(app.selected_place)
            .on_select(Msg::SelectPlace)
            .context_menu(move |i| {
                Menu::new("City")
                    .item(MenuItem::new("Move Up", Msg::MovePlace(i, -1)).enabled(i > 0))
                    .item(MenuItem::new("Move Down", Msg::MovePlace(i, 1)).enabled(i + 1 < n))
                    .separator()
                    .item(MenuItem::new("Remove", Msg::RemovePlace(i)))
            })
            .id("places")
            .into()
    };
    column![hero, divider(), body].width(Length::Fill).height(Length::Fill).into()
}

fn place_row(app: &Tokei, p: &crate::app::Place) -> Element<Msg> {
    let Ok(zone) = jiff::tz::TimeZone::get(&p.zone) else { return text(p.name.clone()).into() };
    let (lat, lon) = p.coords();
    let t = clock::city_time(app.now, &app.local, &zone, lat, lon, app.h24);
    let (glyph, role, said) = match t.daylight {
        Daylight::Day => (Icon::Sun, Role::Warning, "day"),
        Daylight::Night => (Icon::Moon, Role::Accent, "night"),
        Daylight::Twilight { rising: true } => (Icon::Sunrise, Role::Warning, "dawn"),
        Daylight::Twilight { rising: false } => (Icon::Sunset, Role::Violet, "dusk"),
    };
    let mut time = vec![digits(&t.time, mono(30.0, 400.0), Role::Text)];
    if let Some(s) = t.suffix {
        time.push(text(s).font(mono(13.0, 500.0)).role(Role::Text2).into());
    }
    let inner = row![
        icon(glyph).size(20.0).role(role).label(said),
        column![
            text(p.name.clone()).semibold().size(15.0),
            text(format!("{} · {}", t.day.word(), t.offset)).size(12.0).role(Role::Text3),
        ]
        .spacing(2.0),
        spacer(),
        aizome_kit::row(time).spacing(4.0).align(Align::End),
    ]
    .spacing(12.0)
    .padding([0.0, 16.0])
    .width(Length::Fill)
    .height(Length::Fill);
    rule(inner)
        .key(format!("{}|{}", p.name, p.zone))
        .label(format!("{}, {} {}", p.name, t.time, t.day.word()))
        .tooltip(format!("{} · {}", p.country(), p.zone))
        .into()
}

/// A list's row with a rule under it, as the boards' lists have.
fn rule(inner: impl Into<Element<Msg>>) -> aizome_kit::widgets::layout::Container<Msg> {
    container(inner).edges(Edge::Line, Sides::BOTTOM).width(Length::Fill).height(Length::Fill)
}

// Alarms.

fn alarms(app: &Tokei) -> Element<Msg> {
    let body: Element<Msg> = if app.alarms.is_empty() {
        hint("No alarms. Add one with + or ❖N.")
    } else {
        let rows: Vec<Element<Msg>> = app.alarms.iter().enumerate().map(|(i, a)| alarm_row(app, i, a)).collect();
        list(rows)
            .row_height(68.0)
            .selected(app.selected_alarm)
            .on_select(Msg::SelectAlarm)
            .on_activate(Msg::EditAlarm)
            .context_menu(|i| {
                Menu::new("Alarm")
                    .item(MenuItem::new("Edit…", Msg::EditAlarm(i)))
                    .separator()
                    .item(MenuItem::new("Delete", Msg::DeleteAlarm(i)))
            })
            .id("alarms")
            .into()
    };
    let status: Element<Msg> = match (&app.schedule_error, app.next_alarm()) {
        (Some(e), _) => text(e.clone()).size(11.0).role(Role::Danger).into(),
        (None, Some((at, a))) => {
            let z = at.to_zoned(app.local.clone());
            let (t, s) = hm(z.hour(), z.minute(), app.h24);
            let when = format!("{t}{}", s.map(|s| format!(" {s}")).unwrap_or_default());
            let day = z.strftime("%A").to_string();
            let wait = in_words(at.duration_since(app.now).as_secs_f64().max(0.0) as u64);
            let what = if a.label.is_empty() { String::new() } else { format!(" ({})", a.label) };
            aizome_kit::status_text(format!("Next: {day} {when}{what}, in {wait}"))
        }
        (None, None) => aizome_kit::status_text("No alarm set"),
    };
    column![body, aizome_kit::status_bar(vec![status])].width(Length::Fill).height(Length::Fill).into()
}

/// "8 h 12 min", "2 days 3 h", "5 min".
pub fn in_words(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, (secs % 86_400) / 3600, (secs % 3600).div_ceil(60).min(59));
    match (d, h, m) {
        (0, 0, m) => format!("{} min", m.max(1)),
        (0, h, 0) => format!("{h} h"),
        (0, h, m) => format!("{h} h {m} min"),
        (1, 0, _) => "1 day".into(),
        (1, h, _) => format!("1 day {h} h"),
        (d, 0, _) => format!("{d} days"),
        (d, h, _) => format!("{d} days {h} h"),
    }
}

fn alarm_row(app: &Tokei, i: usize, a: &Alarm) -> Element<Msg> {
    let (t, suffix) = hm(a.hour as i8, a.minute as i8, app.h24);
    let role = if a.enabled { Role::Text } else { Role::Text3 };
    let mut time = vec![digits(&t, mono(32.0, 400.0), role)];
    if let Some(s) = suffix {
        time.push(text(s).font(mono(13.0, 500.0)).role(Role::Text3).into());
    }
    let label = if a.label.is_empty() { "Alarm".to_owned() } else { a.label.clone() };
    let mut detail = format!("{label} · {}", a.repeat_words());
    if a.wake {
        detail.push_str(" · wakes the computer");
    }
    let inner = row![
        column![aizome_kit::row(time).spacing(4.0).align(Align::End), text(detail).size(12.0).role(Role::Text3)]
            .spacing(2.0),
        spacer(),
        toggle(a.enabled).on_toggle(move |on| Msg::ToggleAlarm(i, on)).label(format!("{label} on")),
    ]
    .padding([0.0, 16.0])
    .width(Length::Fill)
    .height(Length::Fill);
    rule(inner).key(a.id.clone()).label(format!("{label}, {t}, {}", if a.enabled { "on" } else { "off" })).into()
}

// The timer.

fn timer(app: &Tokei) -> Element<Msg> {
    let tm = &app.timer;
    let left = tm.left(app.mono);
    let (status, status_role) = match tm.state {
        TimerState::Ready => ("Ready".to_owned(), Role::Text3),
        TimerState::Paused { .. } => ("Paused".to_owned(), Role::Warning),
        TimerState::Done => ("Done".to_owned(), Role::Success),
        TimerState::Running { .. } => {
            let end = app.now.checked_add(jiff::SignedDuration::try_from(left).unwrap_or_default()).unwrap_or(app.now);
            let z = end.to_zoned(app.local.clone());
            let (t, s) = hm(z.hour(), z.minute(), app.h24);
            (format!("Ends {t}{}", s.map(|s| format!(" {s}")).unwrap_or_default()), Role::Text3)
        }
    };
    let face = stack![
        ring(tm.fraction(app.mono), 260.0),
        column![digits(&timer_text(left), mono(52.0, 400.0), Role::Text), text(status).role(status_role)]
            .spacing(2.0)
            .align(Align::Center),
    ]
    .align_x(Align::Center)
    .align_y(Align::Center);
    let presets: Vec<Element<Msg>> = PRESETS
        .iter()
        .map(|m| {
            let label = if *m == 60 { "1 h".to_owned() } else { format!("{m} min") };
            let chosen = tm.length == Duration::from_secs(m * 60);
            button(label).compact().selected(chosen).on_press(Msg::TimerPreset(*m)).into()
        })
        .collect();
    let idle = matches!(tm.state, TimerState::Ready | TimerState::Done);
    let main = match tm.state {
        TimerState::Running { .. } => "Pause",
        TimerState::Paused { .. } => "Resume",
        _ => "Start",
    };
    let entry = row![
        text("Length").role(Role::Text2),
        text_field(&app.timer_entry)
            .mono()
            .placeholder("5:00")
            .on_change(Msg::TimerEntry)
            .on_submit(Msg::TimerSet)
            .enabled(idle)
            .id("timer-length")
            .width(96.0),
        button("Set").on_press_maybe(idle.then_some(Msg::TimerSet)),
    ]
    .spacing(8.0);
    column![
        space(8.0),
        face,
        aizome_kit::row(presets).spacing(6.0),
        entry,
        row![
            button("Cancel")
                .on_press_maybe((!matches!(tm.state, TimerState::Ready)).then_some(Msg::TimerCancel))
                .id("timer-cancel"),
            button(main).primary().on_press(Msg::TimerStart).id("timer-start"),
        ]
        .spacing(12.0),
    ]
    .spacing(18.0)
    .align(Align::Center)
    .padding(16.0)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// The stopwatch.

fn stopwatch(app: &Tokei) -> Element<Msg> {
    let w = &app.stopwatch;
    let now = app.mono;
    let total = w.elapsed(now);
    let running = w.running();
    let side = if running {
        button("Lap").on_press(Msg::WatchLap).id("watch-lap")
    } else {
        button("Reset").on_press_maybe(w.started().then_some(Msg::WatchReset)).id("watch-reset")
    };
    let main = if running { button("Stop").destructive().primary() } else { button("Start").primary() };
    let splits = w.splits();
    let extremes = w.extremes();
    let last = w.laps.last().copied().unwrap_or_default();
    // The lap under way first, then the finished ones, newest first.
    let mut rows: Vec<(usize, Duration, Duration, Role)> = Vec::new();
    if w.started() {
        rows.push((splits.len() + 1, total.saturating_sub(last), total, Role::Text));
    }
    for (i, s) in splits.iter().enumerate().rev() {
        let role = match extremes {
            Some((f, _)) if f == i => Role::Success,
            Some((_, s)) if s == i => Role::Danger,
            _ => Role::Text,
        };
        rows.push((i + 1, *s, w.laps[i], role));
    }
    let laps: Element<Msg> = if rows.is_empty() {
        hint("Laps show here. Space starts and stops; L takes a lap.")
    } else {
        let rows = std::rc::Rc::new(rows);
        let n = rows.len();
        virtual_list(n, 32.0, move |i| {
            let (lap, split, total, role) = rows[i];
            row![
                text(format!("Lap {lap}")).role(Role::Text2),
                spacer(),
                digits(&stopwatch_text(split), mono(15.0, 500.0), role),
                space(28.0),
                digits(&stopwatch_text(total), mono(15.0, 400.0), Role::Text3),
            ]
            .padding([0.0, 16.0])
            .width(Length::Fill)
            .key(lap)
        })
        .id("laps")
        .into()
    };
    column![
        container(digits(&stopwatch_text(total), mono(56.0, 400.0), Role::Text))
            .padding([28.0, 16.0, 18.0, 16.0])
            .align_x(Align::Center)
            .width(Length::Fill),
        row![side, main.on_press(Msg::WatchToggle).id("watch-start")].spacing(12.0),
        space(12.0),
        divider(),
        laps,
    ]
    .align(Align::Center)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// Sheets.

pub fn sheet(app: &Tokei, s: &Sheet) -> Element<Msg> {
    match s {
        Sheet::AddCity { query, results, picked, .. } => add_city(app, query, results, *picked),
        Sheet::Alarm { draft, index, time, error, .. } => edit_alarm(app, draft, *index, time, error.as_deref()),
    }
}

fn add_city(app: &Tokei, query: &str, results: &[&'static crate::cities::City], picked: usize) -> Element<Msg> {
    let list_el: Element<Msg> = if query.trim().is_empty() {
        hint("Type a city, a country or a time zone.")
    } else if results.is_empty() {
        hint(format!("No city matches “{}”.", query.trim()))
    } else {
        let now = app.now;
        let local = app.local.clone();
        let h24 = app.h24;
        let found: std::rc::Rc<Vec<&'static crate::cities::City>> = std::rc::Rc::new(results.to_vec());
        let rows = found.clone();
        virtual_list(found.len(), 40.0, move |i| {
            let c = rows[i];
            let (time, offset) = match jiff::tz::TimeZone::get(&c.zone) {
                Ok(z) => {
                    let t = clock::city_time(now, &local, &z, c.lat, c.lon, h24);
                    (format!("{}{}", t.time, t.suffix.map(|s| format!(" {s}")).unwrap_or_default()), t.offset)
                }
                Err(_) => (String::new(), String::new()),
            };
            row![
                column![text(c.name.clone()).semibold(), text(c.country.clone()).size(11.5).role(Role::Text3)]
                    .spacing(1.0),
                spacer(),
                column![
                    text(time).font(mono(13.0, 500.0)).role(Role::Text2),
                    text(offset).size(11.5).role(Role::Text3)
                ]
                .spacing(1.0)
                .align(Align::End),
            ]
            .padding([0.0, 10.0])
            .width(Length::Fill)
        })
        .selected([picked])
        .on_select(Msg::CityPick)
        .on_activate(|i| Msg::CityAdd(Some(i)))
        .id("city-results")
        .into()
    };
    column![
        text("Add a City").semibold().size(15.0),
        search_field(query)
            .placeholder("City, country or zone")
            .on_change(Msg::CityQuery)
            .on_submit(Msg::CityAdd(None))
            .id("city-search"),
        container(list_el).border(Edge::Line).height(260.0).width(Length::Fill),
        row![
            spacer(),
            button("Cancel").cancel().on_press(Msg::CloseSheet),
            button("Add").default().on_press_maybe((!results.is_empty()).then_some(Msg::CityAdd(None))),
        ]
        .spacing(8.0)
        .width(Length::Fill),
    ]
    .spacing(12.0)
    .padding(16.0)
    .width(400.0)
    .into()
}

fn label(s: &str) -> Element<Msg> {
    text(s.to_owned()).role(Role::Text2).width(64.0).into()
}

fn edit_alarm(app: &Tokei, a: &Alarm, index: Option<usize>, time: &str, error: Option<&str>) -> Element<Msg> {
    let _ = app;
    let days: Vec<Element<Msg>> = (1..=7u8)
        .map(|d| {
            let on = a.days.contains(&d);
            let b = button(DAY_NAMES[usize::from(d - 1)])
                .compact()
                .on_press(Msg::DraftDay(d))
                .width(36.0)
                .label(format!("Repeat on {}", DAY_NAMES[usize::from(d - 1)]));
            if on { b.primary().into() } else { b.into() }
        })
        .collect();
    let sounds: Vec<&str> = SOUNDS.iter().map(|(_, n)| *n).collect();
    let sound_i = SOUNDS.iter().position(|(id, _)| *id == a.sound);
    let mut buttons: Vec<Element<Msg>> = Vec::new();
    if let Some(i) = index {
        buttons.push(button("Delete").destructive().on_press(Msg::DeleteAlarm(i)).into());
    }
    buttons.push(spacer().into());
    buttons.push(button("Cancel").cancel().on_press(Msg::CloseSheet).into());
    buttons.push(
        button("Save").default().on_press_maybe(error.is_none().then_some(Msg::SaveAlarm)).id("alarm-save").into(),
    );
    column![
        text(if index.is_some() { "Edit Alarm" } else { "New Alarm" }).semibold().size(15.0),
        row![
            label("Time"),
            text_field(time)
                .mono()
                .placeholder("7:30")
                .on_change(Msg::DraftTime)
                .on_submit(Msg::SaveAlarm)
                .error(error)
                .id("alarm-time")
                .width(140.0)
        ]
        .spacing(8.0)
        .align(Align::Start),
        row![
            label("Label"),
            text_field(&a.label)
                .placeholder("Alarm")
                .on_change(Msg::DraftLabel)
                .on_submit(Msg::SaveAlarm)
                .id("alarm-label")
        ]
        .spacing(8.0),
        row![label("Repeat"), aizome_kit::row(days).spacing(4.0)].spacing(8.0),
        row![
            label("Sound"),
            dropdown(sounds, sound_i).on_select(Msg::DraftSound).width(170.0),
            icon_button(Icon::Volume2).on_press(Msg::PreviewSound).tooltip("Play it").label("Play the sound"),
        ]
        .spacing(8.0),
        row![
            label("Snooze"),
            stepper(f64::from(a.snooze_minutes)).range(1.0..=60.0).step(1.0).suffix(" min").on_change(Msg::DraftSnooze)
        ]
        .spacing(8.0),
        row![label(""), checkbox("Wake the computer from sleep", a.wake).on_toggle(Msg::DraftWake)].spacing(8.0),
        space(2.0),
        aizome_kit::row(buttons).spacing(8.0).width(Length::Fill),
    ]
    .spacing(12.0)
    .padding(16.0)
    .width(440.0)
    .into()
}
