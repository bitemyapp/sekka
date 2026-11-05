//! What a line of text keeps of an input method's composition, as its mode
//! decides: a plain field the text, a password field only how many
//! characters ([`Masked`]). The kit's `text_field` and Aizome's `field`
//! both build on it.
//!
//! A field's [`Mode`] is a type: [`Plain`] or [`Secret`]. It decides what
//! the field draws of its value, what of it leaves the field (read out,
//! copied, dragged, offered as the selection), what input methods are told
//! it takes, and what the field keeps of what they compose. A password
//! field has no flag to forget, and nothing of a composition in clear to
//! draw.
//!
//! [`Focus`] is what goes with the focus: the caret's blink and the
//! composition. Both live only while the field has the focus, so losing
//! it drops them, and an input method's event applies only through
//! [`Focus::hear`], which drops one that belongs to a composition the
//! field already dropped.

#![deny(clippy::wildcard_enum_match_arm)]

use std::borrow::Cow;

use sidestep_ui::TextPurpose;

use crate::editing::floor;
use crate::widget::{EventCx, SemanticRole};

/// How a password's characters show.
pub const BULLET: char = '•';

/// How a field shows and keeps text: [`Plain`] or [`Secret`]. Only the kit
/// makes modes.
pub trait Mode: sealed::Sealed + 'static {
    /// What the field keeps of an input method's composition.
    type Kept: Kept;
    /// The other mode: a field that changes mode drops what it kept in it.
    type Other: Mode;
    /// What the field is, to assistive technology.
    const ROLE: SemanticRole;
    /// What input methods are told the field takes.
    const PURPOSE: TextPurpose;
    /// `s` of the value, as the field draws it.
    fn shown(s: &str) -> Cow<'_, str>;
    /// How many bytes `s` of the value takes as drawn.
    fn shown_len(s: &str) -> usize;
    /// The offset in `value` of the place `shown` bytes into it as drawn.
    fn value_offset(value: &str, shown: usize) -> usize;
    /// `s` of the value as it may leave the field (read out, copied,
    /// dragged, offered as the selection): none of a password.
    fn exposed(s: &str) -> Option<&str>;
}

/// A field showing its text.
pub enum Plain {}

/// A password field: dots for its text, none of it leaving the field,
/// input methods told it's a password, and of what they compose, only how
/// many characters.
pub enum Secret {}

impl Mode for Plain {
    type Kept = Preedit;
    type Other = Secret;
    const ROLE: SemanticRole = SemanticRole::TextInput;
    const PURPOSE: TextPurpose = TextPurpose::Normal;
    fn shown(s: &str) -> Cow<'_, str> {
        Cow::Borrowed(s)
    }
    fn shown_len(s: &str) -> usize {
        s.len()
    }
    fn value_offset(value: &str, shown: usize) -> usize {
        floor(value, shown)
    }
    fn exposed(s: &str) -> Option<&str> {
        Some(s)
    }
}

impl Mode for Secret {
    type Kept = Masked;
    type Other = Plain;
    const ROLE: SemanticRole = SemanticRole::PasswordInput;
    const PURPOSE: TextPurpose = TextPurpose::Password;
    fn shown(s: &str) -> Cow<'_, str> {
        Cow::Owned(dots(s.chars().count()))
    }
    fn shown_len(s: &str) -> usize {
        s.chars().count() * BULLET.len_utf8()
    }
    fn value_offset(value: &str, shown: usize) -> usize {
        let n = shown / BULLET.len_utf8();
        value.char_indices().nth(n).map_or(value.len(), |(i, _)| i)
    }
    fn exposed(_: &str) -> Option<&str> {
        None
    }
}

/// What a mode keeps of an input method's composition, made only as the
/// input method's event arrives.
pub trait Kept: sealed::Keep + 'static {
    /// What the field draws of it.
    fn drawn(&self) -> Cow<'_, str>;
    /// Where the input method's caret is in what's drawn, in bytes, if it
    /// shows one.
    fn drawn_caret(&self) -> Option<usize>;
}

/// What a plain field keeps of the text an input method is composing in
/// it: the text, shown where it's being typed.
#[derive(Debug)]
pub struct Preedit {
    text: String,
    caret: Option<usize>,
}

