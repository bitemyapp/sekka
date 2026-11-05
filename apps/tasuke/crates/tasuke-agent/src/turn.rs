//! The agent loop for one user turn.

use crate::agent::Control;
use crate::event::{Event, FinishReason, Usage};
use crate::ids::TurnNumber;
use crate::options::Options;
use crate::session::Session;

pub(crate) fn run(session: &Session, text: &str, options: Options, ctl: &Control) {
    let _ = (session, text, options);
    ctl.send(Event::TurnStarted { turn: TurnNumber(1) });
    ctl.send(Event::Error { text: "the agent loop is not written yet".into(), retryable: false });
    ctl.send(Event::TurnFinished { reason: FinishReason::Failed, usage: Usage::default(), decode_tps: None });
}
