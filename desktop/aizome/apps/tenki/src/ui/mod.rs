//! Tenki's window: the sidebar of places (a search field on top, the
//! places below, dragged to reorder) and the place showing: its current
//! conditions large, the next 48 hours, ten days, and tiles of detail.

pub mod widgets;

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::row as hrow;
use aizome_kit::{Align, column, row};

use crate::app::{Locating, Msg, Tenki, Weather, has_position};
use crate::forecast::{Day, Forecast, Hour, Now};
use crate::places::{Origin, Place};
use crate::store::Consent;
use crate::time::{self, Clock, LocalTime};
use crate::units::Units;
use crate::weather::{self, UvBand, condition};
use widgets::{
    DayRow, Gauge, HourCell, RowLabel, click_row, columns, compass, day_row, gauge, grid, hour_cell, place_list,
    sun_arc,
};

/// The sidebar's width.
pub const SIDEBAR: f64 = 236.0;
const PAD: f64 = 20.0;
const GAP: f64 = 14.0;

/// The whole window.
pub fn window(app: &Tenki) -> Element<Msg> {
    let content = column![toolbar_for(app), scroll(body(app)).id("main").key("main")].width(Fill).height(Fill);
    sidebar_layout(sidebar(app), content).first_size(SIDEBAR).into()
}

// The sidebar.

fn sidebar(app: &Tenki) -> Element<Msg> {
    let field = search_field(&app.search.text)
        .placeholder("Search for a city")
        .on_change(Msg::Search)
        .on_submit(Msg::AddFirst)
        .id("search");
    let mut col = column![container(field).padding([10.0, 10.0, 6.0, 10.0]).width(Fill)].width(Fill).height(Fill);
    if app.search.text.trim().is_empty() {
        col = col.push(caption("Places")).push(places(app));
    } else {
        col = col.push(caption("Add a place")).push(results(app));
    }
    col.push(spacer())
        .push(divider())
        .push(container(text("Weather data by Open-Meteo.com").size(11.0).role(Role::Text3)).padding([7.0, 12.0]))
        .into()
}

fn places(app: &Tenki) -> Element<Msg> {
    let rows = app.rows();
    if rows.is_empty() {
        return container(text("Search above to add a city.").size(12.0).role(Role::Text3).wrap())
            .padding([4.0, 12.0])
            .width(Fill)
            .into();
    }
    let now = app.now();
    let units = app.units();
    let labels = rows.iter().map(|p| RowLabel { name: p.name.clone(), detail: sidebar_detail(app, p) }).collect();
    let elements = rows.iter().map(|p| place_row(app, p, now, units)).collect();
    let pinned = app.pinned();
    let n = rows.len();
    place_list(
        elements,
        labels,
        app.selected_index(),
        pinned,
        Msg::Select,
        |(a, b)| Msg::Move(a, b),
        Msg::Remove,
        move |i| {
            let movable = i >= pinned;
            Menu::new("Place")
                .item(MenuItem::new("Move Up", Msg::Move(i, i.saturating_sub(1))).enabled(movable && i > pinned))
                .item(MenuItem::new("Move Down", Msg::Move(i, i + 1)).enabled(movable && i + 1 < n))
                .separator()
                .item(MenuItem::new("Remove", Msg::Remove(i)))
        },
    )
}

fn sidebar_detail(app: &Tenki, p: &Place) -> String {
    match (p.origin, has_position(p), &app.locating) {
        (Origin::Current, false, Locating::Failed(_)) => "Location unavailable".into(),
        (Origin::Current, false, _) => "Locating…".into(),
        _ => p.subtitle(),
    }
}

