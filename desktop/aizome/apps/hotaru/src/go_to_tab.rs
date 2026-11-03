//! Go to Tab (❖P): a palette over the window, drawn as the desktop's (❖Space)
//! is, that finds a tab by its title, the program in front in it, its folder
//! or what its screen shows. What it knows besides titles is gathered once, on
//! a thread, as it opens: each pane's screen as shown then, and what /proc
//! says of its shell's terminal. Nothing is gathered while it's closed, and
//! what was is dropped when it closes.

use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aizome_face::look;
use aizome_face::widgets::drawn::{around, drawn};
use aizome_face::widgets::field::field;
use aizome_face::widgets::keys::keys;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, KeyEvent, NamedKey, Wrap};
use aizome_kit::widget::SemanticRole;
use aizome_kit::widgets::layout::{column, row, stack};
use aizome_search::Query;
use aizome_theme::typeface;
use hotaru_core::Snapshot;

use crate::app::Msg;

const WIDTH: f64 = 760.0;
const HEIGHT: f64 = 460.0;
const QUERY: f64 = 52.0;
const FOOTER: f64 = 34.0;
const ROW: f64 = 46.0;
/// Below the tab bar, so the tabs it finds still show.
const TOP: f64 = 40.0;
/// A screen is searched once the query is this long: shorter, nearly every
/// screen has it.
const SCREEN_QUERY: usize = 2;

/// What the palette's elements say.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Query(String),
    /// ↑ ↓ (by rows), Page Up and Page Down.
    Move(isize),
    /// The wheel over the results, in points.
    Scroll(f64),
    /// A row clicked: go to its tab.
    Click(usize),
    Go,
    Close,
}

/// What the palette knows of a pane besides its title, from the thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub pane: u64,
    /// The program in front in its terminal: the shell's own name at a prompt.
    pub program: Option<String>,
    /// Its folder, with `~` for home.
    pub folder: Option<String>,
    /// Its screen when the palette opened, as logical lines (soft wraps
    /// joined), without the blank ones at the bottom.
    pub lines: Vec<String>,
}

/// A pane, as the thread is asked about it.
pub struct Source {
    pub pane: u64,
    /// Its shell's process, while it runs.
    pub pid: Option<u32>,
    /// The last validated local OSC 7 directory; none before the first report
    /// or after an empty report clears it. No startup deadline rules it out.
    pub reported: Option<PathBuf>,
    /// Last known/startup directory, used only if no report or live cwd exists.
    pub fallback: Option<PathBuf>,
    pub snapshot: Arc<Snapshot>,
}

/// Gather each pane's facts: on a thread, never the interface's.
pub fn gather(sources: Vec<Source>, home: Option<PathBuf>) -> Vec<Facts> {
    sources
        .into_iter()
        .map(|source| {
            let front = source.pid.and_then(foreground);
            let folder = source
                .reported
                .or_else(|| source.pid.and_then(|pid| std::fs::read_link(format!("/proc/{pid}/cwd")).ok()))
                .or(source.fallback);
            Facts {
                pane: source.pane,
                program: front.and_then(program),
                folder: folder.map(|folder| tilde(&folder, home.as_deref())),
                lines: screen(&source.snapshot),
            }
        })
        .collect()
}

