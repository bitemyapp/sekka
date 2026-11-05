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
//! it drops them. A widget reaches its `Focus` only through itself
//! ([`Composes`]), keyed by its own mode, and changes it through a
//! [`Composer`]: taking the focus tells input methods the mode's purpose,
//! an input method's event applies only through [`Composer::hear`], and a
//! composition the field dropped (text the program set, or the field's
//! other mode) discards the input method's next event and tells the input
//! method to start over.

#![deny(clippy::wildcard_enum_match_arm)]

use std::borrow::Cow;
use std::marker::PhantomData;

use sidestep_ui::TextPurpose;

use crate::editing::floor;
use crate::layout::Rect;
use crate::widget::{EventCx, LayoutCx, PaintCx, SemanticRole};

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

    /// A proof that the kit is making a kept composition: no code outside
    /// this module can make one, so [`Keep::keep`] can't be called from
    /// outside the kit, even through a generic bound.
    pub struct Token(());

    /// How a mode keeps an input method's composition: made only here.
    pub trait Keep {
        /// Kept of `text`, with the input method's caret `caret` bytes
        /// into it.
        fn keep(text: &str, caret: Option<usize>, _: Token) -> Self;
    }

    impl Keep for super::Preedit {
        fn keep(text: &str, caret: Option<usize>, _: Token) -> Self {
            super::Preedit { text: text.to_owned(), caret: caret.filter(|c| text.is_char_boundary(*c)) }
        }
    }

    impl Keep for super::Masked {
        fn keep(text: &str, caret: Option<usize>, _: Token) -> Self {
            let caret = caret.and_then(|c| text.get(..c)).map(|s| s.chars().count());
            super::Masked { chars: text.chars().count(), caret }
        }
    }

    pub(super) fn keep<K: Keep>(text: &str, caret: Option<usize>) -> K {
        K::keep(text, caret, Token(()))
    }
}

/// A widget that keeps an input method's composition as its mode
/// decides. Its [`Focus`] is reached only through the widget itself
/// ([`EventCx::composition`], [`EventCx::composer`], and `composition` on
/// the layout and paint contexts), so a widget of one mode can't keep its
/// composition in another mode's.
///
/// A widget generic over its mode ties its `Composes` to that mode, as the
/// kit's `text_field` and Aizome's `field` do: their methods and `Widget`
/// impls hold only `where Self: Composes<Mode = M>`. A password field whose
/// `Composes` names the plain mode then isn't a widget:
///
/// ```compile_fail,E0271
/// use aizome_kit::composing::{Composes, Mode, Plain, Secret};
/// use std::marker::PhantomData;
/// struct Field<M>(PhantomData<M>);
/// impl<M: Mode> Composes for Field<M> {
///     type Mode = Plain;
/// }
/// trait Paint {}
/// impl<M: Mode> Paint for Field<M> where Self: Composes<Mode = M> {}
/// fn password(f: &Field<Secret>) -> &dyn Paint {
///     f
/// }
/// ```
///
/// ```
/// use aizome_kit::composing::{Composes, Mode, Secret};
/// use std::marker::PhantomData;
/// struct Field<M>(PhantomData<M>);
/// impl<M: Mode> Composes for Field<M> {
///     type Mode = M;
/// }
/// trait Paint {}
/// impl<M: Mode> Paint for Field<M> where Self: Composes<Mode = M> {}
/// fn password(f: &Field<Secret>) -> &dyn Paint {
///     f
/// }
/// ```
pub trait Composes {
    type Mode: Mode;
}

