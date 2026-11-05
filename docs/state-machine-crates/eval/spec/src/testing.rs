//! Conformance traces, their runner, and differential property tests.
//!
//! An implementation crate's `tests/spec.rs` calls, per target,
//!
//! ```ignore
//! conformance::<pointer::Spec, impl_x::pointer::Pointer>(&[]);
//! differential::<pointer::Spec, impl_x::pointer::Pointer, impl_enum::pointer::Pointer>();
//! ```
//!
//! `conformance` runs every trace and fails unless all pass, except the
//! names listed as expected failures (which must then fail). `differential`
//! drives the implementation and the reference (`impl-enum`) with the same
//! random environment and fails at the first step where their effects or
//! states differ, printing the events that led there (proptest shrinks
//! them).
//!
//! Environment variables:
//! - `SME_PROPTEST_CASES`: cases per differential test (default 256).
//! - `SME_RESULTS`: a file to append one JSON line per trace result to.

use std::fmt::Debug;
use std::panic::{AssertUnwindSafe, catch_unwind};

use proptest::prelude::*;
use proptest::test_runner::{Config as RunnerConfig, TestCaseError, TestRunner};

/// One machine's specification, as the runner sees it. Each target module
/// has a marker type `Spec` implementing this.
pub trait Target: 'static {
    const NAME: &'static str;
    type Config: Clone + Debug;
    type Event: Clone + Debug;
    type Effect: Clone + Debug + PartialEq;
    type State: Copy + Debug + PartialEq;
    /// Random environment state for the property tests.
    type Env;

    fn traces() -> Vec<Trace<Self>>;
    /// Configurations the property tests pick from.
    fn configs() -> Vec<Self::Config>;
    fn env(config: &Self::Config) -> Self::Env;
    /// The next event, given the reference machine's state and the effects
    /// of the previous step (so responses can answer requests).
    fn next(env: &mut Self::Env, tape: &mut Tape, state: Self::State, effects: &[Self::Effect]) -> Self::Event;
    /// A state the machine never leaves (a finished job): the property
    /// tests then start both machines afresh and go on.
    fn terminal(_state: Self::State) -> bool {
        false
    }
}

/// Any implementation of a target's `Machine` trait, seen uniformly.
/// Blanket-implemented in each target module; don't implement it by hand.
pub trait Driven<T: Target> {
    fn boot(config: T::Config) -> Self;
    fn step(&mut self, event: T::Event, out: &mut Vec<T::Effect>);
    fn observe(&self) -> T::State;
}

/// A sequence of events with the exact effects and state expected after each.
pub struct Trace<T: Target + ?Sized> {
    pub name: &'static str,
    pub about: &'static str,
    /// The catalog id when this trace is a confirmed-bug scenario, expressed
    /// as the correct behaviour.
    pub bug: Option<&'static str>,
    pub config: T::Config,
    pub steps: Vec<Step<T>>,
}

pub struct Step<T: Target + ?Sized> {
    pub event: T::Event,
    pub effects: Vec<T::Effect>,
    pub state: T::State,
}

impl<T: Target> Trace<T> {
    pub fn new(name: &'static str, about: &'static str, config: T::Config) -> Self {
        Self { name, about, bug: None, config, steps: Vec::new() }
    }

    /// Marks a confirmed-bug scenario.
    pub fn bug(mut self, id: &'static str) -> Self {
        self.bug = Some(id);
        self
    }

    pub fn step(mut self, event: T::Event, effects: impl IntoIterator<Item = T::Effect>, state: T::State) -> Self {
        self.steps.push(Step { event, effects: effects.into_iter().collect(), state });
        self
    }

    /// The events alone (benchmarks replay these).
    pub fn events(&self) -> impl Iterator<Item = &T::Event> {
        self.steps.iter().map(|s| &s.event)
    }
}

/// The outcome of one trace.
#[derive(Clone, Debug)]
pub struct TraceResult {
    pub name: &'static str,
    pub bug: Option<&'static str>,
    pub error: Option<String>,
}

