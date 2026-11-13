# State-machine crates for Sekka's machines

Which Rust state-machine crate, if any, should Sekka use when it turns the
implicit state machines in the [catalog](implicit-state-machines.md) into
explicit ones? To find out, six of the catalog's High-priority machines were
implemented eleven ways: three plain-Rust baselines and eight crates. Every
implementation was checked against the same conformance traces, property
tests and compile-time probes, then measured. This page gives the answer,
the evidence and the design rules that follow from it. The workspace, the
raw data and the scripts to rerun everything are in
[`state-machine-crates/eval/`](state-machine-crates/eval/).

## Answer

**Write the machines as hand-written typestate**, laid out as described in
[Design rules](#design-rules-for-the-reification). Sekka adopted this on
2026-11-04. No crate beat it on
expected value. It has the strongest compile-time guarantees measured (32 of
36 illegal operations rejected, none forgeable), no dependency, and ordinary
Rust code. Once the shared context lives beside the states instead of inside
them, it costs the same as a plain `enum` on the hottest path, Hotaru's
per-cell wrap.

| Rank | Implementation | EV score | In one line |
|---:|---|---:|---|
| 1 | typestate, by hand | 4.20 (4.40 with the layout rule) | Strongest guarantees, no dependency; the most lines |
| 2 | `enum` + `match`, by hand | 3.83 | Fastest and shortest; types check only which data a state carries |
| 3 | fsmentry 0.4.0 | 3.58 | The best crate: checked edge list at enum speed; no event model or hierarchy |
| 4 | statum 0.9.0 | 2.83 | Typestate's guarantees, but bypassable from any crate, and more lines than by hand |
| 5 | statig 0.4.1 | 2.60 | Good hierarchy and entry/exit; 2–4× slower on nested machines; few type checks |
| 6 | stateforward-sml 2.0.0 | 2.50 | The most expressive DSL; silent semantic bugs in a two-day-old release |
| 7 | state-machines 0.34.0 | 2.48 | Good typed API; slowest and largest; heavy churn |
| 8 | rust-fsm 0.8.0 | 2.30 | Only the transition table; nothing checked at compile time |
| 9 | smlang 0.8.0 | 2.28 | Flat tables with static dispatch; superstates multiply; dormant |
| 10 | finny 0.3.0 | 1.53 | Slow, large, nothing checked at compile time |

The score is a weighted sum of 1–5 ratings, explained in
[Ranking](#ranking-by-expected-value). Ranks 1–3 hold under any reasonable
weighting. Ranks 4–9 are within 0.55 of each other, so their order is weak.

## What was compared

### Targets

Each target is a High-priority machine from the catalog, reduced to a
normative spec (`eval/spec/src/<target>/SPEC.md`) with conformance traces.
Sixteen of the traces are the catalog's confirmed bugs, written as the
correct behaviour.

| Target | Catalog | Shape | Stresses |
|---|---|---|---|
| `pointer` | KIT-13, UIW-01 | 6 states; an `Inside` superstate owns the hover | Hot path (every pointer event); effect order |
| `wrap` | HCO-12 | pending wrap × autowrap (4 states) over the cursor and row links | Hottest path (every printed cell); `Repeat(n)` |
| `polkit` | SYB-09, SHB-03 | `Active` superstate, conversation numbers, retry timer | Data in states, stale events, back-off |
| `transfer` | HAKO-05 | per-item states, merge frames, set-aside data | Nested sub-machine, awaited responses |
| `lock` | SHB-12, SYB-12 | two orthogonal regions; `Lockscreen ⊃ Locked ⊃ Checking` | Regions, entry/exit order, delay tokens |
| `install` | GEN-07, GEN-06 | 20 linear states with an irreversible point | Ordering enforceable at compile time |

The specs simplify the real machines, as their `SPEC.md`s list. For
example, wrap has no graphemes or resize, and polkit handles one request at
a time.

### Implementations

- **Baselines.** `implicit` is the current code's shape (flags, `Option`s,
  counters) with the confirmed bugs kept. `enum` is one `enum` with data in
  its variants and one `match`; it is the reference. `typestate` has one type
  per state, consuming transitions and a runtime wrapper enum.
- **Crates.** statig, smlang, stateforward-sml (imported as `sml`), finny,
  fsmentry, statum, state-machines and rust-fsm. These were the
  [survey](state-machine-crates/survey.md)'s shortlist from about 40 crates.

Each crate slot was written by its own agent in the crate's idiom (its
states, superstates, guards, actions, entry/exit and generated handles) and
recorded in `eval/impl-<slot>/NOTES.md`. An independent reviewer then checked
each slot for two kinds of error. One is a crate given too much credit: a
hand-written enum doing the work behind a crate facade, or a compile-fail
case that fails for the wrong reason. The other is a crate judged too
harshly: a workaround written where the crate has a feature for it. The
implementers applied the [reviews](state-machine-crates/reviews/). Every
slot represents its crate fairly enough to rank.

