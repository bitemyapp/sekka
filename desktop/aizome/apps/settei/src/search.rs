//! The sidebar's search: which panes and options a query finds, and what
//! of their titles to highlight.
//!
//! Settings are found by words people think of, so matching is by words
//! first: every word of the query must begin a word of the title, the
//! keywords or (for an option) its pane's title, or sit inside the title.
//! Only when nothing at all matches that way does the search fall back to
//! `aizome-search`'s fuzzy matcher, for typos of order ("kyebaord").

use std::ops::Range;

use crate::pane::SettingEntry;

/// A pane the query finds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Its index in the window's list of panes.
    pub pane: usize,
    /// Whether the pane itself matched (else only its options did).
    pub pane_matched: bool,
    /// What of its title to highlight.
    pub ranges: Vec<Range<usize>>,
    /// Its options the query finds, best first.
    pub entries: Vec<EntryHit>,
    pub score: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryHit {
    pub id: String,
    pub title: String,
    pub ranges: Vec<Range<usize>>,
    pub score: i64,
}

/// What search knows of a pane.
pub struct PaneInfo<'a> {
    pub title: &'a str,
    pub keywords: &'a [&'a str],
    pub entries: &'a [SettingEntry],
}

/// How many options show under a pane in the results.
pub const ENTRIES_PER_PANE: usize = 6;

/// The panes and options `query` finds, best first.
pub fn find(query: &str, panes: &[PaneInfo]) -> Vec<Hit> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits = by_words(&words, panes);
    if hits.is_empty() {
        hits = fuzzy(query, panes);
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.pane.cmp(&b.pane)));
    hits
}

fn by_words(words: &[String], panes: &[PaneInfo]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (i, p) in panes.iter().enumerate() {
        let kw: Vec<&str> = p.keywords.to_vec();
        let pane_score = score(words, p.title, &kw, &[]);
        let mut entries: Vec<EntryHit> = p
            .entries
            .iter()
            .filter_map(|e| {
                let kws: Vec<&str> = e.keywords.iter().map(String::as_str).collect();
                let mut context: Vec<&str> = vec![p.title];
                context.extend(e.under.iter().map(String::as_str));
                let s = score(words, &e.title, &kws, &context)?;
                Some(EntryHit { id: e.id.clone(), title: e.title.clone(), ranges: ranges(words, &e.title), score: s })
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.score));
        // An option found only through its pane's title says nothing more
        // than the pane: keep those out when the pane itself matched.
        if pane_score.is_some() {
            entries.retain(|e| !e.ranges.is_empty() || e.score >= 3 * words.len() as i64);
        }
        entries.truncate(ENTRIES_PER_PANE);
        if pane_score.is_none() && entries.is_empty() {
            continue;
        }
        let best_entry = entries.first().map_or(0, |e| e.score);
        hits.push(Hit {
            pane: i,
            pane_matched: pane_score.is_some(),
            ranges: ranges(words, p.title),
            entries,
            score: pane_score.map_or(best_entry, |s| s + 2).max(best_entry),
        });
    }
    hits
}

/// A candidate's score when every word matches, else `None`.
fn score(words: &[String], title: &str, keywords: &[&str], context: &[&str]) -> Option<i64> {
    let title_l = title.to_lowercase();
    let mut total = 0;
    for w in words {
        let s = if starts_word(&title_l, w) {
            if title_l.starts_with(w.as_str()) { 6 } else { 4 }
        } else if (w.len() >= 3 && title_l.contains(w.as_str()))
            || keywords.iter().any(|k| starts_word(&k.to_lowercase(), w))
        {
            2
        } else if context.iter().any(|c| starts_word(&c.to_lowercase(), w)) {
            1
        } else {
            return None;
        };
        total += s;
    }
    Some(total)
}

/// Whether a word of `text` (lowercase) begins with `w`.
fn starts_word(text: &str, w: &str) -> bool {
    text.match_indices(w).any(|(i, _)| i == 0 || !text[..i].chars().next_back().is_some_and(char::is_alphanumeric))
}