/// The process group in front on `pid`'s terminal.
fn foreground(pid: u32) -> Option<u32> {
    tpgid(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The terminal's foreground group from /proc/PID/stat: the sixth field after
/// the command's name, which is in parentheses and may hold anything.
fn tpgid(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    // state ppid pgrp session tty_nr tpgid
    let group: i64 = rest.split_whitespace().nth(5)?.parse().ok()?;
    u32::try_from(group).ok().filter(|group| *group > 0)
}

/// A process's name as it was started (its first argument's last component),
/// or as the kernel keeps it.
fn program(pid: u32) -> Option<String> {
    let mut line = Vec::new();
    if let Ok(file) = std::fs::File::open(format!("/proc/{pid}/cmdline")) {
        let _ = file.take(4096).read_to_end(&mut line);
    }
    let name = program_name(&line).or_else(|| {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(comm.trim_end().to_owned()).filter(|name| !name.is_empty())
    })?;
    Some(name.chars().filter(|c| !c.is_control()).take(64).collect())
}

/// The program's name from its command line: `-fish`, as login shells are
/// started, is fish.
fn program_name(cmdline: &[u8]) -> Option<String> {
    let first = cmdline.split(|byte| *byte == 0).next()?;
    let first = String::from_utf8_lossy(first);
    let name = first.rsplit('/').next()?.trim_start_matches('-');
    (!name.is_empty()).then(|| name.to_owned())
}

/// `path` with `~` for `home`.
fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn screen(snapshot: &Snapshot) -> Vec<String> {
    let text = snapshot.plain_text();
    let mut lines: Vec<String> = text.lines().map(|line| line.trim_end().to_owned()).collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

/// A pane the palette may go to.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub tab: u64,
    pub pane: u64,
    /// The tab's place in the bar, from 1.
    pub number: usize,
    /// How many panes its tab has.
    pub panes: usize,
    pub title: String,
    /// It's the pane its tab shows in front.
    pub front: bool,
    pub bell: bool,
    /// Its tab is the one shown.
    pub current: bool,
    /// The keys that show its tab.
    pub shortcut: Option<String>,
    /// How long ago its tab was shown: 0 for the one shown now.
    pub recency: usize,
}

/// A row of the palette: a tab, and the pane it goes to.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub tab: u64,
    pub pane: u64,
    pub number: usize,
    pub panes: usize,
    pub title: String,
    pub title_ranges: Vec<Range<usize>>,
    /// The program and folder, or the line of the screen that matched.
    pub detail: String,
    pub detail_ranges: Vec<Range<usize>>,
    /// The line of the pane's screen with the query on it, which the preview
    /// shows: for a match on the screen alone, the one the row shows.
    pub line: Option<usize>,
    pub bell: bool,
    pub current: bool,
    pub shortcut: Option<String>,
}

/// How well a pane matched, best last: its title, program or folder before
/// its screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    Screen,
    Named,
}

/// "fish · ~/work/sekka".
fn about(facts: Option<&Facts>) -> String {
    let Some(facts) = facts else { return String::new() };
    match (&facts.program, &facts.folder) {
        (Some(program), Some(folder)) => format!("{program} · {folder}"),
        (Some(only), None) | (None, Some(only)) => only.clone(),
        (None, None) => String::new(),
    }
}

/// The tabs for `text`, best first. A tab's title, program and folder match
/// as the desktop's palette matches (fuzzily, words in any order); its panes'
/// screens match the query as written, ignoring ASCII case. Without a query,
/// the tabs come as last shown; with one, the tab shown comes after those
/// matching as well.
pub fn rank(text: &str, candidates: &[Candidate], facts: Option<&[Facts]>) -> Vec<Row> {
    let mut query = Query::new(text);
    let needle = text.trim().to_ascii_lowercase();
    let screens = needle.chars().count() >= SCREEN_QUERY;
    // Per tab: (tier, score, it's the pane in front, how long ago it was shown, the row).
    let mut best: Vec<(Tier, u32, bool, usize, Row)> = Vec::new();
    for candidate in candidates {
        let facts = facts.and_then(|all| all.iter().find(|facts| facts.pane == candidate.pane));
        let about = about(facts);
        let mut row = Row {
            tab: candidate.tab,
            pane: candidate.pane,
            number: candidate.number,
            panes: candidate.panes,
            title: candidate.title.clone(),
            title_ranges: Vec::new(),
            detail: about.clone(),
            detail_ranges: Vec::new(),
            line: None,
            bell: candidate.bell,
            current: candidate.current,
            shortcut: candidate.shortcut.clone(),
        };
        let found = if query.is_empty() {
            // Without a query each tab is its pane in front.
            candidate.front.then_some((Tier::Named, 0))
        } else {
            let named = query.matches(&about);
            let seen = facts.filter(|_| screens).and_then(|facts| on_screen(&needle, &facts.lines));
            row.line = seen.map(|(line, _)| line);
            let title =
                query.matches(&candidate.title).filter(|title| named.as_ref().is_none_or(|n| title.score >= n.score));
            if let Some(title) = title {
                row.title_ranges = title.ranges;
                Some((Tier::Named, title.score))
            } else if let Some(named) = named {
                row.detail_ranges = named.ranges;
                Some((Tier::Named, named.score))
            } else if let Some(facts) = facts
                && let Some((line, at)) = seen
            {
                let (snippet, range) = snippet(&facts.lines[line], at, needle.len());
                row.detail = snippet;
                row.detail_ranges = vec![range];
                Some((Tier::Screen, 0))
            } else {
                None
            }
        };
        let Some((tier, score)) = found else { continue };
        let key = (tier, score, candidate.front);
        match best.iter_mut().find(|(.., row)| row.tab == candidate.tab) {
            Some(entry) if (entry.0, entry.1, entry.2) < key => {
                *entry = (tier, score, candidate.front, candidate.recency, row)
            }
            Some(_) => {}
            None => best.push((tier, score, candidate.front, candidate.recency, row)),
        }
    }
    // With a query, the tab shown already comes last of its tier: it's
    // somewhere else that's wanted.
    let here = |row: &Row| row.current && !query.is_empty();
    best.sort_by(|a, b| b.0.cmp(&a.0).then(here(&a.4).cmp(&here(&b.4))).then(b.1.cmp(&a.1)).then(a.3.cmp(&b.3)));
    best.into_iter().map(|(.., row)| row).collect()
}

