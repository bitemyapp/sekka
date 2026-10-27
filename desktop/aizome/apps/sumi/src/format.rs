//! The Format menu's commands on Markdown: bold, italic, code and
//! strikethrough wrapped round the selection (or the word at the caret)
//! and taken off again; links; headings, quotes, lists and code blocks on
//! whole lines; Return carrying a list on; and Copy Style / Paste Style.
//!
//! Each command is a function of the text and the selection that gives
//! one [`Edit`]: what to replace, with what, and what to select after, so
//! it undoes as one step.

use std::ops::Range;

use aizome_kit::Selection;
use ropey::Rope;

use crate::syntax::markdown::list_marker;
use crate::syntax::{Kind, Language, LineState, tokens};

/// Replace characters `range` with `text`, then select `select`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
    pub select: Selection,
}

/// A style that wraps text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Inline {
    Bold,
    Italic,
    Code,
    Strike,
}

impl Inline {
    pub fn marker(self) -> &'static str {
        match self {
            Inline::Bold => "**",
            Inline::Italic => "*",
            Inline::Code => "`",
            Inline::Strike => "~~",
        }
    }
}

/// A kind of list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum List {
    Bullet,
    Numbered,
    Task,
}

/// What a line is, before its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockStyle {
    /// 1–6, or 0 for none.
    pub heading: u8,
    pub quote: bool,
    pub list: Option<List>,
}

/// A copied style: the line's and the text's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub block: BlockStyle,
    pub inline: Vec<Inline>,
}

fn line_text(rope: &Rope, line: usize) -> String {
    crate::syntax::cache::line_text(rope, line).into_owned()
}

fn line_end(rope: &Rope, line: usize) -> usize {
    rope.line_to_char(line) + line_text(rope, line).chars().count()
}

