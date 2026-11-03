//! A big collection made manageable: what of the games found shows, before
//! kids' rules say which of those they may play.
//!
//! - **Systems** a grown-up turned off show nothing.
//! - **Variants** (demos, betas, prototypes, samples, kiosk and test
//!   discs, bad dumps, hacks, trainers, translations) are left out when the
//!   grown-ups say so; a BIOS is never a game.
//! - **One of each game**: regions and revisions of a game (a set has a
//!   USA, a Europe and a Japan Kirby, and a Rev 1) show as one, the
//!   household's region first, then the World's, then the latest revision.
//!   What isn't a region, a language or a revision stays part of the game
//!   (`Gran Turismo 2 (Arcade Mode)` and `(Simulation Mode)` are two discs;
//!   `Disc 1` and `Disc 2` too).
//!
//! It reads the tags No-Intro's and Redump's names carry, so a well-named
//! set needs no database; anything else is a game of its own.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::library::Game;
use crate::rules::Settings;

/// Regions a name can say, as No-Intro and Redump spell them.
const REGIONS: &[&str] = &[
    "USA",
    "Europe",
    "Japan",
    "World",
    "Asia",
    "Australia",
    "Brazil",
    "Canada",
    "China",
    "France",
    "Germany",
    "Hong Kong",
    "Italy",
    "Korea",
    "Netherlands",
    "Spain",
    "Sweden",
    "Taiwan",
    "UK",
    "United Kingdom",
    "Russia",
    "Scandinavia",
    "Latin America",
    "Mexico",
    "Argentina",
    "Denmark",
    "Finland",
    "Norway",
    "Poland",
    "Portugal",
    "Greece",
    "India",
    "Ireland",
    "Belgium",
    "Switzerland",
    "Austria",
    "New Zealand",
    "South Africa",
    "Unknown",
];

/// Words that make a release something other than the game as sold.
const VARIANT_WORDS: &[&str] = &[
    "Demo",
    "Beta",
    "Proto",
    "Prototype",
    "Sample",
    "Kiosk",
    "Program",
    "Test Program",
    "Debug",
    "Alpha",
    "Promo",
    "Possible Proto",
    "Prerelease",
    "Pirate",
];

/// Which region a household plays first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Region {
    /// From the system's language (`LANG`).
    #[default]
    Auto,
    Usa,
    Europe,
    Japan,
}

impl Region {
    /// What it says in the settings.
    pub fn label(self) -> &'static str {
        match self {
            Region::Auto => "As the computer's language",
            Region::Usa => "USA",
            Region::Europe => "Europe",
            Region::Japan => "Japan",
        }
    }

    /// The region named first, `Auto` read from `lang` (`en_GB.UTF-8`).
    fn resolved(self, lang: &str) -> Region {
        match self {
            Region::Auto => {
                let country = lang.split(['.', '@']).next().unwrap_or("").split('_').nth(1).unwrap_or("");
                match (lang.get(..2).unwrap_or(""), country) {
                    ("ja", _) | (_, "JP") => Region::Japan,
                    (_, "US" | "CA" | "MX" | "BR") | ("", _) => Region::Usa,
                    _ => Region::Europe,
                }
            }
            r => r,
        }
    }

    /// Regions in the order a household prefers them.
    pub fn order(self, lang: &str) -> &'static [&'static str] {
        match self.resolved(lang) {
            Region::Japan => &["Japan", "World", "Asia", "USA", "Europe"],
            Region::Europe => &["Europe", "World", "UK", "United Kingdom", "Australia", "USA", "Japan"],
            _ => &["USA", "World", "Canada", "Europe", "Australia", "Japan"],
        }
    }
}

/// What a name's tags say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub regions: Vec<String>,
    /// The revision (`Rev 1`, `Rev A`, `v1.1`), 0 for the first release.
    pub revision: u32,
    /// Not the game as sold (a demo, a beta, a bad dump, a hack…).
    pub variant: bool,
    /// A console's BIOS, not a game.
    pub bios: bool,
    /// Tags that make it a game of its own (`Disc 2`, `Arcade Mode`).
    pub own: Vec<String>,
    /// The name without tags, lower case: with `own`, which game it is.
    pub base: String,
}

impl Tags {
    /// The game it is, whatever its region or revision.
    pub fn identity(&self) -> String {
        let mut s = self.base.clone();
        for t in &self.own {
            s.push('|');
            s.push_str(&t.to_lowercase());
        }
        s
    }
}

/// The tags in a file's name (without its extension).
pub fn tags(stem: &str) -> Tags {
    let mut t = Tags::default();
    let mut base = String::new();
    let mut rest = stem;
    while let Some(i) = rest.find(['(', '[']) {
        base.push_str(&rest[..i]);
        let close = if rest.as_bytes()[i] == b'(' { ')' } else { ']' };
        let Some(j) = rest[i..].find(close) else {
            base.push_str(&rest[i..]);
            rest = "";
            break;
        };
        let inner = rest[i + 1..i + j].trim();
        if close == ')' {
            paren(inner, &mut t);
        } else {
            bracket(inner, &mut t);
        }
        rest = &rest[i + j + 1..];
    }
    base.push_str(rest);
    t.base = base.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    t
}