fn place_row(app: &Tenki, p: &Place, now: i64, units: Units) -> Element<Msg> {
    let w = app.weather_of(p).and_then(|w| w.forecast.as_ref().map(|f| (f, w.fetched_at)));
    let shown = w.map(|(f, at)| f.now(now, at));
    let temp = shown.as_ref().and_then(|n| n.temperature).map_or_else(|| "—".to_owned(), |c| units.degrees(c));
    let glyph = shown.as_ref().and_then(|n| n.code.map(|c| condition(c, n.is_day).icon));
    let mut name = hrow(Vec::new()).spacing(5.0);
    if p.origin == Origin::Current {
        name = name.push(icon(Icon::Navigation).size(11.0).role(Role::Text2));
    }
    name = name.push(text(p.name.clone()).size(13.0).semibold());
    let mut r = hrow(vec![
        column![name, text(sidebar_detail(app, p)).size(11.0).role(Role::Text3)].spacing(2.0).width(Fill).into(),
    ])
    .spacing(8.0)
    .padding([0.0, 12.0])
    .height(widgets::PLACE_ROW);
    if let Some(g) = glyph {
        r = r.push(icon(g).size(18.0).role(Role::Text2));
    }
    r.push(text(temp).mono().tabular().size(16.0).weight(500.0)).key(p.id.as_str()).into()
}

fn results(app: &Tenki) -> Element<Msg> {
    let s = &app.search;
    let note = |t: String| -> Element<Msg> {
        container(text(t).size(12.0).role(Role::Text3).wrap()).padding([4.0, 12.0]).width(Fill).into()
    };
    let typed = s.text.trim();
    if typed.chars().count() < 2 {
        return note("Type a city's name.".into());
    }
    let answered = s.answered.as_deref() == Some(typed);
    if let Some(e) = &s.error {
        return note(e.clone());
    }
    if !answered || s.busy {
        if s.results.is_empty() {
            return note("Searching…".into());
        }
    } else if s.results.is_empty() {
        return note(format!("No places named “{typed}”."));
    }
    let rows: Vec<Element<Msg>> = s
        .results
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let region = f.region();
            let label = format!("{}, {}", f.name, region);
            let body = column![text(f.name.clone()).size(13.0).semibold(), text(region).size(11.0).role(Role::Text3)]
                .spacing(2.0);
            click_row(container(body).padding([6.0, 12.0]).width(Fill), Msg::Add(i), label)
                .key(f.id)
                .id(format!("result-{i}"))
        })
        .collect();
    let mut col = aizome_kit::widgets::layout::column(rows).width(Fill);
    col = col.push(container(text("Return adds the first.").size(11.0).role(Role::Faint)).padding([6.0, 12.0]));
    col.into()
}

// The toolbar.

fn toolbar_for(app: &Tenki) -> Element<Msg> {
    let units = app.units();
    let which = usize::from(units == Units::Imperial);
    toolbar(vec![
        icon_button(Icon::RefreshCw).toolbar().on_press(Msg::Refresh).tooltip("Refresh (❖R)").into(),
        toolbar_separator(),
        segmented(["°C", "°F"], Some(which))
            .on_select(|i| Msg::SetUnits(if i == 1 { Units::Imperial } else { Units::Metric }))
            .tooltip("Units")
            .id("units")
            .into(),
        spacer().into(),
        status_text(status(app)),
    ])
}

/// What the toolbar says about the forecast showing: "Updated 12 min ago".
pub fn status(app: &Tenki) -> String {
    let Some(p) = app.selected_place() else { return String::new() };
    let Some(w) = app.weather_of(&p) else { return String::new() };
    let ago = time::ago(app.now(), w.fetched_at);
    match (&w.forecast, w.loading, &w.error) {
        (None, true, _) => "Fetching…".into(),
        (Some(_), true, _) => "Updating…".into(),
        (Some(_), false, Some(e)) if e.is_offline() => format!("Offline · updated {ago}"),
        (Some(_), false, Some(_)) => format!("Couldn't update · updated {ago}"),
        (Some(_), false, None) => format!("Updated {ago}"),
        (None, false, Some(e)) if e.is_offline() => "Offline".into(),
        (None, false, _) => String::new(),
    }
}

// The main view.

fn body(app: &Tenki) -> Element<Msg> {
    let mut col = aizome_kit::widgets::layout::column(Vec::new()).spacing(GAP).padding(PAD).width(Fill);
    if app.settings.location == Consent::Ask {
        col = col.push(location_banner());
    }
    let Some(place) = app.selected_place() else {
        return col.push(empty_state(app)).into();
    };
    if place.origin == Origin::Current && !has_position(&place) {
        return col.push(locating_state(app)).into();
    }
    let w = app.weather_of(&place);
    let inner = match w.and_then(|w| w.forecast.as_ref().map(|f| (f, w))) {
        Some((f, w)) => forecast_view(app, &place, f, w),
        None => waiting_state(&place, w),
    };
    col.push(inner).into()
}

