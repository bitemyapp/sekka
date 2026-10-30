//! What's typed in the search field: words to find, and `key:value`
//! filters, as Console's search takes tokens.
//!
//! | Typed | Finds |
//! |---|---|
//! | `word`, `"a phrase"` | in the message, what wrote it or its unit; a capital letter makes it exact |
//! | `-word` | messages without it (any term can be negated) |
//! | `/regex/` | a regular expression over the message (`(?i)` and the rest of Rust's syntax) |
//! | `app:NAME` (`from:`, `source:`) | what wrote it |
//! | `unit:NAME` | its systemd unit (`.service` may be left out) |
//! | `pid:N` | its process |
//! | `level:err` (`priority:`, `p:`) | that level or worse |
//! | `since:10m`, `until:14:30` (`after:`, `before:`) | its time ([`Clock::parse`]) |
//! | `id:ID` | its catalog id (`MESSAGE_ID`) |
//! | `FIELD=value` | a journal field: read again from the journal |

use std::ops::Range;
use std::sync::Arc;

use regex::Regex;

use crate::entry::{Entry, Priority};
use crate::time::Clock;

/// A word found without a regular expression: exactly, or in any case
/// (ASCII). The regular expression stays for highlighting.
#[derive(Clone, Debug)]
enum Literal {
    Exact(Box<memchr::memmem::Finder<'static>>),
    Fold(Box<aho_corasick::AhoCorasick>),
}

impl Literal {
    fn is_match(&self, s: &str) -> bool {
        match self {
            Literal::Exact(f) => f.find(s.as_bytes()).is_some(),
            Literal::Fold(a) => a.is_match(s),
        }
    }
}

#[derive(Clone, Debug)]
enum Term {
    /// A word or phrase, as a regular expression of its text (and found
    /// faster where it's ASCII).
    Text {
        re: Regex,
        fast: Option<Literal>,
    },
    Pattern {
        re: Regex,
    },
    Source(String),
    Unit(String),
    Pid(u32),
    Id(String),
}

#[derive(Clone, Debug, Default)]
pub struct Search {
    terms: Vec<(Term, bool)>,
    /// The worst level shown, at most.
    pub level: Option<Priority>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// Journal fields to match where the entries are read.
    pub fields: Vec<(String, String)>,
    /// What couldn't be read, in words.
    pub problems: Vec<String>,
}

/// The keys search takes, for completion: (key, what it's for).
pub const KEYS: [(&str, &str); 7] = [
    ("app:", "what wrote it"),
    ("unit:", "its systemd unit"),
    ("level:", "that level or worse"),
    ("since:", "after a time (10m, 2h, 14:30, yesterday)"),
    ("until:", "before a time"),
    ("pid:", "its process"),
    ("id:", "its catalog id"),
];

/// The tokens of `input`: words, "quoted phrases" and /patterns/.
fn tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => {
                cur.push(c);
                // A pattern's slash ends it only before a space or the end.
                if q != '/' || chars.peek().is_none_or(|n| n.is_whitespace()) {
                    quote = None;
                }
            }
            Some(_) => cur.push(c),
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => {
                let starts = cur.is_empty() || cur == "-" || cur.ends_with(':');
                if (c == '"' || c == '/') && starts {
                    quote = Some(c);
                }
                cur.push(c);
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"').map(|s| s.strip_suffix('"').unwrap_or(s)).unwrap_or(s)
}

impl Search {
    pub fn parse(input: &str, clock: &Clock) -> Search {
        let mut s = Search::default();
        for tok in tokens(input) {
            let (not, tok) = match tok.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest.to_owned()),
                _ => (false, tok),
            };
            // A journal field: FIELD=value.
            if let Some((f, v)) = tok.split_once('=')
                && !not
                && crate::query::check_field(f).is_ok()
                && f.len() > 1
            {
                s.fields.push((f.to_owned(), unquote(v).to_owned()));
                continue;
            }
            if let Some((key, value)) = tok.split_once(':')
                && !value.is_empty()
            {
                let value = unquote(value);
                let term = match key.to_ascii_lowercase().as_str() {
                    "app" | "from" | "source" | "src" => Some(Term::Source(value.to_lowercase())),
                    "unit" | "u" => Some(Term::Unit(value.to_owned())),
                    "pid" => match value.parse() {
                        Ok(p) => Some(Term::Pid(p)),
                        Err(_) => {
                            s.problems.push(format!("“{value}” isn't a process id"));
                            continue;
                        }
                    },
                    "id" => Some(Term::Id(value.to_ascii_lowercase())),
                    "level" | "priority" | "p" | "lvl" => {
                        match Priority::parse(value) {
                            Some(p) => s.level = Some(p),
                            None => s.problems.push(format!("“{value}” isn't a level")),
                        }
                        continue;
                    }
                    k @ ("since" | "after" | "until" | "before") => {
                        match clock.parse(value) {
                            Some(t) if matches!(k, "since" | "after") => s.since = Some(t),
                            Some(t) => s.until = Some(t),
                            None => s.problems.push(format!("“{value}” isn't a time")),
                        }
                        continue;
                    }
                    _ => None,
                };
                if let Some(t) = term {
                    s.terms.push((t, not));
                    continue;
                }
            }
            if tok.len() > 2 && tok.starts_with('/') && tok.ends_with('/') {
                match Regex::new(&tok[1..tok.len() - 1]) {
                    Ok(re) => s.terms.push((Term::Pattern { re }, not)),
                    Err(_) => s.problems.push(format!("{tok} isn't a pattern Nisshi can read")),
                }
                continue;
            }
            let text = unquote(&tok);
            if text.is_empty() {
                continue;
            }
            // Smart case: lower case finds any case.
            let exact = text.chars().any(char::is_uppercase);
            let pattern = format!("{}{}", if exact { "" } else { "(?i)" }, regex::escape(text));
            let fast = if exact {
                Some(Literal::Exact(Box::new(memchr::memmem::Finder::new(text.as_bytes()).into_owned())))
            } else if text.is_ascii() {
                aho_corasick::AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build([text])
                    .ok()
                    .map(|a| Literal::Fold(Box::new(a)))
            } else {
                None
            };
            if let Ok(re) = Regex::new(&pattern) {
                s.terms.push((Term::Text { re, fast }, not));
            }
        }
        s
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.level.is_none() && self.since.is_none() && self.until.is_none()
    }

    pub fn matches(&self, e: &Entry) -> bool {
        self.matches_in(e, &mut Names::default())
    }

    /// [`matches`](Search::matches), remembering what names decided.
    pub fn matches_in(&self, e: &Entry, names: &mut Names) -> bool {
        if self.level.is_some_and(|l| e.priority > l) {
            return false;
        }
        if self.since.is_some_and(|t| e.time < t) || self.until.is_some_and(|t| e.time > t) {
            return false;
        }
        self.terms.iter().enumerate().all(|(k, (t, not))| term_matches(k, t, e, names) != *not)
    }

    /// Where the words and patterns are in `text`, for highlighting.
    pub fn highlights(&self, text: &str) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for (t, not) in &self.terms {
            if *not {
                continue;
            }
            if let Term::Text { re, .. } | Term::Pattern { re } = t {
                out.extend(re.find_iter(text).filter(|m| !m.is_empty()).take(64).map(|m| m.range()));
            }
        }
        out.sort_by_key(|r| r.start);
        // Overlaps join.
        let mut merged: Vec<Range<usize>> = Vec::new();
        for r in out {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        merged
    }

    /// The terms that the journal can find too (`--grep`): the words and
    /// patterns, joined as one PCRE2 pattern, if there are only those.
    pub fn grep(&self) -> Option<String> {
        let mut parts = Vec::new();
        for (t, not) in &self.terms {
            match t {
                Term::Text { re, .. } | Term::Pattern { re } if !*not => parts.push(format!("(?=.*{})", re.as_str())),
                _ => return None,
            }
        }
        if parts.is_empty() {
            return None;
        }
        // Lookaheads: every word, in any order. `(?i)` inside each holds
        // for it alone.
        Some(format!("^{}", parts.join("")).replace("(?=.*(?i)", "(?=(?i).*"))
    }
}

