//! A paragraph of an answer: rich text that wraps at a reading line
//! height, whose links open with a click and whose words select with a
//! drag (a word with a double click, the paragraph with a triple) and
//! copy with ❖C. The kit's `rich_text` draws runs but takes no pointer;
//! Omperor's kit has links and selection of its own, this one not yet.

use std::ops::Range;
use std::rc::Rc;

use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Cursor, PointerButton, TextLayout, Wrap};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, StateView, Status,
    Widget,
};
use aizome_kit::{Element, FontSpec, Role, Span};

/// What a link click sends: its target.
pub type OnLink<Msg> = Rc<dyn Fn(String) -> Msg>;

pub struct Prose<Msg> {
    spans: Vec<Span>,
    links: Vec<(Range<usize>, String)>,
    font: Option<FontSpec>,
    role: Role,
    line_height: f64,
    on_link: Option<OnLink<Msg>>,
}

/// Runs of text; a run with a link opens it (`spans` with their links).
pub fn prose<Msg>(runs: impl IntoIterator<Item = (Span, Option<String>)>) -> Prose<Msg> {
    let mut spans = Vec::new();
    let mut links = Vec::new();
    let mut at = 0;
    for (span, link) in runs {
        let end = at + span.text.len();
        if let Some(l) = link {
            links.push((at..end, l));
        }
        at = end;
        spans.push(span);
    }
    Prose { spans, links, font: None, role: Role::Text, line_height: 0.0, on_link: None }
}

impl<Msg> Prose<Msg> {
    pub fn font(mut self, font: FontSpec) -> Self {
        self.font = Some(font);
        self
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    pub fn line_height(mut self, multiple: f64) -> Self {
        self.line_height = multiple;
        self
    }

    pub fn on_link(mut self, f: OnLink<Msg>) -> Self {
        self.on_link = Some(f);
        self
    }
}

impl<Msg: Clone + Send + 'static> From<Prose<Msg>> for Element<Msg> {
    fn from(p: Prose<Msg>) -> Element<Msg> {
        let text: String = p.spans.iter().map(|s| s.text.as_str()).collect();
        let on_link = p.on_link.map(|f| {
            let f: Rc<dyn Fn(String) -> MsgFn> = Rc::new(move |target: String| MsgFn::new(f(target)));
            f
        });
        let w = ProseWidget {
            spans: p.spans,
            links: p.links,
            text,
            font: p.font,
            role: p.role,
            line_height: p.line_height,
            on_link,
            laid: None,
        };
        use aizome_kit::Layoutable;
        Element::new(w, Vec::new()).width(aizome_kit::layout::Length::Fill)
    }
}

struct ProseWidget {
    spans: Vec<Span>,
    links: Vec<(Range<usize>, String)>,
    text: String,
    font: Option<FontSpec>,
    role: Role,
    line_height: f64,
    on_link: Option<Rc<dyn Fn(String) -> MsgFn>>,
    laid: Option<TextLayout>,
}

#[derive(Default)]
struct ProseState {
    /// The selection, in bytes: where it began and where it is.
    anchor: usize,
    head: usize,
    dragging: bool,
    /// A link pressed and not yet dragged from.
    pressed_link: Option<usize>,
    down_at: Option<Point>,
}

impl ProseState {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

impl ProseWidget {
    fn offset(&self, p: Point) -> usize {
        self.laid.as_ref().map_or(0, |l| l.hit_test(p).offset).min(self.text.len())
    }

    /// The link under `p`, if the press is on its text.
    fn link_at(&self, p: Point) -> Option<usize> {
        let l = self.laid.as_ref()?;
        let hit = l.hit_test(p);
        let c = hit.character;
        // Only on the line's text, not the room after its end.
        let on_text = l.lines().iter().any(|line| p.y >= line.top && p.y < line.top + line.height && p.x <= line.x + line.width);
        if !on_text {
            return None;
        }
        self.links.iter().position(|(r, _)| r.start <= c && c < r.end)
    }