/// The lines a selection covers (not the one it ends at the start of).
fn lines_of(rope: &Rope, sel: Selection) -> Range<usize> {
    let r = sel.range();
    let n = rope.len_chars();
    let a = rope.char_to_line(r.start.min(n));
    let mut b = rope.char_to_line(r.end.min(n));
    if b > a && rope.line_to_char(b) == r.end {
        b -= 1;
    }
    a..b + 1
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// Wrap the selection (or the word at the caret) in `style`'s markers, or
/// take them off if it has them.
pub fn toggle_inline(rope: &Rope, sel: Selection, style: Inline) -> Option<Edit> {
    let n = rope.len_chars();
    let r = sel.range();
    let (a, b) = (r.start.min(n), r.end.min(n));
    let lines = lines_of(rope, sel);
    if lines.len() > 1 {
        return if style == Inline::Code { Some(code_block(rope, sel)) } else { per_line(rope, sel, style) };
    }
    let m = style.marker();
    let mc = chars(m);
    let line = lines.start;
    let ls = rope.line_to_char(line);
    let text = line_text(rope, line);
    let cs: Vec<char> = text.chars().collect();
    let (mut a, mut b) = (a - ls, b - ls);
    if a == b {
        // The word at the caret, if there's one.
        let mut s = a;
        while s > 0 && is_word(cs[s - 1]) {
            s -= 1;
        }
        let mut e = a;
        while e < cs.len() && is_word(cs[e]) {
            e += 1;
        }
        if s == e {
            // Nothing to wrap: empty markers to type between, or take
            // empty ones off.
            if a >= mc && a + mc <= cs.len() && marked(&cs, a - mc, m) && marked(&cs, a, m) {
                let at = ls + a - mc;
                return Some(Edit { range: at..at + 2 * mc, text: String::new(), select: Selection::caret(at) });
            }
            let at = ls + a;
            return Some(Edit { range: at..at, text: format!("{m}{m}"), select: Selection::caret(at + mc) });
        }
        let caret = a;
        (a, b) = (s, e);
        let edit = wrap_or_unwrap(&cs, a, b, style)?;
        // The caret stays in its word.
        let shift = if edit.1 { -(mc as isize) } else { mc as isize };
        let at = (caret as isize + shift).max(0) as usize;
        return Some(Edit {
            range: ls + edit.0.start..ls + edit.0.end,
            text: edit.2,
            select: Selection::caret(ls + at),
        });
    }
    // Emphasis can't start or end with a space: wrap what's inside them.
    while a < b && cs[a].is_whitespace() {
        a += 1;
    }
    while b > a && cs[b - 1].is_whitespace() {
        b -= 1;
    }
    let (range, unwrapped, text, inner) = wrap_or_unwrap_sel(&cs, a, b, style)?;
    let select = Selection { anchor: ls + inner.start, head: ls + inner.end };
    let _ = unwrapped;
    Some(Edit { range: ls + range.start..ls + range.end, text, select })
}

fn marked(cs: &[char], at: usize, m: &str) -> bool {
    m.chars().enumerate().all(|(i, c)| cs.get(at + i) == Some(&c))
}

/// The run of `c` before `at` and from `at`.
fn run_before(cs: &[char], at: usize, c: char) -> usize {
    cs[..at].iter().rev().take_while(|x| **x == c).count()
}

fn run_after(cs: &[char], at: usize, c: char) -> usize {
    cs[at..].iter().take_while(|x| **x == c).count()
}

/// Whether the markers of `style` sit right round `a..b`.
fn surrounded(cs: &[char], a: usize, b: usize, style: Inline) -> bool {
    let c = style.marker().chars().next().unwrap_or('*');
    let (before, after) = (run_before(cs, a, c), run_after(cs, b, c));
    match style {
        // `*` alone or the outer one of `***`; not a bold's `**`.
        Inline::Italic => before % 2 == 1 && after % 2 == 1,
        Inline::Bold => before >= 2 && after >= 2,
        Inline::Code => before >= 1 && after >= 1,
        Inline::Strike => before >= 2 && after >= 2,
    }
}

/// For a word: the range to replace, whether markers came off, the text.
fn wrap_or_unwrap(cs: &[char], a: usize, b: usize, style: Inline) -> Option<(Range<usize>, bool, String)> {
    let (range, unwrapped, text, _) = wrap_or_unwrap_sel(cs, a, b, style)?;
    Some((range, unwrapped, text))
}

/// The range to replace, whether markers came off, the text, and where
/// the selected text ends up (line columns).
fn wrap_or_unwrap_sel(
    cs: &[char],
    a: usize,
    b: usize,
    style: Inline,
) -> Option<(Range<usize>, bool, String, Range<usize>)> {
    let m = style.marker();
    let mc = chars(m);
    let inner: String = cs[a..b].iter().collect();
    if a >= mc && surrounded(cs, a, b, style) {
        // Inside the markers: take them off.
        return Some((a - mc..b + mc, true, inner, a - mc..b - mc));
    }
    if b - a >= 2 * mc
        && marked(cs, a, m)
        && marked(cs, b - mc, m)
        && (style != Inline::Italic || !(marked(cs, a, "**") && marked(cs, b - 2, "**")) || b - a >= 6)
    {
        // The markers are selected too.
        let text: String = cs[a + mc..b - mc].iter().collect();
        let len = chars(&text);
        return Some((a..b, true, text, a..a + len));
    }
    let len = b - a;
    Some((a..b, false, format!("{m}{inner}{m}"), a + mc..a + mc + len))
}

/// An inline style on each selected line's text.
fn per_line(rope: &Rope, sel: Selection, style: Inline) -> Option<Edit> {
    let lines = lines_of(rope, sel);
    let start = rope.line_to_char(lines.start);
    let end = line_end(rope, lines.end - 1);
    let m = style.marker();
    // Off if every non-empty line already has it all round.
    let texts: Vec<String> = lines.clone().map(|l| line_text(rope, l)).collect();
    let content = |t: &str| -> Range<usize> {
        // The line's text after its block markers.
        let (prefix, _) = split_prefix(t);
        let p = prefix.len();
        let body = &t[p..];
        let lead = body.len() - body.trim_start().len();
        p + lead..t.trim_end().len().max(p + lead)
    };
    let all_on = texts.iter().filter(|t| !t.trim().is_empty()).all(|t| {
        let r = content(t);
        let s = &t[r];
        s.len() >= 2 * m.len() && s.starts_with(m) && s.ends_with(m)
    });
    let mut out = String::new();
    for (i, t) in texts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let r = content(t);
        if r.is_empty() {
            out.push_str(t);
            continue;
        }
        let s = &t[r.clone()];
        out.push_str(&t[..r.start]);
        if all_on {
            out.push_str(&s[m.len()..s.len() - m.len()]);
        } else {
            out.push_str(m);
            out.push_str(s);
            out.push_str(m);
        }
        out.push_str(&t[r.end..]);
    }
    let len = chars(&out);
    Some(Edit { range: start..end, text: out, select: Selection { anchor: start, head: start + len } })
}

