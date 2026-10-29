//! Filters Nisshi suggests, each counted on what's read and offered only
//! when it finds something:
//!
//! - for a message selected: its source, the source's problems, the
//!   topics Nisshi knows that source writes about (the message's own
//!   first), messages like it (or hiding them), its process, its unit,
//!   and the words that set its source apart, for programs Nisshi doesn't
//!   know;
//! - with nothing selected: what stands out in the boot: errors and who
//!   wrote them, units that failed, crashes, a burst of errors, and a
//!   message repeated so often it drowns the rest;
//! - while a key's value is typed in search (`app:`, `unit:`, `level:`):
//!   the values there are.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aizome_kit::Icon;
use regex::Regex;

use crate::entry::{Entry, Priority};
use crate::filter::{COREDUMP, Chip, Filter, Scope, UNIT_FAILED};
use crate::model::{Store, View};
use crate::shape;
use crate::time::Clock;

/// What a suggestion does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Add these chips (and set the scope and level, if given).
    Narrow { chips: Vec<Chip>, scope: Option<Scope>, level: Option<Priority> },
    /// Show the messages around one, everything else cleared.
    Context(u64),
    /// Put this in the search field.
    Search(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub label: String,
    pub count: Option<usize>,
    pub icon: Icon,
    /// What it does, in a sentence (its tooltip).
    pub why: String,
    pub action: Action,
}

fn narrow(chips: Vec<Chip>) -> Action {
    Action::Narrow { chips, scope: None, level: None }
}

/// The most suggestions offered at once.
pub const MOST: usize = 9;

