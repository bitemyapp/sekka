//! The composer's text: several lines that grow with what's written (up
//! to a limit, then scroll), where Return sends and ⇧Return (or ⌥Return)
//! starts a new line.
//!
//! The kit's `text_editor` takes Return for itself, and modifier keys
//! reach no widget, so a program can't tell Return from ⇧Return through
//! it (Omperor's kit adds `on_key` for this). This field is small and of
//! its own: the program keeps the text in a kit [`EditorContent`] and
//! applies the [`EditorAction`]s it sends, as with `text_editor`, so undo,
//! typing bursts and the selection are the kit's.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::editor::{EditorSnapshot, Granularity, Selection};
use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Length, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Cursor, Ime, Key, KeyEvent, NamedKey, PointerButton, TextLayout, Wrap};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics,
    StateView, Status, Widget,
};
use aizome_kit::{EditorAction, EditorContent, Element, FontSpec, Layoutable, Span};

pub struct Composer<Msg> {
    snap: EditorSnapshot,
    placeholder: Arc<str>,
    font: Option<FontSpec>,
    line_height: f64,
    lines: (usize, usize),
    padding: (f64, f64),
    on_action: Option<Rc<dyn Fn(EditorAction) -> Msg>>,
    on_send: Option<Msg>,
    label: Option<Arc<str>>,
}

/// A composer showing `content`.
pub fn composer<Msg>(content: &EditorContent) -> Composer<Msg> {
    Composer {
        snap: content.snapshot(),
        placeholder: Arc::from(""),
        font: None,
        line_height: 1.45,
        lines: (2, 10),
        padding: (12.0, 10.0),
        on_action: None,
        on_send: None,
        label: None,
    }
}

impl<Msg> Composer<Msg> {
    pub fn placeholder(mut self, t: impl Into<Arc<str>>) -> Self {
        self.placeholder = t.into();
        self
    }

    pub fn font(mut self, f: FontSpec) -> Self {
        self.font = Some(f);
        self
    }

    /// As tall as `min` lines at least, `max` at most (then it scrolls).
    pub fn lines(mut self, min: usize, max: usize) -> Self {
        self.lines = (min.max(1), max.max(min.max(1)));
        self
    }

    /// Room around the text, across and down.
    pub fn padding(mut self, x: f64, y: f64) -> Self {
        self.padding = (x, y);
        self
    }

    pub fn on_action(mut self, f: impl Fn(EditorAction) -> Msg + 'static) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    /// What Return sends (none: Return does nothing, ⇧Return a new line).
    pub fn on_send(mut self, msg: Option<Msg>) -> Self {
        self.on_send = msg;
        self
    }

    pub fn accessible_label(mut self, l: impl Into<Arc<str>>) -> Self {
        self.label = Some(l.into());
        self
    }
}

impl<Msg: Clone + Send + 'static> From<Composer<Msg>> for Element<Msg> {
    fn from(c: Composer<Msg>) -> Element<Msg> {
        let text = c.snap.rope.to_string();
        let on_action = c.on_action.map(|f| {
            let f: Rc<dyn Fn(EditorAction) -> MsgFn> = Rc::new(move |a| MsgFn::new(f(a)));
            f
        });
        let w = ComposerWidget {
            text,
            selection: c.snap.selection,
            version: c.snap.version,
            can_undo: c.snap.can_undo,
            can_redo: c.snap.can_redo,
            placeholder: c.placeholder,
            font: c.font,
            line_height: c.line_height,
            lines: c.lines,
            padding: c.padding,
            on_action,
            on_send: c.on_send.map(MsgFn::new),
            label: c.label,
            laid: None,
            shown: String::new(),
            preedit_at: None,
            line_h: 20.0,
            size: Size::ZERO,
        };
        Element::new(w, Vec::new()).width(Length::Fill)
    }
}

struct ComposerWidget {
    text: String,
    /// In characters, as the kit's editor keeps it.
    selection: Selection,
    version: u64,
    can_undo: bool,
    can_redo: bool,
    placeholder: Arc<str>,
    font: Option<FontSpec>,
    line_height: f64,
    lines: (usize, usize),
    padding: (f64, f64),
    on_action: Option<Rc<dyn Fn(EditorAction) -> MsgFn>>,
    on_send: Option<MsgFn>,
    label: Option<Arc<str>>,
    laid: Option<TextLayout>,
    /// The text as laid out: with what's being composed at the caret, and
    /// a zero-width space so a last empty line has somewhere to be.
    shown: String,
    /// Where the composition starts in `shown`, and its length.
    preedit_at: Option<(usize, usize)>,
    line_h: f64,
    size: Size,
}

