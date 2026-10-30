//! The key file syntax shared by desktop entries, icon theme `index.theme`
//! files, `mimeapps.list` and `mimeinfo.cache`, as the Desktop Entry
//! Specification 1.5 defines it ("Basic format of the file", "Possible
//! value types").
//!
//! Lines are group headers (`[Group Name]`), entries (`Key=Value`, or
//! `Key[locale]=Value` for a localised value), comments (`#…`) and blank
//! lines. Whitespace around the `=` is ignored, as are `\r` line ends and
//! a byte order mark. Malformed lines (no `=`, an unterminated header, an
//! empty key, entries before the first group) are skipped, never an error.
//! A key repeated in a group keeps its last value and a repeated group is
//! merged into the first, as GLib does.
//!
//! Values are kept raw; [`unescape`] applies the string escapes (`\s`,
//! `\n`, `\t`, `\r`, `\\`) and [`split_list`] splits a `;`-separated list
//! (with `\;` for a literal semicolon). Unknown escapes are kept as written,
//! so sloppy files (`Exec=sh -c "echo \$HOME"`) still mean what their
//! authors meant.

use std::collections::HashMap;

/// One line of a key file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Line<'a> {
    /// `[name]`
    Group(&'a str),
    /// `key=value` or `key[locale]=value`, value still escaped.
    Entry { key: &'a str, locale: Option<&'a str>, value: &'a str },
    /// Blank, a comment, or malformed.
    Other,
}

/// The lines of `text`, a byte order mark dropped.
pub(crate) fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    text.strip_prefix('\u{feff}').unwrap_or(text).lines().map(parse_line)
}

/// Classify one line. Desktop files are mostly translations, tens of
/// thousands of lines on a typical system, so this is one pass over the
/// key's bytes.
pub(crate) fn parse_line(line: &str) -> Line<'_> {
    let line = line.trim_ascii();
    let bytes = line.as_bytes();
    match bytes.first() {
        None | Some(b'#') => return Line::Other,
        Some(b'[') => {
            return match line.get(1..).and_then(|rest| rest.strip_suffix(']')) {
                Some(name)
                    if !name.is_empty() && !name.bytes().any(|b| b == b'[' || b == b']' || b.is_ascii_control()) =>
                {
                    Line::Group(name)
                }
                _ => Line::Other,
            };
        }
        Some(_) => {}
    }
    // Find `=`, and the `[` and `]` of a locale suffix, in the key.
    let (mut open, mut close, mut equals) = (None, None, None);
    for (i, &byte) in bytes.iter().enumerate() {
        match byte {
            b'=' => {
                equals = Some(i);
                break;
            }
            b'[' if open.is_none() => open = Some(i),
            b']' if close.is_none() && open.is_some() => close = Some(i),
            b'[' | b']' => return Line::Other,
            _ => {}
        }
    }
    let Some(equals) = equals else {
        return Line::Other;
    };
    // The byte positions found are ASCII characters, so these slices are
    // at character boundaries.
    let (Some(key_part), Some(value)) = (line.get(..equals), line.get(equals + 1..)) else {
        return Line::Other;
    };
    let key_part = key_part.trim_ascii_end();
    let value = value.trim_ascii_start();
    let (key, locale) = match (open, close) {
        (None, None) => (key_part, None),
        // `]` must end the key part (before any spaces in front of `=`).
        (Some(open), Some(close)) if close + 1 == key_part.len() && close > open + 1 => {
            match (key_part.get(..open), key_part.get(open + 1..close)) {
                (Some(key), Some(locale)) => (key, Some(locale)),
                _ => return Line::Other,
            }
        }
        _ => return Line::Other,
    };
    if key.is_empty() || key.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control()) {
        return Line::Other;
    }
    Line::Entry { key, locale, value }
}

/// A string value with its escapes applied: `\s` space, `\n` newline, `\t`
/// tab, `\r` carriage return, `\\` backslash. Any other backslash is kept
/// with the character after it.
pub(crate) fn unescape(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A list value: split at each `;` not written `\;`, each item unescaped
/// (with `\;` becoming `;`), empty items dropped.
pub(crate) fn split_list(raw: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            ';' => {
                if !item.trim().is_empty() {
                    items.push(std::mem::take(&mut item));
                }
                item.clear();
            }
            '\\' => match chars.next() {
                Some(';') => item.push(';'),
                Some('s') => item.push(' '),
                Some('n') => item.push('\n'),
                Some('t') => item.push('\t'),
                Some('r') => item.push('\r'),
                Some('\\') => item.push('\\'),
                Some(other) => {
                    item.push('\\');
                    item.push(other);
                }
                None => item.push('\\'),
            },
            c => item.push(c),
        }
    }
    if !item.trim().is_empty() {
        items.push(item);
    }
    items
}