/// Whether a field has the focus, and what goes with it: the caret's blink
/// and what an input method is composing, kept as the field's mode keeps
/// it. Away, it keeps neither.
///
/// It's kept for the widget, keyed by the widget's own mode: there's no
/// public way to make one, so a password field can't fetch a plain one, as
/// a maintainer could by changing one type when `Focus` had a public
/// `Default`:
///
/// ```compile_fail,E0308
/// use aizome_kit::composing::{Composes, Focus, Plain, Secret};
/// use aizome_kit::widget::EventCx;
/// struct PasswordInput;
/// impl Composes for PasswordInput {
///     type Mode = Secret;
/// }
/// fn ime(cx: &mut EventCx, w: &PasswordInput) {
///     let focus: &mut Focus<Plain> = cx.composition(w);
/// }
/// ```
///
/// ```compile_fail,E0277
/// use aizome_kit::composing::{Focus, Plain};
/// use aizome_kit::widget::EventCx;
/// fn ime(cx: &mut EventCx) {
///     let focus = cx.state::<Focus<Plain>>();
/// }
/// ```
///
/// An input method's event applies only through [`Composer::hear`], which
/// drops one that belongs to a composition the field dropped (text the
/// program set, or the field's other mode) and tells the input method to
/// start over itself. Composing without hearing doesn't compile, nor does
/// applying what `hear` found stale:
///
/// ```compile_fail,E0599
/// use aizome_kit::composing::{Composes, Secret};
/// use aizome_kit::widget::EventCx;
/// struct PasswordInput;
/// impl Composes for PasswordInput {
///     type Mode = Secret;
/// }
/// fn ime(cx: &mut EventCx, w: &PasswordInput) {
///     cx.composition(w).compose("WW", None);
/// }
/// ```
///
/// ```compile_fail,E0599
/// use aizome_kit::composing::{Composes, Secret};
/// use aizome_kit::widget::EventCx;
/// struct PasswordInput;
/// impl Composes for PasswordInput {
///     type Mode = Secret;
/// }
/// fn ime(cx: &mut EventCx, w: &PasswordInput) {
///     cx.composer(w).hear().commit();
/// }
/// ```
///
/// Nor does entering the focus other than through
/// [`Composer::arrive`], which drops what the other mode was composing:
///
/// ```compile_fail,E0599
/// use aizome_kit::composing::{Composes, Secret};
/// use aizome_kit::widget::EventCx;
/// struct PasswordInput;
/// impl Composes for PasswordInput {
///     type Mode = Secret;
/// }
/// fn focused(cx: &mut EventCx, w: &PasswordInput) {
///     cx.composition(w).came();
/// }
/// ```
///
/// The widget's own mode, through the widget:
///
/// ```
/// use aizome_kit::composing::{Composes, Focus, Secret};
/// use aizome_kit::widget::EventCx;
/// struct PasswordInput;
/// impl Composes for PasswordInput {
///     type Mode = Secret;
/// }
/// fn focused(cx: &mut EventCx, w: &PasswordInput) {
///     cx.composer(w).arrive(None);
/// }
/// fn ime(cx: &mut EventCx, w: &PasswordInput) {
///     let _: &mut Focus<Secret> = cx.composition(w);
///     if let Some(composing) = cx.composer(w).hear() {
///         composing.compose("WW", None);
///     }
/// }
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
    /// The field dropped a composition the input method may still have
    /// (the program set the text, or the field changed mode): nothing
    /// shows, and the input method's next event for it is discarded and
    /// the input method told to start over.
    Dropped,
}

/// Where the kit keeps a widget's [`Focus`], under the widget's mode: not
/// nameable outside this module.
struct Slot<M: Mode>(Focus<M>);

impl<M: Mode> Default for Slot<M> {
    fn default() -> Self {
        Slot(Focus(None))
    }
}

/// Where the focus is, for a field to catch up with the kit's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Away,
    Here,
}

impl<M: Mode> Focus<M> {
    pub fn place(&self) -> Place {
        match self.0 {
            None => Place::Away,
            Some(_) => Place::Here,
        }
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
}

/// The composition, in step with the input method: its event applies.
pub struct InStep<'f, M: Mode>(&'f mut Composing<M::Kept>);

impl<M: Mode> InStep<'_, M> {
    /// The input method composes `text` ("" when it stops), with its caret
    /// `caret` bytes into it, kept as the field's mode keeps it.
    pub fn compose(self, text: &str, caret: Option<usize>) {
        *self.0 = if text.is_empty() { Composing::Idle } else { Composing::Now(sealed::keep(text, caret)) };
    }

    /// The input method commits its text (the field types it): nothing is
    /// being composed.
    pub fn commit(self) {
        *self.0 = Composing::Idle;
    }
}

/// A widget's focus at an event, with what it tells input methods:
/// [`EventCx::composer`].
pub struct Composer<'c, 'a, 'e, M: Mode> {
    cx: &'c mut EventCx<'a, 'e>,
    _mode: PhantomData<fn() -> M>,
}