#[derive(Default)]
struct ComposerState {
    focused: bool,
    scroll: f64,
    preedit: String,
    /// The caret's x for ↑ and ↓ to keep to.
    goal_x: Option<f64>,
    /// A drag selecting: what was pressed (a character, word or line) and
    /// how many clicks made it.
    drag: Option<(Range<usize>, u32)>,
    /// What the scroll was last kept for: the version and the caret.
    kept_for: Option<(u64, usize)>,
}

const ZWSP: &str = "\u{200B}";

impl ComposerWidget {
    fn spec(&self, cx_theme: &aizome_kit::Theme) -> TextSpec {
        let font = self.font.unwrap_or(FontSpec::ui(cx_theme.metrics.body));
        let mut spec = TextSpec::new(font, cx_theme.text());
        spec.wrap = Wrap::Words;
        spec.line_height = self.line_height;
        spec
    }

    fn chars(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte of character `c` in the text.
    fn byte(&self, c: usize) -> usize {
        self.text.char_indices().nth(c).map_or(self.text.len(), |(b, _)| b)
    }

    /// The character at byte `b` of the text.
    fn char_at(&self, b: usize) -> usize {
        let b = b.min(self.text.len());
        self.text[..b].chars().count()
    }

    /// A byte of the text as a byte of what's shown (past a composition).
    fn shown_byte(&self, b: usize) -> usize {
        match self.preedit_at {
            Some((at, len)) if b >= at => b + len,
            _ => b,
        }
    }

    fn text_origin(&self, scroll: f64) -> Point {
        Point::new(self.padding.0, self.padding.1 - scroll)
    }

    /// The character under `p` (the widget's coordinates).
    fn hit(&self, p: Point, scroll: f64) -> usize {
        let Some(l) = &self.laid else { return 0 };
        let o = self.text_origin(scroll);
        let mut b = l.hit_test(Point::new(p.x - o.x, p.y - o.y)).offset;
        if let Some((at, len)) = self.preedit_at
            && b > at
        {
            b = b.saturating_sub(len).max(at);
        }
        self.char_at(b.min(self.text.len()))
    }

    /// The caret before character `c`, in the text's coordinates.
    fn caret_rect(&self, c: usize) -> Option<Rect> {
        let l = self.laid.as_ref()?;
        let b = self.shown_byte(self.byte(c));
        let caret = l.caret(b, false);
        let h = if caret.height > 0.0 { caret.height } else { self.line_h };
        Some(Rect::new(caret.x, caret.top, caret.x + 1.5, caret.top + h))
    }

    fn emit(&self, cx: &mut EventCx, a: EditorAction) {
        if let Some(f) = &self.on_action {
            let m = f(a);
            cx.emit(&m);
        }
    }

    fn select(&self, cx: &mut EventCx, anchor: usize, head: usize) {
        self.emit(cx, EditorAction::Select(Selection { anchor, head }));
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// The word boundary from character `at`, forward or back.
    fn word_bound(&self, at: usize, forward: bool) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let n = chars.len();
        let mut i = at.min(n);
        if forward {
            while i < n && !Self::is_word(chars[i]) {
                i += 1;
            }
            while i < n && Self::is_word(chars[i]) {
                i += 1;
            }
        } else {
            while i > 0 && !Self::is_word(chars[i - 1]) {
                i -= 1;
            }
            while i > 0 && Self::is_word(chars[i - 1]) {
                i -= 1;
            }
        }
        i
    }

    /// The word around character `at`.
    fn word(&self, at: usize) -> Range<usize> {
        let chars: Vec<char> = self.text.chars().collect();
        let n = chars.len();
        let at = at.min(n);
        if at < n && !Self::is_word(chars[at]) {
            return at..at + 1;
        }
        let mut s = at;
        while s > 0 && Self::is_word(chars[s - 1]) {
            s -= 1;
        }
        let mut e = at;
        while e < n && Self::is_word(chars[e]) {
            e += 1;
        }
        s..e
    }

    /// The text's line (between newlines) around character `at`.
    fn line(&self, at: usize) -> Range<usize> {
        let chars: Vec<char> = self.text.chars().collect();
        let n = chars.len();
        let at = at.min(n);
        let mut s = at;
        while s > 0 && chars[s - 1] != '\n' {
            s -= 1;
        }
        let mut e = at;
        while e < n && chars[e] != '\n' {
            e += 1;
        }
        s..e
    }

    /// The laid-out line's ends around character `at` (where ❖← and ❖→
    /// go: the visual line, as macOS does).
    fn visual_line(&self, at: usize) -> Range<usize> {
        let Some(l) = &self.laid else { return self.line(at) };
        let b = self.shown_byte(self.byte(at));
        for line in l.lines() {
            if b >= line.range.start && b <= line.range.end {
                let s = self.char_at(line.range.start.min(self.text.len()));
                let mut e = self.char_at(line.range.end.min(self.text.len()));
                // A wrapped line's end is before its last space or newline.
                if e > s && self.text.chars().nth(e - 1).is_some_and(|c| c == '\n') {
                    e -= 1;
                }
                return s..e;
            }
        }
        self.line(at)
    }

    /// ↑ or ↓ from character `at`, keeping to x `goal`.
    fn vertical(&self, at: usize, down: bool, goal: f64) -> usize {
        let (Some(l), Some(c)) = (&self.laid, self.caret_rect(at)) else { return at };
        let lines = l.lines();
        let mid = (c.y0 + c.y1) / 2.0;
        let i = lines.iter().position(|m| mid >= m.top && mid < m.top + m.height).unwrap_or(0);
        let target = if down { i + 1 } else { i.wrapping_sub(1) };
        match lines.get(target) {
            Some(m) => {
                let b = l.hit_test(Point::new(goal, m.top + m.height / 2.0)).offset;
                self.char_at(b.min(self.text.len()))
            }
            None if down => self.chars(),
            None => 0,
        }
    }

    fn key(&self, cx: &mut EventCx, k: &KeyEvent) -> Status {
        let m = k.modifiers;
        let shift = m.shift();
        let n = self.chars();
        let sel = self.selection;
        let head = sel.head.min(n);
        let goal = || self.caret_rect(head).map_or(0.0, |r| r.x0);
        let to = |cx: &mut EventCx, at: usize, keep_goal: bool| {
            let anchor = if shift { sel.anchor } else { at };
            if !keep_goal {
                cx.state::<ComposerState>().goal_x = None;
            }
            self.select(cx, anchor, at);
        };
        match &k.key {
            Key::Named(NamedKey::Enter) => {
                if shift || m.alt() || m.control() {
                    self.emit(cx, EditorAction::Newline);
                } else if let Some(s) = &self.on_send {
                    cx.emit(s);
                }
            }
            Key::Named(NamedKey::Tab) => return Status::Ignored,
            Key::Named(NamedKey::Escape) => return Status::Ignored,
            Key::Named(NamedKey::ArrowLeft) => {
                let at = if m.super_key() {
                    self.visual_line(head).start
                } else if m.alt() {
                    self.word_bound(head, false)
                } else if !shift && !sel.is_empty() {
                    sel.range().start
                } else {
                    head.saturating_sub(1)
                };
                to(cx, at, false);
            }
            Key::Named(NamedKey::ArrowRight) => {
                let at = if m.super_key() {
                    self.visual_line(head).end
                } else if m.alt() {
                    self.word_bound(head, true)
                } else if !shift && !sel.is_empty() {
                    sel.range().end
                } else {
                    (head + 1).min(n)
                };
                to(cx, at, false);
            }
            Key::Named(NamedKey::ArrowUp) | Key::Named(NamedKey::ArrowDown) => {
                let down = matches!(k.key, Key::Named(NamedKey::ArrowDown));
                let at = if m.super_key() {
                    if down { n } else { 0 }
                } else {
                    let g = *cx.state::<ComposerState>().goal_x.get_or_insert_with(goal);
                    self.vertical(head, down, g)
                };
                to(cx, at, !m.super_key());
            }
            Key::Named(NamedKey::Home) => to(cx, self.visual_line(head).start, false),
            Key::Named(NamedKey::End) => to(cx, self.visual_line(head).end, false),
            Key::Named(NamedKey::Backspace) => {
                let g = if m.super_key() {
                    Granularity::Line
                } else if m.alt() {
                    Granularity::Word
                } else {
                    Granularity::Char
                };
                self.emit(cx, EditorAction::Backspace(g));
            }
            Key::Named(NamedKey::Delete) => {
                let g = if m.alt() { Granularity::Word } else { Granularity::Char };
                self.emit(cx, EditorAction::Delete(g));
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
            _ => match &k.text {
                Some(t) if !t.is_empty() => self.emit(cx, EditorAction::Insert(t.clone())),
                _ => return Status::Ignored,
            },
        }
        Status::Handled
    }

    fn copy(&self, cx: &mut EventCx) -> bool {
        let r = self.selection.range();
        if r.is_empty() {
            return false;
        }
        let s: String = self.text.chars().skip(r.start).take(r.end - r.start).collect();
        cx.set_clipboard_text(&s);
        true
    }

    fn command(&self, cx: &mut EventCx, command: EditCommand) -> bool {
        match command {
            EditCommand::SelectAll => {
                self.emit(cx, EditorAction::SelectAll);
                true
            }
            EditCommand::Copy => self.copy(cx),
            EditCommand::Cut => {
                if self.copy(cx) {
                    self.emit(cx, EditorAction::Insert(String::new()));
                    true
                } else {
                    false
                }
            }
            EditCommand::Paste | EditCommand::PasteAsPlainText => match cx.clipboard_text() {
                Some(t) => {
                    self.emit(cx, EditorAction::Paste(t));
                    true
                }
                None => false,
            },
            EditCommand::Delete => {
                self.emit(cx, EditorAction::Delete(Granularity::Char));
                true
            }
            EditCommand::Undo => {
                self.emit(cx, EditorAction::Undo);
                self.can_undo
            }
            EditCommand::Redo => {
                self.emit(cx, EditorAction::Redo);
                self.can_redo
            }
            _ => false,
        }
    }
}

impl Widget for ComposerWidget {
    fn name(&self) -> &'static str {
        "composer"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let theme = cx.theme().clone();
        let spec = self.spec(&theme);
        let width = if bc.max.width.is_finite() { bc.max.width } else { 480.0 };
        let text_w = (width - 2.0 * self.padding.0).max(40.0);
        let preedit = cx.state::<ComposerState>().preedit.clone();
        let caret_b = self.byte(self.selection.head.min(self.chars()));
        let mut spans = Vec::new();
        if preedit.is_empty() {
            self.preedit_at = None;
            spans.push(Span::new(self.text.clone()));
        } else {
            self.preedit_at = Some((caret_b, preedit.len()));
            spans.push(Span::new(self.text[..caret_b].to_owned()));
            spans.push(Span::new(preedit.clone()).underline());
            spans.push(Span::new(self.text[caret_b..].to_owned()));
        }
        spans.push(Span::new(ZWSP));
        self.shown = spans.iter().map(|s| s.text.as_str()).collect();
        let laid = cx.text_cache().layout_spans(&theme, &spans, &spec, Some(text_w));
        self.line_h = (spec.font.size * if self.line_height > 0.0 { self.line_height } else { 1.25 }).round();
        let content = laid.size().height.max(self.line_h);
        self.laid = Some(laid);
        let (min, max) = self.lines;
        let shown_h = content.clamp(min as f64 * self.line_h, max as f64 * self.line_h);
        let size = bc.constrain(Size::new(width, (shown_h + 2.0 * self.padding.1).ceil()));
        self.size = size;
        // Keep the caret in sight when the text or the caret moved.
        let visible = size.height - 2.0 * self.padding.1;
        let head = self.selection.head;
        let caret = self.caret_rect(head);
        let st = cx.state::<ComposerState>();
        let key = (self.version, head);
        if st.kept_for != Some(key) {
            st.kept_for = Some(key);
            if let Some(c) = caret {
                if c.y0 < st.scroll {
                    st.scroll = c.y0;
                } else if c.y1 > st.scroll + visible {
                    st.scroll = c.y1 - visible;
                }
            }
        }
        st.scroll = st.scroll.clamp(0.0, (content - visible).max(0.0)).round();
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let Some(l) = &self.laid else { return };
        let t = cx.theme();
        let (focused, scroll) = cx.state::<ComposerState>().map_or((false, 0.0), |s| (s.focused, s.scroll));
        let inner = Rect::new(0.0, 0.0, self.size.width, self.size.height);
        cx.canvas.save();
        cx.canvas.clip_rect(inner);
        let o = self.text_origin(scroll);
        if self.text.is_empty() && self.preedit_at.is_none() {
            let spec = TextSpec { color: t.text3(), ..self.spec(t) };
            let p = cx.layout_text(&self.placeholder, &spec, Some((self.size.width - 2.0 * self.padding.0).max(40.0)));
            cx.canvas.draw_text(&p, o);
        }
        let r = self.selection.range();
        if !r.is_empty() {
            let a = self.shown_byte(self.byte(r.start));
            let b = self.shown_byte(self.byte(r.end));
            let fill = if focused { t.selection().with_alpha(0.45) } else { t.line2() };
            for s in l.selection_rects(a..b) {
                cx.canvas.fill_rect(s + o.to_vec2(), fill);
            }
        }
        cx.canvas.draw_text(l, o);
        if focused
            && r.is_empty()
            && let Some(c) = self.caret_rect(self.selection.head)
        {
            cx.canvas.fill_rect(c + o.to_vec2(), t.focus());
        }
        cx.canvas.restore();
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.text).str(&self.placeholder);
        h.add(&(self.selection.anchor, self.selection.head, self.version));
        h.add(&self.preedit_at);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn cursor(&self) -> Option<Cursor> {
        Some(Cursor::Text)
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, clicks, mods } => {
                cx.request_focus();
                cx.capture_pointer();
                let scroll = cx.state::<ComposerState>().scroll;
                let at = self.hit(*pos, scroll);
                let range = match clicks {
                    1 => at..at,
                    2 => self.word(at),
                    _ => self.line(at),
                };
                let st = cx.state::<ComposerState>();
                st.goal_x = None;
                st.drag = Some((range.clone(), (*clicks).min(3)));
                if *clicks == 1 && mods.shift() {
                    self.select(cx, self.selection.anchor, at);
                } else {
                    self.select(cx, range.start, range.end);
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let st = cx.state::<ComposerState>();
                let Some((from, clicks)) = st.drag.clone() else { return Status::Ignored };
                let scroll = st.scroll;
                let at = self.hit(*pos, scroll);
                let (anchor, head) = match clicks {
                    1 => (from.start, at),
                    _ if at < from.start => {
                        let b = if clicks == 3 { self.line(at).start } else { self.word(at).start };
                        (from.end, b)
                    }
                    _ => {
                        let b = if clicks == 3 { self.line(at).end } else { self.word(at).end };
                        (from.start, b)
                    }
                };
                if self.selection.anchor != anchor || self.selection.head != head {
                    self.select(cx, anchor, head);
                }
                Status::Handled
            }
            Event::PointerUp { .. } => {
                cx.release_pointer();
                cx.state::<ComposerState>().drag = None;
                Status::Handled
            }
            Event::Scroll { delta, .. } => {
                let visible = self.size.height - 2.0 * self.padding.1;
                let content = self.laid.as_ref().map_or(0.0, |l| l.size().height);
                let st = cx.state::<ComposerState>();
                let before = st.scroll;
                st.scroll = (st.scroll + delta.y).clamp(0.0, (content - visible).max(0.0)).round();
                if st.scroll != before {
                    cx.invalidate();
                    Status::Handled
                } else {
                    Status::Ignored
                }
            }
            Event::Key(k) if k.pressed => self.key(cx, k),
            Event::Ime(Ime::Commit(text)) => {
                cx.state::<ComposerState>().preedit.clear();
                self.emit(cx, EditorAction::Insert(text.clone()));
                cx.relayout();
                Status::Handled
            }
            Event::Ime(Ime::Preedit { text, .. }) => {
                cx.state::<ComposerState>().preedit = text.clone();
                cx.relayout();
                Status::Handled
            }
            Event::Focus(on) => {
                let st = cx.state::<ComposerState>();
                st.focused = *on;
                if !*on {
                    st.preedit.clear();
                    cx.set_text_input(false, None);
                } else {
                    let scroll = st.scroll;
                    let caret = self.caret_rect(self.selection.head).map(|c| c + self.text_origin(scroll).to_vec2());
                    cx.set_text_input(true, caret);
                }
                cx.invalidate();
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        ComposerWidget::command(self, cx, command)
    }

    fn can(&self, _state: StateView, command: EditCommand) -> bool {
        match command {
            EditCommand::Copy | EditCommand::Cut => !self.selection.is_empty(),
            EditCommand::Paste | EditCommand::PasteAsPlainText | EditCommand::SelectAll => true,
            EditCommand::Undo => self.can_undo,
            EditCommand::Redo => self.can_redo,
            EditCommand::Delete => !self.text.is_empty(),
            _ => false,
        }
    }

    fn selection(&self, _state: StateView) -> Option<String> {
        let r = self.selection.range();
        (!r.is_empty()).then(|| self.text.chars().skip(r.start).take(r.end - r.start).collect())
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::MultilineTextInput,
            label: Some(self.label.as_deref().unwrap_or(&self.placeholder).to_owned()),
            value: Some(self.text.clone()),
            actions: vec![SemanticAction::Focus, SemanticAction::SetValue],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }
}