/// A boolean value: `true` or `false` (and GLib's `1` and `0`).
pub(crate) fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// A whole key file in memory, for the small files (theme indexes, MIME
/// lists): group name → key (with its `[locale]` suffix, if any) → raw
/// value.
#[derive(Clone, Debug, Default)]
pub(crate) struct KeyFile {
    groups: HashMap<String, HashMap<String, String>>,
}

impl KeyFile {
    pub(crate) fn parse(text: &str) -> KeyFile {
        let mut file = KeyFile::default();
        let mut current: Option<String> = None;
        for line in lines(text) {
            match line {
                Line::Group(name) => {
                    if !file.groups.contains_key(name) {
                        file.groups.insert(name.to_owned(), HashMap::new());
                    }
                    current = Some(name.to_owned());
                }
                Line::Entry { key, locale, value } => {
                    let Some(group) = current.as_ref().and_then(|name| file.groups.get_mut(name)) else {
                        continue;
                    };
                    let key = match locale {
                        Some(locale) => format!("{key}[{locale}]"),
                        None => key.to_owned(),
                    };
                    group.insert(key, value.to_owned());
                }
                Line::Other => {}
            }
        }
        file
    }

    pub(crate) fn group(&self, name: &str) -> Option<&HashMap<String, String>> {
        self.groups.get(name)
    }

    /// The raw value of `key` in `group`.
    pub(crate) fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.groups.get(group)?.get(key).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_classified() {
        assert_eq!(parse_line("[Desktop Entry]"), Line::Group("Desktop Entry"));
        assert_eq!(parse_line("  [Desktop Action new]  \r"), Line::Group("Desktop Action new"));
        assert_eq!(parse_line("[Broken"), Line::Other);
        assert_eq!(parse_line("[]"), Line::Other);
        assert_eq!(parse_line("[a[b]"), Line::Other);
        assert_eq!(parse_line("# Name=x"), Line::Other);
        assert_eq!(parse_line(""), Line::Other);
        assert_eq!(parse_line("no equals sign"), Line::Other);
        assert_eq!(parse_line("Name = Files "), Line::Entry { key: "Name", locale: None, value: "Files" });
        assert_eq!(
            parse_line("Name[sr_RS@latin]=Datoteke"),
            Line::Entry { key: "Name", locale: Some("sr_RS@latin"), value: "Datoteke" }
        );
        assert_eq!(
            parse_line("x-scheme-handler/https=a.desktop;b.desktop;"),
            Line::Entry { key: "x-scheme-handler/https", locale: None, value: "a.desktop;b.desktop;" }
        );
        assert_eq!(parse_line("Exec="), Line::Entry { key: "Exec", locale: None, value: "" });
        assert_eq!(parse_line("=value"), Line::Other);
        assert_eq!(parse_line("Name[]=x"), Line::Other);
        assert_eq!(parse_line("Name[de=x"), Line::Other);
        assert_eq!(parse_line("Name]=x"), Line::Other);
        assert_eq!(parse_line("Name[de]x=y"), Line::Other);
        // `=` in the value is part of the value.
        assert_eq!(parse_line("Exec=env A=1 prog"), Line::Entry { key: "Exec", locale: None, value: "env A=1 prog" });
    }

    #[test]
    fn strings_are_unescaped() {
        assert_eq!(unescape(r"a\sb\nc\td\re\\f"), "a b\nc\td\re\\f");
        assert_eq!(unescape(r"keep \$ and \; and trailing \"), r"keep \$ and \; and trailing \");
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape(r"\s leading"), "  leading");
        assert_eq!(unescape("日本語\\s"), "日本語 ");
    }

    #[test]
    fn lists_split_on_unescaped_semicolons() {
        assert_eq!(split_list("a;b;c;"), ["a", "b", "c"]);
        assert_eq!(split_list("a;b"), ["a", "b"]);
        assert_eq!(split_list(r"one\;two;three"), ["one;two", "three"]);
        assert_eq!(split_list(r"x\sy;;\\;"), ["x y", "\\"]);
        assert_eq!(split_list(""), Vec::<String>::new());
        assert_eq!(split_list(";;"), Vec::<String>::new());
        assert_eq!(split_list(r"end\"), [r"end\"]);
    }

    #[test]
    fn booleans() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool(" 1 "), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("True"), None);
        assert_eq!(parse_bool("yes"), None);
    }

    #[test]
    fn key_files_merge_groups_and_keep_the_last_key() {
        let text = "\u{feff}top=ignored\n[A]\nk=1\nk=2\n[B]\nx=y\n[A]\nm=3\nName[de]=Hallo\r\n";
        let file = KeyFile::parse(text);
        assert_eq!(file.groups.len(), 2);
        assert_eq!(file.get("A", "k"), Some("2"));
        assert_eq!(file.get("A", "m"), Some("3"));
        assert_eq!(file.get("A", "Name[de]"), Some("Hallo"));
        assert_eq!(file.get("A", "top"), None);
        assert_eq!(file.get("B", "x"), Some("y"));
        assert_eq!(file.get("C", "x"), None);
        assert_eq!(file.group("B").map(|g| g.len()), Some(1));
    }
}
