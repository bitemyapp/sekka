//! What the locale says about weeks and clocks: the first day of the
//! week, how weeks are numbered (ISO 8601's "the week with the year's
//! first Thursday", or the US's "the week with 1 January"), and whether
//! times read 24-hour. Read from `LC_ALL`, `LC_TIME` or `LANG`'s
//! territory, after CLDR's week data.

use jiff::civil::{Date, Weekday};

use crate::rrule::{add_days, week_start};

/// How weeks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Weeks {
    pub first_day: Weekday,
    /// The fewest days of a new year the first week must have: 4 for
    /// ISO 8601, 1 where the week with 1 January is the first.
    pub min_days: u8,
}

impl Default for Weeks {
    fn default() -> Weeks {
        Weeks { first_day: Weekday::Monday, min_days: 4 }
    }
}

/// Territories whose weeks start on Sunday (CLDR `firstDay`).
const SUNDAY: &[&str] = &[
    "AG", "AS", "BD", "BR", "BS", "BT", "BW", "BZ", "CA", "CN", "CO", "DM", "DO", "ET", "GT", "GU", "HK", "HN", "ID",
    "IL", "IN", "JM", "JP", "KE", "KH", "KR", "LA", "MH", "MM", "MO", "MT", "MX", "MZ", "NI", "NP", "PA", "PE", "PH",
    "PK", "PR", "PT", "PY", "SA", "SG", "SV", "TH", "TT", "TW", "UM", "US", "VE", "VI", "WS", "YE", "ZA", "ZW",
];
/// … on Saturday.
const SATURDAY: &[&str] = &["AE", "AF", "BH", "DJ", "DZ", "EG", "IQ", "IR", "JO", "KW", "LY", "OM", "QA", "SD", "SY"];
/// … on Friday.
const FRIDAY: &[&str] = &["MV"];
/// Territories that number weeks as ISO 8601 does (CLDR `minDays` 4).
const MIN_DAYS_4: &[&str] = &[
    "AD", "AN", "AT", "AX", "BE", "BG", "CH", "CZ", "DE", "DK", "EE", "ES", "FI", "FJ", "FO", "FR", "GB", "GF", "GG",
    "GI", "GP", "GR", "HU", "IE", "IM", "IS", "IT", "JE", "LI", "LT", "LU", "MC", "MQ", "NL", "NO", "PL", "RE", "RU",
    "SE", "SJ", "SK", "SM", "VA",
];
/// Territories whose clocks commonly read 12-hour.
const TWELVE_HOUR: &[&str] = &[
    "US", "CA", "AU", "NZ", "IN", "PH", "PK", "EG", "SA", "MY", "BD", "CO", "MX", "SV", "HN", "NI", "PR", "KR", "TW",
    "JO",
];

/// The locale that sets times: `LC_ALL`, else `LC_TIME`, else `LANG`.
pub fn time_locale() -> String {
    for var in ["LC_ALL", "LC_TIME", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
        {
            return v;
        }
    }
    "C".into()
}

/// The territory of a locale name: `en_US.UTF-8@euro` is `US`.
pub fn territory(locale: &str) -> Option<String> {
    let base = locale.split(['.', '@']).next()?;
    let (_, t) = base.split_once(['_', '-'])?;
    let t = t.to_ascii_uppercase();
    (t.len() == 2 && t.chars().all(|c| c.is_ascii_alphabetic())).then_some(t)
}

impl Weeks {
    /// The weeks of a locale name (`C` and unknown ones: ISO 8601).
    pub fn for_locale(locale: &str) -> Weeks {
        let Some(t) = territory(locale) else { return Weeks::default() };
        let t = t.as_str();
        let first_day = if SUNDAY.contains(&t) {
            Weekday::Sunday
        } else if SATURDAY.contains(&t) {
            Weekday::Saturday
        } else if FRIDAY.contains(&t) {
            Weekday::Friday
        } else {
            Weekday::Monday
        };
        Weeks { first_day, min_days: if MIN_DAYS_4.contains(&t) { 4 } else { 1 } }
    }

    pub fn from_env() -> Weeks {
        Weeks::for_locale(&time_locale())
    }

    /// The first day of `d`'s week.
    pub fn week_start(&self, d: Date) -> Date {
        week_start(d, self.first_day)
    }

    /// `d`'s week number, by these rules.
    pub fn number(&self, d: Date) -> u32 {
        let ws = self.week_start(d);
        // A week belongs to the year that has at least `min_days` of it:
        // the year of its (8 − min_days)th day.
        let anchor = add_days(ws, 7 - self.min_days as i64).unwrap_or(ws);
        let y = anchor.year();
        let Ok(jan1) = Date::new(y, 1, 1) else { return 1 };
        let w1 = self.week_start(add_days(jan1, self.min_days as i64 - 1).unwrap_or(jan1));
        (crate::model::days_between(w1, ws) / 7 + 1) as u32
    }

    /// The weekdays in order from the first.
    pub fn days(&self) -> [Weekday; 7] {
        let mut out = [Weekday::Monday; 7];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.first_day.wrapping_add(i as i64);
        }
        out
    }
}

/// Whether the locale's clocks read 24-hour.
pub fn uses_24h(locale: &str) -> bool {
    territory(locale).is_none_or(|t| !TWELVE_HOUR.contains(&t.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn territories_decide() {
        assert_eq!(Weeks::for_locale("en_US.UTF-8"), Weeks { first_day: Weekday::Sunday, min_days: 1 });
        assert_eq!(Weeks::for_locale("de_DE.UTF-8@euro"), Weeks { first_day: Weekday::Monday, min_days: 4 });
        assert_eq!(Weeks::for_locale("ar_EG.UTF-8").first_day, Weekday::Saturday);
        assert_eq!(Weeks::for_locale("C.UTF-8"), Weeks::default());
        assert!(uses_24h("de_DE.UTF-8") && !uses_24h("en_US.UTF-8") && uses_24h("C"));
    }

    #[test]
    fn week_numbers() {
        let iso = Weeks::default();
        assert_eq!(iso.number(date(2026, 1, 1)), 1);
        assert_eq!(iso.number(date(2027, 1, 1)), 53);
        assert_eq!(iso.number(date(2026, 10, 17)), 42);
        assert_eq!(iso.number(date(2024, 12, 30)), 1);
        let us = Weeks::for_locale("en_US");
        assert_eq!(us.number(date(2026, 1, 1)), 1);
        assert_eq!(us.number(date(2025, 12, 28)), 1);
        assert_eq!(us.number(date(2026, 10, 17)), 42);
        assert_eq!(us.days()[0], Weekday::Sunday);
        assert_eq!(us.days()[6], Weekday::Saturday);
    }
}
