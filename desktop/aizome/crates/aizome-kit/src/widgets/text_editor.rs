//! `text_editor`: many lines of text to edit, for Sumi and Fusen.
//!
//! The program owns an [`EditorContent`] and applies the
//! [`EditorAction`]s the editor sends (`content.perform(action)`). The
//! editor lays out only the lines that show, each through the text cache
//! (a line that didn't change isn't shaped again), and redraws only the
//! lines that changed: typing a character costs its line.
//!
//! Keys are macOS's with ❖ as command: ←/→, ⌥←/→ by word, ❖←/→ to the
//! line's ends, ↑/↓ by visual line, ❖↑/↓ to the document's ends, ⇧ to
//! extend; ⌫ and ⌦ with ⌥ by word and ❖ by line; Tab and ⇧Tab indent;
//! Return keeps the indent. Double and triple clicks select words and
//! lines, and dragging extends by them.

use std::marker::PhantomData;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ropey::Rope;
use sidestep_ui::{Color, Cursor, Ime, Key, Modifiers, NamedKey, PointerButton, TextLayout, Wrap};

use crate::editing::{next_word, prev_word, word_at};
use crate::editor::{EditorAction, EditorContent, EditorSnapshot, Granularity, Highlighter, Selection};
use crate::element::{AnyMsg, Common, Element, MsgMap, RawElement};
use crate::hash::{Fx, PaintHasher};
use crate::layout::{Constraints, Length, Point, Rect, Size, Vec2};
use crate::text::{Span, TextCache, TextSpec};
use crate::theme::{FontSpec, Theme};
use crate::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics,
    StateView, Status, Widget,
};
use crate::widgets::text_field::BLINK;

/// How long the position bar stays after scrolling stops.
const BAR_LINGER: Duration = Duration::from_millis(900);

/// What the Find items ask of the program (the editor has no find bar
/// of its own: the program draws one and searches its content).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindRequest {
    /// ❖F: show the find bar.
    Show,
    Next,
    Previous,
    /// ❖E: search for this.
    UseSelection(String),
    /// ⌥❖F: show find and replace.
    Replace,
}

/// What a click on a character asks of the program, if anything.
type ClickFn = std::rc::Rc<dyn Fn(usize, Modifiers) -> Option<AnyMsg>>;
type KeyFn = std::rc::Rc<dyn Fn(&sidestep_ui::KeyEvent) -> Option<AnyMsg>>;
type PasteFn = std::rc::Rc<dyn Fn(&[String]) -> Option<AnyMsg>>;

