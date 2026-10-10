// SPDX-License-Identifier: GPL-3.0-or-later
//! Choosing a time zone on a world map. Country outlines and standard-time
//! bands come from Natural Earth and names from Unicode CLDR (see
//! `data/zonemap-sources.md`); the zones and their positions come from the
//! live system's tzdata `zone.tab`. Geometry and data only; the widget is
//! part of the UI.
use crate::timezone::TimeZone;
use anyhow::{Context, Result, bail, ensure};
use std::{collections::HashMap, f64::consts::PI, fs, path::Path};

const WORLD: &[u8] = include_bytes!("../data/world.bin");
const NAMES: &str = include_str!("../data/zones.tsv");

/// Latitudes shown. Antarctica is cropped; its zones stay in the list.
pub const NORTH: f64 = 84.0;
pub const SOUTH: f64 = -58.0;
/// Open-water clicks farther than this from every zone select nothing.
const WATER_REACH_KM: f64 = 800.0;

// Natural Earth projection (Šavrič, Jenny, Patterson and Jenny, 2011).
fn x_scale(phi: f64) -> f64 {
    let p2 = phi * phi;
    let p4 = p2 * p2;
    0.8707 - 0.131979 * p2 + p4 * (-0.013791 + p4 * (0.003971 * p2 - 0.001529 * p4))
}
fn y_of(phi: f64) -> f64 {
    let p2 = phi * phi;
    let p4 = p2 * p2;
    phi * (1.007226 + p2 * (0.015085 + p4 * (-0.044475 + 0.028874 * p2 - 0.005916 * p4)))
}
fn y_slope(phi: f64) -> f64 {
    let p2 = phi * phi;
    let p4 = p2 * p2;
    1.007226
        + p2 * (0.015085 * 3.0
            + p4 * (-0.044475 * 7.0 + 0.028874 * 9.0 * p2 - 0.005916 * 11.0 * p4))
}

/// Projected size of the shown map, as width over height.
pub fn aspect() -> f64 {
    2.0 * PI * x_scale(0.0) / (y_of(NORTH.to_radians()) - y_of(SOUTH.to_radians()))
}

/// Position on the map with both axes in 0..=1, y downwards.
pub fn to_unit(lon: f64, lat: f64) -> (f64, f64) {
    let phi = lat.to_radians();
    let width = PI * x_scale(0.0);
    let (top, bottom) = (y_of(NORTH.to_radians()), y_of(SOUTH.to_radians()));
    (
        (lon.to_radians() * x_scale(phi) + width) / (2.0 * width),
        (top - y_of(phi)) / (top - bottom),
    )
}

/// Longitude and latitude of a map position, or `None` outside the globe.
pub fn from_unit(u: f64, v: f64) -> Option<(f64, f64)> {
    let width = PI * x_scale(0.0);
    let (top, bottom) = (y_of(NORTH.to_radians()), y_of(SOUTH.to_radians()));
    let (x, y) = (u * 2.0 * width - width, top - v * (top - bottom));
    let mut phi = y;
    for _ in 0..25 {
        let delta = (y_of(phi) - y) / y_slope(phi);
        phi -= delta;
        if delta.abs() < 1e-10 {
            break;
        }
    }
    let lambda = x / x_scale(phi);
    let lat = phi.to_degrees();
    (lambda.abs() <= PI && (SOUTH..=NORTH).contains(&lat)).then(|| (lambda.to_degrees(), lat))
}

/// Great-circle distance in kilometres.
pub fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (p1, p2) = (a.1.to_radians(), b.1.to_radians());
    let h = ((p2 - p1) / 2.0).sin().powi(2)
        + p1.cos() * p2.cos() * ((b.0 - a.0).to_radians() / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().min(1.0).asin()
}