/// Suggestions for the message selected.
///
/// Without `deep`, what's quick to count: the topics and the words that
/// need patterns run over the source's messages come with it.
pub fn for_entry(
    _store: &Store,
    view: &View,
    filter: &Filter,
    e: &Entry,
    clock: &Clock,
    deep: bool,
) -> Vec<Suggestion> {
    let rows = &view.shown;
    let mut out = Vec::new();
    let source = &e.source;
    // Interned: the same source is the same pointer.
    let theirs: Vec<&Entry> = rows.iter().filter(|x| Arc::ptr_eq(&x.source, source)).map(|x| &**x).collect();
    let only_source = filter.source() == Some(source);
    if !only_source && theirs.len() < rows.len() {
        out.push(Suggestion {
            label: format!("Only {source}"),
            count: Some(theirs.len()),
            icon: Icon::AppWindow,
            why: format!("Show only what {source} wrote"),
            action: narrow(vec![Chip::Source { name: source.clone(), not: false }]),
        });
    }
    let problems = theirs.iter().filter(|x| x.priority.is_problem()).count();
    if problems > 0 && problems < theirs.len() && filter.level.is_none_or(|l| l > Priority::Warning) {
        out.push(Suggestion {
            label: format!("{source}'s problems"),
            count: Some(problems),
            icon: Icon::TriangleAlert,
            why: format!("Warnings, errors and worse from {source}"),
            action: Action::Narrow {
                chips: vec![Chip::Source { name: source.clone(), not: false }],
                scope: None,
                level: Some(Priority::Warning),
            },
        });
    }
    // The topics Nisshi knows: this message's own first, then the rest by
    // how much they find.
    let mut topics_found = false;
    if deep && let Some((known, patterns)) = crate::known::of(e) {
        let mut found: Vec<(bool, usize, usize)> = Vec::new();
        for (t, re) in patterns.iter().enumerate() {
            let label = known.topics[t].0;
            if filter.chips.iter().any(|c| matches!(c, Chip::Topic { label: l, .. } if &**l == label)) {
                continue;
            }
            let n = theirs.iter().filter(|x| re.is_match(&x.message)).count();
            // A topic narrows what its source wrote, or it isn't offered.
            if n > 0 && n < theirs.len() {
                found.push((re.is_match(&e.message), n, t));
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        for (own, n, t) in found.into_iter().take(4) {
            let (label, _) = known.topics[t];
            topics_found = true;
            out.push(Suggestion {
                label: label.into(),
                count: Some(n),
                icon: Icon::Tag,
                why: if own {
                    format!("This message is about {}: show {source}'s others", label.to_lowercase())
                } else {
                    format!("{source}'s messages about {}", label.to_lowercase())
                },
                action: narrow(vec![
                    Chip::Source { name: source.clone(), not: false },
                    Chip::Topic {
                        label: format!("{label} ({})", known.name).into(),
                        re: patterns[t].clone(),
                        not: false,
                    },
                ]),
            });
        }
    }
    // Messages of the same shape.
    let alike = rows.iter().filter(|x| x.shape == e.shape).count();
    let text: Arc<str> = shape::shape(&e.message).into();
    if alike > 1 && !filter.chips.iter().any(|c| matches!(c, Chip::Shape { shape, .. } if *shape == e.shape)) {
        if alike < rows.len() {
            out.push(Suggestion {
                label: "Messages like this".into(),
                count: Some(alike),
                icon: Icon::Layers,
                why: format!("“{}”, whatever its numbers and names", crate::filter::short(&text)),
                action: narrow(vec![Chip::Shape { shape: e.shape, text: text.clone(), not: false }]),
            });
        }
        if alike >= 5 {
            out.push(Suggestion {
                label: "Hide messages like this".into(),
                count: Some(alike),
                icon: Icon::EyeOff,
                why: format!("Leave out “{}”", crate::filter::short(&text)),
                action: narrow(vec![Chip::Shape { shape: e.shape, text, not: true }]),
            });
        }
    }
    // Its process, where its source has several.
    if e.pid > 0 && !filter.chips.iter().any(|c| matches!(c, Chip::Pid(_))) {
        let pids: HashSet<u32> = theirs.iter().map(|x| x.pid).collect();
        if pids.len() > 1 {
            let n = theirs.iter().filter(|x| x.pid == e.pid).count();
            out.push(Suggestion {
                label: format!("Process {}", e.pid),
                count: Some(n),
                icon: Icon::Hash,
                why: format!("Only this run of {source}: {source} ran as {} processes", pids.len()),
                action: narrow(vec![Chip::Pid(e.pid)]),
            });
        }
    }
    // Its unit, where it says more than the source.
    if let Some(unit) = e.unit_shown() {
        let stem = crate::search::unit_stem(Some(unit)).unwrap_or(unit);
        let generic = unit.starts_with("session-")
            || unit.starts_with("user@")
            || unit.ends_with(".slice")
            || unit.as_ref() == "init.scope";
        if !generic && !stem.eq_ignore_ascii_case(source) {
            let n = rows.iter().filter(|x| x.unit.as_ref() == Some(unit) || x.about.as_ref() == Some(unit)).count();
            // Offered where it shows something other than its source does.
            if n > 0 && n < rows.len() && n != theirs.len() {
                out.push(Suggestion {
                    label: format!("Unit {unit}"),
                    count: Some(n),
                    icon: Icon::Settings2,
                    why: format!("What {unit} wrote, and systemd's messages about it"),
                    action: narrow(vec![Chip::Unit { name: unit.clone(), not: false }]),
                });
            }
        }
    }
    // A program Nisshi doesn't know: the words that set it apart.
    if deep && !topics_found {
        for (word, n) in distinctive(&theirs, rows, source).into_iter().take(2) {
            let Ok(re) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(&word))) else { continue };
            out.push(Suggestion {
                label: format!("Mentioning “{word}”"),
                count: Some(n),
                icon: Icon::Tag,
                why: format!("A word {source} uses often and the rest of the journal rarely"),
                action: narrow(vec![
                    Chip::Source { name: source.clone(), not: false },
                    Chip::Topic { label: format!("“{word}”").into(), re: Arc::new(re), not: false },
                ]),
            });
        }
    }
    if filter.is_narrowed() {
        out.push(Suggestion {
            label: "Around this time".into(),
            count: None,
            icon: Icon::Clock,
            why: format!("Every message from a minute either side of {}", clock.time(e.time)),
            action: Action::Context(e.seq),
        });
    }
    out.truncate(MOST);
    out
}

/// Words a source uses often and the others rarely: (word, its messages
/// with it).
fn distinctive(theirs: &[&Entry], all: &crate::model::Shown, source: &str) -> Vec<(String, usize)> {
    if theirs.len() < 6 {
        return Vec::new();
    }
    const STOP: [&str; 40] = [
        "with", "from", "that", "this", "have", "been", "were", "will", "into", "than", "then", "when", "what",
        "which", "there", "their", "would", "could", "should", "about", "after", "before", "while", "because", "other",
        "some", "only", "also", "more", "most", "such", "each", "very", "does", "done", "being", "they", "them",
        "your", "here",
    ];
    let words = |e: &Entry| -> HashSet<String> {
        e.message
            .lines()
            .next()
            .unwrap_or("")
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| w.len() >= 4 && w.len() <= 24)
            .map(str::to_lowercase)
            .filter(|w| !STOP.contains(&w.as_str()))
            .collect()
    };
    let step_of = |n: usize| (n / 4000).max(1);
    let mine: Vec<HashSet<String>> = theirs.iter().step_by(step_of(theirs.len())).map(|e| words(e)).collect();
    let everyone: Vec<HashSet<String>> = all.iter().step_by(step_of(all.len())).map(|e| words(e)).collect();
    let mut in_mine: HashMap<&str, usize> = HashMap::new();
    for set in &mine {
        for w in set {
            *in_mine.entry(w).or_default() += 1;
        }
    }
    let mut in_all: HashMap<&str, usize> = HashMap::new();
    for set in &everyone {
        for w in set {
            if in_mine.contains_key(w.as_str()) {
                *in_all.entry(w).or_default() += 1;
            }
        }
    }
    let source = source.to_lowercase();
    let (n_mine, n_all) = (mine.len() as f64, everyone.len().max(1) as f64);
    let mut scored: Vec<(f64, &str, usize)> = in_mine
        .iter()
        .filter(|(w, n)| **n >= 3 && (**n as f64) < n_mine * 0.9 && !source.contains(**w))
        .map(|(w, n)| {
            let share = *n as f64 / n_mine;
            let elsewhere = (in_all.get(w).copied().unwrap_or(0) as f64 / n_all).max(1e-4);
            (share * (share / elsewhere).ln(), *w, *n)
        })
        .filter(|(s, _, _)| *s > 0.15)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let step = (theirs.len() / 4000).max(1);
    scored.into_iter().map(|(_, w, n)| (w.to_owned(), n * step)).collect()
}

