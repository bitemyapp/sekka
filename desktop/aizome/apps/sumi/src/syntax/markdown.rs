//! Markdown, styled as it's typed: headings, emphasis, code spans and
//! fenced code (coloured in the fence's language), links, lists and task
//! boxes, quotes, rules, tables, HTML comments and YAML front matter.
//!
//! It works a line at a time, as the editor asks, carrying only what
//! spans lines (a fence, front matter, an HTML comment) in the line state.

use super::code::{self, YAML};
use super::{Block, Kind, Language, LineState, Mode, Out};

/// The bytes of leading spaces (a tab counts as one byte, four columns).
fn indent(b: &[u8]) -> (usize, usize) {
    let mut cols = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b' ' => cols += 1,
            b'\t' => cols += 4 - cols % 4,
            _ => break,
        }
        i += 1;
    }
    (i, cols)
}

/// A fence that opens: its character, length and info string's range.
fn fence_open(line: &str, at: usize) -> Option<(u8, usize, std::ops::Range<usize>)> {
    let b = line.as_bytes();
    let ch = *b.get(at)?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let mut len = 0;
    while at + len < b.len() && b[at + len] == ch {
        len += 1;
    }
    if len < 3 {
        return None;
    }
    let rest = &line[at + len..];
    if ch == b'`' && rest.contains('`') {
        return None;
    }
    let start = at + len + (rest.len() - rest.trim_start().len());
    let end = at + len + rest.trim_end().len();
    Some((ch, len, start..end.max(start)))
}

fn fence_closes(line: &str, ch: u8, len: u8) -> bool {
    let (i, cols) = indent(line.as_bytes());
    if cols >= 4 {
        return false;
    }
    let t = &line.as_bytes()[i..];
    let run = t.iter().take_while(|c| **c == ch).count();
    run >= len as usize && t[run..].iter().all(|c| *c == b' ' || *c == b'\t')
}

/// `---`, `***`, `___` (three or more, spaces between allowed).
fn is_rule(t: &str) -> bool {
    let t = t.trim_end();
    let Some(c) = t.chars().next() else { return false };
    if !matches!(c, '-' | '*' | '_') {
        return false;
    }
    t.chars().filter(|x| *x == c).count() >= 3 && t.chars().all(|x| x == c || x == ' ' || x == '\t')
}

