//! `text_field`: one line of text to type in.
//!
//! The program owns the text: the field shows `value`, and every edit
//! comes back as the new text through `on_change`. What only the field
//! cares about — the caret, the selection, what an input method is
//! composing, how far the text is scrolled, its undo history — lives in
//! its state, under its key.
//!
//! A password field is a type of its own, [`PasswordField`]: its
//! [`Mode`] decides what it draws, what leaves it and what input methods
//! are told, and it keeps only a count of what they compose
//! ([`Masked`](crate::composing::Masked)).
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

use crate::composing::{Composes, Kept as _, Mode, Place, Plain, Secret};
use crate::draw;
use crate::editing::{floor, next_char, next_word, prev_char, prev_word, word_at};
use crate::element::{Common, Element, Layoutable, MsgFn, MsgMap, RawElement};
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
/// The timer that brings a field that changed mode while focused its
/// arrival.
const ARRIVE: u32 = 0;
/// Typing within this of the last keystroke undoes with it.
const GROUP: Duration = Duration::from_millis(1000);

/// One line of text: `text_field(&self.name).placeholder("Name").on_change(Msg::Name)`.
pub struct TextField<Msg, M = Plain> {
    pub(crate) common: Common,
    value: Arc<str>,
    placeholder: Option<Arc<str>>,
    on_change: Option<MsgMap<String>>,
    on_submit: Option<MsgFn>,
    on_blur: Option<MsgFn>,
    search: bool,
    error: Option<Arc<str>>,
    enabled: bool,
    mono: bool,
    select_on_focus: Option<Range<usize>>,
    _m: PhantomData<fn() -> (Msg, M)>,
}

/// A password field ([`TextField::password`]).
pub type PasswordField<Msg> = TextField<Msg, Secret>;

impl<Msg, M: Mode> Layoutable for TextField<Msg, M> {
    fn common_mut(&mut self) -> &mut Common {
        &mut self.common
    }
}

