//! A line of text typed in place, drawn as the shell's surfaces need it:
//! the palette's query in 20pt with no frame, the lock screen's password
//! as dots with a focus rail, a Desk renamed in the strip. The program
//! keeps the text (`on_change`); the caret and selection live here. Keys
//! the field doesn't use (↑ ↓ Tab Return Escape) go on to its parent.
//!
//! A password field is a type of its own, [`SecretField`]: its [`Mode`]
//! decides what it draws, what leaves it and what input methods are told,
//! and it keeps only a count of what they compose ([`Masked`]). Both come
//! from [`aizome_kit::composing`], shared with the kit's `text_field`.

use std::rc::Rc;
use std::time::Duration;

use aizome_kit::element::{Element, MsgFn, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Ime, Key, NamedKey};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, Semantics, Status, Widget,
};

use aizome_kit::composing::{Composes, Kept as _, Place};
pub use aizome_kit::composing::{Masked, Mode, Plain, Preedit, Secret};

type Spec = Rc<dyn Fn(&aizome_kit::Theme) -> TextSpec>;

const BLINK: Duration = Duration::from_millis(530);
/// The timer that brings a field that changed mode while focused its
/// arrival.
const ARRIVE: u32 = 0;

/// A field showing `value`, plain unless [`secure`](Field::secure).
pub struct Field<Msg, M = Plain> {
    w: FieldWidget<M>,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

/// A password field ([`Field::secure`]).
pub type SecretField<Msg> = Field<Msg, Secret>;

pub fn field<Msg>(value: &str, spec: impl Fn(&aizome_kit::Theme) -> TextSpec + 'static) -> Field<Msg> {
    Field {
        w: FieldWidget {
            value: value.to_owned(),
            spec: Rc::new(spec),
            placeholder: String::new(),
            on_change: None,
            on_submit: None,
            on_caps: None,
            on_blur: None,
            height: 24.0,
            _mode: std::marker::PhantomData,
        },
        _m: std::marker::PhantomData,
    }
}

impl<Msg: Clone + Send + 'static, M: Mode> Field<Msg, M> {
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

    /// Sent when what's typed stops reaching the field: its window lost
    /// the keyboard while the field had the focus in it, or the focus left
    /// the field (a click beside it).
    pub fn on_blur(mut self, msg: Msg) -> Self {
        self.w.on_blur = Some(MsgFn::new(msg));
        self
    }
}

impl<Msg, M: Mode> Field<Msg, M> {
    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.w.placeholder = p.into();
        self
    }

    pub fn height(mut self, h: f64) -> Self {
        self.w.height = h;
        self
    }
}

impl<Msg> Field<Msg> {
    /// A password field: dots for the characters, nothing copies out,
    /// input methods are told it's a password, and of what they compose it
    /// keeps only how many characters ([`Masked`]), drawn as dots.
    ///
    /// It's a field of another type, so it can't be kept or drawn as a
    /// plain one:
    ///
    /// ```compile_fail,E0308
    /// use aizome_face::widgets::field::{field, Field};
    /// let spec = |t: &aizome_kit::Theme| aizome_face::look::ui(15.0, 400.0, t.text());
    /// let f: Field<()> = field("", spec).secure();
    /// ```
    ///
    /// ```
    /// use aizome_face::widgets::field::{field, SecretField};
    /// use aizome_kit::element::Element;
    /// let spec = |t: &aizome_kit::Theme| aizome_face::look::ui(15.0, 400.0, t.text());
    /// let f: SecretField<()> = field("", spec).secure().placeholder("Password");
    /// let _: Element<()> = f.into();
    /// ```
    pub fn secure(self) -> SecretField<Msg> {
        Field { w: self.w.into_secret(), _m: std::marker::PhantomData }
    }

    /// A password field if `secret`, else this one: for a prompt whose
    /// echo is decided as it runs (PAM's). Either way it's the same
    /// element to the kit, so it keeps the focus when the prompt changes.
    pub fn secure_when(self, secret: bool) -> MaybeSecret<Msg> {
        if secret { MaybeSecret::Secret(self.secure()) } else { MaybeSecret::Plain(self) }
    }
}