fn tile(icon_: Icon, title: &str, note: &str, body: Element<Msg>) -> Element<Msg> {
    let mut head = hrow(vec![caption_row(icon_, title)]).width(Fill);
    if !note.is_empty() {
        head = head.push(spacer()).push(text(note.to_owned()).size(11.0).role(Role::Text3));
    }
    container(column![head, body].spacing(10.0).width(Fill))
        .padding([12.0, 14.0, 14.0, 14.0])
        .fill(Tone::Chrome)
        .border(Edge::Line)
        .bevel()
        .width(Fill)
        .into()
}

fn caption_row(icon_: Icon, title: &str) -> Element<Msg> {
    row![
        icon(icon_).size(13.0).role(Role::Text3),
        text(title.to_owned()).mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3),
    ]
    .spacing(6.0)
    .into()
}

fn location_banner() -> Element<Msg> {
    let words = column![
        text("See the weather where you are?").size(14.0).semibold(),
        text(
            "Tenki can ask GeoClue for your approximate position, to the nearest town, and send it rounded to \
             Open-Meteo. Nothing else leaves the computer. You can change your mind in Settings."
        )
        .size(12.0)
        .role(Role::Text2)
        .wrap(),
        // The buttons under the words, so the words keep the width at any
        // size of the window (beside them they were squeezed to a letter
        // a line in a narrow one).
        row![
            button("Use My Location").primary().on_press(Msg::AllowLocation),
            button("Not Now").on_press(Msg::DeclineLocation),
        ]
        .spacing(8.0),
    ]
    .spacing(6.0)
    .width(Fill);
    container(row![icon(Icon::LocateFixed).size(22.0).role(Role::Warning), words].spacing(12.0).width(Fill))
        .padding([12.0, 14.0])
        .fill(Tone::Raised)
        .border(Edge::Line2)
        .bevel()
        .width(Fill)
        .key("ask-location")
        .into()
}

/// A quiet message in the middle of the main view.
fn notice(glyph: Icon, title: String, lines: Vec<String>, buttons: Vec<Element<Msg>>, busy: bool) -> Element<Msg> {
    let mut col = column![icon(glyph).size(40.0).role(Role::Faint), text(title).size(17.0).semibold()]
        .spacing(8.0)
        .align(Align::Center);
    for l in lines {
        col = col.push(text(l).size(13.0).role(Role::Text2).center().wrap().width(Fill));
    }
    if busy {
        col = col.push(container(progress_indeterminate()).width(180.0));
    }
    if !buttons.is_empty() {
        col = col.push(space(4.0)).push(hrow(buttons).spacing(8.0));
    }
    container(col.width(440.0)).padding([72.0, 20.0]).width(Fill).align_x(Align::Center).into()
}

fn empty_state(app: &Tenki) -> Element<Msg> {
    let mut buttons: Vec<Element<Msg>> = vec![button("Add a Place").primary().on_press(Msg::FocusSearch).into()];
    if app.settings.location != Consent::Allowed {
        buttons.insert(0, button("Use My Location").on_press(Msg::AllowLocation).into());
    }
    notice(
        Icon::MapPin,
        "No places yet".into(),
        vec!["Search for a city in the sidebar to see its weather.".into()],
        buttons,
        false,
    )
}

fn locating_state(app: &Tenki) -> Element<Msg> {
    match &app.locating {
        Locating::Failed(e) => notice(
            Icon::LocateFixed,
            "Location unavailable".into(),
            vec![e.message(), e.hint().into()],
            vec![
                button("Search for a City").on_press(Msg::SearchInstead).into(),
                button("Try Again").primary().on_press(Msg::Locate).into(),
            ],
            false,
        ),
        _ => notice(
            Icon::LocateFixed,
            "Finding where you are…".into(),
            vec!["Finding your approximate location. You can search for a city at any time.".into()],
            vec![button("Search for a City").on_press(Msg::SearchInstead).into()],
            true,
        ),
    }
}

