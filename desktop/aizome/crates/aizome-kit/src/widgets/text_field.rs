//! `text_field`: one line of text to type in.
//!
//! The program owns the text: the field shows `value`, and every edit
//! comes back as the new text through `on_change`. What only the field
//! cares about — the caret, the selection, what an input method is
//! composing, how far the text is scrolled, its undo history — lives in
//! its state, under its key.
//!
//! Keys are macOS's with ❖ as command: ←/→ by character, ⌥←/→ by word,
//! ❖←/→ (and Home/End) to the ends, ⇧ to extend; ⌫ and ⌦ by character,
//! with ⌥ by word and ❖ to the start; ❖A, ❖C, ❖X, ❖V, ❖Z, ⇧❖Z. Return
//! sends `on_submit`; Escape clears a search field.

use std::marker::PhantomData;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sidestep_ui::{Cursor, Ime, Key, NamedKey, PointerButton, TextLayout, Wrap};

use crate::draw;
use crate::editing::{floor, next_char, next_word, prev_char, prev_word, word_at};
use crate::element::{Common, Element, MsgFn, MsgMap, RawElement};
use crate::hash::PaintHasher;
use crate::icons::Icon;
use crate::layout::{Constraints, Length, Point, Rect, Size};
use crate::text::TextSpec;
use crate::theme::{FontSpec, Role, Theme};
use crate::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics,
    Status, Widget,
};

/// How long the caret shows, then hides.
pub(crate) const BLINK: Duration = Duration::from_millis(530);
/// Typing within this of the last keystroke undoes with it.
const GROUP: Duration = Duration::from_millis(1000);
const BULLET: char = '•';

/// One line of text: `text_field(&self.name).placeholder("Name").on_change(Msg::Name)`.
pub struct TextField<Msg> {
    pub(crate) common: Common,
    value: Arc<str>,
    placeholder: Option<Arc<str>>,
    on_change: Option<MsgMap<String>>,
    on_submit: Option<MsgFn>,
    on_blur: Option<MsgFn>,
    password: bool,
    search: bool,
    error: Option<Arc<str>>,
    enabled: bool,
    mono: bool,
    select_on_focus: Option<Range<usize>>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(TextField);

pub fn text_field<Msg>(value: &str) -> TextField<Msg> {
    TextField {
        common: Common::sized(Length::Fill, Length::Shrink),
        value: value.into(),
        placeholder: None,
        on_change: None,
        on_submit: None,
        on_blur: None,
        password: false,
        search: false,
        error: None,
        enabled: true,
        mono: false,
        select_on_focus: None,
        _m: PhantomData,
    }
}

/// A search field: a magnifier, a clear button, Escape clears.
pub fn search_field<Msg>(value: &str) -> TextField<Msg> {
    TextField { search: true, placeholder: Some("Search".into()), ..text_field(value) }
}

impl<Msg: Clone + Send + 'static> TextField<Msg> {
    pub fn on_change(mut self, f: impl Fn(String) -> Msg + 'static) -> Self {
        self.on_change = Some(MsgMap::new(f));
        self
    }

    pub fn on_submit(mut self, msg: Msg) -> Self {
        self.on_submit = Some(MsgFn::new(msg));
        self
    }

    /// Sent when the field gives the focus up (another control clicked,
    /// Tab): editing ends, as Return ends it, for fields whose text is kept
    /// only when done (a name), so it isn't lost by clicking elsewhere.
    pub fn on_blur(mut self, msg: Msg) -> Self {
        self.on_blur = Some(MsgFn::new(msg));
        self
    }
}