/// Many lines of text: `text_editor(&self.content).on_action(Msg::Edit).line_numbers(true)`.
pub struct TextEditor<Msg> {
    pub(crate) common: Common,
    snapshot: EditorSnapshot,
    on_action: Option<MsgMap<EditorAction>>,
    on_find: Option<MsgMap<FindRequest>>,
    on_click: Option<ClickFn>,
    on_key: Option<KeyFn>,
    on_paste: Option<PasteFn>,
    opts: Options,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(TextEditor);

#[derive(Clone)]
struct Options {
    line_numbers: bool,
    wrap: bool,
    mono: bool,
    size: Option<f64>,
    line_height: f64,
    current_line: bool,
    highlighter: Option<Highlighter>,
    /// Ranges to mark (found text), in characters, and the current one.
    marks: Arc<Vec<Range<usize>>>,
    current_mark: Option<usize>,
    read_only: bool,
    placeholder: Option<Arc<str>>,
    /// Bumped by the program when its highlighter's output changes.
    styles_version: u64,
    padding: f64,
    /// As tall as its text, between this many lines and that many.
    fit: Option<(usize, usize)>,
    /// The text's colour, and the caret's (an editor on a coloured ground).
    text_color: Option<Color>,
    caret_color: Option<Color>,
}

pub fn text_editor<Msg>(content: &EditorContent) -> TextEditor<Msg> {
    TextEditor {
        common: Common::sized(Length::Fill, Length::Fill),
        snapshot: content.snapshot(),
        on_action: None,
        on_find: None,
        on_click: None,
        on_key: None,
        on_paste: None,
        opts: Options {
            line_numbers: false,
            wrap: true,
            mono: true,
            size: None,
            line_height: 21.0 / 13.0,
            current_line: true,
            highlighter: None,
            marks: Arc::new(Vec::new()),
            current_mark: None,
            read_only: false,
            placeholder: None,
            styles_version: 0,
            padding: 12.0,
            fit: None,
            text_color: None,
            caret_color: None,
        },
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> TextEditor<Msg> {
    /// Sent with every edit and every move of the caret: apply it with
    /// `content.perform(action)`.
    pub fn on_action(mut self, f: impl Fn(EditorAction) -> Msg + 'static) -> Self {
        self.on_action = Some(MsgMap::new(f));
        self
    }

    /// Sent by the Edit menu's Find items.
    pub fn on_find(mut self, f: impl Fn(FindRequest) -> Msg + 'static) -> Self {
        self.on_find = Some(MsgMap::new(f));
        self
    }

    /// A click on a character the program cares about (a task's box, a
    /// link with ❖): `f(character, modifiers)` gives a message to send
    /// instead of placing the caret, or `None` to let it place the caret.
    pub fn on_click(mut self, f: impl Fn(usize, Modifiers) -> Option<Msg> + 'static) -> Self {
        self.on_click = Some(std::rc::Rc::new(move |at, mods| f(at, mods).map(|m| Box::new(m) as AnyMsg)));
        self
    }

    /// A key the program takes before the editor does (Return sending a
    /// message, ↑ at the top recalling an earlier one): `f(key)` gives a
    /// message to send instead, or `None` to let the editor have it.
    pub fn on_key(mut self, f: impl Fn(&sidestep_ui::KeyEvent) -> Option<Msg> + 'static) -> Self {
        self.on_key = Some(std::rc::Rc::new(move |k| f(k).map(|m| Box::new(m) as AnyMsg)));
        self
    }

    /// A paste the program takes instead of the editor (a picture or files
    /// on the clipboard): `f(types)` gets the clipboard's MIME types and
    /// gives a message to send instead of pasting text, or `None`. The
    /// program reads the data with `Cx::read_clipboard`.
    pub fn on_paste(mut self, f: impl Fn(&[String]) -> Option<Msg> + 'static) -> Self {
        self.on_paste = Some(std::rc::Rc::new(move |types| f(types).map(|m| Box::new(m) as AnyMsg)));
        self
    }
}

impl<Msg> TextEditor<Msg> {
    pub fn line_numbers(mut self, on: bool) -> Self {
        self.opts.line_numbers = on;
        self
    }

    /// Break long lines at the edge (the default), or scroll sideways.
    pub fn wrap(mut self, on: bool) -> Self {
        self.opts.wrap = on;
        self
    }

    /// IBM Plex Mono (the default), or Archivo.
    pub fn mono(mut self, on: bool) -> Self {
        self.opts.mono = on;
        self
    }

    pub fn size(mut self, points: f64) -> Self {
        self.opts.size = Some(points);
        self
    }

    /// Lines this many times the text's size apart (21/13 by default, as
    /// the board's editor).
    pub fn line_height(mut self, multiple: f64) -> Self {
        self.opts.line_height = multiple;
        self
    }

    /// Mark the caret's line (on by default).
    pub fn current_line(mut self, on: bool) -> Self {
        self.opts.current_line = on;
        self
    }

    /// Style lines (syntax, Markdown); bump `version` when what it gives
    /// changes without the text changing.
    pub fn highlighter(mut self, h: Highlighter, version: u64) -> Self {
        self.opts.highlighter = Some(h);
        self.opts.styles_version = version;
        self
    }

    /// Mark these character ranges (found text), `current` the one to
    /// show strongest.
    pub fn marks(mut self, ranges: Arc<Vec<Range<usize>>>, current: Option<usize>) -> Self {
        self.opts.marks = ranges;
        self.opts.current_mark = current;
        self
    }

    pub fn read_only(mut self) -> Self {
        self.opts.read_only = true;
        self
    }

    pub fn placeholder(mut self, text: impl Into<Arc<str>>) -> Self {
        self.opts.placeholder = Some(text.into());
        self
    }

    /// Room around the text.
    pub fn padding(mut self, points: f64) -> Self {
        self.opts.padding = points;
        self
    }

    /// The text's colour (else `text2`), for an editor on a ground of its
    /// own colour (a sticky note).
    pub fn text_color(mut self, c: Color) -> Self {
        self.opts.text_color = Some(c);
        self
    }

    /// The caret's colour (else `focus`).
    pub fn caret_color(mut self, c: Color) -> Self {
        self.opts.caret_color = Some(c);
        self
    }

    /// Be as tall as the text (a composer), at least `min` lines and at
    /// most `max`, scrolling past that; give it `.height(Shrink)`.
    pub fn fit_lines(mut self, min: usize, max: usize) -> Self {
        self.opts.fit = Some((min.max(1), max.max(min.max(1))));
        self
    }
}

impl<Msg> From<TextEditor<Msg>> for Element<Msg> {
    fn from(e: TextEditor<Msg>) -> Element<Msg> {
        let w = EditorWidget {
            snap: e.snapshot,
            on_action: e.on_action,
            on_find: e.on_find,
            on_click: e.on_click,
            on_key: e.on_key,
            on_paste: e.on_paste,
            opts: e.opts,
            lines: Vec::new(),
            gutter: 0.0,
            text_x: 0.0,
            line_h: 0.0,
            caret: None,
            size: Size::ZERO,
            top: (0, 0.0),
            scroll_x: 0.0,
        };
        Element::from_raw(RawElement::new(w, e.common, Vec::new()))
    }
}

/// A line as laid out.
#[derive(Clone)]
/// A line's text in runs, ready to lay out ([`EditorWidget::parts`]).
struct LineParts {
    start: usize,
    text: String,
    preedit: Option<Range<usize>>,
    selection: Option<Range<usize>>,
    selects_break: bool,
    base: TextSpec,
    spans: Vec<Span>,
    key: u64,
}

struct Laid {
    line: usize,
    /// Its first character.
    start: usize,
    y: f64,
    h: f64,
    layout: TextLayout,
    /// What it shows (the preedit inserted), its byte to char map's
    /// shift, and the selection in it (bytes of what it shows).
    text: Arc<str>,
    preedit: Option<Range<usize>>,
    selection: Option<Range<usize>>,
    /// The selection runs on past the line's end.
    selects_break: bool,
    key: u64,
}

/// How far a press on the selection moves before it drags the text out.
const DRAG_SLOP: f64 = 4.0;

/// What an editor remembers between views.
#[derive(Default)]
pub(crate) struct EditorState {
    top_line: usize,
    top_offset: f64,
    scroll_x: f64,
    goal_x: Option<f64>,
    preedit: String,
    preedit_caret: Option<usize>,
    blink_off: bool,
    focused: bool,
    /// Dragging out a selection by character (1), word (2) or line (3),
    /// from this range.
    drag: Option<(u32, Range<usize>)>,
    /// A press inside the selection, where and at which character: moved
    /// far enough, the selected text is dragged out; released, the caret
    /// goes there.
    drag_text: Option<(Point, usize)>,
    /// Show the caret at the next layout (it moved by the keyboard).
    reveal: bool,
    /// The lines drawn last: line, y, height, key.
    drawn: Vec<(usize, f64, f64, u64)>,
    /// How far sideways the lines drawn last were scrolled.
    drawn_x: f64,
    caret: Option<Rect>,
    width: f64,
    /// The selection the last layout showed.
    shown_selection: Option<Selection>,
    /// The position bar shows until then (it scrolled).
    bar_until: Option<Instant>,
    /// When the caret blinks next.
    next_blink: Option<Instant>,
}

struct EditorWidget {
    snap: EditorSnapshot,
    on_action: Option<MsgMap<EditorAction>>,
    on_find: Option<MsgMap<FindRequest>>,
    on_click: Option<ClickFn>,
    on_key: Option<KeyFn>,
    on_paste: Option<PasteFn>,
    opts: Options,
    lines: Vec<Laid>,
    gutter: f64,
    text_x: f64,
    line_h: f64,
    caret: Option<Rect>,
    size: Size,
    top: (usize, f64),
    scroll_x: f64,
}

/// The text of `line`, without its line break.
fn line_text(rope: &Rope, line: usize) -> String {
    let s = rope.line(line);
    let mut t = s.to_string();
    while t.ends_with('\n') || t.ends_with('\r') {
        t.pop();
    }
    t
}

/// The byte offset of character `c` of `text`.
fn byte_of(text: &str, c: usize) -> usize {
    text.char_indices().nth(c).map_or(text.len(), |(i, _)| i)
}

impl EditorWidget {
    fn font(&self, t: &Theme) -> FontSpec {
        let size = self.opts.size.unwrap_or(t.metrics.body);
        if self.opts.mono { FontSpec::mono(size) } else { FontSpec::ui(size) }
    }

    fn base_spec(&self, t: &Theme) -> TextSpec {
        // Lines `line_height` times the type's size apart, as CSS has it:
        // the layout's multiple is of the font's own line height.
        let spec = self.font(t);
        let m = t.font(spec).metrics();
        let natural = (m.ascent + m.descent + m.leading).max(1.0);
        TextSpec {
            wrap: if self.opts.wrap { Wrap::Words } else { Wrap::None },
            line_height: (spec.size * self.opts.line_height / natural).max(0.5),
            ..TextSpec::new(spec, self.opts.text_color.unwrap_or(t.text2()))
        }
    }

    fn editable(&self) -> bool {
        !self.opts.read_only && self.on_action.is_some()
    }

    fn line_count(&self) -> usize {
        self.snap.rope.len_lines()
    }

    /// Lay `line` out at `width`, with the selection, what's composed and
    /// the program's styles.
    fn lay(
        &self,
        t: &Theme,
        cache: &mut TextCache,
        line: usize,
        width: Option<f64>,
        preedit: &str,
        focused: bool,
    ) -> Laid {
        let p = self.parts(t, line, preedit, focused);
        let layout = if p.spans.is_empty() {
            cache.layout(t, "", &p.base, width)
        } else {
            cache.layout_spans(t, &p.spans, &p.base, width)
        };
        let lh = (self.font(t).size * self.opts.line_height).round();
        let h = layout.size().height.max(lh).round();
        Laid {
            line,
            start: p.start,
            y: 0.0,
            h,
            layout,
            text: p.text.into(),
            preedit: p.preedit,
            selection: p.selection,
            selects_break: p.selects_break,
            key: p.key,
        }
    }

    /// Lay out `lines` together, ahead of their being laid out one by one
    /// (see [`TextCache::prefetch_spans`]): the lines a jump shows, none
    /// of which has shown before.
    #[allow(clippy::too_many_arguments)]
    fn prefetch(
        &self,
        t: &Theme,
        cache: &mut TextCache,
        lines: Range<usize>,
        width: Option<f64>,
        focused: bool,
        caret_line: usize,
        preedit: &str,
    ) {
        let texts: Vec<(Vec<Span>, TextSpec, Option<f64>)> = lines
            .map(|l| self.parts(t, l, if l == caret_line { preedit } else { "" }, focused))
            .filter(|p| !p.spans.is_empty())
            .map(|p| (p.spans, p.base, width))
            .collect();
        cache.prefetch_spans(t, &texts);
    }

    /// What line `line` shows (with the selection, what's composed and the
    /// program's styles), in runs, ready to lay out.
    fn parts(&self, t: &Theme, line: usize, preedit: &str, focused: bool) -> LineParts {
        let rope = &self.snap.rope;
        let start = rope.line_to_char(line);
        let raw = line_text(rope, line);
        let len_chars = raw.chars().count();
        let sel = self.snap.selection.range();
        let head = self.snap.selection.head;
        // What's composed, at the caret in its line.
        let mut text = raw.clone();
        let mut pre = None;
        let caret_here = head >= start && head <= start + len_chars;
        if !preedit.is_empty() && caret_here {
            let b = byte_of(&raw, head - start);
            text.insert_str(b, preedit);
            pre = Some(b..b + preedit.len());
        }
        let shift = |b: usize| match &pre {
            Some(p) if b >= p.start => b + p.len(),
            _ => b,
        };
        let selection = if sel.is_empty() || sel.end < start || sel.start > start + len_chars {
            None
        } else {
            let a = byte_of(&raw, sel.start.saturating_sub(start).min(len_chars));
            let b = byte_of(&raw, (sel.end.min(start + len_chars)).saturating_sub(start));
            Some(shift(a)..shift(b))
        };
        let selects_break = !sel.is_empty() && sel.start <= start + len_chars && sel.end > start + len_chars;
        // Runs: the program's styles, marks, the selection's colour, the
        // composition's underline.
        let base = self.base_spec(t);
        let mut cuts: Vec<usize> = vec![0, text.len()];
        let mut styled: Vec<(Range<usize>, Span)> = Vec::new();
        if let Some(h) = &self.opts.highlighter {
            for s in h(line, &raw) {
                let r = shift(s.range.start.min(raw.len()))..shift(s.range.end.min(raw.len()));
                styled.push((r, s.span));
            }
        }
        if let Some(r) = &selection
            && focused
        {
            styled.push((r.clone(), Span::new("").color(t.on_selection())));
        }
        if let Some(r) = &pre {
            styled.push((r.clone(), Span::new("").underline()));
        }
        for (r, _) in &styled {
            cuts.push(r.start.min(text.len()));
            cuts.push(r.end.min(text.len()));
        }
        cuts.retain(|c| text.is_char_boundary(*c));
        cuts.sort_unstable();
        cuts.dedup();
        let mut spans = Vec::new();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a == b {
                continue;
            }
            let mut span = Span::new(&text[a..b]);
            // Later styles over earlier ones, field by field.
            for (r, s) in &styled {
                if r.start <= a && b <= r.end {
                    if s.role.is_some() {
                        span.role = s.role;
                    }
                    if s.color.is_some() {
                        span.color = s.color;
                    }
                    if s.weight.is_some() {
                        span.weight = s.weight;
                    }
                    span.italic |= s.italic;
                    span.mono |= s.mono;
                    if s.size.is_some() {
                        span.size = s.size;
                    }
                    span.underline |= s.underline;
                    span.strikethrough |= s.strikethrough;
                    if s.background.is_some() {
                        span.background = s.background;
                    }
                    if s.alpha.is_some() {
                        span.alpha = s.alpha;
                    }
                }
            }
            spans.push(span);
        }
        let mut fx = Fx::default();
        use std::hash::{Hash, Hasher};
        text.hash(&mut fx);
        (selection.clone(), pre.clone(), selects_break, focused).hash(&mut fx);
        // Search marks are painted behind the glyphs, independently of
        // the text and selection. Changing a query (or its current match)
        // must redraw the affected lines even when their text stays put.
        for (i, mark) in self.opts.marks.iter().enumerate() {
            if mark.end < start || mark.start > start + len_chars {
                continue;
            }
            (
                mark.start.saturating_sub(start).min(len_chars),
                mark.end.saturating_sub(start).min(len_chars),
                Some(i) == self.opts.current_mark,
            )
                .hash(&mut fx);
        }
        for (r, s) in &styled {
            (
                r.start,
                r.end,
                s.role,
                s.weight.map(f32::to_bits),
                s.italic,
                s.underline,
                s.strikethrough,
                s.alpha.map(f32::to_bits),
                s.mono,
                s.size.map(f64::to_bits),
            )
                .hash(&mut fx);
        }
        let key = fx.finish();
        LineParts { start, text, preedit: pre, selection, selects_break, base, spans, key }
    }

    fn wrap_width(&self, t: &Theme, width: f64) -> Option<f64> {
        let _ = t;
        self.opts.wrap.then(|| (width - self.text_x - self.opts.padding).max(40.0))
    }

    /// The character at `p` (the editor's coordinates).
    fn hit(&self, p: Point) -> usize {
        let n = self.snap.rope.len_chars();
        let Some(first) = self.lines.first() else { return n };
        if p.y < first.y {
            return if first.line == 0 { 0 } else { first.start };
        }
        let laid = self.lines.iter().find(|l| p.y < l.y + l.h).or(self.lines.last());
        let Some(l) = laid else { return n };
        let hit = l.layout.hit_test(Point::new(p.x - self.text_x + self.scroll_x, (p.y - l.y).min(l.h - 1.0)));
        let mut b = hit.offset.min(l.text.len());
        if let Some(pre) = &l.preedit {
            if b >= pre.end {
                b -= pre.len();
            } else if b > pre.start {
                b = pre.start;
            }
        }
        let raw_text: String = l.text.chars().collect();
        let raw = match &l.preedit {
            Some(pre) => format!("{}{}", &raw_text[..pre.start], &raw_text[pre.end..]),
            None => raw_text,
        };
        let b = b.min(raw.len());
        (l.start + raw[..b].chars().count()).min(n)
    }

    /// The caret's rectangle for character `at`, if its line is laid out.
    fn caret_rect(&self, at: usize, preedit_caret: Option<usize>) -> Option<Rect> {
        let l = self.lines.iter().find(|l| {
            let len = line_text(&self.snap.rope, l.line).chars().count();
            at >= l.start && at <= l.start + len
        })?;
        let raw = line_text(&self.snap.rope, l.line);
        let mut b = byte_of(&raw, at - l.start);
        if let Some(pre) = &l.preedit {
            b = pre.start + preedit_caret.unwrap_or(pre.len()).min(pre.len());
        }
        let c = l.layout.caret(b, false);
        let x = (self.text_x + c.x - self.scroll_x).round();
        // As tall as the type, centred in its row (the board's caret).
        let h = (self.line_h * 0.76).round().min(c.height.max(1.0));
        let y = l.y + c.top + ((c.height - h) / 2.0).max(0.0);
        Some(Rect::new(x, y.round(), x + 2.0, (y + h).round()))
    }

    fn emit(&self, cx: &mut EventCx, a: EditorAction) {
        if let Some(f) = &self.on_action {
            cx.state::<EditorState>().reveal = true;
            cx.state::<EditorState>().blink_off = false;
            cx.emit_any(f.make(a));
        }
    }

    fn edit(&self, cx: &mut EventCx, a: EditorAction) {
        if self.editable() {
            cx.state::<EditorState>().goal_x = None;
            self.emit(cx, a);
        }
    }

    fn select(&self, cx: &mut EventCx, anchor: usize, head: usize) {
        self.emit(cx, EditorAction::Select(Selection { anchor, head }));
    }

    /// Whether `p` falls on the selection's text: between its two ends,
    /// and not on the near side of either end's caret.
    fn on_selection(&self, p: Point) -> bool {
        let r = self.snap.selection.range();
        if r.is_empty() {
            return false;
        }
        let at = self.hit(p);
        if at < r.start || at > r.end {
            return false;
        }
        let before_start = at == r.start && self.caret_rect(r.start, None).is_some_and(|c| p.x < c.x0);
        let after_end = at == r.end && self.caret_rect(r.end, None).is_some_and(|c| p.x > c.x0);
        !before_start && !after_end
    }

    /// Move the caret to `to`, extending the selection with `extend`.
    fn move_to(&self, cx: &mut EventCx, to: usize, extend: bool, keep_goal: bool) {
        if !keep_goal {
            cx.state::<EditorState>().goal_x = None;
        }
        let anchor = if extend { self.snap.selection.anchor } else { to };
        self.select(cx, anchor, to);
    }

    /// The character a line up (or down) from the caret, keeping its x.
    fn vertical(&self, cx: &mut EventCx, down: bool, lines: usize) -> usize {
        let head = self.snap.selection.head;
        let t = cx.theme().clone();
        let focused = cx.state::<EditorState>().focused;
        let width = self.wrap_width(&t, self.size.width);
        let rope = &self.snap.rope;
        let mut line = rope.char_to_line(head.min(rope.len_chars()));
        let laid = self.lay(&t, cx.text_cache(), line, width, "", focused);
        let raw = line_text(rope, line);
        let caret = laid.layout.caret(byte_of(&raw, head - laid.start), false);
        let goal = *cx.state::<EditorState>().goal_x.get_or_insert(caret.x);
        let mut y = caret.top + caret.height / 2.0;
        let mut layout = laid;
        for _ in 0..lines.max(1) {
            let rows = layout.layout.lines();
            let row = rows.iter().position(|r| y >= r.top && y < r.top + r.height).unwrap_or(0);
            if down {
                if row + 1 < rows.len() {
                    y = rows[row + 1].top + rows[row + 1].height / 2.0;
                } else if line + 1 < rope.len_lines() {
                    line += 1;
                    layout = self.lay(&t, cx.text_cache(), line, width, "", focused);
                    y = 1.0;
                } else {
                    return rope.len_chars();
                }
            } else if row > 0 {
                y = rows[row - 1].top + rows[row - 1].height / 2.0;
            } else if line > 0 {
                line -= 1;
                layout = self.lay(&t, cx.text_cache(), line, width, "", focused);
                let rows = layout.layout.lines();
                y = rows.last().map_or(1.0, |r| r.top + r.height / 2.0);
            } else {
                return 0;
            }
        }
        let hit = layout.layout.hit_test(Point::new(goal, y));
        let raw = line_text(rope, line);
        layout.start + raw[..hit.offset.min(raw.len())].chars().count()
    }

    fn line_bounds(&self, at: usize) -> (usize, usize) {
        let rope = &self.snap.rope;
        let line = rope.char_to_line(at.min(rope.len_chars()));
        let start = rope.line_to_char(line);
        (start, start + line_text(rope, line).chars().count())
    }

    fn word_bounds(&self, at: usize, forward: bool) -> usize {
        let (start, end) = self.line_bounds(at);
        let raw: String = self.snap.rope.slice(start..end).to_string();
        let b = byte_of(&raw, at - start);
        if forward {
            if at >= end {
                return (at + 1).min(self.snap.rope.len_chars());
            }
            start + raw[..next_word(&raw, b)].chars().count()
        } else {
            if at <= start {
                return at.saturating_sub(1);
            }
            start + raw[..prev_word(&raw, b)].chars().count()
        }
    }

    fn copy(&self, cx: &mut EventCx) -> bool {
        let r = self.snap.selection.range();
        if r.is_empty() {
            return false;
        }
        let text = self.snap.rope.slice(r).to_string();
        cx.set_clipboard_text(&text);
        true
    }

    /// Where the position bar shows, along the right edge.
    fn bar_strip(&self) -> Rect {
        Rect::new(self.size.width - 8.0, 0.0, self.size.width, self.size.height)
    }

    fn scroll_by(&self, cx: &mut EventCx, delta: Vec2) -> bool {
        let t = cx.theme().clone();
        let width = self.wrap_width(&t, self.size.width);
        let focused = cx.state::<EditorState>().focused;
        let lines = self.line_count();
        let (mut top, mut off) = (
            cx.state::<EditorState>().top_line.min(lines.saturating_sub(1)),
            cx.state::<EditorState>().top_offset + delta.y,
        );
        while off < 0.0 && top > 0 {
            top -= 1;
            off += self.lay(&t, cx.text_cache(), top, width, "", focused).h;
        }
        loop {
            if off <= 0.0 {
                break;
            }
            let h = self.lay(&t, cx.text_cache(), top, width, "", focused).h;
            if off < h || top + 1 >= lines {
                off = off.min(h - self.line_h).max(0.0);
                break;
            }
            off -= h;
            top += 1;
        }
        let off = off.max(0.0);
        let mut x = cx.state::<EditorState>().scroll_x;
        if !self.opts.wrap {
            x = (x + delta.x).max(0.0);
        }
        let now = cx.now();
        let st = cx.state::<EditorState>();
        let moved = st.top_line != top || (st.top_offset - off).abs() > 0.01 || (st.scroll_x - x).abs() > 0.01;
        st.top_line = top;
        st.top_offset = off;
        st.scroll_x = x;
        if moved {
            st.bar_until = Some(now + BAR_LINGER);
            cx.request_tick(BAR_LINGER + Duration::from_millis(20));
            // The lines move as pixels (see layout); the bar is drawn again.
            cx.relayout();
            cx.invalidate_rect(self.bar_strip());
        }
        moved
    }

    fn key(&mut self, cx: &mut EventCx, k: &sidestep_ui::KeyEvent) -> Status {
        if let Some(f) = &self.on_key
            && let Some(msg) = f(k)
        {
            cx.emit_any(msg);
            return Status::Handled;
        }
        let m = k.modifiers;
        let shift = m.shift();
        let n = self.snap.rope.len_chars();
        let sel = self.snap.selection;
        let head = sel.head.min(n);
        match &k.key {
            Key::Named(NamedKey::ArrowLeft) => {
                let to = if m.super_key() {
                    self.line_bounds(head).0
                } else if m.alt() {
                    self.word_bounds(head, false)
                } else if !shift && !sel.is_empty() {
                    sel.range().start
                } else {
                    head.saturating_sub(1)
                };
                self.move_to(cx, to, shift, false);
            }
            Key::Named(NamedKey::ArrowRight) => {
                let to = if m.super_key() {
                    self.line_bounds(head).1
                } else if m.alt() {
                    self.word_bounds(head, true)
                } else if !shift && !sel.is_empty() {
                    sel.range().end
                } else {
                    (head + 1).min(n)
                };
                self.move_to(cx, to, shift, false);
            }
            Key::Named(NamedKey::ArrowUp) => {
                let to = if m.super_key() { 0 } else { self.vertical(cx, false, 1) };
                self.move_to(cx, to, shift, !m.super_key());
            }
            Key::Named(NamedKey::ArrowDown) => {
                let to = if m.super_key() { n } else { self.vertical(cx, true, 1) };
                self.move_to(cx, to, shift, !m.super_key());
            }
            Key::Named(NamedKey::Home) => self.move_to(cx, self.line_bounds(head).0, shift, false),
            Key::Named(NamedKey::End) => self.move_to(cx, self.line_bounds(head).1, shift, false),
            Key::Named(NamedKey::PageDown) | Key::Named(NamedKey::PageUp) => {
                let rows = ((self.size.height / self.line_h.max(1.0)) as usize).saturating_sub(1).max(1);
                let down = matches!(k.key, Key::Named(NamedKey::PageDown));
                let to = self.vertical(cx, down, rows);
                self.move_to(cx, to, shift, true);
            }
            Key::Named(NamedKey::Backspace) => {
                let g = if m.super_key() {
                    Granularity::Line
                } else if m.alt() {
                    Granularity::Word
                } else {
                    Granularity::Char
                };
                self.edit(cx, EditorAction::Backspace(g));
            }
            Key::Named(NamedKey::Delete) => {
                let g = if m.alt() { Granularity::Word } else { Granularity::Char };
                self.edit(cx, EditorAction::Delete(g));
            }
            Key::Named(NamedKey::Enter) => self.edit(cx, EditorAction::Newline),
            Key::Named(NamedKey::Tab) if !m.control() && !m.super_key() => {
                if !self.editable() {
                    return Status::Ignored;
                }
                self.edit(cx, if shift { EditorAction::Outdent } else { EditorAction::Indent });
            }
            Key::Character(c) if m.super_key() && !m.control() => {
                let done = match c.to_lowercase().as_str() {
                    "a" => self.command(cx, EditCommand::SelectAll),
                    "c" => self.command(cx, EditCommand::Copy),
                    "x" => self.command(cx, EditCommand::Cut),
                    "v" => self.command(cx, EditCommand::Paste),
                    "z" if shift => self.command(cx, EditCommand::Redo),
                    "z" => self.command(cx, EditCommand::Undo),
                    _ => false,
                };
                return if done { Status::Handled } else { Status::Ignored };
            }
            Key::Character(c) if m.control() && !m.super_key() && c == "k" => {
                self.edit(cx, EditorAction::Delete(Granularity::Line))
            }
            _ => match &k.text {
                Some(t) if !t.is_empty() => self.edit(cx, EditorAction::Insert(t.clone())),
                _ => return Status::Ignored,
            },
        }
        Status::Handled
    }
}

impl Widget for EditorWidget {
    fn name(&self) -> &'static str {
        "text-editor"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let mut size = Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 480.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 320.0 },
        );
        self.size = size;
        let font = self.font(t);
        self.line_h = (font.size * self.opts.line_height).round();
        let lines = self.line_count();
        // The board's gutter: numbers right-aligned in a column 30pt wide
        // at 13pt (wider for more digits), then 14pt to the text.
        let em = font.size / 13.0;
        self.gutter = if self.opts.line_numbers {
            let digits = (lines.max(1) as f64).log10().floor() + 1.0;
            let w = cx.measure("0", &TextSpec::new(FontSpec::mono(font.size), t.faint())).width;
            (digits.max(2.0) * w + 14.4 * em).ceil()
        } else {
            0.0
        };
        self.text_x = if self.opts.line_numbers { self.gutter + (14.0 * em).round() } else { self.opts.padding };
        let width = self.wrap_width(t, size.width);
        if let Some((min, max)) = self.opts.fit {
            // As tall as the lines laid out, within the limits.
            let room = self.opts.padding * 2.0;
            let cap = max as f64 * self.line_h;
            let mut h = 0.0;
            for line in 0..lines {
                h += self.lay(t, cx.env.text, line, width, "", false).h;
                if h >= cap {
                    break;
                }
            }
            let fitted = h.clamp(min as f64 * self.line_h, cap) + room;
            size.height = bc.constrain(Size::new(size.width, fitted)).height;
            self.size = size;
        }
        let st = cx.state::<EditorState>();
        let focused = st.focused;
        let preedit = st.preedit.clone();
        let preedit_caret = st.preedit_caret;
        // Show the caret when the editor moved it (keys, clicks) or the
        // program did (found text, a line gone to): the program's moves
        // are new selections the editor didn't send.
        let own = std::mem::take(&mut st.reveal);
        let program_moved = st.shown_selection.is_some_and(|s| s != self.snap.selection);
        st.shown_selection = Some(self.snap.selection);
        let reveal = own || program_moved;
        let width_changed = (st.width - size.width).abs() > 0.5;
        st.width = size.width;
        let mut top = st.top_line.min(lines.saturating_sub(1));
        let st_top = top;
        let mut prefetched = false;
        let mut off = st.top_offset;
        let mut scroll_x = st.scroll_x;
        let pad = self.opts.padding;
        // Show the caret: scroll up to its line, or down until it's in
        // sight. A jump the program made far out of sight puts the line a
        // third of the way down, with what comes before it.
        let head = self.snap.selection.head.min(self.snap.rope.len_chars());
        let caret_line = self.snap.rope.char_to_line(head);
        let rows = ((size.height - 2.0 * pad) / self.line_h.max(1.0)).floor().max(1.0) as usize;
        let far = |top: usize| caret_line + 1 < top || caret_line > top + rows + 1;
        if reveal && !own && far(top) {
            top = caret_line.saturating_sub(rows / 3);
            off = 0.0;
        } else if reveal {
            if caret_line < top || (caret_line == top && off > 0.0) {
                top = caret_line;
                off = 0.0;
            } else {
                // From the caret's line up, until the view is full: a
                // screen of lines none of which showed, after a jump, laid
                // out together first.
                if caret_line > top + rows {
                    let from = caret_line.saturating_sub(rows + 1);
                    self.prefetch(t, cx.env.text, from..caret_line + 1, width, focused, caret_line, &preedit);
                    prefetched = true;
                }
                let mut h = 0.0;
                let mut first = caret_line;
                loop {
                    let lh = self.lay(t, cx.env.text, first, width, "", focused).h;
                    if h + lh > size.height - 2.0 * pad {
                        break;
                    }
                    h += lh;
                    if first == 0 || first <= top {
                        break;
                    }
                    first -= 1;
                }
                if first > top {
                    top = first;
                    off = 0.0;
                }
            }
        }
        // Most of a screen of lines that didn't show (a jump, a page), laid
        // out together first.
        if !prefetched && top.abs_diff(st_top) > rows / 2 {
            self.prefetch(t, cx.env.text, top..(top + rows + 2).min(lines), width, focused, caret_line, &preedit);
        }
        let mut laid = Vec::new();
        let mut y = pad - off;
        let mut line = top;
        while line < lines && y < size.height {
            let p = if line == caret_line { preedit.as_str() } else { "" };
            let mut l = self.lay(t, cx.env.text, line, width, p, focused);
            l.y = y.round();
            y += l.h;
            laid.push(l);
            line += 1;
        }
        self.lines = laid;
        self.caret = self.caret_rect(head, preedit_caret);
        // Sideways, without wrapping: keep the caret in sight.
        if !self.opts.wrap
            && reveal
            && let Some(c) = self.caret
        {
            let visible = size.width - self.text_x - self.opts.padding;
            let x = c.x0 - self.text_x + scroll_x;
            if x - scroll_x > visible {
                scroll_x = x - visible + 8.0;
            } else if x < scroll_x {
                scroll_x = (x - 8.0).max(0.0);
            }
            self.caret = self.caret_rect(head, preedit_caret).map(|_| c);
        }
        self.top = (top, off);
        self.scroll_x = scroll_x;
        // What changed since the last layout: the lines whose look or
        // place moved, and the caret.
        let st = cx.state::<EditorState>();
        let mut old = std::mem::take(&mut st.drawn);
        let mut old_caret = st.caret;
        let old_x = std::mem::replace(&mut st.drawn_x, scroll_x);
        st.top_line = top;
        st.top_offset = off;
        st.scroll_x = scroll_x;
        let current = |l: &Laid| self.opts.current_line && l.line == caret_line;
        let drawn: Vec<(usize, f64, f64, u64)> = self
            .lines
            .iter()
            .map(|l| (l.line, l.y, l.h, l.key ^ (current(l) as u64) ^ (self.gutter.to_bits() << 1)))
            .collect();
        st.drawn = drawn.clone();
        st.caret = self.caret;
        let full = width_changed || old.is_empty();
        if !full {
            // Scrolled: the lines still showing move as pixels (sideways
            // only the text, past the gutter), and are compared where
            // they went.
            let bar = [self.bar_strip()];
            let dx = old_x - scroll_x;
            if dx != 0.0 {
                cx.scrolled(Rect::new(self.gutter, 0.0, size.width, size.height), Vec2::new(dx, 0.0), &bar);
            }
            let dy = drawn.iter().find_map(|d| old.iter().find(|o| o.0 == d.0).map(|o| d.1 - o.1)).unwrap_or(0.0);
            if dy != 0.0 {
                cx.scrolled(Rect::from_origin_size(Point::ZERO, size), Vec2::new(0.0, dy), &bar);
                for o in &mut old {
                    o.1 += dy;
                }
            }
            old_caret = old_caret.map(|c| c + Vec2::new(if c.x0 >= self.gutter { dx } else { 0.0 }, dy));
            let mut damage = Vec::new();
            for d in &drawn {
                if !old.contains(d) {
                    damage.push(Rect::new(0.0, d.1, size.width, d.1 + d.2));
                }
            }
            for o in &old {
                if !drawn.contains(o) {
                    damage.push(Rect::new(0.0, o.1, size.width, o.1 + o.2));
                }
            }
            // Below the last line, where lines went.
            let end = drawn.last().map_or(0.0, |d| d.1 + d.2);
            let old_end = old.last().map_or(0.0, |d| d.1 + d.2);
            if old_end > end {
                damage.push(Rect::new(0.0, end, size.width, old_end));
            }
            if old_caret != self.caret {
                damage.extend(old_caret.map(|r| r.inflate(1.0, 1.0)));
                damage.extend(self.caret.map(|r| r.inflate(1.0, 1.0)));
            }
            for d in damage {
                cx.invalidate_rect(d);
            }
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let size = cx.size();
        let focused = cx.is_focused() && cx.window_active();
        let head = self.snap.selection.head.min(self.snap.rope.len_chars());
        let caret_line = self.snap.rope.char_to_line(head);
        let num_spec = TextSpec::new(FontSpec { tabular: true, ..FontSpec::mono(self.font(t).size) }, t.faint());
        let marks = &self.opts.marks;
        for l in &self.lines {
            let row = Rect::new(0.0, l.y, size.width, l.y + l.h);
            if !cx.canvas.needs(row) {
                continue;
            }
            if self.opts.current_line && l.line == caret_line && cx.is_focused() {
                cx.canvas.fill_rect(row, t.focus().with_alpha(0.10));
            }
            let origin = Point::new(self.text_x - self.scroll_x, l.y);
            cx.canvas.save();
            cx.canvas.clip_rect(Rect::new(self.gutter, 0.0, size.width, size.height));
            // Found text, then the selection, under the glyphs.
            if !marks.is_empty() {
                let raw = line_text(&self.snap.rope, l.line);
                let len = raw.chars().count();
                for (i, m) in marks.iter().enumerate() {
                    if m.end < l.start || m.start > l.start + len {
                        continue;
                    }
                    let a = byte_of(&raw, m.start.saturating_sub(l.start).min(len));
                    let b = byte_of(&raw, m.end.saturating_sub(l.start).min(len));
                    let c = if Some(i) == self.opts.current_mark {
                        t.focus().with_alpha(0.55)
                    } else {
                        t.focus().with_alpha(0.25)
                    };
                    for r in l.layout.selection_rects(a..b) {
                        cx.canvas.fill_rect(r + origin.to_vec2(), c);
                    }
                }
            }
            if let Some(sel) = &l.selection {
                let fill = if focused { t.selection() } else { t.line2() };
                for r in l.layout.selection_rects(sel.clone()) {
                    cx.canvas.fill_rect((r + origin.to_vec2()).inflate(0.0, 0.0), fill);
                }
                if l.selects_break {
                    let end = l.layout.caret(l.text.len(), false);
                    let x = origin.x + end.x;
                    cx.canvas.fill_rect(Rect::new(x, l.y + end.top, x + 6.0, l.y + end.top + end.height), fill);
                }
            }
            cx.canvas.draw_text(&l.layout, origin);
            cx.canvas.restore();
            if self.opts.line_numbers {
                let spec = if l.line == caret_line {
                    TextSpec { color: t.text3(), ..num_spec.clone() }
                } else {
                    num_spec.clone()
                };
                // On the first row's baseline.
                let number = cx.layout_text(&format!("{}", l.line + 1), &spec, None);
                let base = l.layout.lines().first().map_or(0.0, |r| r.baseline);
                let x = (self.gutter - number.size().width).round();
                cx.canvas.draw_text(&number, Point::new(x, (l.y + base - number.first_baseline()).round()));
            }
        }
        if self.snap.rope.len_chars() == 0
            && let Some(p) = &self.opts.placeholder
        {
            let spec = TextSpec::new(self.font(t), t.text3());
            let y = self.lines.first().map_or(8.0, |l| l.y);
            cx.draw_text_in(
                p,
                &spec,
                Rect::new(self.text_x, y, size.width, y + self.line_h),
                crate::layout::Align::Start,
            );
        }
        let blink_off = cx.state::<EditorState>().is_some_and(|s| s.blink_off);
        if focused
            && !blink_off
            && self.snap.selection.is_empty()
            && !self.opts.read_only
            && let Some(c) = self.caret
            && c.x0 >= self.gutter
        {
            cx.canvas.fill_rect(c, self.opts.caret_color.unwrap_or(t.focus()));
        }
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        // A thin bar showing where the view is in the document, while it
        // scrolls (as the scroll views' bars).
        let lines = self.line_count().max(1);
        if lines as f64 * self.line_h <= self.size.height {
            return;
        }
        let now = cx.now();
        if !cx.state::<EditorState>().is_some_and(|s| s.bar_until.is_some_and(|t| now < t)) {
            return;
        }
        let t = cx.theme();
        let h = self.size.height;
        let len = (h * (h / (lines as f64 * self.line_h))).max(24.0).min(h - 4.0);
        let y = 2.0 + (h - 4.0 - len) * (self.top.0 as f64 / (lines.saturating_sub(1)).max(1) as f64);
        let x = self.size.width - 6.0;
        cx.canvas.fill_rect(Rect::new(x, y.round(), x + 3.0, (y + len).round()), t.line2().with_alpha(0.8));
    }

