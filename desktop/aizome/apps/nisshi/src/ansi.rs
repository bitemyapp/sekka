//! Terminal colours in messages. Programs whose output journald keeps
//! (Aizome's compositor through `tracing`, test runners, build tools) write
//! escape sequences; journald keeps them as they are. A message is read
//! into its plain text, which search, shapes and copying use, and the
//! styles of its runs, which the window draws in the theme's colours.
//! Sequences that aren't styles (cursor movement, titles, hyperlinks' ends)
//! and control characters other than newlines and tabs are left out.

/// A colour, as the window draws it: the theme's, by name, so that what a
/// terminal draws in red is red in Night and in Day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hue {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    /// Black and the greys: quiet text.
    Gray,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub hue: Option<Hue>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    fn plain(&self) -> bool {
        *self == Style::default()
    }
}

/// A styled stretch of the plain text: byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Run {
    pub start: u32,
    pub end: u32,
    pub style: Style,
}

/// The plain text of `s` and its styled runs, if it has escape sequences
/// or control characters; `None` when it's plain already.
pub fn parse(s: &str) -> Option<(String, Vec<Run>)> {
    if !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    let mut runs: Vec<Run> = Vec::new();
    let mut style = Style::default();
    let mut chars = s.chars().peekable();
    let push = |out: &mut String, runs: &mut Vec<Run>, c: char, style: Style| {
        let start = out.len() as u32;
        out.push(c);
        if style.plain() {
            return;
        }
        let end = out.len() as u32;
        match runs.last_mut() {
            Some(r) if r.end == start && r.style == style => r.end = end,
            _ => runs.push(Run { start, end, style }),
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // CSI: parameters, then a final byte; only `m` (styles) counts.
                Some('[') => {
                    let mut params = String::new();
                    for p in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&p) {
                            if p == 'm' {
                                style = sgr(style, &params);
                            }
                            break;
                        }
                        params.push(p);
                    }
                }
                // OSC (titles, hyperlinks): to BEL or ST; a link's text stays.
                Some(']') => {
                    while let Some(p) = chars.next() {
                        if p == '\u{7}' {
                            break;
                        }
                        if p == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // Character sets: one more character.
                Some('(' | ')' | '*' | '+') => {
                    chars.next();
                }
                _ => {}
            },
            '\n' | '\t' => push(&mut out, &mut runs, c, style),
            c if c.is_control() => {}
            c => push(&mut out, &mut runs, c, style),
        }
    }
    Some((out, runs))
}

/// The style after a Select Graphic Rendition's parameters.
fn sgr(mut style: Style, params: &str) -> Style {
    let codes: Vec<u16> =
        if params.is_empty() { vec![0] } else { params.split([';', ':']).map(|p| p.parse().unwrap_or(0)).collect() };
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            22 => (style.bold, style.dim) = (false, false),
            23 => style.italic = false,
            24 => style.underline = false,
            c @ 30..=37 => style.hue = Some(basic(c - 30)),
            c @ 90..=97 => style.hue = Some(basic(c - 90)),
            39 => style.hue = None,
            38 => match codes.get(i + 1) {
                Some(5) => {
                    style.hue = codes.get(i + 2).map(|&n| indexed(n));
                    i += 2;
                }
                Some(2) => {
                    // 38;2;r;g;b, or 38:2::r:g:b with a colour space.
                    let rgb: Vec<u16> = codes[i + 2..].iter().copied().take(4).collect();
                    let rgb = if rgb.len() == 4 && rgb[0] == 0 { &rgb[1..] } else { &rgb[..rgb.len().min(3)] };
                    if let [r, g, b] = rgb {
                        style.hue = Some(nearest(*r as u8, *g as u8, *b as u8));
                    }
                    i += 1 + rgb.len();
                }
                _ => {}
            },
            // Backgrounds, and 48's colour: not drawn.
            48 => match codes.get(i + 1) {
                Some(5) => i += 2,
                Some(2) => i += 4,
                _ => {}
            },
            _ => {}
        }
        i += 1;
    }
    style
}

fn basic(n: u16) -> Hue {
    match n {
        1 => Hue::Red,
        2 => Hue::Green,
        3 => Hue::Yellow,
        4 => Hue::Blue,
        5 => Hue::Magenta,
        6 => Hue::Cyan,
        7 => Hue::White,
        _ => Hue::Gray,
    }
}