impl<Msg> From<Field<Msg>> for Element<Msg> {
    fn from(f: Field<Msg>) -> Element<Msg> {
        Element::new(FieldNode::Plain(f.w), Vec::new())
    }
}

impl<Msg> From<SecretField<Msg>> for Element<Msg> {
    fn from(f: SecretField<Msg>) -> Element<Msg> {
        Element::new(FieldNode::Secret(f.w), Vec::new())
    }
}

/// A field that's a password field or not as a prompt decides
/// ([`Field::secure_when`]).
pub enum MaybeSecret<Msg> {
    Plain(Field<Msg>),
    Secret(SecretField<Msg>),
}

impl<Msg> From<MaybeSecret<Msg>> for Element<Msg> {
    fn from(f: MaybeSecret<Msg>) -> Element<Msg> {
        match f {
            MaybeSecret::Plain(f) => f.into(),
            MaybeSecret::Secret(f) => f.into(),
        }
    }
}

struct FieldWidget<M> {
    value: String,
    spec: Spec,
    placeholder: String,
    on_change: Option<MsgMap<String>>,
    on_submit: Option<MsgFn>,
    /// Told when Caps Lock turns on or off, as keys show it.
    on_caps: Option<MsgMap<bool>>,
    /// Told when the field loses the focus, or its window the keyboard.
    on_blur: Option<MsgFn>,
    height: f64,
    _mode: std::marker::PhantomData<fn() -> M>,
}

/// What the field keeps between views, whatever its mode: the caret and
/// the selection's other end, as byte offsets, the value they were for,
/// and Caps Lock as keys showed it. What goes with the focus (the caret's
/// blink, what an input method is composing) is its mode's
/// [`Focus`](aizome_kit::composing::Focus), which the kit keeps beside it.
#[derive(Default)]
struct FieldState {
    caret: usize,
    anchor: usize,
    seen: String,
    caps: Option<bool>,
}

/// Whether the value was the field's last view of it, or text the program
/// set.
enum Seen {
    Same,
    Set,
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

impl FieldWidget<Plain> {
    /// The same field as a password field. There's no way back.
    fn into_secret(self) -> FieldWidget<Secret> {
        let FieldWidget { value, spec, placeholder, on_change, on_submit, on_blur, on_caps, height, _mode } = self;
        FieldWidget {
            value,
            spec,
            placeholder,
            on_change,
            on_submit,
            on_blur,
            on_caps,
            height,
            _mode: std::marker::PhantomData,
        }
    }
}

/// The widget the kit keeps for a field. The kit tells elements apart by
/// their widget's type, so it's one type for both modes: a field that
/// stops echoing (a PAM prompt for a code, then the password) is the same
/// element, and keeps the focus, caret and Caps Lock state. Each mode keeps
/// its own composition, and only
/// [`From<SecretField>`](Element) makes the secret one.
enum FieldNode {
    Plain(FieldWidget<Plain>),
    Secret(FieldWidget<Secret>),
}

impl FieldNode {
    fn get(&self) -> &dyn Widget {
        match self {
            FieldNode::Plain(w) => w,
            FieldNode::Secret(w) => w,
        }
    }

    fn get_mut(&mut self) -> &mut dyn Widget {
        match self {
            FieldNode::Plain(w) => w,
            FieldNode::Secret(w) => w,
        }
    }
}

/// Every method [`FieldWidget`] has of its own, passed to the mode's.
impl Widget for FieldNode {
    fn name(&self) -> &'static str {
        self.get().name()
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        self.get_mut().layout(cx, bc)
    }

    fn paint(&self, cx: &mut PaintCx) {
        self.get().paint(cx)
    }

    fn hash(&self, h: &mut PaintHasher) {
        self.get().hash(h)
    }

