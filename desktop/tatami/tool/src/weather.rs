// SPDX-License-Identifier: MIT OR Apache-2.0
//! The weather beside the clock, after Omarchy 4's weather widget (shell/
//! plugins/panels/weather; MIT, see ../../config/LICENSE): the current
//! conditions' icon in the bar and, on hover, a card with the temperature,
//! place, feels-like temperature, wind, humidity and the next three days.
//! As in Omarchy, wttr.in finds the place from the IP address unless one is
//! set (Setup › Weather, or a right click), and Open-Meteo has the conditions
//! and forecast. Units follow the place's country, then the locale. Reports
//! are cached for 15 minutes; Waybar asks every minute.
use crate::util;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub const SIGNAL: i32 = 7;
/// Fetch again after this long.
const FRESH: u64 = 15 * 60;
/// Stop showing a report this old (offline for hours).
const STALE: u64 = 6 * 60 * 60;

const DIM: &str = "#565f89";
const RULE: &str = "#3b4261";
const ACCENT: &str = "#7aa2f7";
/// Characters per column in the card.
const COLUMN: usize = 12;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_file() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|dir| dir.starts_with('/'))
        .unwrap_or_else(|| format!("{}/.cache", util::home()));
    PathBuf::from(base).join("tatami/weather.json")
}

fn place_file() -> PathBuf {
    util::state_dir().join("weather-place.json")
}

fn write(path: &PathBuf, value: &Value) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, value.to_string());
}

fn read(path: &PathBuf) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn get(url: &str) -> Option<Value> {
    let body = util::output("curl", &["-fsS", "--max-time", "5", url]).ok()?;
    serde_json::from_str(&body).ok()
}

/// A number that wttr.in sends as a string and Open-Meteo as a number.
fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

/// Omarchy's icons for wttr.in's (WWO) condition codes.
pub fn wttr_glyph(code: i64, night: bool) -> &'static str {
    match code {
        113 if night => "\u{e32b}",
        113 => "\u{e30d}",
        116 if night => "\u{e32e}",
        116 => "\u{e302}",
        143 | 248 | 260 if night => "\u{e346}",
        143 | 248 | 260 => "\u{e313}",
        176 | 263 | 353 if night => "\u{e333}",
        176 | 263 | 353 => "\u{e308}",
        179 | 227 | 230 | 323 | 326 | 368 if night => "\u{e327}",
        179 | 227 | 230 | 323 | 326 | 368 => "\u{e30a}",
        182 | 185 | 281 | 284 | 311 | 314 | 317 | 320 | 350 | 362 | 365 | 374 | 377 => "\u{e3ad}",
        200 | 386 | 389 | 392 | 395 => "\u{e31d}",
        266 | 293 | 296 | 299 | 302 | 305 | 308 | 356 | 359 => "\u{e318}",
        329 | 332 | 335 | 338 | 371 => "\u{e31a}",
        _ => "\u{e33d}",
    }
}

/// Open-Meteo's (WMO) codes, by way of the wttr.in code Omarchy maps them to.
pub fn wmo_glyph(code: i64, night: bool) -> &'static str {
    let wttr = match code {
        0 => 113,
        1 | 2 => 116,
        45 | 48 => 143,
        51 | 53 | 55 | 56 | 57 | 61 => 266,
        63 | 65 | 66 | 67 | 80 | 81 | 82 => 308,
        71 | 73 | 75 | 77 | 85 | 86 => 338,
        95 | 96 | 99 => 389,
        _ => 119,
    };
    wttr_glyph(wttr, night)
}

/// The United States, Liberia and Myanmar use Fahrenheit and miles; an
/// unknown country leaves it to the locale.
pub fn country_imperial(country: &str) -> Option<bool> {
    let country = country.trim().to_lowercase().replace(['.', '_', '-'], " ");
    if country.is_empty() {
        return None;
    }
    Some(matches!(
        country.as_str(),
        "us" | "usa"
            | "united states"
            | "united states of america"
            | "liberia"
            | "myanmar"
            | "burma"
    ))
}

