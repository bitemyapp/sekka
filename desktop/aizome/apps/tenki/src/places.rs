//! Places: the ones the user keeps, found with Open-Meteo's geocoding
//! search, the city of the machine's time zone, and the current location.

use serde::{Deserialize, Serialize};

/// Where place searches go.
pub const SEARCH_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// How many results a search asks for.
pub const RESULTS: usize = 10;

/// The query for places named like `name`.
pub fn query(name: &str) -> Vec<(&'static str, String)> {
    vec![
        ("name", name.trim().to_owned()),
        ("count", RESULTS.to_string()),
        ("language", "en".into()),
        ("format", "json".into()),
    ]
}

/// A place a search found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub id: i64,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub country_code: Option<String>,
    #[serde(default)]
    pub admin1: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub population: Option<u64>,
}

impl Found {
    /// Its region and country: "Illinois, United States".
    pub fn region(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if let Some(a) = self.admin1.as_deref().filter(|a| !a.is_empty() && *a != self.name) {
            parts.push(a);
        }
        if let Some(c) = self.country.as_deref().filter(|c| !c.is_empty()) {
            parts.push(c);
        }
        parts.join(", ")
    }

    pub fn place(&self, origin: Origin) -> Place {
        Place {
            id: format!("geonames:{}", self.id),
            name: self.name.clone(),
            region: self.region(),
            latitude: self.latitude,
            longitude: self.longitude,
            timezone: self.timezone.clone(),
            origin,
        }
    }
}

#[derive(Deserialize)]
struct SearchAnswer {
    #[serde(default)]
    results: Vec<Found>,
    #[serde(default)]
    error: bool,
    reason: Option<String>,
}

/// Read the places of a search's answer (none is an empty list: Open-Meteo
/// leaves `results` out).
pub fn parse_search(json: &str) -> Result<Vec<Found>, String> {
    let a: SearchAnswer = serde_json::from_str(json).map_err(|e| format!("the search's answer didn't read: {e}"))?;
    if a.error {
        return Err(a.reason.unwrap_or_else(|| "the search was refused".into()));
    }
    Ok(a.results)
}

/// How a place came to be in the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// The user searched for it.
    #[default]
    Search,
    /// The city of the machine's time zone, added when there was nothing.
    TimeZone,
    /// Where GeoClue says the machine is.
    Current,
}

/// A place whose weather Tenki shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// `geonames:5128581`, or `current`.
    pub id: String,
    pub name: String,
    /// "New York, United States".
    #[serde(default)]
    pub region: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub origin: Origin,
}

/// The id of the current location's row.
pub const CURRENT_ID: &str = "current";

impl Place {
    /// The key its forecast is cached under: its position to 0.01°.
    pub fn cache_key(&self) -> String {
        cache_key(self.latitude, self.longitude)
    }

    /// What its row says under its name.
    pub fn subtitle(&self) -> String {
        match self.origin {
            Origin::TimeZone => "Your time zone".into(),
            _ => self.region.clone(),
        }
    }
}

/// The cache key of a position: `35.69_139.69`.
pub fn cache_key(lat: f64, lon: f64) -> String {
    use crate::forecast::round2;
    format!("{:.2}_{:.2}", round2(lat), round2(lon))
}

/// A position as people read it: "43.07° N, 89.40° W".
pub fn coordinates(lat: f64, lon: f64) -> String {
    let ns = if lat < 0.0 { 'S' } else { 'N' };
    let ew = if lon < 0.0 { 'W' } else { 'E' };
    format!("{:.2}° {ns}, {:.2}° {ew}", lat.abs(), lon.abs())
}

// The time zone's city.

/// The machine's time zone: `$TZ`, else where `/etc/localtime` points
/// (`…/zoneinfo/America/Chicago`).
pub fn system_timezone() -> Option<String> {
    if let Some(tz) = std::env::var("TZ").ok().map(|t| t.trim_start_matches(':').to_owned()).filter(|t| t.contains('/'))
    {
        return Some(tz.rsplit_once("zoneinfo/").map_or(tz.clone(), |(_, z)| z.to_owned()));
    }
    let target = std::fs::canonicalize("/etc/localtime").ok()?;
    let s = target.to_string_lossy();
    let (_, zone) = s.rsplit_once("zoneinfo/")?;
    let zone = zone.trim_start_matches("posix/").trim_start_matches("right/");
    zone.contains('/').then(|| zone.to_owned())
}

/// The city a time zone is named for: `America/Argentina/Buenos_Aires` is
/// "Buenos Aires". Zones named for no city (`Etc/UTC`) have none.
pub fn timezone_city(tz: &str) -> Option<String> {
    let (area, rest) = tz.split_once('/')?;
    if matches!(area, "Etc" | "SystemV" | "US" | "Canada" | "Brazil" | "Chile" | "Mexico") {
        return None;
    }
    let city = rest.rsplit('/').next()?.replace('_', " ");
    (!city.is_empty()).then_some(city)
}

/// Of a search for a time zone's city, the place in that zone (the most
/// populous, if several are), else none: Paris, Texas is not Europe/Paris.
pub fn in_timezone<'a>(results: &'a [Found], tz: &str) -> Option<&'a Found> {
    results.iter().filter(|r| r.timezone.as_deref() == Some(tz)).max_by_key(|r| r.population.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = r#"{"results":[
        {"id":4717560,"name":"Paris","latitude":33.66094,"longitude":-95.55551,"country_code":"US","admin1":"Texas","timezone":"America/Chicago","population":24782,"country":"United States"},
        {"id":2988507,"name":"Paris","latitude":48.85341,"longitude":2.3488,"country_code":"FR","admin1":"Île-de-France","timezone":"Europe/Paris","population":2138551,"country":"France"}
    ],"generationtime_ms":0.6}"#;

    #[test]
    fn reads_searches() {
        let r = parse_search(SEARCH).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].region(), "Texas, United States");
        assert_eq!(in_timezone(&r, "Europe/Paris").map(|f| f.id), Some(2988507));
        assert_eq!(in_timezone(&r, "Asia/Tokyo"), None);
        assert!(parse_search(r#"{"generationtime_ms":0.2}"#).unwrap().is_empty());
        let p = r[1].place(Origin::Search);
        assert_eq!(p.id, "geonames:2988507");
        assert_eq!(p.cache_key(), "48.85_2.35");
    }

    #[test]
    fn time_zone_cities() {
        assert_eq!(timezone_city("America/Chicago").as_deref(), Some("Chicago"));
        assert_eq!(timezone_city("America/Argentina/Buenos_Aires").as_deref(), Some("Buenos Aires"));
        assert_eq!(timezone_city("Etc/UTC"), None);
        assert_eq!(timezone_city("UTC"), None);
    }

    #[test]
    fn coordinates_read_as_people_write_them() {
        assert_eq!(coordinates(43.0731, -89.4012), "43.07° N, 89.40° W");
        assert_eq!(coordinates(-33.87, 151.21), "33.87° S, 151.21° E");
    }
}
