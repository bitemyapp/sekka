# Review: finny slot (finny =0.3.0, finny_derive 0.3.0)

This review applies `IMPLEMENTING.md`, including "Transitions and forging".
Paths: workspace `../eval/`. Crate sources:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/finny-0.3.0/src/` and
`…/finny_derive-0.3.0/src/`. Every line number comes from a single-file
`cat -n` or `grep -n`.

What I ran (component checks only; my own target dirs; no workspace edits):

- `cargo test --locked -p impl-finny`: 12/12 spec tests, `bypass.rs` and the
  compile-fail suite pass. Conformance passes 21/14/16/20/18/16.
- smoke (`--features finny`): every finny row says `same as enum`. In ns per
  event, finny vs enum: pointer-stream 13.2 vs 7.1, wrap-80 9.7 vs 3.9,
  wrap-200 9.5 vs 5.1, replays 13–30 vs 8–17. `bin/sizes` matches NOTES:
  216 / 128 / 224 / 136 / 464 / 392.
- `metrics/sme.py compile-fail` and `loc` match NOTES §4 and §6. The loc
  baselines have moved since NOTES was written; see S7.
- A scratch crate outside the workspace:
  `finny-probe` (not kept).
  - `src/main.rs` checks that a `#[finny_fsm]` compiles and dispatches inside
    a private module, tests the stale `shake` behind L6, prints part sizes,
    and times construction separately from `handle`.
  - `src/bin/fresh_router.rs` forges a fresh `Router` from outside the crate,
    without `fsm_mut()`.

## 1. Idiom fidelity, per target

Every spec transition row is a finny builder statement: transitions,
guards, actions, entry and exit, sub-machines and regions. No hand-written
state enum runs beside the generated one. The hand code is small and falls
into three kinds: event mapping and routing in `handle`, per-level effect
buffers drained in a fixed order, and guards that predict a choice finny
can't make after the action has run.

| target | grade | in the DSL | hand code beside it |
|---|---|---|---|
| pointer | idiomatic | all rows. 34 transitions in `inside_fsm` (`pointer.rs:171-298`) and `router_fsm` (`:300-318`). The superstate exit is `Inside`'s `on_exit` (`:311-313`); its `Leave`/`StartDrag` are `Router` rows on the sub-machine state (`:314-315`) | `handle` chooses the level and wraps a pointer event that arrives `Outside` in `Enter` (`:359-372`). The `Enter` action re-queues it (`:304-307`) |
| wrap | partial (honest) | all rows, in two regions (`wrap.rs:205`, `:208-313`) | `lands_pending` (`:136-148`) repeats `print`'s logic as a guard, because the target is fixed before the action runs. The mode is mirrored into `Screen::autowrap` (`:309-310`). `Repeat` is re-queued one print at a time, and `handle` drains after each queued event (`:364-372`) |
| polkit | partial (honest) | all rows. `Active` is a sub-machine; `start` is `Starting`'s entry and back-off is `RetryWait`/`Unavailable`'s entry (`polkit.rs:168-179`) | the spec's conditional exits are transition actions (exits can't see the event). Ending a request is an internal row plus a queued `End` (`:363-388`); see claim 7. `end_running`/`conversation` read `Active`'s current state (`:141-161`) |
| transfer | partial (inherent) | all 26 transitions, flat (`transfer.rs:251-462`) | the frame stack is a `Vec` in the context (`:159-163`); a sub-machine can't nest itself. `has_more` (`:167-169`) duplicates the choice inside `next`, giving guarded pairs |
| lock | partial (honest) | all rows. Two regions, `Lockscreen ⊃ Locked`, entry and exit on both (`lock.rs:158-329`) | `LockConfirmed` is re-queued into `Lockscreen` (`:261-269`). `Accepted` is a top-level row guarded two levels down (`:253-260`). `handle` splits `PamDone` by outcome (`:375-381`) and drains the inner buffer first (`:390-393`) |
| install | idiomatic | all rows. Stage runs are entry actions; `Executing`'s entry and exit hold and release the inhibitor (`install.rs:198-205`, `:334-337`) | `deliver` bubbles by hand: the event goes to the sub-machine, then to `Installer` on `Err` (`:376-380`). The end-of-sub-machine results are `Installer` rows guarded on `Preparing::at` (`:152-160`, `:254-293`) |

