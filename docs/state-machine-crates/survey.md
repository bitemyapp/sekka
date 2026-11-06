# Rust state-machine crates: survey (2026-11-04)

This survey picks the crates a follow-up experiment should benchmark when reifying Sekka's implicit state machines. The targets are GUI interaction modes (press/drag/hover), protocol and session lifecycles with data in states (polkit conversation, file-transfer job with conflict prompts), and hot paths (terminal VT parser, per-frame compositor scheduling). This is research only: nothing was compiled or run.

**Method.**
- **Discovery:** crates.io search and keyword APIs (`state-machine`, `fsm`, `statemachine`, `statechart`, `typestate`, `state-machines`, `finite-state-machine`, `hsm`, plus text queries sorted by downloads and by recent downloads), using the User-Agent `sekka-state-machine-survey`.
- **Per-crate metadata:** the crates.io crate API for versions, dates, downloads, yanks, `rust_version` and edition, and the reverse-dependency API.
- **Repository signals:** `gh api` (and the GitLab API) for stars, open issues, last push, commits and issue text.
- **Feature claims:** checked by reading the published `.crate` sources (README, Cargo.toml, `lib.rs`, macro codegen) rather than trusting READMEs.
- **Who read what:** the core crates were read directly; two parallel research passes read the long tail against the same template and were spot-checked (fsmentry, stateless, stateforward-sml, moku and the download anomalies were re-verified).

**Sekka context checked locally.**
- Sekka's workspaces are edition 2024 with declared `rust-version` 1.90–1.95, and the pinned nixpkgs `rustc` is 1.99.0.
- No FSM crate is in any Sekka manifest today.
- Hotaru's VT parser (`apps/hotaru/crates/hotaru-core/src/parser.rs`) is already a hand-written `enum State` plus `match`, which is Baseline A.

**Download counts are a weak signal.**
- statig shows 4.7M downloads with only 7 published dependents.
- statecraft shows 81k with **0** dependents and daily counts swinging between 1 and 3,154.
- yasm shows about 13.6k and stateforward-sml 778, both with 0 dependents.
- obzenflow-fsm shows 47k, but its two dependents total about 830.

**2026 trend.** Many of the newest crates are single-author bursts with heavy LLM assistance (Claude trailers, `CLAUDE.md`/`AGENTS.md`, `[verified]` commit prefixes). Several have READMEs that contradict their code. These are flagged below.

## Legend

- `Y` means supported, `N` not supported, `~` partial or by hand, `?` unverified.
- **Hier** = hierarchical/nested states.
- **Data** = data carried inside states.
- **Ctx** = shared context.
- **Tmr/Dfr** = built-in timers / deferred-event queue.
- **Hist** = history states.
- **TS** = compile-time-checked transitions (typestate or legal-edge handles).
- **Invalid** = what an event with no transition does at runtime.
- **Style:**
  - `DSL` is a declarative (function-like) macro DSL.
  - `attr-impl` is a proc-macro attribute on an impl block or module.
  - `trait` means the user implements a trait.
  - `typestate` means one type per state.
  - `interp` means an interpreted graph at runtime.

## Summary table

| Crate | Latest (date) | DL total / 90d | Last commit | Style | Hier | Guards | Entry/Exit | Data | Ctx | Async | Tmr/Dfr | Hist | TS | Invalid | Flags |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| *Baseline A: enum + match* | – | – | – | hand-written | ~ | ~ | ~ | Y | Y | ~ | N/N | N | N | your `match` | zero cost |
| *Baseline B: typestate by hand* | – | – | – | hand-written | N | ~ | ~ | Y | Y | Y | N/N | N | Y | compile error | needs enum wrapper for runtime events |
| **statig** | 0.4.1 (2025-07-05) | 4.71M / 197k | 2025-11-29 | attr-impl | Y | ~ | Y | Y | Y | Y | N/N | N | N | ignored (Top) | release stale; async needs Rust ≥1.85 |
| **rust-fsm** | 0.8.0 (2025-07-21) | 1.52M / 349k | 2025-07-21 | DSL + trait | N | N | N | N (DSL) | N | N | N/N | N | N | `Err` | flat Mealy table only |
| sm | 0.9.0 (2019-11-09) | 1.95M / 65k | 2020-12 | typestate DSL | N | N | N | N | N | N | N/N | N | Y | compile error | **ARCHIVED**; syn 0.15 |
| **smlang** | 0.8.0 (2024-08-09) | 1.10M / 254k | 2025-01-31 | DSL (Boost.SML) | N | Y | Y | Y | Y | Y | N/N | N | N | `Err(InvalidEvent)` | master 13 commits ahead; syn 1 |
| **stateforward-sml** | 2.0.0 (2026-11-02) | 778 / 708 | 2026-11-02 | DSL (Boost.SML) | Y | Y | Y | Y | Y | Y | N/Y | shallow | N | `Err` (configurable) | 4 months old, 2.0 API churn, 1 maintainer |
| **finny** | 0.3.0 (2026-09-30) | 27k / 3.7k | 2026-09-30 | attr on builder fn | Y | Y | Y | ~ (all states live) | Y | Y (tokio) | Y/Y | N | N | `Err(NoTransition)` | revived in a 2-day burst after 4 years |
| sfsm | 0.4.3 (2022-05-15) | 23k / 463 | 2022-06 | DSL + traits (polled) | ~ | Y | Y | Y | N | N | N/N | N | N | n/a (guard-polled) | **UNMAINTAINED** |
| machine | 0.3.0 (2019-06-14) | 40k / 2.1k | 2021-08 | DSL | N | ~ | N | Y | N | N | N/N | N | N | sticky `Error` variant | **UNMAINTAINED**; writes to `target/` at compile time |
| typestate | 0.8.0 (2021-09-02) | 43k / 1.1k | 2023-07 | typestate (attr-mod) | N | N | N | Y | ~ | N | N/N | N | Y | compile error | **ARCHIVED** |
| state_machine_future | 0.2.0 (2018-11-10) | 279k / 10.6k | 2019-07 | derive (futures 0.1) | N | N | N | Y | N | futures 0.1 | N/N | N | Y | compile error | **ARCHIVED** |
| **statum** | 0.9.0 (2026-06-09) | 39k / 3.9k | 2026-08-31 | typestate (attrs) | N | ~ | N | Y | Y | ? | N/N | N | Y | compile error | MSRV 1.93; rapid churn |
| **state-machines** | 0.34.0 (2026-11-01) | 20k / 5.9k | 2026-11-01 | DSL → typestate + dynamic | Y | Y | Y | Y (dynamic: may be `None`) | Y | Y | deadlines/? | Y | Y | compile error / `Err` | **MSRV 1.99**; 0.9→0.34 in 2026; 5 yanked |
| edfsm | 0.12.1 (2026-10-02) | 24k / 5.8k | 2026-10-02 | attr-impl (command/event) | N | ~ | entry only | Y | Y | via edfsm-machine | N/N | N | N | `None` / exhaustive | event-sourcing ceremony |
| **fsmentry** | 0.4.0 (2025-04-07) | 6.4k / 73 | 2025-04-07 | graph DSL + entry API | ~ | ~ | N | Y | N | n/a | N/N | N | ~ (legal-edge handles) | compile error | quiet but finished; bus factor 1 |
| stateless | 0.5.0 (2026-04-07) | 3.6k / 708 | 2026-04-07 | DSL table | N | outside | N | N | N | n/a | N/N | N | N | `None` | unit states and events only |
| typed-fsm | 0.4.8 (2025-12-02) | 1.2k / 179 | 2025-12-24 | `macro_rules!` | N | ~ | Y | Y | Y | N | N/N | N | N | your `match`; `dispatch` returns `()` | `concurrent` queue shared across instances |
| moku | 0.4.0 (2026-02-14) | 2.2k / 35 | 2026-02-14 | attr-mod + traits | Y | ~ | Y | Y (built in `enter`) | Y (ancestors) | N | N/N | N | N | ignored | `log::info!` on every event |
| hsmc | 0.6.0 (2026-05-04) | 342 / 105 | 2026-05-04 | DSL | Y | **N** | Y | **N** | Y | Y | Y/N | N | N | ignored | no guards, no state data |
| kaori-hsm | 0.1.1 (2024-03-09) | 2.9k / 8 | 2024-05 | trait (QP style) | Y | ~ | Y | N | Y | N | N/N | N | N | ignored | **UNMAINTAINED**; unsafe transmute |
| statecraft | 0.1.0 (2026-07-07) | 81k / 57k | 2026-07-07 | typestate traits | N | ~ | N | Y | Y | Y | N/N | N | Y | compile error / `Err` | 0 dependents; just helper traits |
| statecraft-fsm | 0.2.0 (2026-08-20) | 278 / 260 | 2026-08-20 | attr-impl | N | ~ | N | N | Y | async-only | N/N | N | N | `Err` | public `state` field |
| obzenflow-fsm | 0.4.0 (2026-11-03) | 47k / 10k | 2026-11-03 | DSL + runtime | N | ~ | Y | Y | Y | Y (tokio required) | polled timeout/N | N | N | `Err` | `HashMap<String>` + boxed future per event |
| timed-fsm | 0.5.0 (2026-07-19) | 3.8k / 2.5k | 2026-07-24 | trait | N | ~ | N | ~ | Y | Y | Y (timer commands)/N | N | N | your code | LLM-written; not an FSM library |
| state-shift | 2.1.1 (2025-03-20) | 24k / 2.8k | 2025-03-20 | typestate (attrs) | N | N | N | N | Y | ~ | N/N | N | Y | compile error | builder-ordering tool; 1 yanked |
| qubit-state-machine | 0.11.1 (2026-11-04) | 2.8k / 1.9k | 2026-11-04 | interp (builder) | N | N | post-commit callback | N | N | N | N/N | N | N | `Err` | atomics; ~10 sibling crates |
| async_fsm | 0.1.4 (2025-05-11) | 3.3k / 16 | 2025-05-12 | trait + tokio actor | N | ~ | entry only | N | ~ (read-only) | tokio | N/N | previous state only | N | ignored | dormant; event map grows without bound |
| squads-rustfsm / rustfsm | 0.3.1 (2025-09-25) / 0.1.0 (2021) | 12k / 864; 6.5k | (Temporal in-tree) | DSL | N | ~ | N | Y | Y | N | N/N | N | ~ | `Err` | unofficial snapshot; rustfsm clones machine per event |
| ironstate | 0.1.4 (2026-08-29) | 439 / 152 | 2026-10-24 | derive + trait | N | ~ | N | Y (cloned) | **N** | N | N/N | log | N | `Err` | LLM-generated |
| tramli | 3.9.0 (2026-09-04) | 905 / 131 | 2026-10-16 | interp (workflow) | sub-flows | Y | Y | `TypeId` map | Y | N | TTL/N | log | N | `Err` | 37 releases in 3 days; LLM-generated |
| scxml | 0.2.3 (2026-09-13) | 2.0k / 1.9k | 2026-09-13 | SCXML model + simulator | modelled | named only | not run | N | N | N | modelled | modelled | N | `Err` | "not a runtime executor" (README) |
| rustate | 0.3.0 (2025-04-28) | 5.4k / 44 | 2026-07-28 | interp (XState-like) | Y | Y | Y | N | `Arc<RwLock>` | async-only | N/N | Y | N | `Ok(false)` | clones context per event |
| xmachines-runtime | 1.1.0 (2026-11-04) | 111 / 111 | 2026-11-04 | interp (XState JSON) | Y | Y | Y | N | Y | effects | Y/? | Y | N | ignored (?) | days old; MSRV 1.99; JSON per event |
| banish | 1.4.1 (2026-04-03) | 351 / 50 | 2026-10-29 | rule-block DSL | N | rules | entry only | ~ | ~ | Y | N/N | N | N | n/a (no events) | not an event-driven machine |
| yasm | 0.6.0 (2026-07-17) | 13.6k / 8.7k | 2026-07-17 | `macro_rules!` + runtime | N | N | callbacks | N | N | N | N/N | log | N | `Err` | `Vec` per transition; README overclaims |
| extfsm | 0.14.1 (2026-08-17) | 16.9k / 77 | 2026-08-17 | interp (builder) | N | N | Y | N | Y | N | N/Y | N | N | `Err` | `HashMap` + `dyn` + `RefCell` |
| mode | 0.4.1 (2021-08-13) | 17k / 237 | 2025-08 (docs only) | trait container | N | ~ | N | Y | N | N | N/N | N | N | n/a | dormant; adds little over Baseline A |
| sad_machine | 1.0.0 (2021-09-08) | 15.6k / 806 | 2022-03 | typestate-ish DSL | N | N | N | N | N | N | N/N | N | ~ | compile error | **UNMAINTAINED** fork of sm |