/// What each term decided of a name (a source or a unit, interned: one
/// name is one pointer), so that a view matches each name once rather
/// than once per message.
#[derive(Default)]
pub struct Names(crate::entry::QuickMap<(usize, usize, u8), bool>);

impl Names {
    fn decide(&mut self, term: usize, which: u8, name: &Arc<str>, f: impl FnOnce(&str) -> bool) -> bool {
        let key = (term, crate::entry::address(name), which);
        *self.0.entry(key).or_insert_with(|| f(name))
    }
}

fn term_matches(k: usize, t: &Term, e: &Entry, names: &mut Names) -> bool {
    match t {
        Term::Text { re, fast } => {
            let hit = |s: &str| match fast {
                Some(l) => l.is_match(s),
                None => re.is_match(s),
            };
            hit(&e.message)
                || names.decide(k, 0, &e.source, hit)
                || e.unit.as_ref().is_some_and(|u| names.decide(k, 0, u, hit))
                || e.about.as_ref().is_some_and(|u| names.decide(k, 0, u, hit))
        }
        Term::Pattern { re } => re.is_match(&e.message),
        Term::Source(s) => {
            let same = |x: &str| x.eq_ignore_ascii_case(s) || !x.is_ascii() && x.to_lowercase() == *s;
            names.decide(k, 1, &e.source, same)
                || e.unit.as_ref().is_some_and(|u| names.decide(k, 2, u, |u| unit_stem(Some(u)).is_some_and(same)))
        }
        Term::Unit(u) => [&e.unit, &e.about].into_iter().flatten().any(|x| names.decide(k, 3, x, |x| unit_is(x, u))),
        Term::Pid(p) => e.pid == *p,
        Term::Id(id) => e.message_id.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(id)),
    }
}

/// A unit's name without its type: `sshd` for `sshd.service`.
pub fn unit_stem(unit: Option<&str>) -> Option<&str> {
    let u = unit?;
    Some(u.rsplit_once('.').map_or(u, |(stem, _)| stem))
}

