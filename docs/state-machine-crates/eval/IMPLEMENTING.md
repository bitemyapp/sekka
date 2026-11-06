# Implementing the targets with a state-machine crate

This workspace compares ways of writing six of Sekka's state machines: three
plain-Rust baselines and eight crates. Your job is one crate: implement all
six targets with it, idiomatically, make them pass conformance and the
property tests, write the compile-fail probes, and record what you learned
in `NOTES.md`. Everything you need is in this file and in each target's
`SPEC.md`.

## The workspace

| Path | What it is |
|---|---|
| `spec/` | Per target (`src/<target>/`): `SPEC.md` (normative), the types and the `Machine` trait (`mod.rs`), conformance traces (`traces.rs`), the random environment for property tests (`env.rs`), and benchmark workloads (`workload.rs`, pointer and wrap). `src/testing.rs`: the runner. |
| `impl-implicit/` | Baseline: the current Sekka code's shape (flags, `Option`s, counters). Keeps the confirmed bugs behind `BUGS`; `--features fixed` turns them off. |
| `impl-enum/` | Baseline: plain `enum` + `match`, data in variants. **The reference** the property tests compare against. |
| `impl-typestate/` | Baseline: hand-written typestate (one type per state, consuming transitions, a runtime wrapper `Any`). |
| `impl-<crate>/` | One slot per crate: `statig`, `smlang`, `sml` (stateforward-sml, used as `sml`), `finny`, `fsmentry`, `statum`, `state-machines`, `rust-fsm`. Each starts with stubs; `impl-statig/src/pointer.rs` is done as a worked example. |
| `compile-fail/` | The trybuild runner, and the probes of each target's illegal operations per implementation (`cases/<impl>/<target>/…`); each implementation runs its own from `tests/compile_fail.rs`. |
| `bench/` | criterion benches (`benches/machines.rs`), gungraun instruction counts (`benches/instructions.rs`), `bin/smoke.rs`, `bin/sizes.rs`. The registry already lists every slot; each slot is a feature (all on by default). |
| `metrics/sme.py` | LoC, compile time, sizes, compile-fail classification, conformance summary. |

The targets (read the `SPEC.md` before you start each one):

| Target | Catalog | Shape | What to watch |
|---|---|---|---|
| `pointer` | KIT-13, UIW-01 | 6 states; `Inside` superstate owns the hover | Effect order (hover before delivery, except after a release); hot path |
| `wrap` | HCO-12 | pending × autowrap (4 states), context: cursor, links | Hottest path: per printed cell; `Repeat(n)` emits many effects |
| `polkit` | SYB-09, SHB-03 | `Active` superstate, conversation numbers, retry timer | Stale conversation events; back-off and `Unavailable` |
| `transfer` | HAKO-05 | per-item states, merge frames (nested sub-machine), aside data | Only the awaited response for the current item counts |
| `lock` | SHB-12, SYB-12 | two orthogonal regions; `Lockscreen ⊃ Locked ⊃ Checking`; entry/exit actions; shake timer | Exit-action order; delay token rules |
| `install` | GEN-07, GEN-06 | 20 linear states; irreversible point at `Erase` | Which events are ignored where |

## Your slot and the rules

- **You may edit** only `impl-<crate>/` (sources, `tests/spec.rs`, its
  `Cargo.toml`, `NOTES.md`) and `compile-fail/cases/<crate>/`. Other agents
  work in this workspace at the same time.
- **Don't** edit `spec/`, the baselines, `bench/`, `compile-fail/src` or
  `tests`, `metrics/`, the workspace `Cargo.toml`, or other slots. If the
  spec looks wrong or ambiguous, implement its most literal reading and
  write the question under "Spec questions" in `NOTES.md`.
- **Don't** commit, don't run full benchmarks (phase 3 does, pinned and
  serialized), don't use `unsafe`, and don't add dependencies beyond your
  crate and its features.
- The crate's version is pinned (`=x.y.z`) in your `Cargo.toml`. If it
  doesn't build, or can't express any target at all, use the survey's
  reserve closest to its design point (`moku`, `stateless`, `typed-fsm`,
  `edfsm`, `hsmc`) in the same slot, and say so at the top of `NOTES.md`.

## Building and testing

Every command goes through the wrapper, with **your own target directory**
(it isolates `HOME` but uses the real `~/.cargo` registry):

```sh
CARGO_TARGET_DIR=<scratch>/t/sme-<crate> ./isolate default <scratch> \
  bash -c 'cd docs/state-machine-crates/eval && cargo test --locked -p impl-<crate>'
```

The examples below show only the part inside `bash -c`.

- `cargo test --locked -p impl-<crate> --test spec`: conformance and
  differential tests for all six targets. Filter with `-- pointer`. Add `-- --nocapture` to see
  `conformance <target> <impl>: N/M traces pass`.
