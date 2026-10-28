//! Which files a filter lets through: shell-style patterns (`*.png`,
//! `IMG_????.[jJ][pP][gG]`), matched without regard to case as people
//! name files, and MIME types (`image/png`, `image/*`), matched through
//! the shared MIME-info database as Files reads it, subclasses included
//! (`text/plain` takes Markdown and Rust). Folders always show: they're
//! the way to the files.

use hako::model::mime::MimeDb;

use crate::request::{Filter, Rule};

impl Filter {
    /// Whether a file called `name`, of type `mime`, passes.
    pub fn admits(&self, db: &MimeDb, name: &str, mime: &str) -> bool {
        self.rules.iter().any(|r| match r {
            Rule::Glob(p) => glob(p, name),
            Rule::Mime(m) => mime_matches(db, m, mime),
        })
    }

    /// The extension its first pattern names (`*.png` → `png`), for a
    /// name saved under it.
    pub fn extension(&self) -> Option<String> {
        self.rules.iter().find_map(|r| match r {
            Rule::Glob(p) => {
                let ext = p.strip_prefix("*.")?;
                (!ext.is_empty() && ext.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.'))
                    .then(|| ext.to_owned())
            }
            Rule::Mime(_) => None,
        })
    }
}

/// Whether `mime` is of the type `rule` names: the type itself or one of
/// its subclasses, any type of a family (`image/*`), or anything (`*`,
/// `*/*`).
pub fn mime_matches(db: &MimeDb, rule: &str, mime: &str) -> bool {
    let rule = rule.trim().to_ascii_lowercase();
    if rule == "*" || rule == "*/*" {
        return true;
    }
    if let Some(family) = rule.strip_suffix("/*") {
        return mime.split('/').next() == Some(family);
    }
    db.is_a(mime, &rule) || &*db.canonical(&rule) == mime
}

/// Whether `name` matches the shell pattern `pattern` (`*`, `?`, and
/// `[…]` classes with ranges and `!` or `^` negation), ignoring case.
pub fn glob(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut ni) = (0, 0);
    // Where the last `*` was, and where in the name it was tried.
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ni));
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                '[' => {
                    if let Some((hit, end)) = class(&p, pi, n[ni])
                        && hit
                    {
                        pi = end;
                        ni += 1;
                        continue;
                    } else if class(&p, pi, n[ni]).is_none() && n[ni] == '[' {
                        // An unclosed `[` is itself.
                        pi += 1;
                        ni += 1;
                        continue;
                    }
                }
                c if c == n[ni] => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                _ => {}
            }
        }
        // A mismatch: let the last `*` take one more character.
        match star {
            Some((sp, sn)) => {
                pi = sp + 1;
                ni = sn + 1;
                star = Some((sp, sn + 1));
            }
            None => return false,
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// The class starting at `p[at]` (a `[`): whether `c` is in it, and where
/// the pattern goes on after it; `None` when it isn't closed.
fn class(p: &[char], at: usize, c: char) -> Option<(bool, usize)> {
    let mut i = at + 1;
    let negated = matches!(p.get(i), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let mut hit = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((hit != negated, i + 1));
        }
        first = false;
        if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&e| e != ']') {
            if p[i] <= c && c <= p[i + 2] {
                hit = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                hit = true;
            }
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(glob("*.png", "photo.png"));
        assert!(glob("*.png", "PHOTO.PNG"));
        assert!(!glob("*.png", "photo.png.txt"));
        assert!(glob("*", ".hidden"));
        assert!(glob("IMG_????.jpg", "img_0042.JPG"));
        assert!(!glob("IMG_????.jpg", "img_042.jpg"));
        assert!(glob("*.[ch]", "main.c"));
        assert!(glob("*.[a-c]pp", "x.bpp"));
        assert!(!glob("*.[!c]", "main.c"));
        assert!(glob("*.[^c]", "main.h"));
        assert!(glob("a*b*c", "aXXbYYbc"));
        assert!(glob("[]]x", "]x"));
        assert!(glob("[x", "[x"));
        assert!(!glob("", "a"));
        assert!(glob("Makefile", "makefile"));
    }

    #[test]
    fn mime_types() {
        let db = MimeDb::get();
        assert!(mime_matches(db, "image/*", "image/png"));
        assert!(!mime_matches(db, "image/*", "text/plain"));
        assert!(mime_matches(db, "*/*", "application/pdf"));
        assert!(mime_matches(db, "image/png", "image/png"));
        assert!(mime_matches(db, "IMAGE/PNG", "image/png"));
        // Every text type is text.
        assert!(mime_matches(db, "text/plain", "text/markdown"));
        assert!(!mime_matches(db, "image/png", "image/jpeg"));
    }

    #[test]
    fn filters_and_their_extensions() {
        let db = MimeDb::get();
        let f =
            Filter { name: "Images".into(), rules: vec![Rule::Mime("image/jpeg".into()), Rule::Glob("*.png".into())] };
        assert!(f.admits(db, "a.PNG", "image/png"));
        assert!(f.admits(db, "b.jpg", "image/jpeg"));
        assert!(!f.admits(db, "c.txt", "text/plain"));
        assert_eq!(f.extension().as_deref(), Some("png"));
        let any = Filter { name: "All".into(), rules: vec![Rule::Glob("*".into())] };
        assert_eq!(any.extension(), None);
    }
}
