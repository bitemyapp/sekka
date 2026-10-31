//! Bounded copy formatting over immutable content, intended for worker threads.
use crate::hyperlink::{Hyperlink, LinkIdentity, LinkRef};
use crate::{Color, ContentSnapshot, Rgb, Style, Underline, attrs};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CopyFormat {
    Plain,
    Vt,
    Html,
    #[default]
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: (usize, usize),
    /// Exclusive column; `(snapshot.rows, 0)` is accepted only for linear ranges.
    pub end: (usize, usize),
    pub rectangle: bool,
}

#[derive(Clone, Debug)]
pub struct FormatOptions {
    pub format: CopyFormat,
    pub selection: Option<Selection>,
    pub trim: bool,
    pub unwrap: bool,
    /// Aggregate output bytes, including both representations in mixed mode.
    pub max_bytes: usize,
}
impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            format: CopyFormat::Mixed,
            selection: None,
            trim: true,
            unwrap: true,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Default)]
pub struct FormattedCopy {
    pub plain: Option<Vec<u8>>,
    pub html: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    Cancelled,
    LimitExceeded,
    InvalidSelection,
    AllocationFailed,
}
impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "Copy was cancelled",
            Self::LimitExceeded => "Copy exceeds the configured byte limit",
            Self::InvalidSelection => "Selection is outside the captured content",
            Self::AllocationFailed => "Could not allocate the copy buffer",
        })
    }
}
impl std::error::Error for FormatError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HtmlTag {
    Style,
    Link,
}
impl HtmlTag {
    fn closing(self) -> &'static [u8] {
        match self {
            Self::Style => b"</div>",
            Self::Link => b"</a>",
        }
    }
}
struct Output<'a> {
    bytes: Vec<u8>,
    limit: usize,
    cancel: &'a AtomicBool,
    html_tags: Vec<(HtmlTag, Vec<u8>)>,
}
impl Output<'_> {
    fn remember_html_tag(&mut self, tag: HtmlTag, start: usize) -> Result<(), FormatError> {
        let mut opening = Vec::new();
        opening
            .try_reserve(self.bytes.len() - start)
            .map_err(|_| FormatError::AllocationFailed)?;
        opening.extend_from_slice(&self.bytes[start..]);
        self.html_tags
            .try_reserve(1)
            .map_err(|_| FormatError::AllocationFailed)?;
        self.html_tags.push((tag, opening));
        Ok(())
    }
    fn close_html_tag(&mut self, tag: HtmlTag) -> Result<(), FormatError> {
        let Some((inner, opening)) = self.html_tags.pop() else {
            return Ok(());
        };
        self.put(inner.closing())?;
        if inner != tag {
            // A terminal's style and link scopes can cross. Close and reopen
            // the inner element so copied HTML has correctly nested tags.
            self.close_html_tag(tag)?;
            self.put(&opening)?;
            self.html_tags.push((inner, opening));
        }
        Ok(())
    }
    fn put(&mut self, bytes: &[u8]) -> Result<(), FormatError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(FormatError::Cancelled);
        }
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(FormatError::LimitExceeded);
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| FormatError::AllocationFailed)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn spaces(&mut self, mut count: usize) -> Result<(), FormatError> {
        const SPACES: [u8; 256] = [b' '; 256];
        while count > 0 {
            let n = count.min(SPACES.len());
            self.put(&SPACES[..n])?;
            count -= n;
        }
        Ok(())
    }
    fn text(&mut self, text: &str, html: bool) -> Result<(), FormatError> {
        if !html {
            return self.put(text.as_bytes());
        }
        for c in text.chars() {
            self.character(c)?;
        }
        Ok(())
    }
    fn character(&mut self, c: char) -> Result<(), FormatError> {
        match c {
            '<' => self.put(b"&lt;"),
            '>' => self.put(b"&gt;"),
            '&' => self.put(b"&amp;"),
            '"' => self.put(b"&quot;"),
            '\'' => self.put(b"&#39;"),
            c if c.is_ascii() => self.put(&[c as u8]),
            c => self.put(format!("&#{};", c as u32).as_bytes()),
        }
    }
    fn uri_text(&mut self, text: &str) -> Result<(), FormatError> {
        for c in text.chars() {
            if c.is_control() {
                let mut bytes = [0; 4];
                for byte in c.encode_utf8(&mut bytes).as_bytes() {
                    self.put(format!("%{byte:02X}").as_bytes())?;
                }
            } else {
                self.character(c)?;
            }
        }
        Ok(())
    }
    fn uri(&mut self, mut bytes: &[u8]) -> Result<(), FormatError> {
        while !bytes.is_empty() {
            match std::str::from_utf8(bytes) {
                Ok(text) => {
                    self.uri_text(text)?;
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    self.uri_text(
                        std::str::from_utf8(&bytes[..valid]).expect("validated UTF8 prefix"),
                    )?;
                    bytes = &bytes[valid..];
                    let invalid = error.error_len().unwrap_or(bytes.len());
                    for byte in &bytes[..invalid] {
                        self.put(format!("%{byte:02X}").as_bytes())?;
                    }
                    bytes = &bytes[invalid..];
                }
            }
        }
        Ok(())
    }
}

