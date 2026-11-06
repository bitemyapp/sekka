# Review: impl-state-machines (state-machines =0.34.0)

Reviewer's checks: `cargo test --locked -p impl-state-machines` (12/12 spec
tests, 36/36 compile-fail cases), and the differential tests at
`SME_PROPTEST_CASES=3000 --release` (6 × 3000 cases pass). The crate source
I read: `state-machines-0.34.0/src/lib.rs`, `state-machines-macro-0.34.0/src/`
(`codegen/{typestate,dynamic,branching,scopes,utils}.rs`, `regions.rs`,
`validation.rs`), `state-machines-core-0.14.0/src/lib.rs`, and the README.
Experiments ran in a copy of the workspace, outside it:
`sm-exp/sme` (not kept)
(variants `impl-state-machines/src/{pointer_v1,wrap_v1,wrap_v2,hidden}.rs`,
driver `bench/src/bin/smexp.rs`). I edited nothing in the workspace.

The compile-fail audit applies the two rules added to IMPLEMENTING.md after
the implementer finished: "Transitions and forging", and "In a typed API a
transition stands for its event".

## 0. The brief's questions, answered from the source

- **Is there a context accessor?** No public one. `ctx` is a private
  field of the typed machine (`codegen/typestate.rs:161`). The wrapper's
  `inner: Option<AnyXState>` is private (`codegen/dynamic.rs:928`), and
  `AnyXState` is a private enum (`dynamic.rs:136`). A grep of the macro
  finds no `ctx()`, `context_mut()` or `into_parts`. **However,** the macro
  expands in the implementer's module, so module privacy lets code there
  reach `self.machine.inner` and `m.ctx`. The callbacks already rely on this
  (`self.ctx`, `self.__state_data_*`). A ctx-owned `Vec` drained through
  `inner` works, and the digests stay the same as enum's (§5). So the
  `Rc<RefCell<Vec>>` is the implementation's choice, not something the crate
  forces.
- **A way to pass `&mut` data into callbacks?** Only the context and the
  state data:
  - callbacks and hooks are called on `mut self`, so they may take
    `&mut self`;
  - local callbacks get `&payload`;
  - `data:` factories get `&mut payload` (`typestate.rs:826`, `:466`);
  - guards get `(&self, &ctx, &payload)` (`typestate.rs:600`).

  The context type has no lifetime parameter, and a generic `C` gets no
  bounds in the generated `impl<C> Name<C, Leaf>`. So the caller's sink
  can't be lent for one `handle`. A payload can't carry `&mut` either: the
  generated `XEvent` enum has no lifetime, and callbacks only see `&payload`.
  NOTES is right here.
- **A lighter dynamic wrapper?** No. `DynamicX` is the only synchronous
  runtime dispatcher. The others (`runtime::Runner`, `Parallel`, `regions:`)
  are heavier, async, and need the `runtime` feature.
- **`no_std` or static mode?** The facade is always `#![no_std]` + `alloc`
  (`state-machines-0.34.0/src/lib.rs:1`). There is no allocation-free or
  bookkeeping-free variant of the wrapper. Its `completions` `Vec` allocates
  only with `final_states`, which isn't used here.
- **What the wrapper stores** (`dynamic.rs:926-930`):
  - `epoch: u64`;
  - `scope_epochs: [u64; leaves + superstates]`, updated by `__sm_commit`
    on every committed external edge (`scopes.rs:45-47`), whether or not
    `runtime` is enabled;
  - `inner: Option<AnyXState>`;
  - `last_state`;
  - `completions: Vec<CompletionEvent>`.

  That is 40 bytes + 8 per state and superstate, plus the typed machine and
  its tag. NOTES is right.
