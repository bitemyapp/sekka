//! A line of text typed in place, drawn as the shell's surfaces need it:
//! the palette's query in 20pt with no frame, the lock screen's password
//! as dots with a focus rail, a Desk renamed in the strip. The program
//! keeps the text (`on_change`); the caret and selection live here. Keys
//! the field doesn't use (↑ ↓ Tab Return Escape) go on to its parent.

use std::rc::Rc;
use std::time::Duration;

use aizome_kit::element::{Element, MsgFn, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Ime, Key, NamedKey};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget,
};

type Spec = Rc<dyn Fn(&aizome_kit::Theme) -> TextSpec>;

/// A field showing `value`.
pub struct Field<Msg> {
    w: FieldWidget,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

pub fn field<Msg>(value: &str, spec: impl Fn(&aizome_kit::Theme) -> TextSpec + 'static) -> Field<Msg> {
    Field {
        w: FieldWidget {
            value: value.to_owned(),
            spec: Rc::new(spec),
            placeholder: String::new(),
            secure: false,
            on_change: None,
            on_submit: None,
            on_caps: None,
            height: 24.0,
        },
        _m: std::marker::PhantomData,
    }
}

impl<Msg: Clone + Send + 'static> Field<Msg> {
    pub fn on_change(mut self, f: impl Fn(String) -> Msg + 'static) -> Self {
        self.w.on_change = Some(MsgMap::new(f));
        self
    }

    pub fn on_submit(mut self, msg: Msg) -> Self {
        self.w.on_submit = Some(MsgFn::new(msg));
        self
    }

    pub fn on_caps(mut self, f: impl Fn(bool) -> Msg + 'static) -> Self {
        self.w.on_caps = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> Field<Msg> {
    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.w.placeholder = p.into();
        self
    }

    /// Dots for the characters, and nothing copies out.
    pub fn secure(mut self) -> Self {
        self.w.secure = true;
        self
    }

    pub fn height(mut self, h: f64) -> Self {
        self.w.height = h;
        self
    }
}

impl<Msg> From<Field<Msg>> for Element<Msg> {
    fn from(f: Field<Msg>) -> Element<Msg> {
        Element::new(f.w, Vec::new())
    }
}

struct FieldWidget {
    value: String,
    spec: Spec,
    placeholder: String,
    secure: bool,
    on_change: Option<MsgMap<String>>,
    on_submit: Option<MsgFn>,
    /// Told when Caps Lock turns on or off, as keys show it.
    on_caps: Option<MsgMap<bool>>,
    height: f64,
}

/// What the field keeps between views: the caret and the selection's
/// other end, as byte offsets, and the value they were for.
#[derive(Default)]
struct FieldState {
    caret: usize,
    anchor: usize,
    seen: String,
    preedit: String,
    blink_off: bool,
    caps: Option<bool>,
}

fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].char_indices().next_back().map_or(0, |(j, _)| j)
}

fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(s.len(), |c| i + c.len_utf8())
}

fn word_left(s: &str, i: usize) -> usize {
    let t = &s[..i];
    let trimmed = t.trim_end();
    trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |j| j + 1)
}

fn word_right(s: &str, i: usize) -> usize {
    let t = &s[i..];
    let start = t.len() - t.trim_start().len();
    t[start..].find(|c: char| c.is_whitespace()).map_or(s.len(), |j| i + start + j)
}

impl FieldWidget {
    /// The state, settled against the value (the program may have
    /// changed it).
    fn settle(&self, st: &mut FieldState) {
        if st.seen != self.value {
            // Text the program set: the caret goes to its end.
            st.caret = self.value.len();
            st.anchor = st.caret;
            st.seen = self.value.clone();
        }
        st.caret = st.caret.min(self.value.len());
        st.anchor = st.anchor.min(self.value.len());
        while !self.value.is_char_boundary(st.caret) {
            st.caret -= 1;
        }
        while !self.value.is_char_boundary(st.anchor) {
            st.anchor -= 1;
        }
    }

    fn shown(&self, s: &str) -> String {
        if self.secure { "•".repeat(s.chars().count()) } else { s.to_owned() }
    }

    /// Replace the selection with `with`.
    fn replace(&self, cx: &mut EventCx, with: &str) {
        let (caret, anchor) = {
            let st = cx.state::<FieldState>();
            (st.caret, st.anchor)
        };
        let (a, b) = (caret.min(anchor), caret.max(anchor));
        let mut v = self.value.clone();
        v.replace_range(a..b, with);
        let at = a + with.len();
        let st = cx.state::<FieldState>();
        st.caret = at;
        st.anchor = at;
        st.seen = v.clone();
        self.changed(cx, v);
    }

