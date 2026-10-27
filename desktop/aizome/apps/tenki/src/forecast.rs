//! A forecast, as Open-Meteo's forecast API gives it: current conditions,
//! hourly for 48 hours and daily for 10 days, in metric, with times on the
//! place's own clock (`timezone=auto`).
//!
//! Open-Meteo leaves out what a model doesn't have (a `null` in a series,
//! or a series missing), so every value but the time is optional here and
//! the views show a dash for what's missing.

use serde::Deserialize;

use crate::time::LocalTime;

/// Where forecasts come from.
pub const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

const CURRENT: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,dew_point_2m,is_day,precipitation,weather_code,wind_speed_10m,wind_direction_10m,wind_gusts_10m,uv_index";
const HOURLY: &str = "temperature_2m,apparent_temperature,precipitation_probability,precipitation,weather_code,wind_speed_10m,relative_humidity_2m,uv_index,is_day";
const DAILY: &str = "weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum,precipitation_probability_max,sunrise,sunset,uv_index_max,wind_speed_10m_max,wind_direction_10m_dominant";

/// Hours of the hourly forecast.
pub const HOURS: usize = 48;
/// Days of the daily forecast.
pub const DAYS: usize = 10;

/// The query for a forecast at `lat`, `lon` (rounded to 0.01°, about a
/// kilometre, which is finer than the models and keeps a precise position
/// from leaving the machine).
pub fn query(lat: f64, lon: f64) -> Vec<(&'static str, String)> {
    vec![
        ("latitude", format!("{:.2}", round2(lat))),
        ("longitude", format!("{:.2}", round2(lon))),
        ("current", CURRENT.into()),
        ("hourly", HOURLY.into()),
        ("daily", DAILY.into()),
        ("timezone", "auto".into()),
        ("forecast_days", DAYS.to_string()),
        ("forecast_hours", HOURS.to_string()),
    ]
}

/// A coordinate to 0.01°.
pub fn round2(v: f64) -> f64 {
    let r = (v * 100.0).round() / 100.0;
    if r == 0.0 { 0.0 } else { r }
}

/// The weather at a place: now, by the hour and by the day.
#[derive(Clone, Debug, PartialEq)]
pub struct Forecast {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
    /// IANA name: `Asia/Tokyo`.
    pub timezone: String,
    /// `JST`, or `GMT+9`.
    pub timezone_abbreviation: String,
    /// The place's offset from UTC when it was fetched, in seconds.
    pub utc_offset: i64,
    pub current: Current,
    pub hourly: Vec<Hour>,
    pub daily: Vec<Day>,
}

