//! Time zones for a picker and a map, from tzdata's `zone1970.tab` (one row
//! per zone that differs from others since 1970, with the countries it
//! covers and its main city's coordinates) and `iso3166.tab` (country
//! names).

use crate::error::{Error, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// A time zone.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// The IANA name (`America/Argentina/Buenos_Aires`).
    pub id: String,
    /// The first part (`America`), for grouping.
    pub region: String,
    /// The last part, readable (`Buenos Aires`).
    pub city: String,
    /// ISO 3166 codes of the countries it covers, most populous first.
    pub countries: Vec<String>,
    /// Their names, when `iso3166.tab` was found.
    pub country_names: Vec<String>,
    /// Degrees north (negative south) and east (negative west) of the
    /// main city.
    pub latitude: f64,
    pub longitude: f64,
    /// tzdata's note (`most of Brazil`).
    pub comment: String,
}

/// Where tzdata is: `$TZDIR`, NixOS's `/etc/zoneinfo`, the usual places.
pub fn zoneinfo_dir() -> Option<PathBuf> {
    let candidates = std::env::var_os("TZDIR").map(PathBuf::from).into_iter().chain(
        ["/etc/zoneinfo", "/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo"].map(PathBuf::from),
    );
    candidates.into_iter().find(|dir| dir.join("zone1970.tab").is_file())
}

/// Every zone, with country names, sorted by id; UTC included.
pub fn zones() -> Result<Vec<Zone>> {
    let dir = zoneinfo_dir().ok_or_else(|| Error::NotFound("tzdata's zone1970.tab".into()))?;
    zones_in(&dir)
}

/// The zones of the tzdata in `dir`.
pub fn zones_in(dir: &Path) -> Result<Vec<Zone>> {
    let table = std::fs::read_to_string(dir.join("zone1970.tab"))?;
    let names = std::fs::read_to_string(dir.join("iso3166.tab")).map(|t| parse_iso3166(&t)).unwrap_or_default();
    let mut zones = parse_zone1970(&table);
    for zone in &mut zones {
        zone.country_names = zone.countries.iter().filter_map(|c| names.get(c).cloned()).collect();
    }
    zones.push(Zone {
        id: "UTC".into(),
        region: "Etc".into(),
        city: "UTC".into(),
        countries: Vec::new(),
        country_names: Vec::new(),
        latitude: 0.0,
        longitude: 0.0,
        comment: "Coordinated Universal Time".into(),
    });
    zones.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(zones)
}

/// Rows of `zone1970.tab`: `codes<TAB>coordinates<TAB>TZ[<TAB>comments]`.
pub fn parse_zone1970(text: &str) -> Vec<Zone> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let codes = fields.next()?;
            let (latitude, longitude) = parse_coordinates(fields.next()?)?;
            let id = fields.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let comment = fields.next().unwrap_or("").trim().to_owned();
            Some(Zone {
                region: id.split('/').next().unwrap_or(id).to_owned(),
                city: city_name(id),
                countries: codes.split(',').map(str::trim).filter(|c| !c.is_empty()).map(str::to_owned).collect(),
                country_names: Vec::new(),
                latitude,
                longitude,
                comment,
                id: id.to_owned(),
            })
        })
        .collect()
}

/// `code<TAB>name` rows of `iso3166.tab`.
pub fn parse_iso3166(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (code, name) = line.split_once('\t')?;
            Some((code.trim().to_owned(), name.trim().to_owned()))
        })
        .collect()
}

/// A zone's city, readable: the last part with spaces for underscores.
pub fn city_name(id: &str) -> String {
    id.rsplit('/').next().unwrap_or(id).replace('_', " ")
}

/// ISO 6709 coordinates as tzdata writes them: `±DDMM±DDDMM` or
/// `±DDMMSS±DDDMMSS`.
pub fn parse_coordinates(text: &str) -> Option<(f64, f64)> {
    let text = text.trim();
    // The longitude starts at the second sign.
    let split = text.char_indices().skip(1).find(|(_, c)| *c == '+' || *c == '-')?.0;
    let (lat, lon) = text.split_at(split);
    Some((angle(lat, 2)?, angle(lon, 3)?))
}