/// What stands out, with nothing selected: read from the view's tallies,
/// so it costs little however many messages there are.
pub fn overview(store: &Store, view: &View, filter: &Filter, clock: &Clock) -> Vec<Suggestion> {
    let rows = &view.shown;
    let notable = &view.notable;
    let mut out = Vec::new();
    if rows.is_empty() {
        return out;
    }
    if view.errors > 0 && filter.level.is_none_or(|l| l > Priority::Error) && filter.scope != Scope::Problems {
        out.push(Suggestion {
            label: "Errors".into(),
            count: Some(view.errors),
            icon: Icon::CircleX,
            why: format!(
                "Errors and worse, from {}",
                crate::count(notable.errors_by_source.len(), "source", "sources")
            ),
            action: Action::Narrow { chips: Vec::new(), scope: None, level: Some(Priority::Error) },
        });
        // Who wrote most of them.
        if filter.source().is_none() {
            let mut by: Vec<(&Arc<str>, usize)> = notable.errors_by_source.iter().map(|(s, n)| (s, *n)).collect();
            by.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            for (s, n) in by.into_iter().take(2) {
                out.push(Suggestion {
                    label: format!("{s}'s errors"),
                    count: Some(n),
                    icon: Icon::AppWindow,
                    why: format!("The errors {s} wrote"),
                    action: Action::Narrow {
                        chips: vec![Chip::Source { name: s.clone(), not: false }],
                        scope: None,
                        level: Some(Priority::Error),
                    },
                });
            }
        }
    }
    // Units that failed.
    for u in notable.failed.iter().take(2) {
        let n = rows.iter().filter(|e| e.unit.as_ref() == Some(u) || e.about.as_ref() == Some(u)).count();
        out.push(Suggestion {
            label: format!("{u} failed"),
            count: Some(n),
            icon: Icon::CircleAlert,
            why: format!("What {u} wrote, and systemd's messages about it"),
            action: narrow(vec![Chip::Unit { name: u.clone(), not: false }]),
        });
    }
    // Crashes.
    if filter.scope != Scope::Crashes && notable.crashes > 0 {
        let label = match notable.crashed.as_slice() {
            [] => "Crashes".to_owned(),
            [one] => format!("{one} crashed"),
            [one, ..] => format!("{one} and {} crashed", crate::count(notable.crashed.len() - 1, "other", "others")),
        };
        out.push(Suggestion {
            label,
            count: Some(notable.crashes),
            icon: Icon::Bug,
            why: "Core dumps, segfaults and panics".into(),
            action: Action::Narrow { chips: Vec::new(), scope: Some(Scope::Crashes), level: None },
        });
    }
    // Out of memory: who the kernel killed last.
    if let Some(e) = notable.last_oom.and_then(|i| store.entries.get(i as usize)) {
        let victim = e.message.split_once('(').and_then(|(_, r)| r.split_once(')')).map(|(n, _)| n.to_owned());
        out.push(Suggestion {
            label: match victim {
                Some(v) => format!("Out of memory: {v} killed"),
                None => "Out of memory".into(),
            },
            count: Some(notable.oom),
            icon: Icon::MemoryStick,
            why: "The kernel ran out of memory and ended processes to free some".into(),
            action: narrow(vec![Chip::Topic {
                label: "Out of memory".into(),
                re: Arc::new(Regex::new(r"(?i)out of memory|oom-kill|invoked oom-killer|oom_reaper").unwrap()),
                not: false,
            }]),
        });
    }
    // A burst of errors: a stretch with far more than usual.
    let h = &view.histogram;
    if filter.range().is_none() && h.buckets.len() > 4 && view.errors > 0 {
        let mut counts: Vec<u32> = h.buckets.iter().map(|b| b[0]).collect();
        let (peak, &most) = counts.iter().enumerate().max_by_key(|(_, n)| **n).unwrap();
        counts.sort_unstable();
        let typical = counts[counts.len() * 3 / 4].max(1);
        if most >= 8 && most >= typical * 5 {
            let width = (h.to - h.from) / h.buckets.len() as i64;
            let from = h.from + width * peak as i64;
            let (from, to) = (from - width, from + 2 * width);
            out.push(Suggestion {
                label: format!("Errors at {}", clock.short(from + width)),
                count: Some(most as usize),
                icon: Icon::Zap,
                why: format!("A burst of {most} errors, where there are usually under {}", typical.max(2)),
                action: narrow(vec![Chip::Range { from, to }]),
            });
        }
    }
    // A message so frequent it hides the rest: found in a sample, counted
    // whole.
    if rows.len() >= 200 {
        let step = (rows.len() / 4000).max(1);
        let mut by: HashMap<u64, (usize, &Entry)> = HashMap::new();
        for e in rows.iter().step_by(step) {
            by.entry(e.shape).or_insert((0, e)).0 += 1;
        }
        let sampled = rows.len().div_ceil(step);
        if let Some((s, (n, e))) = by.into_iter().max_by_key(|(_, (n, _))| *n)
            && n * 5 >= sampled
        {
            let n = rows.iter().filter(|x| x.shape == s).count();
            let text: Arc<str> = shape::shape(&e.message).into();
            out.push(Suggestion {
                label: format!("Hide “{}”", crate::filter::short(&text)),
                count: Some(n),
                icon: Icon::EyeOff,
                why: format!(
                    "{} of every 10 messages shown are this one, from {}",
                    (n * 10 / rows.len()).max(2),
                    e.source
                ),
                action: narrow(vec![Chip::Shape { shape: s, text, not: true }]),
            });
        }
    }
    out.truncate(MOST);
    out
}