impl<Msg> TextField<Msg> {
    pub fn placeholder(mut self, text: impl Into<Arc<str>>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Dots for the characters; nothing copies out.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    pub fn search(mut self) -> Self {
        self.search = true;
        self
    }

    /// Something's wrong with the text: a red edge and `message` under it.
    pub fn error(mut self, message: Option<impl Into<Arc<str>>>) -> Self {
        self.error = message.map(Into::into);
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// IBM Plex Mono.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// What's selected when the field first takes the focus (byte
    /// offsets into its text): a file name without its extension, for a
    /// field renaming a file. Give the field a `.key` of its own per use,
    /// so a new one starts afresh.
    pub fn select_on_focus(mut self, range: Range<usize>) -> Self {
        self.select_on_focus = Some(range);
        self
    }
}

impl<Msg> From<TextField<Msg>> for Element<Msg> {
    fn from(f: TextField<Msg>) -> Element<Msg> {
        let w = FieldWidget {
            value: f.value,
            placeholder: f.placeholder,
            on_change: f.on_change,
            on_submit: f.on_submit,
            on_blur: f.on_blur,
            password: f.password,
            search: f.search,
            error: f.error,
            enabled: f.enabled,
            mono: f.mono,
            select_on_focus: f.select_on_focus,
            laid: None,
            shown: String::new(),
            caret: 0,
            sel: 0..0,
            preedit: 0..0,
            scroll: 0.0,
        };
        Element::from_raw(RawElement::new(w, f.common, Vec::new()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    Typing,
    Deleting,
    Other,
}

/// How far a press on the selection moves before it drags the text out.
const DRAG_SLOP: f64 = 4.0;

/// What a field remembers between views.
#[derive(Default)]
pub(crate) struct FieldState {
    caret: usize,
    anchor: usize,
    scroll: f64,
    preedit: String,
    preedit_caret: Option<usize>,
    blink_off: bool,
    selecting: bool,
    /// A press inside the selection, where and at which offset: moved far
    /// enough, the selected text is dragged out; released, the caret goes
    /// there.
    drag_text: Option<(Point, usize)>,
    focused: bool,
    history: Vec<(String, usize, usize)>,
    future: Vec<(String, usize, usize)>,
    last_edit: Option<(Instant, EditKind)>,
    /// The field has had the focus (its starting selection is used up).
    primed: bool,
}

struct FieldWidget {
    value: Arc<str>,
    placeholder: Option<Arc<str>>,
    on_change: Option<MsgMap<String>>,
    on_submit: Option<MsgFn>,
    on_blur: Option<MsgFn>,
    password: bool,
    search: bool,
    error: Option<Arc<str>>,
    enabled: bool,
    mono: bool,
    select_on_focus: Option<Range<usize>>,
    // As laid out: the text shown (dots, what's composed), its layout,
    // the caret and selection in it, the scroll.
    laid: Option<TextLayout>,
    shown: String,
    caret: usize,
    sel: Range<usize>,
    preedit: Range<usize>,
    scroll: f64,
}

impl FieldWidget {
    fn spec(&self, t: &Theme, color: sidestep_ui::Color) -> TextSpec {
        let f = if self.mono { FontSpec::mono(t.metrics.body) } else { FontSpec::ui(t.metrics.body) };
        TextSpec { wrap: Wrap::None, ..TextSpec::new(f, color) }
    }

    fn editable(&self) -> bool {
        self.enabled && self.on_change.is_some()
    }

    fn box_height(&self, t: &Theme) -> f64 {
        t.metrics.control
    }

    fn left(&self) -> f64 {
        if self.search { 24.0 } else { 7.0 }
    }

    fn right(&self) -> f64 {
        if self.search && !self.value.is_empty() { 24.0 } else { 7.0 }
    }

    fn clear_button(&self, size: Size, t: &Theme) -> Rect {
        let h = self.box_height(t);
        Rect::new(size.width - 22.0, (h - 16.0) / 2.0, size.width - 6.0, (h + 16.0) / 2.0)
    }

    /// A value offset as an offset in the text shown.
    fn shown_offset(&self, value_offset: usize, preedit: &str, caret: usize) -> usize {
        let base = if self.password {
            self.value[..value_offset.min(self.value.len())].chars().count() * BULLET.len_utf8()
        } else {
            value_offset
        };
        if !preedit.is_empty() && value_offset > caret { base + preedit.len() } else { base }
    }

    /// An offset in the text shown as a value offset.
    fn value_offset(&self, shown: usize) -> usize {
        if self.password {
            let n = shown / BULLET.len_utf8();
            self.value.char_indices().nth(n).map_or(self.value.len(), |(i, _)| i)
        } else {
            floor(&self.value, shown)
        }
    }

    fn hit(&self, p: Point, t: &Theme) -> usize {
        let Some(l) = &self.laid else { return 0 };
        let y = (self.box_height(t) - l.size().height) / 2.0;
        let hit = l.hit_test(Point::new(p.x - self.left() + self.scroll, p.y - y));
        if self.value.is_empty() { 0 } else { self.value_offset(hit.offset) }
    }

    fn selection(st: &FieldState) -> Range<usize> {
        st.caret.min(st.anchor)..st.caret.max(st.anchor)
    }

    /// Whether `p` falls on the selected text (never a password's).
    fn on_selection(&self, st: &FieldState, p: Point) -> bool {
        let r = Self::selection(st);
        let r = r.start.min(self.value.len())..r.end.min(self.value.len());
        if self.password || r.is_empty() {
            return false;
        }
        let Some(l) = &self.laid else { return false };
        let x = |o: usize| self.left() + l.caret(self.shown_offset(o, "", st.caret), false).x - st.scroll;
        p.x >= x(r.start) && p.x <= x(r.end)
    }

    /// Replace the selection with `with`: the new value goes to the
    /// program.
    fn replace(&self, cx: &mut EventCx, with: &str, kind: EditKind) {
        if !self.editable() {
            return;
        }
        let value = self.value.to_string();
        let now = cx.now();
        let st = cx.state::<FieldState>();
        let (caret, anchor) = (st.caret.min(value.len()), st.anchor.min(value.len()));
        let range = caret.min(anchor)..caret.max(anchor);
        let group =
            st.last_edit.is_some_and(|(t, k)| k == kind && kind != EditKind::Other && now.duration_since(t) < GROUP);
        if !group {
            st.history.push((value.clone(), caret, anchor));
            if st.history.len() > 200 {
                st.history.remove(0);
            }
        }
        st.future.clear();
        st.last_edit = Some((now, kind));
        let mut new = value;
        new.replace_range(range.clone(), with);
        let at = range.start + with.len();
        st.caret = at;
        st.anchor = at;
        st.blink_off = false;
        self.emit(cx, new);
    }

    fn emit(&self, cx: &mut EventCx, new: String) {
        if let Some(f) = &self.on_change {
            cx.emit_any(f.make(new));
        }
        cx.invalidate();
    }

    fn set_caret(&self, cx: &mut EventCx, at: usize, extend: bool) {
        let st = cx.state::<FieldState>();
        st.caret = at;
        if !extend {
            st.anchor = at;
        }
        st.blink_off = false;
        st.last_edit = None;
        cx.relayout();
        cx.invalidate();
        self.follow_caret(cx);
    }

    /// Scroll so the caret shows, and tell the input method where it is.
    fn follow_caret(&self, cx: &mut EventCx) {
        let Some(l) = &self.laid else { return };
        let t = cx.theme().clone();
        let w = cx.size().width;
        let visible = (w - self.left() - self.right()).max(1.0);
        let caret = cx.state::<FieldState>().caret.min(self.value.len());
        let x = l.caret(self.shown_offset(caret, "", caret), false).x;
        let st = cx.state::<FieldState>();
        if x - st.scroll > visible - 2.0 {
            st.scroll = x - visible + 2.0;
        } else if x < st.scroll {
            st.scroll = x;
        }
        st.scroll = st.scroll.clamp(0.0, (l.size().width - visible + 2.0).max(0.0));
        let h = self.box_height(&t);
        let caret_rect = Rect::new(self.left() + x - st.scroll, 3.0, self.left() + x - st.scroll + 2.0, h - 3.0);
        if st.focused {
            cx.set_text_input(true, Some(caret_rect));
        }
    }

    fn copy(&self, cx: &mut EventCx) -> bool {
        let st = cx.state::<FieldState>();
        let r = FieldWidget::selection(st);
        if self.password || r.is_empty() || r.end > self.value.len() {
            return false;
        }
        let text = self.value[r].to_owned();
        cx.set_clipboard_text(&text);
        true
    }

    fn undo(&self, cx: &mut EventCx, redo: bool) -> bool {
        let value = self.value.to_string();
        let st = cx.state::<FieldState>();
        let (from, to) = if redo { (&mut st.future, &mut st.history) } else { (&mut st.history, &mut st.future) };
        let Some((v, c, a)) = from.pop() else { return false };
        to.push((value, st.caret, st.anchor));
        st.caret = c;
        st.anchor = a;
        st.last_edit = None;
        self.emit(cx, v);
        true
    }

    fn key(&mut self, cx: &mut EventCx, k: &sidestep_ui::KeyEvent) -> Status {
        let m = k.modifiers;
        let value = self.value.clone();
        let st = cx.state::<FieldState>();
        let (caret, anchor) = (st.caret.min(value.len()), st.anchor.min(value.len()));
        let sel = caret.min(anchor)..caret.max(anchor);
        let shift = m.shift();
        match &k.key {
            Key::Named(NamedKey::ArrowLeft) => {
                let to = if m.super_key() {
                    0
                } else if m.alt() {
                    prev_word(&value, caret)
                } else if !shift && !sel.is_empty() {
                    sel.start
                } else {
                    prev_char(&value, caret)
                };
                self.set_caret(cx, to, shift);
            }
            Key::Named(NamedKey::ArrowRight) => {
                let to = if m.super_key() {
                    value.len()
                } else if m.alt() {
                    next_word(&value, caret)
                } else if !shift && !sel.is_empty() {
                    sel.end
                } else {
                    next_char(&value, caret)
                };
                self.set_caret(cx, to, shift);
            }
            Key::Named(NamedKey::Home | NamedKey::ArrowUp) => self.set_caret(cx, 0, shift),
            Key::Named(NamedKey::End | NamedKey::ArrowDown) => self.set_caret(cx, value.len(), shift),
            Key::Named(NamedKey::Backspace) => {
                if sel.is_empty() {
                    let from = if m.super_key() {
                        0
                    } else if m.alt() {
                        prev_word(&value, caret)
                    } else {
                        prev_char(&value, caret)
                    };
                    cx.state::<FieldState>().anchor = from;
                }
                self.replace(cx, "", EditKind::Deleting);
            }
            Key::Named(NamedKey::Delete) => {
                if sel.is_empty() {
                    let to = if m.alt() { next_word(&value, caret) } else { next_char(&value, caret) };
                    cx.state::<FieldState>().anchor = to;
                }
                self.replace(cx, "", EditKind::Deleting);
            }
            Key::Named(NamedKey::Enter) => match &self.on_submit {
                Some(msg) => cx.emit(msg),
                None => return Status::Ignored,
            },
            Key::Named(NamedKey::Escape) if self.search && !value.is_empty() => {
                cx.state::<FieldState>().anchor = 0;
                cx.state::<FieldState>().caret = value.len();
                self.replace(cx, "", EditKind::Other);
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
                Some(t) if !t.is_empty() && !t.contains(['\n', '\r', '\t']) => {
                    self.replace(cx, t, EditKind::Typing);
                }
                _ => return Status::Ignored,
            },
        }
        Status::Handled
    }
}

impl Widget for FieldWidget {
    fn name(&self) -> &'static str {
        "text-field"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.env.theme;
        let st = cx.state::<FieldState>();
        let caret = floor(&self.value, st.caret);
        let anchor = floor(&self.value, st.anchor);
        st.caret = caret;
        st.anchor = anchor;
        let preedit = st.preedit.clone();
        let preedit_caret = st.preedit_caret;
        let mut scroll = st.scroll;
        let focused = st.focused;
        // What shows: the text (as dots for a password) with what an input
        // method is composing at the caret.
        let mut shown: String =
            if self.password { BULLET.to_string().repeat(self.value.chars().count()) } else { self.value.to_string() };
        let at = self.shown_offset(caret, "", caret);
        if !preedit.is_empty() {
            shown.insert_str(at, &preedit);
        }
        let empty = shown.is_empty();
        let text = if empty { self.placeholder.as_deref().unwrap_or("").to_owned() } else { shown.clone() };
        let color = if empty {
            t.text3()
        } else if self.enabled {
            t.text()
        } else {
            t.faint()
        };
        let laid = cx.env.text.layout(t, &text, &self.spec(t, color), None);
        let width = if bc.max.width.is_finite() { bc.max.width } else { 160.0 };
        let visible = (width - self.left() - self.right()).max(1.0);
        let caret_shown = at + preedit_caret.unwrap_or(preedit.len()).min(preedit.len());
        if !empty {
            let x = laid.caret(caret_shown, false).x;
            if x - scroll > visible - 2.0 {
                scroll = x - visible + 2.0;
            } else if x < scroll {
                scroll = x;
            }
            scroll = scroll.clamp(0.0, (laid.size().width - visible + 2.0).max(0.0));
        } else {
            scroll = 0.0;
        }
        cx.state::<FieldState>().scroll = scroll;
        self.scroll = scroll;
        self.caret = if empty { 0 } else { caret_shown };
        let (a, b) = (
            self.shown_offset(caret.min(anchor), &preedit, caret),
            self.shown_offset(caret.max(anchor), &preedit, caret),
        );
        self.sel = if focused || a != b { a..b } else { 0..0 };
        self.preedit = at..at + preedit.len();
        self.shown = shown;
        self.laid = Some(laid);
        let h = self.box_height(t) + if self.error.is_some() { 16.0 } else { 0.0 };
        bc.constrain(Size::new(width, h))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let size = cx.size();
        let h = self.box_height(t);
        let r = Rect::new(0.0, 0.0, size.width, h);
        let edge = if self.error.is_some() {
            t.role(Role::Danger)
        } else if self.enabled {
            t.line2()
        } else {
            t.line()
        };
        draw::sunken(cx.canvas, t, r, t.sunk(), edge);
        if self.search {
            Icon::Search.draw(cx.canvas, Point::new(7.0, (h - 14.0) / 2.0), 14.0, t.text3());
            if !self.value.is_empty() {
                let b = self.clear_button(size, t);
                Icon::CircleX.draw(cx.canvas, Point::new(b.x0 + 1.0, b.y0 + 1.0), 14.0, t.text3());
            }
        }
        let Some(l) = &self.laid else { return };
        let inner = Rect::new(self.left() - 2.0, 1.0, size.width - self.right() + 2.0, h - 1.0);
        let y = ((h - l.size().height) / 2.0).round();
        let origin = Point::new(self.left() - self.scroll, y);
        let focused = cx.is_focused() && cx.window_active();
        cx.canvas.save();
        cx.canvas.clip_rect(inner);
        if !self.sel.is_empty() && !self.shown.is_empty() {
            let fill = if focused { t.selection() } else { t.line2() };
            for s in l.selection_rects(self.sel.clone()) {
                cx.canvas.fill_rect((s + origin.to_vec2()).intersect(inner), fill);
            }
        }
        cx.canvas.draw_text(l, origin);
        if !self.preedit.is_empty() {
            // What's being composed, underlined.
            for s in l.selection_rects(self.preedit.clone()) {
                let s = s + origin.to_vec2();
                cx.canvas.fill_rect(Rect::new(s.x0, s.y1 - 1.0, s.x1, s.y1), t.text());
            }
        }
        let blink_off = cx.state::<FieldState>().is_some_and(|s| s.blink_off);
        if focused && self.sel.is_empty() && !blink_off && self.editable() {
            let c = l.caret(self.caret, false);
            let x = (origin.x + c.x).round().max(inner.x0 + 1.0);
            let ch = (c.height).max(t.metrics.body + 2.0).min(h - 6.0);
            let cy = ((h - ch) / 2.0).round();
            cx.canvas.fill_rect(Rect::new(x, cy, x + 2.0, cy + ch), t.focus());
        }
        cx.canvas.restore();
        if let Some(e) = &self.error {
            let spec = TextSpec::new(FontSpec::ui(t.metrics.small), t.role(Role::Danger));
            cx.draw_text_in(e, &spec, Rect::new(2.0, h, size.width, size.height), crate::layout::Align::Start);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.shown).add(&(
            self.caret,
            self.sel.clone(),
            self.preedit.clone(),
            self.enabled,
            self.search,
            self.password,
        ));
        h.f64(self.scroll).add(&self.error.as_deref()).add(&self.placeholder.as_deref());
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

    fn focusable(&self) -> Focusable {
        if self.enabled { Focusable::Click } else { Focusable::No }
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Always
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.enabled {
            return Status::Ignored;
        }
        let t = cx.theme().clone();
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, clicks, mods } => {
                if self.search && !self.value.is_empty() && self.clear_button(cx.size(), &t).contains(*pos) {
                    cx.state::<FieldState>().anchor = 0;
                    cx.state::<FieldState>().caret = self.value.len();
                    self.replace(cx, "", EditKind::Other);
                    return Status::Handled;
                }
                let at = self.hit(*pos, &t);
                let value = self.value.clone();
                // On the selection: maybe the start of a drag out of it.
                if *clicks == 1 && !mods.shift() && self.on_selection(cx.state::<FieldState>(), *pos) {
                    let st = cx.state::<FieldState>();
                    st.selecting = false;
                    st.drag_text = Some((*pos, at));
                    return Status::Handled;
                }
                let st = cx.state::<FieldState>();
                st.drag_text = None;
                match clicks {
                    2 => {
                        let w = word_at(&value, at);
                        st.anchor = w.start;
                        st.caret = w.end;
                    }
                    n if *n >= 3 => {
                        st.anchor = 0;
                        st.caret = value.len();
                    }
                    _ => {
                        st.caret = at;
                        if !mods.shift() {
                            st.anchor = at;
                        }
                        st.selecting = true;
                    }
                }
                st.blink_off = false;
                cx.relayout();
                cx.invalidate();
                Status::Handled
            }
            Event::PointerMove { pos, .. } if cx.is_pressed() => {
                if let Some((from, _)) = cx.state::<FieldState>().drag_text {
                    if (*pos - from).hypot() >= DRAG_SLOP {
                        let st = cx.state::<FieldState>();
                        st.drag_text = None;
                        let r = Self::selection(st);
                        let text = self.value.get(r).unwrap_or_default().to_owned();
                        cx.start_drag(sidestep_ui::DragSource::new().text(&text).actions(true, false));
                    }
                    return Status::Handled;
                }
                if cx.state::<FieldState>().selecting {
                    let at = self.hit(*pos, &t);
                    if cx.state::<FieldState>().caret != at {
                        self.set_caret(cx, at, true);
                    }
                }
                Status::Handled
            }
            Event::PointerUp { .. } => {
                let st = cx.state::<FieldState>();
                st.selecting = false;
                // A press on the selection that didn't drag: the caret
                // goes where it was.
                if let Some((_, at)) = st.drag_text.take() {
                    st.caret = at;
                    st.anchor = at;
                    st.blink_off = false;
                    cx.relayout();
                    cx.invalidate();
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed => self.key(cx, k),
            Event::Ime(Ime::Commit(text)) => {
                cx.state::<FieldState>().preedit.clear();
                self.replace(cx, text, EditKind::Typing);
                Status::Handled
            }
            Event::Ime(Ime::Preedit { text, selection }) => {
                let st = cx.state::<FieldState>();
                st.preedit = text.clone();
                st.preedit_caret = selection.as_ref().map(|r| r.end);
                cx.rebuild();
                cx.invalidate();
                Status::Handled
            }
            Event::Focus(on) => {
                let st = cx.state::<FieldState>();
                st.focused = *on;
                st.blink_off = false;
                if *on
                    && !std::mem::replace(&mut st.primed, true)
                    && let Some(r) = &self.select_on_focus
                {
                    let v = &self.value;
                    let (a, b) = (floor(v, r.start.min(v.len())), floor(v, r.end.min(v.len())));
                    st.anchor = a.min(b);
                    st.caret = a.max(b);
                }
                if *on {
                    cx.request_tick(BLINK);
                    self.follow_caret(cx);
                } else {
                    cx.state::<FieldState>().preedit.clear();
                    if let Some(msg) = &self.on_blur {
                        cx.emit(msg);
                    }
                }
                cx.rebuild();
                cx.invalidate();
                Status::Handled
            }
            Event::Tick => {
                let st = cx.state::<FieldState>();
                if st.focused {
                    if cx.theme().motion() {
                        let st = cx.state::<FieldState>();
                        st.blink_off = !st.blink_off;
                    }
                    cx.invalidate();
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
                let n = self.value.len();
                let st = cx.state::<FieldState>();
                st.anchor = 0;
                st.caret = n;
                cx.relayout();
                cx.invalidate();
                true
            }
            EditCommand::Copy => self.copy(cx),
            EditCommand::Cut => {
                if self.editable() && self.copy(cx) {
                    self.replace(cx, "", EditKind::Other);
                    true
                } else {
                    false
                }
            }
            EditCommand::Paste | EditCommand::PasteAsPlainText => {
                if !self.editable() {
                    return false;
                }
                let Some(text) = cx.clipboard_text() else { return false };
                let one_line: String = text.split(['\n', '\r']).collect::<Vec<_>>().join(" ");
                self.replace(cx, &one_line, EditKind::Other);
                true
            }
            EditCommand::Delete => {
                self.replace(cx, "", EditKind::Other);
                true
            }
            EditCommand::Undo => self.editable() && self.undo(cx, false),
            EditCommand::Redo => self.editable() && self.undo(cx, true),
            _ => false,
        }
    }

    fn can(&self, state: crate::widget::StateView, command: EditCommand) -> bool {
        let st = state.get::<FieldState>();
        let sel = st.is_some_and(|s| s.caret != s.anchor);
        match command {
            EditCommand::Copy => sel && !self.password,
            EditCommand::Cut => sel && !self.password && self.editable(),
            EditCommand::Delete => sel && self.editable(),
            EditCommand::Paste | EditCommand::PasteAsPlainText => self.editable(),
            EditCommand::SelectAll => !self.value.is_empty(),
            EditCommand::Undo => self.editable() && st.is_some_and(|s| !s.history.is_empty()),
            EditCommand::Redo => self.editable() && st.is_some_and(|s| !s.future.is_empty()),
            _ => false,
        }
    }

    fn selection(&self, state: crate::widget::StateView) -> Option<String> {
        let st = state.get::<FieldState>()?;
        let r = st.caret.min(st.anchor)..st.caret.max(st.anchor);
        (!self.password && !r.is_empty() && r.end <= self.value.len()).then(|| self.value[r].to_owned())
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: if self.password {
                SemanticRole::PasswordInput
            } else if self.search {
                SemanticRole::SearchInput
            } else {
                SemanticRole::TextInput
            },
            label: self.placeholder.as_deref().map(str::to_owned),
            value: (!self.password).then(|| self.value.to_string()),
            disabled: !self.enabled,
            actions: vec![SemanticAction::Focus, SemanticAction::SetValue],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        // Inspection and search may describe a password field, but must
        // never publish its contents (including credentials in dialogs).
        (!self.password).then_some(&self.value)
    }
}