- `SME_PROPTEST_CASES=5000 cargo test --locked --release -p impl-<crate>`: a
  deeper property run before you finish (default 256 cases per target).
- `cargo fmt -p impl-<crate>` before counting lines (workspace `rustfmt.toml`).
- Keep your `Cargo.toml` valid at every moment: every cargo command in the
  workspace parses every member's manifest.
- **Cargo.lock** already holds all eight crates. Always pass `--locked`. If
  you must change dependencies (a crate feature that pulls an optional
  dependency, a reserve crate), update the lock once under the shared lock
  and say so in `NOTES.md`:
  `flock .cargo-lock cargo update -p <dep>` (or the
  same `flock` around one `cargo build` without `--locked`).

## Implementing a target

Each `src/<target>.rs` must export:

```rust
pub const IMPLEMENTED: bool = true;   // the stub says false
pub struct Pointer { … }              // the name is fixed: Pointer, Wrap, Polkit, Transfer, Lock, Install
impl spec::pointer::Machine for Pointer {
    fn new(config: Config) -> Self;
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>);
    fn state(&self) -> StateName;
}
```

Guidance:

- **The `SPEC.md` transition table is normative**, including the order of
  effects within a step and which events are ignored where. The traces
  check exact effect sequences and the `StateName` after every event.
  `impl-enum/src/<target>.rs` is a readable reference (~200 lines).