impl Preedit {
    /// The text being composed.
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Kept for Preedit {
    fn drawn(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }
    fn drawn_caret(&self) -> Option<usize> {
        self.caret
    }
}

/// What a password field keeps of the text an input method is composing in
/// it: how many characters, never which, so nothing can draw them.
///
/// The text itself is dropped as the input method's event arrives, so
/// drawing it, the mistake that showed a password's composition in clear,
/// doesn't compile:
///
/// ```compile_fail,E0599
/// use aizome_kit::composing::Masked;
/// fn drawn(composing: &Masked) -> String {
///     composing.text().to_owned()
/// }
/// ```
///
/// Nor does writing it beside the dots, as the fields' paint did
/// (`shown(value) + preedit`):
///
/// ```compile_fail,E0277
/// use aizome_kit::composing::Masked;
/// fn all(dots: &str, preedit: &Masked) -> String {
///     format!("{dots}{preedit}")
/// }
/// ```
///
/// A plain field's composition has its text; a password's has a count to
/// draw as dots:
///
/// ```
/// use aizome_kit::composing::{Kept, Masked, Preedit};
/// fn plain(dots: &str, preedit: &Preedit) -> String {
///     format!("{dots}{}", preedit.text())
/// }
/// fn secret(dots: &str, preedit: &Masked) -> String {
///     format!("{dots}{}", preedit.drawn())
/// }
/// ```
#[derive(Debug)]
pub struct Masked {
    chars: usize,
    /// The input method's caret, in characters.
    caret: Option<usize>,
}

impl Masked {
    /// How many characters are being composed.
    pub fn chars(&self) -> usize {
        self.chars
    }
}

impl Kept for Masked {
    fn drawn(&self) -> Cow<'_, str> {
        Cow::Owned(dots(self.chars))
    }
    fn drawn_caret(&self) -> Option<usize> {
        self.caret.map(|n| n * BULLET.len_utf8())
    }
}

/// A password's characters, as dots.
fn dots(n: usize) -> String {
    BULLET.to_string().repeat(n)
}

mod sealed {
    /// Only the kit's modes are modes.
    pub trait Sealed {}
    impl Sealed for super::Plain {}
    impl Sealed for super::Secret {}

    /// How a mode keeps an input method's composition: made only here.
    pub trait Keep {
        /// Kept of `text`, with the input method's caret `caret` bytes
        /// into it.
        fn keep(text: &str, caret: Option<usize>) -> Self;
    }

    impl Keep for super::Preedit {
        fn keep(text: &str, caret: Option<usize>) -> Self {
            super::Preedit { text: text.to_owned(), caret: caret.filter(|c| text.is_char_boundary(*c)) }
        }
    }

    impl Keep for super::Masked {
        fn keep(text: &str, caret: Option<usize>) -> Self {
            let caret = caret.and_then(|c| text.get(..c)).map(|s| s.chars().count());
            super::Masked { chars: text.chars().count(), caret }
        }
    }
}

/// Whether a field has the focus, and what goes with it: the caret's blink
/// and what an input method is composing, kept as the field's mode keeps
/// it. Away, it keeps neither.
///
/// An input method's event applies only through [`hear`](Focus::hear), so
/// it can't be applied to a composition the field dropped (text the
/// program set). Composing without hearing doesn't compile:
///
/// ```compile_fail,E0599
/// use aizome_kit::composing::{Focus, Secret};
/// let mut focus = Focus::<Secret>::default();
/// focus.came();
/// focus.compose("WW", None);
/// ```
///
/// nor does typing what a dropped composition commits:
///
/// ```compile_fail,E0599
/// use aizome_kit::composing::{Focus, Heard, Secret};
/// fn commit(focus: &mut Focus<Secret>) {
///     match focus.hear() {
///         Heard::InStep(composing) => composing.commit(),
///         Heard::Stale(start_over) => start_over.commit(),
///     }
/// }
/// ```
///
/// Heard, an event applies; text the program sets drops the composition,
/// and the input method's next event for it is stale; losing the focus
/// drops it too:
///
/// ```
/// use aizome_kit::composing::{Focus, Heard, Secret};
/// let mut focus = Focus::<Secret>::default();
/// focus.came();
/// match focus.hear() {
///     Heard::InStep(composing) => composing.compose("WW", None),
///     Heard::Stale(_) => unreachable!(),
/// }
/// assert_eq!(focus.composing().map(|m| m.chars()), Some(2));
/// focus.value_changed();
/// assert!(focus.composing().is_none());
/// assert!(matches!(focus.hear(), Heard::Stale(_)));
/// match focus.hear() {
///     Heard::InStep(composing) => composing.compose("x", None),
///     Heard::Stale(_) => unreachable!("told once"),
/// }
/// focus.went();
/// assert!(focus.composing().is_none());
/// ```
pub struct Focus<M: Mode>(Option<Here<M::Kept>>);