pub fn locale_imperial(locale: &str) -> bool {
    let locale = locale.replace('.', "_");
    ["en_US", "en-US", "en_LR", "en-LR"].iter().any(|prefix| {
        locale
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['_', '.', '-', '@']))
    }) || locale == "my"
        || locale.starts_with("my_")
}

fn locale() -> String {
    ["LC_ALL", "LC_MEASUREMENT", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

/// Day of the week of a YYYY-MM-DD date (Sakamoto's method).
pub fn weekday(date: &str) -> Option<&'static str> {
    let mut parts = date.get(..10)?.split('-');
    let (mut y, m, d): (i64, usize, i64) = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if !(1..=12).contains(&m) {
        return None;
    }
    if m < 3 {
        y -= 1;
    }
    let day = (y + y / 4 - y / 100 + y / 400 + T[m - 1] + d).rem_euclid(7);
    Some(
        [
            "SUNDAY",
            "MONDAY",
            "TUESDAY",
            "WEDNESDAY",
            "THURSDAY",
            "FRIDAY",
            "SATURDAY",
        ][day as usize],
    )
}

/// A query string value.
pub fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
}

/// Current conditions and the next three days from Open-Meteo, in metric.
pub fn from_open_meteo(forecast: &Value) -> Option<(Value, Vec<Value>)> {
    let current = &forecast["current"];
    let night = number(&current["is_day"]) == Some(0.0);
    let code = number(&current["weather_code"]).unwrap_or(3.0) as i64;
    let now = json!({
        "temp": number(&current["temperature_2m"])?,
        "feels": number(&current["apparent_temperature"])?,
        "humidity": number(&current["relative_humidity_2m"])?,
        "wind": number(&current["wind_speed_10m"])?,
        "glyph": wmo_glyph(code, night),
    });
    let daily = &forecast["daily"];
    let days = (1..4)
        .filter_map(|i| {
            Some(json!({
                "date": daily["time"][i].as_str()?,
                "max": number(&daily["temperature_2m_max"][i])?,
                "min": number(&daily["temperature_2m_min"][i])?,
                "glyph": wmo_glyph(number(&daily["weather_code"][i]).unwrap_or(3.0) as i64, false),
            }))
        })
        .collect();
    Some((now, days))
}

/// The same from wttr.in, when Open-Meteo does not answer: today and the
/// following two days, with each day's icon at noon.
pub fn from_wttr(report: &Value) -> Option<(Value, Vec<Value>)> {
    let current = &report["current_condition"][0];
    let now = json!({
        "temp": number(&current["temp_C"])?,
        "feels": number(&current["FeelsLikeC"])?,
        "humidity": number(&current["humidity"])?,
        "wind": number(&current["windspeedKmph"])?,
        "glyph": wttr_glyph(number(&current["weatherCode"]).unwrap_or(119.0) as i64, false),
    });
    let days = report["weather"]
        .as_array()
        .into_iter()
        .flatten()
        .skip(1)
        .take(3)
        .filter_map(|day| {
            let noon = day["hourly"]
                .as_array()?
                .iter()
                .min_by_key(|hour| (number(&hour["time"]).unwrap_or(0.0) as i64 - 1200).abs())?;
            Some(json!({
                "date": day["date"].as_str()?,
                "max": number(&day["maxtempC"])?,
                "min": number(&day["mintempC"])?,
                "glyph": wttr_glyph(number(&noon["weatherCode"]).unwrap_or(119.0) as i64, false),
            }))
        })
        .collect();
    Some((now, days))
}