No target is a *facade*, and none is *under-used*. finny's timers are left
unused on purpose, following IMPLEMENTING, and NOTES §3 explains why even a
custom `FsmTimers` couldn't keep `ArmRetry` in place in the effect order.
History doesn't exist in finny. Wrap's two regions are equivalent to the
spec's nesting, since "pending kept" holds across the mode change.

## 2. Compile-fail audit (under "Transitions and forging")

**Forging means.** finny generates these public:

- `FsmFrontend`'s fields `backend`, `queue`, `inspect` and `timers`
  (`fsm/fsm_impl.rs:162-163`).
- `FsmBackendImpl`'s fields `context`, `states` and `current_states`
  (`:9-11`), with `DerefMut` to the context.
- A public `AsMut<S>` for every state, on the states storage.
- A public type per transition (`codegen.rs:615`) whose `execute_transition`
  runs the row from any current state, without its guard
  (`fsm/transitions.rs:116`; see `tests/bypass.rs`).

All of these need `&mut` to a frontend or backend. Outside code can get one
in only two ways:

1. **`fsm_mut()`** on all six wrappers. The implementation chose to expose
   it, so rule 1 says to hide it. Doing so doesn't cost the typed API: a
   delegating `pub fn dispatch(&mut self, e: impl Into<XEvents>) ->
   finny::FsmResult<()>` keeps finny's generated dispatch (and its
   `Err(NoTransition)`) reachable without the fields.
2. **Building a fresh machine with `FsmFactory::new_with`** (or a
   `FsmBackendImpl` literal) needs an owned context. Wrap, polkit, transfer,
   lock and install have contexts with private fields and no public
   constructor, so this is closed for them. The same holds for every
   sub-machine.
   - pointer is the exception. `Router`'s context is the public
     `spec::pointer::Config`. `src/bin/fresh_router.rs` builds a `Router`
     from outside, writes `current_states`, and runs `Pressed`'s release:
     `Ok(())`, `Inside` back in `Hovering`, owner placeholder `Node(0)`.
   - A private-field context (`pub struct RouterCtx { config: Config }`)
     closes this route. Not re-exporting `Router` is not enough, because
     generic code can name `F` from `fsm()`'s `&FsmFrontend<F, …>`.

The transition types can also be hidden with a private module: a
`#[finny_fsm]` inside a private `mod` with selective `pub use` compiles and
dispatches (`finny-probe/src/hidden.rs`). They are harmless once no `&mut`
backend is reachable.

**Result:** after fixes M1 and M2, no forging means stays public. So there
are no forge cases, and NOTES §4 must say so (rule 3). It should also say
plainly that the encapsulation is the wrapper's, not finny's. Any code
holding a finny frontend, which is the normal way to use finny, can set
`current_states`, every state struct and the context, and can run any
transition type unguarded. Phase 3 should weigh that in maintenance risk.
This differs from the coordinator's examples, but it is how rule 1 applies
here. It matches the sml review, where private contexts closed the
crate's setters.

If the coordinator decides instead that finny's frontend must stay
reachable as "the generated dispatch", then the alternative applies. Keep
`fsm_mut()`, and add one `cases/finny/<t>/forge/F-public-fields.rs` per
target built from the forged bodies listed below; polkit's would forge
`Active` into `Prompted` in `Starting`, then `Submit` → `WriteAnswer` (K1).
The reclassifications are the same under either reading.

**Re-check of the 36 cases.** Forging never decides a class (rule 4).
These change:

| case | now | after | why / new `main` |
|---|---|---|---|
| P5 hover-outside | A (writes `Inside`'s `context.hot` via `fsm_mut`) | **T** | No `Outside` row writes `InsideCtx`, and leaving clears it (`pointer.rs:311-313`). New `main`: hover node 3, `Leave`, then `Capture`/`Removed`/`StartDrag`/`DragEnded` while `Outside`. Re-entering over nothing emits no `HoverOff(3)`, and a typed `dispatch(InsideEvents::from(Capture(..)))` while `Outside` is `Err(NoTransition)` |
| P6 owner-mismatch | A (writes `current_states` to `Pressed`) | **T** | Each entry into an owner state through the rows sets its owner: press (`:189-196`), capture (`:199-201`, `:255-257`, `:283-285`), edge press (`:181-188`). New `main`: drive those entries and assert `get_state::<Pressed>().owner` or `edge` and the `Up` target. Don't use `reject` (`Pressed {}`): finny holds a placeholder `Pressed { owner: Node(0) }` from construction, and a row that forgets `to.owner = …` compiles |
| W1 pending-off-the-edge | A (writes `current_states[0]`) | **T** | `Pending` is entered only by a print that lands in the last column, or by `RestoreCursor` of a pending save. New `main`: assert `col == cols-1` every time `state()` is `PendingWrap` (print to the edge, save/restore, `Tab`, `CursorTo`) |
| W5 dangling-link | A (writes `context.links`) | **T** | Links are set only in `soft_wrap`, which line-feeds (`wrap.rs:129-134`). New `main`: after each `SoftWrap{r}`, the next `Put` is on row r+1; `EraseLine(0)` on a row never wrapped emits no `Unlink` |
| T3 lost-aside | A (writes `current_states`, `Probing.item`, `frames`) | **T** | `Restoring` is left only by `Restored` for its item (`transfer.rs:417-437`). New `main`: in `Restoring`, `Probed`/`Placed`/`Disposed`/`Restored` for item 1 change nothing; `Restored{item 0, ok: false}` then emits `Problem(Stranded)` and `Outcome(Failed)` before the next `Probe` |
| I4 deferred-without-space-check | A (writes `current_states` and `Prepared.deferred`) | **T** | The deferred `Evaluated` rows check the room (`install.rs:254-275`). New `main`: the GEN-07 numbers end in `Refused`, never `Prepared` |
| I5 cancel-after-erase | A (writes `current_states` to `Cancelling`) | **T** | `Executing` has no `Cancel` row. New `main`: `Cancel` in `Erase` is ignored, `StepDone` goes to `Format`, and no `Cancelled` appears |
| L6 shake-outside-locked | A (writes `Locked`'s `context.shake`) | **A, rewritten**, or **T** | Without forging, the shake goes stale. Shake once, then `UnlockRequested`: `get_state::<Locked>().context.shake == 1` while `Unlocked`, and still 1 after `Lock` (verified in `finny-probe`; `Locked`'s `on_exit`, `lock.rs:203-207`, never resets it). This is a data-carrying probe in the rule's list, read through the state accessor, so it stays `accept` without forging. If S5 resets the shake on exit, L6 becomes `runtime` |

These stay:

- **P2, P4, I6 stay `accept`.** Each is a read through finny's state
  accessor (`get_state::<S>()`) of a state struct that outlives its state.
  The rule counts state-shape inspection for these probes, and reading is
  not forging.
- **P1, K1, T5 stay `runtime`.** They use the generated `dispatch`, which is
  transition API; repoint them at the delegating `dispatch` (fix M2).
- **All other runtime cases are correct as written.** Each asserts the
  refusal (no effect, unchanged state, or `Err`). Forging would defeat
  them, so they stay `runtime`.
- **P3 stays `runtime`.** It is borderline:
  - A `DragsOnlyDragging` row's action only gets `from: &mut
    DragsOnlyDragging`, so it can't reach `beneath`. That is finny's one
    structural guarantee here.
  - The stale `DragsOnlyPressed.beneath` is still readable while dragging,
    which P4 already counts.
  - Don't move it to `reject`. A `get_state::<DragsOnlyDragging>().beneath`
    E0609 would credit a guarantee the machine doesn't have.
- **No `.stderr` files are needed**, because there are no rejects.
- **No probe is missing.** Sending a sub-machine event at the top level
  (E0277, quoted in NOTES §4) is an addressing error, not a probe's
  guarantee.

After the fixes the tally is 0 R, 32 T, 4 A (P2, P4, I6, L6) and no forge
cases. With S5 it is 0 R, 33 T, 3 A.

## 3. NOTES.md claims (spot-checked against the crate source)

Verified:

1. **"Sub-machines never bubble."** A forwarded event's result is returned
   as is, and `break 'fsm_regions` skips the remaining regions
   (`codegen.rs:883-888`). The parent's rows are on its own event variants,
   so an event goes to exactly one level. The real reason a parent region
   can't also see it is that variant split; `handle` or a re-queue has to
   fan it out (lock's `LockConfirmed`).
2. **"A sub-machine can't signal or exit its parent."**
   `FsmEventQueueSub::enqueue` takes only `Into<FSub::Events>`
   (`fsm/queue.rs:311`, `:344`). Through the parent's `From`, that sends
   the event back to the sub-machine. Actions are generic over `Q`, so they
   can't reach `.parent`.
3. **"A sub-machine sees only its own context, built once."** The
   `with_context` closure runs only in the parent's `new_state`
   (`codegen.rs:129`, `:148`). The parent can write the sub-machine's
   context (`polkit.rs:340-346`), but not the reverse.
4. **"The target is chosen before the action runs."** The target is a fixed
   type per row. `execute_transition` runs exit, action and entry, then sets
   the current state (`fsm/transitions.rs:116`). There is no choice or
   junction pseudostate.
5. **"Exit actions don't see the event."**
   `on_exit(&mut self, &mut EventContext)` (`fsm/transitions.rs:13`). Entry
   is the same (`:11`).
6. **"Transition types are public, and `execute_transition` runs from any
   state without its guard."** True (`codegen.rs:615`;
   `tests/bypass.rs` passes). Two overstatements:
   - It is no stronger than writing `current_states`, which needs the same
     `&mut` backend, so "worse" (§4) overstates it.
   - The types are hidable with a private module (§2).
7. **"A sub-machine can't contain itself, so merge frames are a `Vec`."**
   True: the sub-machine is stored by value in the parent's states struct
   (`codegen.rs:311`). The reference uses a `Vec` too.
8. Also true:
   - The current state is one array per machine
     (`codegen.rs:336`).
   - `FsmTimersNull` refuses timers and needs no threads.
   - `heapless` is always a dependency.
   - The quoted error messages exist (`parse_fsm.rs:263`, `:270`, `:311`;
     `parse_blocks.rs:25`; `utils.rs:22`, `:37`).
   - The sizes add up: buffers are 136/216, 56/128, 144/224, 400/464 and
     264/392 bytes.
   - finny is the most verbose slot (2300 lines).

Wrong or overstated:

- **Polkit's two-step end through the queue** (`polkit.rs:8-12`, §2) is
  presented as forced, but it is a concision choice. Guards can see
  `states.active`'s current state, and `last_conv` survives the stop. So
  three guarded `Active → Idle` rows per ending event can do it in one
  dispatch with no queue: `EndHelper`, then `CancelRetry`, then plain
  reply/close. That is about 12 rows instead of 5.
- **"Every state struct implements `Default` because finny builds them all
  up front"** overstates it. `FsmStateFactory` is blanket-implemented for
  `Default` (`fsm/states.rs:74`), and a state can implement
  `new_state(&context)` itself. The pre-entry placeholder value is what is
  inherent.
- **§4's "Not counted, but worse" and §9's "transitions runnable from any
  state"** must be reworded under the new rule (M3). They are crate
  properties reachable only through `&mut` to a frontend, not separate
  findings.
- **§6's baselines are stale.** loc now gives enum 1195 and statig 1596, not
  1165 and 1630. The 2.0× still holds (1.92×).

## 4. Test integrity

- `tests/spec.rs` and `tests/compile_fail.rs` differ from statig's and
  rust-fsm's only in the crate name. Against impl-enum's, they differ by the
  expected differential and `NOT_EXPRESSED` lines.
- `NOT_EXPRESSED` is empty and nothing is `#[ignore]`d. NOTES reports 5000
  property cases passing; at the default 256 they pass in my run.
- `tests/bypass.rs` is extra evidence, not a weakened test. It depends on
  `fsm_mut()`, so it must move with fix M2.

## 5. Benchmark distortions

There is no per-event allocation, `dyn` dispatch or heap queue. Ignored
events return `Err(NoTransition)`, a `Copy` enum, and `handle` drops it.
Events are cloned into sub-machines (`codegen.rs:883`) and out of the
queue, but they are `Copy`-sized.

Sizes: the effect buffers dominate. Without them the sizes are 80 / 72 /
80 / 136 / 64 / 128 bytes, against enum's 28 / 32 / 28 / 40 / 28 / 24. The
rest is every state struct at once, the current-state arrays, the queue
(`FsmEventQueueArray` of one event: Router 32, JobFsm 20, Installer 40,
Session 12 bytes), and the per-level config copies.

- The buffers are **inherent**. finny's context is owned by the backend for
  the machine's life, so it can't borrow the caller's `out`.
- Their **capacity is a choice**, but a tight one: pointer needs 7, and lock
  needs 33 when entering the lock screen with 32 outputs.
- transfer correctly uses a `Vec`, since popping frames is unbounded. It
  costs one allocation per fresh machine.

Cost: `finny-probe` measured each `Machine::new` and the `handle` loop
separately, over the conformance traces.

| target | `new()` finny | `new()` enum | `handle` per event, finny | `handle` per event, enum | ratio |
|---|---|---|---|---|---|
| pointer | 17 ns | 0.3 ns | 15.7 ns | 6.7 ns | 2.3× |
| wrap | 5.5 ns | 3.5 ns | 6.6 ns | 3.4 ns | 1.9× |
| polkit | 18 ns | 0.7 ns | 13.3 ns | 5.1 ns | 2.6× |
| transfer | 13 ns | 0.9 ns | 11.6 ns | 5.1 ns | 2.3× |
| lock | 22 ns | 0.7 ns | 10.9 ns | 3.2 ns | 3.4× |
| install | 14 ns | 0.3 ns | 10.8 ns | 3.6 ns | 3.0× |

Construction (building every state, the buffers, and `start()`) adds only
about 2–4 ns per replay event. The ~2× is mostly per-event dispatch, and it
is **inherent**:

- a generated `match (state, event)` per region;
- a second full dispatch, with a cloned event and a new `DispatchContext`,
  for every event handled by a sub-machine. That is every pointer event,
  most polkit and lock events, and two dispatches for install's bubbled
  results;
- `dispatch_queue` after every dispatch;
- guards that recompute what the action computes (`lands_pending`,
  `has_more`), because the target is chosen first;
- buffering plus a drain copy per level.

The choices are small:

- transfer and install never enqueue, but run a capacity-1
  `FsmEventQueueArray`. `FsmEventQueueNull` would save 20 and 40 bytes and
  one empty `pop_front` per event (S2).
- polkit's queued `End` costs an extra dispatch on request ends only (claim
  7 above).