- **Could `handle` drive the typed API directly?** Yes. `wrap_v2.rs` drops
  `dynamic: true`, holds its own `enum St { N(Screen<Normal>), … }` and
  dispatches the 48 (state, event) pairs to the typed methods. It needs
  `IntoSt` impls for the 16 generated outcome enums: about 110 lines of hand
  dispatch. Its digest matches enum's, and it saves about 16% on wrap
  (23.2 → 19.4 ns/event). That is the wrapper's whole share. The rest is
  inherent to the consume-and-rebuild typed transitions: they move the whole
  machine, and return `Result<(Outcome, usize), (Self, GuardError)>`, with a
  56-byte `GuardError`. IMPLEMENTING tells slots to use the dynamic one, and
  its cost is the crate's documented runtime path, so I don't require this.
- **`new_init_state` builds any state with its data absent:** confirmed.
  `dynamic.rs:295` with `empty_storage_inits`. It runs no entry hooks, and
  `into_x()` hands back the typed machine. Under the new rule this is
  *forging* (§2). I checked that it can be hidden:
  - the macro goes in a private submodule;
  - `DynamicX` is not re-exported;
  - naming it then fails with E0603 (`sm-exp/.../src/hidden.rs`).

  `into_dynamic()` and the wrapper's methods stay reachable on a value.
- **Regions need the async `runtime` feature:** confirmed.
  - `regions.rs:538` emits `__sm_require_runtime!()`, which is a
    `compile_error!` without `runtime` (`src/lib.rs:37`).
  - The generated `handle` is `pub async fn` (`regions.rs:391`).
  - Routes split payloads; there are no cross-region guards.
- **Callbacks can take `&mut self` despite the README:** confirmed.
  README:1327 says "Callbacks take `&self`, not `&mut self`". In the code,
  transition methods are `fn e(mut self …)`, and callbacks are called as
  `self.cb(..)` / `new_machine.cb(..)`. In my test, a `data:` factory and an
  entry hook both mutated `self.ctx`.