/// The last line of `lines` holding `needle` (lowercase), and where: the
/// newest output is at the bottom.
fn on_screen(needle: &str, lines: &[String]) -> Option<(usize, usize)> {
    lines.iter().enumerate().rev().find_map(|(index, line)| {
        // ASCII lowering keeps every byte where it was.
        line.to_ascii_lowercase().find(needle).map(|at| (index, at))
    })
}

/// Enough of `line` round the match at `at` (`len` bytes) to show in a row,
/// and where the match is in it.
fn snippet(line: &str, at: usize, len: usize) -> (String, Range<usize>) {
    const BEFORE: usize = 24;
    let indent = line.len() - line.trim_start().len();
    let mut start = indent.min(at);
    if at - start > BEFORE {
        start = at - BEFORE;
        while !line.is_char_boundary(start) {
            start += 1;
        }
        let shown = format!("…{}", &line[start..]);
        let offset = '…'.len_utf8() + at - start;
        return (shown, offset..offset + len);
    }
    (line[start..].to_owned(), at - start..at - start + len)
}

/// The palette while it's open.
pub struct Palette {
    pub query: String,
    pub selected: usize,
    pub scroll: f64,
    /// Which gathering is this palette's.
    pub token: u64,
    pub facts: Option<Arc<[Facts]>>,
    pub rows: Vec<Row>,
    /// How many tabs there are.
    pub tabs: usize,
}

impl Palette {
    pub fn new(token: u64, tabs: usize) -> Self {
        Palette { query: String::new(), selected: 0, scroll: 0.0, token, facts: None, rows: Vec::new(), tabs }
    }

    /// Rank again, keeping the selected tab selected while it's listed.
    pub fn refresh(&mut self, candidates: &[Candidate], fresh: bool) {
        let kept = self.rows.get(self.selected).map(|row| row.tab).filter(|_| !fresh);
        self.rows = rank(&self.query, candidates, self.facts.as_deref());
        self.selected = match kept.and_then(|tab| self.rows.iter().position(|row| row.tab == tab)) {
            Some(index) => index,
            // Without a query, the tab shown before this one: ↩ goes back.
            None if self.query.trim().is_empty() => usize::from(self.rows.len() > 1),
            None => 0,
        };
        self.reveal();
    }

    pub fn step(&mut self, by: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() as isize - 1;
        self.selected = (self.selected as isize + by).clamp(0, last) as usize;
        self.reveal();
    }

    pub fn scroll_by(&mut self, points: f64) {
        self.scroll = (self.scroll + points).clamp(0.0, self.most());
    }

    fn most(&self) -> f64 {
        (self.rows.len() as f64 * ROW - body_height()).max(0.0)
    }