- wrap could use one 4-state region instead of two regions plus a mirror.
  It would save a missed match per print, at about 30 more rows. Not worth
  requiring.

Nothing here must change for the benchmarks to be fair.

## 6. Required fixes

1. **must** (forging rule 1): Remove `fsm_mut()` from all six wrappers
   (`pointer.rs:332`, `wrap.rs:329`, `polkit.rs:414`, `transfer.rs:475`,
   `lock.rs:342`, `install.rs:370`). Add to each a delegating
   `pub fn dispatch(&mut self, e: impl Into<RouterEvents | CursorEvents |
   AgentEvents | JobFsmEvents | SessionEvents | InstallerEvents>) ->
   finny::FsmResult<()>`. Repoint P1, K1 and T5 at it. Move
   `tests/bypass.rs` into a `#[cfg(test)]` test inside `src/pointer.rs`,
   where it can reach the private frontend and keep its evidence.
2. **must** (forging rule 1): Close pointer's outside construction. Give
   `Router` a private-field context (`pub struct RouterCtx { config: Config
   }`, `with_context(|c| InsideCtx::new(c.config))`). After this, no forging
   means stays public in any target; check with `fresh_router.rs`, which
   must stop compiling.
   - Optionally, also put each `#[finny_fsm]` in a private module so the
     transition types are unnameable.
   - If you keep `Config` as the context instead, add
     `pointer/forge/F-public-fields.rs`. Build `Router::new_with(…)`, set
     `current_states` to `Inside`/`Pressed`, and dispatch a release
     (commits P1/P6). Name `states`/`context` and the transition types in
     a comment.