- **Macro generics aren't hygienic:** confirmed. `ctx_generics` emits a bare
  `<C>` (`codegen/utils.rs:98`). `state_machine! { name: C, … }` fails with
  E0574 and four E0109 ("type arguments are not allowed on type parameter
  `C`").
- **Also found:** `initialize()` is generated on **every** leaf type, not
  only the initial one (`typestate.rs:189, 333`). It re-runs the active
  entry hooks on a machine already in that state: my test counted the hook
  twice. That is one more public forging means (§2).

## 1. Idiom fidelity, per target

In all six targets, the crate's DSL holds the states, superstates, guards,
the before/after/exit/entry placement, and the data:

- leaf and superstate data;
- `data:` factories;
- `kind: external`;
- `hierarchical: true` (child first);
- `branching` with typed outcome enums.

Hand code beside the DSL:

- the effect bodies of the callbacks;
- the facade's event mapping;
- in `lock`, the composition of two regions.

None of the targets hides an `enum` + `match`.

| target | DSL holds | hand code beside it | rows in the DSL | grade |
|---|---|---|---|---|
| pointer | `Inside(Hover)` superstate with exit hook `clear_hover` (pointer.rs:97-106). Owner states' data from factories. `moved`/`removed` child-first (108-118, 163-171). 3-way `press` branching (119-137). The before/after split gives the spec's track-vs-deliver order (139-145). | effect bodies (194-308), facade map (329-344) | ~100% | idiomatic |
| wrap | `Autowrap`/`NoAutowrap` superstates and 13 events (104-185). One callback for all four states through the `Mode` consts on the state types (189-229). | `ends_pending` predicts what `print` will do (75-84 vs 87-101), because guards run before callbacks (inherent: `branching.rs` selects before callbacks run). The REP loop is in the facade, one transition per cell (322-329, recorded). | ~95% | idiomatic |
| polkit | `Active(Request)` with exit hook `close_dialog` (78-88). Conversation numbers from factories. `kind: external` re-entry for another cookie (97-100). Back-off as branching (133-149). | `Request` takes a placeholder `Default` (superstate data has no factory: `validation.rs:275-285`, recorded). OpenDialog is emitted from the factory (212-215, recorded). | ~100% | idiomatic |
| transfer | `Running(Frames)` superstate. Item data from 8 factories. **next** is a factory (313-316). Every "item done" is a `has_next`/fallback pair (113-207). | The merge frames are data (`Frames::next`, 39-56), not nested machines: `invoke_child` needs `runtime` and async (recorded). `push_top_frame` after Start (348-350, recorded). | ~100% | idiomatic |
| lock | `Locker` nests `Lockscreen ⊃ Locked` with lifecycle hooks (44-111). `Delay` is a second machine (270-302). | The two regions are composed in `Lock::handle` (363-407). The cross-region guard becomes the `sleep(LockPhase)` payload (379-383). The "settled" relay is a phase comparison (396-404). The crate's regions are async-only (verified), so this is honest and recorded. | ~90% | partial |
| install | `Preparing`/`Executing` superstates. Executing's StayAwake/AllowSleep hooks. One generic `run` entry hook per stage through the `Step` consts (75-93, 167-227). `hierarchical` StageFailed (155-162). | `Failed{erased}` in after-callbacks (edge-dependent, recorded) | ~100% | idiomatic |

**Under-used, minor:** `final_states` could declare install's four terminal
states and transfer's `Finished`. The macro would then reject outgoing edges
from them. The cost: `__sm_record_completions` would push a
`CompletionEvent` into the wrapper's `Vec`, which means an allocation per
machine. Optional; worth a line in NOTES.

## 2. Compile-fail audit (under the new rules)

The 36 cases compile or fail as declared, and every `.stderr` matches
(trybuild passes). For the 22 rejects, the first error is the guarantee:
no such method or variant, or the wrong state type. None of them stops at a
typo, an import, or a private constructor.

### The forging rule reclassifies

| case | was | now | reason and new probe |
|---|---|---|---|
| P6 owner-mismatch | accept | **runtime** | Went through `new_init_state` only, which is forging. Every typed edge into `Pressed`, `DragsOnlyPressed` and `DragsOnlyDragging` declares a `data:` factory. But the typed accessor returns `Option`, so the shape doesn't rule out "no owner". New probe: drive `press` and `capture` into each owner state and assert that `*_data()` is `Some(owner)`. |
| W1 pending-off-the-edge | accept | **runtime** | Went through `new_init_state` only. By transitions, `PendingWrap` is entered only by `print` under `ends_pending`, by `restore_cursor` under `saved_pending`, or by `set_autowrap`, `tab` and RI keeping a pending wrap. New probe: print to `cols-2` (outcome `Normal`), then once more (outcome `PendingWrap`), and assert the column. |

### The "a transition stands for its event" clause reclassifies

I compiled each new probe in the scratch copy; the first errors are quoted.

| case | was | now | new probe and first error |
|---|---|---|---|
| P1 release-without-owner | runtime | **reject** | Every typed path that emits `Up` is an owner state's `release`. It demands a `Router<Pressed\|DragsOnly*>`, which only `press`, `capture` or `moved` can build. Probe: `Router::<Pressed>::release(hovering, lift)`, giving `E0308 expected Router<Pressed>, found Router<Hovering>`. This matches typestate's and statum's P1. |
| L1 release-delay-before-confirmed | accept | **reject** | The only typed release of a pending sleep's delay is `Delay<SleepPending>::lock_settled()`, which is named for "lock confirmed or lock screen left". Calling it *is* that event, just as `prepared.confirm(..)` is the confirmation. Probe: apply the other release, `Delay::<Held>::sleep(pending, LockPhase::Locked)`, giving `E0308 expected Delay<Held>, found Delay<SleepPending>`. Keep a note in NOTES: no type ties `lock_settled` to the `Locker`; only the facade's relay does (lock.rs:396-404). |
| I4 deferred-without-space-check | accept | **reject** | The only typed path into a deferred `Prepared` is `evaluated`'s `fits_unpacked` candidate. The in-memory route's `cached` doesn't exist on `Evaluation`. Probe: `evaluation.cached()`, giving `E0599 no method named cached found for Installer<Evaluation>` (found for `Cache` and `Cancelling`). This matches typestate's updated I4. |

Unchanged:

- **K2, L2, T5 stay `runtime`.** Forging (`*_data_mut`) would defeat them,
  and the rule keeps such cases `runtime`.
- **T6 stays `runtime`.** Optional, and fair game under the new clause:
  split `Asking` into a mergeable and a non-mergeable state, branching from
  `probed`. That costs about 4 DSL lines, and T6 would then be rejected (no
  `Listing` variant).
- **P2, P4, I6 stay `reject`.** The generic `state_data_<s>()` accessors
  compile on every state type and return `None` there. That matches the
  baselines' convention, where another variant's shape isn't this state's.
  NOTES already says so.

**Tally after these changes:** reject 25, runtime 11, accept 0, plus forge
cases. Was 22/10/4.

### Forging: hide what can be hidden, then one forge case per target

1. **`new_init_state` can be hidden** (rule 1), as I checked:
   - put each `state_machine!` and its `impl` blocks *and the facade* in a
     private submodule;
   - re-export the typed machine, the state markers, the outcome enums, the
     payload and data types, `Ctx` and the facade;
   - don't re-export `DynamicX`;
   - drop the facades' `pub fn machine()` (nothing uses it), or make it
     `pub(crate)`.

   Then:
   - `wrap` keeps no forging means: no data states, no lifecycle hooks, a
     private context. Write no forge case for it, and say so in NOTES §4.
   - If hiding is declined, `F-new-init-state.rs` (W1) is wrap's forge case.
2. **These stay public:**
   - the typed `with_<s>_data`, `<s>_data_mut` and generic
     `state_data_<s>_mut` (`typestate.rs:997, 1054`);
   - the wrapper's `set_<s>_data` and `<s>_data_mut`, reachable through
     `into_dynamic()` (`dynamic.rs:690`);
   - `initialize()` on every leaf (re-runs entry hooks).

   Each target with data writes one forge case:
   - pointer `F-pressed-data-mut.rs`: rewrite a typed `Pressed`'s owner, and
     `release` delivers `Up` to a node that never took the press (P1);
   - polkit `F-starting-data-mut.rs` (K2);
   - transfer `F-probing-data-mut.rs` (T5);
   - lock `F-checking-data-mut.rs` (L2);
   - install `F-with-prepared-data.rs`: an in-memory `Prepared`, then
     `.with_prepared_data(true)`, then `confirm(true)`, then `step_done` ×3
     runs `BuildOnTarget` with no unpacked-room check (I4).

   Name the other means in each case's comment, including `initialize()`:
   lock re-emits RequestLock, OpenSurface and LockedHint(true); install's
   `Installer<Erase>::initialize()` repeats StayAwake and Run(Erase).

### Probe quality

- **T1 (stays reject):** the probe calls `replacing.disposed(..)`, which
  "acknowledges" a disposal. It doesn't attempt to dispose. Use the
  reference reading instead: placement failed, now try to dispose. That is
  `replacing.placed((item, Placed::Skip))` → `Restoring`, then
  `restoring.placed((item, Placed::Ok))`, giving E0599 (no `placed` on
  `Job<Restoring>`).
- **No missing probes;** every ID has a case.

## 3. NOTES.md claims (spot-checked)

Correct:

- `DynamicX` calls the typed methods (dynamic.rs `handle_match_arms`); it is
  no second engine.
- Data is `None` after `new` or `new_init_state`; entering a state
  initialises it with `Default` or the factory's value.
- Typed accessors return `Option`. README:577 still says "no Option
  wrapper!".
- README:1327's `&self` claim is wrong.
- `SubstateOf` impls exist, but no blanket impl uses them (typestate.rs
  tail).
- Regions and timers need `runtime` and are async.
- The hook order: around, guards, before, exits, factory, entries, after.
- Superstate data can't come from a factory (`validation.rs:275-285`).
- The reserved event names (`validation.rs:230-240`, plus `schema`).
- The wrapper's layout and sizes.
- Three syn majors in the lock (1.0.109, 2.0.119, 3.0.9).
- The tally of 22/10/4.
- 23 published versions (0.1.0 … 0.34.0, from the index cache).

Wrong or overstated:

1. **"Cost: one allocation per machine"** (§3, `out.rs` docs). It is
   **two**: the `Rc` box and the `Vec::with_capacity(16)` buffer
   (out.rs:20). Measured: new+drop of `Pointer` takes 26.4 ns against 0.2
   for enum.
2. **"Nothing more direct works" / "forced by the missing context access"**
   (§3, §9). Overstated. A ctx-owned `Vec` drained through the
   module-private `inner` works, through the same privacy the callbacks use.
   It is faster on pointer and replays (§5).
3. **§4's L1, P6, W1, I4 explanations** are superseded by the two rules
   (§2). "That's how P6, W1 and I4 are accepted" now describes forge cases.
4. **"23 versions in about a year, 0.9 → 0.34 within 2026":** the count is
   right. The dates can't be checked offline (the index has none), so mark
   them as from the survey.
5. **"The context has no accessor at all"** is true of the public API. Add
   that in-module access through `inner` exists.

## 4. Test integrity

- `tests/spec.rs` is identical to impl-statig's apart from the crate name,
  and `tests/compile_fail.rs` to impl-rust-fsm's apart from the slot name.
  Both match impl-enum's plus the differential tests.
- `NOT_EXPRESSED` is empty, with no `#[ignore]`.
- Conformance: 105/105 traces.
- Differential: 256 cases (debug) and 3000 cases (release) per target, all
  passing.

## 5. Benchmark distortions

Measured with `SME_SMOKE=1` workloads, best of 15 runs (replays best of
300), `taskset -c 5`, load ≈ 3.8 from other agents, ns/event. Every
variant's digest equals enum's.

| workload | enum | v0 as submitted | v1: ctx `Vec` drained via `inner` | v2: typed API direct + ctx `Vec` |
|---|---|---|---|---|
| pointer-stream | 5.98 | 17.2 / 18.6 | **14.6 / 14.7** | – |
| wrap-80 | 3.84 | 23.4 / 23.2 | 23.1 / 26.1 | **19.4 / 19.5** |
| wrap-200 | 3.85 | 23.0 / 25.1 | 23.3 / 23.4 | **19.7 / 19.5** |
| replay-pointer (21 traces, 4.6 events each) | 6.77 | 22.1 / 22.4 | **16.7 / 16.7** | – |
| replay-wrap (14 traces) | 4.31 | 19.9 | 20.6 | 19.5 |

- **`Rc<RefCell<Vec>>` (avoidable).**
  - It costs a `RefCell` flag per push and an extra `Rc` allocation per
    machine.
  - The replays build one machine per trace, so the 26 ns of construction
    spreads over 3–6 ns of every replay event (4.6–10 events per trace).
  - Fix 5 removes about 15% on pointer-stream and about 25% on
    replay-pointer; wrap doesn't change.
- **The dynamic wrapper** costs about 16% of wrap. That is inherent to the
  crate's runtime path; it can be avoided only by hand-written dispatch.
- **Inherent: the per-event consume-and-rebuild.**
  - the `inner.take()` → typed method → re-wrap moves of 48–120-byte
    machines;
  - the epoch and `scope_epochs` commit;
  - a `DynamicError` with static strings for each ignored event (no
    allocation).
- **Also inherent: guards before callbacks.** Wrap's predict-then-print
  duplication follows from it. So does REP running as n separate `handle`s.
- **No per-event cost to fix beyond the buffer:** no per-event allocation,
  cloning, `dyn` dispatch or queue.
  - Transfer's `Frames` `Vec` is allocated per job, which is the same as
    enum's in the replays.
- **Ranking:** neither fix changes the rank. Even v1 and v2 stay the
  slowest slot: pointer 14.7 against statig's 10.9; wrap 19.4 against about
  11 for typestate and statum.

## 6. Required fixes

1. **must:** Reclassify P6 and W1 from `accept` to `runtime`, with the
   transition-API probes described in §2. Move their `new_init_state`
   bodies into forge cases if `new_init_state` stays public (fix 3).
2. **must:** Reclassify P1 (`runtime` → `reject`), L1 (`accept` →
   `reject`) and I4 (`accept` → `reject`), with the probes in §2's second
   table. Write the `.stderr` with `TRYBUILD=overwrite`; the first errors
   are E0308, E0308 and E0599 as quoted. In L1's comment, say that only the
   facade's relay links `lock_settled` to the `Locker`.
3. **must:** Hide `new_init_state` (rule 1).
   - Wrap each target's `state_machine!`, its `impl` blocks and its facade
     in a private `mod`.
   - `pub use` the typed machine, the state markers, the outcome enums, the
     payload and data types, `Ctx`, the facade and `IMPLEMENTED`.
   - Don't `pub use` `DynamicX`, `XState` or `XEvent`.
   - Remove the unused `pub fn machine()`.
   - Check that the compile-fail cases still build. They name only the
     typed API, apart from the P6, W1 and I4 bodies moving per fixes 1–2.
4. **must:** Add forge cases for pointer, polkit, transfer, lock and
   install, as listed in §2, each naming the other public means (including
   `initialize()` on every leaf). Wrap gets none if fix 3 is done. Record
   this in NOTES §4.
5. **must:** Replace `Out` (`Rc<RefCell<Vec>>`) with a `Vec<Effect>` owned
   by each `Ctx`, and drain it in the facade through the wrapper's
   module-private field. Per target:

   ```rust
   let ctx = match self.machine.inner.as_mut() {
       Some(AnyRouterState::Outside(m)) => &mut m.ctx, /* one arm per leaf */
       None => return, // poisoned
   };
   for e in ctx.out.drain(..) { out.push(e); }
   ```

   - The facade must sit in the macro's module (fix 3).
   - `wrap::reset` must keep the buffer (`mem::take`).
   - `transfer::Frames::next` must take `&mut Vec<Effect>`.
   - `lock` must drain the `Locker`'s buffer after each `locker.handle` and
     the `Delay`'s after each `delay.handle`, in call order.
   - Give the probes a small `pub fn take_effects<S>(m: &mut X<S>) ->
     Vec<Effect>` in place of `Out::take`.

   Working versions for pointer and wrap:
   `sm-exp/sme/impl-state-machines/src/{pointer_v1,wrap_v1}.rs`. Re-run
   smoke and `sizes`, and update the NOTES §3 tables.
6. **should:** Rewrite T1 to the reference reading (§2). It stays
   `reject`.
7. **should:** Correct NOTES items 1–5 of §3:
   - two allocations, not one;
   - in-module `inner` access exists;
   - the typed-API-direct measurement (about 16% on wrap; not adopted);
   - the new tally;
   - the dates "from the survey".
8. **should (optional):** Split `Asking` for T6, and note `final_states`
   as an unused option with its completion-`Vec` cost.

## 7. Overall

The implementation represents the crate fairly enough to rank it once the
must-fixes land.

- All six targets are written in the crate's own idiom: hierarchy with
  superstate data, lifecycle hooks, factories, child-first and branching
  events. No `enum` + `match` hides behind the DSL.
- Each workaround is real, recorded, and traceable to the source:
  - superstate data can't come from a factory;
  - hooks get no payload;
  - guards run before callbacks;
  - regions are async-only.
- The tests are unweakened and pass deeply.

The two distortions run in opposite directions:

- **Too harsh on speed:** the `Rc<RefCell>` buffer, with its extra
  allocation, makes the crate look slower than it is.
- **Inconsistent compile-fail classes under the new rules:**
  - three cases were scored `accept` only by forging, or below what the
    typed API guarantees (P6, W1, I4);
  - P1 and L1 move to `reject` under the new clause;
  - the forge cases are missing.

Even fixed, state-machines stays the slowest and largest slot. It is about
2.4× enum on pointer and 5× on wrap. Most of that is inherent:
consume-and-rebuild typed transitions, every data slot held as an `Option`,
and the wrapper's 40 + 8·scopes bytes and per-event bookkeeping.