    /// Scroll so the selected row shows.
    fn reveal(&mut self) {
        let top = self.selected as f64 * ROW;
        if top < self.scroll {
            self.scroll = top;
        }
        if top + ROW > self.scroll + body_height() {
            self.scroll = top + ROW - body_height();
        }
        self.scroll = self.scroll.clamp(0.0, self.most());
    }
}

fn body_height() -> f64 {
    HEIGHT - QUERY - FOOTER - 2.0
}

/// What a key the query leaves does.
pub fn key_event(k: &KeyEvent) -> Option<Event> {
    let m = k.modifiers;
    Some(match &k.key {
        Key::Named(NamedKey::Escape) => Event::Close,
        Key::Named(NamedKey::Enter) => Event::Go,
        Key::Named(NamedKey::ArrowUp) => Event::Move(-1),
        Key::Named(NamedKey::ArrowDown) => Event::Move(1),
        Key::Named(NamedKey::PageUp) => Event::Move(-6),
        Key::Named(NamedKey::PageDown) => Event::Move(6),
        Key::Named(NamedKey::Tab) => Event::Move(if m.shift() { -1 } else { 1 }),
        _ => return None,
    })
}

/// The palette over the window, under the tab bar: a click beside it closes it.
pub fn view(p: &Palette) -> Element<Msg> {
    let scrim =
        drawn(0.0, 0.0, "go-to-tab-scrim", |_| {}).on_click(Msg::GoToTab(Event::Close)).width(Fill).height(Fill);
    let selected = p.rows.get(p.selected);
    let facts = selected.and_then(|row| p.facts.as_deref()?.iter().find(|facts| facts.pane == row.pane));
    let body = row(vec![results(p), detail(selected, facts, p.facts.is_some())]).height(body_height());
    let content = column(vec![query_row(p), body.into(), footer()]).width(Fill);
    let frame = around(content, [1.0, 1.0, 1.0, 1.0], Size::new(0.0, HEIGHT), "go-to-tab-frame", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        look::shadow(cx.canvas, t, b);
        look::panel(cx.canvas, t, b);
    })
    .overflow(3.0)
    .max_width(WIDTH);
    let palette = container(keys(frame, |k| key_event(k).map(Msg::GoToTab)))
        .padding([TOP, 24.0, 24.0, 24.0])
        .align_x(Align::Center)
        .width(Fill);
    stack(vec![scrim.into(), palette.into()]).align_x(Align::Center).width(Fill).height(Fill).into()
}

fn query_row(p: &Palette) -> Element<Msg> {
    let prompt = drawn(22.0, QUERY - 1.0, "go-to-tab-prompt", |cx| {
        let t = cx.theme();
        let s = look::mono(18.0, typeface::WEIGHT_REGULAR, t.focus());
        cx.draw_text_in("❯", &s, cx.bounds(), Align::Start);
    });
    let query = field(&p.query, |t| look::ui(18.0 * t.metrics.factor, typeface::WEIGHT_MEDIUM, t.text()))
        .placeholder("Tabs by title, program, folder or screen…")
        .height(QUERY - 1.0)
        .on_change(|text| Msg::GoToTab(Event::Query(text)))
        .on_submit(Msg::GoToTab(Event::Go));
    let count = match p.tabs {
        1 => "1 tab".to_owned(),
        n => format!("{n} tabs"),
    };
    let context = drawn(0.0, QUERY - 1.0, count.clone(), move |cx| {
        let t = cx.theme();
        let s = look::mono(11.5, typeface::WEIGHT_REGULAR, t.text3());
        cx.draw_text_in(&count, &s, cx.bounds(), Align::End);
    })
    .width(64.0);
    let line = row(vec![prompt.into(), Element::from(query).id("go-to-tab").width(Fill), context.into()]).spacing(10.0);
    around(line, [0.0, 18.0, 1.0, 18.0], Size::new(0.0, QUERY), "go-to-tab-query", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(Rect::new(0.0, b.y1 - 1.0, b.x1, b.y1), t.line());
    })
    .width(Fill)
    .into()
}