/// `[text](url)` over the selection.
pub fn link(rope: &Rope, sel: Selection, text: &str, url: &str) -> Edit {
    let r = sel.range();
    let n = rope.len_chars();
    let r = r.start.min(n)..r.end.min(n);
    let s = format!("[{text}]({url})");
    let len = chars(&s);
    // Select the link's text if it's empty (to type it), else put the
    // caret after the link.
    let select = if text.is_empty() { Selection::caret(r.start + 1) } else { Selection::caret(r.start + len) };
    Edit { range: r, text: s, select }
}

/// A line split into its block markers (indent, quotes, list marker, task
/// box, heading #s) and the rest.
fn split_prefix(t: &str) -> (&str, BlockStyle) {
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    let mut style = BlockStyle::default();
    while i < b.len() && b[i] == b'>' {
        style.quote = true;
        i += 1;
        if i < b.len() && b[i] == b' ' {
            i += 1;
        }
    }
    if let Some(m) = list_marker(&t[i..]) {
        let marker = t.as_bytes()[i];
        style.list = Some(if marker.is_ascii_digit() { List::Numbered } else { List::Bullet });
        i += m;
        let rest = &t[i..];
        if (rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]"))
            && matches!(rest.as_bytes().get(3), None | Some(b' '))
        {
            style.list = Some(List::Task);
            i = (i + 4).min(t.len());
        }
    }
    let hashes = t[i..].bytes().take_while(|c| *c == b'#').count();
    if (1..=6).contains(&hashes) && matches!(b.get(i + hashes), None | Some(b' ')) {
        style.heading = hashes as u8;
        i = (i + hashes + 1).min(t.len());
    }
    (&t[..i], style)
}

/// A line's prefix for `style`, with `indent` before it and numbered
/// lists counting `n`.
fn prefix_for(indent: &str, style: BlockStyle, n: usize) -> String {
    let mut s = indent.to_owned();
    if style.quote {
        s.push_str("> ");
    }
    match style.list {
        Some(List::Bullet) => s.push_str("- "),
        Some(List::Numbered) => s.push_str(&format!("{n}. ")),
        Some(List::Task) => s.push_str("- [ ] "),
        None => {}
    }
    if style.heading > 0 {
        s.push_str(&"#".repeat(style.heading as usize));
        s.push(' ');
    }
    s
}