fn waiting_state(place: &Place, w: Option<&Weather>) -> Element<Msg> {
    match w.and_then(|w| w.error.as_ref()) {
        Some(e) if !w.is_some_and(|w| w.loading) => notice(
            Icon::CloudOff,
            format!("No forecast for {} yet", place.name),
            vec![e.message(), "Tenki tries again each minute.".into()],
            vec![button("Try Again").primary().on_press(Msg::Refresh).into()],
            false,
        ),
        _ => notice(Icon::CloudSun, format!("Fetching the forecast for {}…", place.name), Vec::new(), Vec::new(), true),
    }
}

fn forecast_view(app: &Tenki, place: &Place, f: &Forecast, w: &Weather) -> Element<Msg> {
    let now = app.now();
    let units = app.units();
    let shown = f.now(now, w.fetched_at);
    let days = f.days_from(now);
    let ten = tile(Icon::Calendar, "10-day forecast", "", ten_days(days, f, &shown, now, units));
    let details = grid(detail_tiles(f, &shown, now, units, app.clock), 200.0, 12.0);
    column![
        hero(place, f, &shown, now, units, app.clock),
        tile(
            Icon::Clock,
            "Next 48 hours",
            &dry_note(f.hours_from(now)),
            hourly(f.hours_from(now), now, f, app.clock, units)
        ),
        columns(ten, details, 1000.0, 0.52, GAP),
    ]
    .spacing(GAP)
    .width(Fill)
    .into()
}

fn hero(place: &Place, f: &Forecast, n: &Now, now: i64, units: Units, clock: Clock) -> Element<Msg> {
    let local = f.local(now);
    let mut sub = Vec::new();
    let region = match place.origin {
        Origin::TimeZone if !place.region.is_empty() => format!("{} · your time zone", place.region),
        _ => place.subtitle(),
    };
    if !region.is_empty() {
        sub.push(region);
    }
    sub.push(format!("{} {}", local.weekday_short(), clock.time(local)));
    let temp = n.temperature.map_or_else(|| "—".to_owned(), |c| units.degrees(c));
    let cond = n.code.map(|c| condition(c, n.is_day));
    let today = f.today(now);
    let hl = match today {
        Some(d) => format!(
            "H {}  L {}",
            d.max.map_or("—".into(), |c| units.degrees(c)),
            d.min.map_or("—".into(), |c| units.degrees(c))
        ),
        None => String::new(),
    };
    let mut feels = n.apparent.map(|c| format!("Feels like {}", units.degrees(c))).unwrap_or_default();
    if n.forecast {
        feels = format!("Forecast for {} · {feels}", clock.hour(n.time));
    }
    let mut cond_row = hrow(Vec::new()).spacing(8.0);
    if let Some(c) = cond {
        cond_row = cond_row
            .push(icon(c.icon).size(28.0).role(widgets::glyph_role(c.icon)))
            .push(text(c.text).size(19.0).weight(500.0));
    }
    let side = column![
        cond_row,
        text(hl).mono().tabular().size(14.0).weight(500.0).role(Role::Text2),
        text(feels).size(13.0).role(Role::Text3)
    ]
    .spacing(6.0);
    column![
        text(place.name.clone()).size(24.0).semibold(),
        text(sub.join(" · ")).size(13.0).role(Role::Text3),
        row![text(temp).size(92.0).weight(300.0).tabular().key("temp"), side].spacing(22.0),
    ]
    .spacing(2.0)
    .width(Fill)
    .key("hero")
    .into()
}

/// Whether no hour shown has a real chance of rain.
fn is_dry(hours: &[Hour]) -> bool {
    hours.iter().all(|h| h.precipitation_probability.unwrap_or(0.0) < 5.0)
}

fn dry_note(hours: &[Hour]) -> String {
    if !hours.is_empty() && is_dry(hours) { "No rain expected".into() } else { String::new() }
}

