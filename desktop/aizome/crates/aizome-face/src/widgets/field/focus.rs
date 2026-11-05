//! What a field keeps while it has the focus: the caret's blink, and what
//! an input method is composing in it. Both live only in [`Here`], so
//! losing the focus drops them, and a composition is kept only as the
//! field's mode keeps it ([`Masked`](super::Masked) for a password).

#![deny(clippy::wildcard_enum_match_arm)]

use aizome_kit::widget::EventCx;

use super::mode::sealed::Kept;

/// Whether the field has the focus (`None` when it hasn't), and what goes
/// with it.
pub(super) struct Focus<K>(Option<Here<K>>);

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

impl<K> Default for Focus<K> {
    fn default() -> Self {
        Focus(None)
    }
}

/// Where the focus is, for the field to catch up with the kit's.
pub(super) enum Place {
    Away,
    Here,
}

/// What an input method's event finds.
pub(super) enum Heard<'f, K> {
    /// The field is in step with the input method: the event applies.
    InStep(InStep<'f, K>),
    /// The event belongs to a composition the field dropped: the input
    /// method is to start over, and the event goes no further.
    Stale(StartOver),
}

/// A composition the field dropped, which the input method still has: it's
/// told to start over.
#[must_use = "tell the input method to start over"]
pub(super) struct StartOver(());

impl StartOver {
    pub(super) fn tell(self, cx: &mut EventCx) {
        cx.reset_text_input();
    }
}

/// The composition, in step with the input method: its event applies.
pub(super) struct InStep<'f, K>(&'f mut Composing<K>);

impl<K: Kept> InStep<'_, K> {
    /// The input method composes `text` ("" when it stops), kept as the
    /// field's mode keeps it.
    pub(super) fn compose(self, text: &str) {
        *self.0 = if text.is_empty() { Composing::Idle } else { Composing::Now(K::keep(text)) };
    }

    /// The input method commits its text (the field inserts it): nothing
    /// is being composed.
    pub(super) fn commit(self) {
        *self.0 = Composing::Idle;
    }
}

impl<K: Kept> Focus<K> {
    pub(super) fn place(&self) -> Place {
        match self.0 {
            None => Place::Away,
            Some(_) => Place::Here,
        }
    }

    /// The field took the focus: the caret shows, nothing is composed yet.
    pub(super) fn came(&mut self) {
        self.0 = Some(Here::new());
    }

    /// It lost the focus. The kit turned text input off, which ends the
    /// input method's composition; the field's goes with the focus.
    pub(super) fn went(&mut self) {
        self.0 = None;
    }

    /// The caret blinks.
    pub(super) fn blink(&mut self) {
        if let Some(h) = &mut self.0 {
            h.blink_off = !h.blink_off;
        }
    }

    /// Something was typed or the caret moved: it shows at once.
    pub(super) fn wake(&mut self) {
        if let Some(h) = &mut self.0 {
            h.blink_off = false;
        }
    }

    /// Whether the caret is in the off half of its blink.
    pub(super) fn blink_off(&self) -> bool {
        self.0.as_ref().is_some_and(|h| h.blink_off)
    }

    /// What's being composed, as the field's mode keeps it: what it draws.
    pub(super) fn composing(&self) -> Option<&K> {
        match self.0.as_ref().map(|h| &h.composing) {
            Some(Composing::Now(k)) => Some(k),
            Some(Composing::Idle | Composing::Dropped) | None => None,
        }
    }

    /// The program changed the value: what was being composed into the old
    /// one is dropped, and the input method is told at the next event.
    pub(super) fn value_changed(&mut self) {
        if let Some(h) = &mut self.0 {
            match h.composing {
                Composing::Now(_) => h.composing = Composing::Dropped,
                Composing::Idle | Composing::Dropped => {}
            }
        }
    }

    /// An event other than the input method's: if the field dropped a
    /// composition, the input method is to start over now.
    pub(super) fn owed(&mut self) -> Option<StartOver> {
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
    pub(super) fn hear(&mut self) -> Heard<'_, K> {
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