impl<'c, M: Mode> Composer<'c, '_, '_, M> {
    fn slot<N: Mode>(&mut self) -> &mut Focus<N> {
        &mut self.cx.state::<Slot<N>>().0
    }

    /// Into the focus. If the widget had it in its other mode (a prompt
    /// that stopped or started echoing, "Show password"), what was being
    /// composed there is dropped: the input method was restarted with the
    /// new purpose, and its next event, sent for the old composition, is
    /// discarded.
    fn enter(&mut self) {
        let other = self.slot::<M::Other>();
        let changed_mode = match other.place() {
            Place::Here => Composing::Dropped,
            Place::Away => Composing::Idle,
        };
        other.0 = None;
        self.slot::<M>().0 = Some(Here { blink_off: false, composing: changed_mode });
    }

    /// The widget took the focus, or found it had it (after changing
    /// mode): the caret shows, and input methods are told what it takes,
    /// with the caret at `caret`, in the widget's coordinates.
    pub fn arrive(mut self, caret: Option<Rect>) {
        self.enter();
        self.cx.set_text_input_as(M::PURPOSE, true, caret);
    }

    /// The caret moved: input methods are told where it is, while the
    /// widget has the focus.
    pub fn tell_caret(mut self, caret: Option<Rect>) {
        match self.slot::<M>().place() {
            Place::Here => self.cx.set_text_input_as(M::PURPOSE, true, caret),
            Place::Away => {}
        }
    }

    /// The widget lost the focus: input methods stop, and what was being
    /// composed goes with the focus.
    pub fn went(mut self) {
        self.slot::<M>().0 = None;
        self.cx.set_text_input_as(M::PURPOSE, false, None);
    }

    /// At an event other than the input method's: if the widget dropped a
    /// composition, the input method starts over now.
    pub fn catch_up(mut self) {
        self.start_over_if_dropped();
    }

    /// If the widget dropped a composition, it's idle again and the input
    /// method starts over: what the input method sends next is its own.
    /// Whether it did.
    fn start_over_if_dropped(&mut self) -> Started {
        let started = match &mut self.slot::<M>().0 {
            Some(h) => match h.composing {
                Composing::Dropped => {
                    h.composing = Composing::Idle;
                    Started::Over
                }
                Composing::Idle | Composing::Now(_) => Started::Already,
            },
            None => Started::Already,
        };
        match started {
            Started::Over => self.cx.reset_text_input(),
            Started::Already => {}
        }
        started
    }

    /// An input method's event: the composition, if the event applies.
    /// One for a composition the widget dropped is `None`, and the input
    /// method has been told to start over. It comes only to the focused
    /// widget, so the widget is here if it hadn't heard.
    pub fn hear(mut self) -> Option<InStep<'c, M>> {
        if let Place::Away = self.slot::<M>().place() {
            self.enter();
        }
        match self.start_over_if_dropped() {
            Started::Over => None,
            Started::Already => {
                let cx = self.cx;
                let h = cx.state::<Slot<M>>().0.0.as_mut()?;
                Some(InStep(&mut h.composing))
            }
        }
    }
}

/// Whether the input method was just told to start over.
enum Started {
    Over,
    Already,
}

impl<'a, 'e> EventCx<'a, 'e> {
    /// `w`'s focus and composition, as its own mode keeps them.
    pub fn composition<W: Composes>(&mut self, _: &W) -> &mut Focus<W::Mode> {
        &mut self.state::<Slot<W::Mode>>().0
    }

    /// `w`'s focus at this event, with what it tells input methods.
    pub fn composer<W: Composes>(&mut self, _: &W) -> Composer<'_, 'a, 'e, W::Mode> {
        Composer { cx: self, _mode: PhantomData }
    }
}

impl LayoutCx<'_, '_> {
    /// `w`'s focus and composition, as its own mode keeps them.
    pub fn composition<W: Composes>(&mut self, _: &W) -> &mut Focus<W::Mode> {
        &mut self.state::<Slot<W::Mode>>().0
    }
}

impl<'a> PaintCx<'a> {
    /// `w`'s focus and composition, as its own mode keeps them.
    pub fn composition<W: Composes>(&self, _: &W) -> Option<&'a Focus<W::Mode>> {
        self.state::<Slot<W::Mode>>().map(|s| &s.0)
    }
}