/// Conditions now (Open-Meteo's 15-minute data).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Current {
    pub time: LocalTime,
    pub temperature: Option<f64>,
    pub apparent: Option<f64>,
    pub humidity: Option<f64>,
    pub dew_point: Option<f64>,
    pub is_day: bool,
    pub precipitation: Option<f64>,
    pub code: Option<u8>,
    /// km/h.
    pub wind_speed: Option<f64>,
    /// Where the wind comes from, degrees clockwise from north.
    pub wind_direction: Option<f64>,
    pub wind_gusts: Option<f64>,
    pub uv: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hour {
    pub time: LocalTime,
    pub temperature: Option<f64>,
    pub apparent: Option<f64>,
    /// Percent.
    pub precipitation_probability: Option<f64>,
    /// mm in the hour before.
    pub precipitation: Option<f64>,
    pub code: Option<u8>,
    pub wind_speed: Option<f64>,
    pub humidity: Option<f64>,
    pub uv: Option<f64>,
    pub is_day: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Day {
    /// Midnight that day.
    pub date: LocalTime,
    pub code: Option<u8>,
    pub max: Option<f64>,
    pub min: Option<f64>,
    pub precipitation_sum: Option<f64>,
    pub precipitation_probability: Option<f64>,
    pub sunrise: Option<LocalTime>,
    pub sunset: Option<LocalTime>,
    pub uv_max: Option<f64>,
    pub wind_max: Option<f64>,
    pub wind_direction: Option<f64>,
}

// Open-Meteo's JSON.

#[derive(Deserialize)]
struct Raw {
    latitude: Option<f64>,
    longitude: Option<f64>,
    elevation: Option<f64>,
    timezone: Option<String>,
    timezone_abbreviation: Option<String>,
    utc_offset_seconds: Option<i64>,
    current: Option<RawCurrent>,
    #[serde(default)]
    hourly: RawHourly,
    #[serde(default)]
    daily: RawDaily,
    #[serde(default)]
    error: bool,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct RawCurrent {
    time: String,
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    relative_humidity_2m: Option<f64>,
    dew_point_2m: Option<f64>,
    is_day: Option<f64>,
    precipitation: Option<f64>,
    weather_code: Option<f64>,
    wind_speed_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
    wind_gusts_10m: Option<f64>,
    uv_index: Option<f64>,
}

type Series = Vec<Option<f64>>;

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawHourly {
    time: Vec<String>,
    temperature_2m: Series,
    apparent_temperature: Series,
    precipitation_probability: Series,
    precipitation: Series,
    weather_code: Series,
    wind_speed_10m: Series,
    relative_humidity_2m: Series,
    uv_index: Series,
    is_day: Series,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawDaily {
    time: Vec<String>,
    weather_code: Series,
    temperature_2m_max: Series,
    temperature_2m_min: Series,
    precipitation_sum: Series,
    precipitation_probability_max: Series,
    sunrise: Vec<Option<String>>,
    sunset: Vec<Option<String>>,
    uv_index_max: Series,
    wind_speed_10m_max: Series,
    wind_direction_10m_dominant: Series,
}

fn at(s: &Series, i: usize) -> Option<f64> {
    s.get(i).copied().flatten().filter(|v| v.is_finite())
}

fn code(v: Option<f64>) -> Option<u8> {
    v.filter(|c| (0.0..=255.0).contains(c)).map(|c| c as u8)
}

/// Read a forecast from Open-Meteo's answer.
pub fn parse(json: &str) -> Result<Forecast, String> {
    let raw: Raw = serde_json::from_str(json).map_err(|e| format!("Open-Meteo's answer didn't read: {e}"))?;
    if raw.error {
        return Err(raw.reason.unwrap_or_else(|| "Open-Meteo refused the request".into()));
    }
    let c = raw.current.ok_or("Open-Meteo's answer has no current conditions")?;
    let current = Current {
        time: LocalTime::parse(&c.time).ok_or("the current time didn't read")?,
        temperature: c.temperature_2m,
        apparent: c.apparent_temperature,
        humidity: c.relative_humidity_2m,
        dew_point: c.dew_point_2m,
        is_day: c.is_day.unwrap_or(1.0) >= 0.5,
        precipitation: c.precipitation,
        code: code(c.weather_code),
        wind_speed: c.wind_speed_10m,
        wind_direction: c.wind_direction_10m,
        wind_gusts: c.wind_gusts_10m,
        uv: c.uv_index,
    };
    let h = &raw.hourly;
    let hourly = h
        .time
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            Some(Hour {
                time: LocalTime::parse(t)?,
                temperature: at(&h.temperature_2m, i),
                apparent: at(&h.apparent_temperature, i),
                precipitation_probability: at(&h.precipitation_probability, i),
                precipitation: at(&h.precipitation, i),
                code: code(at(&h.weather_code, i)),
                wind_speed: at(&h.wind_speed_10m, i),
                humidity: at(&h.relative_humidity_2m, i),
                uv: at(&h.uv_index, i),
                is_day: at(&h.is_day, i).unwrap_or(1.0) >= 0.5,
            })
        })
        .collect();
    let d = &raw.daily;
    let time = |v: &Vec<Option<String>>, i: usize| v.get(i).cloned().flatten().and_then(|s| LocalTime::parse(&s));
    let daily = d
        .time
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            Some(Day {
                date: LocalTime::parse(t)?,
                code: code(at(&d.weather_code, i)),
                max: at(&d.temperature_2m_max, i),
                min: at(&d.temperature_2m_min, i),
                precipitation_sum: at(&d.precipitation_sum, i),
                precipitation_probability: at(&d.precipitation_probability_max, i),
                sunrise: time(&d.sunrise, i),
                sunset: time(&d.sunset, i),
                uv_max: at(&d.uv_index_max, i),
                wind_max: at(&d.wind_speed_10m_max, i),
                wind_direction: at(&d.wind_direction_10m_dominant, i),
            })
        })
        .collect();
    Ok(Forecast {
        latitude: raw.latitude.unwrap_or(0.0),
        longitude: raw.longitude.unwrap_or(0.0),
        elevation: raw.elevation,
        timezone: raw.timezone.unwrap_or_else(|| "GMT".into()),
        timezone_abbreviation: raw.timezone_abbreviation.unwrap_or_default(),
        utc_offset: raw.utc_offset_seconds.unwrap_or(0),
        current,
        hourly,
        daily,
    })
}

