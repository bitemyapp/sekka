//! The cities Tokei knows: tzdata's zone1970.tab (each zone's main city)
//! and a few hundred major cities that share a zone, embedded from
//! `data/cities.tsv` (25 KB), and finding them by what's typed.

use std::sync::OnceLock;

/// A city and its time zone.
#[derive(Clone, Debug, PartialEq)]
pub struct City {
    pub name: String,
    pub country: String,
    /// The IANA zone (`Asia/Tokyo`).
    pub zone: String,
    /// Degrees north and east.
    pub lat: f64,
    pub lon: f64,
    /// The name, country and zone folded for matching.
    key: String,
}

const DATA: &str = include_str!("../data/cities.tsv");

/// Every city, in the table's order.
pub fn all() -> &'static [City] {
    static ALL: OnceLock<Vec<City>> = OnceLock::new();
    ALL.get_or_init(|| parse(DATA))
}

pub fn parse(text: &str) -> Vec<City> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split('\t');
            let name = f.next()?.to_owned();
            let country = f.next()?.to_owned();
            let zone = f.next()?.to_owned();
            let lat = f.next()?.trim().parse().ok()?;
            let lon = f.next()?.trim().parse().ok()?;
            let key = fold(&format!("{name} {country} {}", zone.replace(['/', '_'], " ")));
            Some(City { name, country, zone, lat, lon, key })
        })
        .collect()
}

/// The city with this name and zone, if it's known.
pub fn find(name: &str, zone: &str) -> Option<&'static City> {
    all().iter().find(|c| c.name == name && c.zone == zone)
}

/// Lower case without accents, for matching: "São Paulo" is "sao paulo".
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ą' => "a",
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => "e",
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ı' => "i",
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => "o",
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ű' | 'ů' => "u",
            'ý' | 'ÿ' => "y",
            'ñ' | 'ń' | 'ň' => "n",
            'ç' | 'ć' | 'č' => "c",
            'ś' | 'š' | 'ş' => "s",
            'ź' | 'ż' | 'ž' => "z",
            'ł' => "l",
            'ğ' => "g",
            'ř' => "r",
            'ď' => "d",
            'ť' => "t",
            'æ' => "ae",
            'œ' => "oe",
            'ß' => "ss",
            '\'' | '.' | '’' => "",
            '-' => " ",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(plain);
    }
    out
}

/// The cities matching `query`, best first: every word typed must begin
/// a word of the city's name, country or zone; a name that begins with
/// the query comes first, then whole-word matches, then shorter names.
pub fn search(query: &str, limit: usize) -> Vec<&'static City> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut hits: Vec<(u32, usize, &'static City)> = Vec::new();
    for (i, c) in all().iter().enumerate() {
        let parts: Vec<&str> = c.key.split_whitespace().collect();
        if !words.iter().all(|w| parts.iter().any(|p| p.starts_with(w))) {
            continue;
        }
        let name = fold(&c.name);
        let score = if name == q {
            0
        } else if name.starts_with(&q) {
            1
        } else if fold(&c.country).starts_with(&q) {
            3
        } else {
            2
        };
        hits.push((score, i, c));
    }
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.name.len().cmp(&b.2.name.len())).then(a.1.cmp(&b.1)));
    hits.into_iter().take(limit).map(|(_, _, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses_and_its_zones_resolve() {
        let cities = all();
        assert!(cities.len() > 500, "{}", cities.len());
        for c in cities {
            assert!(jiff::tz::TimeZone::get(&c.zone).is_ok(), "{} has an unknown zone {}", c.name, c.zone);
            assert!((-90.0..=90.0).contains(&c.lat) && (-180.0..=180.0).contains(&c.lon), "{}", c.name);
        }
    }

    #[test]
    fn finds_by_name_country_and_without_accents() {
        assert_eq!(search("tokyo", 5)[0].zone, "Asia/Tokyo");
        assert_eq!(search("sao paulo", 5)[0].name, "São Paulo");
        assert_eq!(search("San Fran", 5)[0].zone, "America/Los_Angeles");
        let japan: Vec<&str> = search("japan", 50).iter().map(|c| c.name.as_str()).collect();
        assert!(japan.contains(&"Osaka") && japan.contains(&"Tokyo"), "{japan:?}");
        // Every word must match: "new z" is New Zealand's cities, not New York.
        let nz = search("new z", 10);
        assert!(!nz.is_empty() && nz.iter().all(|c| c.country == "New Zealand"));
        assert_eq!(search("Reykjavik", 3)[0].name, "Reykjavík");
        assert_eq!(search("kolkata", 3)[0].zone, "Asia/Kolkata");
        assert!(search("zzzz", 5).is_empty());
        assert!(search("  ", 5).is_empty());
    }
}