/// Change each selected line's block style with `f`; the selection keeps
/// to the same text.
fn restyle_lines(rope: &Rope, sel: Selection, f: impl Fn(BlockStyle, bool) -> BlockStyle) -> Option<Edit> {
    let lines = lines_of(rope, sel);
    let start = rope.line_to_char(lines.start);
    let end = line_end(rope, lines.end - 1);
    let mut out = String::new();
    let mut changed = false;
    let mut number = 0;
    // Where each line's old and new prefixes end, to move the selection.
    let mut shifts: Vec<(usize, usize, usize, usize)> = Vec::new(); // (line start in old, old prefix chars, new line start, new prefix chars)
    let mut new_pos = start;
    for (k, l) in lines.clone().enumerate() {
        let t = line_text(rope, l);
        if k > 0 {
            out.push('\n');
            new_pos += 1;
        }
        let (prefix, old) = split_prefix(&t);
        let indent: String = t.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let blank = t.trim().is_empty();
        let new = f(old, blank);
        if new.list == Some(List::Numbered) {
            number += 1;
        }
        let rest = &t[prefix.len()..];
        let p = if blank && new == BlockStyle::default() { String::new() } else { prefix_for(&indent, new, number) };
        let p = if blank && !new.quote && new.list.is_none() && new.heading == 0 { String::new() } else { p };
        let line = if blank && new.quote && new.list.is_none() { format!("{indent}>") } else { format!("{p}{rest}") };
        if line != t {
            changed = true;
        }
        shifts.push((rope.line_to_char(l), chars(prefix), new_pos, chars(&line) - chars(rest)));
        new_pos += chars(&line);
        out.push_str(&line);
    }
    if !changed {
        return None;
    }
    let map = |pos: usize| -> usize {
        let l = rope.char_to_line(pos.min(rope.len_chars()));
        let k = l.saturating_sub(lines.start).min(shifts.len() - 1);
        let (old_start, old_p, new_start, new_p) = shifts[k];
        let col = pos.saturating_sub(old_start);
        if col <= old_p { new_start + new_p } else { new_start + new_p + (col - old_p) }
    };
    let select = if sel.is_empty() {
        Selection::caret(map(sel.head))
    } else {
        let len = chars(&out);
        let (a, h) = (map(sel.anchor), map(sel.head));
        let whole = sel.range().start <= start && sel.range().end >= end;
        if whole { Selection { anchor: start, head: start + len } } else { Selection { anchor: a, head: h } }
    };
    Some(Edit { range: start..end, text: out, select })
}

/// Make the selected lines a heading of `level` (0: body text); a heading
/// of that level already goes back to body text.
pub fn heading(rope: &Rope, sel: Selection, level: u8) -> Option<Edit> {
    let lines = lines_of(rope, sel);
    let all = lines.clone().all(|l| split_prefix(&line_text(rope, l)).1.heading == level);
    let level = if all && level > 0 { 0 } else { level };
    restyle_lines(rope, sel, |s, _| BlockStyle { heading: level, list: if level > 0 { None } else { s.list }, ..s })
}

/// Quote the selected lines, or unquote them if they all are.
pub fn toggle_quote(rope: &Rope, sel: Selection) -> Option<Edit> {
    let lines = lines_of(rope, sel);
    let all = lines.clone().all(|l| split_prefix(&line_text(rope, l)).1.quote);
    restyle_lines(rope, sel, |s, _| BlockStyle { quote: !all, ..s })
}

/// Make the selected lines a list of `kind`, or plain lines if they are.
pub fn toggle_list(rope: &Rope, sel: Selection, kind: List) -> Option<Edit> {
    let lines = lines_of(rope, sel);
    let all = lines.clone().all(|l| {
        let t = line_text(rope, l);
        t.trim().is_empty() || split_prefix(&t).1.list == Some(kind)
    });
    restyle_lines(rope, sel, |s, blank| {
        if blank { s } else { BlockStyle { list: if all { None } else { Some(kind) }, heading: 0, ..s } }
    })
}

/// Fence the selected lines as a code block.
pub fn code_block(rope: &Rope, sel: Selection) -> Edit {
    let lines = lines_of(rope, sel);
    let start = rope.line_to_char(lines.start);
    let end = line_end(rope, lines.end - 1);
    let body: String = rope.slice(start..end).to_string();
    let text = format!("```\n{body}\n```");
    // The caret after the opening fence, to name the language.
    Edit { range: start..end, text, select: Selection::caret(start + 3) }
}