/// A new report for the saved place, or for wherever the IP address is.
fn fetch() -> Option<Value> {
    let saved = read(&place_file());
    let (wttr, name, country, latitude, longitude) = match &saved {
        Some(place) => (
            None,
            place["name"].as_str().unwrap_or_default().to_owned(),
            place["country"].as_str().unwrap_or_default().to_owned(),
            number(&place["latitude"]),
            number(&place["longitude"]),
        ),
        None => {
            let report = get("https://wttr.in/?format=j1");
            let area = report.as_ref().map(|r| r["nearest_area"][0].clone());
            let text = |key: &str| {
                area.as_ref()
                    .and_then(|a| a[key][0]["value"].as_str())
                    .unwrap_or_default()
                    .to_owned()
            };
            let (name, country) = (text("areaName"), text("country"));
            let latitude = area.as_ref().and_then(|a| number(&a["latitude"]));
            let longitude = area.as_ref().and_then(|a| number(&a["longitude"]));
            (report, name, country, latitude, longitude)
        }
    };
    let open_meteo = latitude.zip(longitude).and_then(|(lat, lon)| {
        get(&format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
             &daily=weather_code,temperature_2m_max,temperature_2m_min\
             &current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code,is_day\
             &forecast_days=4&timezone=auto"
        ))
    });
    let (current, days) = open_meteo
        .as_ref()
        .and_then(from_open_meteo)
        .or_else(|| wttr.as_ref().and_then(from_wttr))?;
    Some(json!({
        "fetched": now(),
        "place": name,
        "imperial": country_imperial(&country).unwrap_or_else(|| locale_imperial(&locale())),
        "current": current,
        "days": days,
    }))
}

fn degrees(celsius: f64, imperial: bool) -> i64 {
    if imperial {
        (celsius * 9.0 / 5.0 + 32.0).round() as i64
    } else {
        celsius.round() as i64
    }
}

fn speed(kmh: f64, imperial: bool) -> String {
    if imperial {
        format!("{} mph", (kmh * 0.621_371).round() as i64)
    } else {
        format!("{} km/h", kmh.round() as i64)
    }
}

/// A weather icon. The Nerd Font's weather icons are all two characters
/// wide, which keeps the card's columns lined up.
fn icon(glyph: &str, size: &str) -> String {
    format!("<span font_family='JetBrainsMono Nerd Font' size='{size}'>{glyph}</span>")
}

/// Cells padded to the column width; `widths` are their visible lengths
/// (markup excluded).
fn row(cells: &[(String, usize)]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(i, (text, width))| {
            if i + 1 == cells.len() {
                text.clone()
            } else {
                format!("{text}{}", " ".repeat(COLUMN.saturating_sub(*width).max(1)))
            }
        })
        .collect()
}

fn plain(text: &str) -> (String, usize) {
    (escape(text), text.chars().count())
}

/// The hover card, in Pango markup.
pub fn card(report: &Value) -> Option<String> {
    let imperial = report["imperial"].as_bool().unwrap_or(false);
    let unit = if imperial { "F" } else { "C" };
    let current = &report["current"];
    let glyph = current["glyph"].as_str().unwrap_or("\u{e33d}");
    let temp = degrees(number(&current["temp"])?, imperial);
    let feels = format!("{}°{unit}", degrees(number(&current["feels"])?, imperial));
    let wind = speed(number(&current["wind"])?, imperial);
    let humidity = format!("{}%", number(&current["humidity"])?.round() as i64);
    let place = report["place"].as_str().unwrap_or_default().to_uppercase();
    let mut lines = vec![
        format!(
            "{}  <span size='300%' weight='bold'>{temp}</span><span size='150%' rise='14pt'>°{unit}</span>",
            icon(glyph, "300%")
        ),
        if place.is_empty() {
            String::new()
        } else {
            format!(
                "<span color='{ACCENT}'>\u{f041}</span> <span color='{DIM}'>{}</span>",
                escape(&place)
            )
        },
        String::new(),
        format!(
            "<span color='{DIM}'>{}</span>",
            row(&[plain("FEELS"), plain("WIND"), plain("HUMID")])
        ),
        row(&[plain(&feels), plain(&wind), plain(&humidity)]),
    ];
    let days = report["days"].as_array().cloned().unwrap_or_default();
    if !days.is_empty() {
        lines.push(format!(
            "<span color='{RULE}'>{}</span>",
            "\u{2500}".repeat(COLUMN * 2 + 10)
        ));
        let names: Vec<(String, usize)> = days
            .iter()
            .map(|day| plain(day["date"].as_str().and_then(weekday).unwrap_or("")))
            .collect();
        lines.push(format!("<span color='{DIM}'>{}</span>", row(&names)));
        let temps: Vec<(String, usize)> = days
            .iter()
            .filter_map(|day| {
                let high = format!("{}°", degrees(number(&day["max"])?, imperial));
                let low = format!("{}°", degrees(number(&day["min"])?, imperial));
                let width = 3 + high.chars().count() + 1 + low.chars().count();
                let text = format!(
                    "{} {high} <span color='{DIM}'>{low}</span>",
                    icon(day["glyph"].as_str().unwrap_or("\u{e33d}"), "medium")
                );
                Some((text, width))
            })
            .collect();
        lines.push(row(&temps));
    }
    Some(lines.join("\n"))
}

