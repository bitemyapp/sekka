# Review: impl-sml (stateforward-sml =2.0.0, imported as `sml`)

Crate source read: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/stateforward-sml-2.0.0/`
(`docs/dsl.md`, `docs/sml-cpp-parity.md`, `docs/migrating-to-2.0.md`, `src/policy.rs`,
`src/utility.rs`, `tests/process_dsl.rs`, `tests/orthogonal_dsl.rs`,
`tests/sml_cpp_examples.rs`) and `stateforward-sml-macros-2.0.0/src/`
(below, `macros/` = that directory).

What I ran:

- `cargo test --locked -p impl-sml` (target dir `sme-review-sml`):
  all green. 12 spec tests, 4 `crate_findings` tests, trybuild ran 36 cases, all ok.
- `metrics/sme.py compile-fail` and `metrics/sme.py loc`.
- My own probes, in a scratch workspace outside the slot:
  `smlprobe/` (not kept).
  - `probe/tests/probe.rs`: sizes; a stale child state; composite A to composite B;
    a flat `process` against an anonymous row; an unhandled processed event in a flat
    machine; the crate's own sml.cpp `defer_and_process` port with `E1` sent directly.
  - `probe/tests/l1.rs`: L1 through a public internal event.
  - `sml-copy/`: a copy of `impl-sml/src`. In it I removed lock's no-op rows and
    sealed the internal events of lock and install. All 6 targets' conformance
    passes, and lock and install pass 3000-case differentials in release.
  - `sealcheck/`: outside code can't build the sealed events (E0423).

Nothing in the workspace was edited.

## 0. Why 37 cases when the spec defines 36

The 37 comes from an arithmetic slip in NOTES.md §4, which gives the tally as "5 rejected
(P2, P3, P4, P6, I6), 31 refused at run time, 1 accepted (I4)". There is no extra case.
`compile-fail/cases/sml/` holds exactly 36 `.rs` files, one per probe:

- 5 `reject`: P2, P3, P4, P6, I6.
- 30 `runtime`: P1, P5, K1–K6, T1–T6, W1–W5, L1–L7, I1–I3, I5. That makes 2+6+6+5+7+4 = 30, not 31.
- 1 `accept`: I4.

There are no duplicates and no stray files. Trybuild ran 36 cases, and the metrics table
fills each probe cell exactly once.

## 1. Idiom fidelity, per target

| Target | Rows in `sml!` | Share of spec rows in the DSL | Grade |
|---|---|---|---|
| pointer | 30 (20 child, 10 parent) | ~100% | idiomatic |
| wrap | 47 | ~100% | idiomatic |
| polkit | 23 | ~100% of rows; ~80% of effect selection | partial |
| transfer | 21 | ~100% of per-item rows; frames hand-coded | partial |
| lock | 34 | ~100% | idiomatic |
| install | 54 | ~100% | idiomatic (fix the public internal events, §6.1) |

- **pointer** (`impl-sml/src/pointer.rs:50-86`). The DSL holds a lot:
  - `Inside` is a real child table.
  - The `on_entry` hook resets the hover and the `on_exit` hook emits `UnderOff`/`HoverOff` (pointer.rs:79-80, 150-163).
  - `Outside` uses `/ defer = state<Inside>` for "enter, then as Hovering" (pointer.rs:75-77).
  - Owners and the edge press are state payloads.
  - `Removed` falls through child-first to the parent row (pointer.rs:63, 70-71, 83).

  Hand code is limited to actions (`track`, `release`). Every (event, state) cell of the
  spec table is a row. The hover is context data because a composite carries no payload
  (verified, §3). **idiomatic**.
- **wrap** (wrap.rs:35-87). A flat table of all four states and all 13 events, with
  wildcards for the "any other state" rows. The cursor, links and saved cursor are
  context, which the spec defines as machine-wide data anyway.
  - Guards run before actions, so each print row's guard predicts the outcome
    (`after`, wrap.rs:151) and the action recomputes it.
  - The choice-state alternative (`/ print = Printed`, then anonymous rows) would add a
    completion pass per cell. The double computation is an honest cost of the crate.

  **idiomatic**.
- **polkit** (polkit.rs:39-67). What sits in the DSL:
  - `Active` is a child table.
  - Entry hooks: `Active` opens the dialog, `Starting` starts the helper, `RetryWait` arms the timer.
  - Guard expressions such as `[current && exhausted]`.
  - The replaced request uses `defer` (polkit.rs:60). This is actually more literal than
    the self-transition the implementer first tried: it also covers the `!screen` case.

  What is hand code: the "end the request" rows (polkit.rs:60-64) and `ChooseIdentity`
  (polkit.rs:50) decide which exit effects to emit (`EndHelper`, `CancelRetry`) from two
  context mirrors of the child state (`helper`, `retry_armed`, polkit.rs:84-87, 94-104,
  189-195), not from the table. NOTES records why, and the reasons check out:
  - parent rows can't see the child state;
  - exit hooks can't see the target;
  - `CloseDialog` must follow the action's `Reply`.

  `ChooseIdentity` could be split per source state so that the table says what ends
  (§6, should). **partial**.
- **transfer** (transfer.rs:53-77). A flat table whose states carry the awaited item and
  aside. Guards compare items, and `Next` is a choice state with anonymous rows. The
  spec's nested merge sub-machine is plain code in the context: the frame stack, popping
  frames, `RemoveEmptySource`, and the folder `Outcome` that the spec calls entry/exit
  actions (transfer.rs:96-125, 133-150). sml's composites are a fixed tree, so a dynamic
  recursion can't be a child table. Honest, and recorded. The reference uses the same `Vec`. **partial**.
- **lock** (lock.rs:26-72). The most complete use of the crate:
  - an orthogonal root;
  - nested `Lockscreen ⊃ Locked`;
  - entry/exit hooks;
  - completions (`state<Locked> / release_lock = X`, `state<Lockscreen> / hint_unlocked = Unlocked`) that unwind in the spec's order;
  - `process(...)` for one region to talk to the other.

  `has_text`/`shake` are context data, since composites carry no payload. Two wrinkles:
  - The internal events are public (L1 hole, §2).
  - The no-op rows at lock.rs:62-65 aren't needed (§3).

  **idiomatic**.
- **install** (install.rs:25-88). Two child tables. Each stage runs from its entry hook,
  `Executing`'s entry/exit holds and releases the inhibitor, and a failure bubbles to
  `state<Executing> + StageFailed`. Workarounds:
  - The route is kept in the context: a child's initial state takes `Default`, and the
    codegen never builds a payload for a composite target (verified).
  - Preparation ends through processed `Ready`/`Stop`. These are public today, which is
    what makes I4 an accept. They can be sealed (§2, §6.1).

  **idiomatic**.

## 2. Compile-fail audit, with the "Transitions and forging" rule applied

**Forging means.** None stays public, so no `forge` case is due. NOTES.md §4 must say so
(rule 3). The reasons:

- The crate-generated state setters are gated on the `TestingPolicy` policy slot through
  the sealed `TestingAccess` trait (macros `composite_codegen.rs:801-817`,
  `codegen.rs:2050-2062, 2096`). These are `set_current_states`, `set_child_state`,
  `set_<child>_state` and `new_with_state_data`.
- `TestingPolicy` itself is a public type, so the gate alone doesn't hide them. What does
  is that every implementation machine is `XStateMachine<Ctx, NoPolicy | OneDeferred>`.
  Building a `TestingPolicy` machine needs a context value, and all six contexts (`Router`,
  `Screen`, `Agent`, `Job`, `Shell`, `Installer`) have private fields and no public
  constructor.
- 2.0 has no context getter (`docs/migrating-to-2.0.md`).
- Writing the public `machine` field can only move in another machine, which was itself
  reached through the transition API.
- `initialize()` re-runs only the root initial state's entry hook
  (`composite_codegen.rs:632-653, 1013-1016`), and none of the six root initial states has one.

**Cases that disagree with the rule or with the crate:**

| Case | Now | Should be | Why |
|---|---|---|---|
| `lock/runtime/L1-release-delay-before-confirmed.rs` | T | **A** as implemented, or T after §6.1 | The case shows only the `SleepPending` path, where a no-op row swallows `E::SleepWhileLocked`. Through the same transition API, `SessionStarted`, `Lock`, then `E::SleepWhileLocked` while `Locking`/`Held` emits `ReleaseDelay` before any confirmation (my `probe/tests/l1.rs`: `[ReleaseDelay]`, state `Locking/NotHeld`). A real `PrepareForSleep` afterwards then holds nothing, so the machine sleeps with the lock unconfirmed. The case's own comment admits it ("Sent while the delay is merely Held, that internal event does release it"). Under the new rule, a public internal event sent through `process_event` is the transition API, so this commits L1. |
| `install/accept/I4-deferred-without-space-check.rs` | A | T after §6.1 | Correct for the code as written: a public internal event `InstallEvents::Ready(true)` is sent through `process_event`. But the publicness is the implementation's choice, not the crate's (see below). |

All other cases are correct:

- The 5 `reject` cases fail first on the guarantee (unit variant `NativeDrag`, payload
  `Node` not `EdgePress`, missing argument, two fields against one). Their `.stderr`
  files match (trybuild green).
- Every `runtime` case asserts `Err`, the absence of the effect, and an unchanged state.
- P5 and L6 are T where enum/statig are R. That is honest: the hover and the shake must be
  context because composite states have no payload. The shake can't move into
  `Idle(u8)`/`Checking(..)` either, because `Locked`'s exit needs it, and a child exit
  hook would also fire on `Idle` to `Checking`.
- W1 and T3 are T where enum is A. That is right: there is no state setter or context write.

**The internal events can be sealed.** NOTES finding 5 says internal (processed) events
"are variants of the public events enum, so any caller can send them". That is
overstated, so I4's accept and the L1 hole are too harsh on the crate:

- `process(...)` takes any `syn::Expr` (`macros/parser/transition.rs:121-130, 151-160`),
  and the expression is evaluated inside the implementation's module.
- So declare the event as `event<T>` with a private-field payload, for example
  `pub struct Ready(bool); pub struct Stop(());` and
  `pub struct SleepWhileLocking(()); pub struct SleepWhileLocked(());`.
- Then `InstallEvents::Ready(Ready)` still exists, but outside code can't build a `Ready`
  (E0423, my `sealcheck/`).

I made exactly this change in `sml-copy/src/{install,lock}.rs`:

- rows `state<Preparing> + event<Ready> / prepare = Prepared(bool)` and
  `Held + event<SleepWhileLocked> / release_for_sleep = NotHeld`;
- `process(Ready(true))`.

All 18 lock and 16 install traces pass, and both pass 3000-case differentials.

**Missing probe nuance (P2, should).** After `StartDrag` from `Pressed(n)`, the child's
storage still holds `Pressed(n)`. `m.machine.is(&PointerInsideStates::Pressed(Node(0)))`
answers `true` in `NativeDrag` (probe `stale_child_state_is_queryable`). This is
documented ("the child query also answers while the child is inactive").

- The owner can't be read: `visit_current_states` visits root states only.
- It isn't routed to: the child resets on re-entry.

So P2 stays a valid `reject`. The case comment or NOTES should still mention the stale
retained child, and that `state()` must test the root first, as `pointer.rs:342-357` does.

## 3. NOTES.md claims checked

| Claim | Verdict |
|---|---|
| Finding 1: composite self-transition | Real defect, understated (see list below) |
| Finding 2: sync `process(E)` not queued | Real defect (see list below) |
| Finding 3: unhandled processed event aborts the step | Real defect; lock's no-op rows overstated (see list below) |
| Finding 4: flat orthogonal table needs `X` | Real generator bug (see list below) |
| Finding 5: internal events are public | Overstated: the implementation can seal them (§2) |
| Guards run before actions | Correct |
| Exit hooks can't depend on the target | Correct, with one nuance |
| Composite states carry no data | Correct |
| Defer queue 440 vs 80 bytes (pointer), 248 vs 72 (polkit) | Correct |
| Other sizes 48/48/36/40 | Correct |
| Wildcard fills only states with no row yet | Correct |
| Flat action takes a `Name(T)` payload by value | Correct |
| Edition 2018, MSRV 1.90 | Correct |
| LoC: sml 1713, lock 238 | Correct; comparison numbers stale |
| Tally 5/31/1 | Wrong (§0) |
| "`set_current_states` exists only with the sealed `TestingPolicy`" | Imprecise |

Detail on the claims that need more than a verdict:

- **Finding 1, composite self-transition: a real defect, and understated.**
  - Cause: `composite_lifecycle` (`macros/composite_codegen.rs:4559-4576`, with the same
    gate inline for completion rows at 4770-4785 and 4849-4864). It emits the child exit
    only for composite to non-composite, and the child reset and entry only for
    non-composite to composite.
  - So every composite-to-composite transition skips both, not only the self-transition.
    My probe `composite_to_composite::a_to_b`:
    - `state<Ka> + Swap = state<Kb>` logs only `["exit Ka", "enter Kb"]`: no `exit A2`, no `enter B1`.
    - Swapping back leaves `Ka` in `A2`, so the child is not reset: history without `(H)`.
  - This contradicts `docs/dsl.md`: "an explicit target equal to the source is an external
    self-transition, so exit and entry callbacks do run", and "leaving and re-entering the
    child resets it to its initial state".
  - The tree generator already has the right per-child exit and entry code
    (`scalar_exit_children`/`scalar_enter_children`, 2134-2139). Only the gate is wrong.
    No target has a composite-to-composite edge after polkit's switch to `defer`.
- **Finding 2, sync `process(E)` not queued: a real defect.** It is not a misuse.
  - Cause: the sync paths recurse (`let _ = self.__sml_process_event_unlocked(..)?;` at
    `macros/orthogonal_codegen.rs:1671`, `composite_codegen.rs:4423`, `codegen.rs:999`).
    Only async machines push to `self.pending`.
  - `docs/sml-cpp-parity.md` claims "allocation-free queues and post-transition iterative
    dispatch in sync/async flat, orthogonal, parent, and child tables". sml.cpp drains its
    process queue after the event's anonymous and deferred work.
  - Observable effects:
    - In orthogonal machines, later regions see P before E (the slot's test).
    - In flat machines too: `A + E / process(P) = B`, `B / to_c = C` logs
      `["P in B", "B->C (anonymous)"]`, where sml.cpp would handle P in C (my probe).
  - The crate's own test (`tests/orthogonal_dsl.rs:514-537`) ends in `[X, X]` either way,
    so it doesn't catch this.
- **Finding 3, an untaken processed event aborts the step: a real defect.**
  - The `?` returns `Err(InvalidEvent)` after the source transition. Later regions,
    the deferred drain and `stabilize` are all skipped.
  - The same generator's deferred drain tolerates `InvalidEvent`/`TransitionsFailed`
    (`orthogonal_codegen.rs:1711-1717`, `composite_codegen.rs:4457-4463`).
  - The crate's own sml.cpp port `defer_and_process` (`tests/sml_cpp_examples.rs:619-646`)
    passes only because its processed `E2` runs inside such a drain. Sent directly in
    `s1`, `E1` returns `Err` although the machine is in `s2` (my probe `direct`).
  - **Overstated:** "Lock adds no-op rows so its processed events are always taken
    (lock.rs:64-65)". In `sml-copy`, lock without lock.rs:62-65 passes 18/18 traces and the
    differential at 256 and 3000 cases. `handle` ignores the `Err`, and no later region or
    completion had work pending in those steps. The rows are defensive, not required.
- **Finding 4, a flat orthogonal table needs a row to `X`: a real generator bug.**
  `orthogonal_codegen.rs:768` emits `matches!(state, States::X)` unconditionally. The
  embedded path (1081) and the flat generator (`codegen.rs:1341`) check that `X` exists
  first. My two-region table without `X` fails with E0599 spanned over the macro. Nothing
  in the docs requires `X`.
- **"Guards run before actions": correct.** Each row is
  `if !handled && candidate && guard { exit; action; replace; entry; process; drain }`
  (`composite_codegen.rs:4519-4540`, `orthogonal_codegen.rs:1788-1809`). Nothing lets an
  action pick the target, apart from a choice state or an `exception` row.
- **"Exit hooks can't depend on the target": correct.** Exit callbacks get the temporary
  context and the source payload only (`composite_codegen.rs:4479-4489`,
  `orthogonal_codegen.rs:1744-1758`). Nuance worth a line in NOTES:
  `transition_callback(&self, old, new)` does see both root states
  (`composite_codegen.rs:851, 2558`). But it is `&self`, gets no temporary context, and
  runs after the action, so it can't emit effects here.
- **"Composite states carry no data": correct.** `target_has_data` requires
  `composite.is_none()` (`composite_codegen.rs:4495`), and the parent enums' composite
  variants are unit (`InstallStates::Preparing`, `LockStates::Lockscreen`).
- **Defer queue sizes: correct.** The probe measured `PointerStateMachine<Router>` at 440
  bytes against 80 with `OneDeferred`, and polkit at 248 against 72.
  - The default is `EventQueue<E, 16>` (`src/policy.rs:311-316`): `[Option<E>; N]` plus
    head and len (`src/utility.rs:160-164`), and `Option<PointerEvents>` is 24 bytes.
  - `sml_policies!(OneDeferred { defer_queue: DeferOne })` is the documented policy slot.
    One slot suffices: every defer row's target drains it in the same step. It is fair for the benchmarks.
- **The other sizes are correct**: wrap 48, transfer 48, lock 36, install 40 (probe `wrapper_sizes`).
- **Wildcard: correct.** A wildcard fills only states that have no row for the event yet (`macros/parser/mod.rs:957-972`).
- **Payload by value: correct.** A flat action takes a `Name(T)` payload by value (transfer.rs:160, 169).
- **Edition and MSRV: correct** (both crates' `Cargo.toml`).
- **LoC: correct for sml** (total 1713, lock 238). The comparison numbers are stale: enum is
  now 1195 with lock at 211, and statig is 1596.
- **The tally is wrong** (§0).
- **"`set_current_states` exists only with the sealed `TestingPolicy`" is imprecise.**
  `TestingPolicy` is public. The sealed part is `TestingAccess`. What actually protects the
  implementation is its private contexts.

## 4. Test integrity

- `tests/spec.rs` and `tests/compile_fail.rs` are identical to statig's, smlang's, finny's,
  fsmentry's, statum's, state-machines' and rust-fsm's, apart from the crate name and the
  doc comment.
- `NOT_EXPRESSED` is empty and there is no `#[ignore]`.
- `tests/crate_findings.rs` is an addition and weakens nothing. Each of its tests asserts
  the release's actual behaviour, with the documented expectation in a comment, and I ran all 4 green.