fn panic_text(e: Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| e.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

fn run_one<T: Target, M: Driven<T>>(trace: &Trace<T>) -> Result<(), String> {
    let mut m = M::boot(trace.config.clone());
    let mut out = Vec::new();
    for (i, step) in trace.steps.iter().enumerate() {
        out.clear();
        m.step(step.event.clone(), &mut out);
        let state = m.observe();
        if out != step.effects || state != step.state {
            return Err(format!(
                "step {i}: {:?}\n    expected effects {:?} and state {:?}\n    got      effects {:?} and state {:?}",
                step.event, step.effects, step.state, out, state
            ));
        }
    }
    Ok(())
}

/// Runs every trace of `T` against `M`.
pub fn run<T: Target, M: Driven<T>>() -> Vec<TraceResult> {
    T::traces()
        .iter()
        .map(|t| {
            let error = match catch_unwind(AssertUnwindSafe(|| run_one::<T, M>(t))) {
                Ok(r) => r.err(),
                Err(p) => Some(format!("panicked: {}", panic_text(p))),
            };
            TraceResult { name: t.name, bug: t.bug, error }
        })
        .collect()
}

fn implementation<M>() -> &'static str {
    let n = std::any::type_name::<M>();
    n.split("::").next().unwrap_or(n)
}

fn record<T: Target, M>(results: &[TraceResult]) {
    let Ok(path) = std::env::var("SME_RESULTS") else { return };
    use std::io::Write;
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
    for r in results {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        // One write per line: tests run in parallel and append to one file.
        let line = format!(
            "{{\"impl\":\"{}\",\"target\":\"{}\",\"trace\":\"{}\",\"bug\":{},\"pass\":{},\"error\":{}}}\n",
            implementation::<M>(),
            T::NAME,
            r.name,
            r.bug.map_or("null".into(), |b| format!("\"{b}\"")),
            r.error.is_none(),
            r.error.as_deref().map_or("null".into(), |e| format!("\"{}\"", esc(e))),
        );
        let _ = f.write_all(line.as_bytes());
    }
}

/// Runs the traces and fails unless every one passes, except those named in
/// `expected_failures`, each of which must fail. The baseline
/// `impl-implicit` lists its confirmed-bug traces there; a crate
/// implementation lists only what it could not express (say why in
/// `NOTES.md`).
pub fn conformance<T: Target, M: Driven<T>>(expected_failures: &[&str]) {
    let results = run::<T, M>();
    record::<T, M>(&results);
    let passed = results.iter().filter(|r| r.error.is_none()).count();
    eprintln!("conformance {} {}: {passed}/{} traces pass", T::NAME, implementation::<M>(), results.len());
    let names: Vec<&str> = results.iter().map(|r| r.name).collect();
    let mut problems = Vec::new();
    for x in expected_failures {
        if !names.contains(x) {
            problems.push(format!("expected failure {x:?} names no trace"));
        }
    }
    for r in &results {
        let expected = expected_failures.contains(&r.name);
        match (&r.error, expected) {
            (Some(e), false) => problems.push(format!("trace {:?} failed:\n  {e}", r.name)),
            (None, true) => problems.push(format!("trace {:?} was expected to fail but passed", r.name)),
            (Some(e), true) => eprintln!(
                "  expected failure {}{}: {}",
                r.name,
                r.bug.map(|b| format!(" ({b})")).unwrap_or_default(),
                e.lines().next().unwrap_or("")
            ),
            (None, false) => {}
        }
    }
    if !problems.is_empty() {
        panic!("{} conformance for {}:\n{}", T::NAME, implementation::<M>(), problems.join("\n"));
    }
}

/// The names of `T`'s confirmed-bug traces (the expected failures of a
/// baseline that keeps the bugs).
pub fn bug_traces<T: Target>() -> Vec<&'static str> {
    T::traces().iter().filter(|t| t.bug.is_some()).map(|t| t.name).collect()
}

/// A source of choices for the random environment: proptest generates the
/// tape, so shrinking it shortens and simplifies the event stream. An
/// exhausted tape reads as zeros.
pub struct Tape {
    choices: Vec<u32>,
    at: usize,
}

impl Tape {
    pub fn new(choices: Vec<u32>) -> Self {
        Self { choices, at: 0 }
    }
    pub fn exhausted(&self) -> bool {
        self.at >= self.choices.len()
    }
    pub fn raw(&mut self) -> u32 {
        let c = self.choices.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        c
    }
    /// In `0..n`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.raw() % n }
    }
    /// True with probability about `per_mille / 1000`.
    pub fn chance(&mut self, per_mille: u32) -> bool {
        self.below(1000) < per_mille
    }
    pub fn pick<'a, X>(&mut self, xs: &'a [X]) -> &'a X {
        &xs[self.below(xs.len() as u32) as usize]
    }
}