    fn hit(&self) -> bool {
        self.get().hit()
    }

    fn focusable(&self) -> Focusable {
        self.get().focusable()
    }

    fn focus_ring(&self) -> FocusRing {
        self.get().focus_ring()
    }

    fn cursor(&self) -> Option<aizome_kit::sidestep_ui::Cursor> {
        self.get().cursor()
    }

    fn semantics(&self) -> Option<Semantics> {
        self.get().semantics()
    }

    fn text(&self) -> Option<&str> {
        self.get().text()
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        self.get_mut().event(cx, event)
    }

    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        self.get_mut().command(cx, command)
    }

    fn can(&self, state: aizome_kit::widget::StateView, command: EditCommand) -> bool {
        self.get().can(state, command)
    }

    fn selection(&self, state: aizome_kit::widget::StateView) -> Option<String> {
        self.get().selection(state)
    }
}

impl<M: Mode> Composes for FieldWidget<M> {
    type Mode = M;
}

impl<M: Mode> FieldWidget<M>
where
    Self: Composes<Mode = M>,
{
    /// The state, settled against the value (the program may have
    /// changed it): the caret goes to the end of text the program set.
    #[must_use = "what was being composed into text the program replaced is dropped"]
    fn settle(&self, st: &mut FieldState) -> Seen {
        let seen = if st.seen != self.value {
            st.caret = self.value.len();
            st.anchor = st.caret;
            st.seen = self.value.clone();
            Seen::Set
        } else {
            Seen::Same
        };
        st.caret = st.caret.min(self.value.len());
        st.anchor = st.anchor.min(self.value.len());
        while !self.value.is_char_boundary(st.caret) {
            st.caret -= 1;
        }
        while !self.value.is_char_boundary(st.anchor) {
            st.anchor -= 1;
        }
        seen
    }

    /// The field has the focus: input methods are told what it takes, and
    /// the caret shows, then blinks. What it was composing in its other
    /// mode (a prompt that echoed, then one that doesn't) is dropped.
    fn arrive(&self, cx: &mut EventCx) {
        cx.composer(self).arrive(Some(Rect::new(0.0, 0.0, 2.0, self.height)));
        cx.request_tick(BLINK);
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
        cx.composition(self).wake();
        cx.invalidate();
    }

    fn selected(&self, cx: &mut EventCx) -> Option<String> {
        let st = cx.state::<FieldState>();
        let (a, b) = (st.caret.min(st.anchor), st.caret.max(st.anchor));
        M::exposed(&self.value[a..b]).map(str::to_owned)
    }

    /// An input method's event: what it composes shows as the mode keeps
    /// it; what it commits is typed.
    fn ime(&self, cx: &mut EventCx, ime: &Ime) -> Status {
        let Some(composing) = cx.composer(self).hear() else { return Status::Handled };
        match ime {
            Ime::Commit(s) => {
                composing.commit();
                self.replace(cx, s);
            }
            Ime::Preedit { text, selection } => {
                composing.compose(text, selection.as_ref().map(|r| r.end));
                cx.invalidate();
            }
        }
        Status::Handled
    }
}

