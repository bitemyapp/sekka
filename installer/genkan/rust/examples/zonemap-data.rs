// SPDX-License-Identifier: GPL-3.0-or-later
//! Regenerates the time zone map data embedded by `src/zonemap.rs`:
//! `data/world.bin` (country outlines and standard-time bands) and
//! `data/zones.tsv` (English zone, city and country names).
//!
//! ```text
//! cargo run --example zonemap-data -- SOURCE_DIR rust/data
//! ```
//!
//! SOURCE_DIR holds the pinned inputs listed in `data/zonemap-sources.md`.
use serde_json::Value;
use std::{collections::BTreeMap, error::Error, fs, path::Path};

/// Douglas–Peucker tolerances in degrees. At the widest map (about 900 px
/// for 360°) a degree is 2.5 px, so country outlines stay within a few
/// tenths of a pixel.
const COUNTRY_TOLERANCE: f64 = 0.05;
const BAND_TOLERANCE: f64 = 0.1;
/// Rings smaller than this many square degrees are dropped, except each
/// country's largest, so small island states still appear.
const MIN_AREA: f64 = 0.02;
/// The map is cropped here; everything further south is Antarctica.
const SOUTH: f64 = -58.0;

type Ring = Vec<[f64; 2]>;
type Failure = Box<dyn Error>;

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, out] = args.as_slice() else {
        return Err("usage: zonemap-data SOURCE_DIR DATA_DIR".into());
    };
    let (source, out) = (Path::new(source), Path::new(out));
    let world = world(source)?;
    fs::write(out.join("world.bin"), &world)?;
    let names = names(source)?;
    fs::write(out.join("zones.tsv"), &names)?;
    eprintln!(
        "world.bin {} bytes, zones.tsv {} lines",
        world.len(),
        names.lines().count()
    );
    Ok(())
}

fn read(path: &Path) -> Result<Value, Failure> {
    serde_json::from_slice(&fs::read(path)?).map_err(|e| format!("{}: {e}", path.display()).into())
}

fn polygons(geometry: &Value) -> Vec<Ring> {
    let coordinates = &geometry["coordinates"];
    let polygons: Vec<&Value> = match geometry["type"].as_str() {
        Some("Polygon") => vec![coordinates],
        Some("MultiPolygon") => coordinates.as_array().into_iter().flatten().collect(),
        _ => Vec::new(),
    };
    polygons
        .into_iter()
        .flat_map(|polygon| polygon.as_array().into_iter().flatten())
        .map(|ring| {
            ring.as_array()
                .into_iter()
                .flatten()
                .filter_map(|point| Some([point[0].as_f64()?, point[1].as_f64()?]))
                .collect()
        })
        .collect()
}

fn area(ring: &[[f64; 2]]) -> f64 {
    let doubled: f64 = ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum();
    doubled.abs() / 2.0
}

fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

fn simplify(ring: &[[f64; 2]], tolerance: f64) -> Ring {
    if ring.len() < 4 {
        return ring.to_vec();
    }
    let mut keep = vec![false; ring.len()];
    keep[0] = true;
    keep[ring.len() - 1] = true;
    let mut spans = vec![(0, ring.len() - 1)];
    while let Some((first, last)) = spans.pop() {
        let (index, far) = (first + 1..last)
            .map(|i| (i, distance(ring[i], ring[first], ring[last])))
            .fold(
                (0, 0.0),
                |best, next| if next.1 > best.1 { next } else { best },
            );
        if far > tolerance {
            keep[index] = true;
            spans.push((first, index));
            spans.push((index, last));
        }
    }
    ring.iter()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(*point))
        .collect()
}

/// Simplified rings of one feature: rings entirely south of the crop and
/// slivers are dropped, but the largest ring always survives.
fn shapes(geometry: &Value, tolerance: f64) -> Vec<Ring> {
    let rings: Vec<Ring> = polygons(geometry)
        .into_iter()
        .filter(|ring| ring.iter().any(|p| p[1] > SOUTH))
        .map(|ring| simplify(&ring, tolerance))
        .filter(|ring| ring.len() >= 4)
        .collect();
    let largest = rings.iter().map(|r| area(r)).fold(0.0, f64::max);
    rings
        .into_iter()
        .filter(|ring| {
            let a = area(ring);
            a >= MIN_AREA || a == largest
        })
        .collect()
}

fn quantize(degrees: f64) -> i16 {
    (degrees * 100.0).round() as i16
}

fn push_rings(out: &mut Vec<u8>, rings: &[Ring]) -> Result<(), Failure> {
    out.extend(u16::try_from(rings.len())?.to_le_bytes());
    for ring in rings {
        out.extend(u16::try_from(ring.len())?.to_le_bytes());
        for point in ring {
            out.extend(quantize(point[0]).to_le_bytes());
            out.extend(quantize(point[1]).to_le_bytes());
        }
    }
    Ok(())
}