/// `text` with `ranges` drawn as the desktop's palette draws what matched.
fn spans(text: &str, ranges: &[Range<usize>], blue: Color) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut at = 0;
    for range in ranges {
        if range.end > text.len() || range.start < at {
            continue;
        }
        if range.start > at {
            spans.push(Span::new(&text[at..range.start]));
        }
        spans.push(Span::new(&text[range.clone()]).color(blue).underline().weight(typeface::WEIGHT_BOLD));
        at = range.end;
    }
    if at < text.len() {
        spans.push(Span::new(&text[at..]));
    }
    spans
}

fn result(r: &Row, index: usize, on: bool) -> Element<Msg> {
    let (tab, label) = (r.tab, r.title.clone());
    let r = r.clone();
    drawn(0.0, ROW, (format!("{r:?}"), on), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        if on {
            cx.canvas.fill_rect(b, t.selection());
            cx.canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, b.y1), t.focus());
        } else if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.raised()));
        }
        let (fg, dim) = if on { (t.on_selection(), t.on_selection().with_alpha(0.78)) } else { (t.text(), t.text3()) };
        let blue = if on { look::Hue::Focus.color(t) } else { look::Hue::Blue.color(t) };
        let mut base = look::ui(14.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, fg);
        base.wrap = Wrap::None;
        let title = cx.text_cache().layout_spans(t, &spans(&r.title, &r.title_ranges, blue), &base, None);
        // Alone, the title is centred; over a second line, at the top. The
        // number, the keys and the bell keep to its line.
        let top = if r.detail.is_empty() { ((ROW - title.size().height) / 2.0).round() } else { 7.0 };
        let line = |x0: f64, x1: f64| Rect::new(x0, top, x1, top + title.size().height);
        let number = look::mono(12.0, typeface::WEIGHT_MEDIUM, if r.current { t.focus() } else { dim });
        cx.draw_text_in(&r.number.to_string(), &number, line(14.0, 38.0), Align::End);
        if r.bell {
            let bell = look::mono(12.0, typeface::WEIGHT_BOLD, look::Hue::Amber.color(t));
            cx.draw_text_in("•", &bell, line(2.0, 12.0), Align::Center);
        }
        let x = 50.0;
        let right = b.x1 - 16.0;
        let key_spec = look::mono(12.0, typeface::WEIGHT_REGULAR, dim);
        let mut keys_w = r
            .shortcut
            .as_ref()
            .map_or(0.0, |shortcut| cx.draw_text_in(shortcut, &key_spec, line(right - 64.0, right), Align::End));
        if r.current {
            let mut here = look::mono(10.0, typeface::WEIGHT_MEDIUM, dim);
            here.letter_spacing = 1.2;
            let end = right - keys_w - if keys_w > 0.0 { 10.0 } else { 0.0 };
            keys_w += 10.0 + cx.draw_text_in("THIS TAB", &here, line(end - 80.0, end), Align::End);
        }
        let room = (right - keys_w - 10.0 - x).max(0.0);
        cx.canvas.save();
        cx.canvas.clip_rect(Rect::new(x, 0.0, x + room, ROW));
        cx.canvas.draw_text(&title, Point::new(x, top));
        cx.canvas.restore();
        if !r.detail.is_empty() {
            let mut small = look::mono(11.5, typeface::WEIGHT_REGULAR, dim);
            small.wrap = Wrap::None;
            let detail = cx.text_cache().layout_spans(t, &spans(&r.detail, &r.detail_ranges, blue), &small, None);
            cx.canvas.save();
            cx.canvas.clip_rect(Rect::new(x, 0.0, right, ROW));
            cx.canvas.draw_text(&detail, Point::new(x, 26.0));
            cx.canvas.restore();
        }
    })
    .on_click(Msg::GoToTab(Event::Click(index)))
    .text(label, SemanticRole::ListItem)
    .key(("go-to-tab-row", tab))
    .into()
}