pub fn format_copy(
    snapshot: &ContentSnapshot,
    options: &FormatOptions,
    cancel: &AtomicBool,
) -> Result<FormattedCopy, FormatError> {
    let mut result = FormattedCopy::default();
    if matches!(
        options.format,
        CopyFormat::Plain | CopyFormat::Vt | CopyFormat::Mixed
    ) {
        let kind = if options.format == CopyFormat::Vt {
            CopyFormat::Vt
        } else {
            CopyFormat::Plain
        };
        result.plain = Some(render(
            snapshot,
            options,
            kind,
            cancel,
            options.max_bytes,
            true,
            true,
        )?);
    }
    if matches!(options.format, CopyFormat::Html | CopyFormat::Mixed) {
        let available = options.max_bytes - result.plain.as_ref().map_or(0, Vec::len);
        result.html = Some(render(
            snapshot,
            options,
            CopyFormat::Html,
            cancel,
            available,
            options.format == CopyFormat::Html,
            true,
        )?);
    }
    Ok(result)
}

fn rgb(color: Color, palette: &[Rgb; 256]) -> Option<Rgb> {
    match color {
        Color::Default => None,
        Color::Indexed(i) => Some(palette[i as usize]),
        Color::Rgb(c) => Some(c),
    }
}

fn style_open(
    out: &mut Output<'_>,
    style: Style,
    kind: CopyFormat,
    palette: Option<&[Rgb; 256]>,
) -> Result<(), FormatError> {
    if kind == CopyFormat::Vt {
        out.put(b"\x1b[0m")?;
        for (flag, code) in [
            (attrs::BOLD, 1),
            (attrs::DIM, 2),
            (attrs::ITALIC, 3),
            (attrs::BLINK, 5),
            (attrs::INVERSE, 7),
            (attrs::HIDDEN, 8),
            (attrs::STRIKE, 9),
            (attrs::OVERLINE, 53),
        ] {
            if style.attrs & flag != 0 {
                out.put(format!("\x1b[{code}m").as_bytes())?;
            }
        }
        match style.underline() {
            Underline::None => {}
            Underline::Single => out.put(b"\x1b[4m")?,
            u => out.put(format!("\x1b[4:{}m", u as u8).as_bytes())?,
        }
        for (prefix, color) in [
            (38, style.foreground),
            (48, style.background),
            (58, style.underline_color),
        ] {
            match color {
                Color::Default => {}
                Color::Indexed(i) if palette.is_none() => {
                    out.put(format!("\x1b[{prefix};5;{i}m").as_bytes())?
                }
                _ => {
                    let Rgb(r, g, b) = rgb(color, palette.unwrap_or(&[Rgb(0, 0, 0); 256])).unwrap();
                    out.put(format!("\x1b[{prefix};2;{r};{g};{b}m").as_bytes())?;
                }
            }
        }
    } else {
        let start = out.bytes.len();
        out.put(b"<div style=\"display: inline;")?;
        for (property, color) in [
            ("color", style.foreground),
            ("background-color", style.background),
            ("text-decoration-color", style.underline_color),
        ] {
            match color {
                Color::Default => {}
                Color::Indexed(i) if palette.is_none() => {
                    out.put(format!("{property}: var(--vt-palette-{i});").as_bytes())?
                }
                _ => {
                    let Rgb(r, g, b) = rgb(color, palette.unwrap_or(&[Rgb(0, 0, 0); 256])).unwrap();
                    out.put(format!("{property}: rgb({r}, {g}, {b});").as_bytes())?;
                }
            }
        }
        if style.underline() != Underline::None
            || style.attrs & (attrs::STRIKE | attrs::OVERLINE | attrs::BLINK) != 0
        {
            out.put(b"text-decoration-line:")?;
            if style.underline() != Underline::None {
                out.put(b" underline")?;
            }
            for (flag, text) in [
                (attrs::STRIKE, " line-through"),
                (attrs::OVERLINE, " overline"),
                (attrs::BLINK, " blink"),
            ] {
                if style.attrs & flag != 0 {
                    out.put(text.as_bytes())?;
                }
            }
            out.put(b";")?;
        }
        if let Some(name) = match style.underline() {
            Underline::None => None,
            Underline::Single => Some("solid"),
            Underline::Double => Some("double"),
            Underline::Curly => Some("wavy"),
            Underline::Dotted => Some("dotted"),
            Underline::Dashed => Some("dashed"),
        } {
            out.put(format!("text-decoration-style: {name};").as_bytes())?;
        }
        for (flag, text) in [
            (attrs::BOLD, "font-weight: bold;"),
            (attrs::ITALIC, "font-style: italic;"),
            (attrs::DIM, "opacity: 0.5;"),
            (attrs::HIDDEN, "visibility: hidden;"),
            (attrs::INVERSE, "filter: invert(100%);"),
        ] {
            if style.attrs & flag != 0 {
                out.put(text.as_bytes())?;
            }
        }
        out.put(b"\">")?;
        out.remember_html_tag(HtmlTag::Style, start)?;
    }
    Ok(())
}
fn style_close(out: &mut Output<'_>, kind: CopyFormat) -> Result<(), FormatError> {
    if kind == CopyFormat::Vt {
        out.put(b"\x1b[0m")
    } else {
        out.close_html_tag(HtmlTag::Style)
    }
}
fn link_open(out: &mut Output<'_>, link: &Hyperlink, kind: CopyFormat) -> Result<(), FormatError> {
    if kind == CopyFormat::Html {
        let start = out.bytes.len();
        out.put(b"<a href=\"")?;
        // Preserve Unicode destinations, unlike upstream's byte-wise numeric
        // entities. Undecodable bytes remain lossless URI percent escapes;
        // existing ASCII percent escapes are left intact.
        out.uri(&link.uri)?;
        out.put(b"\">")?;
        out.remember_html_tag(HtmlTag::Link, start)
    } else {
        out.put(b"\x1b]8;")?;
        if let LinkIdentity::Explicit(id) = &link.id {
            out.put(b"id=")?;
            out.put(id)?;
        }
        out.put(b";")?;
        out.put(&link.uri)?;
        out.put(b"\x1b\\")
    }
}
fn link_close(out: &mut Output<'_>, kind: CopyFormat) -> Result<(), FormatError> {
    if kind == CopyFormat::Html {
        out.close_html_tag(HtmlTag::Link)
    } else {
        out.put(b"\x1b]8;;\x1b\\")
    }
}