Bold names are on the shortlist at the end.

## Baselines

### Baseline A: plain `enum` + `match`

- **What it is:** a `State` enum whose variants carry their own data, plus one `fn handle(&mut self, ev: Event) -> Effect` that matches `(state, event)`. Owned data moves between states with `std::mem::replace(&mut self.state, State::Poisoned)` or `mem::take`, and new data is constructed in the arm.
- **Performance:** one jump table or branch chain per event. Nothing is allocated, nothing is dispatched through a vtable, and the code is as small as the logic itself. This is what the crates below generate at best.
- **Correctness:** exhaustive `match` forces every (state, event) pair to be handled or explicitly skipped with `_`. Data lives only in the variant that owns it. However, any arm may construct any state, so the transition graph itself is not checked, and a `_ => {}` arm silently drops invalid events.
- **Expressiveness:** anything at all, but hierarchy, entry/exit ordering, history, timers and queues are hand-written each time.
- **Concision:** a press/drag/hover machine with a guard and drag data is about 30–45 lines, with no macro or DSL to learn.
- **In-repo references:** Hotaru's VT parser is already this shape (`enum State` plus `match self.state`, 417 lines, `apps/hotaru/crates/hotaru-core/src/parser.rs`). alacritty's `vte` uses per-state `advance_*` functions dispatched by `match self.state` (https://github.com/alacritty/vte/blob/master/src/lib.rs). For protocol lifecycles, the sans-I/O state-machine style of `quinn-proto` and `str0m` is the large-scale precedent.
- **Sources:** https://hoverbear.org/blog/rust-state-machine-pattern/ ; https://doc.rust-lang.org/std/mem/fn.replace.html

### Baseline B: typestate by hand

- **What it is:** one type per state, either `struct Machine<S> { shared, state: S }` or separate structs, with transitions as `fn(self) -> Machine<Next>`. Branching transitions return an enum (`enum Next { A(Machine<A>), B(Machine<B>) }`).
- **Performance:** transitions are zero-cost moves. At runtime, events still have to be routed through an enum wrapper of all states, which brings back Baseline A's `match`.
- **Correctness:** this is the strongest compile-time guarantee available. Calling a transition from the wrong state is a type error, and state data is only reachable in its own state. It only helps when the call order is known statically (builders, setup and teardown sequences, a protocol driven by one async task). For externally driven events (GUI input, D-Bus signals) every event needs a runtime `match` on the wrapper enum, and most of the guarantee is lost at that boundary.
- **Concision:** verbose. You write one `impl` per state plus the wrapper enum, so 60–100 lines for the drag machine.
- **Sources:** http://cliffle.com/blog/rust-typestate/ ; https://docs.rust-embedded.org/book/static-guarantees/typestate-programming.html ; https://yoric.github.io/post/rust-typestate/ ; the statig FAQ on typestate versus event-driven machines (https://github.com/mdeloof/statig#faq)

---

## Core crates (read from the published sources)

### statig 0.4.1 (2025-07-05)

- **Maintenance:** 808 stars, 10 open issues and PRs. The last commit was 2025-11-29 (an unreleased fix for swapped `before_transition` arguments, #77), and no release has followed. Open PRs from 2026 (#79, #80, #83, #84 "Fix main branch") have not been merged, so it is slow but not abandoned. It has a single maintainer (mdeloof).
- **Downloads:** 4.71M total, 197k in the last 90 days. There are only 7 published reverse dependencies, so the volume comes from unpublished applications or CI.
- **License / MSRV / edition:** MIT. Declared MSRV is 1.66 and the crate is edition 2021. The `async` feature in fact needs Rust 1.85 or later (`AsyncFnOnce`, issue #71). The macro depends on `proc-macro-error2`, which cargo-audit flags as unmaintained (issue #81). There are no edition-2024 bug reports.
- **API style:** a proc-macro attribute on an `impl` block. `#[state_machine]` sits on the impl, `#[state]`/`#[superstate]`/`#[action]` on its methods. It can also be used without macros by implementing the traits.
- **Dispatch and allocation:** the generated `State` enum (leaf states with their local data) and a borrowing `Superstate<'_>` enum are dispatched with `match`. An event walks up the superstate chain while handlers return `Super`. Transitions compute the shortest exit/entry path. Nothing is heap-allocated, it is `no_std`, and it has no runtime dependencies (serde and bevy are optional).
- **Features:**
  - Hierarchy: yes, nested superstates.
  - Guards: plain Rust in the handler; there is no declarative guard syntax.
  - Entry/exit actions: yes.
  - Data in states: yes, as enum-variant fields, shared with superstates by reference.
  - Shared context: yes, both the `self` storage and an external `&mut Context`.
  - Async: yes, behind a feature.
  - Timers and deferred events: no.
  - History: no.
  - Typestate: no.
  - Parallel regions: no.
  - Introspection hooks: before/after dispatch and before/after transition.
- **Invalid events:** a handler returns `Super`, and `Top` treats everything as handled, so unhandled events are silently ignored.
- **Compile-time guarantees:** state-local data is only reachable from that state and its superstates. There is no transition table, so any handler may transition to any state.
- **Concision:** about 40–50 lines for press/drag/hover, because each state is an ordinary function with a `match`.
- **Limitation:** handlers get borrowed (`&mut`) state data and the event as `&Event`, so moving owned, non-`Clone` data into the next state needs `mem::take`. There are no timers and no queue.
- **Sources:** https://crates.io/crates/statig ; https://docs.rs/statig/0.4.1 ; https://github.com/mdeloof/statig ; https://github.com/mdeloof/statig/issues/71 ; https://github.com/mdeloof/statig/issues/81 ; the README "Implementation" section in the crate tarball

### rust-fsm 0.8.0 (2025-07-21)

- **Maintenance:** 262 stars and 1 open issue. The last commit was the 0.8.0 release on 2025-07-21. Community PRs adding guards (#17, #18) were closed unmerged on 2025-12-29. Quiet but intact.
- **Downloads:** 1.52M total, 349k in 90 days, from only 4 published reverse dependencies.
- **License / MSRV / edition:** MIT, no declared MSRV, edition 2021, DSL on syn 2.
- **API style:** a declarative function-like proc-macro DSL (`state_machine! { Closed(Unsuccessful) => Open [SetupTimer], ... }`) generating an implementation of the `StateMachineImpl` trait. You can also implement the trait by hand.
- **Dispatch and allocation:** `consume()` calls `transition(&State, &Input) -> Option<State>` and then `output(&State, &Input)`, each a `match`. Nothing is allocated, and it is `no_std` without the `std` feature.
- **Features:**
  - Hierarchy, guards, entry/exit, context, async, timers, history: none.
  - Data in states: no in the DSL (the README says so); only by hand-implementing the trait.
  - Outputs: Mealy outputs (`[Output]`).
  - Diagrams: Mermaid doc diagrams behind a feature.
  - Typestate: no.
- **Invalid events:** `Err(TransitionImpossibleError)`.
- **Compile-time guarantees:** only the declared (state, input) pairs exist in the generated `transition`.
- **Concision:** very short for flat unit-state machines (about 10 lines), but the drag-with-data case cannot be expressed in the DSL.
- **Limitation:** a pure transition table with no data, guards or context. It suits a Mealy-machine description and little else.
- **Sources:** https://crates.io/crates/rust-fsm ; https://docs.rs/rust-fsm/0.8.0 ; https://github.com/eugene-babichenko/rust-fsm ; `src/lib.rs` (`StateMachine::consume`) ; https://github.com/eugene-babichenko/rust-fsm/pull/17

### sm 0.9.0 (2019-11-09): UNMAINTAINED (repository archived)

- **Maintenance:** github.com/rustic-games/sm was archived on 2020-12-10 (192 stars). There have been no releases since 2019.
- **Downloads:** 1.95M total and 65k in 90 days, sustained by `appinsights`.
- **License / edition:** MIT OR Apache-2.0, edition 2018. The macro depends on syn 0.15, which adds an ancient syn to the build graph.
- **API style:** a typestate declarative macro (`sm! { Lock { InitialStates {..} TurnKey { Locked => Unlocked } } }`) generating `Machine<State, Event>` with zero-sized states. A `Variant` enum of every state-by-event pair handles runtime matching.
- **Features:** typestate only. States are unit ZSTs and carry no data. There is no hierarchy, guards, actions, context or async.
- **Invalid events:** compile error (the method doesn't exist).
- **Limitation:** abandoned, no data in states, and the variant enum grows with states × events.
- **Sources:** https://crates.io/crates/sm ; https://github.com/rustic-games/sm

### smlang 0.8.0 (2024-08-09)

- **Maintenance:** 242 stars and 11 open issues. The last commit was 2025-01-31. `master` is 13 commits ahead of v0.8.0 (syn 2 migration, async entry/exit) with no release. Hierarchical states have been an open request since 2022 (#31). Slow but used in production (ethercrab, Quartiq's `booster` and `thermostat-eem`).
- **Downloads:** 1.10M total, 254k in 90 days.
- **License / MSRV / edition:** MIT OR Apache-2.0. MSRV is undeclared (issue #87). Edition 2018. The released macro uses syn 1.
- **API style:** a declarative proc-macro DSL in Boost.SML syntax: `*Idle + Press(Pos) [hit_test] / start_drag = Dragging(DragData)`.
- **Dispatch and allocation:** generated `process_event` is a nested `match self.state { .. => match event { .. } }`. Guards and actions are methods on a user-implemented `StateMachineContext` trait. Actions return the destination state's data, which is moved in. It is `#![no_std]` with no allocation and no runtime dependencies besides the macro.
- **Features:**
  - Hierarchy: no (#31).
  - Guards: yes, with boolean guard expressions `[a && !b]`.
  - Entry/exit: generated `on_entry_*`/`on_exit_*` hooks.
  - Data in states: yes, and lifetimes are allowed.
  - Event data: yes.
  - Shared context: yes, plus an optional per-call `temporary_context`.
  - Async: guards and actions can be native `async fn` in traits.
  - Wildcards and patterns: `_ + Ev`, `A | B + Ev`.
  - Internal transitions: yes.
  - Timers, deferred events, history, typestate, regions: none.
  - Diagrams: Graphviz behind a feature.
- **Invalid events:** `Err(Error::InvalidEvent)`. All guards false gives `Err(TransitionsFailed)`. A guard or action error gives `GuardFailed(E)` or `ActionFailed(E)`.
- **Compile-time guarantees:** only declared transitions are generated, and state data exists only in its variant. Guards and actions are trait methods, so the compiler demands every one is implemented.
- **Concision:** about 25–35 lines for the drag machine (a 4-line table plus the context impl).
- **Limitation:** flat machines only, and the last release lags `master`.
- **Sources:** https://crates.io/crates/smlang ; https://github.com/korken89/smlang-rs ; https://github.com/korken89/smlang-rs/blob/master/docs/dsl.md ; `smlang-macros/src/codegen.rs` (error enum and `process_event`) ; https://github.com/korken89/smlang-rs/issues/31

### finny 0.3.0 (2026-09-30)

- **Maintenance:** 73 stars and 1 open issue. The single author (hashmismatch) revived it after four quiet years (0.2.0 was 2022-01-24) in a two-day burst of commits on 2026-09-29/30: edition 2024, async FSMs on tokio, tracing in place of slog, serde save/restore and a web inspector. The commit history reads like a large, possibly tool-assisted modernization. It is too soon to call it maintained.
- **Downloads:** 27k total, 3.7k in 90 days.
- **License / edition:** MIT OR Apache-2.0, edition 2024, macro on syn 3. Dependencies are `arraydeque`, `heapless 0.7`, `derive_more 0.99`, and optionally tokio, tracing and serde.
- **API style:** a proc-macro over a builder-style function (`#[finny_fsm] fn my_fsm(mut fsm: FsmBuilder<..>) -> BuiltFsm { fsm.state::<A>().on_event::<E>().transition_to::<B>().guard(..).action(..) }`). States and events are plain structs. Closures are lifted into generated code at compile time.
- **Dispatch and allocation:** generated `match` dispatch. **All state structs live at once** in a generated `<Fsm>States` struct, with a `CurrentState` enum per region. The event queue is `VecDeque` by default (allocates), with heapless and array queues for `no_std`. Timers use std threads or tokio.
- **Features:**
  - Hierarchy: yes, as sub-machines.
  - Guards: yes.
  - Entry/exit: yes.
  - Data in states: per-state struct storage (persistent, constructed up front), not variant data.
  - Shared context: yes, and generic.
  - Async: yes (`FsmAsyncBuilder`, tokio).
  - Timers: yes, per state. Event queue with run-to-completion: yes.
  - History: no.
  - Typestate: no.
  - Orthogonal regions: yes.
  - Inspection: yes (`Inspect`, tracing, web inspector).
- **Invalid events:** `Err(FsmError::NoTransition)`. Queued-event failures are reported to the inspector.
- **Compile-time guarantees:** the transition graph is validated at compile time, including rejecting a transition that is unreachable behind an unguarded one.
- **Concision:** about 35–50 lines (builder chain plus state, event and context structs).
- **Limitation:** state data exists even when the state is inactive, so it does not make invalid data unrepresentable. The default queue allocates. The project has a bus factor of one and a fresh rewrite.
- **Sources:** https://crates.io/crates/finny ; https://docs.rs/finny/0.3.0 ; https://github.com/hashmismatch/finny.rs ; `finny_derive/src/codegen.rs` (states store) ; `finny/src/fsm/queue.rs`

### sfsm 0.4.3 (2022-05-15): UNMAINTAINED

- **Maintenance:** gitlab.com/sfsm/sfsm last committed on 2022-06-01, with project activity last on 2023-07-24.
- **Downloads:** 23k total, 463 in 90 days.
- **License / edition:** MIT, edition 2018, macro on syn 1.
- **API style:** a declarative macro (`add_state_machine!(Name, Init, [States], [A => B])`) plus traits. States are structs implementing `State` (`entry`/`execute`/`exit`). A transition is `Transition<B>` (`guard() -> TransitGuard`, `action`) together with `From<A> for B`, so data moves from the old state to the new one.
- **Dispatch:** **polled**, not event-driven. `step()` runs `execute()` and then polls each outgoing transition's `guard()`. Events arrive through a separate `add_messages!` push/poll mechanism. It uses an enum with no allocation and is `no_std`.
- **Features:**
  - Hierarchy: by nesting a machine inside a state struct.
  - Guards: yes.
  - Entry/exit: yes.
  - Data in states: yes, moved with `From`.
  - Fallible machines: yes, with an error state.
  - Async, timers, history, typestate: none.
- **Invalid events:** not applicable, since transitions are guard-polled.
- **Limitation:** abandoned, and its step/poll model fits control loops rather than event-driven GUI or protocol code.
- **Sources:** https://crates.io/crates/sfsm ; https://gitlab.com/sfsm/sfsm ; `sfsm-base-0.4.3/src/lib.rs` and `fallible.rs`

### machine 0.3.0 (2019-06-14): UNMAINTAINED

- **Maintenance:** rust-bakery/machine was last pushed on 2021-08-06 and has 12 open issues.
- **Downloads:** 40k total.
- **License / edition:** MIT/Apache-2.0, with syn 0.15 and quote 0.6.
- **API style:** declarative proc-macros (`machine!(enum Traffic { Green { count: u8 }, .. })`, `transitions!(Traffic, [(Green, Advance) => Orange])`, `methods!`). States are structs carrying data, and you implement `on_<msg>` on each state.
- **Invalid events:** the machine moves to an automatically added `Traffic::Error` variant, which is sticky and carries no information.
- **Limitation:** abandoned. The proc-macro also writes generated code and `.dot` files to `target/machine` at compile time, which can fail in a read-only Nix build sandbox. There is no hierarchy, guards or async.
- **Sources:** https://crates.io/crates/machine ; https://github.com/rust-bakery/machine ; `src/lib.rs` docs

### typestate 0.8.0 (2021-09-02): UNMAINTAINED (repository archived)

- **Maintenance:** rustype/typestate-rs was archived (last push 2023-07-04). It has 154 stars and has been on 0.8.0 since 2021.
- **Downloads:** 43k total.
- **License / edition:** MIT OR Apache-2.0, edition 2018.
- **API style:** a proc-macro on a module (`#[typestate] mod m { #[automaton] struct A; #[state] struct S { data }; trait S { fn to_t(self) -> T; } }`). It checks the automaton at compile time (initial and final states, reachability, productive states) and generates enums for branching transitions. It can emit diagrams.
- **Features:** typestate with data in states. Branching returns a generated enum. There is no hierarchy, actions, async or runtime dispatch.
- **Limitation:** abandoned. Its guarantees are the same as hand-written typestate plus graph lints.
- **Sources:** https://crates.io/crates/typestate ; https://github.com/rustype/typestate-rs

### state_machine_future 0.2.0 (2018-11-10): UNMAINTAINED (repository archived)

- **Maintenance:** fitzgen/state_machine_future was archived (last push 2019-07-11). It depends on `futures 0.1`.
- **Downloads:** 279k total, 10.6k in 90 days, from legacy dependents.
- **What it did:** derived a futures-0.1 `Future` from an enum of states with typestate-checked `poll_<state>` transitions.
- **Successors:** there is no direct successor crate. Native `async fn` and `async` blocks are compiler-generated state machines and replaced it. `simple_coro` (about 2.6k downloads in 90 days) abuses async/await for coroutine-style machines, and `gen` blocks are still nightly-only. For a protocol driven by one task, a plain `async fn` is the modern equivalent; for externally driven events it does not apply.
- **Sources:** https://crates.io/crates/state_machine_future ; https://github.com/fitzgen/state_machine_future ; https://crates.io/crates/simple_coro

### statum 0.9.0 (2026-06-09)

- **Maintenance:** 260 stars and 5 open issues. The last commit was 2026-08-31, with very rapid churn: 0.8.0 to 0.8.10 between 2026-04-12 and 04-15, then 0.9.0. Commits are prefixed `[verified]`, which suggests agent-driven development. Open issue #21, "PRD: simplify Statum into Lite with Validators core", signals that the API may change again.
- **Downloads:** 39k total, 3.9k in 90 days.
- **License / MSRV / edition:** MIT, MSRV 1.93. The facade crate is edition 2021. The macro crate is about 13k lines on syn 2. `linkme` is pulled in only with `introspection`.
- **API style:** typestate through attribute macros. `#[state] enum S { Draft, InReview(Data), Published }` generates one marker type per variant. `#[machine] struct M<S> { shared fields }` holds shared context. `#[transition] impl M<Draft> { fn submit(self, ..) -> M<InReview> }` declares edges. `#[validators]` rebuilds a typed machine from stored data into a `SomeState` enum for runtime matching.
- **Features:**
  - Typestate: yes.
  - Data in states: yes (`transition_with(data)`).
  - Shared context: the machine fields.
  - Branching: returns are an enum or `Result`.
  - Hierarchy, entry/exit, async-specific support, timers, history: none.
  - Rehydration and validation from persistence: yes.
  - Introspection: a graph and `MachineTransitionRecorder` behind a feature.
- **Invalid events:** compile error. At runtime the boundary is a `SomeState` match.
- **Compile-time guarantees:** the strongest of any maintained crate. Methods and data exist only on the legal phase.
- **Concision:** about 40–60 lines for the drag machine, and GUI events still need a wrapper `match`.
- **Limitation:** suited to protocol and workflow lifecycles (polkit conversation, transfer job). For externally driven GUI events it reduces to Baseline A plus macros.
- **Sources:** https://crates.io/crates/statum ; https://docs.rs/statum/0.9.0 ; https://github.com/eboody/statum ; https://github.com/eboody/statum/issues/21

### state-machines 0.34.0 (2026-11-01)

- **Maintenance:** 138 stars and 0 open issues, pushed 2026-11-01. There have been 23 versions since 2025-10-10, of which the first 5 are yanked, and it jumped from 0.9 to 0.34 within 2026. The author (seuros) maintains the Ruby `state_machines` gem, and the README calls it "a learning-focused Rust port". Feature growth is extremely fast: history, regions, deadlines, snapshots, invoked child machines. It has 10 published reverse dependencies, mostly the author's own crates.
- **Downloads:** 20k total, 5.9k in 90 days.
- **License / MSRV / edition:** MIT OR Apache-2.0. **MSRV 1.99**, the current stable, which is above Sekka's declared 1.90–1.95, though the pinned nixpkgs rustc is 1.99.0. Edition 2024, macro on syn 3.
- **API style:** a declarative macro DSL in the Ruby gem's style (`state_machine! { name, initial, states: [..], events { e { guards: [..], before: [..], transition: { from, to } } } }`). It generates **both** a typestate API (`Machine<C, State>`, where `light.next()` returns `Result<Machine<C, Next>, (Machine<C, Prev>, GuardError)>`) and, optionally (`dynamic: true` or the feature), a runtime `Dynamic<Machine>` with `handle(Event)`.
- **Features:**
  - Hierarchy: yes (superstates via `SubstateOf`).
  - Guards: yes, plus `unless` and guarded branching.
  - Entry/exit: yes (`lifecycle:` hooks).
  - Data in states: yes (`Open(OpenData)`), but dynamic-mode data may be `None` ("lazy data initialization") unless a `data:` factory is declared.
  - Shared context: yes.
  - Async: yes.
  - Timers: state-scoped deadlines.
  - History: yes, shallow and deep.
  - Typestate: yes.
  - Orthogonal regions: yes.
  - Snapshots: serde snapshot and restore.
  - Introspection: Mermaid/JSON export and graph validation.
  - `no_std`: yes.
- **Invalid events:** compile error in typestate mode. In dynamic mode, `Err(DynamicError::InvalidTransition{..})` and related variants; a machine interrupted mid-async-dispatch becomes `Poisoned`.
- **Compile-time guarantees:** typestate mode is fully checked. Dynamic mode checks only at runtime.
- **Concision:** about 30–40 lines of DSL for the drag machine, plus callback impls.
- **Limitation:** the newest-Rust MSRV, very high API churn, and a single author. Its huge semantics surface is newer than its test history.
- **Sources:** https://crates.io/crates/state-machines ; https://docs.rs/state-machines/0.34.0 ; https://github.com/state-machines/state-machines-rs ; README sections "Dynamic Dispatch Mode", "State Data Accessors", "Error Handling" and "History States" in the 0.34.0 tarball

### edfsm 0.12.1 (2026-10-02)

- **Maintenance:** github.com/titanclass/edfsm has 36 stars and 8 open issues, pushed 2026-10-02. It is maintained by a company (Titan Class P/L), and its sibling crates are `edfsm-machine` and `edfsm-kv-store`.
- **Downloads:** 24k total, 5.8k in 90 days.
- **License / edition:** Apache-2.0, edition 2021, with `edfsm-macros` as its only runtime dependency.
- **API style:** a proc-macro attribute on an `impl Fsm` block containing a mini-DSL (`command!(Idle => Start => Started => Running); event!(..); ignore_command!(..); state!(Running / entry)`). It follows an event-sourcing split: commands produce events through effects, events change state, and state can be rebuilt by replaying events.
- **Dispatch:** generated `match` on `(state, command)` and `(state, event)`, with no allocation and `no_std`. `edfsm-machine` runs an FSM as a future.
- **Features:**
  - Entry handlers: yes. Exit handlers: no.
  - Data in states: yes (state structs inside a user enum).
  - Effect handlers / context: yes (`SE`).
  - Async: through `edfsm-machine`.
  - Hierarchy, guards (beyond returning `Option`), timers, history, typestate: none.
- **Invalid events:** if `ignore_*` declarations exist, matching is exhaustive and undeclared pairs fail to compile. Otherwise unmatched pairs return `None`.
- **Limitation:** the command/event/effect formalism adds ceremony for GUI modes. It is a good fit for replayable protocol logs.
- **Sources:** https://crates.io/crates/edfsm ; https://github.com/titanclass/edfsm ; `src/lib.rs` (`Fsm::step`)

---

## Newer and long-tail crates

### stateforward-sml 2.0.0 (2026-11-02)

- **Maintenance:**
  - Its history is a detached copy of smlang-rs (korken89, ryan-summers and others are the top contributors), not a GitHub fork. A single maintainer (gabewillen) has continued it since 2026-07.
  - 6 releases in 4 months, including two majors. The first 2.0 PR was reverted the same night, and 2.0 removed the `state()`/`context()` getters (`docs/migrating-to-2.0.md`).
  - 1 star, 0 reverse dependencies. docs.rs has no 2.0.0 docs; 1.5.0 built.
- **License / MSRV / edition:** MIT OR Apache-2.0 (a Boost licence file is also shipped), MSRV 1.90, edition 2018, `#![no_std]`, `forbid(unsafe)`. The package is renamed in use: `sml = { package = "stateforward-sml" }`.
- **API style:** a declarative Boost.SML DSL (`src + Ev [guard] / action = dst`, `state<T>`, `defer`, `(H)` history, adjacent tables forming composites) plus a generated context trait.
- **Dispatch and allocation:** nested `match`. The default `NoPolicy` path has no heap and no `dyn`, and its defer/process queues are bounded.
- **Features:**
  - Hierarchy: yes (composite trees).
  - Guards: yes, with boolean expressions.
  - Entry/exit: yes.
  - Data in states: yes, produced by the action.
  - Shared context: yes.
  - Async: yes.
  - Timers: no. Deferred events: yes.
  - History: shallow only.
  - Orthogonal regions: yes.
  - Exception rows and completion transitions: yes.
- **Invalid events:** `Err(InvalidEvent)` or `Err(TransitionsFailed)`, configurable with `unexpected_event<E>` rows.
- **Concision:** about 25–30 lines.
- **Limitation:** a young API with rapid churn from one maintainer. Outside code reads state through a visitor closure.
- **Sources:**
  - https://crates.io/crates/stateforward-sml
  - https://github.com/stateforward/sml.rs
  - https://github.com/stateforward/sml.rs/blob/main/macros/src/codegen.rs
  - https://github.com/stateforward/sml.rs/blob/main/docs/sml-cpp-parity.md
  - The README sections "Orthogonal and composite machines" and "DSL at a glance" in the 2.0.0 tarball

### moku 0.4.0 (2026-02-14)

- **Maintenance:** one author, 83 commits of steady work since 2024-10. Last commit 2026-02-14, 18 stars, 1 open issue (docs), 0 reverse dependencies. It has a trybuild compile-fail suite.
- **License / MSRV / edition:** MIT OR Apache-2.0. No declared MSRV; it needs at least 1.75 for return-position `impl Trait` in traits. Edition 2021, `no_std` with `default-features = false`.
- **Runtime dependencies:** moku-macros, bitflags and log.
- **API style:** `#[moku::state_machine]` on a module, plus a `TopState` or `Substate<Parent>` impl for each state struct.
- **Dispatch and allocation:**
  - The generated machine is a monomorphized tree of nested enums.
  - Inactive states do not exist, and nothing is allocated per transition.
  - The `std` feature stores the machine name as a `String`.
  - Every event and transition calls `log::info!`, so the experiment must filter or compile out logging.
- **Features:**
  - Hierarchy: yes.
  - Guards: plain Rust.
  - Entry/exit: yes. `enter(ctx)` constructs the state and may redirect; `exit(self)` consumes it.
  - Data in states: yes, first-class.
  - Shared context: typed access to ancestor states.
  - Async, timers, history, regions: no. It uses a polling `update()`.
- **Invalid events:** they bubble up to the top state and are dropped silently. `Response::Drop` stops the bubbling.
- **Limitation:**
  - Transition targets are a `Copy` unit `State` enum, so the transition graph is not checked.
  - Data built in `enter` comes only from context, so event data (such as a press position) has to be staged in an ancestor state first.
- **Sources:** https://crates.io/crates/moku ; https://github.com/harrisonmg/moku ; https://docs.rs/crate/moku/0.4.0/source/src/lib.rs (`StateEnum`, `Substate::enter`/`exit`)

### hsmc 0.6.0 (2026-05-04)

- **Maintenance:** one author. Eight releases in 13 days, then silence. 2 stars, 0 reverse dependencies. The README claims Creusot, cargo-mutants and Miri work, and the code is probably LLM-assisted.
- **Design:** a declarative `statechart!` DSL plus a generated `FooActions` trait. It dispatches through a `u16` event id and `&'static [u16]` action lists, uses heapless fixed-capacity queues and timers, and is `no_std`.
- **Features:**
  - Hierarchy, entry/exit: yes.
  - `after`/`every` timers: yes.
  - Async: tokio or embassy.
  - Guards: no, by design.
  - Data in states: no.
- **Gotcha:** the generated code tests `cfg(feature = "tokio"|"embassy")`, which is evaluated in the caller's crate.
- **Verdict:** excluded. It lacks guards and state data.
- **Sources:** https://crates.io/crates/hsmc ; https://github.com/swaits/hsmc/blob/main/hsmc-macros/src/codegen.rs

### kaori-hsm 0.1.1 (2024-03-09): UNMAINTAINED

- **Design:** a hierarchical machine in Samek's QP style. State handlers are fn pointers, and entry, exit and init arrive as synthetic events. Generics are erased with an `unsafe` `transmute` that relies on `repr(Rust)` layout.
- **Features:** no state data, and no way to query the current state (#39 has gone unanswered). The last push was 2024-05.
- **Sources:** https://crates.io/crates/kaori-hsm ; https://github.com/AntoineMugnier/kaori-hsm ; https://docs.rs/crate/kaori-hsm/0.1.1/source/src/state.rs

### scxml 0.2.3 (2026-09-13)

- **What it is:** an SCXML/JSON document model with a parser, validator, DOT/Mermaid export and a test simulator. Its own README says it is "not a runtime executor".
- **Simulator:** it uses `HashMap<&str, ..>`, allocates a `Vec` per `send`, and never runs actions.
- **Maintenance:** passively maintained, with agent-assisted commits (`AGENTS.md`).
- **Verdict:** excluded. It may be useful later for exporting diagrams.
- **Sources:** https://crates.io/crates/scxml ; https://github.com/GnomesOfZurich/scxml/blob/main/src/simulate.rs

### rustate 0.3.0 (2025-04-28)

- **What it is:** an XState-like runtime builder with string state ids.
- **Per-event cost:** `async fn send` takes a tokio `RwLock`, clones the whole context and the matched transition, and boxes futures for every guard and action. It has 13 runtime dependencies.
- **Maintenance:** no release for 18 months. The repository has since become an "agent" prototype.
- **Verdict:** excluded.
- **Sources:** https://crates.io/crates/rustate ; https://docs.rs/crate/rustate/0.3.0/source/src/machine.rs

### xmachines-runtime 1.1.0 (2026-11-04)

- **What it is:** a port of the XState v6 interpreter that takes machine JSON and a registry of named Rust guards and actions.
- **Per-event cost:** every `send` converts the event to `serde_json::Value`, and logic sits behind `Arc<dyn Logic>`. The crate's own `cost-model.json` puts microsteps at roughly 100–830 ns.
- **Maintenance:**
  - The GitLab project was created on 2026-10-29.
  - 0.0.0 was published and yanked on 2026-11-03, and 1.0 and 1.1 were released on 2026-11-04.
  - Its MSRV is 1.99.
- **Verdict:** excluded.
- **Sources:** https://crates.io/crates/xmachines-runtime ; https://gitlab.com/xmachin-es/xmachines-rs ; https://docs.rs/crate/xmachines-runtime/1.1.0/source/src/actor.rs

### banish 1.4.1 (2026-04-03)

- **What it is:** a rule-block DSL that expands into a `loop { match idx }` fixed point running to completion. It is a control-flow block, not a stored machine you feed events to.
- **Gotcha:** choosing the entry state with `dispatch(expr)` string-matches the variant name and panics if nothing matches.
- **Signals:** 118 stars but only 351 downloads.
- **Verdict:** excluded.
- **Sources:** https://crates.io/crates/banish ; https://github.com/LoganFlaherty/banish/blob/main/banish_derive/src/lib.rs

### yasm 0.6.0 (2026-07-17)

- **What it is:** a `macro_rules!` DSL plus a runtime instance with unit-only states and inputs.
- **Per-transition cost:** `transition()` builds a fresh `Vec` through `can_accept()`, then does up to 3 SipHash lookups for `Box<dyn Fn>` callbacks and pushes to the history.
- **README claims:** "compile-time prevention of invalid transitions" is false. The only compile-time check is a const assertion against duplicate pairs.
- **Downloads:** about 10k of its downloads are on 0.6.0, with 0 dependents.
- **Verdict:** excluded.
- **Sources:** https://crates.io/crates/yasm ; https://github.com/kookyleo/yasm/blob/main/src/instance.rs

### extfsm 0.14.1 (2026-08-17)

- **Origin and status:** written at Juniper in 2017 and modernized in 2026 (edition 2024).
- **Design:** a `HashMap<(State, Event), Box<dyn Fn>>` inside `Rc<RefCell>`, cloning the state and event on each lookup. It has an internal event queue, which is reallocated on every processing cycle.
- **Features:** no guards and no hierarchy. A `NoTransition` error is documented as "FSM needs to be shut down".
- **Verdict:** excluded.
- **Sources:** https://crates.io/crates/extfsm ; https://docs.rs/crate/extfsm/0.14.1/source/src/lib.rs

### mode 0.4.1 (2021-08-13): dormant

- **What it is:** a `Family`/`Mode` trait container that takes the current mode out of an `Option`, runs your closure, and puts the result back. If you follow the README's `Box<dyn Trait>` style, every transition allocates.
- **Assessment:** it adds almost nothing over Baseline A.
- **Sources:** https://crates.io/crates/mode ; https://docs.rs/crate/mode/0.4.1/source/src/automaton.rs

### sad_machine 1.0.0 (2021-09-08): UNMAINTAINED

- **What it is:** a fork of `sm` that generates per-state "how did I get here" enums. States cannot carry data.
- **Typestate is partial:** transition methods take `&self`, so the old state is not consumed.
- **Status:** built on syn 1, last push 2022-03.
- **Sources:** https://crates.io/crates/sad_machine ; https://github.com/steinuil/sad_machine

### fsmentry 0.4.0 (2025-04-07)

- **Maintenance:** last release and push on 2025-04-07, 44 stars, 0 issues ever, one author (aatifsyed), 1 reverse dependency (speakeasy). It looks finished rather than abandoned.
- **License / edition:** MIT OR Apache-2.0, edition 2021, with no runtime dependencies. The generated code is `no_std`.
- **API style:** a graph DSL (`Idle -> Pressed(Pos) -> Dragging(Drag) -> Idle`) generating:
  - a state enum;
  - an `entry()` method returning an `Entry` enum of per-state handle structs.
- **What the handles do:** each handle exposes `as_ref`/`as_mut` for the state's data, and **only the legal outgoing transitions** as methods. Moving to a data-carrying state requires supplying its data, and you get the old data back when you leave.
- **Dispatch and allocation:** `match` plus `mem::replace`, with no allocation.
- **Features:**
  - Hierarchy: only by nesting machines.
  - Guards: plain Rust.
  - Entry/exit: no.
  - Context, async, timers, history: no.
- **Invalid events:** there is no event model. An illegal edge is a compile error. A handle built against the wrong state panics, or is UB with `unsafe(true)`, but only code inside the generated module can do that.
- **Concision:** about 5 lines of graph plus about 20 lines of `match (state.entry(), event)`.
- **Assessment:** it sits between Baselines A and B, giving compile-time legal edges over a runtime enum at zero cost.
- **Sources:** https://crates.io/crates/fsmentry ; https://docs.rs/fsmentry/0.4.0 ; https://github.com/aatifsyed/fsmentry ; README in the 0.4.0 tarball

### stateless 0.5.0 (2026-04-07)

- **What it is:** a pure transition-table macro (`*Idle + Start = Running`, `_ + Reset = Idle`) generating unit `State`/`Event` enums and `process_event(&self, Event) -> Option<State>`. The function is an `if matches!` chain that LLVM can lower to a jump table.
- **Extras:** `ALL`, `DOT` and `valid_events()`.
- **Signals:** 1 star, one author, no runtime dependencies.
- **Limitation:** no data, guards or actions; you put them in your own wrapper.
- **Sources:** https://crates.io/crates/stateless ; https://github.com/matthewjberger/stateless/blob/main/src/lib.rs

### typed-fsm 0.4.8 (2025-12-02)

- **What it is:** a `macro_rules!` DSL generating an enum plus `match`, with per-state `entry`/`process`/`exit` closures, data-carrying variants and `&mut Ctx`. It is `#![no_std]` and allocates nothing.
- **Bug:** the `concurrent` feature keeps its lock and heapless queue in **per-type statics**, so instances share them and a queued event can reach the wrong instance.
- **Maintenance:** 12 versions in 10 days, Claude-assisted (it ships a `CLAUDE.md`), quiet since 2025-12.
- **Sources:** https://crates.io/crates/typed-fsm ; https://github.com/afmiguel/typed-fsm/blob/main/src/fsm.rs

### statecraft 0.1.0 (2026-07-07)

- **What it is:** typestate helper traits (`Transitionable<N>`, `TryTransitionable<N, R>`) plus trivial derives. It is the Hoverbear pattern with names attached.
- **Downloads:** 81k, with **0 reverse dependencies** and daily counts from 1 to 3,154. That looks like private CI; releases are cut by Zetier's `aperturec-release[bot]`.
- **Verdict:** excluded, because it adds nothing over Baseline B to benchmark.
- **Sources:** https://crates.io/crates/statecraft ; https://github.com/Zetier/statecraft/blob/main/src/lib.rs ; https://crates.io/api/v1/crates/statecraft/reverse_dependencies

### statecraft-fsm 0.2.0 (2026-08-20)

- **What it is:** `#[fsm]` on an impl block, with an async-only `apply`. States are `Copy` and carry no data.
- **Gotcha:** `state` is a public field, so code can bypass the transitions despite the "compile-time validated" claim.
- **Other details:** MSRV 1.96.1, one author.
- **Sources:** https://crates.io/crates/statecraft-fsm ; https://github.com/Sebkd/statecraft/blob/main/statecraft-fsm-macros/src/codegen.rs

### obzenflow-fsm 0.4.0 (2026-11-03)

- **What it is:** an async-first `fsm!` DSL over a runtime engine.
- **Per-event cost:** a `HashMap<(String, String), Arc<dyn Fn>>` lookup with two `to_string()` calls, a boxed future, a `Vec` of actions, and `async_trait` boxing.
- **Dependencies:** tokio is mandatory.
- **Downloads:** they are not explained by its two dependents.
- **Sources:** https://crates.io/crates/obzenflow-fsm ; https://github.com/ObzenFlow/obzenflow-fsm/blob/main/src/machine.rs

### timed-fsm 0.5.0 (2026-07-19)

- **What it is:** a `TimedStateMachine` trait whose `on_event`/`on_timeout` return a `Response` of actions plus declarative timer Set/Kill commands, each a `Vec`. It is part of the awase keyboard remapper.
- **Maintenance:** 21 of its 24 commits carry Claude trailers.
- **Assessment:** it is not an FSM library. Its **timers as pure outputs** pattern is worth copying by hand for double-click detection and frame deadlines.
- **Sources:** https://crates.io/crates/timed-fsm ; https://github.com/cuzic/awase/blob/main/crates/timed-fsm/src/response.rs

### state-shift 2.1.1 (2025-03-20)

- **What it is:** typestate builder attributes (`#[type_state]`, `#[require]`, `#[switch_to]`). All states share the struct's fields, so states carry no data of their own.
- **Assessment:** it is a builder-ordering tool, not a runtime machine.
- **Other signals:** one version (0.1.0) is yanked, 271 stars, quiet since 2025-03.
- **Sources:** https://crates.io/crates/state-shift ; https://github.com/ozgunozerk/state-shift

### qubit-state-machine 0.11.1 (2026-11-04)

- **What it is:** a lock-free shared lifecycle table with three engines:
  - a standard `HashMap` engine that does an `ArcSwap` CAS and allocates an `Arc` per transition;
  - a flat-array `u64` CAS engine;
  - a typed wrapper over the flat-array engine.
- **Features:** no data, guards or context.
- **Dependencies:** about 10 sibling `qubit-*` crates.
- **Sources:** https://crates.io/crates/qubit-state-machine ; https://github.com/qubit-ltd/rs-state-machine/blob/main/src/fast/fast_state_machine.rs

### async_fsm 0.1.4 (2025-05-11): dormant

- **What it is:** a tokio actor that dispatches through `HashMap<State, Box<dyn Transition>>` and an `async_trait` box per event.
- **Bugs:** transitions get only `&Data`, so they cannot mutate the context. It also inserts every distinct event value into a `HashMap<Event, Instant>`, which grows without bound.
- **Sources:** https://crates.io/crates/async_fsm ; https://github.com/lookas/async_fsm/blob/main/async_fsm/src/lib.rs

### squads-rustfsm 0.3.1 (2025-09-25) and rustfsm 0.1.0 (2021-04-22)

- **squads-rustfsm:** a 2-line re-export of a 2025 snapshot of Temporal's in-tree `fsm!` macro (`A --(Event(T), handler)--> B;`), published unofficially by `edward-sqds`.
  - The generated code hard-codes `::rustfsm::` and `::derive_more::`.
  - The snapshot predates Temporal's codegen-determinism fix (sdk-rust #1633, 2026-10-06), which matters for reproducible Nix builds.
- **rustfsm 0.1.0:** its `on_event_mut` clones the whole machine on every event.
- **Temporal today:** the macro lives in `temporalio/sdk-rust` (`crates/macros/src/fsm_impl.rs`) and drives about 18 workflow machines.
- **Recommendation:** to try an "events in, commands out" design, vendor that file rather than depend on either crate.
- **Sources:**
  - https://crates.io/crates/squads-rustfsm
  - https://crates.io/crates/rustfsm
  - https://github.com/temporalio/sdk-rust/blob/main/crates/macros/src/fsm_impl.rs
  - https://github.com/temporalio/sdk-rust/pull/1633

### ironstate 0.1.4 (2026-08-29)

- **What it is:** `TransitionRules::transition(&self, &Event) -> Option<Self>`. The transition function sees **no context**.
- **Per-transition cost:** a successful `apply` clones the state three times and calls a boxed clock. Listeners are a `Vec<Box<dyn Fn>>`.
- **Maintenance:** 24 of its last 30 commits are "Generated with Claude Code".
- **Sources:** https://crates.io/crates/ironstate ; https://github.com/kassian-dev/ironstate

### tramli 3.9.0 (2026-09-04)

- **What it is:** a request-workflow engine. Flows live in a `HashMap<String, FlowInstance>`, context is a `HashMap<TypeId, Box<dyn CloneAny>>`, and each step formats several strings with `format!("{:?}")`. Guarded steps deep-clone the context.
- **Release history:** 37 of its 39 versions were published between 2026-04-07 and 04-09, LLM-assisted.
- **README claims:** "build-time" validation means a runtime `build()` call.
- **Sources:** https://crates.io/crates/tramli ; https://github.com/opaopa6969/tramli

---

## Complementary correctness tools (not FSM libraries)

- **proptest-state-machine 0.9.0** (2026-10-10, 3.86M downloads, 1.0M in 90 days, MSRV 1.88) does model-based testing: it generates transition sequences against a reference model. It fits the experiment's correctness criterion for whichever crate wins. https://crates.io/crates/proptest-state-machine
- **stateright 0.31.0** (2025-07-27, 15.4M downloads) is an explicit-state model checker for actor and protocol designs. It could check the polkit and file-transfer protocols. https://crates.io/crates/stateright
- **trybuild** compile-fail suites are how the better crates test their own guarantees. statig, smlang, stateforward-sml, fsmentry, moku, statum and hsmc list it in their published manifests. The experiment can reuse it to show which illegal transitions each candidate rejects at compile time.

---

## Recommended shortlist (8 crates plus the 2 baselines)

The shortlist is chosen so that each design point Sekka cares about has at least one representative and no two candidates measure the same thing.

| # | Candidate | Design point it represents | Best-fit Sekka targets | Benchmark notes |
|---|---|---|---|---|
| A | **Baseline: enum + `match`** | Zero-cost floor and concision reference | All of them; Hotaru's parser already is this | Use `mem::replace` for owned moves |
| B | **Baseline: typestate by hand** | Compile-time-correctness ceiling | Polkit conversation, transfer job | Measure the runtime wrapper enum too |
| 1 | **statig** | Most-used hierarchical machine; method-per-state with state-local data, context and async | Hover ⊃ press ⊃ drag modes; polkit conversation (async) | Both sync and `async` builds; note the borrowed-data and `mem::take` cost |
| 2 | **smlang** | Most-used declarative transition-table DSL (Boost.SML) with guards, actions, state and event data, async, `no_std` | Transfer job with conflict prompts; press/drag | Released 0.8.0 (syn 1); also consider a `master` git pin |
| 3 | **stateforward-sml** | The same DSL lineage plus hierarchy, orthogonal regions, shallow history and bounded defer queues | Nested GUI modes; compositor scheduling (regions, defer) | Compare per-transition cost and code size directly against smlang. Pin the exact version because of churn |
| 4 | **finny** | Only maintained crate with built-in **timers** plus a run-to-completion queue, regions, sub-machines and a compile-time graph check | Per-frame compositor scheduling; double-click and long-press timers | Use the array or heapless queue (the default `VecDeque` allocates). All states are stored at once, so measure size |
| 5 | **fsmentry** | Runtime enum with **compile-time legal-edge handles** and data in states, zero cost | Transfer job conflict prompts; polkit conversation; drag | Tests whether it gives typestate-like safety at Baseline A's cost |
| 6 | **statum** | Maintained typestate with data in states and typed rehydration | Polkit conversation driven by one async task; lifecycle steps persisted to disk | Expect zero transition cost; measure concision and the runtime `SomeState` boundary |
| 7 | **state-machines** | Upper bound on expressiveness: hierarchy, guards, entry/exit, history, regions, deadlines, async, in both **typestate and dynamic** modes | A stress test on the richest machines in the catalog | MSRV 1.99 is above Sekka's declared 1.90–1.95 but matches the pinned rustc 1.99.0. Benchmark both modes. Its dynamic-mode data may be `None` |
| 8 | **rust-fsm** | Most-used flat Mealy table (unit states with outputs) | VT parser (unit states plus actions), small modes | `consume` does two `match`es (`transition` and then `output`); compare against Baseline A on the VT hot path |

**Reserves, if one of the above fails to build or is disqualified:**
- **moku:** hierarchical, carries data, allocation-free. Filter `log` out.
- **stateless:** a smaller pure table than rust-fsm.
- **typed-fsm:** enum plus `match` with entry/exit. Avoid its `concurrent` feature.
- **edfsm:** an event-sourced command/event split for replayable protocol logs.
- **hsmc:** timers without heap, but no guards or state data.
- **Temporal's in-tree `fsm!`:** vendor it to try an "events in, commands out" design.

**Patterns to borrow, not depend on:** timed-fsm's "timer commands as outputs", which is how to keep timers out of the machine for the compositor hot path.

## Excluded, and why

**Unmaintained, archived or legacy:**
- **sm:** archived 2020; ZST states without data; syn 0.15.
- **sad_machine:** unmaintained fork of sm; no data.
- **machine:** abandoned 2019–21; syn 0.15; writes into `target/machine` during macro expansion.
- **typestate:** archived 2023.
- **state_machine_future:** archived; futures 0.1. Native `async fn` superseded it.
- **sfsm:** unmaintained since 2022; its step/poll model fits control loops, not events.
- **kaori-hsm:** unmaintained; unsafe `transmute`; no state data; current state can't be queried.
- **mode:** dormant; adds little over Baseline A.
- **async_fsm:** dormant; tokio actor; unbounded event map.
- **rustfsm 0.1.0:** clones the machine on every event; superseded in-tree at Temporal.
- **Not individually evaluated:** these older crates with negligible use since 2022–2024: fsm (2015), microstate (2016), mealy (2016), funfsm (2017), macro_machine (2017), statechart (2017), state_machine (2018), async-hsm (2020), statemachine-macro (2020), apparat (2021), fi-night (2021), director (2022), etsm (2022), dd_statechart (2022, GPL-3.0), rs_state_machine (2022), nefsm (2023), finite-state-machine (2023), fsm-macro (2023), fluent_state_machine (2024), krossbar-state-machine (2024), hfsm (2024), rsfsm (2024), stateflow (2025), and typestates (renamed to typestate, with yanked versions).

**Heap, `dyn` or interpreted dispatch on every event (wrong for GUI and hot paths):**
- **obzenflow-fsm:** `String` keys in a `HashMap` plus a boxed future per event; tokio required.
- **extfsm:** `HashMap` + `dyn` + `RefCell`; no guards.
- **yasm:** a `Vec` per transition; unit states; README overclaims.
- **qubit-state-machine:** an atomic CAS and an `Arc` per transition on the standard engine; no data or guards; ~10 sibling dependencies.
- **rustate:** clones the context and takes an async `RwLock` per event.
- **xmachines-runtime:** JSON on every event; days old; MSRV 1.99.
- **tramli:** a request-workflow engine with `TypeId` maps and `format!` per step.
- **scxml:** a document model and simulator, not a runtime.

**Wrong shape for Sekka's targets:**
- **statecraft:** typestate helper traits that add nothing over Baseline B; 0 dependents.
- **state-shift:** a builder-ordering typestate tool with no per-state data.
- **statecraft-fsm:** async-only; states without data; public `state` field.
- **banish:** a run-to-completion rule block, not an event-driven machine.
- **timed-fsm:** a timer-command protocol, not an FSM; its pattern is worth borrowing.
- **ironstate:** transitions cannot see context; clones per step.
- **hsmc:** no guards or state data; kept only as a reserve.

**Unofficial or fragile packaging:**
- **squads-rustfsm:** an unofficial snapshot of Temporal's macro with hard-coded `::rustfsm` paths, predating Temporal's codegen-determinism fix.

**Framework-bound:** bevy_state, seldom_state, bevy_gearbox and bevy_fsm need Bevy ECS. proptest-state-machine and stateright are testing tools; see the complementary tools above.

**Domain or engine crates, not general FSM libraries, and with negligible adoption:**
- fsm-rs (72 downloads), tokio-fsm (203), id_effect_fsm, juncture (LLM-agent graphs), purrgress, statewright-engine, harel-machines (an LCA utility), typestate-groups, qail-workflow, ironflow-engine.
- Protocol-specific "state machine" crates such as sp-state-machine, quinn-proto and pio.

**Name collisions found by the searches** (not state-machine crates): moore (HDL compiler), sml (markup), finite (floats), gearbox, awsm, transitions (value tweening), stately (CRUD).

## Cross-cutting observations for the experiment

- **Maintenance:**
  - The established leaders are stale. statig hasn't released since 2025-07 and has unmerged 2026 PRs. smlang hasn't released since 2024-08, with 13 commits unreleased. rust-fsm never merged guards.
  - The feature-rich newcomers (stateforward-sml, state-machines, finny 0.3, statum) are 2026 single-author bursts with high churn. Pin exact versions.
- **Timers are rare:** only finny, state-machines (deadlines), hsmc and xmachines-runtime have them. For compositor frame scheduling, expect to keep timers outside the machine, following timed-fsm's "timer commands as outputs" pattern.
- **Invalid-event handling splits three ways:**
  - silently ignored: statig, moku, hsmc;
  - `Err`: smlang, stateforward-sml, rust-fsm, finny, state-machines dynamic mode, edfsm;
  - compile error: statum, fsmentry, state-machines typestate mode, Baseline B.
- **Build graph cost:**
  - smlang 0.8 still pulls in syn 1, while finny and state-machines use syn 3. statig depends on `proc-macro-error2`, which cargo-audit flags as unmaintained.
  - Measure clean-build time and binary-size deltas alongside the per-transition cost.
- **Hidden per-event costs:** of the zero-alloc candidates, moku logs every event at info level. finny's default queue allocates. statig passes events by reference, so moving owned event data into a state means a clone or `mem::take`.