impl<M: Mode> Widget for FieldWidget<M>
where
    Self: Composes<Mode = M>,
{
    fn name(&self) -> &'static str {
        "field"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let st = cx.state::<FieldState>();
        if let Seen::Set = self.settle(st) {
            cx.composition(self).value_changed();
        }
        // Focused in a mode that hasn't arrived (the field just changed
        // mode): it arrives at once, not at the caret's next blink, so few
        // of the input method's events from the old mode reach it.
        if cx.is_focused()
            && let Place::Away = cx.composition(self).place()
        {
            cx.set_timer(ARRIVE, Duration::ZERO);
        }
        bc.constrain(Size::new(bc.max.width.min(1e5), self.height))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let spec = (self.spec)(t);
        let st = cx.state::<FieldState>();
        let (caret, anchor) = match st {
            Some(s) => (s.caret.min(self.value.len()), s.anchor.min(self.value.len())),
            None => (self.value.len(), self.value.len()),
        };
        // What an input method is composing, as the mode keeps it: a
        // password's as dots.
        let focus = cx.composition(self);
        let blink_off = focus.is_some_and(|f| f.blink_off());
        let composing = focus.and_then(|f| f.composing()).map(|k| k.drawn());
        let composing = composing.as_deref().unwrap_or("");
        let focused = cx.is_focused();
        if self.value.is_empty() && composing.is_empty() {
            let mut p = spec.clone();
            p.color = t.faint();
            cx.draw_text_in(&self.placeholder, &p, b, aizome_kit::Align::Start);
        }
        let before = M::shown(&self.value[..caret.min(anchor)]);
        let sel = M::shown(&self.value[caret.min(anchor)..caret.max(anchor)]);
        let all = format!("{}{}", M::shown(&self.value), composing);
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
            let upto = format!("{}{}", M::shown(&self.value[..caret]), composing);
            let x = cx.layout_text(&upto, &spec, None).size().width.round();
            cx.canvas.fill_rect(Rect::new(x, y + 1.0, x + 2.0, y + h - 1.0), t.focus());
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.value).str(&self.placeholder).add(&M::PURPOSE);
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
            role: M::ROLE,
            label: (!self.placeholder.is_empty()).then(|| self.placeholder.clone()),
            value: M::exposed(&self.value).map(str::to_owned),
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        M::exposed(&self.value)
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        {
            let st = cx.state::<FieldState>();
            if let Seen::Set = self.settle(st) {
                cx.composition(self).value_changed();
            }
        }
        match event {
            Event::Focus(true) => {
                self.arrive(cx);
                cx.invalidate();
                return Status::Handled;
            }
            Event::Focus(false) => {
                cx.composer(self).went();
                if let Some(msg) = &self.on_blur {
                    cx.emit(msg);
                }
                cx.invalidate();
                return Status::Handled;
            }
            _ => {}
        }
        // Focused without having heard so (the field changed mode while
        // focused): input methods are told what it takes now.
        if cx.is_focused()
            && let Place::Away = cx.composition(self).place()
        {
            self.arrive(cx);
        }
        if let Event::Ime(ime) = event {
            return self.ime(cx, ime);
        }
        // A composition the field dropped (the program set the text, or it
        // changed mode): the input method starts over.
        cx.composer(self).catch_up();
        match event {
            Event::PointerDown { .. } => {
                cx.request_focus();
                Status::Handled
            }
            Event::WindowFocus(false) => match &self.on_blur {
                Some(msg) => {
                    cx.emit(msg);
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::Tick => {
                if cx.is_focused() {
                    cx.composition(self).blink();
                    cx.request_tick(BLINK);
                    cx.invalidate();
                }
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
                cx.composition(self).wake();
                let (caret, anchor) = {
                    let st = cx.state::<FieldState>();
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
            if let Seen::Set = self.settle(st) {
                cx.composition(self).value_changed();
            }
        }
        match command {
            EditCommand::Paste | EditCommand::PasteAsPlainText => {
                if let Some(t) = cx.clipboard_text() {
                    let line = t.lines().next().unwrap_or("").to_owned();
                    self.replace(cx, &line);
                }
                true
            }
            EditCommand::Copy | EditCommand::Cut if M::exposed(&self.value).is_some() => {
                if let Some(s) = self.selected(cx)
                    && !s.is_empty()
                {
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
            EditCommand::Copy | EditCommand::Cut => M::exposed(&self.value).is_some(),
            _ => false,
        }
    }

    fn selection(&self, state: aizome_kit::widget::StateView) -> Option<String> {
        let st = state.get::<FieldState>()?;
        let (a, b) = (st.caret.min(st.anchor).min(self.value.len()), st.caret.max(st.anchor).min(self.value.len()));
        (a < b).then(|| M::exposed(&self.value[a..b]).map(str::to_owned)).flatten()
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