/// A rule (`---`) on a line of its own after the caret's line.
pub fn rule(rope: &Rope, sel: Selection) -> Edit {
    let line = rope.char_to_line(sel.head.min(rope.len_chars()));
    let end = line_end(rope, line);
    let blank = line_text(rope, line).trim().is_empty();
    let text = if blank { "---\n".to_owned() } else { "\n\n---\n".to_owned() };
    let at = if blank { rope.line_to_char(line) } else { end };
    let len = chars(&text);
    Edit { range: at..if blank { end } else { at }, text, select: Selection::caret(at + len) }
}

/// Return in a list or a quote: the next line starts as this one did
/// (the next number, an empty task box); on an empty item, Return ends the
/// list instead.
pub fn continue_list(rope: &Rope, sel: Selection) -> Option<Edit> {
    if !sel.is_empty() {
        return None;
    }
    let caret = sel.head.min(rope.len_chars());
    let line = rope.char_to_line(caret);
    let ls = rope.line_to_char(line);
    let t = line_text(rope, line);
    let (prefix, style) = split_prefix(&t);
    if style.list.is_none() && !style.quote {
        return None;
    }
    if style.heading > 0 {
        return None;
    }
    let col = caret - ls;
    let pchars = chars(prefix);
    if col < pchars {
        return None;
    }
    let rest = &t[prefix.len()..];
    if rest.trim().is_empty() && col == chars(&t) {
        // An empty item: end the list (keep the quote, if any, once).
        return Some(Edit { range: ls..ls + chars(&t), text: String::new(), select: Selection::caret(ls) });
    }
    let indent: String = t.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let n = if style.list == Some(List::Numbered) {
        let digits: String =
            t.trim_start().trim_start_matches(['>', ' ']).chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<usize>().unwrap_or(0) + 1
    } else {
        0
    };
    let mut next = prefix_for(&indent, BlockStyle { heading: 0, ..style }, n);
    // Keep the marker the list uses (*, +, 2) …).
    if style.list == Some(List::Bullet) || style.list == Some(List::Task) {
        let marker = t.trim_start().trim_start_matches(['>', ' ']).chars().next().unwrap_or('-');
        if matches!(marker, '*' | '+') {
            next = next.replacen("- ", &format!("{marker} "), 1);
        }
    }
    let text = format!("\n{next}");
    let len = chars(&text);
    Some(Edit { range: caret..caret, text, select: Selection::caret(caret + len) })
}

/// The style at the selection: its line's, and the emphasis and code the
/// selection's start is in.
pub fn copy_style(rope: &Rope, sel: Selection) -> Style {
    let at = sel.range().start.min(rope.len_chars());
    let line = rope.char_to_line(at);
    let t = line_text(rope, line);
    let (_, block) = split_prefix(&t);
    let col = at - rope.line_to_char(line);
    let byte = t.char_indices().nth(col).map_or(t.len(), |(b, _)| b);
    let (toks, _) = tokens(Language::Markdown, LineState::default(), &t, line.max(1));
    let mut inline = Vec::new();
    for tk in toks {
        let inside = tk.range.start <= byte && byte < tk.range.end.max(tk.range.start + 1);
        if !inside {
            continue;
        }
        let add: &[Inline] = match tk.kind {
            Kind::Strong => &[Inline::Bold],
            Kind::Emphasis => &[Inline::Italic],
            Kind::StrongEmphasis => &[Inline::Bold, Inline::Italic],
            Kind::Code => &[Inline::Code],
            Kind::Strikethrough => &[Inline::Strike],
            _ => &[],
        };
        for i in add {
            if !inline.contains(i) {
                inline.push(*i);
            }
        }
    }
    Style { block: BlockStyle { heading: block.heading, quote: block.quote, list: block.list }, inline }
}

