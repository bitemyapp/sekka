//! Find and replace: plain text or a regular expression, with or without
//! case, whole words or not; every match in characters; replacements with
//! `$1` groups when it's a regular expression.

use std::ops::Range;

use regex::{Regex, RegexBuilder};
use ropey::Rope;

/// What's looked for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Query {
    pub text: String,
    pub regex: bool,
    pub case: bool,
    pub word: bool,
}

/// Matches shown (and counted exactly) at most.
pub const LIMIT: usize = 100_000;

impl Query {
    /// The query as a regular expression, or why it isn't one.
    pub fn compile(&self) -> Result<Regex, String> {
        let body = if self.regex { self.text.clone() } else { regex::escape(&self.text) };
        let body = if self.word { format!(r"\b(?:{body})\b") } else { body };
        RegexBuilder::new(&body).case_insensitive(!self.case).multi_line(true).size_limit(1 << 24).build().map_err(
            |e| match e {
                regex::Error::Syntax(s) => {
                    s.lines().last().unwrap_or("Not a regular expression").trim_start_matches("error: ").to_owned()
                }
                regex::Error::CompiledTooBig(_) => "The expression is too big".to_owned(),
                _ => "Not a regular expression".to_owned(),
            },
        )
    }
}

/// The text of a rope, borrowed when it's in one piece.
pub fn text_of(rope: &Rope) -> std::borrow::Cow<'_, str> {
    let s = rope.slice(..);
    match s.as_str() {
        Some(t) => t.into(),
        None => rope.to_string().into(),
    }
}

/// Every non-empty match (in characters), at most `limit` of them.
pub fn search(text: &str, re: &Regex, limit: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    // Characters counted as we go: matches come in order.
    let (mut byte, mut ch) = (0usize, 0usize);
    for m in re.find_iter(text) {
        if m.start() == m.end() {
            continue;
        }
        ch += text[byte..m.start()].chars().count();
        let len = text[m.start()..m.end()].chars().count();
        out.push(ch..ch + len);
        ch += len;
        byte = m.end();
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// What a match of `re` (`matched`, found in `context`) is replaced with:
/// `replacement` as it is, or with its `$1`, `${name}` filled in when the
/// query is a regular expression.
pub fn expand(re: &Regex, matched: &str, replacement: &str, regex: bool) -> String {
    if !regex {
        return replacement.to_owned();
    }
    match re.captures(matched) {
        Some(c) => {
            let mut out = String::new();
            c.expand(replacement, &mut out);
            out
        }
        None => replacement.to_owned(),
    }
}

/// Replace every match in `text`: the span from the first match to the
/// last (characters) and what it becomes, as one edit; and how many.
pub fn replace_all(text: &str, re: &Regex, replacement: &str, regex: bool) -> Option<(Range<usize>, String, usize)> {
    let mut first = None;
    let mut last = 0;
    let mut count = 0;
    let mut out = String::new();
    for c in re.captures_iter(text) {
        let m = c.get(0).expect("the whole match");
        if m.start() == m.end() {
            continue;
        }
        match first {
            None => first = Some(m.start()),
            Some(_) => out.push_str(&text[last..m.start()]),
        }
        if regex {
            c.expand(replacement, &mut out);
        } else {
            out.push_str(replacement);
        }
        last = m.end();
        count += 1;
    }
    let first = first?;
    let a = text[..first].chars().count();
    let b = a + text[first..last].chars().count();
    Some((a..b, out, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> Query {
        Query { text: text.into(), ..Query::default() }
    }

    #[test]
    fn options() {
        let t = "Damage is per layer. damage contains damaged";
        let re = q("damage").compile().unwrap();
        assert_eq!(search(t, &re, LIMIT), [0..6, 21..27, 37..43]);
        let re = Query { case: true, ..q("damage") }.compile().unwrap();
        assert_eq!(search(t, &re, LIMIT), [21..27, 37..43]);
        let re = Query { word: true, ..q("damage") }.compile().unwrap();
        assert_eq!(search(t, &re, LIMIT), [0..6, 21..27]);
        let re = Query { regex: true, ..q(r"d\w+ed") }.compile().unwrap();
        assert_eq!(search(t, &re, LIMIT), vec![37..44]);
        assert!(Query { regex: true, ..q("(") }.compile().is_err());
        assert_eq!(search("é a é", &q("é").compile().unwrap(), LIMIT), [0..1, 4..5], "in characters");
        assert_eq!(search("aaa", &q("a").compile().unwrap(), 2).len(), 2);
    }

    #[test]
    fn replacing() {
        let re = Query { regex: true, ..q(r"(\w+)@(\w+)") }.compile().unwrap();
        assert_eq!(expand(&re, "me@host", "$2 at ${1}", true), "host at me");
        let (r, s, n) = replace_all("x a@b y c@d z", &re, "$2", true).unwrap();
        assert_eq!((r, s.as_str(), n), (2..11, "b y d", 2));
        let re = q("$1").compile().unwrap();
        let (_, s, _) = replace_all("cost $1", &re, "$2", false).unwrap();
        assert_eq!(s, "$2", "plain text stays plain");
        assert!(replace_all("nothing", &re, "x", false).is_none());
    }
}