/// Rings in degrees (longitude, latitude). Holes are rings too: containment
/// and filling use the even-odd rule.
pub struct Shape {
    pub rings: Vec<Vec<(f64, f64)>>,
    bounds: (f64, f64, f64, f64),
}
impl Shape {
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        let (west, south, east, north) = self.bounds;
        if lon < west || lon > east || lat < south || lat > north {
            return false;
        }
        let mut inside = false;
        for ring in &self.rings {
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                if (a.1 > lat) != (b.1 > lat) && lon < (b.0 - a.0) * (lat - a.1) / (b.1 - a.1) + a.0
                {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

pub struct Country {
    /// ISO 3166 code as in zone.tab; `--` for areas without one.
    pub code: String,
    pub shape: Shape,
}

/// A region keeping one standard UTC offset.
pub struct Band {
    pub offset_minutes: i32,
    pub shape: Shape,
}

pub struct Atlas {
    pub countries: Vec<Country>,
    pub bands: Vec<Band>,
}

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        ensure!(self.0.len() >= N, "truncated map data");
        let (head, rest) = self.0.split_at(N);
        self.0 = rest;
        Ok(head.try_into()?)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take()?))
    }
    fn shape(&mut self) -> Result<Shape> {
        let mut rings = Vec::new();
        let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for _ in 0..self.u16()? {
            let mut ring = Vec::new();
            for _ in 0..self.u16()? {
                let lon = f64::from(self.i16()?) / 100.0;
                let lat = f64::from(self.i16()?) / 100.0;
                bounds = (
                    bounds.0.min(lon),
                    bounds.1.min(lat),
                    bounds.2.max(lon),
                    bounds.3.max(lat),
                );
                ring.push((lon, lat));
            }
            rings.push(ring);
        }
        Ok(Shape { rings, bounds })
    }
}

impl Atlas {
    /// The embedded outlines; the format is described in the generator.
    pub fn embedded() -> Result<Self> {
        let mut data = Reader(WORLD);
        ensure!(&data.take::<5>()? == b"ZMAP\x01", "unknown map data");
        let mut countries = Vec::new();
        for _ in 0..data.u16()? {
            let code = String::from_utf8(data.take::<2>()?.to_vec())?;
            countries.push(Country {
                code,
                shape: data.shape()?,
            });
        }
        let mut bands = Vec::new();
        for _ in 0..data.u16()? {
            let offset_minutes = i32::from(data.i16()?);
            bands.push(Band {
                offset_minutes,
                shape: data.shape()?,
            });
        }
        ensure!(data.0.is_empty(), "trailing map data");
        Ok(Self { countries, bands })
    }

    pub fn country_at(&self, lon: f64, lat: f64) -> Option<&str> {
        self.countries
            .iter()
            .find(|c| c.shape.contains(lon, lat))
            .map(|c| c.code.as_str())
    }

    pub fn band_at(&self, lon: f64, lat: f64) -> Option<i32> {
        self.bands
            .iter()
            .find(|b| b.shape.contains(lon, lat))
            .map(|b| b.offset_minutes)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub zone: String,
    /// ISO 3166 code from zone.tab; empty for UTC.
    pub country: String,
    pub city: String,
    pub country_name: String,
    /// CLDR names; any may be empty.
    pub generic: String,
    pub standard: String,
    pub daylight: String,
    /// Longitude and latitude; `None` for UTC.
    pub position: Option<(f64, f64)>,
    /// Standard offset of the band containing `position`, in minutes.
    pub band: Option<i32>,
}
impl Place {
    /// "City, Country", as listed and searched.
    pub fn label(&self) -> String {
        if self.country_name.is_empty() {
            self.city.clone()
        } else {
            format!("{}, {}", self.city, self.country_name)
        }
    }