### Compile-time probes

Each spec ends with numbered illegal operations: 36 in all. Examples are K3,
restarting a polkit conversation after an unanswered failure without
backing off (SYB-09's spin loop), and I4, reaching `Prepared` on the deferred
route without the space check (GEN-07). Every implementation has one
trybuild case per probe, classed by what happens:

- **R**: rejected at compile time;
- **T**: compiles, but the implementation refuses at run time;
- **A**: compiles, and the illegal operation happens.

Two rules, set after the first reviews, keep the classes comparable
(`eval/IMPLEMENTING.md`, "Transitions and forging"):

- **A probe is classed by the transition API**: `handle` and the typed
  transitions. *Forging* never decides a class. Forging means building a
  machine in a chosen state, writing its data, or taking an edge without its
  guard. Each slot hid every forging means its crate let it hide. Where the
  crate generates one as public, the target gets one **forge** case.
- **In a typed API a transition stands for its event.** Calling
  `prepared.confirm(..)` is the confirmation.

## Results

All numbers are from [`eval/results/results.md`](state-machine-crates/eval/results/results.md),
which traces every cell to a raw file. The instruction counts are callgrind
`Ir` from gungraun, per event or per printed cell, with the harness's
overhead subtracted. They are the primary performance number, because the
host was shared. "LTO" is thin LTO with one codegen unit, which is how
`apps/hotaru` ships.

| Implementation | Conformance (105 traces, 20k property cases) | R / T / A (36) | Forge (targets) | Instr. ratio to enum, geomean of 9 workloads (LTO) | wrap-80 per cell (LTO) | pointer per event (LTO) | Lines (× enum) | Size, bytes | Crate-only dev build |
|---|---|---|---|---:|---:|---:|---:|---|---:|
| implicit (bugs kept) | 89 (16 bug traces fail) | 0 / 21 / 15 | 0 | — | 0.96 | 0.88 | 1422 (1.19) | 28–64 | 0.24 s |
| enum | all | 7 / 29 / 0 | 0 | 1.00 | 1.00 | 1.00 | 1195 (1.00) | 24–40 | 0.25 s |
| typestate | all | **32 / 4 / 0** | 0 | 1.35 | 2.10 | 1.18 | 2139 (1.79) | 24–64 | 0.30 s |
| typestate, layout rule (wrap only) | wrap: all | wrap probes still R | 0 | — | **1.00** | — | wrap 404 (2.13 × enum's wrap) | — | — |
| statig | all | 7 / 29 / 0 | 0 | 2.15 | 1.82 | 1.40 | 1596 (1.34) | 24–48 | 0.36 s |
| smlang | all | 7 / 29 / 0 | 0 | 1.51 | 1.76 | 1.74 | 1966 (1.65) | 28–48 | 0.52 s |
| sml | all | 5 / 31 / 0 | 0 | 1.33 | 0.98 | 1.37 | 1729 (1.45) | 36–80 | 0.68 s |
| finny | all | 0 / 32 / 4 | 0 | 2.56 | 2.60 | 2.21 | 2311 (1.93) | 112–464 | 0.94 s |
| fsmentry | all | 23 / 10 / 3 | 5 | **1.10** | 1.00 | 1.31 | 1487 (1.24) | 24–40 | 0.37 s |
| statum | all | **32 / 4 / 0** | 6 | 1.62 | 2.39 | 1.44 | 2328 (1.95) | 24–64 | 1.01 s |
| state-machines | all | 26 / 10 / 0 | 5 | 4.02 | 5.52 | 2.98 | 2162 (1.81) | 168–336 | 1.11 s |
| rust-fsm | all | 0 / 32 / 4 | 6 | 1.66 | 1.95 | 1.62 | 1496 (1.25) | 20–40 | 0.31 s |

Notes on the table:

- **Correctness.** Every implementation except `implicit` passes all 105
  traces, six differential property tests at 20,000 cases each against
  `enum`, and its trybuild cases. Every smoke digest matches `enum`'s.
  `implicit` fails exactly its 16 confirmed-bug traces and passes all 105
  with `--features fixed`.
- **The typestate layout row** comes from a follow-up experiment on wrap
  alone ([below](#why-typestate-wrap-cost-21-and-the-fix)). The other
  typestate rows measure the layout the experiment started with.
- **Wall-clock** (criterion, pinned to one core) agrees with the instruction
  counts on the long streams. There are two exceptions: statum's pointer
  stream ran at 0.72× enum's time (its timed binary is compiled
  differently, and its instructions-per-ns is higher), and the short replay
  benchmarks varied more than 10% between runs. See the results' anomalies
  section.
- **Build sensitivity.** The same enum code counts 95 to 122 instructions
  per pointer event, depending on what else the bench crate contains, because
  the inliner's choices change. Ratios within about 1.3× of each other on
  pointer and the replays aren't robust. The wrap streams are, except sml's,
  which drops from 2.46× to 0.98× enum under LTO.
- **Clean builds** with dependencies take 0.29 s for enum, 0.34 s for
  typestate and 1.65–4.4 s for the crates, which bring their proc macros and
  `syn`.

## Ranking by expected value

Each implementation gets a 1–5 rating on five criteria. The weights reflect
what the reification is for: the catalog's High machines are High because
their bugs reach privilege, data loss, hangs or the user.

| Criterion | Weight | What earns a 5 |
|---|---:|---|
| Correctness | 0.35 | Many probes rejected at compile time, nothing forgeable from outside, no crate defects found |
| Performance | 0.20 | LTO instruction geomean ≤ 1.15× enum (≤ 1.4: 4, ≤ 1.7: 3, ≤ 2.2: 2, worse: 1) |
| Expressiveness | 0.15 | Every target's structure (hierarchy, regions, entry/exit, data, guards) held by the mechanism without workarounds |
| Concision | 0.15 | Lines ≈ enum's (≤ 1.3×: 4, ≤ 1.5×: 3, ≤ 1.7×: 2.5, ≤ 1.85×: 2, worse: 1.5) |
| Maintenance risk | 0.15 | No dependency (5), or a mature, maintained crate |

| Implementation | Correctness | Performance | Expressiveness | Concision | Maintenance | Score |
|---|---:|---:|---:|---:|---:|---:|
| typestate | 5 | 4 (5 with the layout rule) | 4 | 2 | 5 | **4.20** (4.40) |
| enum | 2.5 | 5 | 3 | 5 | 5 | 3.83 |
| fsmentry | 3.5 | 5 | 2 | 4 | 3 | 3.58 |
| statum | 4 | 3 | 2 | 1.5 | 2 | 2.83 |
| statig | 2 | 2 | 4 | 3 | 3 | 2.60 |
| sml | 1 | 4 | 5 | 3 | 1 | 2.50 |
| state-machines | 3.5 | 1 | 4 | 2 | 1 | 2.48 |
| rust-fsm | 1 | 3 | 1 | 4 | 4 | 2.30 |
| smlang | 2 | 3 | 2 | 2.5 | 2 | 2.28 |
| finny | 1 | 1 | 3 | 1.5 | 2 | 1.53 |

Why the ratings:

- **Correctness** starts from the R count and the forge column, then loses
  for crate defects that would silently break a Sekka machine (listed under
  [Crate defects found](#crate-defects-found)).
  - `enum` gets 2.5 rather than 2: `match` is exhaustive, and it has no
    macro to misread.
  - statum gets 4, not 5: its typed API rejects as much as typestate's, but
    every edge stays forgeable through generated public API.
- **Expressiveness** rates how much of the spec's structure the mechanism
  holds.
  - Hand-written code can express everything. typestate makes the structure
    visible in types (4); `enum` leaves it in match arms (3).
  - sml's composites, regions, entry/exit and defer map closest to the specs
    (5).
- **Maintenance** uses the survey's data, corrected where the
  implementations found it wrong:
  - stateforward-sml 2.0.0 was released two days before the experiment;
  - state-machines went from 0.9 to 0.34 within 2026;
  - smlang's last release was in August 2024, and it is still on syn 1;
  - statig's hook fix has been unreleased since November 2025.

**Sensitivity.** typestate stays first if performance is weighted 0.30 and
correctness 0.25 (4.10 against enum's 4.08 as measured; 4.40 with the layout
rule). fsmentry stays the best crate unless expressiveness is weighted far
above everything else, when sml or statig would lead. The middle crates swap
places with small weight changes.

## Per implementation

Each slot's `NOTES.md` has the full account and the review has the checks.
These are the findings that decided the ratings.

- **typestate (by hand).**
  - **Guarantees:** rejects 32 of 36 probes and nothing outside its module
    can forge it.
  - **Cost:** the most lines among the baselines (1.79× enum). Superstates
    are `macro_rules!` that put the shared edges on each state, and events
    still route through a runtime `match` on a wrapper enum. The typed layer
    limits what each arm can call (the `Starting` arm has no `answer`); it
    doesn't remove the router.
  - **Speed:** its one real cost, on wrap, came from its layout and is fixed
    below.
  - **The four probes it can't reject:**
    - K2, T5 and L2 ask whether a message belongs to the current instance: a
      conversation number, item id or PAM check id that arrives at run time
      from another process or thread. Types know the state, not the
      instance.
    - L5 is a signature left too loose (`refused(note: LockNote)`).

  [Design rules](#design-rules-for-the-reification) covers both.
- **enum (by hand).**
  - **Strengths:** fastest and shortest, with no dependency.
  - **Limit:** its types check only which data a state carries (7 rejects,
    P2–P6, L6, I6). Any arm may build any state, so the transition graph
    isn't checked.
- **fsmentry.**
  - **Strengths:** a short graph DSL of legal edges generates handles. With
    event-named edges, split nodes and proof tokens it rejects 23 probes,
    at enum's speed on wrap. It has no runtime dependency.
  - **Limits:**
    - There is no event model, and only one edge is allowed per pair of
      states, so W1, W3 and L1 stay accepted.
    - Hierarchy means nesting a machine in a state's data. Effects and
      entry/exit are hand-written.
    - The generated `AsMut` on data states can't be hidden (forge in five
      targets).
    - A misspelt node silently becomes a new state, and generated methods
      lack `#[inline]`.
- **statum.**
  - **Strengths:** its `#[transition]` methods give exactly typestate's
    classes (32/4/0).
  - **Limits:**
    - For every declared edge, even from a private method, it generates
      public `CanTransitionTo/With/Map` impls and `transition_map`, which take
      the edge without its body, and `state_data` is always `pub`. Sealing
      payloads cost 143 lines, and a payload can still be moved out of a
      donor machine. That is the polkit forge case, which brings SYB-09's
      spin back from outside the crate.
    - It writes more lines than typestate by hand.
    - Transitions may return only `::core::result::Result`, `Option` or
      `Branch`. Two edges with the same payload type clash (E0119).
    - Its macros read source files and take another path under
      rust-analyzer.
- **statig.**
  - **Strengths:** superstates sharing borrowed state data, plus entry/exit,
    put the specs' effect order in place without hand sequencing.
  - **Limits:**
    - It recomputes the transition path through the hierarchy on every
      transition: about 26% of polkit's cost, which runs at 3.1× enum (3.9×
      under LTO).
    - It has no regions, so lock is two machines.
    - It checks only data shape at compile time.
    - In 0.4.1, `before_transition` receives source and target swapped, and
      the safe `state_mut` can start an uninitialised machine in any state.
- **stateforward-sml.**
  - **Strengths:** composites, orthogonal regions, entry/exit, choice rows
    and `defer` fit lock and install closely.
  - **Limits:** in 2.0.0, any composite-to-composite transition skips the
    child's exit, reset and entry. That silently dropped polkit's
    `StartHelper` until the implementation routed around it. Sync
    `process(E)` recurses instead of queueing.
- **state-machines.**
  - **Strengths:** one DSL generates a typed API with good errors (26 rejects)
    and a dynamic machine built on it, so the two can't drift.
  - **Limits:**
    - The dynamic wrapper, an `Option` slot per data state, and
      consume-and-rebuild transitions make it the slowest (4.0× geomean) and
      largest.
    - Regions need the async runtime.
    - Its generic names aren't hygienic: a machine named `C` breaks.
- **rust-fsm.**
  - **Strengths:** its table is clean for small dataless machines, such as
    lock's 8-row delay region.
  - **Limits:** the DSL holds only unit states and inputs and one output per
    row. Guards, data, hierarchy, regions and entry/exit are code beside it.
    Duplicate rows compile, and the first one wins.
- **smlang.**
  - **Strengths:** a generic context dispatches effects statically, without
    allocation.
  - **Limits:**
    - Guards and actions are reusable only where state and event data types
      match, so each superstate row multiplies into one row and method per
      substate.
    - If an action fails, the source state's exit hook has already run, and a
      retry runs it again.
    - State equality ignores data.
- **finny.**
  - **Strengths:** it checks the transition graph's shape at build time.
  - **Limits:**
    - Every state struct is alive at once (112–464 bytes).
    - Sub-machines can't pass events up to their parent or leave themselves.
    - The target state is chosen before the action runs.
    - Its generated transition types run from any state and skip the guard,
      for anyone holding the machine `&mut`.

## Why typestate wrap cost 2.1× and the fix

The measured typestate wrap kept the whole screen context inside every
state: `pub struct Normal(Screen)`, with `Screen` 32 bytes including the
`u128` row links. The runtime wrapper `Any` was 48 bytes. Each printed cell
moved that value about four times:

1. `mem::replace` copied it out and wrote a placeholder in.
2. The consuming transition took it by value and returned a differently
   laid-out `Ready` enum.
3. `.into()` re-tagged it as `Any`.
4. The result was written back.

LLVM can't keep a 48-byte value with a `u128` in registers through those
enum conversions. Under LTO, callgrind counted 37 data reads and 35 writes per
cell, against the enum's 14 and 9: 136 instructions against 65. The enum
version stores a 1-byte state beside the context and changes it in place. At
80 columns, 79 of every 80 cells don't change state at all.

The follow-up in
[`eval/experiments/typestate-wrap/`](state-machine-crates/eval/experiments/typestate-wrap/)
measured four layouts. All pass wrap's traces and the differential test.
Instruction ratios to enum, default build / LTO:

| Layout | wrap-80 | replay-wrap | Guarantees |
|---|---:|---:|---|
| states own `Screen` (measured above) | 2.16 / 2.10 | 1.90 / 1.84 | all |
| states own `Box<Screen>` | 1.27 / 1.24 | 1.41 / 1.32 | all, API unchanged |
| zero-sized state tokens, `&mut Screen` passed to each transition | 1.14 / 1.14 | 1.13 / 1.13 | weaker: one machine's token works with another machine's screen |
| states hold `&'s mut Screen`; the machine stores a 1-byte tag | 1.13 / 1.15 | 1.12 / 1.13 | all |
| the same, with `handle` routing `Print` first | **1.00 / 1.00** | **1.08 / 1.08** | all |

In the borrowed layout, `pub struct Normal<'s>(&'s mut Screen)` is
pointer-sized, so moving a state costs nothing. Only the module can rebuild a
typed state from the tag (`Any::open` is private). The typed API is exposed
as

```rust
pub fn with_typed<R>(&mut self, f: impl for<'s> FnOnce(Any<'s>) -> (Any<'s>, R)) -> R
```

which hands out the current state and requires the state it ends in back.
Re-running the probes against it:
- W1 (relabelling `Normal` as `Pending`) and W4 (`Normal::print` on a
  `Pending`) are E0308.
- W5 (a direct soft wrap) is E0599.
- A `Pending` literal is E0423.
- Returning machine A's state to machine B fails with "lifetime may not live
  long enough".

The remaining 13% before routing `Print` first was `handle` classifying every
event before matching it. That is hot-path ordering, not typestate.

## Design rules for the reification

1. **State types hold only their own state's data**, such as polkit's
   `Request` and `Conversation` or transfer's set-aside path. Shared context
   (the screen, the cursor, config, counters like `last_conv`) lives in the
   machine. States borrow it as `&'s mut Ctx` and are then pointer-sized.
   The machine stores a tag plus any state data.
2. **Make instance checks impossible to forget.** Typed transitions that act
   on an event from another process or thread take a proof value only the
   comparison can produce, for example
   `conversation.check(event_conv) -> Option<Current>`. A new event path
   that skips the comparison then doesn't compile. This targets K2, T5 and
   L2, the stale-result bug class behind SYB-09 and similar entries.
3. **Narrow signatures that let the router choose a wrong value.**
   `refused(r: Refusal)` and `wrong_password()`, or `pam_done(outcome)`,
   instead of `refused(note: LockNote)` (L5, SHB-12).
4. **Route the hottest event first** in `handle`.
5. **Keep the typed layer the only way to change state**: private fields, no
   public constructors of mid-machine states, and `with_typed` where code
   outside the module needs typed access. Adopt the slot's compile-fail
   probes as the machine's trybuild tests.

Where a machine has no hot path and its states carry their own data, as
polkit, transfer, lock and install do, rule 1 matters less: polkit already
passes its context by `&mut` and runs at 1.17× enum. Rules 2 and 3 matter
everywhere.

## Crate defects found

The implementers found these, and the reviewers confirmed them against the
crate sources. None has been reported upstream.

| Crate | Defect | Evidence |
|---|---|---|
| statig 0.4.1 | `before_transition` gets source and target swapped (`inner.rs:42`) | `reviews/statig.md` |
| statig 0.4.1 | Safe `UninitializedStateMachine::state_mut` starts a machine in any state, running its entry actions | `reviews/statig.md` |
| smlang 0.8.0 | An action returning `Err` leaves the state unchanged after its exit hook ran; a retry runs the hook again | `reviews/smlang.md` |
| smlang 0.8.0 | State equality compares only the variant (`Pressed(1) == Pressed(99)`) | `reviews/smlang.md` |
| stateforward-sml 2.0.0 | Any composite-to-composite transition skips the child's exit, reset and entry (`composite_codegen.rs:4559`) | `eval/impl-sml/tests/crate_findings.rs` |
| stateforward-sml 2.0.0 | Sync `process(E)` recurses instead of queueing; an untaken processed event aborts the step | same |
| stateforward-sml 2.0.0 | An orthogonal table without children and no row reaching `X` doesn't compile (`orthogonal_codegen.rs:768`) | same |
| statum 0.9.0 | Public `CanTransitionTo/With/Map` and `transition_map` for every declared edge, even from private methods; `state_data` always `pub` | `reviews/statum.md` |
| state-machines 0.34.0 | Generic names aren't hygienic (a machine named `C` breaks); `initialize()` re-runs entry hooks | `reviews/state-machines.md` |
| fsmentry 0.4.0 | A misspelt node silently creates a state; generated methods aren't `#[inline]` | `reviews/fsmentry.md` |
| rust-fsm 0.8.0 | Duplicate rows compile and the first wins | `reviews/rust-fsm.md` |
| finny 0.3.0 | Generated transition types run from any state, skipping the guard | `reviews/finny.md` |

## Limits of this evaluation

- **The targets are reductions.** The specs are smaller than the real
  machines. They keep the states, the confirmed bugs and the hot loops, but
  drop I/O, graphemes, resize and multiple requests.
- **The machine was shared.** Other sessions' builds and VMs ran throughout.
  Instruction counts don't depend on load. Wall-clock runs were pinned, but
  the pinned core's SMT sibling wasn't reserved.
- **The bench crate's composition moves the reference** by up to 29% on
  pointer. Read pointer and replay ratios below 1.3× as ties.
- **One implementation per crate.** A crate expert might write a faster or
  safer slot. The reviews looked for exactly that and corrected what they
  found, but a second idiom could still change a rating by a point.
- **The typestate layout fix was measured on wrap only.** Pointer and the
  replays still use the starting layout.

## Reproducing

Everything runs from the root flake's default dev shell through
`eval/isolate` (a private HOME, no display, no session bus). From the
repository root:

```sh
s=/tmp/sme-scratch
CARGO_TARGET_DIR=$s/target docs/state-machine-crates/eval/isolate default $s \
  bash -c 'cd docs/state-machine-crates/eval && cargo test --locked --workspace'
```

`eval/results/run.sh` reruns the whole measurement in order. The
instruction counts need valgrind and `gungraun-runner` 0.20.0; the script
says how to get them. `eval/IMPLEMENTING.md` is the contract every slot
followed, and `method/` holds the briefs the agents worked from.