fn results(p: &Palette) -> Element<Msg> {
    let room = body_height();
    let first = (p.scroll / ROW).floor() as usize;
    let mut rows: Vec<Element<Msg>> = Vec::new();
    for (index, r) in p.rows.iter().enumerate().skip(first) {
        if index as f64 * ROW >= p.scroll + room {
            break;
        }
        rows.push(result(r, index, index == p.selected));
    }
    if p.rows.is_empty() {
        let waiting = p.facts.is_none() && p.query.trim().chars().count() >= SCREEN_QUERY;
        let text = if waiting { "Looking at the screens…" } else { "No tab matches." };
        rows.push(
            drawn(0.0, 80.0, text, move |cx| {
                let t = cx.theme();
                let s = look::ui(13.0, typeface::WEIGHT_REGULAR, t.text3());
                cx.draw_text_in(text, &s, Rect::new(16.0, 0.0, cx.bounds().x1, 80.0), Align::Start);
            })
            .into(),
        );
    }
    // Only the rows that show are built; the first starts where the
    // scrolled-to content does.
    let offset = first as f64 * ROW - p.scroll;
    let list = container(column(rows)).padding([offset, 0.0, 0.0, 0.0]).clip().width(Fill).height(room);
    around(list, [0.0, 0.0, 0.0, 0.0], Size::new(0.0, room), "go-to-tab-results", |_| {})
        .on_scroll(|delta| Msg::GoToTab(Event::Scroll(delta.y)))
        .width(Length::FillPortion(std::num::NonZeroU16::new(3).unwrap()))
        .into()
}

/// The selected tab, and its pane's screen round what matched (or its foot).
fn detail(row: Option<&Row>, facts: Option<&Facts>, gathered: bool) -> Element<Msg> {
    let row = row.cloned();
    let lines = facts.map(|facts| facts.lines.clone()).unwrap_or_default();
    let about = about(facts);
    let hash = (format!("{row:?}"), lines.len(), lines.last().cloned(), gathered);
    let label = row.as_ref().map(|row| row.title.clone()).unwrap_or_default();
    drawn(0.0, body_height(), hash, move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        aizome_kit::draw::frost(cx.canvas, t, b, t.sunk());
        cx.canvas.fill_rect(Rect::new(0.0, 0.0, 1.0, b.y1), t.line());
        let Some(row) = &row else { return };
        let (x0, x1) = (18.0, b.x1 - 18.0);
        let mut y = 18.0;
        let mut caption = look::mono(10.5, typeface::WEIGHT_BOLD, t.text3());
        caption.letter_spacing = 10.5 * 0.14;
        let mut head = format!("TAB {}", row.number);
        if row.panes > 1 {
            head.push_str(&format!(" · {} PANES", row.panes));
        }
        cx.draw_text_in(&head, &caption, Rect::new(x0, y, x1, y + 14.0), Align::Start);
        y += 14.0 + 10.0;
        let mut title = look::ui(16.0 * t.metrics.factor, typeface::WEIGHT_SEMIBOLD, t.text());
        title.wrap = Wrap::Words;
        title.max_lines = 2;
        let laid = cx.layout_text(&row.title, &title, Some(x1 - x0));
        cx.canvas.draw_text(&laid, Point::new(x0, y));
        y += laid.size().height + 6.0;
        if !about.is_empty() {
            let s = look::mono(11.5, typeface::WEIGHT_REGULAR, t.text3());
            cx.draw_text_in(&about, &s, Rect::new(x0, y, x1, y + 16.0), Align::Start);
            y += 16.0;
        }
        y += 12.0;
        cx.canvas.fill_rect(Rect::new(x0, y - 6.0, x1, y - 5.0), t.line());
        if lines.is_empty() {
            let s = look::ui(12.5, typeface::WEIGHT_REGULAR, t.text3());
            let text = if gathered { "Nothing on its screen." } else { "Looking at its screen…" };
            cx.draw_text_in(text, &s, Rect::new(x0, y, x1, y + 18.0), Align::Start);
            return;
        }
        // The screen, round the matched line or at its foot.
        let mut s = look::mono(11.0, typeface::WEIGHT_REGULAR, t.text2());
        s.wrap = Wrap::None;
        let height = 15.0;
        let fit = (((b.y1 - 12.0 - y) / height).floor() as usize).max(1);
        let end = match row.line {
            Some(line) => (line + fit / 2 + 1).clamp(fit.min(lines.len()), lines.len()),
            None => lines.len(),
        };
        let start = end.saturating_sub(fit);
        cx.canvas.save();
        cx.canvas.clip_rect(Rect::new(x0 - 6.0, y, x1 + 6.0, b.y1 - 8.0));
        for (index, line) in lines.iter().enumerate().take(end).skip(start) {
            if row.line == Some(index) {
                cx.canvas.fill_rect(Rect::new(x0 - 6.0, y, x1 + 6.0, y + height), t.selection());
            }
            let laid = cx.layout_text(line, &s, None);
            cx.canvas.draw_text(&laid, Point::new(x0, y));
            y += height;
        }
        cx.canvas.restore();
    })
    .text(label, SemanticRole::Group)
    .width(Length::FillPortion(std::num::NonZeroU16::new(2).unwrap()))
    .into()
}