    /// The word around byte `at`.
    fn word(&self, at: usize) -> Range<usize> {
        let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/';
        let mut start = at.min(self.text.len());
        while start > 0 && !self.text.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = start;
        for (i, c) in self.text[..start].char_indices().rev() {
            if !is_word(c) {
                break;
            }
            start = i;
        }
        for (i, c) in self.text[end..].char_indices() {
            if !is_word(c) {
                end += i;
                return start..end;
            }
        }
        end = self.text.len();
        start..end
    }
}

impl Widget for ProseWidget {
    fn name(&self) -> &'static str {
        "prose"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let theme = cx.theme().clone();
        let font = self.font.unwrap_or(FontSpec::ui(theme.metrics.body));
        let mut spec = TextSpec::new(font, theme.role(self.role));
        spec.wrap = Wrap::Words;
        spec.line_height = self.line_height;
        let width = bc.max.width.is_finite().then_some(bc.max.width.max(1.0));
        let laid = cx.text_cache().layout_spans(&theme, &self.spans, &spec, width);
        let s = laid.size();
        self.laid = Some(laid);
        let st = cx.state::<ProseState>();
        let n = self.text.len();
        st.anchor = st.anchor.min(n);
        st.head = st.head.min(n);
        bc.constrain(Size::new(width.unwrap_or(s.width).ceil(), s.height.ceil()))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let Some(l) = &self.laid else { return };
        if let Some(st) = cx.state::<ProseState>() {
            let r = st.range();
            if !r.is_empty() {
                let t = cx.theme();
                let fill = if cx.is_focused() { t.selection().with_alpha(0.38) } else { t.line2() };
                for s in l.selection_rects(r) {
                    cx.canvas.fill_rect(s, fill);
                }
            }
        }
        cx.canvas.draw_text(l, Point::ZERO);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.text);
        for s in &self.spans {
            h.add(&(s.role, s.weight.map(f32::to_bits), s.italic, s.mono, s.underline, s.strikethrough));
            h.add(&s.size.map(f64::to_bits));
            h.add(&s.color.map(|c| [c.r, c.g, c.b, c.a].map(f32::to_bits)));
        }
        h.add(&(self.role, self.line_height.to_bits(), self.font.map(|f| (f.size.to_bits(), f.weight.to_bits()))));
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn focusable(&self) -> Focusable {
        Focusable::Pointer
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
                let at = self.offset(*pos);
                let link = (*clicks == 1 && !mods.shift()).then(|| self.link_at(*pos)).flatten();
                let word = (*clicks == 2).then(|| self.word(at));
                let all = *clicks >= 3;
                let n = self.text.len();
                let st = cx.state::<ProseState>();
                st.pressed_link = link;
                st.down_at = Some(*pos);
                st.dragging = true;
                if all {
                    (st.anchor, st.head) = (0, n);
                } else if let Some(w) = word {
                    (st.anchor, st.head) = (w.start, w.end);
                } else if mods.shift() {
                    st.head = at;
                } else {
                    (st.anchor, st.head) = (at, at);
                }
                cx.invalidate();
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let over_link = self.link_at(*pos).is_some() && self.on_link.is_some();
                cx.set_cursor(if over_link { Cursor::Pointer } else { Cursor::Text });
                let at = self.offset(*pos);
                let st = cx.state::<ProseState>();
                if !st.dragging {
                    return Status::Ignored;
                }
                if st.down_at.is_some_and(|d| (d.x - pos.x).abs() + (d.y - pos.y).abs() > 4.0) {
                    st.pressed_link = None;
                }
                if st.pressed_link.is_none() && st.head != at {
                    st.head = at;
                    cx.invalidate();
                }
                Status::Handled
            }
            Event::PointerUp { button: PointerButton::Left, .. } => {
                cx.release_pointer();
                let st = cx.state::<ProseState>();
                st.dragging = false;
                if let Some(i) = st.pressed_link.take() {
                    let at = st.anchor;
                    st.head = at;
                    if let (Some(f), Some((_, target))) = (&self.on_link, self.links.get(i)) {
                        let m = f(target.clone());
                        cx.emit(&m);
                    }
                }
                Status::Handled
            }
            Event::Focus(false) => {
                let st = cx.state::<ProseState>();
                st.head = st.anchor;
                cx.invalidate();
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        match command {
            EditCommand::Copy => {
                let r = cx.state::<ProseState>().range();
                if r.is_empty() {
                    return false;
                }
                let s = self.text[r].to_owned();
                cx.set_clipboard_text(&s);
                true
            }
            EditCommand::SelectAll => {
                let n = self.text.len();
                let st = cx.state::<ProseState>();
                (st.anchor, st.head) = (0, n);
                cx.invalidate();
                true
            }
            _ => false,
        }
    }

    fn can(&self, state: StateView, command: EditCommand) -> bool {
        match command {
            EditCommand::Copy => state.get::<ProseState>().is_some_and(|s| !s.range().is_empty()),
            EditCommand::SelectAll => true,
            _ => false,
        }
    }

    fn selection(&self, state: StateView) -> Option<String> {
        let r = state.get::<ProseState>()?.range();
        (!r.is_empty()).then(|| self.text[r].to_owned())
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Label, label: Some(self.text.clone()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }
}

/// The rectangle a wrapped layout's line takes, to find lines by.
#[allow(dead_code)]
fn line_rect(l: &TextLayout, i: usize) -> Option<Rect> {
    l.lines().get(i).map(|m| Rect::new(m.x, m.top, m.x + m.width, m.top + m.height))
}
