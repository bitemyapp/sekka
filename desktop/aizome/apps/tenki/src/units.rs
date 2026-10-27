//! Units: metric (°C, km/h, mm) or imperial (°F, mph, in). Forecasts are
//! fetched and kept in metric and converted as they're shown, so switching
//! needs no new fetch. The default follows the locale's measurement
//! system, as glibc's `LC_MEASUREMENT` does.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

impl Units {
    /// The locale's system: `LC_ALL`, then `LC_MEASUREMENT`, then `LANG`.
    pub fn from_locale() -> Units {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let locale = var("LC_ALL").or_else(|| var("LC_MEASUREMENT")).or_else(|| var("LANG"));
        locale.map_or(Units::Metric, |l| Units::for_locale(&l))
    }

    /// The system a locale name (`en_US.UTF-8`) measures in: imperial in
    /// the United States, Liberia and Myanmar (glibc's `measurement 2`).
    pub fn for_locale(locale: &str) -> Units {
        let name = locale.split(['.', '@']).next().unwrap_or("");
        let territory = name.split_once('_').map_or("", |(_, t)| t);
        match territory {
            "US" | "LR" | "MM" | "AS" | "GU" | "MP" | "PR" | "VI" | "UM" => Units::Imperial,
            _ => Units::Metric,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Units::Metric => "Metric (°C, km/h, mm)",
            Units::Imperial => "Imperial (°F, mph, in)",
        }
    }

    pub fn degree_symbol(self) -> &'static str {
        match self {
            Units::Metric => "°C",
            Units::Imperial => "°F",
        }
    }

    /// A temperature from °C.
    pub fn temp(self, celsius: f64) -> f64 {
        match self {
            Units::Metric => celsius,
            Units::Imperial => celsius * 9.0 / 5.0 + 32.0,
        }
    }

    /// "18°", rounded, never "-0°".
    pub fn degrees(self, celsius: f64) -> String {
        let v = self.temp(celsius).round();
        let v = if v == 0.0 { 0.0 } else { v };
        format!("{v:.0}°")
    }

    /// A speed from km/h.
    pub fn speed(self, kmh: f64) -> f64 {
        match self {
            Units::Metric => kmh,
            Units::Imperial => kmh / 1.609_344,
        }
    }

    pub fn speed_unit(self) -> &'static str {
        match self {
            Units::Metric => "km/h",
            Units::Imperial => "mph",
        }
    }

    /// "14" (in the speed unit), rounded.
    pub fn speed_value(self, kmh: f64) -> String {
        format!("{:.0}", self.speed(kmh).round())
    }

    /// "2.4 mm" or "0.09 in".
    pub fn precipitation(self, mm: f64) -> String {
        match self {
            Units::Metric if mm <= 0.0 => "0 mm".into(),
            Units::Metric if mm < 0.1 => "<0.1 mm".into(),
            Units::Metric => format!("{mm:.1} mm"),
            Units::Imperial => {
                let inches = mm / 25.4;
                if inches <= 0.0 {
                    "0 in".into()
                } else if inches < 0.01 {
                    "<0.01 in".into()
                } else {
                    format!("{inches:.2} in")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_choose_their_system() {
        assert_eq!(Units::for_locale("en_US.UTF-8"), Units::Imperial);
        assert_eq!(Units::for_locale("en_GB.UTF-8"), Units::Metric);
        assert_eq!(Units::for_locale("ja_JP.UTF-8"), Units::Metric);
        assert_eq!(Units::for_locale("my_MM"), Units::Imperial);
        assert_eq!(Units::for_locale("C.UTF-8"), Units::Metric);
        assert_eq!(Units::for_locale("POSIX"), Units::Metric);
        assert_eq!(Units::for_locale("sr_RS@latin"), Units::Metric);
    }

    #[test]
    fn conversions_and_words() {
        assert_eq!(Units::Imperial.degrees(20.0), "68°");
        assert_eq!(Units::Metric.degrees(-0.3), "0°");
        assert_eq!(Units::Metric.degrees(-4.6), "-5°");
        assert_eq!(Units::Imperial.speed_value(16.09344), "10");
        assert_eq!(Units::Metric.precipitation(2.44), "2.4 mm");
        assert_eq!(Units::Metric.precipitation(0.04), "<0.1 mm");
        assert_eq!(Units::Metric.precipitation(0.0), "0 mm");
        assert_eq!(Units::Imperial.precipitation(25.4), "1.00 in");
    }
}