/// Whether `unit` is the one `name` says: whole, or without `.service`,
/// or with `*` for anything.
pub fn unit_is(unit: &str, name: &str) -> bool {
    if name.contains('*') {
        let parts: Vec<&str> = name.split('*').collect();
        let mut at = 0;
        for (i, p) in parts.iter().enumerate() {
            match unit[at..].find(p) {
                Some(found) if i > 0 || found == 0 => at += found + p.len(),
                _ => return false,
            }
        }
        return parts.last().is_some_and(|p| p.is_empty()) || at == unit.len();
    }
    unit == name || unit.strip_suffix(".service") == Some(name)
}

/// What's being typed, if it's a key's value: (`app:`, "net").
pub fn completing(input: &str) -> Option<(&'static str, &str)> {
    if input.ends_with(char::is_whitespace) {
        return None;
    }
    let last = input.rsplit(char::is_whitespace).next()?;
    let last = last.strip_prefix('-').unwrap_or(last);
    let (key, partial) = last.split_once(':')?;
    let key = match key.to_ascii_lowercase().as_str() {
        "app" | "from" | "source" | "src" => "app:",
        "unit" | "u" => "unit:",
        "level" | "priority" | "p" | "lvl" => "level:",
        _ => return None,
    };
    Some((key, partial))
}

/// `input` with what's being typed replaced by `value` (and a space).
pub fn complete(input: &str, value: &str) -> String {
    let start = input.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let last = &input[start..];
    let neg = if last.starts_with('-') { "-" } else { "" };
    let key = last.trim_start_matches('-').split(':').next().unwrap_or("");
    let value = if value.contains(char::is_whitespace) { format!("\"{value}\"") } else { value.to_owned() };
    format!("{}{neg}{key}:{value} ", &input[..start])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Cursor, Transport};

    fn entry(source: &str, unit: Option<&str>, message: &str, priority: Priority) -> Entry {
        Entry {
            time: 1_000,
            mono: 0,
            priority,
            transport: Transport::Journal,
            facility: None,
            source: source.into(),
            unit: unit.map(Arc::from),
            about: None,
            pid: 42,
            uid: Some(1000),
            message: message.into(),
            message_id: None,
            shape: 0,
            boot: None,
            cursor: Cursor::None,
            seq: 0,
            flags: 0,
            styles: None,
            launch: None,
        }
    }

    fn clock() -> Clock {
        Clock::fixed("UTC", 1_000_000_000)
    }

    #[test]
    fn words_phrases_and_case() {
        let e = entry("NetworkManager", Some("NetworkManager.service"), "device wlp14s0: state change", Priority::Info);
        let m = |q: &str| Search::parse(q, &clock()).matches(&e);
        assert!(m("state"));
        assert!(!m("STATE"));
        assert!(m("networkmanager"));
        assert!(m("\"state change\""));
        assert!(!m("\"change state\""));
        assert!(m("device -dhcp"));
        assert!(!m("-device"));
        assert!(m("/wlp\\d+s\\d/"));
        assert!(m("/(?i)STATE CHANGE/"));
    }

    #[test]
    fn keys() {
        let e = entry("sshd", Some("sshd.service"), "Failed password for root", Priority::Warning);
        let m = |q: &str| Search::parse(q, &clock()).matches(&e);
        assert!(m("app:sshd"));
        assert!(m("app:SSHD"));
        assert!(!m("-app:sshd"));
        assert!(m("unit:sshd"));
        assert!(m("unit:sshd.service"));
        assert!(m("unit:ssh*"));
        assert!(!m("unit:sshd-keygen"));
        assert!(m("pid:42"));
        assert!(m("level:warning"));
        assert!(!m("level:err"));
        assert!(m("since:@0 until:@1"));
        assert!(!m("since:@1"));
        let s = Search::parse("pid:x level:loud since:soon", &clock());
        assert_eq!(s.problems.len(), 3);
        let s = Search::parse("_SYSTEMD_UNIT=sshd.service hello", &clock());
        assert_eq!(s.fields, [("_SYSTEMD_UNIT".to_owned(), "sshd.service".to_owned())]);
    }

    #[test]
    fn highlights_and_the_journals_pattern() {
        let s = Search::parse("fail pass", &clock());
        assert_eq!(s.highlights("Failed password"), [0..4, 7..11]);
        let g = s.grep().unwrap();
        assert_eq!(g, "^(?=(?i).*fail)(?=(?i).*pass)");
        assert_eq!(Search::parse("app:x fail", &clock()).grep(), None);
    }

    #[test]
    fn completion() {
        assert_eq!(completing("error app:net"), Some(("app:", "net")));
        assert_eq!(completing("error -unit:"), Some(("unit:", "")));
        assert_eq!(completing("error app:net "), None);
        assert_eq!(completing("error"), None);
        assert_eq!(complete("error app:net", "NetworkManager"), "error app:NetworkManager ");
        assert_eq!(complete("-app:te", "Google Chrome"), "-app:\"Google Chrome\" ");
    }
}