/// What of `title` the words match, for highlighting (byte ranges).
fn ranges(words: &[String], title: &str) -> Vec<Range<usize>> {
    let lower = title.to_lowercase();
    // Lowercasing can change lengths outside ASCII: highlight only when it
    // didn't.
    if lower.len() != title.len() {
        return Vec::new();
    }
    let mut out: Vec<Range<usize>> = Vec::new();
    for w in words {
        let at = lower
            .match_indices(w.as_str())
            .find(|(i, _)| *i == 0 || !lower[..*i].chars().next_back().is_some_and(char::is_alphanumeric))
            .or_else(|| if w.len() >= 3 { lower.match_indices(w.as_str()).next() } else { None });
        if let Some((i, m)) = at {
            out.push(i..i + m.len());
        }
    }
    out.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

fn fuzzy(query: &str, panes: &[PaneInfo]) -> Vec<Hit> {
    let mut q = aizome_search::Query::new(query);
    let mut hits = Vec::new();
    for (i, p) in panes.iter().enumerate() {
        let pane = q.matches(p.title);
        let mut entries: Vec<EntryHit> = p
            .entries
            .iter()
            .filter_map(|e| {
                let m = q.matches(&e.title)?;
                Some(EntryHit { id: e.id.clone(), title: e.title.clone(), ranges: m.ranges, score: i64::from(m.score) })
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.score));
        entries.truncate(ENTRIES_PER_PANE);
        if pane.is_none() && entries.is_empty() {
            continue;
        }
        let best = entries.first().map_or(0, |e| e.score);
        let score = pane.as_ref().map_or(0, |m| i64::from(m.score)).max(best);
        hits.push(Hit {
            pane: i,
            pane_matched: pane.is_some(),
            ranges: pane.map(|m| m.ranges).unwrap_or_default(),
            entries,
            score,
        });
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes() -> Vec<(String, Vec<&'static str>, Vec<SettingEntry>)> {
        vec![
            (
                "Keyboard".into(),
                vec!["typing"],
                vec![
                    SettingEntry::new("caps-lock", "Caps Lock key").keywords(&["escape", "control", "remap"]),
                    SettingEntry::new("repeat-rate", "Key repeat rate"),
                ],
            ),
            (
                "Appearance".into(),
                vec!["dark", "theme"],
                vec![SettingEntry::new("theme", "Theme").keywords(&["night", "day"])],
            ),
            ("Network".into(), vec!["wifi", "ethernet"], vec![]),
        ]
    }

    fn run(q: &str) -> Vec<Hit> {
        let p = panes();
        let infos: Vec<PaneInfo> = p.iter().map(|(t, k, e)| PaneInfo { title: t, keywords: k, entries: e }).collect();
        find(q, &infos)
    }

    #[test]
    fn words_find_panes_and_options() {
        let h = run("caps");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].pane, 0);
        assert!(!h[0].pane_matched);
        assert_eq!(h[0].entries[0].id, "caps-lock");
        assert_eq!(h[0].entries[0].ranges, vec![0..4]);

        let h = run("remap");
        assert_eq!(h[0].entries[0].id, "caps-lock", "keywords find options");

        let h = run("wifi");
        assert_eq!(h[0].pane, 2);
        assert!(h[0].pane_matched);

        let h = run("night");
        assert_eq!(h[0].pane, 1);
        assert_eq!(h[0].entries[0].id, "theme");
    }

    #[test]
    fn every_word_must_match() {
        assert!(run("caps theme").is_empty() || run("caps theme").iter().all(|h| h.pane != 0));
        let h = run("key rate");
        assert_eq!(h[0].entries[0].id, "repeat-rate");
    }

    #[test]
    fn typos_fall_back_to_fuzzy() {
        let h = run("kybd");
        assert!(h.iter().any(|h| h.pane == 0), "{h:?}");
        assert!(run("").is_empty());
    }
}