fn hourly(hours: &[Hour], now: i64, f: &Forecast, clock: Clock, units: Units) -> Element<Msg> {
    if hours.is_empty() {
        return text("The hourly forecast has run out: it comes back when Tenki can update.")
            .size(12.0)
            .role(Role::Text3)
            .into();
    }
    let this_hour = f.local(now).hour_start();
    let bars = !is_dry(hours);
    let cells: Vec<Element<Msg>> = hours
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let is_now = h.time == this_hour;
            let day_start = h.time.hour == 0 && i > 0;
            let label = if is_now {
                "Now".to_owned()
            } else if day_start {
                h.time.weekday_short().to_owned()
            } else {
                clock.hour(h.time)
            };
            let c = condition(h.code.unwrap_or(0), h.is_day);
            let temp = h.temperature.map_or_else(|| "—".into(), |t| units.degrees(t));
            let access = format!(
                "{}, {}, {}{}",
                clock.hour(h.time),
                c.text,
                temp,
                h.precipitation_probability.map(|p| format!(", {p:.0}% chance of rain")).unwrap_or_default()
            );
            hour_cell(HourCell {
                label,
                strong: is_now || day_start,
                day_start,
                icon: c.icon,
                temp,
                probability: h.precipitation_probability,
                bars,
                label_for_access: access,
            })
            .key(h.time)
        })
        .collect();
    scroll(hrow(cells).align(Align::Start))
        .horizontal()
        .height(if bars { widgets::HOUR_HEIGHT } else { widgets::HOUR_HEIGHT_DRY } + 12.0)
        .key("hours")
        .into()
}

fn ten_days(days: &[Day], f: &Forecast, n: &Now, now: i64, units: Units) -> Element<Msg> {
    let Some(span) = f.span(now) else {
        return text("No days ahead in this forecast.").size(12.0).role(Role::Text3).into();
    };
    let today = f.local(now).date();
    let rows: Vec<Element<Msg>> = days
        .iter()
        .take(crate::forecast::DAYS)
        .enumerate()
        .map(|(i, d)| {
            let is_today = d.date == today;
            let c = condition(d.code.unwrap_or(0), true);
            let label = if is_today { "Today".to_owned() } else { d.date.weekday_short().to_owned() };
            let lo_text = d.min.map_or("—".into(), |c| units.degrees(c));
            let hi_text = d.max.map_or("—".into(), |c| units.degrees(c));
            let access = format!("{label}: {}, {lo_text} to {hi_text}", c.text);
            day_row(DayRow {
                label,
                today: is_today,
                icon: c.icon,
                probability: d.precipitation_probability,
                lo: d.min,
                hi: d.max,
                lo_text,
                hi_text,
                span,
                now: if is_today { n.temperature } else { None },
                rule: i > 0,
                label_for_access: access,
            })
            .key(d.date)
        })
        .collect();
    aizome_kit::widgets::layout::column(rows).width(Fill).into()
}

/// A detail tile: its caption, a value large, a line or two, and a
/// picture of it.
fn detail(
    icon_: Icon,
    title: &str,
    value: Element<Msg>,
    lines: Vec<String>,
    picture: Option<Element<Msg>>,
    key: &str,
) -> Element<Msg> {
    let mut col = column![caption_row(icon_, title), value].spacing(6.0).width(Fill);
    for l in lines.into_iter().filter(|l| !l.is_empty()) {
        col = col.push(text(l).size(12.0).role(Role::Text2).max_lines(2));
    }
    if let Some(p) = picture {
        col = col.push(spacer()).push(p);
    }
    container(col)
        .padding([12.0, 14.0])
        .fill(Tone::Chrome)
        .border(Edge::Line)
        .bevel()
        .width(Fill)
        .height(Fill)
        .min_height(132.0)
        .key(key)
        .into()
}

/// A tile's value: the number large and light, its unit small.
fn value(v: String, unit: &str) -> Element<Msg> {
    let mut r = hrow(vec![text(v).size(30.0).weight(300.0).tabular().into()]).spacing(4.0).align(Align::End);
    if !unit.is_empty() {
        r = r.push(container(text(unit.to_owned()).size(13.0).role(Role::Text3)).padding([0.0, 0.0, 5.0, 0.0]));
    }
    r.into()
}

