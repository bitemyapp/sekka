//! A field's mode, plain or secret, as a type. What a field shows of its
//! value, what of it leaves the field, what input methods are told it takes
//! and what it keeps of what they compose all follow from it, so a password
//! field has no flag to forget, and nothing of a composition in clear to
//! draw.

#![deny(clippy::wildcard_enum_match_arm)]

use std::borrow::Cow;

use aizome_kit::sidestep_ui::TextPurpose;
use aizome_kit::widget::SemanticRole;

/// How a field shows and keeps text: [`Plain`] or [`Secret`]. Only this
/// crate makes modes.
pub trait Mode: sealed::Mode {}

/// A field showing its text.
pub enum Plain {}

/// A password field: dots for its text, nothing of it copied out or read
/// out, input methods told it's a password, and of what they compose, only
/// how many characters.
pub enum Secret {}

impl Mode for Plain {}
impl Mode for Secret {}

/// What a plain field keeps of the text an input method is composing in
/// it: the text, shown where it's being typed.
#[derive(Debug)]
pub struct Preedit {
    text: String,
}

impl Preedit {
    /// The text being composed.
    pub fn text(&self) -> &str {
        &self.text
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
/// use aizome_face::widgets::field::Masked;
/// fn drawn(composing: &Masked) -> String {
///     composing.text().to_owned()
/// }
/// ```
///
/// Nor does writing it beside the dots, as the field's paint did
/// (`all = shown(value) + preedit`):
///
/// ```compile_fail,E0277
/// use aizome_face::widgets::field::Masked;
/// fn all(dots: &str, preedit: &Masked) -> String {
///     format!("{dots}{preedit}")
/// }
/// ```
///
/// A plain field's composition has its text; a password's has a count to
/// draw as dots:
///
/// ```
/// use aizome_face::widgets::field::{Masked, Preedit};
/// fn plain(dots: &str, preedit: &Preedit) -> String {
///     format!("{dots}{}", preedit.text())
/// }
/// fn secret(dots: &str, preedit: &Masked) -> String {
///     format!("{dots}{}", "•".repeat(preedit.chars()))
/// }
/// ```
#[derive(Debug)]
pub struct Masked {
    chars: usize,
}

impl Masked {
    /// How many characters are being composed.
    pub fn chars(&self) -> usize {
        self.chars
    }
}

/// A password's characters, as dots.
fn dots(n: usize) -> String {
    "•".repeat(n)
}

pub(super) mod sealed {
    use super::*;

    /// What a mode decides. Outside this crate it can't be named, so no
    /// other mode can be made.
    pub trait Mode: 'static {
        /// What the field keeps of an input method's composition.
        type Kept: Kept;
        /// The other mode, whose state a field that changes mode drops.
        type Other: super::Mode;
        /// What the field is, to assistive technology.
        const ROLE: SemanticRole;
        /// What input methods are told the field takes.
        const PURPOSE: TextPurpose;
        /// `s` of the value, as the field draws it.
        fn shown(s: &str) -> Cow<'_, str>;
        /// `s` of the value, as it may leave the field (read out, copied,
        /// offered as the selection): none of a password.
        fn exposed(s: &str) -> Option<&str>;
    }

    /// What a mode keeps of an input method's composition.
    pub trait Kept: 'static {
        /// Kept of `text`, as the input method composes it.
        fn keep(text: &str) -> Self;
        /// What the field draws of it.
        fn drawn(&self) -> Cow<'_, str>;
    }

    impl Mode for Plain {
        type Kept = Preedit;
        type Other = Secret;
        const ROLE: SemanticRole = SemanticRole::TextInput;
        const PURPOSE: TextPurpose = TextPurpose::Normal;
        fn shown(s: &str) -> Cow<'_, str> {
            Cow::Borrowed(s)
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
        fn exposed(_: &str) -> Option<&str> {
            None
        }
    }

    impl Kept for Preedit {
        fn keep(text: &str) -> Self {
            Preedit { text: text.to_owned() }
        }
        fn drawn(&self) -> Cow<'_, str> {
            Cow::Borrowed(&self.text)
        }
    }

    impl Kept for Masked {
        fn keep(text: &str) -> Self {
            Masked { chars: text.chars().count() }
        }
        fn drawn(&self) -> Cow<'_, str> {
            Cow::Owned(dots(self.chars))
        }
    }
}
