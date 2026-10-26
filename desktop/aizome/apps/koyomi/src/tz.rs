//! Time zones named by `TZID`s as real files name them: IANA names
//! (with or without a leading `/` or a vendor prefix), Windows names
//! from Outlook and Exchange, and `VTIMEZONE` definitions of the file's
//! own, read as POSIX rules (or a fixed offset) when nothing else fits.

use std::collections::HashMap;
use std::sync::Arc;

use jiff::tz::{Offset, TimeZone};

use crate::ics::{Component, value};

/// A zone as an event names it: its `TZID`, and what that resolved to
/// (none: an unknown zone, read as floating time).
#[derive(Clone, Debug)]
pub struct Tz {
    pub id: Arc<str>,
    pub zone: Option<TimeZone>,
}

impl PartialEq for Tz {
    fn eq(&self, other: &Tz) -> bool {
        self.id == other.id
    }
}

impl Tz {
    /// The zone `id` names in the IANA database.
    pub fn iana(id: &str) -> Tz {
        Tz { id: id.into(), zone: TimeZone::get(id).ok() }
    }

    /// The IANA name, when the zone is one.
    pub fn is_iana(&self) -> bool {
        TimeZone::get(&self.id).is_ok()
    }
}

/// The zone this computer is in, with its IANA name if it has one.
pub fn system() -> Tz {
    named(TimeZone::system())
}

/// A zone with its IANA name, found where jiff couldn't find it (an
/// `/etc/localtime` that links through other links into a zoneinfo
/// folder, as on NixOS and in containers). Without a name, its id is
/// empty and new events made in it are floating.
pub fn named(zone: TimeZone) -> Tz {
    if let Some(name) = zone.iana_name() {
        return Tz { id: name.into(), zone: Some(zone.clone()) };
    }
    if std::env::var_os("TZ").is_none()
        && let Ok(path) = std::fs::canonicalize("/etc/localtime")
        && let Some((_, name)) = path.to_string_lossy().split_once("/zoneinfo/")
        && let Ok(z) = TimeZone::get(name)
    {
        return Tz { id: name.into(), zone: Some(z) };
    }
    Tz { id: "".into(), zone: Some(zone) }
}