fn angle(text: &str, degree_digits: usize) -> Option<f64> {
    let sign = match text.chars().next()? {
        '+' => 1.0,
        '-' => -1.0,
        _ => return None,
    };
    let digits = &text[1..];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let part = |range: std::ops::Range<usize>| -> Option<f64> { digits.get(range)?.parse::<f64>().ok() };
    let degrees = part(0..degree_digits)?;
    let minutes = part(degree_digits..degree_digits + 2)?;
    let seconds = match digits.len() - degree_digits {
        2 => 0.0,
        4 => part(degree_digits + 2..degree_digits + 4)?,
        _ => return None,
    };
    Some(sign * (degrees + minutes / 60.0 + seconds / 3600.0))
}

/// Zones by region (`Africa`, `America`…), each sorted by city.
pub fn by_region(zones: &[Zone]) -> BTreeMap<String, Vec<&Zone>> {
    let mut regions: BTreeMap<String, Vec<&Zone>> = BTreeMap::new();
    for zone in zones {
        regions.entry(zone.region.clone()).or_default().push(zone);
    }
    for list in regions.values_mut() {
        list.sort_by(|a, b| a.city.cmp(&b.city));
    }
    regions
}

/// The zone whose city is nearest a point (a click on the map), by
/// great-circle distance.
pub fn nearest(zones: &[Zone], latitude: f64, longitude: f64) -> Option<&Zone> {
    let distance = |zone: &Zone| {
        let (lat1, lon1) = (latitude.to_radians(), longitude.to_radians());
        let (lat2, lon2) = (zone.latitude.to_radians(), zone.longitude.to_radians());
        let a = ((lat2 - lat1) / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
        2.0 * a.sqrt().min(1.0).asin()
    };
    zones
        .iter()
        .filter(|z| z.id != "UTC")
        .min_by(|a, b| distance(a).partial_cmp(&distance(b)).unwrap_or(std::cmp::Ordering::Equal))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# tzdb timezone descriptions\n#codes\tcoordinates\tTZ\tcomments\n\
        AD\t+4230+00131\tEurope/Andorra\n\
        AR\t-3436-05827\tAmerica/Argentina/Buenos_Aires\tBuenos Aires (BA, CF)\n\
        US\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\n\
        AE,OM,RE,SC,TF\t+2518+05518\tAsia/Dubai\tCrozet\n\
        broken line\n\
        XX\t+99\tNowhere/Bad\n";

    #[test]
    fn zones_parse_with_coordinates() {
        let zones = parse_zone1970(TABLE);
        assert_eq!(zones.len(), 4, "bad rows are skipped");
        let ba = &zones[1];
        assert_eq!((ba.region.as_str(), ba.city.as_str()), ("America", "Buenos Aires"));
        assert!((ba.latitude - -34.6).abs() < 1e-9 && (ba.longitude - -58.45).abs() < 1e-9);
        let ny = &zones[2];
        assert!((ny.latitude - (40.0 + 42.0 / 60.0 + 51.0 / 3600.0)).abs() < 1e-9);
        assert!((ny.longitude - -(74.0 + 0.0 / 60.0 + 23.0 / 3600.0)).abs() < 1e-9);
        assert_eq!(zones[3].countries, ["AE", "OM", "RE", "SC", "TF"]);
        assert_eq!(zones[3].comment, "Crozet");
    }

    #[test]
    fn regions_and_the_nearest_city() {
        let zones = parse_zone1970(TABLE);
        let regions = by_region(&zones);
        assert_eq!(regions.keys().collect::<Vec<_>>(), ["America", "Asia", "Europe"]);
        assert_eq!(regions["America"][0].city, "Buenos Aires");
        // Philadelphia is nearest New York.
        assert_eq!(nearest(&zones, 39.95, -75.16).unwrap().id, "America/New_York");
        assert_eq!(nearest(&zones, 25.2, 55.3).unwrap().id, "Asia/Dubai");
    }

    #[test]
    fn country_names() {
        let names = parse_iso3166("#code\tname\nAD\tAndorra\nUS\tUnited States\n");
        assert_eq!(names["US"], "United States");
        assert_eq!(parse_coordinates("+4230+00131"), Some((42.5, 1.0 + 31.0 / 60.0)));
        assert_eq!(parse_coordinates("4230+00131"), None);
    }
}