/// One of the 256 colours.
fn indexed(n: u16) -> Hue {
    match n {
        0..=7 => basic(n),
        8..=15 => basic(n - 8),
        16..=231 => {
            let n = n - 16;
            let level = |v: u16| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            nearest(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        _ => {
            if n >= 244 {
                Hue::White
            } else {
                Hue::Gray
            }
        }
    }
}

/// The theme's colour nearest an RGB one, by its hue (greys by lightness).
fn nearest(r: u8, g: u8, b: u8) -> Hue {
    let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max - min < 0.15 {
        return if max > 0.6 { Hue::White } else { Hue::Gray };
    }
    let d = max - min;
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    match h {
        h if !(20.0..330.0).contains(&h) => Hue::Red,
        h if h < 70.0 => Hue::Yellow,
        h if h < 165.0 => Hue::Green,
        h if h < 200.0 => Hue::Cyan,
        h if h < 260.0 => Hue::Blue,
        _ => Hue::Magenta,
    }
}

/// The text with its runs written back as escape sequences, as a
/// terminal program writes them (for stand-ins and tests).
pub fn encode(plain: &str, runs: &[Run]) -> String {
    let mut out = String::with_capacity(plain.len() + runs.len() * 12);
    let mut at = 0;
    for r in runs {
        let (start, end) = (r.start as usize, r.end as usize);
        out.push_str(&plain[at..start]);
        let s = r.style;
        let mut codes: Vec<&str> = Vec::new();
        if s.bold {
            codes.push("1");
        }
        if s.dim {
            codes.push("2");
        }
        if s.italic {
            codes.push("3");
        }
        if s.underline {
            codes.push("4");
        }
        if let Some(h) = s.hue {
            codes.push(match h {
                Hue::Red => "31",
                Hue::Green => "32",
                Hue::Yellow => "33",
                Hue::Blue => "34",
                Hue::Magenta => "35",
                Hue::Cyan => "36",
                Hue::White => "37",
                Hue::Gray => "90",
            });
        }
        out.push_str(&format!("\u{1b}[{}m{}\u{1b}[0m", codes.join(";"), &plain[start..end]));
        at = end;
    }
    out.push_str(&plain[at..]);
    out
}

/// The level a program wrote at the start of its line, where journald
/// filed its output at one level (what a service prints goes in as Info):
/// `tracing`'s (`2026-10-29T21:04:01Z  WARN target: …`), log4j's and
/// Python's (`ERROR: …`, `WARNING …`), bracketed (`[warn] …`, `[E] …`).
pub fn written_level(plain: &str) -> Option<crate::entry::Priority> {
    use crate::entry::Priority;
    let words: Vec<&str> = plain.split_whitespace().take(4).collect();
    let level = |w: &str| -> Option<Priority> {
        let w = w.trim_end_matches(':');
        let inner = w.strip_prefix('[').and_then(|w| w.strip_suffix(']'));
        match (w, inner.map(str::to_ascii_lowercase).as_deref()) {
            ("ERROR" | "ERR", _) | (_, Some("error" | "err" | "e")) => Some(Priority::Error),
            ("FATAL" | "CRITICAL" | "CRIT" | "PANIC", _) | (_, Some("fatal" | "critical" | "crit")) => {
                Some(Priority::Critical)
            }
            ("WARN" | "WARNING", _) | (_, Some("warn" | "warning" | "w")) => Some(Priority::Warning),
            ("DEBUG" | "TRACE", _) | (_, Some("debug" | "trace" | "d")) => Some(Priority::Debug),
            ("INFO" | "NOTICE", _) | (_, Some("info" | "notice" | "i")) => Some(Priority::Info),
            _ => None,
        }
    };
    // The first word, or the one after a timestamp (a date and a time).
    let stamp = |w: &str| w.len() >= 8 && w.starts_with(|c: char| c.is_ascii_digit()) && w.contains([':', 'T', '-']);
    if let Some(l) = words.first().and_then(|w| level(w)) {
        return Some(l);
    }
    if words.len() >= 2 && stamp(words[0]) {
        if let Some(l) = level(words[1]) {
            return Some(l);
        }
        if words.len() >= 3 && stamp(words[1]) {
            return level(words[2]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Priority;

    #[test]
    fn tracing_lines() {
        let raw = "\u{1b}[2m2026-10-27T10:07:19.925606Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2maizome_comp_core::shell\u{1b}[0m\u{1b}[2m:\u{1b}[0m window closed \u{1b}[3mid\u{1b}[0m\u{1b}[2m=\u{1b}[0m37";
        let (plain, runs) = parse(raw).unwrap();
        assert_eq!(plain, "2026-10-27T10:07:19.925606Z  INFO aizome_comp_core::shell: window closed id=37");
        let at = |s: &str| plain.find(s).unwrap() as u32;
        assert_eq!(runs[0], Run { start: 0, end: 27, style: Style { dim: true, ..Style::default() } });
        assert_eq!(runs[1].style.hue, Some(Hue::Green));
        assert_eq!((runs[1].start, runs[1].end), (at(" INFO"), at(" INFO") + 5));
        assert!(runs.iter().any(|r| r.style.italic && r.start == at("id=")));
        assert_eq!(written_level(&plain), Some(Priority::Info));
        assert_eq!(parse("plain text"), None);
    }

    #[test]
    fn colours_of_every_kind() {
        let hue = |raw: &str| parse(raw).unwrap().1.first().and_then(|r| r.style.hue);
        assert_eq!(hue("\u{1b}[1;31mfail\u{1b}[0m"), Some(Hue::Red));
        assert_eq!(hue("\u{1b}[93mwarn"), Some(Hue::Yellow));
        assert_eq!(hue("\u{1b}[38;5;34mok"), Some(Hue::Green));
        // Orange reads as a warning.
        assert_eq!(hue("\u{1b}[38;5;208mhot"), Some(Hue::Yellow));
        assert_eq!(hue("\u{1b}[38;5;245mquiet"), Some(Hue::White));
        assert_eq!(hue("\u{1b}[38;2;80;140;255mlink"), Some(Hue::Blue));
        assert_eq!(hue("\u{1b}[38:2::200:60:200mmauve"), Some(Hue::Magenta));
        assert_eq!(hue("\u{1b}[48;5;1m\u{1b}[36mcyan on red"), Some(Hue::Cyan));
        let (plain, runs) = parse("\u{1b}[1mbold\u{1b}[22m then plain").unwrap();
        assert_eq!(plain, "bold then plain");
        assert_eq!(runs, [Run { start: 0, end: 4, style: Style { bold: true, ..Style::default() } }]);
    }

    #[test]
    fn what_isnt_style_goes() {
        // Cursor movement, a title, a hyperlink (its text stays), a
        // progress bar's returns, a bell.
        let (plain, runs) = parse(
            "\u{1b}[2K\u{1b}[1Gbuilding\r\u{1b}]0;title\u{7}see \u{1b}]8;;https://example.org\u{1b}\\docs\u{1b}]8;;\u{1b}\\ now\u{7}\tdone",
        )
        .unwrap();
        assert_eq!(plain, "buildingsee docs now\tdone");
        assert!(runs.is_empty());
        let (plain, _) = parse("caf\u{e9} \u{1b}[31m\u{2713}\u{1b}[0m\nnext").unwrap();
        assert_eq!(plain, "caf\u{e9} \u{2713}\nnext");
    }

    #[test]
    fn written_back_as_read() {
        let raw = "\u{1b}[2m2026\u{1b}[0m \u{1b}[1;32mINFO\u{1b}[0m plain \u{1b}[3;90mid\u{1b}[0m=1";
        let (plain, runs) = parse(raw).unwrap();
        assert_eq!(parse(&encode(&plain, &runs)), Some((plain, runs)));
    }

    #[test]
    fn levels_written_in_the_text() {
        assert_eq!(
            written_level("2026-10-29T21:04:01.470760Z  WARN determinate_nixd::auth: Auth failed"),
            Some(Priority::Warning)
        );
        assert_eq!(written_level("ERROR: could not connect"), Some(Priority::Error));
        assert_eq!(written_level("[warn] slow"), Some(Priority::Warning));
        assert_eq!(written_level("2026-10-29 16:04:52,123 CRITICAL db gone"), Some(Priority::Critical));
        assert_eq!(written_level("2026-10-29 16:04:52 DEBUG x"), Some(Priority::Debug));
        assert_eq!(written_level("The error was harmless"), None);
        assert_eq!(written_level("warn me later"), None);
    }
}