fn detail_tiles(f: &Forecast, n: &Now, now: i64, units: Units, clock: Clock) -> Vec<Element<Msg>> {
    let today = f.today(now);
    let dash = || "—".to_owned();
    let mut tiles = Vec::new();

    // Feels like.
    let feels_line = match (n.apparent, n.temperature) {
        (Some(a), Some(t)) if (a - t).abs() < 1.5 => "Close to the actual temperature.".to_owned(),
        (Some(a), Some(t)) if a < t && n.wind_speed.unwrap_or(0.0) >= 10.0 => {
            "The wind makes it feel colder.".to_owned()
        }
        (Some(a), Some(t)) if a < t => "Feels colder than it is.".to_owned(),
        (Some(_), Some(_)) if n.humidity.unwrap_or(0.0) >= 60.0 => "Humidity makes it feel warmer.".to_owned(),
        (Some(_), Some(_)) => "Feels warmer than it is.".to_owned(),
        _ => String::new(),
    };
    let actual = n.temperature.map(|t| format!("Actual {}", units.degrees(t))).unwrap_or_default();
    tiles.push(detail(
        Icon::Thermometer,
        "Feels like",
        value(n.apparent.map_or_else(dash, |a| units.degrees(a)), ""),
        vec![feels_line, actual],
        None,
        "feels",
    ));

    // UV.
    let uv = n.uv.or(today.and_then(|d| d.uv_max));
    let band = uv.map(UvBand::of);
    let uv_value = uv.map_or_else(dash, |u| format!("{:.0}", u.round()));
    let uv_lines = vec![
        band.map(|b| b.advice().to_owned()).unwrap_or_default(),
        today.and_then(|d| d.uv_max).map(|m| format!("Today's high {:.0}", m.round())).unwrap_or_default(),
    ];
    let uv_picture = uv.map(|u| gauge(Gauge::Uv(u)));
    tiles.push(detail(
        Icon::Sun,
        "UV index",
        value(uv_value, band.map_or("", UvBand::name)),
        uv_lines,
        uv_picture,
        "uv",
    ));

    // Wind.
    let wind_value = n.wind_speed.map_or_else(dash, |s| units.speed_value(s));
    let mut wind_lines = Vec::new();
    if let Some(d) = n.wind_direction {
        wind_lines.push(format!("From the {}", compass_name(weather::compass(d))));
    }
    if let Some(g) = n.wind_gusts {
        wind_lines.push(format!("Gusts {} {}", units.speed_value(g), units.speed_unit()));
    }
    let wind = container(
        row![
            column![caption_row(Icon::Wind, "Wind"), value(wind_value, units.speed_unit())]
                .push(
                    aizome_kit::widgets::layout::column(
                        wind_lines.into_iter().map(|l| text(l).size(12.0).role(Role::Text2).into()).collect()
                    )
                    .spacing(6.0)
                )
                .spacing(6.0)
                .width(Fill),
            compass(n.wind_direction),
        ]
        .align(Align::Start)
        .spacing(8.0)
        .width(Fill),
    )
    .padding([12.0, 14.0])
    .fill(Tone::Chrome)
    .border(Edge::Line)
    .bevel()
    .width(Fill)
    .height(Fill)
    .min_height(132.0)
    .key("wind");
    tiles.push(wind.into());

    // Humidity.
    let hum_value = n.humidity.map_or_else(dash, |h| format!("{h:.0}%"));
    let dew = n.dew_point.map(|d| format!("Dew point {}", units.degrees(d))).unwrap_or_default();
    let hum_picture = n.humidity.map(|h| gauge(Gauge::Fill(h / 100.0)));
    tiles.push(detail(Icon::Droplets, "Humidity", value(hum_value, ""), vec![dew], hum_picture, "humidity"));

    // Sunrise and sunset.
    let local = f.local(now);
    let (rise, set) = (today.and_then(|d| d.sunrise), today.and_then(|d| d.sunset));
    let sun = sun_times(rise, set, local, f, clock, now);
    tiles.push(detail(sun.icon, sun.title, value(sun.value, sun.unit), sun.lines, Some(sun_arc(sun.at)), "sun"));

    // Precipitation.
    let today_sum = today.and_then(|d| d.precipitation_sum);
    let next = f.precipitation_next(now, 24);
    let next_line = if next > 0.0 {
        format!("{} expected in the next 24 h", units.precipitation(next))
    } else {
        "None expected in the next 24 h".to_owned()
    };
    let chance =
        today.and_then(|d| d.precipitation_probability).map(|p| format!("{p:.0}% chance today")).unwrap_or_default();
    tiles.push(detail(
        Icon::Umbrella,
        "Precipitation",
        value(today_sum.map_or_else(dash, |s| units.precipitation(s)), "today"),
        vec![next_line, chance],
        None,
        "precipitation",
    ));
    tiles
}

