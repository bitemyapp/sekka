//! A turn played from a script: what the fixtures' agent does, event by
//! event, with the time between them, so the window can be seen working
//! (streaming, looking, asking) before the agent loop exists, and in
//! pictures that don't change from run to run.
//!
//! A script stops at a call that asks and goes on once the person
//! decides, with what it scripted for Run or for Don't run.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::agent::{Approval, Decision, Event, FinishReason, Outcome, ToolCall, ToolOutput, Usage};

/// One event and the pause before it.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub after: Duration,
    pub event: Event,
}

/// A turn's events, and what follows each decision.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    steps: VecDeque<Step>,
    on_allow: HashMap<String, Vec<Step>>,
    on_deny: HashMap<String, Vec<Step>>,
}

/// Builds a script as a turn would happen.
#[derive(Clone, Debug, Default)]
pub struct Builder {
    script: Script,
    /// Where steps go: the turn, or a decision's continuation.
    into: Into,
    /// Milliseconds a token takes to stream.
    pace: u64,
}

#[derive(Clone, Debug, Default)]
enum Into {
    #[default]
    Turn,
    Allow(String),
    Deny(String),
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

impl Builder {
    pub fn new() -> Builder {
        Builder { pace: 16, ..Builder::default() }
    }

    fn push(&mut self, after: Duration, event: Event) {
        let step = Step { after, event };
        match &self.into {
            Into::Turn => self.script.steps.push_back(step),
            Into::Allow(id) => self.script.on_allow.entry(id.clone()).or_default().push(step),
            Into::Deny(id) => self.script.on_deny.entry(id.clone()).or_default().push(step),
        }
    }

    pub fn event(mut self, after_ms: u64, event: Event) -> Builder {
        self.push(ms(after_ms), event);
        self
    }

    pub fn start(self) -> Builder {
        self.event(120, Event::TurnStarted { turn: 1 })
    }

    /// Text streamed a few characters at a time, as tokens come.
    fn stream(mut self, text: &str, reasoning: bool) -> Builder {
        let n = text.chars().count();
        let mut chunk = String::new();
        let mut first = true;
        for (i, ch) in text.chars().enumerate() {
            chunk.push(ch);
            // About four characters a token, cut at spaces where it can.
            if (chunk.len() >= 4 && ch == ' ') || chunk.len() >= 7 || i + 1 == n {
                let t = std::mem::take(&mut chunk);
                let event = if reasoning { Event::ReasoningDelta { text: t } } else { Event::TextDelta { text: t } };
                let wait = if first { 260 } else { self.pace };
                first = false;
                self.push(ms(wait), event);
            }
        }
        self
    }

    pub fn think(mut self, text: &str) -> Builder {
        // Thinking streams as fast, but there's more of it than shows.
        let pace = std::mem::replace(&mut self.pace, 70);
        let mut b = self.stream(text, true);
        b.pace = pace;
        b
    }

    pub fn say(self, text: &str) -> Builder {
        self.stream(text, false)
    }

    /// A call that only looks, and its output after `took_ms`.
    pub fn look(
        self,
        id: &str,
        name: &str,
        args: serde_json::Value,
        output: &str,
        exit: Option<i32>,
        took_ms: u64,
    ) -> Builder {
        let output = ToolOutput { text: output.to_owned(), exit, ..ToolOutput::default() };
        self.event(180, Event::ToolCall { id: id.into(), call: ToolCall::new(name, args), needs: Approval::None })
            .event(30, Event::ToolStarted { id: id.into() })
            .event(took_ms, Event::ToolOutput { id: id.into(), output })
    }

    /// A call that asks; the steps after it until [`Builder::then`] are
    /// what Run leads to, then [`Builder::otherwise`]'s what Don't run does.
    pub fn ask(mut self, id: &str, name: &str, args: serde_json::Value, needs: Approval) -> Builder {
        self.push(ms(220), Event::ToolCall { id: id.into(), call: ToolCall::new(name, args), needs });
        self.into = Into::Allow(id.to_owned());
        self
    }

    /// Run was pressed: it starts, and gives `output`.
    pub fn runs(self, id: &str, output: &str, exit: Option<i32>, took_ms: u64) -> Builder {
        let output = ToolOutput { text: output.to_owned(), exit, ..ToolOutput::default() };
        self.event(60, Event::ToolStarted { id: id.into() }).event(took_ms, Event::ToolOutput { id: id.into(), output })
    }

    /// What Don't run leads to (it begins with the refusal reported back).
    pub fn otherwise(mut self, id: &str) -> Builder {
        self.into = Into::Deny(id.to_owned());
        let output = ToolOutput { outcome: Outcome::Denied, ..ToolOutput::default() };
        self.event(40, Event::ToolOutput { id: id.into(), output })
    }

    /// Back to the turn's own steps.
    pub fn then(mut self) -> Builder {
        self.into = Into::Turn;
        self
    }