    fn hash(&self, h: &mut PaintHasher) {
        // The lines diff themselves, and move as pixels as it scrolls; the
        // editor as a whole is redrawn when its gutter changes.
        h.add(&(self.opts.line_numbers, self.opts.wrap, self.opts.read_only)).f64(self.gutter);
        // An empty editor shows its placeholder, which the program may change.
        if let Some(placeholder) = &self.opts.placeholder {
            h.str(placeholder);
        }
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn cursor(&self) -> Option<Cursor> {
        Some(Cursor::Text)
    }

    fn clips(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Scroll { delta, .. } => {
                if self.scroll_by(cx, *delta) {
                    Status::Handled
                } else {
                    Status::Ignored
                }
            }
            Event::PointerDown { pos, button: PointerButton::Left, clicks, mods } => {
                let at = self.hit(*pos);
                // A click the program takes (a task's box, a link with ❖).
                if *clicks == 1
                    && let Some(f) = &self.on_click
                    && let Some(msg) = f(at, *mods)
                {
                    cx.emit_any(msg);
                    return Status::Handled;
                }
                let text = &self.snap.rope;
                let r = match clicks {
                    2 => {
                        let (s, e) = self.line_bounds(at);
                        let raw: String = text.slice(s..e).to_string();
                        let w = word_at(&raw, byte_of(&raw, at - s));
                        s + raw[..w.start].chars().count()..s + raw[..w.end].chars().count()
                    }
                    n if *n >= 3 => {
                        let line = text.char_to_line(at.min(text.len_chars()));
                        let s = text.line_to_char(line);
                        let e =
                            if line + 1 < text.len_lines() { text.line_to_char(line + 1) } else { text.len_chars() };
                        s..e
                    }
                    _ => at..at,
                };
                // On the selection: maybe the start of a drag out of it.
                if *clicks == 1 && !mods.shift() && self.on_selection(*pos) {
                    let st = cx.state::<EditorState>();
                    st.drag = None;
                    st.drag_text = Some((*pos, at));
                    return Status::Handled;
                }
                cx.state::<EditorState>().drag_text = None;
                cx.state::<EditorState>().drag = Some(((*clicks).clamp(1, 3), r.clone()));
                if mods.shift() && *clicks == 1 {
                    self.select(cx, self.snap.selection.anchor, at);
                } else {
                    self.select(cx, r.start, r.end);
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } if cx.is_pressed() => {
                if let Some((from, _)) = cx.state::<EditorState>().drag_text {
                    if (*pos - from).hypot() >= DRAG_SLOP {
                        cx.state::<EditorState>().drag_text = None;
                        let text = self.snap.rope.slice(self.snap.selection.range()).to_string();
                        cx.start_drag(sidestep_ui::DragSource::new().text(&text).actions(true, false));
                    }
                    return Status::Handled;
                }
                let Some((kind, from)) = cx.state::<EditorState>().drag.clone() else { return Status::Handled };
                let at = self.hit(*pos);
                let (anchor, head) = match kind {
                    1 => (from.start, at),
                    _ if at < from.start => {
                        let (s, _) = self.line_bounds(at);
                        let b = if kind == 3 { s } else { self.word_bounds(at + 1, false).min(at) };
                        (from.end, b)
                    }
                    _ => {
                        let (_, e) = self.line_bounds(at);
                        let b = if kind == 3 {
                            (e + 1).min(self.snap.rope.len_chars())
                        } else {
                            self.word_bounds(at, true).max(at)
                        };
                        (from.start, b)
                    }
                };
                let s = self.snap.selection;
                if s.anchor != anchor || s.head != head {
                    self.select(cx, anchor, head);
                }
                // Past the edges, scroll.
                if pos.y < 0.0 || pos.y > self.size.height {
                    let d = if pos.y < 0.0 { pos.y } else { pos.y - self.size.height };
                    self.scroll_by(cx, Vec2::new(0.0, d));
                }
                Status::Handled
            }
            Event::PointerUp { .. } => {
                cx.state::<EditorState>().drag = None;
                // A press on the selection that didn't drag: the caret
                // goes where it was.
                if let Some((_, at)) = cx.state::<EditorState>().drag_text.take() {
                    self.select(cx, at, at);
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed => self.key(cx, k),
            Event::Ime(Ime::Commit(text)) => {
                cx.state::<EditorState>().preedit.clear();
                self.edit(cx, EditorAction::Insert(text.clone()));
                Status::Handled
            }
            Event::Ime(Ime::Preedit { text, selection }) => {
                let st = cx.state::<EditorState>();
                st.preedit = text.clone();
                st.preedit_caret = selection.as_ref().map(|r| r.end);
                cx.relayout();
                Status::Handled
            }
            Event::Focus(on) => {
                let st = cx.state::<EditorState>();
                st.focused = *on;
                st.blink_off = false;
                if !*on {
                    st.preedit.clear();
                }
                if *on {
                    let now = cx.now();
                    cx.state::<EditorState>().next_blink = Some(now + BLINK);
                    cx.request_tick(BLINK);
                    let caret = self.caret.unwrap_or(Rect::new(self.text_x, 4.0, self.text_x + 2.0, 20.0));
                    cx.set_text_input(true, Some(caret));
                }
                // The selection changes colour with the focus.
                cx.relayout();
                cx.invalidate();
                Status::Handled
            }
            Event::WindowFocus(_) => {
                cx.relayout();
                cx.invalidate();
                Status::Ignored
            }
            Event::Tick => {
                let now = cx.now();
                // The position bar goes once scrolling has stopped a while.
                let st = cx.state::<EditorState>();
                if st.bar_until.is_some_and(|t| now >= t) {
                    st.bar_until = None;
                    cx.invalidate_rect(self.bar_strip());
                }
                // The caret blinks on its own ticks, not the bar's.
                let st = cx.state::<EditorState>();
                let blink_due = st.next_blink.is_none_or(|b| now + Duration::from_millis(10) >= b);
                if st.focused && blink_due {
                    st.next_blink = Some(now + BLINK);
                    if cx.theme().motion() {
                        let st = cx.state::<EditorState>();
                        st.blink_off = !st.blink_off;
                    }
                    if let Some(c) = self.caret {
                        cx.invalidate_rect(c.inflate(1.0, 1.0));
                        cx.set_text_input(true, Some(c));
                    }
                    cx.request_tick(BLINK);
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        match command {
            EditCommand::SelectAll => {
                self.emit(cx, EditorAction::SelectAll);
                true
            }
            EditCommand::Copy => self.copy(cx),
            EditCommand::Cut => {
                if self.editable() && self.copy(cx) {
                    self.edit(cx, EditorAction::Insert(String::new()));
                    true
                } else {
                    false
                }
            }
            EditCommand::Paste if self.editable() && self.on_paste.is_some() => {
                let types = cx.clipboard_types();
                let taken = self.on_paste.as_ref().and_then(|f| f(&types));
                match taken {
                    Some(msg) => {
                        cx.emit_any(msg);
                        true
                    }
                    None => match cx.clipboard_text() {
                        Some(t) => {
                            self.edit(cx, EditorAction::Paste(t));
                            true
                        }
                        None => false,
                    },
                }
            }
            EditCommand::Paste | EditCommand::PasteAsPlainText => match cx.clipboard_text() {
                Some(t) if self.editable() => {
                    self.edit(cx, EditorAction::Paste(t));
                    true
                }
                _ => false,
            },
            EditCommand::Delete => {
                self.edit(cx, EditorAction::Delete(Granularity::Char));
                true
            }
            EditCommand::Undo => {
                self.edit(cx, EditorAction::Undo);
                self.snap.can_undo
            }
            EditCommand::Redo => {
                self.edit(cx, EditorAction::Redo);
                self.snap.can_redo
            }
            EditCommand::Find
            | EditCommand::FindNext
            | EditCommand::FindPrevious
            | EditCommand::UseSelectionForFind
            | EditCommand::Replace => {
                let Some(f) = &self.on_find else { return false };
                let req = match command {
                    EditCommand::Find => FindRequest::Show,
                    EditCommand::FindNext => FindRequest::Next,
                    EditCommand::FindPrevious => FindRequest::Previous,
                    EditCommand::Replace => FindRequest::Replace,
                    _ => FindRequest::UseSelection(self.snap.rope.slice(self.snap.selection.range()).to_string()),
                };
                cx.emit_any(f.make(req));
                true
            }
        }
    }

    fn can(&self, _state: StateView, command: EditCommand) -> bool {
        let sel = !self.snap.selection.is_empty();
        match command {
            EditCommand::Copy => sel,
            EditCommand::Cut | EditCommand::Delete => sel && self.editable(),
            EditCommand::Paste | EditCommand::PasteAsPlainText => self.editable(),
            EditCommand::SelectAll => self.snap.rope.len_chars() > 0,
            EditCommand::Undo => self.editable() && self.snap.can_undo,
            EditCommand::Redo => self.editable() && self.snap.can_redo,
            EditCommand::UseSelectionForFind => sel && self.on_find.is_some(),
            _ => self.on_find.is_some(),
        }
    }

    fn selection(&self, _state: StateView) -> Option<String> {
        let r = self.snap.selection.range();
        (!r.is_empty()).then(|| self.snap.rope.slice(r).to_string())
    }

    fn reveal(&mut self, _cx: &mut EventCx, _rect: Rect) -> bool {
        false
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::MultilineTextInput,
            value: Some(format!("{} lines", self.line_count())),
            disabled: self.opts.read_only,
            actions: vec![SemanticAction::Focus, SemanticAction::SetValue],
            ..Default::default()
        })
    }
}