/// The field has the focus: the caret blinks, and an input method may be
/// composing.
struct Here<K> {
    blink_off: bool,
    composing: Composing<K>,
}

enum Composing<K> {
    Idle,
    /// An input method's composition, into the value as the field last
    /// saw it.
    Now(K),
    /// The program changed the value under a composition: it no longer
    /// shows, and the input method is yet to be told to start over.
    Dropped,
}

impl<K> Here<K> {
    fn new() -> Self {
        Here { blink_off: false, composing: Composing::Idle }
    }
}

impl<M: Mode> Default for Focus<M> {
    fn default() -> Self {
        Focus(None)
    }
}

/// Where the focus is, for a field to catch up with the kit's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Away,
    Here,
}

/// What an input method's event finds.
pub enum Heard<'f, M: Mode> {
    /// The field is in step with the input method: the event applies.
    InStep(InStep<'f, M>),
    /// The event belongs to a composition the field dropped: the input
    /// method is to start over, and the event goes no further.
    Stale(StartOver),
}

/// A composition the field dropped, which the input method still has: it's
/// told to start over.
#[must_use = "tell the input method to start over"]
pub struct StartOver(());

impl StartOver {
    pub fn tell(self, cx: &mut EventCx) {
        cx.reset_text_input();
    }
}

/// The composition, in step with the input method: its event applies.
pub struct InStep<'f, M: Mode>(&'f mut Composing<M::Kept>);

impl<M: Mode> InStep<'_, M> {
    /// The input method composes `text` ("" when it stops), with its caret
    /// `caret` bytes into it, kept as the field's mode keeps it.
    pub fn compose(self, text: &str, caret: Option<usize>) {
        *self.0 = if text.is_empty() {
            Composing::Idle
        } else {
            Composing::Now(<M::Kept as sealed::Keep>::keep(text, caret))
        };
    }

    /// The input method commits its text (the field types it): nothing is
    /// being composed.
    pub fn commit(self) {
        *self.0 = Composing::Idle;
    }
}

impl<M: Mode> Focus<M> {
    pub fn place(&self) -> Place {
        match self.0 {
            None => Place::Away,
            Some(_) => Place::Here,
        }
    }

    /// The field took the focus: the caret shows, nothing is composed yet.
    pub fn came(&mut self) {
        self.0 = Some(Here::new());
    }

    /// It lost the focus. The kit turns text input off, which ends the
    /// input method's composition; the field's goes with the focus.
    pub fn went(&mut self) {
        self.0 = None;
    }

    /// The caret blinks.
    pub fn blink(&mut self) {
        if let Some(h) = &mut self.0 {
            h.blink_off = !h.blink_off;
        }
    }

    /// Something was typed or the caret moved: it shows at once.
    pub fn wake(&mut self) {
        if let Some(h) = &mut self.0 {
            h.blink_off = false;
        }
    }

    /// Whether the caret is in the off half of its blink.
    pub fn blink_off(&self) -> bool {
        self.0.as_ref().is_some_and(|h| h.blink_off)
    }

    /// What's being composed, as the field's mode keeps it: what it draws.
    pub fn composing(&self) -> Option<&M::Kept> {
        match self.0.as_ref().map(|h| &h.composing) {
            Some(Composing::Now(k)) => Some(k),
            Some(Composing::Idle | Composing::Dropped) | None => None,
        }
    }

    /// The program changed the value: what was being composed into the old
    /// one is dropped, and the input method is told at the next event.
    pub fn value_changed(&mut self) {
        if let Some(h) = &mut self.0 {
            match h.composing {
                Composing::Now(_) => h.composing = Composing::Dropped,
                Composing::Idle | Composing::Dropped => {}
            }
        }
    }

    /// At an event other than the input method's: if the field dropped a
    /// composition, the input method is to start over now.
    pub fn owed(&mut self) -> Option<StartOver> {
        let h = self.0.as_mut()?;
        match h.composing {
            Composing::Dropped => {
                h.composing = Composing::Idle;
                Some(StartOver(()))
            }
            Composing::Idle | Composing::Now(_) => None,
        }
    }

    /// An event of the input method's. It comes only to the focused
    /// widget, so the field is here if it hadn't heard.
    pub fn hear(&mut self) -> Heard<'_, M> {
        let h = self.0.get_or_insert_with(Here::new);
        match h.composing {
            Composing::Dropped => {
                h.composing = Composing::Idle;
                Heard::Stale(StartOver(()))
            }
            Composing::Idle | Composing::Now(_) => Heard::InStep(InStep(&mut h.composing)),
        }
    }
}