/// A `(…)` tag.
fn paren(inner: &str, t: &mut Tags) {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.iter().all(|p| REGIONS.contains(p)) {
        t.regions.extend(parts.iter().map(|p| (*p).to_owned()));
    } else if parts.iter().all(|p| language(p)) {
        // Languages: the same game.
    } else if let Some(r) = inner.strip_prefix("Rev ") {
        t.revision =
            r.parse().unwrap_or_else(|_| r.bytes().next().map_or(1, |b| u32::from(b.saturating_sub(b'A')) + 1));
    } else if let Some(v) = inner.strip_prefix('v').filter(|v| v.starts_with(|c: char| c.is_ascii_digit())) {
        // v1.1 is a revision of v1.0.
        let n: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        t.revision = n.first().copied().unwrap_or(0) * 100 + n.get(1).copied().unwrap_or(0);
    } else if VARIANT_WORDS.iter().any(|w| inner == *w || inner.starts_with(&format!("{w} "))) {
        t.variant = true;
    } else if !date(inner) {
        t.own.push(inner.to_owned());
    }
}

/// A `[…]` tag: GoodTools' marks.
fn bracket(inner: &str, t: &mut Tags) {
    match inner {
        "BIOS" => t.bios = true,
        // Verified good.
        "!" => {}
        _ => {
            let c = inner.bytes().next().unwrap_or(b' ');
            // Bad, hacked, trained, translated, overdumped, alternate,
            // fixed, pirate.
            if matches!(c, b'b' | b'h' | b't' | b'T' | b'o' | b'a' | b'f' | b'p') {
                t.variant = true;
            } else {
                t.own.push(inner.to_owned());
            }
        }
    }
}

/// `En`, `Fr`, `Pt-BR`.
fn language(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() == 2 && b[0].is_ascii_uppercase() && b[1].is_ascii_lowercase())
        || (b.len() == 5 && b[2] == b'-' && b[0].is_ascii_uppercase() && b[3].is_ascii_uppercase())
}

/// A release date (`1995-03-01`, `1995-03`).
fn date(s: &str) -> bool {
    s.len() >= 7 && s.as_bytes()[4] == b'-' && s.as_bytes()[..4].iter().all(u8::is_ascii_digit)
}

/// The games that show, by their places in `games`, in order.
pub fn shown(games: &[Game], s: &Settings, lang: &str) -> Vec<usize> {
    let order = s.region.order(lang);
    let rank = |t: &Tags| -> usize {
        t.regions.iter().filter_map(|r| order.iter().position(|o| o == r)).min().unwrap_or(order.len())
    };
    let mut best: HashMap<(&str, String), (usize, Tags)> = HashMap::new();
    let mut out = Vec::new();
    for (i, g) in games.iter().enumerate() {
        if s.systems_off.contains(g.system.id) {
            continue;
        }
        let stem = g.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let t = tags(&stem);
        if t.bios || (s.hide_variants && t.variant) {
            continue;
        }
        if !s.one_of_each {
            out.push(i);
            continue;
        }
        let key = (g.system.id, t.identity());
        match best.get(&key) {
            Some((_, b))
                if (rank(b), u32::MAX - b.revision, b.variant) <= (rank(&t), u32::MAX - t.revision, t.variant) => {}
            _ => {
                best.insert(key, (i, t));
            }
        }
    }
    if s.one_of_each {
        out.extend(best.into_values().map(|(i, _)| i));
        out.sort_unstable();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_say_their_regions_revisions_and_variants() {
        let t = tags("Super Mario World (USA)");
        assert_eq!((t.regions.as_slice(), t.revision, t.variant), (&["USA".to_owned()][..], 0, false));
        assert_eq!(t.base, "super mario world");
        let t = tags("Legend of Zelda, The - A Link to the Past (Europe) (En,Fr,De) (Rev 1)");
        assert_eq!((t.regions.len(), t.revision, t.own.len()), (1, 1, 0));
        assert_eq!(tags("Sonic (Japan, USA) (Rev A)").revision, 1);
        assert_eq!(tags("Doom (v1.1)").revision, 101);
        assert!(tags("Star Fox 2 (Japan) (Proto)").variant);
        assert!(tags("Tetris (World) (Beta 2)").variant);
        assert!(tags("Kirby [h1C]").variant && tags("Kirby [b]").variant && tags("Kirby [T+Eng]").variant);
        assert!(!tags("Kirby [!]").variant);
        assert!(tags("[BIOS] PlayStation (USA) (v3.0)").bios);
        // Two discs, and two games, stay apart.
        let a = tags("Gran Turismo 2 (USA) (Arcade Mode) (Rev 1)");
        let b = tags("Gran Turismo 2 (USA) (Simulation Mode) (Rev 2)");
        assert_ne!(a.identity(), b.identity());
        assert_ne!(
            tags("Final Fantasy VII (USA) (Disc 1)").identity(),
            tags("Final Fantasy VII (USA) (Disc 2)").identity()
        );
        // Dates are a release's, not a game's.
        assert_eq!(tags("Zelda (Japan) (1995-03-01)").own.len(), 0);
    }

    #[test]
    fn the_households_region_comes_first() {
        assert_eq!(Region::Auto.order("en_US.UTF-8")[0], "USA");
        assert_eq!(Region::Auto.order("en_GB.UTF-8")[0], "Europe");
        assert_eq!(Region::Auto.order("ja_JP.UTF-8")[0], "Japan");
        assert_eq!(Region::Auto.order("de_DE.UTF-8")[0], "Europe");
        assert_eq!(Region::Auto.order("")[0], "USA");
        assert_eq!(Region::Japan.order("en_US")[0], "Japan");
    }
}