/// What Waybar shows: the icon, or an empty slot (which keeps the clock
/// centered) while there is no report.
pub fn bar_json(report: Option<&Value>, now: u64) -> String {
    let usable = report.filter(|r| {
        number(&r["fetched"]).is_some_and(|fetched| now.saturating_sub(fetched as u64) < STALE)
    });
    match usable.and_then(|r| Some((r, card(r)?))) {
        Some((report, card)) => json!({
            "text": report["current"]["glyph"].as_str().unwrap_or("\u{e33d}"),
            "tooltip": card,
            "class": "weather",
        }),
        None => json!({ "text": " ", "tooltip": "Weather unavailable", "class": "empty" }),
    }
    .to_string()
}

/// `tatami weather`: refresh when the report is old, then print it.
pub fn bar() -> String {
    let cached = read(&cache_file());
    let fresh = cached
        .as_ref()
        .and_then(|r| number(&r["fetched"]))
        .is_some_and(|fetched| now().saturating_sub(fetched as u64) < FRESH);
    let report = if fresh {
        cached
    } else {
        match fetch() {
            Some(report) => {
                write(&cache_file(), &report);
                Some(report)
            }
            None => cached,
        }
    };
    bar_json(report.as_ref(), now())
}

/// A click on the icon: fetch now.
pub fn refresh() {
    match fetch() {
        Some(report) => write(&cache_file(), &report),
        None => util::notify(
            "\u{e33d}",
            "Weather unavailable",
            "Check the network connection",
        ),
    }
    util::signal_waybar(SIGNAL);
}

