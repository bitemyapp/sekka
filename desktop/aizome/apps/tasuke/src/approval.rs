//! An approval, from the card's appearing to the call's running or not.
//!
//! ```text
//! Pending ──Run──▶ Sent ──the call starts──▶ Allowed
//!    │                └──it comes back refused──▶ Denied
//!    ├──Don't run──▶ Denied
//!    └──the turn ends or is stopped──▶ Withdrawn
//! ```
//!
//! A card answers once: a second press (a double click, the keyboard
//! after the pointer) changes nothing and sends nothing. Nothing but the
//! card's two buttons decides; no key presses Run.

use crate::agent::Decision;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Waiting for the person: why the policy asks, and whether it runs
    /// as root (which asks for a password after Run).
    Pending {
        reason: String,
        root: bool,
    },
    /// Run was pressed and sent; the agent hasn't started it yet (for
    /// root, polkit's dialog is up).
    Sent {
        root: bool,
    },
    Allowed,
    Denied {
        note: Option<String>,
    },
    /// The turn ended before anyone answered.
    Withdrawn,
}

/// Why a decision wasn't taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    AlreadyDecided,
}

impl Gate {
    pub fn pending(reason: impl Into<String>, root: bool) -> Gate {
        Gate::Pending { reason: reason.into(), root }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Gate::Pending { .. })
    }

    /// The person pressed a button: what to send the agent, if anything.
    pub fn decide(&mut self, decision: Decision) -> Result<Decision, Refused> {
        let Gate::Pending { root, .. } = self else { return Err(Refused::AlreadyDecided) };
        *self = match &decision {
            Decision::Allow => Gate::Sent { root: *root },
            Decision::Deny { note } => Gate::Denied { note: note.clone() },
        };
        Ok(decision)
    }

    /// The agent started the call.
    pub fn started(&mut self) {
        if matches!(self, Gate::Sent { .. } | Gate::Pending { .. }) {
            *self = Gate::Allowed;
        }
    }

    /// The call came back without running (denied here, or polkit's
    /// password was refused or cancelled).
    pub fn refused(&mut self) {
        if !matches!(self, Gate::Denied { .. }) {
            *self = Gate::Denied { note: None };
        }
    }

    /// The turn ended (finished, stopped, failed).
    pub fn turn_ended(&mut self) {
        if matches!(self, Gate::Pending { .. } | Gate::Sent { .. }) {
            *self = Gate::Withdrawn;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_is_sent_once_and_the_call_starting_allows_it() {
        let mut g = Gate::pending("It changes a service", false);
        assert_eq!(g.decide(Decision::Allow), Ok(Decision::Allow));
        assert_eq!(g, Gate::Sent { root: false });
        // A second press does nothing.
        assert_eq!(g.decide(Decision::Allow), Err(Refused::AlreadyDecided));
        assert_eq!(g.decide(Decision::Deny { note: None }), Err(Refused::AlreadyDecided));
        g.started();
        assert_eq!(g, Gate::Allowed);
        g.turn_ended();
        assert_eq!(g, Gate::Allowed, "what ran stays ran");
    }

    #[test]
    fn dont_run_denies_at_once() {
        let mut g = Gate::pending("It writes a file", false);
        assert_eq!(g.decide(Decision::Deny { note: None }), Ok(Decision::Deny { note: None }));
        assert_eq!(g, Gate::Denied { note: None });
        g.started();
        assert_eq!(g, Gate::Denied { note: None }, "a denied call never shows as run");
    }

    #[test]
    fn root_waits_for_the_password_and_a_refused_password_denies() {
        let mut g = Gate::pending("It runs as root", true);
        g.decide(Decision::Allow).unwrap();
        assert_eq!(g, Gate::Sent { root: true });
        g.refused();
        assert_eq!(g, Gate::Denied { note: None });
    }

    #[test]
    fn an_unanswered_card_is_withdrawn_when_the_turn_ends() {
        let mut g = Gate::pending("It changes a service", false);
        g.turn_ended();
        assert_eq!(g, Gate::Withdrawn);
        assert_eq!(g.decide(Decision::Allow), Err(Refused::AlreadyDecided));
    }
}
