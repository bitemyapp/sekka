//! iCalendar (RFC 5545) as text: content lines unfolded and split into
//! properties with parameters, nested into components; and written back,
//! escaped and folded at 75 octets.
//!
//! This layer knows nothing of events: [`Component`] is any `BEGIN:…`/
//! `END:…` block and [`Prop`] any property, kept with its raw value so
//! what Koyomi doesn't understand survives a round trip untouched.
//! [`value`] reads and writes the value types (dates, date-times,
//! durations, text).

pub mod value;

use std::fmt::Write as _;

/// A property: `NAME;PARAM=a,b;OTHER="x:y":value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prop {
    /// Upper case.
    pub name: String,
    /// Parameter names upper case; values as written, quotes removed.
    pub params: Vec<(String, Vec<String>)>,
    /// The value as written (still escaped, for text).
    pub value: String,
}

impl Prop {
    pub fn new(name: &str, value: impl Into<String>) -> Prop {
        Prop { name: name.to_ascii_uppercase(), params: Vec::new(), value: value.into() }
    }

    /// A text property, its value escaped.
    pub fn text(name: &str, text: &str) -> Prop {
        Prop::new(name, value::escape_text(text))
    }

    pub fn with_param(mut self, name: &str, value: impl Into<String>) -> Prop {
        self.params.push((name.to_ascii_uppercase(), vec![value.into()]));
        self
    }

    /// The first value of parameter `name`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).and_then(|(_, v)| v.first()).map(String::as_str)
    }

    /// The value as text, unescaped.
    pub fn text_value(&self) -> String {
        value::unescape_text(&self.value)
    }
}

/// A component: `BEGIN:VEVENT` … `END:VEVENT`, with its properties and the
/// components inside it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Component {
    /// Upper case: `VCALENDAR`, `VEVENT`, `VALARM`, `VTIMEZONE`, …
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn new(name: &str) -> Component {
        Component { name: name.to_ascii_uppercase(), props: Vec::new(), children: Vec::new() }
    }

    /// The first property called `name`.
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    /// Every property called `name`.
    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name == name)
    }

    /// The first `name` property's text, unescaped.
    pub fn text(&self, name: &str) -> Option<String> {
        self.prop(name).map(Prop::text_value)
    }

    pub fn push(&mut self, prop: Prop) {
        self.props.push(prop);
    }

    /// The components called `name` inside it.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Component> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
}

/// Unfold content lines: a line break followed by a space or a tab
/// continues the line before. Takes CRLF, LF alone and CR alone, and a
/// byte-order mark.
pub fn unfold(text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines: Vec<String> = Vec::new();
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    for raw in normalized.split('\n') {
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        if raw.trim().is_empty() {
            continue;
        }
        lines.push(raw.to_owned());
    }
    lines
}

/// Split one content line into a property, or `None` if it isn't one
/// (no colon, no name).
pub fn parse_line(line: &str) -> Option<Prop> {
    // The name runs to the first ';' or ':'.
    let name_end = line.find([';', ':'])?;
    let name = line[..name_end].trim();
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.') {
        return None;
    }
    // Some writers (Apple's vCard heritage) prefix a group: `item1.NAME`.
    let name = name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase();
    let mut params = Vec::new();
    let bytes = line.as_bytes();
    let mut i = name_end;
    while i < bytes.len() && bytes[i] == b';' {
        i += 1;
        let start = i;
        while i < bytes.len() && bytes[i] != b'=' && bytes[i] != b';' && bytes[i] != b':' {
            i += 1;
        }
        let pname = line[start..i].trim().to_ascii_uppercase();
        let mut values = Vec::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            loop {
                if i < bytes.len() && bytes[i] == b'"' {
                    i += 1;
                    let s = i;
                    while i < bytes.len() && bytes[i] != b'"' {
                        i += 1;
                    }
                    values.push(line[s..i].to_owned());
                    if i < bytes.len() {
                        i += 1;
                    }
                    // Anything between the closing quote and the next
                    // separator is a writer's mistake: skip it.
                    while i < bytes.len() && !matches!(bytes[i], b',' | b';' | b':') {
                        i += 1;
                    }
                } else {
                    let s = i;
                    while i < bytes.len() && !matches!(bytes[i], b',' | b';' | b':') {
                        i += 1;
                    }
                    values.push(line[s..i].to_owned());
                }
                if i < bytes.len() && bytes[i] == b',' {
                    i += 1;
                    continue;
                }
                break;
            }
        }
        if !pname.is_empty() {
            params.push((pname, values));
        }
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    Some(Prop { name, params, value: line[i + 1..].to_owned() })
}