/// What the views show for "now": the current conditions while they're
/// fresh, else (an old forecast, offline) the forecast for this hour.
#[derive(Clone, Debug, PartialEq)]
pub struct Now {
    pub time: LocalTime,
    pub temperature: Option<f64>,
    pub apparent: Option<f64>,
    pub humidity: Option<f64>,
    pub dew_point: Option<f64>,
    pub is_day: bool,
    pub code: Option<u8>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub wind_gusts: Option<f64>,
    pub uv: Option<f64>,
    /// From the hourly forecast rather than observed conditions.
    pub forecast: bool,
}

impl Forecast {
    /// The place's wall clock at Unix time `now`.
    pub fn local(&self, now: i64) -> LocalTime {
        LocalTime::from_seconds(now + self.utc_offset)
    }

    /// The hours from the one under way at `now` on.
    pub fn hours_from(&self, now: i64) -> &[Hour] {
        let start = self.local(now).hour_start();
        let i = self.hourly.iter().position(|h| h.time >= start).unwrap_or(self.hourly.len());
        &self.hourly[i..]
    }

    /// The days from today (at the place) on.
    pub fn days_from(&self, now: i64) -> &[Day] {
        let today = self.local(now).date();
        let i = self.daily.iter().position(|d| d.date >= today).unwrap_or(self.daily.len());
        &self.daily[i..]
    }

    /// Today at the place, if the forecast still covers it.
    pub fn today(&self, now: i64) -> Option<&Day> {
        let today = self.local(now).date();
        self.daily.iter().find(|d| d.date == today)
    }

    /// The conditions to show at `now`, for a forecast fetched at
    /// `fetched_at`.
    pub fn now(&self, now: i64, fetched_at: i64) -> Now {
        let c = &self.current;
        let fresh = now - fetched_at < 90 * 60;
        let hour = self.hours_from(now).first().filter(|h| h.time.hour_start() == self.local(now).hour_start());
        match (fresh, hour) {
            (false, Some(h)) => Now {
                time: h.time,
                temperature: h.temperature,
                apparent: h.apparent,
                humidity: h.humidity,
                dew_point: None,
                is_day: h.is_day,
                code: h.code,
                wind_speed: h.wind_speed,
                wind_direction: self.today(now).and_then(|d| d.wind_direction),
                wind_gusts: None,
                uv: h.uv,
                forecast: true,
            },
            _ => Now {
                time: c.time,
                temperature: c.temperature,
                apparent: c.apparent,
                humidity: c.humidity,
                dew_point: c.dew_point,
                is_day: c.is_day,
                code: c.code,
                wind_speed: c.wind_speed,
                wind_direction: c.wind_direction,
                wind_gusts: c.wind_gusts,
                uv: c.uv.or_else(|| hour.and_then(|h| h.uv)),
                forecast: false,
            },
        }
    }