/// Format, little-endian: `ZMAP`, version 1; u16 country count, each a
/// two-letter code (`--` when unassigned) and rings; u16 band count, each an
/// i16 standard offset in minutes and rings. Rings are a u16 count of
/// points, each i16 longitude and latitude in hundredths of a degree.
fn world(source: &Path) -> Result<Vec<u8>, Failure> {
    let mut countries: BTreeMap<String, Vec<Ring>> = BTreeMap::new();
    for feature in read(&source.join("ne_50m_admin_0_countries.geojson"))?["features"]
        .as_array()
        .ok_or("countries: no features")?
    {
        let code = feature["properties"]["ISO_A2_EH"].as_str().unwrap_or("-99");
        let code = if code.len() == 2 { code } else { "--" };
        if code == "AQ" {
            continue;
        }
        countries
            .entry(code.into())
            .or_default()
            .extend(shapes(&feature["geometry"], COUNTRY_TOLERANCE));
    }
    let mut bands: BTreeMap<i16, Vec<Ring>> = BTreeMap::new();
    for feature in read(&source.join("ne_10m_time_zones.geojson"))?["features"]
        .as_array()
        .ok_or("time zones: no features")?
    {
        let hours = feature["properties"]["zone"]
            .as_f64()
            .ok_or("time zone without offset")?;
        bands
            .entry((hours * 60.0).round() as i16)
            .or_default()
            .extend(shapes(&feature["geometry"], BAND_TOLERANCE));
    }
    let mut out = b"ZMAP\x01".to_vec();
    out.extend(u16::try_from(countries.len())?.to_le_bytes());
    for (code, rings) in &countries {
        out.extend(code.as_bytes());
        push_rings(&mut out, rings)?;
    }
    out.extend(u16::try_from(bands.len())?.to_le_bytes());
    for (offset, rings) in &bands {
        out.extend(offset.to_le_bytes());
        push_rings(&mut out, rings)?;
    }
    Ok(out)
}

/// Calls `visit` with the slash-joined path of every leaf of CLDR's nested
/// zone tables. Leaves are arrays (metazone history) or objects carrying
/// names.
fn leaves<'a>(prefix: &str, value: &'a Value, visit: &mut dyn FnMut(String, &'a Value)) {
    let Some(map) = value.as_object() else {
        visit(prefix.into(), value);
        return;
    };
    if ["exemplarCity", "long", "short", "_type"]
        .iter()
        .any(|k| map.contains_key(*k))
    {
        visit(prefix.into(), value);
        return;
    }
    for (key, child) in map {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}/{key}")
        };
        leaves(&path, child, visit);
    }
}

fn text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && !s.contains(['\t', '\n']))
}

/// One line per zone and alias: `zone city generic standard daylight`,
/// tab-separated, where empty fields fall back at run time. Country names
/// follow as `=CC name`.
fn names(source: &Path) -> Result<String, Failure> {
    let names = read(&source.join("timeZoneNames.json"))?;
    let names = &names["main"]["en"]["dates"]["timeZoneNames"];
    let metazones = read(&source.join("metaZones.json"))?;
    let history = &metazones["supplemental"]["metaZones"]["metazoneInfo"]["timezone"];
    let bcp47 = read(&source.join("timezone.json"))?;
    let territories = read(&source.join("territories.json"))?;

    let mut current: BTreeMap<String, String> = BTreeMap::new();
    leaves("", history, &mut |zone, uses| {
        let now = uses.as_array().into_iter().flatten().find_map(|entry| {
            let used = &entry["usesMetazone"];
            used.get("_to")
                .is_none()
                .then(|| used["_mzone"].as_str())
                .flatten()
        });
        if let Some(metazone) = now {
            current.insert(zone, metazone.into());
        }
    });
    let mut zone_names: BTreeMap<String, &Value> = BTreeMap::new();
    leaves("", &names["zone"], &mut |zone, value| {
        zone_names.insert(zone, value);
    });

    let mut lines: BTreeMap<String, String> = BTreeMap::new();
    let groups = bcp47["keyword"]["u"]["tz"]
        .as_object()
        .ok_or("bcp47: no zones")?;
    for group in groups.values().filter(|v| v.is_object()) {
        let mut ids: Vec<&str> = group["_alias"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if let Some(iana) = group["_iana"].as_str() {
            ids.push(iana);
        }
        // CLDR keys its names by the first alias, its canonical ID.
        let Some(&canonical) = ids.first() else {
            continue;
        };
        let own = zone_names.get(canonical).copied().unwrap_or(&Value::Null);
        let metazone = current
            .get(canonical)
            .map(|m| &names["metazone"][m])
            .unwrap_or(&Value::Null);
        let pick = |kind: &str| {
            text(&own["long"][kind])
                .or_else(|| text(&metazone["long"][kind]))
                .unwrap_or("")
        };
        let city = text(&own["exemplarCity"]).unwrap_or("");
        let fields = format!(
            "{city}\t{}\t{}\t{}",
            pick("generic"),
            pick("standard"),
            pick("daylight")
        );
        for id in ids {
            lines.insert(id.into(), fields.clone());
        }
    }
    let mut out = String::from(
        "# Generated by examples/zonemap-data.rs from Unicode CLDR; see zonemap-sources.md.\n",
    );
    for (zone, fields) in &lines {
        out.push_str(&format!("{zone}\t{fields}\n"));
    }
    for (code, name) in territories["main"]["en"]["localeDisplayNames"]["territories"]
        .as_object()
        .ok_or("territories: missing")?
    {
        if code.len() == 2
            && code.bytes().all(|b| b.is_ascii_uppercase())
            && let Some(name) = text(name)
        {
            out.push_str(&format!("={code}\t{name}\n"));
        }
    }
    Ok(out)
}
