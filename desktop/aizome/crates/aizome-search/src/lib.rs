//! Aizome's search: what the command palette, the Toolchest's type-ahead,
//! menu type-ahead and Find stand on.
//!
//! - [`Query`] and [`rank`] match a typed query against short strings (menu
//!   commands, windows, apps, settings) with Helix's matcher, so typos of
//!   order ("fnd rep") and Unicode both behave.
//! - [`files::FileIndex`] walks the places a person keeps files, honouring
//!   `.gitignore` the way ripgrep, fd and Omperor's file picker do, and ranks
//!   paths by basename first, then by the whole path, nudged by how recently
//!   and how often each was opened ([`frecency`]).
//! - [`calc`] evaluates arithmetic typed into the palette ("15% of 80",
//!   "2^10 / 3").
//!
//! fff-search was evaluated for the file scope and not used: a non-ASCII
//! query that matches nothing returns every file (see Omperor's
//! `fff-search-repro`), and the crate brings libgit2 and LMDB into the shell
//! process, which is meant to stay small.

pub mod calc;
pub mod files;
pub mod frecency;
pub mod recent;

use std::ops::Range;

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// A query compiled for matching, with the matcher's scratch space. Reuse
/// one across many candidates: building it allocates, matching doesn't.
pub struct Query {
    text: String,
    pattern: Pattern,
    matcher: Matcher,
    chars: Vec<char>,
    indices: Vec<u32>,
}

/// A candidate's score and the characters that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Higher is better. Comparable only between candidates of one query.
    pub score: u32,
    /// Byte ranges of the candidate that matched, merged where adjacent,
    /// for highlighting.
    pub ranges: Vec<Range<usize>>,
}

impl Query {
    /// A query for short strings: names, titles, menu items. Words separated
    /// by spaces must all match, in any order; case matters only when the
    /// query has a capital.
    pub fn new(text: &str) -> Query {
        Query::with_config(text, Config::DEFAULT)
    }

    /// A query for paths: separators score as word boundaries and the
    /// matcher prefers matches in later path components.
    pub fn for_paths(text: &str) -> Query {
        Query::with_config(text, Config::DEFAULT.match_paths())
    }

    fn with_config(text: &str, config: Config) -> Query {
        Query {
            text: text.trim().to_owned(),
            // Plain fuzzy atoms: the palette's queries are typed by people
            // who don't know fzf's ^ $ ' ! syntax, so none of it applies.
            pattern: Pattern::new(text, CaseMatching::Smart, Normalization::Smart, AtomKind::Fuzzy),
            matcher: Matcher::new(config),
            chars: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The candidate's score, or `None` when it doesn't match. An empty
    /// query matches everything with score 0.
    pub fn score(&mut self, candidate: &str) -> Option<u32> {
        if self.is_empty() {
            return Some(0);
        }
        let haystack = Utf32Str::new(candidate, &mut self.chars);
        self.pattern.score(haystack, &mut self.matcher)
    }

    /// The score and the matched byte ranges, for highlighting.
    pub fn matches(&mut self, candidate: &str) -> Option<Match> {
        if self.is_empty() {
            return Some(Match { score: 0, ranges: Vec::new() });
        }
        self.indices.clear();
        let haystack = Utf32Str::new(candidate, &mut self.chars);
        let score = self.pattern.indices(haystack, &mut self.matcher, &mut self.indices)?;
        self.indices.sort_unstable();
        self.indices.dedup();
        Some(Match { score, ranges: char_indices_to_ranges(candidate, &self.indices) })
    }
}

/// Turns sorted char indices into merged byte ranges of `text`.
///
/// `Utf32Str::new` treats an ASCII string as bytes and anything else as
/// chars, so in both cases an index counts chars.
fn char_indices_to_ranges(text: &str, indices: &[u32]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = indices.iter().copied().peekable();
    for (n, (byte, ch)) in text.char_indices().enumerate() {
        let Some(&next) = wanted.peek() else { break };
        if next as usize != n {
            continue;
        }
        wanted.next();
        let end = byte + ch.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == byte => last.end = end,
            _ => ranges.push(byte..end),
        }
    }
    ranges
}

/// What a palette row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// A menu command of the focused app.
    Command,
    /// An open window.
    Window,
    /// A file or folder.
    File,
    /// An application to open.
    App,
    /// A pane or option in Settings.
    Setting,
    /// A system action: lock, sleep, restart, switch theme.
    Action,
    /// The answer to arithmetic typed into the palette.
    Answer,
}

impl Scope {
    /// The scopes the palette's Tab key cycles through, after All.
    pub const CYCLE: [Scope; 5] = [Scope::Command, Scope::Window, Scope::File, Scope::App, Scope::Setting];

    pub fn label(self) -> &'static str {
        match self {
            Scope::Command => "Commands",
            Scope::Window => "Windows",
            Scope::File => "Files",
            Scope::App => "Apps",
            Scope::Setting => "Settings",
            Scope::Action => "Actions",
            Scope::Answer => "Answer",
        }
    }
}