    /// The coldest and warmest of the days from `now`, for the ranges'
    /// common scale.
    pub fn span(&self, now: i64) -> Option<(f64, f64)> {
        let days = self.days_from(now);
        let lo = days.iter().filter_map(|d| d.min).fold(f64::INFINITY, f64::min);
        let hi = days.iter().filter_map(|d| d.max).fold(f64::NEG_INFINITY, f64::max);
        (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
    }

    /// Precipitation over the next `hours` hours from `now`, in mm.
    pub fn precipitation_next(&self, now: i64, hours: usize) -> f64 {
        self.hours_from(now).iter().take(hours).filter_map(|h| h.precipitation).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "latitude": 35.7, "longitude": 139.69, "elevation": 40.0,
        "utc_offset_seconds": 32400, "timezone": "Asia/Tokyo", "timezone_abbreviation": "GMT+9",
        "current": {"time": "2026-10-17T14:15", "interval": 900, "temperature_2m": 18.2, "apparent_temperature": 16.9,
            "relative_humidity_2m": 64, "dew_point_2m": 11.2, "is_day": 1, "precipitation": 0.0, "weather_code": 2,
            "wind_speed_10m": 12.4, "wind_direction_10m": 330, "wind_gusts_10m": 25.2, "uv_index": 3.1},
        "hourly": {"time": ["2026-10-17T14:00", "2026-10-17T15:00", "2026-10-17T16:00"],
            "temperature_2m": [18.2, 17.9, null], "precipitation_probability": [0, 10, 35],
            "weather_code": [2, 3, 61], "is_day": [1, 1, 0]},
        "daily": {"time": ["2026-10-17", "2026-10-18"], "weather_code": [61, 3],
            "temperature_2m_max": [21.0, 19.5], "temperature_2m_min": [12.1, 11.0],
            "sunrise": ["2026-10-17T05:52", "2026-10-18T05:53"], "sunset": ["2026-10-17T17:08", null]}
    }"#;

    // 2026-10-17T14:20 in Tokyo.
    const NOW: i64 = 1_792_214_400;

    #[test]
    fn reads_an_answer() {
        let f = parse(SAMPLE).unwrap();
        assert_eq!(f.timezone, "Asia/Tokyo");
        assert_eq!(f.current.time, LocalTime::new(2026, 10, 17, 14, 15));
        assert_eq!(f.current.code, Some(2));
        assert_eq!(f.hourly.len(), 3);
        assert_eq!(f.hourly[2].temperature, None);
        assert_eq!(f.hourly[2].precipitation_probability, Some(35.0));
        assert!(!f.hourly[2].is_day);
        assert_eq!(f.hourly[0].apparent, None, "a missing series is missing values");
        assert_eq!(f.daily[1].sunset, None);
        assert_eq!(f.daily[0].sunrise, Some(LocalTime::new(2026, 10, 17, 5, 52)));
    }

    #[test]
    fn refusals_say_why() {
        let e = parse(r#"{"error": true, "reason": "Latitude must be in range of -90 to 90°."}"#).unwrap_err();
        assert!(e.contains("Latitude"));
        assert!(parse("<html>").is_err());
    }

    #[test]
    fn now_moves_along_the_forecast() {
        let f = parse(SAMPLE).unwrap();
        assert_eq!(f.local(NOW), LocalTime::new(2026, 10, 17, 14, 20));
        assert_eq!(f.hours_from(NOW).len(), 3);
        assert_eq!(f.hours_from(NOW + 3600).len(), 2);
        let fresh = f.now(NOW, NOW - 300);
        assert!(!fresh.forecast);
        assert_eq!(fresh.temperature, Some(18.2));
        // Hours later, offline: this hour's forecast stands in.
        let old = f.now(NOW + 2 * 3600, NOW - 300);
        assert!(old.forecast);
        assert_eq!(old.code, Some(61));
        assert_eq!(f.days_from(NOW + 86_400).len(), 1);
        assert_eq!(f.span(NOW), Some((11.0, 21.0)));
    }

    #[test]
    fn queries_round_positions() {
        let q = query(35.689_47, 139.691_71);
        assert_eq!(q[0], ("latitude", "35.69".to_string()));
        assert_eq!(q[1], ("longitude", "139.69".to_string()));
        assert_eq!(round2(-0.001), 0.0);
    }
}
