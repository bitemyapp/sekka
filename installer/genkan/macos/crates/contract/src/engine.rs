// SPDX-License-Identifier: GPL-3.0-or-later
//! The engine's transcript: newline-separated canonical JSON messages on its
//! standard output, read as it runs.
//!
//! The engine is Asahi's installer with Sekka's adapter, run with a fixed
//! command line in one of two modes: `inspect` reports an [`Inspection`]
//! and changes nothing; `execute` carries out one plan. In `execute` mode,
//! before each stage it writes an [`EngineBody::Intent`] and waits for the
//! helper to answer `{"ack":SEQ}` on its standard input: the helper
//! answers only after the intent is durable in its journal, so no change
//! can happen that the journal doesn't expect. [`Transcript`] enforces the
//! order of messages; anything out of order ends the run.
use serde::{Deserialize, Serialize};

use crate::{
    Digest, Error, Fact, Failure, Inspection, Stage, Text, Verification, Version, canonical,
    invalid, limits, schema_tag,
};

schema_tag!(EngineSchema, "sekka.mac.engine/1");

/// The protocol version the app, helper and engine speak.
pub const ENGINE_PROTOCOL: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineMessage {
    pub schema: EngineSchema,
    pub seq: u64,
    pub message: EngineBody,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EngineBody {
    Hello {
        /// `0.9.2+sekka.1`.
        engine: Text,
        asahi: Version,
        /// The digest of Sekka's adapter source, as packaged.
        adapter: Digest,
        mode: EngineMode,
        protocol: u32,
    },
    Inspection(Box<Inspection>),
    Intent {
        stage: Stage,
        plan: Digest,
    },
    Checkpoint {
        stage: Stage,
        plan: Digest,
        facts: Vec<Fact>,
        verification: Verification,
    },
    Progress {
        stage: Stage,
        done: u64,
        total: u64,
    },
    Log {
        level: LogLevel,
        text: Text,
    },
    Failure {
        stage: Option<Stage>,
        failure: Failure,
    },
    Completion {
        outcome: EngineOutcome,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineMode {
    Inspect,
    Execute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineOutcome {
    Inspected,
    /// macOS's part is finished and verified; Recovery is next.
    WaitingForRecovery,
    Failed,
}

/// The helper's answer to an intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub ack: u64,
}

/// The stages `execute` runs in macOS, in order.
pub fn macos_stages(shrinks: bool) -> Vec<Stage> {
    Stage::ORDER
        .iter()
        .copied()
        .filter(|s| *s != Stage::Import && !s.in_recovery() && (shrinks || *s != Stage::Shrink))
        .collect()
}

/// Reads a transcript message by message and refuses any departure from the
/// protocol: wrong sequence, schema, mode, plan, or stage order, or anything
/// after completion.
#[derive(Debug)]
pub struct Transcript {
    mode: EngineMode,
    plan: Option<Digest>,
    stages: Vec<Stage>,
    next_seq: u64,
    hello: bool,
    inspection: bool,
    open: Option<Stage>,
    done: usize,
    failed: bool,
    outcome: Option<EngineOutcome>,
}

impl Transcript {
    /// Expect an `inspect` run.
    pub fn inspect() -> Self {
        Self::new(EngineMode::Inspect, None, Vec::new())
    }

    /// Expect an `execute` run of the plan with this digest.
    pub fn execute(plan: Digest, shrinks: bool) -> Self {
        Self::new(EngineMode::Execute, Some(plan), macos_stages(shrinks))
    }

    fn new(mode: EngineMode, plan: Option<Digest>, stages: Vec<Stage>) -> Self {
        Self {
            mode,
            plan,
            stages,
            next_seq: 1,
            hello: false,
            inspection: false,
            open: None,
            done: 0,
            failed: false,
            outcome: None,
        }
    }

    /// The stage started and not finished, if any.
    pub fn open_stage(&self) -> Option<Stage> {
        self.open
    }

    pub fn outcome(&self) -> Option<EngineOutcome> {
        self.outcome
    }

    /// Accept one line (without its newline).
    pub fn accept(&mut self, line: &[u8]) -> Result<EngineMessage, Error> {
        let message: EngineMessage =
            canonical::from_slice(line, limits::ENGINE_LINE, EngineSchema::VALUE)?;
        self.check(&message)?;
        Ok(message)
    }

    fn check(&mut self, m: &EngineMessage) -> Result<(), Error> {
        if self.outcome.is_some() {
            return Err(invalid("the engine wrote after it finished"));
        }
        if m.seq != self.next_seq {
            return Err(invalid(format!(
                "engine message {} arrived where {} was expected",
                m.seq, self.next_seq
            )));
        }
        self.next_seq += 1;
        let plan_ok = |p: &Digest| Some(*p) == self.plan;
        match &m.message {
            EngineBody::Hello { mode, protocol, .. } => {
                if self.hello || *mode != self.mode || *protocol != ENGINE_PROTOCOL {
                    return Err(invalid("the engine's greeting doesn't match this run"));
                }
                self.hello = true;
                return Ok(());
            }
            _ if !self.hello => return Err(invalid("the engine didn't greet first")),
            EngineBody::Log { .. } => {}
            EngineBody::Inspection(inspection) => {
                if self.mode != EngineMode::Inspect || self.inspection {
                    return Err(invalid("an inspection arrived outside an inspect run"));
                }
                inspection.check()?;
                self.inspection = true;
            }
            EngineBody::Intent { stage, plan } => {
                if self.failed || self.open.is_some() || !plan_ok(plan) {
                    return Err(invalid("an intent arrived out of turn"));
                }
                if self.stages.get(self.done) != Some(stage) {
                    return Err(invalid(format!("{stage:?} started out of order")));
                }
                self.open = Some(*stage);
            }
            EngineBody::Progress { stage, done, total } => {
                if self.open != Some(*stage) || done > total {
                    return Err(invalid("progress arrived for a stage that isn't running"));
                }
            }
            EngineBody::Checkpoint { stage, plan, .. } => {
                if self.open != Some(*stage) || !plan_ok(plan) {
                    return Err(invalid("a checkpoint arrived for a stage that isn't running"));
                }
                self.open = None;
                self.done += 1;
            }
            EngineBody::Failure { stage, .. } => {
                if self.failed || *stage != self.open && self.mode == EngineMode::Execute {
                    return Err(invalid("a failure arrived for a stage that isn't running"));
                }
                self.open = None;
                self.failed = true;
            }
            EngineBody::Completion { outcome } => {
                let ok = match (self.mode, outcome) {
                    (_, EngineOutcome::Failed) => self.failed,
                    (EngineMode::Inspect, EngineOutcome::Inspected) => self.inspection && !self.failed,
                    (EngineMode::Execute, EngineOutcome::WaitingForRecovery) => {
                        !self.failed && self.open.is_none() && self.done == self.stages.len()
                    }
                    _ => false,
                };
                if !ok {
                    return Err(invalid("the engine's outcome doesn't match what it did"));
                }
                self.outcome = Some(*outcome);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiskChanged, Fact};

    struct W {
        seq: u64,
        lines: Vec<Vec<u8>>,
    }
    impl W {
        fn new() -> Self {
            W { seq: 0, lines: vec![] }
        }
        fn push(&mut self, message: EngineBody) -> &mut Self {
            self.seq += 1;
            self.lines.push(
                canonical::to_vec(&EngineMessage {
                    schema: EngineSchema,
                    seq: self.seq,
                    message,
                })
                .unwrap(),
            );
            self
        }
        fn hello(&mut self, mode: EngineMode) -> &mut Self {
            self.push(EngineBody::Hello {
                engine: "0.9.2+sekka.1".parse().unwrap(),
                asahi: "0.9.2".parse().unwrap(),
                adapter: Digest::of(b"adapter"),
                mode,
                protocol: ENGINE_PROTOCOL,
            })
        }
        fn run(&self, mut t: Transcript) -> Result<Transcript, Error> {
            for line in &self.lines {
                t.accept(line)?;
            }
            Ok(t)
        }
    }

    fn plan() -> Digest {
        Digest::of(b"plan")
    }

    fn stage(w: &mut W, s: Stage) {
        w.push(EngineBody::Intent { stage: s, plan: plan() });
        w.push(EngineBody::Progress { stage: s, done: 1, total: 2 });
        w.push(EngineBody::Checkpoint {
            stage: s,
            plan: plan(),
            facts: vec![Fact::new("ok", 1).unwrap()],
            verification: Verification::Inspected,
        });
    }

    #[test]
    fn an_inspection_run() {
        let mut w = W::new();
        w.hello(EngineMode::Inspect)
            .push(EngineBody::Inspection(Box::new(crate::inspection::tests::sample())))
            .push(EngineBody::Completion { outcome: EngineOutcome::Inspected });
        assert_eq!(w.run(Transcript::inspect()).unwrap().outcome(), Some(EngineOutcome::Inspected));
        // Nothing may change in inspect mode.
        let mut w = W::new();
        w.hello(EngineMode::Inspect);
        stage(&mut w, Stage::Shrink);
        assert!(w.run(Transcript::inspect()).is_err());
    }

    #[test]
    fn a_complete_execution() {
        let mut w = W::new();
        w.hello(EngineMode::Execute);
        for s in macos_stages(true) {
            stage(&mut w, s);
        }
        w.push(EngineBody::Completion { outcome: EngineOutcome::WaitingForRecovery });
        let t = w.run(Transcript::execute(plan(), true)).unwrap();
        assert_eq!(t.outcome(), Some(EngineOutcome::WaitingForRecovery));
    }

    #[test]
    fn early_completion_and_skipped_stages_are_refused() {
        let mut w = W::new();
        w.hello(EngineMode::Execute);
        stage(&mut w, Stage::Shrink);
        w.push(EngineBody::Completion { outcome: EngineOutcome::WaitingForRecovery });
        assert!(w.run(Transcript::execute(plan(), true)).is_err());

        let mut w = W::new();
        w.hello(EngineMode::Execute);
        stage(&mut w, Stage::StubPartition);
        assert!(w.run(Transcript::execute(plan(), true)).is_err(), "the shrink was skipped");
        assert!(w.run(Transcript::execute(plan(), false)).is_ok(), "no shrink was planned");
    }

    #[test]
    fn a_different_plan_or_sequence_is_refused() {
        let mut w = W::new();
        w.hello(EngineMode::Execute).push(EngineBody::Intent {
            stage: Stage::Shrink,
            plan: Digest::of(b"another"),
        });
        assert!(w.run(Transcript::execute(plan(), true)).is_err());

        let mut w = W::new();
        w.hello(EngineMode::Execute);
        w.seq += 1;
        stage(&mut w, Stage::Shrink);
        assert!(w.run(Transcript::execute(plan(), true)).is_err());
    }

    #[test]
    fn a_failure_ends_the_run() {
        let failure = Failure {
            code: "resize".parse().unwrap(),
            message: "Apple's resize failed.".parse().unwrap(),
            apple_diagnostic: Some("Error -69606".parse().unwrap()),
            disk_changed: DiskChanged::No,
        };
        let mut w = W::new();
        w.hello(EngineMode::Execute)
            .push(EngineBody::Intent { stage: Stage::Shrink, plan: plan() })
            .push(EngineBody::Failure {
                stage: Some(Stage::Shrink),
                failure: failure.clone(),
            })
            .push(EngineBody::Log {
                level: LogLevel::Error,
                text: "diskutil said so".parse().unwrap(),
            })
            .push(EngineBody::Completion { outcome: EngineOutcome::Failed });
        assert_eq!(
            w.run(Transcript::execute(plan(), true)).unwrap().outcome(),
            Some(EngineOutcome::Failed)
        );
        let mut more = W::new();
        more.lines = w.lines[..3].to_vec();
        more.seq = 3;
        more.push(EngineBody::Intent { stage: Stage::StubPartition, plan: plan() });
        assert!(more.run(Transcript::execute(plan(), true)).is_err());
    }

    #[test]
    fn non_canonical_lines_are_refused() {
        let mut t = Transcript::inspect();
        let line = br#"{"message":{"payload":{"adapter":"sha256:0000000000000000000000000000000000000000000000000000000000000000","asahi":"0.9.2","engine":"e","mode":"inspect","protocol":1},"type":"hello"}, "schema":"sekka.mac.engine/1","seq":1}"#;
        assert!(matches!(t.accept(line), Err(Error::NotCanonical)));
    }
}