/// A candidate for [`rank`]: what's shown and what else it answers to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    /// The row's main text, matched and highlighted ("Find and Replace…").
    pub title: String,
    /// Other names it goes by, matched as strongly as the title but not
    /// highlighted ("Files" for Hako, beside GNOME's app named "Files").
    pub aliases: Vec<String>,
    /// Other names it answers to, matched but not shown ("Settings" for
    /// Settei, "calculator" for Soroban, "wifi" for Network).
    pub keywords: Vec<String>,
    /// Added to the match score: frecency, being in the focused app, and so on.
    pub boost: i64,
}

/// A ranked candidate: its index in the input, its score and the title's
/// highlight ranges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    pub index: usize,
    pub score: i64,
    pub ranges: Vec<Range<usize>>,
}

/// Ranks candidates against a query, best first, keeping at most `limit`.
///
/// A candidate matches by its title, an alias or any keyword; a title or
/// alias match scores higher than the same match on a keyword, and prefix
/// matches of the title's words outrank scattered ones (the matcher's
/// bonuses). Ties keep the input order, so callers put the likelier items
/// first.
pub fn rank(query: &mut Query, candidates: &[Candidate], limit: usize) -> Vec<Ranked> {
    let mut ranked: Vec<Ranked> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let title = query.matches(&candidate.title);
            let alias = candidate.aliases.iter().filter_map(|a| query.score(a)).max();
            let keyword = candidate
                .keywords
                .iter()
                .filter_map(|k| query.score(k))
                .max()
                // A keyword hit counts for less than the same hit on the title.
                .map(|s| s * 3 / 4);
            let (score, ranges) = match (title, alias.max(keyword)) {
                (Some(t), Some(k)) if k > t.score => (k, Vec::new()),
                (Some(t), _) => (t.score, t.ranges),
                (None, Some(k)) => (k, Vec::new()),
                (None, None) => return None,
            };
            Some(Ranked { index, score: score as i64 + candidate.boost, ranges })
        })
        .collect();
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    ranked.truncate(limit);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(titles: &[&str]) -> Vec<Candidate> {
        titles.iter().map(|t| Candidate { title: t.to_string(), ..Default::default() }).collect()
    }

    #[test]
    fn words_match_in_any_order() {
        let items = candidates(&["Find and Replace…", "Replace and Find Next", "Find All References", "Paste"]);
        let mut q = Query::new("find rep");
        let ranked = rank(&mut q, &items, 10);
        let titles: Vec<&str> = ranked.iter().map(|r| items[r.index].title.as_str()).collect();
        // Both words must match; "References" has no "p", "Paste" no "find".
        assert_eq!(titles.len(), 2, "{titles:?}");
        assert!(titles.contains(&"Replace and Find Next"));
        assert_eq!(titles[0], "Find and Replace…");
    }

    #[test]
    fn non_ascii_queries_that_match_nothing_return_nothing() {
        let items = candidates(&["README.md", "naïve/résumé.md", "日本語/ファイル.txt"]);
        assert!(rank(&mut Query::new("한글"), &items, 10).is_empty());
        assert!(rank(&mut Query::new("🙂"), &items, 10).is_empty());
        let ranked = rank(&mut Query::new("日本"), &items, 10);
        assert_eq!(ranked.len(), 1);
        assert_eq!(items[ranked[0].index].title, "日本語/ファイル.txt");
        // Normalisation: an unaccented query finds accented text.
        assert_eq!(rank(&mut Query::new("resume"), &items, 10).len(), 1);
    }

    #[test]
    fn highlight_ranges_are_byte_ranges() {
        let mut q = Query::new("rés");
        let m = q.matches("naïve/résumé.md").unwrap();
        let text = "naïve/résumé.md";
        let hit: String = m.ranges.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(hit, "rés");
        let m = Query::new("fr").matches("Find and Replace").unwrap();
        assert_eq!(m.ranges, vec![0..1, 9..10]);
    }

    #[test]
    fn keywords_find_but_titles_win() {
        let items = vec![
            Candidate { title: "Soroban".into(), keywords: vec!["Calculator".into()], ..Default::default() },
            Candidate { title: "Calculate Totals".into(), ..Default::default() },
        ];
        let ranked = rank(&mut Query::new("calc"), &items, 10);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].index, 1);
        assert!(ranked[1].ranges.is_empty());
    }

    #[test]
    fn aliases_match_as_titles_do_without_highlights() {
        let items = vec![
            Candidate { title: "Calculator".into(), ..Default::default() },
            Candidate { title: "Soroban".into(), aliases: vec!["Calculator".into()], ..Default::default() },
        ];
        let ranked = rank(&mut Query::new("calc"), &items, 10);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].score, ranked[1].score);
        // Tied, so in the input's order.
        assert_eq!((ranked[0].index, ranked[1].index), (0, 1));
        assert!(!ranked[0].ranges.is_empty());
        assert!(ranked[1].ranges.is_empty());
    }

    #[test]
    fn empty_query_keeps_order() {
        let items = candidates(&["b", "a", "c"]);
        let ranked = rank(&mut Query::new("  "), &items, 2);
        assert_eq!(ranked.iter().map(|r| r.index).collect::<Vec<_>>(), vec![0, 1]);
    }
}
