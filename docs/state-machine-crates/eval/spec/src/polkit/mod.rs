//! `polkit`: one authentication request from polkitd through the shell's
//! dialog and the helper's conversations (catalog SYB-09, SHB-03). See
//! `SPEC.md`.

#[cfg(feature = "testing")]
pub mod env;
#[cfg(feature = "testing")]
pub mod traces;

use crate::Sink;

/// polkitd's cookie for one `BeginAuthentication`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cookie(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Config {
    /// The first automatic retry's delay after an attempt that failed with
    /// no answer given; each further one doubles it (capped at 60 s).
    pub retry_base_ms: u32,
    /// After this many such failures in a row, stop retrying: `Unavailable`.
    pub max_unanswered: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self { retry_base_ms: 500, max_unanswered: 3 }
    }
}

impl Config {
    /// The delay before automatic retry number `n` (1-based).
    pub fn retry_delay(&self, n: u8) -> u32 {
        let shift = u32::from(n.max(1) - 1).min(16);
        self.retry_base_ms.saturating_mul(1 << shift).min(60_000)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// polkitd's `BeginAuthentication`. `identities`: how many may
    /// authenticate; `preferred`: the one to preselect (the person at the
    /// screen); `screen`: the shell has a focused screen for the dialog.
    Begin { cookie: Cookie, identities: u8, preferred: u8, screen: bool },
    /// polkitd's `CancelAuthentication`.
    CancelRequest { cookie: Cookie },
    /// The polkit authority's owner changed: open requests end.
    AuthorityChanged,
    /// The helper for conversation `conv` couldn't be started or reached.
    Unreachable { conv: u32 },
    /// The helper asks for a line (`echo`: show what's typed).
    Prompt { conv: u32, echo: bool },
    /// The helper relays a PAM error message.
    PamError { conv: u32 },
    /// The helper relays a PAM info message.
    PamInfo { conv: u32 },
    /// The helper said SUCCESS.
    Success { conv: u32 },
    /// The attempt ended without SUCCESS: FAILURE, an unknown line, EOF or
    /// a write error.
    Failure { conv: u32 },
    /// The person picked identity `i` in the dialog.
    ChooseIdentity(u8),
    /// The person pressed Authenticate (or Return) with the field's text.
    Submit,
    /// The person dismissed the dialog (Cancel, Escape).
    Dismiss,
    /// The armed retry timer fired.
    RetryTimer,
}

/// A line the dialog shows under the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Note {
    /// PAM's error text.
    PamError,
    /// PAM's info text.
    PamInfo,
    /// "That didn't work. Try again." (an answer was checked and refused).
    TryAgain,
    /// The helper failed before any answer; trying again shortly.
    Retrying,
    /// It keeps failing before any answer: authentication isn't available.
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    OpenDialog {
        cookie: Cookie,
    },
    CloseDialog {
        cookie: Cookie,
    },
    /// Start (spawn or connect) the helper for conversation `conv`.
    StartHelper {
        conv: u32,
        identity: u8,
    },
    /// End conversation `conv` (kill the helper or shut its socket).
    EndHelper {
        conv: u32,
    },
    /// Write the field's text to conversation `conv` and wipe it.
    WriteAnswer {
        conv: u32,
    },
    /// Show the prompt and focus the field.
    ShowPrompt {
        echo: bool,
    },
    ShowNote(Note),
    ArmRetry {
        ms: u32,
    },
    CancelRetry,
    /// Complete `BeginAuthentication`.
    Reply {
        cookie: Cookie,
        authorized: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateName {
    /// No request.
    Idle,
    /// Dialog open; a helper started; no prompt yet.
    Starting,
    /// Waiting for the person's answer.
    Prompted,
    /// The answer was written; waiting for the helper's verdict.
    Checking,
    /// An attempt failed before any answer; the retry timer is armed.
    RetryWait,
    /// Too many such failures in a row; no automatic retries.
    Unavailable,
}

/// An implementation of the polkit machine.
pub trait Machine: Sized {
    fn new(config: Config) -> Self;
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>);
    fn state(&self) -> StateName;
}
