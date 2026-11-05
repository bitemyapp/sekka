# Review: smlang slot (smlang =0.8.0, smlang-macros 0.8.0)

Paths: workspace `../eval/`; crate source
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/smlang-macros-0.8.0/src/{codegen.rs,validation.rs,parser/*.rs}`
and `…/smlang-0.8.0/{README.md,src/lib.rs,tests/test.rs}`. Line numbers
come from single-file `cat -n` reads.

What I ran (component checks only, own target dir, no workspace edits):

- `cargo test --locked -p impl-smlang`: 12/12 spec tests and the
  compile-fail suite (36 cases) pass. Conformance: pointer 21/21, wrap
  14/14, polkit 16/16, transfer 20/20, lock 18/18, install 16/16.
- `SME_PROPTEST_CASES=5000 cargo test --locked --release -p impl-smlang --test spec`: 12/12.
- Smoke (`--features smlang`): every smlang row says `same as enum`.
  ns/event in my run, smlang vs enum: replay-install 13.03 vs 8.44,
  replay-wrap 9.72 vs 7.92, wrap-200 4.66 vs 3.87, replay-pointer 16.35 vs
  18.33. `bin/sizes` gives the numbers in NOTES.
- `metrics/sme.py compile-fail`: the smlang row matches NOTES (6 R, 25 T,
  5 A). `metrics/sme.py loc`: 1883 total, as NOTES says. clippy: no
  warnings.
- A scratch crate outside the workspace,
  `smlang-probe/src/main.rs` (not kept),
  to test the crate claims (output quoted in section 3).

## 1. Idiom fidelity

What the DSL holds: flat leaf states with optional data, events with
optional data, guard expressions over generated guard methods, one action
per row that returns the target's data, `A | B` input patterns, `_`
wildcards, internal transitions, and argument-less `on_entry_*`/`on_exit_*`
hooks. No hierarchy, regions, history or deferral. Guards and actions are
methods of the generated `StateMachineContext` trait, so "hand code" in
guards and actions is the crate's idiom. I count as hand code beside the
DSL only what bypasses `process_event` or keeps state outside the generated
`States`.

No target has a shadow state enum. Every `handle` is an event mapping plus
one `process_event` call, apart from the three exceptions in the table.
Every `state()` is a plain match on the generated `States`.

| target | grade | in the DSL | hand code beside it |
|---|---|---|---|
| pointer | **under-used** | `pointer.rs:31-70`: all six leaves, every spec line (30 rows, 44 after `|`). Leaf data for `Pressed`/`DragsOnlyDragging` (`Node`) and `DragsOnlyPressed` (`EdgePress`). `Inside`'s rows are repeated per substate data type (`:50-66`) | `Inside`'s hover is hoisted into the context `Router` (`:80-83`), not state data. The crate can carry it in the substates' data (section 2, P5). Leaving it in the context turns P5 into `accept` and drops invariant 2 by construction |
| wrap | idiomatic | `wrap.rs:14-53`: the four (pending × autowrap) states, every spec line, and the wildcards where they fit (`_ + Tab` `:33`, guarded `_ + ReverseIndex [at_top]` `:36`, `_ + FullReset` `:51`). The cursor is the context, which is the spec's "machine data (all states)" | `Repeat(n)` loops `n` `Print` events in `handle` (`:259-266`), and the `previous` check sits in `handle`. Honest: the target depends on `n`. The `fills_*` guards predict the action (`:112-121` beside `:126-140`). Recorded |
| polkit | idiomatic | `polkit.rs:34-76`: all six leaves and every spec line (33 rows). `Active`'s request rides in `Run`/`Request` state data. The stale-conversation and cookie checks are guards | none. Entry/exit effects are inside the actions because hooks can't see data or the sink. Recorded, and correct (codegen.rs:273, :279) |
| transfer | partial | `transfer.rs:54-87`: one state per awaited response, with its data (27 rows). Item checks are guards | the merge-frame stack and **next** are context code (`:98-158`), the same as impl-enum. Because a row's target is fixed, "next → Probing or Finished" is two rows with a predicting `more_*` guard (`:113-115`). Recorded |
| lock | partial | two `statemachine!`s: `Delay` (`lock.rs:29-46`) and `Lock` (`:91-127`). Every spec line of both regions is a row | the regions are coupled by actions calling the delay machine (`:177`, `:181-183`, `:281`), and `handle` routes the delay-only events (`:387-389`). `Lockscreen ⊃ Locked` is repeated per data type. Entry/exit effects are in the actions. Recorded |
| install | idiomatic (minor under-use) | `install.rs:34-82`: all 20 leaves and every spec line. `Run(stage)`, `StayAwake`, `Complete` and `Cancelled` come from `on_entry_*` hooks (`:234-282`), as the spec words them | a context `Vec<Effect>` drained in `handle` (`:317-320`), because hooks get no temporary context. Minor under-use: `Interrupt` could be `on_entry_cancelling` (fix 3) |

Share of the spec's transition lines held as rows: about 100% for
pointer, polkit, install and both lock regions, and about 95% for wrap
(only `Repeat`'s loop is outside). In transfer every line is a row, but the
frame push/pop logic of **next** is context code.

**Could wildcards or other smlang features have collapsed the superstate
duplication?** Mostly not. I checked against the parser and a scratch
crate:

- `_ + Ev` expands to every state that has no row for `Ev` *yet*
  (parser/mod.rs:186-192), so states outside the superstate are included
  unless an explicit row for them comes first. Each expansion is
  signature-checked like a hand-written row (validation.rs:21-44, :57-101,
  :108-145). So a wildcard with a guard or an action fails as soon as the
  covered states' data types differ. I reproduced
  `Action \`exit_inside\` can only be reused when all input states, events, and output states have the same data`
  for `_ + Leave / exit_inside = Outside` over `Hovering`/`Pressed(u16)`/`DragsOnlyPressed(Ep)`.
- **Action-less** rows *can* span mixed data types:
  `NativeDrag + Leave,` followed by `_ + Leave = Outside` compiles and does
  the right thing (the scratch crate printed `Ok(Outside)` from
  `DragsOnlyPressed` and `NativeDrag` stayed `NativeDrag`). But this only
  helps when the effects can move into entry hooks, and hooks take
  `&mut self` only (codegen.rs:273, :279). pointer would need install's
  buffer on its hot path to do it (`on_entry_outside` emitting exit-Inside
  from a context hover). polkit's end-request and lock's leave-Lockscreen
  effects need the source state's data (`conv`, `cookie`, `shake`) and the
  triggering event (`ReleaseLock`), which hooks can't see. So the
  implementer's choice is right for pointer, polkit and lock. install is
  the exception (fix 3).
- `transition_callback(&self, old, new)` (codegen.rs:573) sees both
  states with their data. But it takes `&self` and no sink, and it runs
  only on non-self transitions. Effects from it would need interior
  mutability plus a buffer: a hack, not a feature.
- Sharing a data type across substates (`Locked`/`Checking`, or polkit's
  five `Active` substates) would let one action name cover more rows.
  But actions never learn the source state, so polkit would need a
  "what's running" tag in the data (a shadow state). In lock it would
  weaken `Check`'s typing. Not required.

## 2. Compile-fail audit

All six `reject` `.stderr` files match (the suite passes), and each first
error is the guarantee itself:

- P2 E0532 (unit variant `NativeDrag`)
- P3 E0609 (`beneath` on `&Node`)
- P4 E0609 (`at` on `&Node`)
- P6 E0063 (missing `edge`)
- L6 E0618 (`LockStates::Locking` is a unit variant)
- I6 E0026 (`Route` has no `closure`)

Every `runtime` case asserts the refusal it names: an `Err` value
(`InvalidEvent`/`TransitionsFailed`), an unchanged state, and the absence
of the forbidden effect.

Disagreement:

- **P5 hover-outside: `accept` is an implementation artefact, not a crate
  limit (must, fix 1).** The accept rests on the hover being context data
  (`pointer.rs:80-83`). smlang's state data can carry it:
  - `Hovering(Hover)`.
  - `Pressed(Owned)` and `DragsOnlyDragging(Owned)`, with
    `Owned { hover, owner }` (still one type shared by both, so the
    `A | B` rows survive).
  - `DragsOnlyPressed(EdgePress { hover, … })`.

  That is still three data types for `Inside`'s substates, the same count
  as now. The real cost: `Outside` stops sharing `Hovering`'s five
  Move/Press/Release rows (about +5 rows, +5 actions, +2 guards). Then
  `Outside` and `NativeDrag` are unit variants and P5 is `reject`, as in
  impl-enum (`State::Outside { hover }`, E0559) and statig.

Accepted with a caveat, for the coordinator rather than this slot:

- W1, T3, I4 and I5 are `accept` through the generated, safe, public
  `new_with_state(any context, any state)` (codegen.rs:628) and
  `context_mut` (:649). That is a real crate property, and the
  classification matches impl-enum's `pub state` probes. But the same
  trick works for probes classified `runtime`, here and in impl-enum:
  - I1: `new_with_state(ctx, States::Erase(route))`, then `StepDone`,
    gives `Run(Format)` with no confirmation.
  - I2: `States::Copy`, then `StepDone`, gives `Run(Bootloader)`.
  - K1: `States::Prompted(run)`, then `Submit`, gives `WriteAnswer` with
    no prompt shown.
  - K5: `States::RetryWait(req)`, then `RetryTimer`.
  - L7: swapping in `Screen::new` gives a second `TakeDelay`.

  smlang follows the reference's line, so I don't ask the implementer to
  change it. Phase 3 should state one rule for when "write the state"
  counts, and apply it to every slot.
- The probes never call action methods directly (for example
  `agent.write_answer(&mut out, &run)`). The generated trait is `pub` and
  so are its methods. Leaving that path out follows rule 1 ("never prove
  a probe by pushing an `Effect` yourself"), and NOTES section 4 discloses
  it.

No missing probes: all 36 are attempted.

## 3. NOTES.md claims

I checked these against the source and found them correct:

1. **Reuse is keyed by signature.** Guards key on (source-state data,
   event data). Actions also key on target data (validation.rs:21-44; the
   errors are at :97 and :139). `A | B` is one transition per state
   (state_machine.rs:27), so it obeys the same rule. This is the claim
   that superstate rows multiply, and it holds.
2. **Entry/exit hooks take only `&mut self`** (codegen.rs:273, :279).
   They are skipped when the source is the target (:427) and never run for
   the initial state (`new`/`new_with_state` just store it, :628).
3. **Exit runs before the action, and a failed action leaves the hook
   run.** `self.context.on_exit_X()` (:436) comes before the action's
   `.map_err(ActionFailed)?` (:704). The scratch crate logged
   `["exit pressed", "action", "exit pressed", "action"]` for two failing
   tries, with the state still `Pressed(3)`. So a retry re-runs the exit
   hook. A failed action also stops the row search: later guarded rows
   are not tried. No implementation is affected: the only hooks are in
   install, where no action returns `Err`.
4. **`PartialEq` compares the discriminant only** (codegen.rs:585, :598).
   The scratch crate printed `Pressed(1)==Pressed(99): true`. No
   implementation or probe compares data states with `==`: they all use
   `matches!`.
5. **`new_with_state` and `context_mut` are public and accept any state
   with any context** (codegen.rs:628, :649). This is the basis of W1, T3,
   I4, I5 and P5.
6. **syn 1.** smlang-macros 0.8.0 declares `syn = "1"`. The lock resolves
   it to syn 1.0.109, and smlang-macros is its only user. 0.8.0
   (2024-08-07) is the newest version in the registry index cache.
7. **The README is stale**: `log_guard(&Result)` and `log_state_change`
   (README.md:407, :409), and `use smlang::{async_trait, …}` (:266), while
   `src/lib.rs:84` re-exports only `statemachine`. Hook names keep the
   state's case (`on_exit_A`), as rustc confirmed in the scratch crate.
8. The counts check out: rows, guards and actions for pointer and polkit;
   the probe totals (6/25/5); LoC 1883; the sizes; and "every digest same
   as enum".

Wrong or overstated:

- **pointer.** "Duplicating [the hover] into each substate's data would
  give four data types and force one action per substate for every
  `Inside`-wide row." That is wrong: it stays three types, because
  `Pressed`/`DragsOnlyDragging` can share `Owned`. The cost is `Outside`'s
  five rows (see P5).
- **transfer.** "Eight `more_*` guards whose bodies are all
  `Ok(self.more())`." There are seven (`transfer.rs:196, 214, 223, 232,
  244, 253, 259`), and `more_listed` is `e.2 > 0 || self.more()`.
- **Wildcard order.** "A wildcard row placed before a specific row for the
  same event fails with 'State and event combination specified multiple
  times'." That holds only when both rows are unguarded. A guarded
  specific row after the wildcard gives "guarded transition is
  unreachable" (validation.rs:165). A *guarded* wildcard first is legal,
  and wrap.rs:36 relies on it.
- **Build.** "A second syn in a syn 2 workspace." The lock already holds
  syn 2.0.119 and syn 3.0.9, so smlang adds a third major version.
- **install.** "`Preparing` became 9 rows … with 9 thin actions." Part of
  that is a choice, not a crate cost: `Cancelling` is entered only by the
  three `Cancel` rows (`install.rs:42-44`). With `on_entry_cancelling`
  pushing `Interrupt`, they become one action-less row,
  `Checks | Configuration(Root) | Evaluation(Root) | Build(Root) | Cache(Closure) + Cancel = Cancelling`
  (mixed-data action-less rows compile; see section 1).

## 4. Test integrity

- `tests/spec.rs` and `tests/compile_fail.rs` are byte-identical to the
  statig, rust-fsm, fsmentry, statum, finny, sml and state-machines slots
  once the crate name is normalised. `git diff ae3612e` on `impl-smlang/tests` and
  `Cargo.toml` is empty.
- `NOT_EXPRESSED` is empty and there are no `#[ignore]`s. Both are
  justified: all 105 traces and all six differentials pass.

## 5. Benchmark distortions

- No per-event allocation and no `dyn`.
  `temporary_context: &mut impl Sink<Effect>` makes every guard, action
  and `process_event` generic, so dispatch is static. Ignored events
  return fieldless `Error` values, which cost almost nothing.
- **install's buffer** (`install.rs:85-93`, drained at `:317-320`): about
  +55% per event (13.03 vs 8.44 ns in my smoke). The crate forces this
  only because hooks get no temporary context. Putting `Run(stage)` in the
  actions would remove the buffer but drop the hooks. The implementation
  follows the contract (allocated once, capacity 4, at most 3 effects per
  event). Phase 3 should still be told this is the price of smlang's
  hooks, not of smlang dispatch (fix 4).
- **Crate-inherent, fine as they are:**
  - wrap's `fills` guard repeats `print`'s arithmetic, because targets are
    fixed per row. This is most of the ~17% gap.
  - `Repeat(n)` is `n` dispatches.
  - Guards can't hand values to actions: pointer recomputes the press
    target, and transfer evaluates `answered` up to six times and
    `more()` (O(frame depth)) per item done.
- **Sizes.** wrap is 48 B against enum's 32 B because the generated
  `StateMachine { state, context }` keeps the tag outside a 16-aligned
  context (the `u128` links, which enum also uses). That is mostly crate
  layout. `[u64; 2]` links would give 40 B. A note, not a fix.
- transfer's `Vec<Frame>` matches impl-enum and impl-statig.

## 6. Required fixes

1. **must:** Move pointer's hover into the `Inside` substates' state data,
   so P5 is rejected:
   - Use `Hovering(Hover)`, `Pressed(Owned)` and
     `DragsOnlyDragging(Owned)` with `pub struct Owned { pub hover: Hover, pub owner: Node }`,
     and add `hover: Hover` to `EdgePress`. Drop `Router.hover`.
   - Give `Outside` its own `Move`/`Press`×3/`Release` rows. They enter
     with `Hover::default()` and need their own action and guard names.
   - Make the actions return the updated hover inside the target's data,
     and have the exit actions read it from `&data`.
   - Replace `accept/P5-hover-outside.rs` with
     `reject/P5-hover-outside.rs`, for example
     `let _ = States::Outside(Hover::default());` (E0618), plus its
     `.stderr` written with `TRYBUILD=overwrite`. Read it.
   - Update P6's literal so `edge` stays the only missing field.
   - Re-run conformance, the differential, the 5000-case run, smoke and
     sizes.
   - Update NOTES: the pointer section, "Lost: P5", the section 4 counts
     (7 R, 25 T, 4 A) and the LoC.

   If you judge the cost too high, NOTES must say that P5 is `accept` by
   implementation choice, and that smlang could reject it at about +5
   rows.
2. **should:** Correct the four NOTES statements listed in section 3:
   pointer's "four data types", transfer's "eight `more_*` … all
   `Ok(self.more())`", the wildcard-order error, and "second syn".
3. **should:** Give install an `on_entry_cancelling` that pushes
   `Interrupt`, and collapse `install.rs:42-44` into one action-less row.
   That removes `interrupt`, `interrupt_root` and `interrupt_cache`.
   Update the install counts in NOTES. If you keep the three rows instead,
   say in NOTES that this is a choice.
4. **should:** In NOTES section 3, state that install's ~50% replay
   overhead is the buffer that smlang's argument-less hooks force, and
   that an action-only variant without hooks would avoid it. Optionally
   smoke that variant once in a scratch copy and quote the number.
5. **should:** Add to NOTES section 6 (or 1) why wildcards don't collapse
   the superstate rows:
   - An action or guard on a wildcard is signature-checked per expanded
     state.
   - Action-less mixed-data wildcards are legal, but they need the effects
     in hooks.
   - `transition_callback` is `&self`.

   This lets phase 3 see that the duplication is a crate cost.

## 7. Overall

This implementation represents smlang fairly and can be ranked once fix 1
is done. The tables really are smlang tables. Guards, actions, data
states, wildcards, `A | B` patterns and (in install) entry hooks do the
work. Nothing is a facade, the tests are untouched, and every digest
matches the reference. The NOTES' crate findings are accurate: the
signature rule that multiplies superstate rows, argument-less hooks,
exit-before-a-failed-action, discriminant-only `PartialEq`, public
`new_with_state`/`context_mut`, and syn 1. The concision and performance
costs it reports are almost all inherent to the crate. The one result
that is unfair to the crate is pointer P5. It is `accept` only because the
hover was hoisted into the context to save rows; with the hover in state
data, smlang rejects P5 as enum and statig do. Install's buffer overhead
is a real but choice-dependent cost and should be labelled as such. The
cross-slot rule for `new_with_state`-style "write the state" probes needs
settling by the coordinator, but smlang's classification matches the
reference's.