fn cases() -> u32 {
    std::env::var("SME_PROPTEST_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(256)
}

/// The longest event stream a property-test case drives.
pub const MAX_EVENTS: usize = 200;

/// Events so far; `None` marks where both machines started afresh.
struct History<E>(Vec<Option<E>>);

impl<E> History<E> {
    fn push(&mut self, e: E) {
        self.0.push(Some(e));
    }
    fn push_marker(&mut self) {
        self.0.push(None);
    }
    fn len(&self) -> usize {
        self.0.iter().filter(|e| e.is_some()).count()
    }
}

fn differ<T: Target, A: Driven<T>, R: Driven<T>>(config: T::Config, tape: Vec<u32>) -> Result<(), TestCaseError> {
    let mut tape = Tape::new(tape);
    let mut env = T::env(&config);
    let mut a = A::boot(config.clone());
    let mut r = R::boot(config.clone());
    let (mut out_a, mut out_r) = (Vec::new(), Vec::new());
    let mut history = History(Vec::new());
    while !tape.exhausted() && history.len() < MAX_EVENTS {
        let event = T::next(&mut env, &mut tape, r.observe(), &out_r);
        history.push(event.clone());
        out_a.clear();
        out_r.clear();
        a.step(event.clone(), &mut out_a);
        r.step(event, &mut out_r);
        let (sa, sr) = (a.observe(), r.observe());
        if out_a != out_r || sa != sr {
            let shown: Vec<String> =
                history.0.iter().map(|e| e.as_ref().map_or("--".into(), |e| format!("{e:?}"))).collect();
            return Err(TestCaseError::fail(format!(
                "{} diverged from impl-enum after {} events (config {config:?}; `--` marks a fresh start):\n  {}\n  implementation: {out_a:?} -> {sa:?}\n  impl-enum:      {out_r:?} -> {sr:?}",
                T::NAME,
                history.len(),
                shown.join("\n  "),
            )));
        }
        if T::terminal(sr) {
            a = A::boot(config.clone());
            r = R::boot(config.clone());
            env = T::env(&config);
            out_r.clear();
            history.push_marker();
        }
    }
    Ok(())
}

/// How often each state (by its `Debug` text) is reached, and each effect
/// variant emitted (as `effect <Variant>`), when `R` is driven by the random
/// environment: checks that the property tests reach every state and effect.
pub fn coverage<T: Target, R: Driven<T>>(cases: u32) -> std::collections::BTreeMap<String, u64> {
    let mut seen = std::collections::BTreeMap::new();
    let mut rng = crate::SplitMix(0x5eed);
    let configs = T::configs();
    for case in 0..cases {
        let config = configs[case as usize % configs.len()].clone();
        let choices = (0..MAX_EVENTS * 3).map(|_| rng.next_u64() as u32).collect();
        let mut tape = Tape::new(choices);
        let mut env = T::env(&config);
        let mut r = R::boot(config.clone());
        let mut out = Vec::new();
        for _ in 0..MAX_EVENTS {
            let event = T::next(&mut env, &mut tape, r.observe(), &out);
            out.clear();
            r.step(event, &mut out);
            *seen.entry(format!("{:?}", r.observe())).or_insert(0) += 1;
            for e in &out {
                let text = format!("{e:?}");
                let name = text.split([' ', '(', '{']).next().unwrap_or("").to_string();
                *seen.entry(format!("effect {name}")).or_insert(0) += 1;
            }
            if T::terminal(r.observe()) {
                r = R::boot(config.clone());
                env = T::env(&config);
                out.clear();
            }
        }
    }
    seen
}

/// Property test: `A` and the reference `R` (impl-enum) produce the same
/// effects and states on random environments.
pub fn differential<T: Target, A: Driven<T>, R: Driven<T>>() {
    let configs = T::configs();
    let mut runner =
        TestRunner::new(RunnerConfig { cases: cases(), failure_persistence: None, ..RunnerConfig::default() });
    let strategy = (0..configs.len(), proptest::collection::vec(any::<u32>(), 0..MAX_EVENTS * 3));
    let (cases_run, events) = (std::cell::Cell::new(0u64), std::cell::Cell::new(0u64));
    let result = runner.run(&strategy, |(c, tape)| {
        cases_run.set(cases_run.get() + 1);
        events.set(events.get() + tape.len() as u64);
        differ::<T, A, R>(configs[c].clone(), tape)
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    eprintln!("differential {}: {} cases, {} tape choices", T::NAME, cases_run.get(), events.get());
}