3. **must** (forging rule 4): Reclassify the cases that went through only by
   forging, with the new `main`s described in §2:
   - `pointer/accept/P5-hover-outside.rs` → `runtime`
   - `pointer/accept/P6-owner-mismatch.rs` → `runtime`
   - `wrap/accept/W1-pending-off-the-edge.rs` → `runtime`
   - `wrap/accept/W5-dangling-link.rs` → `runtime`
   - `transfer/accept/T3-lost-aside.rs` → `runtime`
   - `install/accept/I4-deferred-without-space-check.rs` → `runtime`
   - `install/accept/I5-cancel-after-erase.rs` → `runtime`
   - `lock/accept/L6-shake-outside-locked.rs`: rewrite without forging. It
     stays `accept` (stale `shake == 1` read while `Unlocked` after
     unlocking mid-shake), or becomes `runtime` if you take S5.
4. **must**: Rewrite NOTES §4 under the new rule:
   - the tally: 0 R, 32 T, 4 A, no forge; or 33 T, 3 A with S5;
   - the rule-3 sentence (no forging means stays public, and why: a private
     frontend, private-field contexts);
   - the crate property stated plainly: anyone holding a finny frontend can
     set `current_states`, any state struct and the context, and run any
     transition type unguarded;
   - drop "Not counted, but worse".
   Update §9's "0 rejects / transitions runnable from any state" sentence to
   match.
