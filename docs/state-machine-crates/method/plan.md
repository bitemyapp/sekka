# State machine crate evaluation: plan

## Question

Which Rust state-machine crate (or no crate) should Sekka use when it reifies
its High-priority implicit state machines? Rank candidates by expected value,
weighing performance, correctness, expressiveness and concision, with
maintenance risk from the survey (`survey.md`).

## Candidates

Baselines: `implicit` (the current code's shape: flags, Options, counters),
`enum` (plain enum + match, data in variants, `mem::replace`), `typestate`
(hand-written typestate with a runtime wrapper enum).

Crates (pin exact versions in `Cargo.toml`): statig 0.4.1, smlang 0.8.0,
stateforward-sml 2.0.0, finny 0.3.0, fsmentry (latest), statum 0.9.0,
state-machines 0.34.0, rust-fsm 0.8.0. If one fails to build or cannot express
a target at all, record that as a result, then try the survey's reserve for
the same design point.

## Targets (all Reify High in the catalog)

| Target | Catalog | Why |
|---|---|---|
| `pointer` | KIT-13 (+ UIW-01) | Hot: every pointer event in every Aizome app. Hover ⊃ press ⊃ drag ⊃ captured; click pairing; capture across a compositor-taken drag. |
| `wrap` | HCO-12 | Hottest: evaluated per printed cell in Hotaru. Pending wrap, autowrap mode, soft-wrap linkage. |
| `polkit` | SYB-09 (+ SHB-03) | Data in states, generation counters dropping stale conversations, helper failure, retry with backoff (the confirmed spin loop must be unrepresentable or at least absent). |
| `transfer` | HAKO-05 | Nested sub-machine with data (paths), conflict prompt, replace-aside with restore on failure, and the rule that a replaced item goes to the Trash unless permanent deletion was chosen. |
| `lock` | SHB-12 (+ SYB-12) | Hierarchy (Locked ⊃ auth stages), entry/exit actions, timers, external events (logind PrepareForSleep, outputs going), the sleep-delay handshake. |
| `install` | GEN-07 (+ GEN-06) | Linear phases with an irreversible point: what ordering can be enforced at compile time. |

## Workspace

A scratch workspace (later copied into the repository as
[`../eval/`](../eval/)), its own Cargo workspace, edition 2024:

- `spec/`: per target a module with `Event`, `Effect` (outputs: effects the
  machine asks for, such as arming a timer or starting a helper; no I/O) and
  a uniform runtime trait

      pub trait Machine: Sized {
          fn new(config: Config) -> Self;
          fn handle(&mut self, event: Event, out: &mut Vec<Effect>);
          fn state(&self) -> StateName;   // a small Copy enum for observation
      }

  plus `SPEC.md` (state chart, events, effects, invariants, the catalog's
  confirmed-bug scenarios) and conformance traces: sequences of events with
  the expected effects and states, including every confirmed-bug scenario
  for that machine expressed as the correct behaviour. A generic
  `conformance::<M: Machine>()` runs them.
- `impl-implicit/`, `impl-enum/`, `impl-typestate/`: the baselines.
- `impl-<crate>/` for each crate: all six targets, idiomatic for the crate.
- `compile-fail/`: per implementation, `trybuild` cases for each target's
  illegal operations listed in its `SPEC.md` (e.g. answer a prompt with no
  conversation; dispose of a set-aside item before deciding Trash vs delete;
  install the bootloader before copying; deliver a click on a release with no
  press). The result per case is "rejected at compile time", "rejected at
  runtime" or "accepted".
- `bench/`: criterion benchmarks over `Machine`: `pointer` replays a
  synthetic but realistic stream (1 kHz motion, presses, drags, enter/leave,
  compositor-taken drags), `wrap` feeds ~10M cells with control characters,
  mode changes and wraps at 80 and 200 columns, and every target has a
  trace-replay microbenchmark (ns per event). Also iai-callgrind instruction
  counts if valgrind is available, since the host is noisy.
- `metrics/`: a script that records lines of code per implementation and
  target (non-blank, non-comment, excluding tests), clean and incremental
  compile time per implementation crate, and the size of the machine types
  (`size_of`).

## Phases

1. Spec agent: builds the workspace, specs, traces, conformance, benches,
   metrics, the three baselines, and one example `impl-statig/pointer` to
   prove the harness works with a crate. Everything compiles and the
   baselines pass conformance.
2. Eight crate agents in parallel, one per crate, each in its own
   `impl-<crate>` directory with its own target dir: implement all six
   targets, pass conformance (or record exactly what could not be
   expressed), add compile-fail cases, and write `impl-<crate>/NOTES.md`
   (workarounds, what the crate made easy or hard, ergonomics, error messages,
   compile errors' quality).
3. Evaluation agent: runs conformance, compile-fail and metrics for all, then
   benchmarks serially, pinned (`taskset -c 3`, sibling 11 left idle), under
   `flock -o ~/.cache/hotaru-runnymede-benchmark.lock`, with the machine's
   load recorded; writes `results.md` and raw data.
4. Report: ranking by expected value with the evidence.
