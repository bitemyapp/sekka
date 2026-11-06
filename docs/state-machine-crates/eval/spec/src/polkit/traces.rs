//! Conformance traces for `polkit`.

use super::{Config, Cookie, Effect as F, Event as E, Note, StateName as S};
use crate::testing::{Driven, Tape, Target, Trace};

/// The `polkit` target.
pub struct Spec;

impl<M: super::Machine> Driven<Spec> for M {
    fn boot(config: Config) -> Self {
        M::new(config)
    }
    fn step(&mut self, event: E, out: &mut Vec<F>) {
        self.handle(event, out);
    }
    fn observe(&self) -> S {
        self.state()
    }
}

impl Target for Spec {
    const NAME: &'static str = "polkit";
    type Config = Config;
    type Event = E;
    type Effect = F;
    type State = S;
    type Env = super::env::Env;

    fn traces() -> Vec<Trace<Self>> {
        traces()
    }
    fn configs() -> Vec<Config> {
        vec![
            Config::default(),
            Config { retry_base_ms: 100, max_unanswered: 1 },
            Config { retry_base_ms: 250, max_unanswered: 5 },
        ]
    }
    fn env(_: &Config) -> super::env::Env {
        super::env::Env::default()
    }
    fn next(env: &mut super::env::Env, tape: &mut Tape, state: S, effects: &[F]) -> E {
        env.next(tape, state, effects)
    }
}

const C1: Cookie = Cookie(1);
const C2: Cookie = Cookie(2);

fn begin(cookie: Cookie) -> E {
    E::Begin { cookie, identities: 1, preferred: 0, screen: true }
}
fn opened(cookie: Cookie, conv: u32, identity: u8) -> [F; 2] {
    [F::OpenDialog { cookie }, F::StartHelper { conv, identity }]
}
fn prompt(conv: u32) -> E {
    E::Prompt { conv, echo: false }
}
const SHOWN: F = F::ShowPrompt { echo: false };