## 5. Benchmark distortions

None that the crate doesn't impose.

- **`&mut dyn Sink<Effect>` temporary context.** One virtual call per effect. A generic
  sink isn't possible: generic parameters are flat-only and must appear in every event
  (`docs/dsl.md`). This is the same choice as statig's worked example.
- **`Err(InvalidEvent)` per ignored event.** A small enum, no allocation.
- **`stabilize` after each handled event.** Crate-imposed.
- **Defer queue.** Pointer's `Outside` entries and polkit's replaced request use push,
  pop and re-dispatch, with a one-slot queue.
- **Wrap's double computation.** The guard predicts and the action prints, which is
  guard-before-action.
- **Allocation.** None per event. The defer clone is of `Copy` events, and transfer's
  `Vec` matches the reference.
- **`initialize()` is never called.** Harmless: no root initial state has an entry hook,
  and there is no anonymous row from an initial state.
- **Lock's no-op rows.** They add two match arms, which is negligible.

## 6. Required fixes

1. **must**: seal the internal events, or reclassify L1. Preferred: declare install's
   `Ready`/`Stop` and lock's `SleepWhileLocking`/`SleepWhileLocked` as `event<T>` with
   private-field payloads, as in `sml-copy/src/install.rs:25-27, 36-43, 73-74, 218-220` and
   `sml-copy/src/lock.rs:20-22, 42, 45, 52, 65-66, 232-236`. Lock then needs its own
   `release_for_sleep(&mut self, out, &SleepWhileLocked)`, because the composite generator
   rejects one callback used with different payload shapes. After sealing:
   - Rewrite I4 as `runtime/I4-deferred-without-space-check.rs`: the deferred route with
     too little room ends `Refused` and never emits `Prepared`; say in a comment that
     `Ready` can't be built outside (E0423).
   - Remove the `E::SleepWhileLocked` step and the "merely Held" caveat from L1, which stays `runtime`.
   - Rewrite NOTES finding 5.

   If you keep the events public instead, the new rule requires L1 to move to
   `accept/L1-release-delay-before-confirmed.rs`, with `SessionStarted`, `Lock`,
   `SleepWhileLocked` asserting `ReleaseDelay` while `LockPhase::Locking`. In that case,
   list L1 with I4 as the rule's reclassifications.