    /// The everyday name, such as "Central Time". Zones without a generic
    /// CLDR name, such as London's, use the name for the current season.
    pub fn name(&self, daylight: bool) -> Option<&str> {
        let seasonal = if daylight && !self.daylight.is_empty() {
            &self.daylight
        } else {
            &self.standard
        };
        [&self.generic, seasonal]
            .into_iter()
            .find(|s| !s.is_empty())
            .map(String::as_str)
    }
}

/// What the map shades for a zone: the country, limited to the zone's
/// standard-time band where that is known.
pub struct Territory {
    pub country: Option<usize>,
    pub bands: Vec<usize>,
}

pub struct Zones {
    pub atlas: Atlas,
    /// UTC first, then every zone in zone.tab sorted by label.
    pub places: Vec<Place>,
}

/// `+DDMM`/`+DDMMSS` latitude or `+DDDMM`/`+DDDMMSS` longitude.
fn coordinate(text: &str, degree_digits: usize) -> Option<f64> {
    let sign = match text.as_bytes().first()? {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let digits = &text[1..];
    if !digits.bytes().all(|b| b.is_ascii_digit())
        || ![degree_digits + 2, degree_digits + 4].contains(&digits.len())
    {
        return None;
    }
    let part = |range: std::ops::Range<usize>| -> f64 {
        digits
            .get(range)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0)
    };
    let degrees = part(0..degree_digits)
        + part(degree_digits..degree_digits + 2) / 60.0
        + part(degree_digits + 2..degree_digits + 4) / 3600.0;
    Some(sign * degrees)
}

/// ISO 6709 position from zone.tab, as (longitude, latitude).
fn position(field: &str) -> Option<(f64, f64)> {
    let split = field.get(1..)?.find(['+', '-'])? + 1;
    let (lat, lon) = field.split_at(split);
    Some((coordinate(lon, 3)?, coordinate(lat, 2)?))
}

fn city_of(zone: &str) -> String {
    zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
}

impl Zones {
    pub fn load(zoneinfo: &Path) -> Result<Self> {
        let atlas = Atlas::embedded()?;
        let mut names: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut countries: HashMap<String, String> = HashMap::new();
        for line in NAMES.lines().filter(|l| !l.starts_with('#')) {
            if let Some(entry) = line.strip_prefix('=') {
                if let Some((code, name)) = entry.split_once('\t') {
                    countries.insert(code.into(), name.into());
                }
            } else {
                let mut fields = line.split('\t');
                if let Some(zone) = fields.next() {
                    names.insert(zone, fields.collect());
                }
            }
        }
        // tzdata's own country names fill any gaps in CLDR.
        let iso = fs::read_to_string(zoneinfo.join("iso3166.tab")).unwrap_or_default();
        for line in iso.lines().filter(|l| !l.starts_with('#')) {
            if let Some((code, name)) = line.split_once('\t') {
                countries.entry(code.into()).or_insert_with(|| name.into());
            }
        }
        let table = fs::read_to_string(zoneinfo.join("zone.tab"))
            .with_context(|| format!("reading {}", zoneinfo.join("zone.tab").display()))?;
        let field = |zone: &str, index: usize| -> String {
            names
                .get(zone)
                .and_then(|f| f.get(index))
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let mut places = Vec::new();
        for line in table.lines().filter(|l| !l.starts_with('#')) {
            let mut fields = line.split('\t');
            let (Some(country), Some(coordinates), Some(zone)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Some(position) = position(coordinates) else {
                continue;
            };
            if TimeZone::parse(zone, zoneinfo).is_err() {
                continue;
            }
            let city = Some(field(zone, 0))
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| city_of(zone));
            places.push(Place {
                zone: zone.into(),
                country: country.into(),
                city,
                country_name: countries.get(country).cloned().unwrap_or(country.into()),
                generic: field(zone, 1),
                standard: field(zone, 2),
                daylight: field(zone, 3),
                position: Some(position),
                band: atlas.band_at(position.0, position.1),
            });
        }
        ensure!(!places.is_empty(), "zone.tab lists no installed time zones");
        places.sort_by_cached_key(|p| p.label().to_lowercase());
        if TimeZone::parse("UTC", zoneinfo).is_ok() {
            places.insert(
                0,
                Place {
                    zone: "UTC".into(),
                    country: String::new(),
                    city: "Coordinated Universal Time (UTC)".into(),
                    country_name: String::new(),
                    generic: field("UTC", 1),
                    standard: Some(field("UTC", 2))
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Coordinated Universal Time".into()),
                    daylight: String::new(),
                    position: None,
                    band: None,
                },
            );
        }
        Ok(Self { atlas, places })
    }

    pub fn find(&self, zone: &str) -> Option<usize> {
        self.places.iter().position(|p| p.zone == zone)
    }

    /// As `find`, but also matches aliases of a listed zone, such as
    /// `Europe/Kiev` or `US/Central` from a geolocation service. tzdata
    /// installs an alias as a link or as an identical copy of its zone.
    pub fn find_equivalent(&self, zone: &str, zoneinfo: &Path) -> Option<usize> {
        self.find(zone).or_else(|| {
            let data = |zone: &str| {
                TimeZone::parse(zone, zoneinfo).ok()?;
                fs::read(zoneinfo.join(zone)).ok()
            };
            let target = data(zone)?;
            self.places
                .iter()
                .position(|p| data(&p.zone).is_some_and(|d| d == target))
        })
    }

    /// The zone for a point on the map. Within a country that has several
    /// zones, the band's standard offset picks the side of a zone boundary,
    /// so western Kentucky gets Central rather than nearer Louisville.
    pub fn locate(&self, lon: f64, lat: f64) -> Result<usize> {
        let here = (lon, lat);
        let located: Vec<(usize, (f64, f64))> = self
            .places
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some((i, p.position?)))
            .collect();
        let nearest = |filter: &dyn Fn(&Place) -> bool| {
            located
                .iter()
                .filter(|(i, _)| filter(&self.places[*i]))
                .map(|(i, at)| (*i, distance_km(here, *at)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
        };
        let band = self.atlas.band_at(lon, lat);
        let in_band = |p: &Place| band.is_some() && p.band == band;
        let Some(country) = self.atlas.country_at(lon, lat) else {
            return match nearest(&|_| true) {
                Some((index, km)) if km <= WATER_REACH_KM => Ok(index),
                _ => bail!("no time zone near this point"),
            };
        };
        let local = |p: &Place| p.country == country;
        let best = nearest(&|p| local(p) && in_band(p));
        let banded = nearest(&in_band);
        let any_local = nearest(&local);
        // Overseas regions drawn as part of their country, such as French
        // Guiana, match a zone in their band elsewhere rather than the capital.
        let chosen = match (best, banded, any_local) {
            (Some(best), _, _) => Some(best),
            (None, Some(banded), Some(local)) if banded.1 < local.1 => Some(banded),
            (None, banded, local) => local.or(banded),
        };
        match chosen.or_else(|| nearest(&|_| true)) {
            Some((index, _)) => Ok(index),
            None => bail!("no time zones are available"),
        }
    }

    pub fn territory(&self, index: usize) -> Territory {
        let Some(place) = self.places.get(index) else {
            return Territory {
                country: None,
                bands: Vec::new(),
            };
        };
        Territory {
            country: self
                .atlas
                .countries
                .iter()
                .position(|c| c.code == place.country),
            bands: self
                .atlas
                .bands
                .iter()
                .enumerate()
                .filter(|(_, b)| Some(b.offset_minutes) == place.band)
                .map(|(i, _)| i)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
# comment
US\t+404251-0740023\tAmerica/New_York\tEastern (most areas)
US\t+381515-0854534\tAmerica/Kentucky/Louisville\tEastern - KY (Louisville area)
US\t+415100-0873900\tAmerica/Chicago\tCentral (most areas)
US\t+394421-1045903\tAmerica/Denver\tMountain (most areas)
US\t+340308-1181434\tAmerica/Los_Angeles\tPacific
FR\t+4852+00220\tEurope/Paris
GF\t+0456-05220\tAmerica/Cayenne
GB\t+513030-0000731\tEurope/London
IE\t+5320-00615\tEurope/Dublin
ES\t+4024-00341\tEurope/Madrid\tSpain (mainland)
ES\t+2806-01524\tAtlantic/Canary\tCanary Islands
RS\t+4450+02030\tEurope/Belgrade
NOWHERE\tbad\tEurope/Bad
";

    fn zones() -> (tempfile::TempDir, Zones) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("zone.tab"), TABLE).unwrap();
        for zone in TABLE
            .lines()
            .filter_map(|l| l.split('\t').nth(2))
            .chain(["UTC"])
        {
            let file = dir.path().join(zone);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"TZif2").unwrap();
        }
        let zones = Zones::load(dir.path()).unwrap();
        (dir, zones)
    }

    fn at(zones: &Zones, lat: f64, lon: f64) -> &str {
        &zones.places[zones.locate(lon, lat).unwrap()].zone
    }

    #[test]
    fn projection_round_trips_and_has_the_cropped_shape() {
        for (lon, lat) in [(0.0, 0.0), (-87.6, 41.8), (151.2, -33.9), (179.0, 80.0)] {
            let (u, v) = to_unit(lon, lat);
            let (back_lon, back_lat) = from_unit(u, v).unwrap();
            assert!((back_lon - lon).abs() < 1e-6 && (back_lat - lat).abs() < 1e-6);
        }
        assert!((to_unit(0.0, NORTH).1).abs() < 1e-9);
        assert!((to_unit(0.0, SOUTH).1 - 1.0).abs() < 1e-9);
        assert!((2.2..2.4).contains(&aspect()));
        // Corners lie outside the globe.
        assert!(from_unit(0.01, 0.01).is_none());
        assert!(from_unit(0.5, 1.2).is_none());
    }

    #[test]
    fn embedded_atlas_has_countries_and_bands() {
        let atlas = Atlas::embedded().unwrap();
        assert!(atlas.countries.len() > 200);
        assert_eq!(atlas.country_at(-87.6, 41.8), Some("US"));
        assert_eq!(atlas.country_at(2.35, 48.85), Some("FR"));
        assert_eq!(atlas.band_at(-87.6, 41.8), Some(-360));
        assert_eq!(atlas.band_at(77.2, 28.6), Some(330));
        assert_eq!(atlas.country_at(-30.0, 30.0), None);
    }

    #[test]
    fn places_have_cldr_names_and_utc_comes_first() {
        let (_dir, zones) = zones();
        assert_eq!(zones.places[0].zone, "UTC");
        assert_eq!(zones.places[0].position, None);
        assert!(zones.find("Europe/Bad").is_none());
        let chicago = &zones.places[zones.find("America/Chicago").unwrap()];
        assert_eq!(chicago.label(), "Chicago, United States");
        assert_eq!(chicago.name(true), Some("Central Time"));
        assert_eq!(chicago.band, Some(-360));
        let (lon, lat) = chicago.position.unwrap();
        assert!((lat - 41.85).abs() < 1e-9 && (lon + 87.65).abs() < 1e-9);
        let london = &zones.places[zones.find("Europe/London").unwrap()];
        assert_eq!(london.label(), "London, United Kingdom");
        assert_eq!(london.name(false), Some("Greenwich Mean Time"));
        assert_eq!(london.name(true), Some("British Summer Time"));
        let labels: Vec<String> = zones.places[1..].iter().map(Place::label).collect();
        let mut sorted = labels.clone();
        sorted.sort_by_key(|l| l.to_lowercase());
        assert_eq!(labels, sorted);
    }

    #[test]
    fn clicks_resolve_to_the_zone_that_keeps_local_time() {
        let (_dir, zones) = zones();
        // Paducah is nearer Louisville but keeps Central time.
        assert_eq!(at(&zones, 37.08, -88.60), "America/Chicago");
        assert_eq!(at(&zones, 38.25, -85.76), "America/Kentucky/Louisville");
        assert_eq!(at(&zones, 40.0, -75.2), "America/New_York");
        assert_eq!(at(&zones, 39.0, -105.5), "America/Denver");
        // Dublin is nearer London than Belfast is, but Ireland is its own zone.
        assert_eq!(at(&zones, 53.35, -6.26), "Europe/Dublin");
        assert_eq!(at(&zones, 54.6, -5.93), "Europe/London");
        assert_eq!(at(&zones, 45.76, 4.84), "Europe/Paris");
        // French Guiana is drawn as part of France.
        assert_eq!(at(&zones, 4.0, -53.0), "America/Cayenne");
        assert_eq!(at(&zones, 28.3, -16.6), "Atlantic/Canary");
        assert_eq!(at(&zones, 37.4, -5.98), "Europe/Madrid");
        // Kosovo has no zone of its own.
        assert_eq!(at(&zones, 42.66, 21.16), "Europe/Belgrade");
        // Open ocean far from any zone selects nothing.
        assert!(zones.locate(-140.0, 0.0).is_err());
    }

    #[test]
    fn territory_is_the_country_limited_to_the_zone_band() {
        let (_dir, zones) = zones();
        let chicago = zones.territory(zones.find("America/Chicago").unwrap());
        let us = chicago.country.unwrap();
        assert_eq!(zones.atlas.countries[us].code, "US");
        assert!(!chicago.bands.is_empty());
        assert!(
            chicago
                .bands
                .iter()
                .all(|&b| zones.atlas.bands[b].offset_minutes == -360)
        );
        let utc = zones.territory(0);
        assert!(utc.country.is_none() && utc.bands.is_empty());
    }

    #[test]
    fn aliases_linked_in_tzdata_find_their_listed_zone() {
        let (dir, zones) = zones();
        let root = dir.path();
        fs::create_dir(root.join("US")).unwrap();
        // Listed zones differ in content; aliases are copies or links.
        for zone in ["America/Chicago", "Europe/Paris"] {
            fs::write(root.join(zone), format!("TZif {zone}")).unwrap();
        }
        fs::copy(root.join("America/Chicago"), root.join("US/Central")).unwrap();
        std::os::unix::fs::symlink("../Europe/Paris", root.join("US/Paris")).unwrap();
        let find = |zone| {
            zones
                .find_equivalent(zone, root)
                .map(|i| zones.places[i].zone.as_str())
        };
        assert_eq!(find("America/Chicago"), Some("America/Chicago"));
        assert_eq!(find("US/Central"), Some("America/Chicago"));
        assert_eq!(find("US/Paris"), Some("Europe/Paris"));
        assert_eq!(find("US/Eastern"), None);
        assert_eq!(find("../../etc/passwd"), None);
    }

    #[test]
    fn utc_has_no_territory() {
        let (_dir, zones) = zones();
        let utc = zones.territory(0);
        assert!(utc.country.is_none() && utc.bands.is_empty());
    }

    #[test]
    fn zone_tab_coordinates_parse_with_and_without_seconds() {
        assert_eq!(
            position("+4852+00220"),
            Some((2.0 + 20.0 / 60.0, 48.0 + 52.0 / 60.0))
        );
        let (lon, lat) = position("-3352+15113").unwrap();
        assert!((lat + 33.8667).abs() < 1e-3 && (lon - 151.2167).abs() < 1e-3);
        assert!(position("+404251-0740023").is_some());
        for bad in ["", "+40", "4852+00220", "+48a2+00220", "+485+00220"] {
            assert!(position(bad).is_none(), "{bad}");
        }
    }
}