    fn changed(&self, cx: &mut EventCx, v: String) {
        if let Some(m) = &self.on_change {
            let msg = m.make(v);
            let cell = std::cell::RefCell::new(Some(msg));
            cx.emit(&MsgFn::from_fn(move || cell.borrow_mut().take().expect("once")));
        }
        cx.invalidate();
    }

    fn move_to(&self, cx: &mut EventCx, to: usize, extend: bool) {
        let st = cx.state::<FieldState>();
        st.caret = to;
        if !extend {
            st.anchor = to;
        }
        st.blink_off = false;
        cx.invalidate();
    }

    fn selected(&self, cx: &mut EventCx) -> String {
        let st = cx.state::<FieldState>();
        let (a, b) = (st.caret.min(st.anchor), st.caret.max(st.anchor));
        self.value[a..b].to_owned()
    }
}

impl Widget for FieldWidget {
    fn name(&self) -> &'static str {
        "field"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let st = cx.state::<FieldState>();
        self.settle(st);
        bc.constrain(Size::new(bc.max.width.min(1e5), self.height))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let spec = (self.spec)(t);
        let st = cx.state::<FieldState>();
        let (caret, anchor, preedit, blink_off) = match st {
            Some(s) => (s.caret.min(self.value.len()), s.anchor.min(self.value.len()), s.preedit.clone(), s.blink_off),
            None => (self.value.len(), self.value.len(), String::new(), false),
        };
        let focused = cx.is_focused();
        if self.value.is_empty() && preedit.is_empty() {
            let mut p = spec.clone();
            p.color = t.faint();
            cx.draw_text_in(&self.placeholder, &p, b, aizome_kit::Align::Start);
        }
        let before = self.shown(&self.value[..caret.min(anchor)]);
        let sel = self.shown(&self.value[caret.min(anchor)..caret.max(anchor)]);
        let all = format!("{}{}", self.shown(&self.value), preedit);
        let layout = cx.layout_text(&all, &spec, None);
        let h = layout.size().height;
        let y = ((b.height() - h) / 2.0).round();
        let w_before = cx.layout_text(&before, &spec, None).size().width;
        if !sel.is_empty() {
            let w_sel = cx.layout_text(&sel, &spec, None).size().width;
            let c = if focused { t.selection() } else { t.line2() };
            cx.canvas.fill_rect(Rect::new(w_before, y, w_before + w_sel, y + h), c);
        }
        cx.canvas.draw_text(&layout, Point::new(0.0, y));
        if focused && sel.is_empty() && !blink_off {
            let upto = format!("{}{}", self.shown(&self.value[..caret]), preedit);
            let x = cx.layout_text(&upto, &spec, None).size().width.round();
            cx.canvas.fill_rect(Rect::new(x, y + 1.0, x + 2.0, y + h - 1.0), t.focus());
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.value).str(&self.placeholder).add(&self.secure);
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn cursor(&self) -> Option<aizome_kit::sidestep_ui::Cursor> {
        Some(aizome_kit::sidestep_ui::Cursor::Text)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: if self.secure { SemanticRole::PasswordInput } else { SemanticRole::TextInput },
            label: (!self.placeholder.is_empty()).then(|| self.placeholder.clone()),
            value: (!self.secure).then(|| self.value.clone()),
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        if self.secure { None } else { Some(&self.value) }
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        {
            let st = cx.state::<FieldState>();
            self.settle(st);
        }
        match event {
            Event::PointerDown { .. } => {
                cx.request_focus();
                Status::Handled
            }
            Event::Focus(on) => {
                cx.set_text_input(*on, Some(Rect::new(0.0, 0.0, 2.0, self.height)));
                if *on {
                    // The caret shows at once, then blinks.
                    cx.state::<FieldState>().blink_off = false;
                    cx.request_tick(Duration::from_millis(530));
                }
                cx.invalidate();
                Status::Handled
            }
            Event::Tick => {
                if cx.is_focused() {
                    let st = cx.state::<FieldState>();
                    st.blink_off = !st.blink_off;
                    cx.request_tick(Duration::from_millis(530));
                    cx.invalidate();
                }
                Status::Handled
            }
            Event::Ime(Ime::Commit(s)) => {
                cx.state::<FieldState>().preedit.clear();
                self.replace(cx, s);
                Status::Handled
            }
            Event::Ime(Ime::Preedit { text, .. }) => {
                cx.state::<FieldState>().preedit = text.clone();
                cx.invalidate();
                Status::Handled
            }
            Event::Key(k) if k.pressed => {
                let m = k.modifiers;
                let caps = m.contains(aizome_kit::sidestep_ui::Modifiers::CAPS_LOCK);
                if cx.state::<FieldState>().caps != Some(caps) {
                    cx.state::<FieldState>().caps = Some(caps);
                    if let Some(f) = &self.on_caps {
                        let msg = f.make(caps);
                        let cell = std::cell::RefCell::new(Some(msg));
                        cx.emit(&MsgFn::from_fn(move || cell.borrow_mut().take().expect("once")));
                    }
                }
                let (caret, anchor) = {
                    let st = cx.state::<FieldState>();
                    st.blink_off = false;
                    (st.caret, st.anchor)
                };
                let v = &self.value;
                let shift = m.shift();
                let word = m.alt() || m.control();
                match &k.key {
                    Key::Named(NamedKey::Backspace) => {
                        if caret == anchor {
                            let from = if m.super_key() {
                                0
                            } else if word {
                                word_left(v, caret)
                            } else {
                                prev_boundary(v, caret)
                            };
                            cx.state::<FieldState>().anchor = from;
                        }
                        self.replace(cx, "");
                        Status::Handled
                    }
                    Key::Named(NamedKey::Delete) => {
                        if caret == anchor {
                            let to = if word { word_right(v, caret) } else { next_boundary(v, caret) };
                            cx.state::<FieldState>().anchor = to;
                        }
                        self.replace(cx, "");
                        Status::Handled
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        let to = if m.super_key() {
                            0
                        } else if word {
                            word_left(v, caret)
                        } else if caret != anchor && !shift {
                            caret.min(anchor)
                        } else {
                            prev_boundary(v, caret)
                        };
                        self.move_to(cx, to, shift);
                        Status::Handled
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        let to = if m.super_key() {
                            v.len()
                        } else if word {
                            word_right(v, caret)
                        } else if caret != anchor && !shift {
                            caret.max(anchor)
                        } else {
                            next_boundary(v, caret)
                        };
                        self.move_to(cx, to, shift);
                        Status::Handled
                    }
                    Key::Named(NamedKey::Home) => {
                        self.move_to(cx, 0, shift);
                        Status::Handled
                    }
                    Key::Named(NamedKey::End) => {
                        self.move_to(cx, v.len(), shift);
                        Status::Handled
                    }
                    Key::Named(NamedKey::Enter) => match &self.on_submit {
                        Some(msg) if !m.super_key() => {
                            cx.emit(msg);
                            Status::Handled
                        }
                        _ => Status::Ignored,
                    },
                    Key::Character(c) if m.super_key() && c.eq_ignore_ascii_case("a") => {
                        let st = cx.state::<FieldState>();
                        st.anchor = 0;
                        st.caret = self.value.len();
                        cx.invalidate();
                        Status::Handled
                    }
                    _ => match &k.text {
                        Some(t) if !m.super_key() && !m.control() && !t.chars().any(char::is_control) => {
                            let t = t.clone();
                            self.replace(cx, &t);
                            Status::Handled
                        }
                        _ => Status::Ignored,
                    },
                }
            }
            _ => Status::Ignored,
        }
    }

    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        {
            let st = cx.state::<FieldState>();
            self.settle(st);
        }
        match command {
            EditCommand::Paste | EditCommand::PasteAsPlainText => {
                if let Some(t) = cx.clipboard_text() {
                    let line = t.lines().next().unwrap_or("").to_owned();
                    self.replace(cx, &line);
                }
                true
            }
            EditCommand::Copy | EditCommand::Cut if !self.secure => {
                let s = self.selected(cx);
                if !s.is_empty() {
                    cx.set_clipboard_text(&s);
                    if command == EditCommand::Cut {
                        self.replace(cx, "");
                    }
                }
                true
            }
            EditCommand::SelectAll => {
                let st = cx.state::<FieldState>();
                st.anchor = 0;
                st.caret = self.value.len();
                cx.invalidate();
                true
            }
            EditCommand::Delete => {
                self.replace(cx, "");
                true
            }
            _ => false,
        }
    }

    fn can(&self, _state: aizome_kit::widget::StateView, command: EditCommand) -> bool {
        match command {
            EditCommand::Paste | EditCommand::PasteAsPlainText | EditCommand::SelectAll | EditCommand::Delete => true,
            EditCommand::Copy | EditCommand::Cut => !self.secure,
            _ => false,
        }
    }

    fn selection(&self, state: aizome_kit::widget::StateView) -> Option<String> {
        if self.secure {
            return None;
        }
        let st = state.get::<FieldState>()?;
        let (a, b) = (st.caret.min(st.anchor).min(self.value.len()), st.caret.max(st.anchor).min(self.value.len()));
        (a < b).then(|| self.value[a..b].to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_characters() {
        let s = "find  rep lace";
        assert_eq!(word_left(s, s.len()), 10);
        assert_eq!(word_left(s, 10), 6);
        assert_eq!(word_right(s, 0), 4);
        assert_eq!(word_right(s, 4), 9);
        assert_eq!(prev_boundary("aé", 3), 1);
        assert_eq!(next_boundary("aé", 1), 3);
    }
}