- **Use the crate's idiom**, not a hand-written `enum` inside a thin
  wrapper: its states, superstates, guards, actions, entry/exit hooks,
  state-local data, generated handles. Where the spec has hierarchy (pointer
  `Inside`, polkit `Active`, lock `Lockscreen ⊃ Locked`, install's
  preparation and execution, transfer's merge frames), use the crate's
  hierarchy if it has one. Where the crate can't express something, do the
  smallest honest workaround and record it.
- **Effects.** Pass `out` through if the crate's context can be generic. If
  its context or actions need a concrete type, either use a context holding
  `&mut dyn Sink<Effect>` (as `impl-statig/src/pointer.rs` does), or
  collect into a buffer in the machine and drain it into `out` at the end of
  `handle`. Don't allocate per event. Say what you chose in `NOTES.md`.
- **Events** are small `Copy` enums. If the crate wants one type per event,
  map in `handle`.
- **Timers are effects** (`ArmRetry`, `ArmShake`, …) and their firing is an
  event (`RetryTimer`, `ShakeTick`). Don't run the crate's own timer
  machinery (threads, tokio); note whether it could have modelled them.
- **No I/O, threads or async runtime.** Use the crate's synchronous API.
- **Unhandled events are ignored** by the spec. If the crate returns `Err`
  for an event it has no transition for, ignore that `Err` in `handle`.
- **Keep the typed API public** (state types, transition functions,
  generated handles, the machine's state accessor) so the compile-fail
  cases can probe it from outside; keep fields private where the crate
  allows.
- **Every trace must pass**, including the confirmed-bug scenarios (marked
  with their catalog id): the correct behaviour is the spec.

### Crate notes (from the survey; check them against the source)

The crates' sources are in `~/.cargo/registry/src/*/<crate>-<version>/`;
read the README and the macro code rather than trusting either alone.

- **statig**: handlers get `&Event` and `&mut` state-local data, so owned
  data moves need `Copy` or `mem::take`; unhandled events bubble to `Top`
  and are ignored; the context type is concrete (see `pointer.rs`).
- **smlang**: flat (no hierarchy: emulate superstates with wildcard rows or
  shared actions); `process_event` returns `Err(InvalidEvent)` for an
  undeclared (state, event) pair, which `handle` must ignore; guards and
  actions are methods of a generated context trait; state data is produced
  by actions.
- **sml** (stateforward-sml 2.0, imported as `sml`): the same DSL lineage
  with composites, orthogonal regions, shallow history and `defer`; the
  2.0 API removed the `state()` getter (state is read through a visitor);
  docs.rs has no 2.0 docs, so read `docs/` and `macros/` in the tarball.
- **finny**: states are structs that all live at once (measure `size_of`);
  use a fixed-size queue, not the default `VecDeque`; don't use its timer
  runtime (timers here are effects); `dispatch` returns
  `Err(NoTransition)` for unhandled events.
- **fsmentry**: a graph DSL generating an `entry()` API of legal-edge
  handles; there is no event model, so `handle` matches
  `(machine.entry(), event)`; transitions to data states take the data.
- **statum**: typestate (`#[state]`, `#[machine]`, `#[transition]`); the
  runtime boundary is a `SomeState`-style enum you match per event; its
  compile-time guarantees are the point, so write the probes carefully.
- **state-machines**: one DSL generates a typestate API and (with
  `dynamic`) a runtime `Dynamic` machine; use the dynamic one for `handle`
  and the typestate one for the probes; dynamic-mode state data may be
  `None` unless a `data:` factory is declared; MSRV 1.99.
- **rust-fsm**: a flat Mealy table without data or guards in the DSL; keep
  the data in your machine beside the generated table (or implement
  `StateMachineImpl` by hand) and record which parts the DSL couldn't hold;
  `consume` returns `Err` for impossible transitions.

### When something can't be expressed

`tests/spec.rs` has `const NOT_EXPRESSED: &[&str] = &[];`. List there only
traces your crate cannot express at all (the test then requires them to
fail), and explain each in `NOTES.md`. If a target is only partly
expressible its differential test will fail too: mark it
`#[ignore = "see NOTES.md"]`. If a target can't be done at all, leave the
stub (`IMPLEMENTED = false`), mark both its tests `#[ignore = "…"]`, and
explain. Never weaken a test to make it pass.

### Reading failures

- Conformance: `trace "name" failed: step i: <event>` with the expected and
  actual effects and state.
- Differential: the shrunk event sequence, then both machines' last effects
  and states. `--` marks where both machines started afresh (after
  `transfer` and `install` finish a job).
- `cargo test --locked -p impl-enum` shows which states the random
  environments reach (`environments_reach_every_state`).

## Benchmarks: smoke only

Build `bench` with only your slot (others are being written in parallel
and may not compile): `--no-default-features --features <crate>` (feature
names are the slot names: `statig`, …, `state-machines`, `rust-fsm`).

- `SME_SMOKE=1 cargo run --locked --release -p bench --no-default-features --features <crate> --bin smoke`:
  one pass over every workload; each of your rows must say `same as enum`
  (the digest of your effect stream equals the reference's). A row saying
  `DIFFERS FROM ENUM` is a bug even if the tests pass.
- `cargo run --locked --release -p bench --no-default-features --features <crate> --bin sizes`:
  `size_of` of your machine types.
- Optionally one quick criterion group:
  `SME_SMOKE=1 cargo bench --locked -p bench --no-default-features --features <crate> --bench machines -- 'replay-lock/<crate>'`.
  Nothing to register: the registry lists every slot and skips stubs.

## Compile-fail cases

Each `SPEC.md` ends with numbered **illegal operations** (pointer P1–P6,
wrap W1–W5, polkit K1–K6, transfer T1–T6, lock L1–L7, install I1–I6).
Write one case per probe per target:

```text
compile-fail/cases/<crate>/<target>/reject/<ID>-<slug>.rs    rejected at compile time (+ .stderr)
compile-fail/cases/<crate>/<target>/runtime/<ID>-<slug>.rs   compiles; refused at run time
compile-fail/cases/<crate>/<target>/accept/<ID>-<slug>.rs    compiles; the illegal operation happens
```

`<ID>-<slug>` as in `SPEC.md` (`K1-answer-without-prompt.rs`). Rules:

1. Attempt the probe at the **most typed level your implementation
   offers**: its state types and transitions, generated constructors and
   handles, state enum variants. Use `Machine::handle` only when that is
   the only way in. Never "prove" a probe by pushing an `Effect` yourself.
2. `reject`: write the `.stderr` with `TRYBUILD=overwrite` (prefix the
   cargo command) and **read it**. The first error must be the guarantee
   itself (no such method, field or variant; wrong state type; moved
   value), not a typo, an import mistake, or only a private constructor
   when a public form shows the same shape (use a variant literal).
3. `runtime`: `main` asserts the refusal (no effect, unchanged state, an
   `Err`).
4. `accept`: `main` asserts that the illegal operation went through.
5. A probe that can't even be attempted with your API: no file; say why in
   `NOTES.md` ("P4: not applicable: …").
6. A case may use only your crate, `spec` and `std`.

### Transitions and forging (added after the phase-2 reviews)

The reviews found the slots, and the baselines, deciding the same question
in different ways: does building a machine directly in a chosen state count
as committing a probe? One rule now applies everywhere.

**A probe's class (`reject`, `runtime`, `accept`) says what the transition
API allows.** The transition API is what a maintainer calls to move the
machine: `Machine::handle`, and the typed transitions the implementation
or crate provides for that purpose (transition methods, `entry()` handles,
typed events, the generated `dispatch`, `process_event`, `consume`). For
the probes about which data a state carries ("read X where X doesn't
exist", such as P2–P6, I6, L6), naming a state type or building a variant
literal to inspect its shape is part of this.

**Forging is every other way of setting the state**:

- building a machine directly in a chosen state (`new_with_state`,
  `new_init_state`, `state_mut` before `init`, a public builder, a variant
  literal written into a public field);
- writing state or context data (`context_mut`, `*_data_mut`, public
  fields);
- taking a declared edge without running its guard and action (for
  example statum's `CanTransitionTo/With/Map` and `transition_map`, finny's
  public transition types).

Forging never decides a probe's class. Instead:

1. Hide every forging means that your crate lets you hide: private fields
   and modules, a seal type, not re-exporting. A means your implementation
   chose to expose is not the crate's fault, so hide it.
2. For each target where a forging means stays public because the crate
   generates it public, write **one** case
   `cases/<slot>/<target>/forge/F-<means>.rs`. Its `main` uses that means
   from outside the implementation to commit one of the target's illegal
   operations (name the probe in a comment) and asserts that it happened.
   If several means stay public, name the others in a comment.
3. If no forging means stays public, write no forge case and say so in
   NOTES.md §4.
4. Re-check every case against the rule. An `accept` that went through only
   by forging moves to `runtime` or `reject` according to the transition
   API; the forging becomes the target's forge case. A `runtime` case that
   forging would defeat stays `runtime`.

`python3 metrics/sme.py compile-fail` prints the forge cases in a last
column.

**In a typed API a transition stands for its event.** Calling
`prepared.confirm(phrase_ok)` *is* the confirmation. A probe of the form
"X without event E" is `reject` when every typed path that commits X goes
through a transition that stands for E: it is named for E, or it demands
E's result or a proof value only E's handling can build. Name typed
transitions for their events where the crate allows it (fsmentry's
`-name->` edges, statum's method names, state-machines' events). Splitting
nodes or adding proof tokens to make a probe `reject` is fair game if it is
idiomatic for the crate; record the lines it cost in NOTES.md.

Run them with `cargo test --locked -p impl-<crate> --test compile_fail`
(prefix `TRYBUILD=overwrite ` to write `.stderr` files), and see the table
with `python3 metrics/sme.py compile-fail`. Worked examples:
`compile-fail/cases/{typestate,enum,implicit,statig}/pointer/`.

## NOTES.md

Write `impl-<crate>/NOTES.md` with these sections (short, concrete, with
file:line references to your code where it helps):

1. **Crate**: name and version; reserve substitution if any.
2. **Per target**: how you modelled it (states, superstates, state data,
   context, guards, actions), what the crate made easy, what needed a
   workaround and why, anything not expressed (traces, probes).
3. **Effects and dispatch**: how `out` reaches the handlers (generic,
   `dyn`, buffer), allocation, per-event costs the crate adds (an `Err` per
   ignored event, cloning, queueing).
4. **Compile-time guarantees**: which probes the crate rejects and why;
   a short excerpt of a typical compile error, and whether it pointed at
   the real mistake.
5. **Errors while writing it**: the worst macro/DSL error messages you hit
   (quote one), and how long they took to resolve.
6. **Ergonomics**: concision, readability, what a Sekka maintainer would
   have to learn, debugging (can you see the state? step through it?).
7. **Build**: anything about MSRV, edition, dependencies, proc-macro
   warnings, build time you noticed.
8. **Spec questions**: anything ambiguous, and the reading you chose.
9. **Verdict**: would you choose this crate for these targets, and for
   which kinds of Sekka machine.

## Lines of code

`python3 metrics/sme.py loc` counts non-blank, non-comment lines, without
`#[cfg(test)]` items. `src/<target>.rs` (or `src/<target>/`) counts for that
target; any other file under `src/` counts as shared (put helpers used by
several targets there, not copies in each target). Format with `cargo fmt`
first. Only code you write counts, not what macros generate; don't move
logic into `build.rs` or your own macros unless that is the crate's idiom.

## For phase 3

- `python3 metrics/sme.py conformance` runs every slot's tests with
  `SME_RESULTS` and prints passes per target (`--fixed`: impl-implicit with
  its bugs off). impl-implicit's expected failures are exactly its
  confirmed-bug traces (`spec::testing::bug_traces`).
- `python3 metrics/sme.py compile [--impl NAME] [--release]`: clean,
  crate-only and incremental compile times, each slot in its own fresh
  target directory.
- `python3 metrics/sme.py sizes`, `python3 metrics/sme.py loc`,
  `python3 metrics/sme.py compile-fail --run`.
- Benchmarks: `cargo bench --locked -p bench --bench machines` (criterion;
  1M pointer events, ~10M printed cells at 80 and 200 columns, trace
  replays). Instruction counts (valgrind from `nix build --inputs-from <repository root>
  nixpkgs#valgrind --out-link <scratch>/valgrind`, the runner from `cargo install gungraun-runner
  --version 0.20.0 --locked --root <scratch>/gungraun`):
  `PATH=<scratch>/valgrind/bin:$PATH GUNGRAUN_RUNNER=<scratch>/gungraun/bin/gungraun-runner cargo bench --locked -p bench --features iai --bench instructions`
  (filter: `-- '*::replay_lock::*'`). A stub reports the harness's fixed
  overhead (about 1.3k instructions for a replay set): subtract it.