fn render(
    snapshot: &ContentSnapshot,
    options: &FormatOptions,
    kind: CopyFormat,
    cancel: &AtomicBool,
    limit: usize,
    defaults: bool,
    resolve_palette: bool,
) -> Result<Vec<u8>, FormatError> {
    let mut out = Output {
        bytes: Vec::new(),
        limit,
        cancel,
        html_tags: Vec::new(),
    };
    out.put(&[])?;
    let mut selection = options.selection.unwrap_or(Selection {
        start: (0, 0),
        end: (snapshot.rows.saturating_sub(1), snapshot.cols),
        rectangle: false,
    });
    if selection.start > selection.end {
        std::mem::swap(&mut selection.start, &mut selection.end);
    }
    if selection.end == (snapshot.rows, 0) && snapshot.rows > 0 && !selection.rectangle {
        selection.end = (snapshot.rows - 1, snapshot.cols);
    }
    if snapshot.rows == 0 {
        return Ok(out.bytes);
    }
    if selection.start.0 >= snapshot.rows
        || selection.end.0 >= snapshot.rows
        || selection.start.1 > snapshot.cols
        || selection.end.1 > snapshot.cols
    {
        return Err(FormatError::InvalidSelection);
    }
    if selection.rectangle && selection.start.1 > selection.end.1 {
        std::mem::swap(&mut selection.start.1, &mut selection.end.1);
    }
    let html = kind == CopyFormat::Html;
    let styled = kind != CopyFormat::Plain;
    if html {
        out.put(b"<div style=\"font-family: monospace; white-space: pre;")?;
        if defaults {
            let Rgb(r, g, b) = snapshot.default_background;
            out.put(format!("background-color: #{r:02x}{g:02x}{b:02x};").as_bytes())?;
            let Rgb(r, g, b) = snapshot.default_foreground;
            out.put(format!("color: #{r:02x}{g:02x}{b:02x};").as_bytes())?;
        }
        out.put(b"\">")?;
    } else if kind == CopyFormat::Vt && defaults {
        for (command, Rgb(r, g, b)) in [
            (10, snapshot.default_foreground),
            (11, snapshot.default_background),
        ] {
            out.put(format!("\x1b]{command};rgb:{r:02x}/{g:02x}/{b:02x}\x1b\\").as_bytes())?;
        }
    }
    let mut style = Style::default();
    let mut link: Option<LinkRef> = None;
    let mut blank_rows = 0usize;
    let mut blanks = 0usize;
    let mut rows = snapshot.rows_range(selection.start.0..selection.end.0 + 1);
    let mut row_index = selection.start.0;
    while let Some(row) = rows
        .next_cancellable(cancel)
        .map_err(|_| FormatError::Cancelled)?
    {
        let mut start = if selection.rectangle || row_index == selection.start.0 {
            selection.start.1
        } else {
            0
        };
        let end = if selection.rectangle || row_index == selection.end.0 {
            selection.end.1
        } else {
            snapshot.cols
        };
        row_index += 1;
        if start >= end {
            continue;
        }
        if row.cells()[start].width() == 0 {
            start = start.saturating_sub(1);
        }
        if row.cells()[start].is_spacer_head() {
            continue;
        }
        if !row.cells()[start..end].iter().any(|cell| {
            cell.has_text()
                || (kind == CopyFormat::Vt
                    && cell.background_only()
                    && row.style(*cell).background != Color::Default)
        }) {
            blank_rows += 1;
            continue;
        }
        if blank_rows > 0 {
            if styled && style != Style::default() {
                style_close(&mut out, kind)?;
                style = Style::default();
            }
            for _ in 0..blank_rows {
                out.put(if kind == CopyFormat::Vt {
                    b"\r\n"
                } else {
                    b"\n"
                })?;
            }
            blank_rows = 0;
        }
        let unwrap = options.unwrap && !selection.rectangle;
        if !row.wrapped() || !unwrap {
            blank_rows += 1;
        }
        if !row.wrap_continuation || !unwrap {
            blanks = 0;
        }
        let mut col = start;
        while col < end {
            let cell = row.cells()[col];
            if cell.width() == 0 || cell.is_spacer_head() {
                col += 1;
                continue;
            }
            let cell_style = row.style(cell);
            let background = cell.background_only() && cell_style.background != Color::Default;
            if (!styled || (!cell.has_text() && !background && cell_style == Style::default()))
                && (!cell.has_text() || (cell.character() == ' ' && options.trim))
            {
                blanks += 1;
                col += 1;
                continue;
            }
            if kind == CopyFormat::Vt && blanks > 0 && blanks <= col - start {
                out.put(format!("\x1b[{blanks}C").as_bytes())?;
                blanks = 0;
            }
            if blanks > 0 {
                if kind == CopyFormat::Vt {
                    if style != Style::default() {
                        style_close(&mut out, kind)?;
                        style = Style::default();
                    }
                    if link.take().is_some() {
                        link_close(&mut out, kind)?;
                    }
                }
                out.spaces(blanks)?;
                blanks = 0;
            }
            if styled && cell_style != style {
                if style != Style::default() && (html || cell_style == Style::default()) {
                    style_close(&mut out, kind)?;
                }
                style = cell_style;
                if style != Style::default() {
                    style_open(
                        &mut out,
                        style,
                        kind,
                        resolve_palette.then_some(&snapshot.palette),
                    )?;
                }
            }
            if styled {
                let next = row.hyperlink_ref(col);
                let same = match (&link, next) {
                    (None, None) => true,
                    (Some(a), Some(b)) => LinkRef::ptr_eq(a, b) || **a == **b,
                    _ => false,
                };
                if !same {
                    if link.is_some() && (html || next.is_none()) {
                        link_close(&mut out, kind)?;
                    }
                    link = next.cloned();
                    if let Some(link) = &link {
                        link_open(&mut out, link, kind)?;
                    }
                }
            }
            if kind == CopyFormat::Vt && background {
                let mut run = 1;
                while col + run < end
                    && row.cells()[col + run].background_only()
                    && row.style(row.cells()[col + run]).background == cell_style.background
                {
                    run += 1;
                }
                out.put(format!("\x1b[{run}X\x1b[{run}C").as_bytes())?;
                col += run;
            } else {
                if let Some(text) = row.grapheme(col) {
                    out.text(text, html)?;
                } else {
                    let mut bytes = [0; 4];
                    out.text(cell.character().encode_utf8(&mut bytes), html)?;
                }
                col += 1;
            }
        }
    }
    if styled && style != Style::default() {
        style_close(&mut out, kind)?;
    }
    if link.is_some() {
        link_close(&mut out, kind)?;
    }
    if html {
        out.put(b"</div>")?;
    }
    if kind == CopyFormat::Vt && !options.trim {
        for _ in 1..blank_rows {
            out.put(b"\r\n")?;
        }
    }
    Ok(out.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Terminal;
    use serde_json::Value;

    fn bytes(value: &Value) -> Vec<u8> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect()
    }

    #[test]
    fn content_formats_match_pinned_ghostty() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../../upstream/hyperlink-fixtures.json"))
                .unwrap();
        let cancel = AtomicBool::new(false);
        let mut failures = Vec::new();
        for case in fixtures["cases"].as_array().unwrap() {
            let mut terminal = Terminal::new(
                case["initial_cols"]
                    .as_u64()
                    .unwrap_or(case["cols"].as_u64().unwrap()) as usize,
                case["initial_rows"]
                    .as_u64()
                    .unwrap_or(case["rows"].as_u64().unwrap()) as usize,
                128,
            );
            terminal.feed(&bytes(&case["input"]));
            if let Some(sizes) = case["resizes"].as_array() {
                for size in sizes {
                    terminal.resize(
                        size[0].as_u64().unwrap() as usize,
                        size[1].as_u64().unwrap() as usize,
                    );
                }
            }
            let snapshot = ContentSnapshot::from(terminal.snapshot());
            let mut options = FormatOptions {
                trim: case["trim"].as_bool().unwrap_or(true),
                unwrap: case["unwrap"].as_bool().unwrap_or(true),
                ..FormatOptions::default()
            };
            if !case["selection"].is_null() {
                let s = &case["selection"];
                options.selection = Some(Selection {
                    start: (
                        s["start"][0].as_u64().unwrap() as usize,
                        s["start"][1].as_u64().unwrap() as usize,
                    ),
                    end: (
                        s["end"][0].as_u64().unwrap() as usize,
                        s["end"][1].as_u64().unwrap() as usize,
                    ),
                    rectangle: s["rectangle"].as_bool().unwrap(),
                });
            }
            for (name, kind) in [
                ("plain", CopyFormat::Plain),
                ("vt", CopyFormat::Vt),
                ("html", CopyFormat::Html),
            ] {
                let actual = render(
                    &snapshot,
                    &options,
                    kind,
                    &cancel,
                    1024 * 1024,
                    false,
                    false,
                )
                .unwrap();
                let repaired = name == "html" && !case["formats"]["html_corrected"].is_null();
                if repaired {
                    assert!(matches!(
                        case["name"].as_str().unwrap(),
                        "style-link-crossed"
                            | "link-style-crossed"
                            | "style-link-transitions-selection(0, 1)-(0, 4)-rectFalse"
                            | "style-link-transitions-selection(0, 2)-(1, 3)-rectFalse"
                            | "style-link-transitions-selection(0, 2)-(1, 3)-rectTrue"
                            | "style-link-transitions-selection(0, 3)-(1, 2)-rectTrue"
                            | "raw-uri"
                            | "unicode-uri"
                            | "unicode-uri-percent"
                            | "invalid-uri-truncated"
                            | "unicode-uri-control"
                    ));
                }
                let expected =
                    bytes(&case["formats"][if repaired { "html_corrected" } else { name }]);
                if actual != expected {
                    failures.push(format!(
                        "{} {name}: actual {:?}; expected {:?}",
                        case["name"],
                        String::from_utf8_lossy(&actual),
                        String::from_utf8_lossy(&expected)
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn output_budget_is_aggregate_and_cancellation_is_explicit() {
        let mut terminal = Terminal::new(10, 2, 128);
        terminal.feed(b"hello");
        let snapshot = terminal.content_snapshot();
        let cancel = AtomicBool::new(false);
        let output = format_copy(&snapshot, &FormatOptions::default(), &cancel).unwrap();
        let total = output.plain.as_ref().unwrap().len() + output.html.as_ref().unwrap().len();
        assert!(
            format_copy(
                &snapshot,
                &FormatOptions {
                    max_bytes: total,
                    ..FormatOptions::default()
                },
                &cancel
            )
            .is_ok()
        );
        assert!(matches!(
            format_copy(
                &snapshot,
                &FormatOptions {
                    max_bytes: total - 1,
                    ..FormatOptions::default()
                },
                &cancel
            ),
            Err(FormatError::LimitExceeded)
        ));
        cancel.store(true, Ordering::Release);
        assert!(matches!(
            format_copy(&snapshot, &FormatOptions::default(), &cancel),
            Err(FormatError::Cancelled)
        ));
    }

    #[test]
    fn copy_uses_captured_defaults_and_palette_with_mixed_background_omitted() {
        let mut terminal = Terminal::new(20, 2, 128);
        let mut colors = crate::TerminalColors {
            foreground: Rgb(0xab, 0xcd, 0xef),
            background: Rgb(0x12, 0x34, 0x56),
            ..crate::TerminalColors::default()
        };
        colors.palette[1] = Rgb(7, 8, 9);
        terminal.set_default_colors(colors);
        terminal.feed(b"hello");
        let captured = terminal.content_snapshot();
        let cancel = AtomicBool::new(false);
        // Directly ported expected strings from pinned formatter.zig tests
        // "Page VT ... colors" and "Page html ... colors" (4270 and 6981).
        let vt = format_copy(
            &captured,
            &FormatOptions {
                format: CopyFormat::Vt,
                ..FormatOptions::default()
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(
            vt.plain.unwrap(),
            b"\x1b]10;rgb:ab/cd/ef\x1b\\\x1b]11;rgb:12/34/56\x1b\\hello"
        );
        let html = format_copy(
            &captured,
            &FormatOptions {
                format: CopyFormat::Html,
                ..FormatOptions::default()
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(html.html.unwrap(),b"<div style=\"font-family: monospace; white-space: pre;background-color: #123456;color: #abcdef;\">hello</div>");
        terminal.feed(b"\r\x1b[31mHELLO");
        let styled = terminal.content_snapshot();
        terminal.feed(b"\x1b]4;1;#ffffff\x1b\\\x1b]10;#000000\x1b\\\x1b]11;#ffffff\x1b\\");
        drop(terminal);
        let mixed = format_copy(&styled, &FormatOptions::default(), &cancel).unwrap();
        assert_eq!(mixed.plain.unwrap(), b"HELLO");
        assert_eq!(mixed.html.unwrap(),b"<div style=\"font-family: monospace; white-space: pre;\"><div style=\"display: inline;color: rgb(7, 8, 9);\">HELLO</div></div>");
    }

    #[test]
    fn selection_bounds_and_empty_selection_are_explicit() {
        let mut terminal = Terminal::new(10, 2, 0);
        terminal.feed(b"hello");
        let content = terminal.content_snapshot();
        let cancel = AtomicBool::new(false);
        for (start, end) in [
            ((0, 11), (0, 12)),
            ((2, 1), (2, 2)),
            ((usize::MAX, 0), (usize::MAX, 1)),
        ] {
            assert!(matches!(
                format_copy(
                    &content,
                    &FormatOptions {
                        selection: Some(Selection {
                            start,
                            end,
                            rectangle: false
                        }),
                        ..FormatOptions::default()
                    },
                    &cancel
                ),
                Err(FormatError::InvalidSelection)
            ));
        }
        let empty = format_copy(
            &content,
            &FormatOptions {
                format: CopyFormat::Plain,
                selection: Some(Selection {
                    start: (0, 3),
                    end: (0, 3),
                    rectangle: false,
                }),
                ..FormatOptions::default()
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(empty.plain.unwrap(), b"");
        let rectangle = format_copy(
            &content,
            &FormatOptions {
                selection: Some(Selection {
                    start: (0, 3),
                    end: (content.rows, 0),
                    rectangle: true,
                }),
                ..FormatOptions::default()
            },
            &cancel,
        );
        assert!(matches!(rectangle, Err(FormatError::InvalidSelection)));
    }
}