    pub fn finish(self, decode_tps: f32) -> Builder {
        let usage = Usage { prompt_tokens: 9_412, completion_tokens: 611 };
        self.event(80, Event::TurnFinished { reason: FinishReason::Done, usage, decode_tps: Some(decode_tps) })
    }

    pub fn build(self) -> Script {
        self.script
    }
}

/// A script being played.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    script: Script,
    /// The call it waits on, if it does.
    waiting: Option<String>,
    /// Bumped when it's stopped, so timers already set don't play on.
    pub generation: u64,
}

impl Player {
    pub fn new(script: Script) -> Player {
        Player { script, waiting: None, generation: 0 }
    }

    /// How long until the next event; `None` when it waits for a
    /// decision or has ended.
    pub fn next_in(&self) -> Option<Duration> {
        if self.waiting.is_some() {
            return None;
        }
        self.script.steps.front().map(|s| s.after)
    }

    /// The next event, now due.
    pub fn advance(&mut self) -> Option<Event> {
        if self.waiting.is_some() {
            return None;
        }
        let step = self.script.steps.pop_front()?;
        if let Event::ToolCall { id, needs, .. } = &step.event
            && needs.asks()
        {
            self.waiting = Some(id.clone());
        }
        Some(step.event)
    }

    /// Every event up to the first that waits (or the end), at once: a
    /// fixture's starting state.
    pub fn fast_forward(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(e) = self.advance() {
            out.push(e);
        }
        out
    }

    /// The person decided: the script goes on with what it has for that.
    pub fn decide(&mut self, id: &str, decision: &Decision) {
        if self.waiting.as_deref() != Some(id) {
            return;
        }
        self.waiting = None;
        let more = match decision {
            Decision::Allow => self.script.on_allow.remove(id),
            Decision::Deny { .. } => self.script.on_deny.remove(id),
        };
        if let Some(more) = more {
            for s in more.into_iter().rev() {
                self.script.steps.push_front(s);
            }
        }
    }

    /// Stop: what's left is dropped and the turn ends as stopped.
    pub fn cancel(&mut self) -> Vec<Event> {
        self.generation += 1;
        self.script.steps.clear();
        self.waiting = None;
        vec![Event::TurnFinished { reason: FinishReason::Cancelled, usage: Usage::default(), decode_tps: None }]
    }

    pub fn is_done(&self) -> bool {
        self.waiting.is_none() && self.script.steps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::transcript::{Body, ToolState, Transcript};

    fn script() -> Script {
        Builder::new()
            .start()
            .say("I'll check.")
            .look("a", "run_command", json!({"command": "wpctl status"}), "Sinks: muted", Some(0), 300)
            .ask(
                "b",
                "run_command",
                json!({"command": "wpctl set-mute @DEFAULT_AUDIO_SINK@ 0"}),
                Approval::Ask { reason: "It changes a setting".into() },
            )
            .runs("b", "", Some(0), 100)
            .say("Unmuted.")
            .finish(70.0)
            .otherwise("b")
            .say("Left as it is.")
            .finish(70.0)
            .then()
            .build()
    }

    fn play(p: &mut Player, t: &mut Transcript) {
        let mut at = 0;
        while let Some(d) = p.next_in() {
            at += d.as_millis() as i64;
            let e = p.advance().unwrap();
            t.apply(e, at);
        }
    }

    #[test]
    fn it_streams_then_waits_for_the_decision() {
        let mut p = Player::new(script());
        let mut t = Transcript::new();
        play(&mut p, &mut t);
        assert!(t.pending_approval().is_some());
        assert!(t.busy);
        assert_eq!(p.next_in(), None, "waits");
        t.decide("b", Decision::Allow);
        p.decide("b", &Decision::Allow);
        play(&mut p, &mut t);
        assert!(p.is_done() && !t.busy);
        assert_eq!(t.tool("b").unwrap().state, ToolState::Done { exit: Some(0) });
        let Some(Body::Answer(a)) = t.items.last().map(|i| &i.body) else { panic!() };
        assert_eq!(a.text, "Unmuted.");
    }

    #[test]
    fn dont_run_follows_the_other_branch() {
        let mut p = Player::new(script());
        let mut t = Transcript::new();
        for e in p.fast_forward() {
            t.apply(e, 0);
        }
        let d = Decision::Deny { note: None };
        t.decide("b", d.clone());
        p.decide("b", &d);
        play(&mut p, &mut t);
        assert_eq!(t.tool("b").unwrap().state, ToolState::Denied);
        let Some(Body::Answer(a)) = t.items.last().map(|i| &i.body) else { panic!() };
        assert_eq!(a.text, "Left as it is.");
    }

    #[test]
    fn stopping_drops_the_rest() {
        let mut p = Player::new(script());
        let mut t = Transcript::new();
        p.advance();
        let g = p.generation;
        for e in p.cancel() {
            t.apply(e, 0);
        }
        assert!(p.is_done());
        assert_ne!(p.generation, g);
    }
}
