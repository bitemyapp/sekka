//! Selecting read-only text: `text(..).selectable()` and
//! `rich_text(..).selectable()`.
//!
//! A press places the selection's anchor and a drag extends it; a double
//! click selects a word and a triple click all of the text. ❖C copies what
//! is selected and ❖A selects all of it (the Edit menu's roles reach the
//! focused text); the selection goes when the text loses the focus. In
//! rich text, a click on a span with a link (a press and release without
//! a drag) sends the text's `on_link` message instead.

use std::ops::Range;
use std::sync::Arc;

use sidestep_ui::{Cursor, PointerButton, TextLayout};

use crate::editing::word_at;
use crate::element::MsgMap;
use crate::layout::{Point, Size};
use crate::widget::{EditCommand, Event, EventCx, PaintCx, StateView, Status};

/// How far a press moves before it's a drag rather than a click on a link.
const CLICK_SLOP: f64 = 4.0;

/// What selectable text keeps between views.
#[derive(Debug, Default)]
pub(crate) struct TextSelection {
    anchor: usize,
    caret: usize,
    /// A press is extending the selection.
    selecting: bool,
    /// Where the press began, to tell a click from a drag.
    pressed_at: Option<Point>,
}

impl TextSelection {
    fn range(&self, len: usize) -> Range<usize> {
        self.anchor.min(self.caret).min(len)..self.anchor.max(self.caret).min(len)
    }
}

/// A selectable text's laid-out lines and links, as its widget holds them.
pub(crate) struct Selectable<'a> {
    pub laid: Option<&'a TextLayout>,
    /// Byte ranges of the text that are links, and where each leads.
    pub links: &'a [(Range<usize>, Arc<str>)],
    pub on_link: Option<&'a MsgMap<String>>,
    /// The bytes of the U+FFFCs that stand for inlines, and what each
    /// copies as.
    pub inlines: &'a [(usize, Arc<str>)],
}

/// Where text laid out as `l` is drawn in a widget of `size`: down the
/// middle of any height beyond its own, as the text widgets draw it.
pub(crate) fn origin(l: &TextLayout, size: Size) -> Point {
    Point::new(0.0, ((size.height - l.size().height) / 2.0).max(0.0).floor())
}

