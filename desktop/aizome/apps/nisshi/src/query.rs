//! What Nisshi asks of the journal, as the helper receives it, and the
//! journalctl arguments it becomes. A query is data, never arguments: the
//! helper (which may run as root) checks it and builds every argument
//! itself, from a fixed set of options that only read.

use serde::{Deserialize, Serialize};

/// Which boot's messages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Boot {
    #[default]
    Current,
    /// Relative to this one: -1 is the boot before.
    Offset(i32),
    /// A boot's id (32 hexadecimal digits).
    Id(String),
    /// Every boot the journal keeps.
    All,
}

/// A read of the journal's entries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    pub boot: Boot,
    /// `FIELD=VALUE` matches, all of which must hold.
    pub matches: Vec<(String, String)>,
    /// µs since the epoch.
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// Messages matching this (PCRE2, as `journalctl --grep`).
    pub grep: Option<String>,
    /// How many, newest first (the newest before `before`, if given).
    pub limit: Option<u32>,
    /// Older entries than this cursor's, newest first.
    pub before: Option<String>,
    /// Newer entries than this cursor's, oldest first, then new ones as
    /// they're written, until cancelled.
    pub follow_after: Option<String>,
    /// New entries as they're written, from now (with no cursor to follow).
    pub follow: bool,
}

/// The most entries one query reads.
pub const MOST: u32 = 1_000_000;

impl Query {
    /// Whether its entries come newest first.
    pub fn newest_first(&self) -> bool {
        self.follow_after.is_none() && !self.follow
    }

    pub fn validate(&self) -> Result<(), String> {
        match &self.boot {
            Boot::Offset(n) if !(-100_000..=0).contains(n) => return Err(format!("no boot {n}")),
            Boot::Id(id) if !is_boot_id(id) => return Err("a boot's id is 32 hexadecimal digits".into()),
            _ => {}
        }
        for (field, value) in &self.matches {
            check_field(field)?;
            if value.len() > 4096 || value.contains(['\n', '\0']) {
                return Err(format!("the value for {field} can't be matched"));
            }
        }
        if let Some(g) = &self.grep
            && (g.is_empty() || g.len() > 2048 || g.contains('\0'))
        {
            return Err("that pattern can't be searched for".into());
        }
        for cursor in [&self.before, &self.follow_after].into_iter().flatten() {
            check_cursor(cursor)?;
        }
        if self.before.is_some() && (self.follow_after.is_some() || self.follow) {
            return Err("a query reads before a cursor or after one, not both".into());
        }
        if let Some(n) = self.limit
            && !(1..=MOST).contains(&n)
        {
            return Err(format!("a query reads 1 to {MOST} entries"));
        }
        for t in [self.since, self.until].into_iter().flatten() {
            if t < 0 {
                return Err("times are after 1970".into());
            }
        }
        Ok(())
    }

    /// journalctl's arguments for it, after [`validate`](Query::validate).
    pub fn args(&self) -> Vec<String> {
        let mut a = common();
        a.push(format!("--output-fields={}", crate::entry::FIELDS.join(",")));
        match &self.boot {
            Boot::Current => a.push("--boot=0".into()),
            Boot::Offset(n) => a.push(format!("--boot={n}")),
            Boot::Id(id) => a.push(format!("--boot={id}")),
            Boot::All => {}
        }
        if let Some(t) = self.since {
            a.push(format!("--since=@{}", seconds(t)));
        }
        if let Some(t) = self.until {
            a.push(format!("--until=@{}", seconds(t)));
        }
        if let Some(g) = &self.grep {
            a.push(format!("--grep={g}"));
        }
        match (&self.before, &self.follow_after) {
            (Some(c), _) => {
                // From the cursor's entry backwards: one more, as that one
                // comes first (and is left out by the reader).
                a.push(format!("--cursor={c}"));
                a.push("--reverse".into());
                a.push(format!("--lines={}", self.limit.unwrap_or(MOST).saturating_add(1)));
            }
            (None, Some(c)) => {
                a.push(format!("--after-cursor={c}"));
                a.push("--follow".into());
                a.push("--no-tail".into());
            }
            (None, None) if self.follow => {
                a.push("--follow".into());
                a.push("--lines=0".into());
            }
            (None, None) => {
                a.push("--reverse".into());
                if let Some(n) = self.limit {
                    a.push(format!("--lines={n}"));
                }
            }
        }
        // Matches last, after every option (a value can't be read as one:
        // fields are upper case).
        a.extend(self.matches.iter().map(|(f, v)| format!("{f}={v}")));
        a
    }
}

/// What every read passes.
fn common() -> Vec<String> {
    ["--no-pager", "--quiet", "--output=json", "--all"].map(String::from).to_vec()
}