/// Setup › Weather and a right click: a place by name, or empty for
/// wherever the IP address is.
pub fn choose_place() {
    crate::menu::after_menu();
    let Ok(text) = crate::menu::walker_filter(
        &[
            "--dmenu",
            "--inputonly",
            "--theme",
            crate::menu::PANEL_THEME,
            "-p",
            "Weather place (empty: automatic)\u{2026}",
        ],
        b"",
    ) else {
        return;
    };
    let name = text.trim();
    if name.is_empty() {
        let _ = fs::remove_file(place_file());
        util::notify(
            "\u{e33d}",
            "Weather place: automatic",
            "Found from the IP address",
        );
    } else {
        let results = get(&format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=5&language=en&format=json",
            encode(name)
        ))
        .and_then(|r| r["results"].as_array().cloned())
        .unwrap_or_default();
        let label = |place: &Value| {
            ["name", "admin1", "country"]
                .iter()
                .filter_map(|key| place[*key].as_str())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let place = match results.len() {
            0 => {
                util::notify("\u{e33d}", "No such place", name);
                return;
            }
            1 => results[0].clone(),
            _ => {
                crate::menu::after_menu();
                let labels: Vec<String> = results.iter().map(label).collect();
                let Some(index) = crate::menu::panel_index("Weather place", &labels, Some(0))
                else {
                    return;
                };
                results[index].clone()
            }
        };
        write(
            &place_file(),
            &json!({
                "name": place["name"],
                "country": place["country"],
                "latitude": place["latitude"],
                "longitude": place["longitude"],
            }),
        );
        util::notify("\u{e33d}", "Weather place", &label(&place));
    }
    let _ = fs::remove_file(cache_file());
    refresh();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_follow_omarchy() {
        assert_eq!(wttr_glyph(113, false), "\u{e30d}");
        assert_eq!(wttr_glyph(113, true), "\u{e32b}");
        assert_eq!(wttr_glyph(389, true), "\u{e31d}");
        assert_eq!(wttr_glyph(999, false), "\u{e33d}");
        assert_eq!(wmo_glyph(0, true), "\u{e32b}");
        assert_eq!(wmo_glyph(2, false), "\u{e302}");
        assert_eq!(wmo_glyph(81, false), "\u{e318}");
        assert_eq!(wmo_glyph(86, false), "\u{e31a}");
    }

    #[test]
    fn units_follow_country_then_locale() {
        assert_eq!(country_imperial("United States of America"), Some(true));
        assert_eq!(country_imperial("Germany"), Some(false));
        assert_eq!(country_imperial(""), None);
        assert!(locale_imperial("en_US.UTF-8"));
        assert!(!locale_imperial("en_GB.UTF-8"));
        assert!(!locale_imperial("en_USX"));
        assert!(locale_imperial("my_MM"));
    }

    #[test]
    fn weekdays_and_encoding() {
        assert_eq!(weekday("2026-10-07"), Some("WEDNESDAY"));
        assert_eq!(weekday("2000-02-29"), Some("TUESDAY"));
        assert_eq!(weekday("nonsense"), None);
        assert_eq!(encode("São Paulo"), "S%C3%A3o%20Paulo");
    }

    #[test]
    fn reports_from_both_sources() {
        let forecast = json!({
            "current": {"temperature_2m": 30.4, "apparent_temperature": 27.8,
                        "relative_humidity_2m": 28, "wind_speed_10m": 12.9,
                        "weather_code": 0, "is_day": 0},
            "daily": {"time": ["2026-10-06", "2026-10-07", "2026-10-08", "2026-10-09"],
                      "weather_code": [0, 0, 3, 61],
                      "temperature_2m_max": [31, 32.2, 33, 33],
                      "temperature_2m_min": [18, 18.3, 17, 24]}
        });
        let (now, days) = from_open_meteo(&forecast).unwrap();
        assert_eq!(now["glyph"], "\u{e32b}");
        assert_eq!(days.len(), 3);
        assert_eq!(days[0]["date"], "2026-10-07");
        let wttr = json!({
            "current_condition": [{"temp_C": "30", "FeelsLikeC": "28", "humidity": "28",
                                   "windspeedKmph": "13", "weatherCode": "116"}],
            "weather": [
                {"date": "2026-10-06", "maxtempC": "31", "mintempC": "18", "hourly": []},
                {"date": "2026-10-07", "maxtempC": "32", "mintempC": "18",
                 "hourly": [{"time": "900", "weatherCode": "113"}, {"time": "1200", "weatherCode": "176"}]}
            ]
        });
        let (now, days) = from_wttr(&wttr).unwrap();
        assert_eq!(now["temp"], 30.0);
        assert_eq!(days.len(), 1);
        assert_eq!(days[0]["glyph"], "\u{e308}");
    }

    #[test]
    fn card_and_bar() {
        let report = json!({
            "fetched": 1000, "place": "Austin & Co", "imperial": true,
            "current": {"temp": 30.4, "feels": 27.8, "humidity": 28, "wind": 12.9, "glyph": "\u{e32b}"},
            "days": [{"date": "2026-10-07", "max": 32.2, "min": 18.3, "glyph": "\u{e30d}"}]
        });
        let card = card(&report).unwrap();
        assert!(card.contains(">87</span>"));
        assert!(card.contains("AUSTIN &amp; CO"));
        assert!(card.contains("82°F") && card.contains("8 mph") && card.contains("28%"));
        assert!(card.contains("WEDNESDAY") && card.contains("90°") && card.contains("65°"));
        let bar: Value = serde_json::from_str(&bar_json(Some(&report), 1000 + 60)).unwrap();
        assert_eq!(bar["text"], "\u{e32b}");
        let old: Value = serde_json::from_str(&bar_json(Some(&report), 1000 + STALE)).unwrap();
        assert_eq!(old["class"], "empty");
        assert_eq!(old["text"], " ");
    }
}