pub fn text_field<Msg>(value: &str) -> TextField<Msg> {
    TextField {
        common: Common::sized(Length::Fill, Length::Shrink),
        value: value.into(),
        placeholder: None,
        on_change: None,
        on_submit: None,
        on_blur: None,
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

impl<Msg: Clone + Send + 'static, M: Mode> TextField<Msg, M> {
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
    /// A password field: dots for the characters, nothing copies or drags
    /// out, input methods are told it's a password, and of what they
    /// compose it keeps only how many characters, drawn as dots.
    ///
    /// It's a field of another type, so it can't be kept or drawn as a
    /// plain one:
    ///
    /// ```compile_fail,E0308
    /// use aizome_kit::widgets::text_field::{text_field, TextField};
    /// let f: TextField<()> = text_field("").password();
    /// ```
    ///
    /// ```
    /// use aizome_kit::element::Element;
    /// use aizome_kit::widgets::text_field::{text_field, PasswordField};
    /// let f: PasswordField<()> = text_field("").password().placeholder("Password");
    /// let _: Element<()> = f.into();
    /// ```
    pub fn password(self) -> PasswordField<Msg> {
        let TextField {
            common,
            value,
            placeholder,
            on_change,
            on_submit,
            on_blur,
            search,
            error,
            enabled,
            mono,
            select_on_focus,
            _m,
        } = self;
        TextField {
            common,
            value,
            placeholder,
            on_change,
            on_submit,
            on_blur,
            search,
            error,
            enabled,
            mono,
            select_on_focus,
            _m: PhantomData,
        }
    }

    /// A password field if `secret`, else this one: for a field shown or
    /// hidden as the person asks ("Show password"), or as the network
    /// says. Either way it's the same element to the kit, so it keeps its
    /// caret and undo history when it changes.
    pub fn password_when(self, secret: bool) -> MaybePassword<Msg> {
        if secret { MaybePassword::Password(self.password()) } else { MaybePassword::Plain(self) }
    }
}

impl<Msg, M: Mode> TextField<Msg, M> {
    pub fn placeholder(mut self, text: impl Into<Arc<str>>) -> Self {
        self.placeholder = Some(text.into());
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

    fn widget(self) -> (FieldWidget<M>, Common) {
        let w = FieldWidget {
            value: self.value,
            placeholder: self.placeholder,
            on_change: self.on_change,
            on_submit: self.on_submit,
            on_blur: self.on_blur,
            search: self.search,
            error: self.error,
            enabled: self.enabled,
            mono: self.mono,
            select_on_focus: self.select_on_focus,
            laid: None,
            shown: String::new(),
            caret: 0,
            sel: 0..0,
            preedit: 0..0,
            scroll: 0.0,
            _mode: PhantomData,
        };
        (w, self.common)
    }
}

impl<Msg> From<TextField<Msg>> for Element<Msg> {
    fn from(f: TextField<Msg>) -> Element<Msg> {
        let (w, common) = f.widget();
        Element::from_raw(RawElement::new(FieldNode::Plain(w), common, Vec::new()))
    }
}

impl<Msg> From<PasswordField<Msg>> for Element<Msg> {
    fn from(f: PasswordField<Msg>) -> Element<Msg> {
        let (w, common) = f.widget();
        Element::from_raw(RawElement::new(FieldNode::Secret(w), common, Vec::new()))
    }
}

/// A text field that's a password field or not as the program decides
/// ([`TextField::password_when`]).
pub enum MaybePassword<Msg> {
    Plain(TextField<Msg>),
    Password(PasswordField<Msg>),
}

impl<Msg> From<MaybePassword<Msg>> for Element<Msg> {
    fn from(f: MaybePassword<Msg>) -> Element<Msg> {
        match f {
            MaybePassword::Plain(f) => f.into(),
            MaybePassword::Password(f) => f.into(),
        }
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

/// What a field remembers between views, whatever its mode. What goes
/// with the focus (the caret's blink, what an input method is composing)
/// is its mode's [`Focus`](crate::composing::Focus), kept beside it.
#[derive(Default)]
pub(crate) struct FieldState {
    caret: usize,
    anchor: usize,
    scroll: f64,
    /// The text as the field last saw it or set it: another is text the
    /// program set.
    seen: String,
    selecting: bool,
    /// A press inside the selection, where and at which offset: moved far
    /// enough, the selected text is dragged out; released, the caret goes
    /// there.
    drag_text: Option<(Point, usize)>,
    history: Vec<(String, usize, usize)>,
    future: Vec<(String, usize, usize)>,
    last_edit: Option<(Instant, EditKind)>,
    /// The field has had the focus (its starting selection is used up).
    primed: bool,
}

/// The widget the kit keeps for a text field. The kit tells elements apart
/// by their widget's type, so it's one type for both modes: a field that
/// turns into a password field ("Show password" unticked) is the same
/// element, and keeps its caret, undo history and focus. Only
/// [`From<PasswordField>`](Element) makes the secret one.
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

    fn hover_paints(&self) -> bool {
        self.get().hover_paints()
    }

    fn cursor(&self) -> Option<Cursor> {
        self.get().cursor()
    }

    fn focusable(&self) -> Focusable {
        self.get().focusable()
    }

    fn focus_ring(&self) -> FocusRing {
        self.get().focus_ring()
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        self.get_mut().event(cx, event)
    }

    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        self.get_mut().command(cx, command)
    }

    fn can(&self, state: crate::widget::StateView, command: EditCommand) -> bool {
        self.get().can(state, command)
    }

    fn selection(&self, state: crate::widget::StateView) -> Option<String> {
        self.get().selection(state)
    }

    fn semantics(&self) -> Option<Semantics> {
        self.get().semantics()
    }

    fn text(&self) -> Option<&str> {
        self.get().text()
    }
}

struct FieldWidget<M> {
    value: Arc<str>,
    placeholder: Option<Arc<str>>,
    on_change: Option<MsgMap<String>>,
    on_submit: Option<MsgFn>,
    on_blur: Option<MsgFn>,
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
    _mode: PhantomData<fn() -> M>,
}

impl<M: Mode> Composes for FieldWidget<M> {
    type Mode = M;
}

impl<M: Mode> FieldWidget<M>
where
    Self: Composes<Mode = M>,
{
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
        let base = M::shown_len(&self.value[..value_offset.min(self.value.len())]);
        if !preedit.is_empty() && value_offset > caret { base + preedit.len() } else { base }
    }

    /// An offset in the text shown as a value offset.
    fn value_offset(&self, shown: usize) -> usize {
        M::value_offset(&self.value, shown)
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
        if M::exposed(&self.value).is_none() || r.is_empty() {
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
        cx.composition(self).wake();
        self.emit(cx, new);
    }

    fn emit(&self, cx: &mut EventCx, new: String) {
        // The field's own edit: when the program takes it, it isn't text
        // the program set.
        cx.state::<FieldState>().seen.clone_from(&new);
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
        st.last_edit = None;
        cx.composition(self).wake();
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
        cx.composer(self).tell_caret(Some(caret_rect));
    }

    /// The field has the focus: its starting selection, input methods told
    /// what it takes, and the caret, which shows, then blinks. What it was
    /// composing in its other mode is dropped.
    fn arrive(&self, cx: &mut EventCx) {
        let st = cx.state::<FieldState>();
        if !std::mem::replace(&mut st.primed, true)
            && let Some(r) = &self.select_on_focus
        {
            let v = &self.value;
            let (a, b) = (floor(v, r.start.min(v.len())), floor(v, r.end.min(v.len())));
            st.anchor = a.min(b);
            st.caret = a.max(b);
        }
        cx.composer(self).arrive(None);
        cx.request_tick(BLINK);
        self.follow_caret(cx);
    }

    /// An input method's event: what it composes shows as the mode keeps
    /// it; what it commits is typed.
    fn ime(&self, cx: &mut EventCx, ime: &Ime) -> Status {
        let Some(composing) = cx.composer(self).hear() else { return Status::Handled };
        match ime {
            Ime::Commit(text) => {
                composing.commit();
                self.replace(cx, text, EditKind::Typing);
            }
            Ime::Preedit { text, selection } => {
                composing.compose(text, selection.as_ref().map(|r| r.end));
                cx.rebuild();
                cx.invalidate();
            }
        }
        Status::Handled
    }

    fn copy(&self, cx: &mut EventCx) -> bool {
        let st = cx.state::<FieldState>();
        let r = Self::selection(st);
        let Some(text) = self.value.get(r).filter(|t| !t.is_empty()).and_then(M::exposed) else { return false };
        let text = text.to_owned();
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

impl<M: Mode> Widget for FieldWidget<M>
where
    Self: Composes<Mode = M>,
{
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
        let mut scroll = st.scroll;
        // Text the program set: what was being composed into the old text
        // is dropped.
        let set = st.seen != *self.value;
        if set {
            st.seen = self.value.to_string();
        }
        let focus = cx.composition(self);
        if set {
            focus.value_changed();
        }
        let focused = match focus.place() {
            Place::Here => true,
            Place::Away => false,
        };
        // What an input method is composing, as the mode keeps it: a
        // password's as dots, with the input method's caret in it.
        let (preedit, preedit_caret) =
            focus.composing().map_or((String::new(), None), |k| (k.drawn().into_owned(), k.drawn_caret()));
        // Focused in a mode that hasn't arrived (the field just changed
        // mode): it arrives at once, not at the caret's next blink, so few
        // of the input method's events from the old mode reach it.
        if !focused && cx.is_focused() {
            cx.set_timer(ARRIVE, Duration::ZERO);
        }
        // What shows: the text (as dots for a password) with what an input
        // method is composing at the caret.
        let mut shown = M::shown(&self.value).into_owned();
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
        let blink_off = cx.composition(self).is_some_and(|f| f.blink_off());
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
            M::PURPOSE,
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
            Event::Focus(true) => {
                self.arrive(cx);
                cx.rebuild();
                cx.invalidate();
                return Status::Handled;
            }
            Event::Focus(false) => {
                cx.composer(self).went();
                if let Some(msg) = &self.on_blur {
                    cx.emit(msg);
                }
                cx.rebuild();
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
                cx.composition(self).wake();
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
                        if let Some(text) = self.value.get(r).and_then(M::exposed) {
                            let text = text.to_owned();
                            cx.start_drag(sidestep_ui::DragSource::new().text(&text).actions(true, false));
                        }
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
                    cx.composition(self).wake();
                    cx.relayout();
                    cx.invalidate();
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed => self.key(cx, k),
            Event::Tick => {
                if let Place::Here = cx.composition(self).place() {
                    if cx.theme().motion() {
                        cx.composition(self).blink();
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
        let copies = M::exposed(&self.value).is_some();
        match command {
            EditCommand::Copy => sel && copies,
            EditCommand::Cut => sel && copies && self.editable(),
            EditCommand::Delete => sel && self.editable(),
            EditCommand::Paste | EditCommand::PasteAsPlainText => self.editable(),
            EditCommand::SelectAll => !self.value.is_empty(),
            EditCommand::Undo => self.editable() && st.is_some_and(|s| !s.history.is_empty()),
            EditCommand::Redo => self.editable() && st.is_some_and(|s| !s.future.is_empty()),
            _ => false,
        }
    }

    fn edits_text(&self) -> bool {
        self.enabled
    }

    fn selection(&self, state: crate::widget::StateView) -> Option<String> {
        let st = state.get::<FieldState>()?;
        let r = st.caret.min(st.anchor)..st.caret.max(st.anchor);
        self.value.get(r).filter(|t| !t.is_empty()).and_then(M::exposed).map(str::to_owned)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: match M::ROLE {
                SemanticRole::TextInput if self.search => SemanticRole::SearchInput,
                role => role,
            },
            label: self.placeholder.as_deref().map(str::to_owned),
            value: M::exposed(&self.value).map(str::to_owned),
            disabled: !self.enabled,
            actions: vec![SemanticAction::Focus, SemanticAction::SetValue],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        // Inspection and search may describe a password field, but must
        // never publish its contents (including credentials in dialogs).
        M::exposed(&self.value)
    }
}