/// One entry with all its fields, by its cursor.
pub fn entry_args(cursor: &str) -> Result<Vec<String>, String> {
    check_cursor(cursor)?;
    let mut a = common();
    a.push(format!("--cursor={cursor}"));
    a.push("--lines=1".into());
    Ok(a)
}

/// The boots the journal keeps.
pub fn boots_args() -> Vec<String> {
    ["--no-pager", "--quiet", "--list-boots", "--output=json"].map(String::from).to_vec()
}

fn seconds(us: i64) -> String {
    format!("{}.{:06}", us.div_euclid(1_000_000), us.rem_euclid(1_000_000))
}

pub fn is_boot_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A journal field's name, as journald allows them: upper case letters,
/// digits and underscores, not starting with a digit, and not the
/// addresses (`__CURSOR`) that aren't fields.
pub fn check_field(f: &str) -> Result<(), String> {
    let ok = !f.is_empty()
        && f.len() <= 64
        && !f.starts_with("__")
        && !f.starts_with(|c: char| c.is_ascii_digit())
        && f.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    if ok { Ok(()) } else { Err(format!("“{f}” isn't a journal field")) }
}

fn check_cursor(c: &str) -> Result<(), String> {
    if c.starts_with(|f: char| f.is_ascii_lowercase())
        && c.len() <= 512
        && c.contains('=')
        && c.bytes().all(|b| b.is_ascii_graphic())
    {
        Ok(())
    } else {
        Err("that isn't a journal cursor".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boots_newest() {
        let q = Query { limit: Some(500), ..Query::default() };
        q.validate().unwrap();
        let a = q.args();
        assert!(a.contains(&"--boot=0".to_owned()));
        assert!(a.contains(&"--reverse".to_owned()));
        assert!(a.contains(&"--lines=500".to_owned()));
        assert!(a.iter().any(|x| x.starts_with("--output-fields=MESSAGE,PRIORITY")));
        assert!(q.newest_first());
    }

    #[test]
    fn following_and_reading_back() {
        let q = Query { follow_after: Some("s=a;i=1".into()), boot: Boot::All, ..Query::default() };
        q.validate().unwrap();
        let a = q.args();
        assert!(a.ends_with(&["--after-cursor=s=a;i=1".into(), "--follow".into(), "--no-tail".into()]));
        assert!(!q.newest_first());
        // With nothing read to follow from: from now.
        let q = Query { follow: true, ..Query::default() };
        q.validate().unwrap();
        assert!(q.args().ends_with(&["--follow".into(), "--lines=0".into()]));
        assert!(!q.newest_first());
        assert!(Query { follow: true, before: Some("s=a".into()), ..Query::default() }.validate().is_err());
        let q = Query {
            before: Some("s=a;i=1".into()),
            limit: Some(10),
            since: Some(1_500_000),
            matches: vec![("_SYSTEMD_UNIT".into(), "sshd.service".into())],
            ..Query::default()
        };
        q.validate().unwrap();
        let a = q.args();
        assert!(a.contains(&"--since=@1.500000".to_owned()));
        assert!(a.contains(&"--lines=11".to_owned()));
        assert_eq!(a.last().map(String::as_str), Some("_SYSTEMD_UNIT=sshd.service"));
    }

    #[test]
    fn nothing_but_reading_gets_through() {
        let bad = [
            Query { matches: vec![("--rotate".into(), "x".into())], ..Query::default() },
            Query { matches: vec![("lower".into(), "x".into())], ..Query::default() },
            Query { matches: vec![("__CURSOR".into(), "x".into())], ..Query::default() },
            Query { matches: vec![("MESSAGE".into(), "a\nb".into())], ..Query::default() },
            Query { before: Some("--vacuum-size=1".into()), ..Query::default() },
            Query { before: Some("s=a b".into()), ..Query::default() },
            Query { boot: Boot::Id("../../etc".into()), ..Query::default() },
            Query { boot: Boot::Offset(4), ..Query::default() },
            Query { grep: Some(String::new()), ..Query::default() },
            Query { limit: Some(0), ..Query::default() },
            Query { since: Some(-1), ..Query::default() },
            Query { before: Some("s=a".into()), follow_after: Some("s=a".into()), ..Query::default() },
        ];
        for q in bad {
            assert!(q.validate().is_err(), "{q:?}");
        }
        assert!(entry_args("--flush").is_err());
        // An option-like value stays one argument after FIELD=.
        let q = Query { matches: vec![("MESSAGE".into(), "--rotate".into())], ..Query::default() };
        q.validate().unwrap();
        assert_eq!(q.args().last().map(String::as_str), Some("MESSAGE=--rotate"));
    }
}