/// The values for a key being typed in search.
pub fn completions(store: &Store, _view: &View, key: &str, partial: &str, input: &str) -> Vec<Suggestion> {
    let p = partial.to_lowercase();
    let mut out = Vec::new();
    match key {
        "app:" => {
            // Over everything read: what's half typed matches nothing yet.
            let mut sources: HashMap<&Arc<str>, usize> = HashMap::new();
            for e in &store.entries {
                *sources.entry(&e.source).or_default() += 1;
            }
            let mut sources: Vec<(&Arc<str>, usize)> =
                sources.into_iter().filter(|(s, _)| s.to_lowercase().starts_with(&p)).collect();
            sources.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            for (s, n) in sources.into_iter().take(MOST) {
                out.push(Suggestion {
                    label: s.to_string(),
                    count: Some(n),
                    icon: Icon::AppWindow,
                    why: format!("Messages from {s}"),
                    action: Action::Search(crate::search::complete(input, s)),
                });
            }
        }
        "unit:" => {
            let mut units: HashMap<&Arc<str>, usize> = HashMap::new();
            for e in &store.entries {
                if let Some(u) = &e.unit {
                    *units.entry(u).or_default() += 1;
                }
            }
            let mut units: Vec<(&Arc<str>, usize)> =
                units.into_iter().filter(|(u, _)| u.to_lowercase().starts_with(&p)).collect();
            units.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            for (u, n) in units.into_iter().take(MOST) {
                out.push(Suggestion {
                    label: u.to_string(),
                    count: Some(n),
                    icon: Icon::Settings2,
                    why: format!("Messages from {u}"),
                    action: Action::Search(crate::search::complete(input, u)),
                });
            }
        }
        "level:" => {
            for level in [Priority::Error, Priority::Warning, Priority::Notice, Priority::Info, Priority::Debug] {
                if level.name().starts_with(&p) || level.word().to_lowercase().starts_with(&p) {
                    let n = store.entries.iter().filter(|e| e.priority <= level).count();
                    out.push(Suggestion {
                        label: format!("{} and worse", level.word()),
                        count: Some(n),
                        icon: Icon::TriangleAlert,
                        why: format!("Messages at the {} level or more severe", level.word().to_lowercase()),
                        action: Action::Search(crate::search::complete(input, level.name())),
                    });
                }
            }
        }
        _ => {}
    }
    out
}

/// A message counts as a crash report or one about a unit failing
/// (for the details' glyph).
pub fn is_notable(e: &Entry) -> bool {
    e.message_id.as_deref() == Some(COREDUMP)
        || e.message_id.as_deref() == Some(UNIT_FAILED)
        || crate::filter::is_crash(e)
}