fn footer() -> Element<Msg> {
    drawn(0.0, FOOTER, "go-to-tab-footer", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(Rect::new(0.0, 0.0, b.x1, 1.0), t.line());
        let mut x = 18.0;
        for (key, label) in [("↑↓", "move"), ("↩", "go"), ("esc", "close")] {
            let ks = look::mono(12.0, typeface::WEIGHT_MEDIUM, t.text2());
            let ls = look::ui(12.0, typeface::WEIGHT_REGULAR, t.text3());
            x += cx.draw_text_in(key, &ks, Rect::new(x, 1.0, b.x1, b.y1), Align::Start) + 6.0;
            x += cx.draw_text_in(label, &ls, Rect::new(x, 1.0, b.x1, b.y1), Align::Start) + 18.0;
        }
        let ls = look::ui(12.0, typeface::WEIGHT_REGULAR, t.text3());
        cx.draw_text_in(
            "Titles, programs, folders and what's on screen",
            &ls,
            Rect::new(x, 1.0, b.x1 - 18.0, b.y1),
            Align::End,
        );
    })
    .width(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(tab: u64, title: &str, recency: usize) -> Candidate {
        Candidate {
            tab,
            pane: tab * 10,
            number: tab as usize,
            panes: 1,
            title: title.into(),
            front: true,
            bell: false,
            current: recency == 0,
            shortcut: None,
            recency,
        }
    }

    fn facts(pane: u64, program: &str, folder: &str, lines: &[&str]) -> Facts {
        Facts {
            pane,
            program: Some(program.into()),
            folder: Some(folder.into()),
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
        }
    }

    #[test]
    fn the_terminals_foreground_group_comes_after_the_command_name() {
        let stat = "4242 (odd) name (x) S 4000 4242 4242 34817 5151 4194304 1 0 0 0";
        assert_eq!(tpgid(stat), Some(5151));
        assert_eq!(tpgid("1 (init) S 0 1 1 0 -1 4194560"), None);
        assert_eq!(program_name(b"-fish\0"), Some("fish".into()));
        assert_eq!(program_name(b"/nix/store/abc-claude/bin/claude\0--resume\0"), Some("claude".into()));
        assert_eq!(program_name(b""), None);
        let home = Path::new("/home/someone");
        assert_eq!(tilde(Path::new("/home/someone/work/sekka"), Some(home)), "~/work/sekka");
        assert_eq!(tilde(home, Some(home)), "~");
        assert_eq!(tilde(Path::new("/etc"), Some(home)), "/etc");
    }

    #[test]
    fn without_a_query_tabs_come_as_last_shown_and_the_one_before_is_selected() {
        let candidates = [candidate(1, "one", 2), candidate(2, "two", 0), candidate(3, "three", 1)];
        let mut palette = Palette::new(1, 3);
        palette.refresh(&candidates, true);
        let order: Vec<u64> = palette.rows.iter().map(|row| row.tab).collect();
        assert_eq!(order, [2, 3, 1]);
        assert_eq!(palette.selected, 1, "↩ goes back to the tab shown before");
    }

    #[test]
    fn titles_programs_and_folders_match_before_screens() {
        let candidates = [
            candidate(1, "Review PR #221 | sekka", 1),
            candidate(2, "Check Uta audio quality | uta", 2),
            candidate(3, "Shell", 3),
        ];
        let all = [
            facts(10, "claude", "~/work/sekka", &["nothing here"]),
            facts(20, "claude", "~/work/uta", &["the nightly build failed"]),
            facts(30, "fish", "~/work/uta", &["$ cargo test", "test review_queue ... ok"]),
        ];
        let rows = rank("uta", &candidates, Some(&all));
        let mut order: Vec<u64> = rows.iter().map(|row| row.tab).collect();
        order.sort_unstable();
        assert_eq!(order, [2, 3], "a title, and a folder");
        let rows = rank("audio", &candidates, Some(&all));
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0].title[rows[0].title_ranges[0].clone()], "audio");
        // A title before a screen, whatever was shown last.
        let rows = rank("review", &candidates, Some(&all));
        assert_eq!(rows.iter().map(|row| row.tab).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(rows[1].line, Some(1));
        // A phrase on a screen, and only there: the line shows, matched.
        let rows = rank("nightly build", &candidates, Some(&all));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tab, 2);
        assert_eq!(rows[0].line, Some(0));
        assert_eq!(&rows[0].detail[rows[0].detail_ranges[0].clone()], "nightly build");
        // Programs match by name.
        let rows = rank("fish", &candidates, Some(&all));
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].tab, rows[0].detail.as_str()), (3, "fish · ~/work/uta"));
        assert_eq!(&rows[0].detail[rows[0].detail_ranges[0].clone()], "fish");
        // Before the thread answers, titles still match.
        assert_eq!(rank("review", &candidates, None).first().map(|row| row.tab), Some(1));
        assert!(rank("nightly build", &candidates, None).is_empty());
        // One letter is too little to look at screens with.
        assert!(rank("q", &[candidate(3, "Shell", 0)], Some(&all)).is_empty());
    }

    #[test]
    fn the_tab_shown_comes_after_others_matching_as_well() {
        let candidates = [candidate(1, "Review PR #213", 0), candidate(2, "Review PR #221", 1)];
        let order = |text| rank(text, &candidates, None).iter().map(|row| row.tab).collect::<Vec<_>>();
        assert_eq!(order("review"), [2, 1]);
        assert_eq!(order("213"), [1]);
        assert_eq!(order(""), [1, 2]);
    }

    #[test]
    fn a_tab_goes_to_the_pane_whose_screen_matched() {
        let mut front = candidate(1, "Shell", 0);
        front.panes = 2;
        let mut beside = front.clone();
        beside.pane = 11;
        beside.front = false;
        let all = [facts(10, "fish", "~", &["ls"]), facts(11, "htop", "~", &["Load average: 1.0", "Tasks: 300"])];
        let rows = rank("tasks", &[front.clone(), beside.clone()], Some(&all));
        assert_eq!(rows.len(), 1, "one row per tab");
        assert_eq!((rows[0].pane, rows[0].line), (11, Some(1)));
        // Without a query, the pane in front.
        assert_eq!(rank("", &[front, beside], Some(&all))[0].pane, 10);
    }

    #[test]
    fn long_lines_show_round_what_matched() {
        let line = format!("    {}needle and more", "x".repeat(60));
        let (shown, range) = snippet(&line, line.find("needle").unwrap(), 6);
        assert!(shown.starts_with('…'));
        assert_eq!(&shown[range], "needle");
        let (shown, range) = snippet("    short needle", 10, 6);
        assert_eq!(shown, "short needle");
        assert_eq!(&shown[range], "needle");
    }

    #[test]
    fn a_pane_without_a_live_process_keeps_its_last_known_folder() {
        let snapshot = Arc::new(hotaru_core::Terminal::new(80, 24, 0).snapshot());
        let source = |reported| Source {
            pane: 1,
            pid: None,
            reported,
            fallback: Some("/startup".into()),
            snapshot: snapshot.clone(),
        };
        assert_eq!(gather(vec![source(None)], None)[0].folder.as_deref(), Some("/startup"));
        assert_eq!(gather(vec![source(Some("/reported".into()))], None)[0].folder.as_deref(), Some("/reported"));
    }
}