/// Give the selection a copied style: its lines the block style, its text
/// (on one line) the emphasis and code.
pub fn paste_style(rope: &Rope, sel: Selection, style: &Style) -> Option<Edit> {
    let lines = lines_of(rope, sel);
    let start = rope.line_to_char(lines.start);
    let end = line_end(rope, lines.end - 1);
    // The lines restyled, and where the selection went in them.
    let (mut text, select) =
        match restyle_lines(rope, sel, |_, blank| if blank { BlockStyle::default() } else { style.block }) {
            Some(e) => (e.text, e.select),
            None => (rope.slice(start..end).to_string(), sel),
        };
    let mut select = select;
    if !select.is_empty() && !style.inline.is_empty() && lines.len() == 1 {
        let cs: Vec<char> = text.chars().collect();
        let r = select.range();
        let (a, b) = (r.start - start, r.end - start);
        let mut inner: String = cs[a..b].iter().collect();
        // Markers the text has round it come off first.
        for i in [Inline::Bold, Inline::Italic, Inline::Code, Inline::Strike] {
            let m = i.marker();
            if inner.len() > 2 * m.len() && inner.starts_with(m) && inner.ends_with(m) {
                inner = inner[m.len()..inner.len() - m.len()].to_owned();
            }
        }
        let open: String = style.inline.iter().map(|i| i.marker()).collect();
        let close: String = style.inline.iter().rev().map(|i| i.marker()).collect();
        let before: String = cs[..a].iter().collect();
        let after: String = cs[b..].iter().collect();
        text = format!("{before}{open}{inner}{close}{after}");
        let s = start + a + chars(&open);
        select = Selection { anchor: s, head: s + chars(&inner) };
    }
    if rope.slice(start..end) == text.as_str() {
        return None;
    }
    Some(Edit { range: start..end, text, select })
}