struct SunTimes {
    icon: Icon,
    title: &'static str,
    value: String,
    /// Beside the value: "tomorrow".
    unit: &'static str,
    lines: Vec<String>,
    at: Option<f64>,
}

/// The sun tile: the next of sunrise and sunset large, the other below,
/// the day's length, and how far through it the sun is.
fn sun_times(
    rise: Option<LocalTime>,
    set: Option<LocalTime>,
    local: LocalTime,
    f: &Forecast,
    clock: Clock,
    now: i64,
) -> SunTimes {
    let (Some(r), Some(s)) = (rise, set) else {
        return SunTimes {
            icon: Icon::Sunrise,
            title: "Sunrise",
            value: "—".into(),
            unit: "",
            lines: vec!["No sunrise or sunset today here.".into()],
            at: None,
        };
    };
    let length = s.seconds() - r.seconds();
    let at = (local.seconds() - r.seconds()) as f64 / length.max(1) as f64;
    let daylight = format!("{} of daylight", time::duration(length / 60));
    if local < r {
        SunTimes {
            icon: Icon::Sunrise,
            title: "Sunrise",
            value: clock.time(r),
            unit: "",
            lines: vec![format!("Sunset {}", clock.time(s)), daylight],
            at: Some(at),
        }
    } else if local < s {
        SunTimes {
            icon: Icon::Sunset,
            title: "Sunset",
            value: clock.time(s),
            unit: "",
            lines: vec![format!("Sunrise {}", clock.time(r)), daylight],
            at: Some(at),
        }
    } else {
        let tomorrow = f.days_from(now).get(1).and_then(|d| d.sunrise);
        SunTimes {
            icon: Icon::Sunrise,
            title: "Sunrise",
            value: tomorrow.map_or("—".into(), |t| clock.time(t)),
            unit: "tomorrow",
            lines: vec![format!("Sunset was at {}", clock.time(s)), daylight],
            at: Some(at),
        }
    }
}

fn compass_name(point: &str) -> &str {
    match point {
        "N" => "north",
        "E" => "east",
        "S" => "south",
        "W" => "west",
        "NE" => "northeast",
        "NW" => "northwest",
        "SE" => "southeast",
        "SW" => "southwest",
        other => other,
    }
}

// Settings.

/// The Settings sheet (❖,): units, and whether to use the location.
pub fn settings(app: &Tenki) -> Element<Msg> {
    let chosen = match app.settings.units {
        None => 0,
        Some(Units::Metric) => 1,
        Some(Units::Imperial) => 2,
    };
    let locale = match Units::from_locale() {
        Units::Metric => "metric",
        Units::Imperial => "imperial",
    };
    let units = dropdown(
        [format!("As the locale ({locale})"), Units::Metric.label().to_owned(), Units::Imperial.label().to_owned()],
        Some(chosen),
    )
    .on_select(Msg::UnitsChoice);
    let allowed = app.settings.location == Consent::Allowed;
    let location = toggle(allowed).on_toggle(|on| if on { Msg::AllowLocation } else { Msg::DeclineLocation });
    column![
        text("Tenki Settings").size(15.0).semibold(),
        form_section(
            "Units",
            vec![form_row("Temperature, wind and rain", units).description("Switching needs no new forecast.").into()]
        ),
        form_section(
            "Location",
            vec![
                form_row("Show my location", location)
                    .description("GeoClue finds the nearest town; Open-Meteo gets the position rounded to 0.01°.")
                    .icon(Icon::LocateFixed)
                    .into()
            ]
        ),
        row![spacer(), button("Done").primary().default().cancel().on_press(Msg::CloseSettings)].width(Fill),
    ]
    .spacing(14.0)
    .padding(20.0)
    .width(460.0)
    .into()
}