/// Windows zone names (CLDR's `windowsZones`, territory 001) to IANA.
const WINDOWS: &[(&str, &str)] = &[
    ("Dateline Standard Time", "Etc/GMT+12"),
    ("UTC-11", "Etc/GMT+11"),
    ("Aleutian Standard Time", "America/Adak"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Marquesas Standard Time", "Pacific/Marquesas"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("UTC-09", "Etc/GMT+9"),
    ("Pacific Standard Time (Mexico)", "America/Tijuana"),
    ("UTC-08", "Etc/GMT+8"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Mountain Standard Time (Mexico)", "America/Mazatlan"),
    ("Mountain Standard Time", "America/Denver"),
    ("Yukon Standard Time", "America/Whitehorse"),
    ("Central America Standard Time", "America/Guatemala"),
    ("Central Standard Time", "America/Chicago"),
    ("Easter Island Standard Time", "Pacific/Easter"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("Canada Central Standard Time", "America/Regina"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Eastern Standard Time (Mexico)", "America/Cancun"),
    ("Eastern Standard Time", "America/New_York"),
    ("Haiti Standard Time", "America/Port-au-Prince"),
    ("Cuba Standard Time", "America/Havana"),
    ("US Eastern Standard Time", "America/Indiana/Indianapolis"),
    ("Turks And Caicos Standard Time", "America/Grand_Turk"),
    ("Paraguay Standard Time", "America/Asuncion"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Venezuela Standard Time", "America/Caracas"),
    ("Central Brazilian Standard Time", "America/Cuiaba"),
    ("SA Western Standard Time", "America/La_Paz"),
    ("Pacific SA Standard Time", "America/Santiago"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("Tocantins Standard Time", "America/Araguaina"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("SA Eastern Standard Time", "America/Cayenne"),
    ("Argentina Standard Time", "America/Argentina/Buenos_Aires"),
    ("Greenland Standard Time", "America/Nuuk"),
    ("Montevideo Standard Time", "America/Montevideo"),
    ("Magallanes Standard Time", "America/Punta_Arenas"),
    ("Saint Pierre Standard Time", "America/Miquelon"),
    ("Bahia Standard Time", "America/Bahia"),
    ("UTC-02", "Etc/GMT+2"),
    ("Azores Standard Time", "Atlantic/Azores"),
    ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
    ("UTC", "Etc/UTC"),
    ("Coordinated Universal Time", "Etc/UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("Sao Tome Standard Time", "Africa/Sao_Tome"),
    ("Morocco Standard Time", "Africa/Casablanca"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("Jordan Standard Time", "Asia/Amman"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Middle East Standard Time", "Asia/Beirut"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("Syria Standard Time", "Asia/Damascus"),
    ("West Bank Standard Time", "Asia/Hebron"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("FLE Standard Time", "Europe/Kyiv"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("South Sudan Standard Time", "Africa/Juba"),
    ("Kaliningrad Standard Time", "Europe/Kaliningrad"),
    ("Sudan Standard Time", "Africa/Khartoum"),
    ("Libya Standard Time", "Africa/Tripoli"),
    ("Namibia Standard Time", "Africa/Windhoek"),
    ("Arabic Standard Time", "Asia/Baghdad"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("Belarus Standard Time", "Europe/Minsk"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("E. Africa Standard Time", "Africa/Nairobi"),
    ("Volgograd Standard Time", "Europe/Volgograd"),
    ("Iran Standard Time", "Asia/Tehran"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Astrakhan Standard Time", "Europe/Astrakhan"),
    ("Azerbaijan Standard Time", "Asia/Baku"),
    ("Russia Time Zone 3", "Europe/Samara"),
    ("Mauritius Standard Time", "Indian/Mauritius"),
    ("Saratov Standard Time", "Europe/Saratov"),
    ("Georgian Standard Time", "Asia/Tbilisi"),
    ("Caucasus Standard Time", "Asia/Yerevan"),
    ("Afghanistan Standard Time", "Asia/Kabul"),
    ("West Asia Standard Time", "Asia/Tashkent"),
    ("Ekaterinburg Standard Time", "Asia/Yekaterinburg"),
    ("Pakistan Standard Time", "Asia/Karachi"),
    ("Qyzylorda Standard Time", "Asia/Qyzylorda"),
    ("India Standard Time", "Asia/Kolkata"),
    ("Sri Lanka Standard Time", "Asia/Colombo"),
    ("Nepal Standard Time", "Asia/Kathmandu"),
    ("Central Asia Standard Time", "Asia/Almaty"),
    ("Bangladesh Standard Time", "Asia/Dhaka"),
    ("Omsk Standard Time", "Asia/Omsk"),
    ("Myanmar Standard Time", "Asia/Yangon"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("Altai Standard Time", "Asia/Barnaul"),
    ("W. Mongolia Standard Time", "Asia/Hovd"),
    ("North Asia Standard Time", "Asia/Krasnoyarsk"),
    ("N. Central Asia Standard Time", "Asia/Novosibirsk"),
    ("Tomsk Standard Time", "Asia/Tomsk"),
    ("China Standard Time", "Asia/Shanghai"),
    ("North Asia East Standard Time", "Asia/Irkutsk"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Ulaanbaatar Standard Time", "Asia/Ulaanbaatar"),
    ("Aus Central W. Standard Time", "Australia/Eucla"),
    ("Transbaikal Standard Time", "Asia/Chita"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("North Korea Standard Time", "Asia/Pyongyang"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("Yakutsk Standard Time", "Asia/Yakutsk"),
    ("Cen. Australia Standard Time", "Australia/Adelaide"),
    ("AUS Central Standard Time", "Australia/Darwin"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("West Pacific Standard Time", "Pacific/Port_Moresby"),
    ("Tasmania Standard Time", "Australia/Hobart"),
    ("Vladivostok Standard Time", "Asia/Vladivostok"),
    ("Lord Howe Standard Time", "Australia/Lord_Howe"),
    ("Bougainville Standard Time", "Pacific/Bougainville"),
    ("Russia Time Zone 10", "Asia/Srednekolymsk"),
    ("Magadan Standard Time", "Asia/Magadan"),
    ("Norfolk Standard Time", "Pacific/Norfolk"),
    ("Sakhalin Standard Time", "Asia/Sakhalin"),
    ("Central Pacific Standard Time", "Pacific/Guadalcanal"),
    ("Russia Time Zone 11", "Asia/Kamchatka"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("UTC+12", "Etc/GMT-12"),
    ("Fiji Standard Time", "Pacific/Fiji"),
    ("Chatham Islands Standard Time", "Pacific/Chatham"),
    ("UTC+13", "Etc/GMT-13"),
    ("Tonga Standard Time", "Pacific/Tongatapu"),
    ("Samoa Standard Time", "Pacific/Apia"),
    ("Line Islands Standard Time", "Pacific/Kiritimati"),
];

/// The IANA zone a Windows zone name stands for.
pub fn windows_to_iana(name: &str) -> Option<&'static str> {
    let name = name.trim();
    WINDOWS.iter().find(|(w, _)| w.eq_ignore_ascii_case(name)).map(|(_, i)| *i)
}

/// An IANA zone found in `tzid`, however it's dressed: `Europe/Berlin`,
/// `/Europe/Berlin`, `/mozilla.org/20050126_1/Europe/Berlin`,
/// `/softwarestudio.org/Olson_20011030_5/America/New_York`, `"UTC"`.
pub fn iana_in(tzid: &str) -> Option<(String, TimeZone)> {
    let t = tzid.trim().trim_matches('"').trim();
    if let Ok(z) = TimeZone::get(t) {
        return Some((t.to_owned(), z));
    }
    let parts: Vec<&str> = t.split('/').filter(|p| !p.is_empty()).collect();
    // The longest tail that names a zone (`America/Argentina/Buenos_Aires`).
    for start in 0..parts.len() {
        let cand = parts[start..].join("/");
        if (cand.contains('/') || parts.len() - start == 1)
            && let Ok(z) = TimeZone::get(&cand)
        {
            return Some((cand, z));
        }
    }
    match t.to_ascii_uppercase().as_str() {
        "Z" | "GMT" | "UTC" | "ETC/UTC" | "UNIVERSAL" => Some(("UTC".into(), TimeZone::UTC)),
        _ => None,
    }
}

/// An observance of a `VTIMEZONE`: when it starts, its offsets, its
/// yearly rule.
struct Observance {
    daylight: bool,
    start: jiff::civil::DateTime,
    from: i32,
    to: i32,
    name: Option<String>,
    month: Option<i8>,
    /// Week of the month, 1–4 or 5 for the last.
    week: Option<i8>,
    /// 0 (Sunday) to 6.
    weekday: Option<i8>,
}

fn observances(vtz: &Component) -> Vec<Observance> {
    let mut out = Vec::new();
    for c in &vtz.children {
        let daylight = match c.name.as_str() {
            "DAYLIGHT" => true,
            "STANDARD" => false,
            _ => continue,
        };
        let Some(start) = c.prop("DTSTART").and_then(|p| value::parse_datetime(&p.value)).map(|(dt, _)| dt) else {
            continue;
        };
        let Some(to) = c.prop("TZOFFSETTO").and_then(|p| value::parse_offset(&p.value)) else { continue };
        let from = c.prop("TZOFFSETFROM").and_then(|p| value::parse_offset(&p.value)).unwrap_or(to);
        let mut o =
            Observance { daylight, start, from, to, name: c.text("TZNAME"), month: None, week: None, weekday: None };
        if let Some(rule) = c.prop("RRULE").and_then(|p| crate::rrule::RRule::parse(&p.value)) {
            o.month = rule.by_month.first().copied().or(Some(start.month()));
            if let Some(&(n, wd)) = rule.by_day.first() {
                o.weekday = Some(wd.to_sunday_zero_offset());
                o.week = Some(match n {
                    n if n < 0 => 5,
                    0 => {
                        // `BYMONTHDAY=8,9,…,14;BYDAY=SU`: the week the days fall in.
                        let first = rule.by_month_day.iter().copied().filter(|d| *d > 0).min().unwrap_or(1);
                        (((first - 1) / 7) + 1).min(5)
                    }
                    n => n.min(5),
                });
            }
        }
        out.push(o);
    }
    out
}

/// A POSIX offset: hours west of UTC, `-1`, `5`, `-5:30`.
fn posix_offset(east: i32) -> String {
    let west = -east;
    let sign = if west < 0 { "-" } else { "" };
    let a = west.abs();
    let (h, m, s) = (a / 3600, (a % 3600) / 60, a % 60);
    match (m, s) {
        (0, 0) => format!("{sign}{h}"),
        (m, 0) => format!("{sign}{h}:{m:02}"),
        (m, s) => format!("{sign}{h}:{m:02}:{s:02}"),
    }
}

fn posix_name(name: Option<&str>, east: i32) -> String {
    match name {
        Some(n) if n.len() >= 3 && n.chars().all(|c| c.is_ascii_alphabetic()) => n.to_owned(),
        _ => {
            let a = east.abs();
            format!("<{}{:02}{:02}>", if east < 0 { '-' } else { '+' }, a / 3600, (a % 3600) / 60)
        }
    }
}

/// A `VTIMEZONE` as a zone: its latest standard and daylight
/// observances as a POSIX rule, else its standard offset fixed.
pub fn from_vtimezone(vtz: &Component) -> Option<TimeZone> {
    let obs = observances(vtz);
    let latest = |daylight: bool| obs.iter().filter(|o| o.daylight == daylight).max_by_key(|o| o.start);
    let std = latest(false).or_else(|| latest(true))?;
    let dst = latest(true).filter(|d| {
        d.month.is_some() && d.week.is_some() && d.weekday.is_some() && std.month.is_some() && !std.daylight
    });
    match dst {
        Some(d) if std.week.is_some() && std.weekday.is_some() => {
            let rule = |o: &Observance| {
                let t = o.start.time();
                format!(
                    "M{}.{}.{}/{}:{:02}:{:02}",
                    o.month.unwrap(),
                    o.week.unwrap(),
                    o.weekday.unwrap(),
                    t.hour(),
                    t.minute(),
                    t.second()
                )
            };
            let posix = format!(
                "{}{}{}{},{},{}",
                posix_name(std.name.as_deref(), std.to),
                posix_offset(std.to),
                posix_name(d.name.as_deref(), d.to),
                posix_offset(d.to),
                rule(d),
                rule(std)
            );
            match TimeZone::posix(&posix) {
                Ok(z) => Some(z),
                Err(e) => {
                    log::debug!("VTIMEZONE as {posix}: {e}");
                    Offset::from_seconds(std.to).ok().map(TimeZone::fixed)
                }
            }
        }
        _ => {
            let _ = std.from;
            Offset::from_seconds(std.to).ok().map(TimeZone::fixed)
        }
    }
}

/// The zones a file names, resolved once: its `VTIMEZONE`s by `TZID`.
#[derive(Clone, Default)]
pub struct Zones {
    defined: HashMap<String, Component>,
    resolved: HashMap<String, Tz>,
}

impl Zones {
    /// The zones defined in `vtimezones`.
    pub fn new<'a>(vtimezones: impl IntoIterator<Item = &'a Component>) -> Zones {
        let mut z = Zones::default();
        for c in vtimezones {
            if let Some(id) = c.text("TZID") {
                z.defined.insert(id.trim().to_owned(), c.clone());
            }
        }
        z
    }

    /// What `tzid` means here: the IANA zone it names (or that its
    /// definition's `X-LIC-LOCATION` names, or its Windows name stands
    /// for), else its definition's rules.
    pub fn resolve(&mut self, tzid: &str) -> Tz {
        let key = tzid.trim().trim_matches('"').to_owned();
        if let Some(t) = self.resolved.get(&key) {
            return t.clone();
        }
        let tz = self.find(&key);
        self.resolved.insert(key, tz.clone());
        tz
    }

    fn find(&self, id: &str) -> Tz {
        let def = self.defined.get(id);
        if let Some((name, zone)) = iana_in(id) {
            return Tz { id: name.into(), zone: Some(zone) };
        }
        if let Some(loc) = def.and_then(|d| d.text("X-LIC-LOCATION")).and_then(|l| iana_in(&l)) {
            return Tz { id: loc.0.into(), zone: Some(loc.1) };
        }
        if let Some(iana) = windows_to_iana(id).and_then(iana_in) {
            return Tz { id: iana.0.into(), zone: Some(iana.1) };
        }
        // `(UTC+01:00) Amsterdam, Berlin, …`: Outlook's display names.
        if let Some(rest) = id.strip_prefix('(').and_then(|r| r.split_once(')')) {
            let names: Vec<&str> = rest.1.split(',').map(str::trim).collect();
            for n in names {
                if let Some((name, zone)) = WINDOWS
                    .iter()
                    .find(|(_, iana)| {
                        iana.rsplit('/').next().is_some_and(|city| city.replace('_', " ").eq_ignore_ascii_case(n))
                    })
                    .and_then(|(_, iana)| iana_in(iana))
                {
                    return Tz { id: name.into(), zone: Some(zone) };
                }
            }
        }
        if let Some(zone) = def.and_then(from_vtimezone) {
            return Tz { id: id.into(), zone: Some(zone) };
        }
        log::debug!("unknown time zone {id:?}: read as floating time");
        Tz { id: id.into(), zone: None }
    }

    /// The definition of `tzid`, if the file had one.
    pub fn definition(&self, tzid: &str) -> Option<&Component> {
        self.defined.get(tzid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::datetime;

    #[test]
    fn names_resolve_however_they_are_dressed() {
        let mut z = Zones::default();
        assert_eq!(&*z.resolve("Europe/Berlin").id, "Europe/Berlin");
        assert_eq!(&*z.resolve("/mozilla.org/20050126_1/Europe/Berlin").id, "Europe/Berlin");
        assert_eq!(&*z.resolve("\"America/New_York\"").id, "America/New_York");
        assert_eq!(&*z.resolve("W. Europe Standard Time").id, "Europe/Berlin");
        assert_eq!(&*z.resolve("Pacific Standard Time").id, "America/Los_Angeles");
        assert!(z.resolve("Nowhere/Special").zone.is_none());
    }

    #[test]
    fn outlook_vtimezones_become_posix_rules() {
        let text = "BEGIN:VTIMEZONE\r\nTZID:Custom Central Europe\r\nBEGIN:STANDARD\r\nDTSTART:16010101T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n";
        let comps = crate::ics::parse(text);
        let mut z = Zones::new(comps.iter());
        let tz = z.resolve("Custom Central Europe");
        let zone = tz.zone.expect("a zone");
        let summer = datetime(2026, 7, 1, 12, 0, 0, 0).to_zoned(zone.clone()).unwrap();
        let winter = datetime(2026, 12, 1, 12, 0, 0, 0).to_zoned(zone.clone()).unwrap();
        assert_eq!(summer.offset().seconds(), 7200);
        assert_eq!(winter.offset().seconds(), 3600);
        // The switch: the last Sunday of March 2026 is the 29th.
        let before = datetime(2026, 3, 29, 1, 30, 0, 0).to_zoned(zone.clone()).unwrap();
        let after = datetime(2026, 3, 29, 3, 30, 0, 0).to_zoned(zone).unwrap();
        assert_eq!((before.offset().seconds(), after.offset().seconds()), (3600, 7200));
    }

    #[test]
    fn a_zone_without_daylight_time_is_fixed() {
        let text = "BEGIN:VTIMEZONE\nTZID:India\nBEGIN:STANDARD\nDTSTART:16010101T000000\nTZOFFSETFROM:+0530\nTZOFFSETTO:+0530\nEND:STANDARD\nEND:VTIMEZONE\n";
        let comps = crate::ics::parse(text);
        let mut z = Zones::new(comps.iter());
        let zone = z.resolve("India").zone.unwrap();
        assert_eq!(datetime(2026, 7, 1, 0, 0, 0, 0).to_zoned(zone).unwrap().offset().seconds(), 19800);
    }
}