pub fn traces() -> Vec<Trace<Spec>> {
    let c = Config::default();
    vec![
        Trace::new("password-accepted", "prompt, answer, SUCCESS: authorized and closed", c)
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(prompt(1), [SHOWN], S::Prompted)
            .step(E::Submit, [F::WriteAnswer { conv: 1 }], S::Checking)
            .step(
                E::Success { conv: 1 },
                [F::EndHelper { conv: 1 }, F::Reply { cookie: C1, authorized: true }, F::CloseDialog { cookie: C1 }],
                S::Idle,
            ),
        Trace::new("wrong-password-then-right", "a refused answer restarts the conversation at once", c)
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(prompt(1), [SHOWN], S::Prompted)
            .step(E::Submit, [F::WriteAnswer { conv: 1 }], S::Checking)
            .step(E::PamError { conv: 1 }, [F::ShowNote(Note::PamError)], S::Checking)
            .step(
                E::Failure { conv: 1 },
                [F::EndHelper { conv: 1 }, F::ShowNote(Note::TryAgain), F::StartHelper { conv: 2, identity: 0 }],
                S::Starting,
            )
            .step(prompt(2), [SHOWN], S::Prompted)
            .step(E::Submit, [F::WriteAnswer { conv: 2 }], S::Checking)
            .step(
                E::Success { conv: 2 },
                [F::EndHelper { conv: 2 }, F::Reply { cookie: C1, authorized: true }, F::CloseDialog { cookie: C1 }],
                S::Idle,
            ),
        Trace::new("stale-conversation number-is-ignored", "events of a superseded conversation do nothing", c)
            .step(E::Begin { cookie: C1, identities: 2, preferred: 0, screen: true }, opened(C1, 1, 0), S::Starting)
            .step(prompt(1), [SHOWN], S::Prompted)
            .step(
                E::ChooseIdentity(1),
                [F::EndHelper { conv: 1 }, F::StartHelper { conv: 2, identity: 1 }],
                S::Starting,
            )
            .step(prompt(1), [], S::Starting)
            .step(E::Failure { conv: 1 }, [], S::Starting)
            .step(E::Success { conv: 1 }, [], S::Starting)
            .step(E::PamInfo { conv: 1 }, [], S::Starting)
            .step(E::Prompt { conv: 2, echo: true }, [F::ShowPrompt { echo: true }], S::Prompted)
            .step(E::ChooseIdentity(2), [], S::Prompted),
        Trace::new("submit-only-answers-a-prompt", "an answer before the prompt, or a second one, isn't sent", c)
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(E::Submit, [], S::Starting)
            .step(prompt(1), [SHOWN], S::Prompted)
            .step(E::Submit, [F::WriteAnswer { conv: 1 }], S::Checking)
            .step(E::Submit, [], S::Checking)
            .step(E::Prompt { conv: 1, echo: true }, [F::ShowPrompt { echo: true }], S::Prompted)
            .step(E::Prompt { conv: 1, echo: false }, [], S::Prompted),
        Trace::new("dismiss-ends-the-request", "Cancel: not authorized; the conversation's late lines do nothing", c)
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(prompt(1), [SHOWN], S::Prompted)
            .step(
                E::Dismiss,
                [F::EndHelper { conv: 1 }, F::Reply { cookie: C1, authorized: false }, F::CloseDialog { cookie: C1 }],
                S::Idle,
            )
            .step(prompt(1), [], S::Idle)
            .step(E::Success { conv: 1 }, [], S::Idle)
            .step(E::Submit, [], S::Idle)
            .step(E::Dismiss, [], S::Idle)
            .step(E::RetryTimer, [], S::Idle),
        Trace::new("polkit-cancels", "CancelAuthentication for this cookie ends it; another cookie's doesn't", c)
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(E::CancelRequest { cookie: C2 }, [], S::Starting)
            .step(
                E::CancelRequest { cookie: C1 },
                [F::EndHelper { conv: 1 }, F::Reply { cookie: C1, authorized: false }, F::CloseDialog { cookie: C1 }],
                S::Idle,
            ),
        Trace::new(
            "new-request-replaces-the-open-one",
            "a second Begin cancels the first; a duplicate cookie is refused",
            c,
        )
        .step(begin(C1), opened(C1, 1, 0), S::Starting)
        .step(begin(C1), [], S::Starting)
        .step(
            E::Begin { cookie: Cookie(9), identities: 0, preferred: 0, screen: true },
            [F::Reply { cookie: Cookie(9), authorized: false }],
            S::Starting,
        )
        .step(
            E::Begin { cookie: C2, identities: 2, preferred: 1, screen: true },
            [
                F::EndHelper { conv: 1 },
                F::Reply { cookie: C1, authorized: false },
                F::CloseDialog { cookie: C1 },
                F::OpenDialog { cookie: C2 },
                F::StartHelper { conv: 2, identity: 1 },
            ],
            S::Starting,
        )
        .step(
            E::Begin { cookie: Cookie(3), identities: 2, preferred: 7, screen: false },
            [
                F::EndHelper { conv: 2 },
                F::Reply { cookie: C2, authorized: false },
                F::CloseDialog { cookie: C2 },
                F::Reply { cookie: Cookie(3), authorized: false },
            ],
            S::Idle,
        )
        .step(
            E::Begin { cookie: Cookie(4), identities: 2, preferred: 7, screen: true },
            opened(Cookie(4), 3, 0),
            S::Starting,
        ),
        Trace::new(
            "refused-without-a-screen-or-anyone",
            "no focused screen, or no identity: not authorized at once",
            c,
        )
        .step(
            E::Begin { cookie: C1, identities: 1, preferred: 0, screen: false },
            [F::Reply { cookie: C1, authorized: false }],
            S::Idle,
        )
        .step(
            E::Begin { cookie: C2, identities: 0, preferred: 0, screen: true },
            [F::Reply { cookie: C2, authorized: false }],
            S::Idle,
        ),
        Trace::new("authority-change-ends-it", "a new polkit authority: the open request is not authorized", c)
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(prompt(1), [SHOWN], S::Prompted)
            .step(E::Submit, [F::WriteAnswer { conv: 1 }], S::Checking)
            .step(
                E::AuthorityChanged,
                [F::EndHelper { conv: 1 }, F::Reply { cookie: C1, authorized: false }, F::CloseDialog { cookie: C1 }],
                S::Idle,
            )
            .step(E::AuthorityChanged, [], S::Idle),
        Trace::new("pam-messages-and-success-without-prompt", "info and errors show; SUCCESS needs no prompt", c)
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(E::PamInfo { conv: 1 }, [F::ShowNote(Note::PamInfo)], S::Starting)
            .step(E::PamError { conv: 1 }, [F::ShowNote(Note::PamError)], S::Starting)
            .step(
                E::Success { conv: 1 },
                [F::EndHelper { conv: 1 }, F::Reply { cookie: C1, authorized: true }, F::CloseDialog { cookie: C1 }],
                S::Idle,
            ),
        // Confirmed bugs (SYB-09 in aizome-sys, SHB-03 in the shell), as the
        // correct behaviour: no automatic retry without a delay, and a limit.
        Trace::new(
            "unreachable-helper-backs-off",
            "the helper's socket is missing: retries wait 500 ms then 1 s, then the dialog says it's unavailable",
            c,
        )
        .bug("SHB-03")
        .step(begin(C1), opened(C1, 1, 0), S::Starting)
        .step(E::Unreachable { conv: 1 }, [F::ShowNote(Note::Retrying), F::ArmRetry { ms: 500 }], S::RetryWait)
        .step(E::RetryTimer, [F::StartHelper { conv: 2, identity: 0 }], S::Starting)
        .step(E::Unreachable { conv: 2 }, [F::ShowNote(Note::Retrying), F::ArmRetry { ms: 1000 }], S::RetryWait)
        .step(E::Submit, [], S::RetryWait)
        .step(E::Unreachable { conv: 2 }, [], S::RetryWait)
        .step(E::RetryTimer, [F::StartHelper { conv: 3, identity: 0 }], S::Starting)
        .step(E::Unreachable { conv: 3 }, [F::ShowNote(Note::Unavailable)], S::Unavailable)
        .step(E::RetryTimer, [], S::Unavailable)
        .step(E::Submit, [], S::Unavailable)
        .step(E::Dismiss, [F::Reply { cookie: C1, authorized: false }, F::CloseDialog { cookie: C1 }], S::Idle),
        Trace::new(
            "helper-exits-without-a-verdict-backs-off",
            "a broken helper exits with no SUCCESS or FAILURE and no prompt: back off, don't respawn at once",
            c,
        )
        .bug("SYB-09")
        .step(begin(C1), opened(C1, 1, 0), S::Starting)
        .step(
            E::Failure { conv: 1 },
            [F::EndHelper { conv: 1 }, F::ShowNote(Note::Retrying), F::ArmRetry { ms: 500 }],
            S::RetryWait,
        )
        .step(E::RetryTimer, [F::StartHelper { conv: 2, identity: 0 }], S::Starting)
        .step(
            E::Failure { conv: 2 },
            [F::EndHelper { conv: 2 }, F::ShowNote(Note::Retrying), F::ArmRetry { ms: 1000 }],
            S::RetryWait,
        )
        .step(E::RetryTimer, [F::StartHelper { conv: 3, identity: 0 }], S::Starting)
        .step(E::Failure { conv: 3 }, [F::EndHelper { conv: 3 }, F::ShowNote(Note::Unavailable)], S::Unavailable)
        .step(E::Failure { conv: 3 }, [], S::Unavailable),
        Trace::new("a-prompt-resets-the-count", "only failures in a row with no prompt count toward the limit", c)
            .bug("SYB-09")
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(
                E::Failure { conv: 1 },
                [F::EndHelper { conv: 1 }, F::ShowNote(Note::Retrying), F::ArmRetry { ms: 500 }],
                S::RetryWait,
            )
            .step(E::RetryTimer, [F::StartHelper { conv: 2, identity: 0 }], S::Starting)
            .step(prompt(2), [SHOWN], S::Prompted)
            .step(
                E::Failure { conv: 2 },
                [F::EndHelper { conv: 2 }, F::ShowNote(Note::Retrying), F::ArmRetry { ms: 500 }],
                S::RetryWait,
            ),
        Trace::new("waiting-retry-dismissed", "dismissing during the back-off cancels the timer", c)
            .bug("SYB-09")
            .step(begin(C1), opened(C1, 1, 0), S::Starting)
            .step(
                E::Failure { conv: 1 },
                [F::EndHelper { conv: 1 }, F::ShowNote(Note::Retrying), F::ArmRetry { ms: 500 }],
                S::RetryWait,
            )
            .step(
                E::Dismiss,
                [F::CancelRetry, F::Reply { cookie: C1, authorized: false }, F::CloseDialog { cookie: C1 }],
                S::Idle,
            )
            .step(E::RetryTimer, [], S::Idle),
        Trace::new(
            "choosing-an-identity-retries-by-hand",
            "after Unavailable, picking an identity starts afresh",
            Config { retry_base_ms: 100, max_unanswered: 1 },
        )
        .bug("SHB-03")
        .step(E::Begin { cookie: C1, identities: 2, preferred: 0, screen: true }, opened(C1, 1, 0), S::Starting)
        .step(E::Unreachable { conv: 1 }, [F::ShowNote(Note::Unavailable)], S::Unavailable)
        .step(E::ChooseIdentity(5), [], S::Unavailable)
        .step(E::ChooseIdentity(1), [F::StartHelper { conv: 2, identity: 1 }], S::Starting)
        .step(E::Unreachable { conv: 2 }, [F::ShowNote(Note::Unavailable)], S::Unavailable)
        .step(E::ChooseIdentity(0), [F::StartHelper { conv: 3, identity: 0 }], S::Starting)
        .step(prompt(3), [SHOWN], S::Prompted),
        Trace::new(
            "identity-change-during-the-back-off",
            "picking an identity cancels the retry timer and starts now",
            c,
        )
        .bug("SYB-09")
        .step(E::Begin { cookie: C1, identities: 2, preferred: 1, screen: true }, opened(C1, 1, 1), S::Starting)
        .step(E::Unreachable { conv: 1 }, [F::ShowNote(Note::Retrying), F::ArmRetry { ms: 500 }], S::RetryWait)
        .step(E::ChooseIdentity(0), [F::CancelRetry, F::StartHelper { conv: 2, identity: 0 }], S::Starting)
        .step(E::RetryTimer, [], S::Starting),
    ]
}