/// Parse iCalendar text into its top-level components (usually one
/// `VCALENDAR`; files that hold several, or bare `VEVENT`s, give them
/// all). Lines that aren't properties are skipped, an `END` that closes
/// nothing open is ignored, and components left open at the end are
/// closed.
pub fn parse(text: &str) -> Vec<Component> {
    let mut top = Vec::new();
    let mut stack: Vec<Component> = Vec::new();
    for line in unfold(text) {
        let Some(prop) = parse_line(&line) else { continue };
        match prop.name.as_str() {
            "BEGIN" => stack.push(Component::new(prop.value.trim())),
            "END" => {
                let name = prop.value.trim().to_ascii_uppercase();
                // Close up to the matching component, if one is open.
                if let Some(at) = stack.iter().rposition(|c| c.name == name) {
                    while stack.len() > at {
                        let done = stack.pop().expect("open component");
                        match stack.last_mut() {
                            Some(parent) => parent.children.push(done),
                            None => top.push(done),
                        }
                    }
                }
            }
            _ => {
                if let Some(c) = stack.last_mut() {
                    c.props.push(prop);
                }
            }
        }
    }
    while let Some(done) = stack.pop() {
        match stack.last_mut() {
            Some(parent) => parent.children.push(done),
            None => top.push(done),
        }
    }
    top
}

/// Whether a parameter value must be quoted.
fn needs_quotes(v: &str) -> bool {
    v.contains([':', ';', ','])
}

/// One property as a content line, unfolded.
pub fn format_prop(p: &Prop) -> String {
    let mut s = p.name.clone();
    for (name, values) in &p.params {
        s.push(';');
        s.push_str(name);
        s.push('=');
        for (i, v) in values.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let v = v.replace('"', "'");
            if needs_quotes(&v) {
                let _ = write!(s, "\"{v}\"");
            } else {
                s.push_str(&v);
            }
        }
    }
    s.push(':');
    s.push_str(&p.value);
    s
}

/// Fold a content line to lines of at most 75 octets (never splitting a
/// UTF-8 character), each ending in CRLF.
pub fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        let n = c.len_utf8();
        if width + n > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += n;
    }
    out.push_str("\r\n");
}

/// Write a component (and what's inside it) as iCalendar text.
pub fn write(c: &Component, out: &mut String) {
    fold(&format!("BEGIN:{}", c.name), out);
    for p in &c.props {
        fold(&format_prop(p), out);
    }
    for k in &c.children {
        write(k, out);
    }
    fold(&format!("END:{}", c.name), out);
}

/// A component as iCalendar text.
pub fn to_string(c: &Component) -> String {
    let mut s = String::new();
    write(c, &mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_and_splits_lines() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Long\r\n  title\r\nDESCRIPTION;LANGUAGE=en:a\\, b\\nc\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let cals = parse(text);
        assert_eq!(cals.len(), 1);
        let ev = &cals[0].children[0];
        assert_eq!(ev.text("SUMMARY").as_deref(), Some("Long title"));
        assert_eq!(ev.text("DESCRIPTION").as_deref(), Some("a, b\nc"));
        assert_eq!(ev.prop("DESCRIPTION").unwrap().param("language"), Some("en"));
    }

    #[test]
    fn quoted_parameters_keep_their_colons() {
        let p = parse_line(
            "ATTENDEE;CN=\"Doe, Jane\";DELEGATED-FROM=\"mailto:a@b.c\",\"mailto:d@e.f\":mailto:jane@example.com",
        )
        .unwrap();
        assert_eq!(p.name, "ATTENDEE");
        assert_eq!(p.param("CN"), Some("Doe, Jane"));
        assert_eq!(p.params[1].1, vec!["mailto:a@b.c".to_owned(), "mailto:d@e.f".to_owned()]);
        assert_eq!(p.value, "mailto:jane@example.com");
        assert_eq!(
            format_prop(&p),
            "ATTENDEE;CN=\"Doe, Jane\";DELEGATED-FROM=\"mailto:a@b.c\",\"mailto:d@e.f\":mailto:jane@example.com"
        );
    }

    #[test]
    fn folds_at_75_octets_on_character_boundaries() {
        let line = format!("SUMMARY:{}", "暦".repeat(40));
        let mut out = String::new();
        fold(&line, &mut out);
        for l in out.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(l.len() <= 75, "{} octets", l.len());
        }
        assert_eq!(unfold(&out), vec![line]);
    }

    #[test]
    fn tolerates_broken_nesting() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:1\nEND:VTODO\nBEGIN:VALARM\nACTION:DISPLAY\nEND:VEVENT\nnot a line\nBEGIN:VEVENT\nUID:2\n";
        let cals = parse(text);
        assert_eq!(cals.len(), 1);
        let evs: Vec<_> = cals[0].children_named("VEVENT").collect();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].children[0].name, "VALARM");
        assert_eq!(evs[1].text("UID").as_deref(), Some("2"));
    }
}