/// Apply an edit to a rope (for tests and for working out what follows).
pub fn apply(rope: &mut Rope, e: &Edit) {
    rope.remove(e.range.clone());
    rope.insert(e.range.start, &e.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, sel: (usize, usize), f: impl Fn(&Rope, Selection) -> Option<Edit>) -> (String, String) {
        let mut r = Rope::from_str(text);
        let s = Selection { anchor: sel.0, head: sel.1 };
        let e = f(&r, s).expect("an edit");
        apply(&mut r, &e);
        let out = r.to_string();
        let selected = r.slice(e.select.range()).to_string();
        (out, selected)
    }

    #[test]
    fn bold_italic_code_toggle() {
        assert_eq!(
            run("a word here", (2, 6), |r, s| toggle_inline(r, s, Inline::Bold)),
            ("a **word** here".into(), "word".into())
        );
        assert_eq!(
            run("a **word** here", (4, 8), |r, s| toggle_inline(r, s, Inline::Bold)),
            ("a word here".into(), "word".into())
        );
        assert_eq!(
            run("a **word** here", (2, 10), |r, s| toggle_inline(r, s, Inline::Bold)),
            ("a word here".into(), "word".into())
        );
        assert_eq!(
            run("a word here", (2, 6), |r, s| toggle_inline(r, s, Inline::Italic)),
            ("a *word* here".into(), "word".into())
        );
        // Italic inside bold adds a star; it doesn't take bold's off.
        assert_eq!(
            run("a **word** here", (4, 8), |r, s| toggle_inline(r, s, Inline::Italic)),
            ("a ***word*** here".into(), "word".into())
        );
        assert_eq!(
            run("a ***word*** here", (5, 9), |r, s| toggle_inline(r, s, Inline::Italic)),
            ("a **word** here".into(), "word".into())
        );
        // The word at the caret.
        let (t, _) = run("a word here", (4, 4), |r, s| toggle_inline(r, s, Inline::Code));
        assert_eq!(t, "a `word` here");
        // Spaces stay outside.
        assert_eq!(run("a word here", (1, 7), |r, s| toggle_inline(r, s, Inline::Bold)).0, "a **word** here");
        // Nothing at the caret: markers to type between.
        let mut r = Rope::from_str("a  b");
        let e = toggle_inline(&r, Selection::caret(2), Inline::Bold).unwrap();
        apply(&mut r, &e);
        assert_eq!((r.to_string().as_str(), e.select), ("a **** b", Selection::caret(4)));
        // Across lines: each line.
        assert_eq!(run("- one\n- two", (0, 11), |r, s| toggle_inline(r, s, Inline::Bold)).0, "- **one**\n- **two**");
        assert_eq!(run("- **one**\n- **two**", (0, 19), |r, s| toggle_inline(r, s, Inline::Bold)).0, "- one\n- two");
        assert_eq!(run("a\nb", (0, 3), |r, s| toggle_inline(r, s, Inline::Code)).0, "```\na\nb\n```");
    }

    #[test]
    fn headings_quotes_lists() {
        assert_eq!(run("Title\ntext", (2, 2), |r, s| heading(r, s, 2)).0, "## Title\ntext");
        assert_eq!(run("## Title", (4, 4), |r, s| heading(r, s, 2)).0, "Title");
        assert_eq!(run("## Title", (4, 4), |r, s| heading(r, s, 3)).0, "### Title");
        assert_eq!(run("## Title", (4, 4), |r, s| heading(r, s, 0)).0, "Title");
        assert_eq!(run("a\nb", (0, 3), toggle_quote).0, "> a\n> b");
        assert_eq!(run("> a\n> b", (0, 7), toggle_quote).0, "a\nb");
        assert_eq!(run("a\nb\nc", (0, 5), |r, s| toggle_list(r, s, List::Numbered)).0, "1. a\n2. b\n3. c");
        assert_eq!(run("- a\n- b", (0, 7), |r, s| toggle_list(r, s, List::Bullet)).0, "a\nb");
        assert_eq!(run("- a", (2, 2), |r, s| toggle_list(r, s, List::Task)).0, "- [ ] a");
        // The caret keeps to its text.
        let mut r = Rope::from_str("Title");
        let e = heading(&r, Selection::caret(3), 1).unwrap();
        apply(&mut r, &e);
        assert_eq!(e.select, Selection::caret(5));
    }

    #[test]
    fn return_carries_lists_on() {
        let (t, _) = run("- one", (5, 5), continue_list);
        assert_eq!(t, "- one\n- ");
        let (t, _) = run("9. nine", (7, 7), continue_list);
        assert_eq!(t, "9. nine\n10. ");
        let (t, _) = run("  * [x] done", (12, 12), continue_list);
        assert_eq!(t, "  * [x] done\n  * [ ] ");
        let (t, _) = run("> quoted", (8, 8), continue_list);
        assert_eq!(t, "> quoted\n> ");
        let (t, _) = run("- one\n- ", (8, 8), continue_list);
        assert_eq!(t, "- one\n", "an empty item ends the list");
        assert!(continue_list(&Rope::from_str("plain"), Selection::caret(5)).is_none());
    }

    #[test]
    fn styles_copy_and_paste() {
        let r = Rope::from_str("## A **bold** word\nplain text");
        let style = copy_style(&r, Selection::caret(8));
        assert_eq!(style.block.heading, 2);
        assert_eq!(style.inline, vec![Inline::Bold]);
        let mut r2 = Rope::from_str("plain text");
        let e = paste_style(&r2, Selection { anchor: 6, head: 10 }, &style).unwrap();
        apply(&mut r2, &e);
        assert_eq!(r2.to_string(), "## plain **text**");
        assert_eq!(r2.slice(e.select.range()).to_string(), "text");
    }

    #[test]
    fn links_and_rules() {
        let mut r = Rope::from_str("see docs now");
        let e = link(&r, Selection { anchor: 4, head: 8 }, "docs", "https://aizome.org");
        apply(&mut r, &e);
        assert_eq!(r.to_string(), "see [docs](https://aizome.org) now");
        let mut r = Rope::from_str("text");
        let e = rule(&r, Selection::caret(2));
        apply(&mut r, &e);
        assert_eq!(r.to_string(), "text\n\n---\n");
    }
}