5. **should**: Make the implementation's own context fields private, with
   read getters where cases read them. This hardens the design if a
   frontend is ever exposed again; it doesn't change any class. The fields
   are `InsideCtx.hot`/`under`; `Screen.row`/`col`/`saved`/`previous`/
   `links`; `Request`'s `pub` fields; `Job.frames`; `Seat.outputs`;
   `Field.has_text`/`shake`/`last_check`. Cases read `context.col`, `row`,
   `last_conv` and `shake`.
6. **should**: transfer and install never enqueue. Use `FsmEventQueueNull`
   (a second alias beside `Front`), or say in NOTES §3 why a capacity-1
   array was kept.
7. **should**: NOTES §2 polkit: say that the queued `End` is a concision
   choice, and that guarded rows would avoid it.
8. **should**: NOTES §2: say "every state needs a pre-entry value (`Default`
   or `FsmStateFactory`)" rather than "must implement `Default`".
9. **should**: NOTES §3: add the sizes without buffers, and the
   construction-versus-`handle` split from §5, so phase 3 can read the
   numbers.
10. **should** (S5): `Locked`'s `on_exit` leaves `shake > 0`. The spec gives
    the shake only to `Locked`. Reset it on exit (no effect change), or
    record the stale value in NOTES. Either way, L6's class follows.
11. **should** (S7): NOTES §6: refresh the baseline line counts (enum 1195,
    statig 1596).

## 7. Overall

The implementation represents finny fairly. Every spec row is in finny's
builder DSL, the workarounds are the smallest honest ones and are recorded,
and the claims about the crate's limits check out against the macro
source. The limits are: no bubbling, no parent signalling, separate
contexts, region skipping, target before action, exits without the event,
and no self-nesting. The performance gap and the sizes come from finny's
design, not from choices in the slot.

The compile-fail table is what needs work. Seven accepts went through only
by writing finny's public fields via the implementation's own `fsm_mut()`,
and the new rule moves them to `runtime`. Once fixes 1–4 are in, the slot is
fit to rank: 0 R / 32 T / 4 A, no forge cases. NOTES should then state the
crate-level forging property in prose, so phase 3 can weigh it.