2. **must**: correct the NOTES §4 tally. Today it is 5 R, 30 T, 1 A, 36 cases. After fix 1
   it is 5 R, 31 T, 0 A.
3. **should**: add the rule-3 sentence to NOTES §4: no forging means stays public, for the
   reasons in §2 (TestingPolicy-gated setters, private contexts, no getters, `initialize()`
   harmless). Also fix the "sealed `TestingPolicy`" wording.
4. **should**: broaden finding 1 to every composite-to-composite edge (self, or A to B in a
   tree), citing `composite_lifecycle` and the inline completion gates. Add a
   `crate_findings` test for A to B: `["exit Ka", "enter Kb"]`, with `Ka` still `A2` after
   swapping back.
5. **should**: in finding 2, cite the parity-doc claim and add the flat case (a processed
   event is handled before the current event's anonymous rows).
6. **should**: lock.rs:62-65. Either drop the no-op rows, or say they are defensive and keep
   `process_event` from returning `Err`, not that lock needs them. Fix NOTES finding 3 to match.
7. **should**: in P2's comment or NOTES, note that the child keeps `Pressed(owner)` during
   `NativeDrag` and that `is(&PointerInsideStates::Pressed(_))` answers `true` there, so
   state queries must test the root first.
8. **should**: in NOTES' exit-hook claim, mention that `transition_callback(&self, old, new)`
   exists but has no temporary context and no `&mut`.
9. **should** (optional): split polkit's `ChooseIdentity` row per source state
   (`Starting|Prompted|Checking` / `RetryWait` / `Unavailable`), so the table rather than
   the mirrors says what it ends. The parent rows still need the mirrors.
10. **should**: refresh the comparison LoC numbers in NOTES §6 (enum 1195, enum lock 211, statig 1596).

## 7. Overall

The implementation represents stateforward-sml fairly and is rankable once fix 1 lands.
Every target puts its transitions in the crate's tables:

- real composites, entry/exit hooks, `defer`, completions, orthogonal regions and
  processed events where the spec has them;
- honest, recorded context workarounds where the crate can't hold the data (superstate
  data, child-state mirrors, transfer's recursive frames).

The crate findings are real defects, not misuses. Composite-to-composite lifecycle is
broken more widely than NOTES says. Sync `process` is not queued, contrary to the parity
doc. An untaken processed event aborts the step. The orthogonal `X` reference is unconditional.

The one place the implementation is too harsh on the crate is the public internal events.
Making them public gave I4 an accept and left an uncounted L1 hole, both of which a sealed
payload type avoids within the crate's idiom. With that fixed and the tally corrected, the
classifications (5 R, 31 T, 0 A, no forge) and the performance numbers stand.