impl Selectable<'_> {
    fn text(&self) -> &str {
        self.laid.map_or("", TextLayout::text)
    }

    fn offset(&self, pos: Point, size: Size) -> usize {
        let Some(l) = self.laid else { return 0 };
        let o = origin(l, size);
        l.hit_test(Point::new(pos.x - o.x, pos.y - o.y)).offset.min(l.text().len())
    }

    fn link_at(&self, pos: Point, size: Size) -> Option<&Arc<str>> {
        let l = self.laid?;
        if self.links.is_empty() {
            return None;
        }
        let o = origin(l, size);
        let hit = l.hit_test(Point::new(pos.x - o.x, pos.y - o.y));
        let (range, target) = self.links.iter().find(|(r, _)| r.contains(&hit.character))?;
        // Only over the link's own glyphs, not the empty end of its line.
        let over = l.selection_rects(range.clone()).iter().any(|r| r.contains(Point::new(pos.x - o.x, pos.y - o.y)));
        over.then_some(target)
    }

    pub fn event(&self, cx: &mut EventCx, event: &Event) -> Status {
        let size = cx.size();
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, clicks, mods } => {
                let at = self.offset(*pos, size);
                let text = self.text().to_owned();
                let st = cx.state::<TextSelection>();
                st.pressed_at = Some(*pos);
                match clicks {
                    2 => {
                        let w = word_at(&text, at);
                        st.anchor = w.start;
                        st.caret = w.end;
                        st.selecting = false;
                    }
                    n if *n >= 3 => {
                        st.anchor = 0;
                        st.caret = text.len();
                        st.selecting = false;
                    }
                    _ => {
                        st.caret = at;
                        if !mods.shift() {
                            st.anchor = at;
                        }
                        st.selecting = true;
                    }
                }
                cx.request_focus();
                cx.invalidate();
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                if cx.is_pressed() {
                    let at = self.offset(*pos, size);
                    let st = cx.state::<TextSelection>();
                    let dragged = st.pressed_at.is_some_and(|from| (*pos - from).hypot() >= CLICK_SLOP);
                    if st.selecting && (dragged || st.pressed_at.is_none()) && st.caret != at {
                        st.caret = at;
                        st.pressed_at = None;
                        cx.invalidate();
                    }
                    cx.set_cursor(Cursor::Text);
                } else {
                    let over_link = self.on_link.is_some() && self.link_at(*pos, size).is_some();
                    cx.set_cursor(if over_link { Cursor::Pointer } else { Cursor::Text });
                }
                Status::Handled
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                let st = cx.state::<TextSelection>();
                st.selecting = false;
                let clicked = st.pressed_at.take().is_some_and(|from| (*pos - from).hypot() < CLICK_SLOP);
                if clicked
                    && let Some(on_link) = self.on_link
                    && let Some(target) = self.link_at(*pos, size)
                {
                    let st = cx.state::<TextSelection>();
                    st.anchor = st.caret;
                    cx.invalidate();
                    let msg = on_link.make(target.to_string());
                    cx.emit_any(msg);
                }
                Status::Handled
            }
            Event::Focus(false) => {
                let st = cx.state::<TextSelection>();
                if st.anchor != st.caret {
                    st.anchor = st.caret;
                    cx.invalidate();
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    pub fn command(&self, cx: &mut EventCx, command: EditCommand) -> bool {
        let text = self.text();
        match command {
            EditCommand::Copy => {
                let r = cx.state::<TextSelection>().range(text.len());
                let Some(selected) = self.copied(r) else { return false };
                cx.set_clipboard_text(&selected);
                true
            }
            EditCommand::SelectAll => {
                let len = text.len();
                let st = cx.state::<TextSelection>();
                st.anchor = 0;
                st.caret = len;
                cx.invalidate();
                true
            }
            _ => false,
        }
    }

    pub fn can(&self, state: StateView, command: EditCommand) -> bool {
        let len = self.text().len();
        match command {
            EditCommand::Copy => state.get::<TextSelection>().is_some_and(|s| !s.range(len).is_empty()),
            EditCommand::SelectAll => len > 0,
            _ => false,
        }
    }

    pub fn selection(&self, state: StateView) -> Option<String> {
        let r = state.get::<TextSelection>()?.range(self.text().len());
        self.copied(r)
    }

    /// What copying bytes `r` gives: their text, with inlines as theirs.
    fn copied(&self, r: Range<usize>) -> Option<String> {
        let text = self.text();
        if r.is_empty() || !text.is_char_boundary(r.start) || !text.is_char_boundary(r.end) {
            return None;
        }
        let mut copied = String::with_capacity(r.len());
        let mut from = r.start;
        for (at, inline) in self.inlines.iter().filter(|(at, _)| r.contains(at)) {
            copied.push_str(&text[from..*at]);
            copied.push_str(inline);
            from = at + '\u{FFFC}'.len_utf8();
        }
        copied.push_str(&text[from..r.end]);
        Some(copied)
    }

    /// Fill the selection behind the text, while the text has the focus.
    pub fn paint_selection(&self, cx: &mut PaintCx) {
        let Some(l) = self.laid else { return };
        if !cx.is_focused() {
            return;
        }
        let Some(st) = cx.state::<TextSelection>() else { return };
        let r = st.range(l.text().len());
        if r.is_empty() {
            return;
        }
        let fill = if cx.window_active() { cx.theme().selection() } else { cx.theme().line2() };
        let o = origin(l, cx.size()).to_vec2();
        for rect in l.selection_rects(r) {
            cx.canvas.fill_rect(rect + o, fill);
        }
    }
}

/// The byte ranges of `spans` that link somewhere.
pub(crate) fn link_ranges(spans: &[crate::text::Span]) -> Vec<(Range<usize>, Arc<str>)> {
    let mut at = 0;
    let mut links = Vec::new();
    for s in spans {
        let end = at + s.text.len();
        if let Some(target) = &s.link {
            links.push((at..end, target.clone()));
        }
        at = end;
    }
    links
}