/// A list item's marker at the start of `t`: its length with the space
/// after it (`- `, `* `, `+ `, `1. `, `2) `).
pub fn list_marker(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    match b.first()? {
        b'-' | b'*' | b'+' => match b.get(1) {
            Some(b' ') | Some(b'\t') => Some(2),
            None => Some(1),
            _ => None,
        },
        b'0'..=b'9' => {
            let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits > 9 {
                return None;
            }
            match (b.get(digits), b.get(digits + 1)) {
                (Some(b'.') | Some(b')'), Some(b' ') | Some(b'\t')) => Some(digits + 2),
                (Some(b'.') | Some(b')'), None) => Some(digits + 1),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The address of the link at byte `at` of `line`, if one's there:
/// `[text](address)`, `<address>`, or an address written out.
pub fn link_at(line: &str, at: usize) -> Option<String> {
    let mut v = Vec::new();
    inline(line, 0, false, &mut Out(Some(&mut v)));
    let hit = |t: &super::Token| t.range.start <= at && at < t.range.end.max(t.range.start + 1);
    let i = v.iter().position(|t| matches!(t.kind, Kind::LinkText | Kind::Url) && hit(t))?;
    let t = &v[i];
    // [text](address): the address is the Url token after the text.
    let url = match t.kind {
        Kind::Url => &line[t.range.clone()],
        _ => match v.get(i + 1) {
            Some(u) if u.kind == Kind::Url && u.range.start == t.range.end => &line[u.range.clone()],
            _ => &line[t.range.clone()],
        },
    };
    let url = url.trim_start_matches(['(', '<']).trim_end_matches([')', '>']);
    let url = url.split_whitespace().next().unwrap_or("");
    if url.starts_with('[') || url.is_empty() {
        return None;
    }
    Some(if url.starts_with("www.") { format!("https://{url}") } else { url.to_owned() })
}

/// Where a task box sits in a line (`- [ ] buy milk`): its byte range
/// (the brackets) and whether it's ticked.
pub fn task_box(line: &str) -> Option<(std::ops::Range<usize>, bool)> {
    let (mut i, cols) = indent(line.as_bytes());
    if cols >= 16 {
        return None;
    }
    // Quotes may hold lists.
    while line[i..].starts_with('>') {
        i += 1;
        i += line[i..].len() - line[i..].trim_start().len();
    }
    let m = list_marker(&line[i..])?;
    let at = i + m;
    let rest = &line[at..];
    let ticked = if rest.starts_with("[ ]") {
        false
    } else if rest.starts_with("[x]") || rest.starts_with("[X]") {
        true
    } else {
        return None;
    };
    match rest.as_bytes().get(3) {
        None | Some(b' ') | Some(b'\t') => Some((at..at + 3, ticked)),
        _ => None,
    }
}

/// Style line `line` (the first of the file if `first`), starting in
/// `state`.
pub fn line(state: LineState, line: &str, first: bool, out: &mut Out) -> LineState {
    let n = line.len();
    match state.block {
        Block::FrontMatter => {
            let t = line.trim_end();
            if t == "---" || t == "..." {
                out.push(0..n, Kind::FenceMark);
                return LineState::default();
            }
            let mode = code::lex(&YAML, state.mode, line, out);
            return LineState { block: Block::FrontMatter, mode };
        }
        Block::Fence { lang, ch, len } => {
            if fence_closes(line, ch, len) {
                out.push(0..n, Kind::FenceMark);
                return LineState::default();
            }
            return match lang.spec() {
                Some(spec) => LineState { block: state.block, mode: code::lex(spec, state.mode, line, out) },
                None => {
                    out.push(0..n, Kind::Code);
                    state
                }
            };
        }
        Block::Html => {
            return match line.find("-->") {
                Some(e) => {
                    out.push(0..e + 3, Kind::Html);
                    let open = inline(line, e + 3, false, out);
                    if open { LineState { block: Block::Html, mode: Mode::Normal } } else { LineState::default() }
                }
                None => {
                    out.push(0..n, Kind::Html);
                    state
                }
            };
        }
        Block::None => {}
    }
    if first && line.trim_end() == "---" {
        out.push(0..n, Kind::FenceMark);
        return LineState { block: Block::FrontMatter, mode: Mode::Normal };
    }
    let b = line.as_bytes();
    let (ws, cols) = indent(b);
    let t = &line[ws..];
    if cols < 4
        && let Some((ch, len, info)) = fence_open(line, ws)
    {
        out.push(ws..ws + len, Kind::FenceMark);
        out.push(info.clone(), Kind::FenceInfo);
        let word = line[info].split(|c: char| c.is_whitespace() || c == '{' || c == ',').next().unwrap_or("");
        let lang = Language::from_name(word).unwrap_or(Language::Plain);
        return LineState { block: Block::Fence { lang, ch, len: len.min(255) as u8 }, mode: Mode::Normal };
    }
    if cols < 4 && is_rule(t) {
        out.push(0..n, Kind::Rule);
        return LineState::default();
    }
    // A heading: up to six #s, then a space or the end.
    if cols < 4 && t.starts_with('#') {
        let hashes = t.bytes().take_while(|c| *c == b'#').count();
        if hashes <= 6 && matches!(b.get(ws + hashes), None | Some(b' ') | Some(b'\t')) {
            out.push(ws..ws + hashes, Kind::HeadingMark);
            out.push(ws + hashes..n, Kind::Heading);
            let open = inline(line, ws + hashes, false, out);
            return after_inline(open);
        }
    }
    let mut pos = ws;
    // Quotes, nested: "> > text".
    let mut quoted = false;
    while pos < n && b[pos] == b'>' {
        out.push(pos..pos + 1, Kind::QuoteMark);
        pos += 1;
        while pos < n && (b[pos] == b' ' || b[pos] == b'\t') {
            pos += 1;
        }
        quoted = true;
    }
    if quoted {
        out.push(pos..n, Kind::Quote);
    }
    // A list item, perhaps a task.
    if let Some(m) = list_marker(&line[pos..]) {
        out.push(pos..pos + m, Kind::Bullet);
        pos += m;
        let rest = &line[pos..];
        let boxed = (rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]"))
            && matches!(rest.as_bytes().get(3), None | Some(b' ') | Some(b'\t'));
        if boxed {
            out.push(pos..pos + 3, Kind::TaskBox);
            if rest.as_bytes()[1] != b' ' {
                out.push((pos + 4).min(n)..n, Kind::TaskDone);
            }
            pos = (pos + 4).min(n);
        }
    }
    let table = t.starts_with('|');
    let open = inline(line, pos, table, out);
    after_inline(open)
}

fn after_inline(html_open: bool) -> LineState {
    if html_open { LineState { block: Block::Html, mode: Mode::Normal } } else { LineState::default() }
}

/// A run of `*`, `_` or `~` that may open or close emphasis.
#[derive(Clone, Copy, Debug)]
struct Delim {
    at: usize,
    len: usize,
    ch: u8,
    open: bool,
    close: bool,
    /// What's left of it to match (from its inner side).
    left: usize,
}

fn is_punct(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace()))
}

fn is_space(c: Option<char>) -> bool {
    c.is_none_or(char::is_whitespace)
}

/// Inline spans from `from` to the end: code, links, URLs, emphasis,
/// strikethrough, table pipes and HTML comments. Whether an HTML comment
/// is left open at the end.
fn inline(line: &str, from: usize, table: bool, out: &mut Out) -> bool {
    let b = line.as_bytes();
    let n = b.len();
    let mut i = from;
    let mut delims: Vec<Delim> = Vec::new();
    let mut html_open = false;
    // What's only needed for the next line's state: an HTML comment left
    // open. Skip the rest when nobody wants the tokens.
    if !out.wants() {
        let mut k = from;
        while let Some(o) = line[k..].find("<!--") {
            let s = k + o + 4;
            match line[s..].find("-->") {
                Some(e) => k = s + e + 3,
                None => return true,
            }
        }
        return false;
    }
    while i < n {
        let c = b[i];
        match c {
            b'\\' => {
                i += if i + 1 < n && b[i + 1].is_ascii_punctuation() { 2 } else { 1 };
                continue;
            }
            b'`' => {
                let run = b[i..].iter().take_while(|x| **x == b'`').count();
                // The matching run of as many backticks.
                let mut j = i + run;
                let mut close = None;
                while j < n {
                    if b[j] == b'`' {
                        let r = b[j..].iter().take_while(|x| **x == b'`').count();
                        if r == run {
                            close = Some(j + r);
                            break;
                        }
                        j += r;
                    } else {
                        j += 1;
                    }
                }
                match close {
                    Some(end) => {
                        out.push(i..end, Kind::Code);
                        i = end;
                    }
                    None => i += run,
                }
                continue;
            }
            b'<' => {
                if line[i..].starts_with("<!--") {
                    match line[i + 4..].find("-->") {
                        Some(e) => {
                            out.push(i..i + 4 + e + 3, Kind::Html);
                            i += 4 + e + 3;
                        }
                        None => {
                            out.push(i..n, Kind::Html);
                            html_open = true;
                            i = n;
                        }
                    }
                    continue;
                }
                // <https://…> and <me@example.com>.
                if let Some(e) = line[i + 1..].find('>') {
                    let inner = &line[i + 1..i + 1 + e];
                    let auto = !inner.contains(' ')
                        && (inner.contains("://")
                            || (inner.contains('@') && inner.contains('.'))
                            || inner.starts_with("mailto:"));
                    if auto {
                        out.push(i..i + e + 2, Kind::LinkText);
                        i += e + 2;
                        continue;
                    }
                }
            }
            b'!' | b'[' => {
                let s = if c == b'!' { i + 1 } else { i };
                if s < n
                    && b[s] == b'['
                    && let Some(end) = link(line, i, s, out)
                {
                    i = end;
                    continue;
                }
            }
            b'h' | b'w' if i == 0 || !b[i - 1].is_ascii_alphanumeric() => {
                let rest = &line[i..];
                if rest.starts_with("https://") || rest.starts_with("http://") || rest.starts_with("www.") {
                    let mut end =
                        rest.find(|ch: char| ch.is_whitespace() || ch == '<' || ch == '>').map_or(n, |e| i + e);
                    while end > i && matches!(b[end - 1], b'.' | b',' | b';' | b':' | b'!' | b'?' | b')' | b'"' | b'\'')
                    {
                        end -= 1;
                    }
                    out.push(i..end, Kind::LinkText);
                    i = end;
                    continue;
                }
            }
            b'*' | b'_' | b'~' => {
                let run = b[i..].iter().take_while(|x| **x == c).count();
                if c == b'~' && run != 2 {
                    i += run;
                    continue;
                }
                let before = line[..i].chars().next_back();
                let after = line[i + run..].chars().next();
                let left = !is_space(after) && (!is_punct(after) || is_space(before) || is_punct(before));
                let right = !is_space(before) && (!is_punct(before) || is_space(after) || is_punct(after));
                let (open, close) = match c {
                    b'_' => (left && (!right || is_punct(before)), right && (!left || is_punct(after))),
                    _ => (left, right),
                };
                delims.push(Delim { at: i, len: run, ch: c, open, close, left: run });
                i += run;
                continue;
            }
            b'|' if table => out.push(i..i + 1, Kind::TablePipe),
            _ => {}
        }
        i += match c {
            0xf0.. => 4,
            0xe0.. => 3,
            0xc0.. => 2,
            _ => 1,
        };
    }
    emphasis(&mut delims, out);
    html_open
}

/// `[text](url)`, `![alt](src)`, `[text][ref]`, `[ref]: url`: the end, if
/// it's a link.
fn link(line: &str, start: usize, bracket: usize, out: &mut Out) -> Option<usize> {
    let b = line.as_bytes();
    let n = b.len();
    // The matching ], allowing one level of nesting ([a [b] c]).
    let mut depth = 0;
    let mut j = bracket;
    let close = loop {
        if j >= n {
            return None;
        }
        match b[j] {
            b'\\' => j += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break j;
                }
            }
            b'`' => {
                // Code inside link text: skip to its end.
                let r = b[j..].iter().take_while(|x| **x == b'`').count();
                if let Some(e) = line[j + r..].find(&line[j..j + r]) {
                    j += r + e + r - 1;
                }
            }
            _ => {}
        }
        j += 1;
    };
    match b.get(close + 1) {
        Some(b'(') => {
            let mut depth = 0;
            let mut k = close + 1;
            while k < n {
                match b[k] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            if k >= n {
                return None;
            }
            out.push(start..close + 1, Kind::LinkText);
            out.push(close + 1..k + 1, Kind::Url);
            Some(k + 1)
        }
        Some(b'[') => {
            let e = line[close + 1..].find(']')? + close + 1;
            out.push(start..close + 1, Kind::LinkText);
            out.push(close + 1..e + 1, Kind::Url);
            Some(e + 1)
        }
        Some(b':') if start == line.len() - line.trim_start().len() => {
            out.push(start..close + 1, Kind::LinkText);
            out.push(close + 1..n, Kind::Url);
            Some(n)
        }
        _ => None,
    }
}

/// Pair emphasis runs, CommonMark's way in short: a closer takes the
/// nearest opener of its kind, two at a time where both have two.
fn emphasis(delims: &mut [Delim], out: &mut Out) {
    let mut stack: Vec<usize> = Vec::new();
    for d in 0..delims.len() {
        let mut closer = delims[d];
        if closer.close {
            while closer.left > 0 {
                let Some(pos) = stack.iter().rposition(|&o| delims[o].ch == closer.ch && delims[o].left > 0) else {
                    break;
                };
                let o = stack[pos];
                let opener = delims[o];
                // Strikethrough needs two on each side.
                if closer.ch == b'~' && (opener.left < 2 || closer.left < 2) {
                    break;
                }
                let take = if closer.ch == b'~' {
                    2
                } else if opener.left >= 3 && closer.left >= 3 {
                    3
                } else if opener.left >= 2 && closer.left >= 2 {
                    2
                } else {
                    1
                };
                // The opener's markers are taken from its inner end, the
                // closer's from its inner start.
                let o_end = opener.at + opener.left;
                let c_start = closer.at + (closer.len - closer.left);
                out.push(o_end - take..o_end, Kind::EmphasisMark);
                out.push(c_start..c_start + take, Kind::EmphasisMark);
                let kind = match (closer.ch, take) {
                    (b'~', _) => Kind::Strikethrough,
                    (_, 3) => Kind::StrongEmphasis,
                    (_, 2) => Kind::Strong,
                    _ => Kind::Emphasis,
                };
                out.push(o_end..c_start, kind);
                delims[o].left -= take;
                closer.left -= take;
                // Openers between them can't close past this pair.
                stack.truncate(pos + 1);
                if delims[o].left == 0 {
                    stack.pop();
                }
            }
            delims[d] = closer;
        }
        if closer.open && closer.left > 0 {
            stack.push(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{Token, tokens};

    fn kinds(line: &str) -> Vec<(String, Kind)> {
        let (t, _) = tokens(Language::Markdown, LineState::default(), line, 5);
        t.into_iter().map(|Token { range, kind }| (line[range].to_owned(), kind)).collect()
    }

    fn has(line: &str, text: &str, kind: Kind) -> bool {
        let k = kinds(line);
        k.iter().any(|(t, x)| t == text && *x == kind)
    }

    #[test]
    fn the_boards_lines() {
        assert!(has("## Scale", "##", Kind::HeadingMark));
        assert!(has("## Scale", " Scale", Kind::Heading));
        assert!(has("### Damage", "###", Kind::HeadingMark));
        assert!(has("buffer of `points × scale` pixels, so the compositor maps", "`points × scale`", Kind::Code));
        assert!(has("- Clock tick: one rect, the clock's own.", "- ", Kind::Bullet));
        assert!(kinds("#hashtag").is_empty(), "no space, no heading");
    }

    #[test]
    fn emphasis_and_links() {
        assert!(has("an *italic* word", "italic", Kind::Emphasis));
        assert!(has("a **bold** word", "bold", Kind::Strong));
        assert!(has("a **bold** word", "**", Kind::EmphasisMark));
        assert!(has("***both***", "both", Kind::StrongEmphasis));
        assert!(has("_under_ and snake_case_name", "under", Kind::Emphasis));
        assert!(!kinds("snake_case_name").iter().any(|(_, k)| *k == Kind::Emphasis));
        assert!(!kinds("2 * 3 * 4").iter().any(|(_, k)| *k == Kind::Emphasis));
        assert!(has("~~gone~~", "gone", Kind::Strikethrough));
        assert!(has("see [the docs](https://x.org/a_(b)) now", "[the docs]", Kind::LinkText));
        assert!(has("see [the docs](https://x.org/a_(b)) now", "(https://x.org/a_(b))", Kind::Url));
        assert!(has("go to https://example.com/x.", "https://example.com/x", Kind::LinkText));
        assert!(has("![alt](a.png)", "![alt]", Kind::LinkText));
        assert!(has("`a*b*c` *d*", "d", Kind::Emphasis));
        assert!(!kinds("`a*b*c`").iter().any(|(_, k)| *k == Kind::Emphasis), "no emphasis in code");
    }

    #[test]
    fn blocks() {
        assert!(has("> quoted *text*", ">", Kind::QuoteMark));
        assert!(has("> quoted *text*", "quoted *text*", Kind::Quote));
        assert!(has("1. first", "1. ", Kind::Bullet));
        assert!(has("- [x] done thing", "[x]", Kind::TaskBox));
        assert!(has("- [x] done thing", "done thing", Kind::TaskDone));
        assert!(has("- [ ] todo", "[ ]", Kind::TaskBox));
        assert!(has("---", "---", Kind::Rule));
        assert!(has("| a | b |", "|", Kind::TablePipe));
        assert_eq!(task_box("  - [ ] milk"), Some((4..7, false)));
        assert_eq!(task_box("> * [X] eggs"), Some((4..7, true)));
        assert_eq!(task_box("- [ ]x"), None);
        assert_eq!(link_at("see [docs](https://a.org/x \"T\") now", 6).as_deref(), Some("https://a.org/x"));
        assert_eq!(link_at("see [docs](https://a.org/x) now", 15).as_deref(), Some("https://a.org/x"));
        assert_eq!(link_at("go to https://b.org.", 8).as_deref(), Some("https://b.org"));
        assert_eq!(link_at("<https://c.org>", 3).as_deref(), Some("https://c.org"));
        assert_eq!(link_at("plain", 2), None);
    }

    #[test]
    fn fences_colour_their_language() {
        let (t, s) = tokens(Language::Markdown, LineState::default(), "```rust", 3);
        assert_eq!(t[0].kind, Kind::FenceMark);
        assert_eq!(t[1].kind, Kind::FenceInfo);
        assert!(matches!(s.block, Block::Fence { lang: Language::Rust, .. }));
        let (t, s2) = tokens(Language::Markdown, s, "fn main() { /* open", 4);
        assert_eq!(t[0].kind, Kind::Keyword);
        assert_eq!(s2.mode, Mode::Comment { depth: 1 });
        let (_, s3) = tokens(Language::Markdown, s2, "``` still a comment */", 5);
        assert!(matches!(s3.block, Block::Fence { .. }), "a fence closes only alone on its line");
        let (_, s4) = tokens(Language::Markdown, s3, "```", 6);
        assert_eq!(s4, LineState::default());
        // Front matter, first line only.
        let (_, s) = tokens(Language::Markdown, LineState::default(), "---", 0);
        assert_eq!(s.block, Block::FrontMatter);
        let (t, s) = tokens(Language::Markdown, s, "pinned: true", 1);
        assert_eq!(t[0].kind, Kind::Key);
        let (_, s) = tokens(Language::Markdown, s, "---", 2);
        assert_eq!(s, LineState::default());
        // HTML comments over lines.
        let (_, s) = tokens(Language::Markdown, LineState::default(), "a <!-- note", 0);
        assert_eq!(s.block, Block::Html);
        let (t, s) = tokens(Language::Markdown, s, "end --> *b*", 1);
        assert_eq!(t[0].kind, Kind::Html);
        assert_eq!(s, LineState::default());
    }

    #[test]
    fn state_without_tokens_agrees() {
        let lines = ["---", "a: 1", "---", "# T", "```py", "x = '''", "'''", "```", "<!-- a", "b -->", "end"];
        let mut with = LineState::default();
        let mut without = LineState::default();
        for (i, l) in lines.iter().enumerate() {
            let mut v = Vec::new();
            with = line(with, l, i == 0, &mut Out(Some(&mut v)));
            without = line(without, l, i == 0, &mut Out(None));
            assert_eq!(with, without, "line {i}");
        }
    }
}
