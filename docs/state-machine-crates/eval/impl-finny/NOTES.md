# impl-finny: notes

## 1. Crate

finny 0.3.0 (finny_derive 0.3.0), default features (`std`, `inspect_tracing`,
`timers_std`), no reserve substitution. All six targets are implemented and
pass: conformance 21/21, 14/14, 16/16, 20/20, 18/18, 16/16 traces; every
differential test passes at 256 and at 5000 cases; `NOT_EXPRESSED` is empty
and nothing is `#[ignore]`d.

Where the survey (`../../survey.md`, `### finny 0.3.0`) was wrong
or incomplete, checked against `~/.cargo/registry/src/*/finny{,_derive}-0.3.0/`:

- "a `CurrentState` enum per region": there is one `<Fsm>CurrentState` enum
  of every state, and the current state is a fixed array
  `[FsmCurrentState<Kind>; regions]` (`codegen.rs:327-337`).
- "Timers use std threads or tokio": `TimersStd`/`TimersStdNoAlloc` are
  polled `Instant` deadlines (`timers/std.rs`), no threads; `FsmTimers` is a
  pluggable trait, and `FsmTimersNull` (used here) refuses every timer.
- "Dependencies ... optionally tokio, tracing and serde": `tracing` is on by
  default (`inspect_tracing`), and `heapless 0.7` is always built.
- "Hierarchy: yes, as sub-machines" is true but understates the limits
  (section 2): a sub-machine has its own context, built once from the
  parent's; it receives only events addressed to it, never bubbles an
  unhandled one up, can't queue a parent event (`FsmEventQueueSub` takes only
  the sub-machine's own events, `queue.rs:311-350`), and can't leave itself.
  An event forwarded into a sub-machine skips the other regions
  (`break 'fsm_regions`, `codegen.rs:883-889`).
- "Closures are lifted into generated code": exactly, textually. Each closure
  must be a literal whose parameters are plain identifiers or `_` (no type
  annotations, no patterns), and the builder function may contain only builder
  method calls, one `on_event` per statement.
- "Concision: about 35–50 lines": that's the README example; these targets
  took 330–462 lines each, the most of any slot (section 6).
- Not in the survey: the frontend's fields (`backend`, `queue`, `inspect`,
  `timers`, `fsm_impl.rs:162-163`) and the backend's (`context`, `states`,
  `current_states`, `:9-11`) are public, and the generated transition types
  are public with a callable `execute_transition` that runs from any state
  without its guard (section 4).

## 2. Per target

Common shape: states and events are plain structs (one event struct per spec
event variant, mapped in `handle`). finny builds every state up front, so each
needs a value before it is ever entered: `Default` (finny's blanket
`FsmStateFactory`, `fsm/states.rs:74`) or a hand-written
`FsmStateFactory::new_state(&context)`. I used `Default`, hand-written with a
placeholder (`Node(0)`, `ItemId(0)`, `AsideId(0)`) where the field's type has
none. State data passes between states through transition actions, which get
`from: &mut From, to: &mut To`; guards get `(event, &ctx, &all_states)`.
Each wrapper keeps the finny frontend private and offers a typed `dispatch`
(finny's, then the effect drain) beside `Machine::handle`, which maps the
spec's event and calls it.

**pointer** (`src/pointer.rs`). Top machine `Router` (`Outside`, `NativeDrag`,
sub-machine `Inside`) at :310; `Inside` (`Hovering`, `Pressed`,
`DragsOnlyPressed`, `DragsOnlyDragging`) at :181, whose context holds the
hover pair, the threshold and the effect buffer. `Router`'s context is
`RouterCtx` (:116), a private-field wrapper of `Config`, only so no `Router`
can be built outside the module. The superstate's exit is
`Router`'s `on_exit` on the `Inside` state (:320), the superstate's
`Leave`/`StartDrag` are `Router` transitions on it. Easy: the hover as the
sub-machine's context, guards on the edge (`states.pressed.owner == ev.0`).
Workarounds: a pointer event arriving `Outside` is wrapped in `Enter` and
re-queued into `Inside` by `Router`'s transition action (:315), so it is
handled "as `Hovering`" after `Inside` starts (finny has no "enter, then
handle here"). `handle` addresses events to `Inside` or `Router` itself
(:372), because a sub-machine only receives events wrapped in its parent's
`Inside(..)` variant. 34 transitions in 35 builder statements.

**wrap** (`src/wrap.rs`). Two orthogonal regions (`initial_states::<(Normal,
Autowrap)>`, :213): the pending wrap (`Normal`, `Pending`) and the mode
(`Autowrap`, `NoAutowrap`), so `SetAutowrap` keeps the pending flag with no
history (the spec's diagram nests the pending states in the mode states,
which finny could only express with history it doesn't have). The cursor,
saved cursor, REP memory and links are the context. Workarounds: (1) finny
chooses the transition, so the next state, before the action runs, so each
`Print` is a guarded pair whose guard predicts where the print lands
(`lands_pending`, :147) and the action then prints: the print logic exists
twice. (2) A guard can't see another region's current state, so the mode
region's entry actions mirror it into `Screen::autowrap` (:317-318).
(3) `Repeat(n)` queues one `Print` and `Repeat(n-1)` (:241); `dispatch` runs
the queue by hand, draining after each event (:339), so the fixed effect
buffer stays one print long.

**polkit** (`src/polkit.rs`). Top `Agent` (`Idle`, sub-machine `Active`) at
:339; `Active` (`Starting`, `Prompted`, `Checking`, `RetryWait`,
`Unavailable`) at :173, with the request and the machine-wide `last_conv` in
its context (a sub-machine's context outlives its runs, so it can hold
machine-wide data; `Agent` reaches it only through the `Active` state struct).
Easy: start(identity) is `Starting`'s entry action (:177), the back-off note is
`RetryWait`'s/`Unavailable`'s entry (:183). Workarounds: finny's exit actions don't know the
transition, and the spec's `EndHelper`/`CancelRetry` on exit depend on it
(not on `Prompt`, `Submit`, `Unreachable`, `RetryTimer`), so they are in the
transition actions. Ending the request from `Agent` takes two steps through
the queue (:374-391): an internal transition on `Active` that ends what runs
there (`end_running`, :163, reads `Active`'s current state, still running),
then the queued `End` leaves `Active`. That is a concision choice, not forced:
guards can read `Active`'s current state too and `last_conv` survives the
stop, so three guarded `Active -> Idle` rows per ending event (conversation:
`EndHelper`; `RetryWait`: `CancelRetry`; neither) would end it in one
dispatch, about 12 rows instead of 5. `Begin` with another cookie also
re-queues the `Begin` to be handled by `Idle`. `Success` leaves `Active` from
`Agent` with a guard that looks into `Active`'s current state
(`conversation`, :152).
Conversation states carry no `conv`: the running conversation is always
`last_conv` (invariant 1), and per-state data wouldn't be scoped anyway.

**transfer** (`src/transfer.rs`). One flat machine `JobFsm` (:252), one state
struct per awaited response holding its item (and aside, `then`). Not
expressed: merge frames as nested sub-machines. A finny sub-machine is a
fixed type stored in its parent's states, so a merge inside a merge would need
a machine containing itself; the frame stack is a `Vec` in the context, as in
the reference. "next" is a choice (another item, or the job finishes) and
finny's target is fixed before the action, so each "item done" transition is
a guarded pair (`has_more`, :167): 26 transitions, 25 guards, each guard
repeating the item check. The effect buffer is a `Vec` (:162): popping frames
emits two effects per frame, and the depth is unbounded. It never queues an
event, so it runs on finny's null queue (`Unqueued`).

**lock** (`src/lock.rs`). Top `Session` (:225) with two regions: the lock
(`Unlocked`, sub-machine `Lockscreen`) and the delay (`NotHeld`, `Held`,
`SleepPending`). `Lockscreen` (:192) has `Locking` and sub-machine `Locked`
(:166: `Idle`, `Checking`). Easy: the superstates' entry/exit actions
(`Lockscreen` entry opens the surfaces, `Locked` entry/exit for
`LockedHint(true)`/`CancelShake`), `ShakeTick` for all of `Locked` as
`Lockscreen`'s internal transition on `Locked` (:216), and the delay
region's `PrepareForSleep` guard reading `Lockscreen`'s current state from the
states storage (it is stopped outside the lock screen). Workarounds:
`has_text` (spec: `Lockscreen`'s) and `last_check` live in `Locked`'s
context, the only level that reads them (a sub-machine can't reach its
parent's context). `LockConfirmed` must reach both regions, but a forwarded
event skips the other regions, so it's a top-level event that the lock region
re-queues into `Lockscreen` (:275). The PAM acceptance leaves `Lockscreen`, so
it's a `Session` transition whose guard looks two levels down (:261);
`handle` splits `PamDone` by outcome into `Accepted` (top) and `Refused`
(`Locked`). A stale value I kept: `Locked`'s exit (:210) cancels a running
shake but doesn't zero `shake`, which only `Locked`'s entry resets; finny
keeps the context, so after unlocking mid-shake `shake` still reads 1 while
`Unlocked` (L6). Zeroing it on exit would change no effect.

**install** (`src/install.rs`). Top `Installer` (:228) with sub-machines
`Preparing` (:163) and `Executing` (:195). Easy and pleasant: each stage's
entry action runs it (`Run`, `Progress(n)`), `Executing`'s entry/exit hold and
release the inhibitor (:336), and finny's ordering (parent entry, then the
sub-machine's initial state; inner exits first) gives `StayAwake,
Progress(1), Run(Erase)` and `AllowSleep, Complete` with no extra code.
Workarounds: the results that end a sub-machine (`Evaluated` deferred,
`Built` without room, `Cached`, the last `StepDone`) are `Installer`
transitions guarded on the sub-machine's state and data (`Preparing::at`,
:154). finny doesn't pass an event a sub-machine ignores to its parent, so
`handle` does (`deliver`, :382): the stage result goes to the sub-machine,
then on `Err(NoTransition)` to `Installer` (which also needs it for
`Cancelling`). Every target state needs its own `fsm.state::<T>()` statement
(:351-354). Like transfer, it never queues and runs on `Unqueued`.

## 3. Effects and dispatch

finny's context is one concrete type per machine, owned by the backend, and a
sub-machine's context is a separate value built once by `with_context` from
its parent's. Neither can borrow the caller's `out` for one `handle`, so each
context holds an effect buffer (`Effects<E, N>`, a fixed array, `lib.rs:49`;
transfer: a `Vec` that keeps its capacity), and each wrapper's `dispatch`
drains it into `out` after finny's `dispatch`. No allocation per event
(transfer's `Vec`s grow only past their previous maximum, like the
reference's frame stack; a fresh transfer machine allocates once). With
several levels each level has its own buffer, and the drain order is per
target: parent first in polkit and install (entering the sub-machine's state
comes before its initial state's entry), innermost first in lock (inner exits
run before the outer transition's actions). It is correct for every trace and
the differential runs, but it is a maintenance hazard: an effect written to
the wrong level's buffer reorders silently.

Pointer, wrap, polkit and lock run on `FsmEventQueueArray` (`Front`,
`lib.rs:39`, capacity 1 or 2); transfer and install never queue and run on
`FsmEventQueueNull` (`Unqueued`, `lib.rs:45`). finny's null queue answers
`enqueue` with `Ok(())` and drops the event (`queue.rs:305-308`), so a
re-queueing action added to those two later would vanish silently. All use
`InspectNull` and `FsmTimersNull`. Costs finny adds per event: the generated
`match (current_state, event)` per region (a miss is counted; `NoTransition`
only if every region misses); for a sub-machine, a second dispatch with the
event cloned (`ev.clone()`) and a new dispatch context; the queue drain after
every `dispatch` (one empty `pop_front`); queued events are cloned again when
dequeued; guards that recompute what the action then does (`lands_pending`,
`has_more`); a buffer push and a drain copy per level; ignored events return
`Err(NoTransition)`, which `handle` drops. Re-queued steps (pointer's entry,
wrap's `Repeat`, polkit's end of request, lock's confirmation) and install's
bubbled results are full extra dispatches.

Sizes (`bench --bin sizes`): pointer 216, wrap 128, polkit 224, transfer 112,
lock 464, install 352 bytes. Most of it is the effect buffers
(`N * size_of::<Effect>() + 8`: pointer 136, wrap 56, polkit 2 × 72, transfer
24 (the `Vec`), lock 328 + 72, install 104 + 56 + 104). Without them: 80, 72,
80, 88, 64 and 88 bytes, against the enum's 28 / 32 / 28 / 40 / 28 / 24 (the
review's figures); the rest is every state struct at once, the current-state
arrays, the queue slot (Router 32, Session 12 bytes) and per-level config
copies. lock's top buffer holds 40 because entering the lock screen opens up
to 32 surfaces; pointer's needs 7.

Construction is not where the time goes. The review timed `Machine::new` and
the `handle` loop apart over the conformance traces (`finny-probe`, before
transfer and install moved to the null queue): `new()` 5.5–22 ns (enum
0.3–3.5 ns), `handle` per event finny vs enum: pointer 15.7 vs 6.7, wrap 6.6 vs
3.4, polkit 13.3 vs 5.1, transfer 11.6 vs 5.1, lock 10.9 vs 3.2, install 10.8
vs 3.6 ns (1.9–3.4×). Construction adds 2–4 ns per replayed event; the rest
is the per-event dispatch above.

Timers: not run. finny could have modelled `ArmRetry`/`CancelRetry` and the
shake as state timers (`on_entry_start_timer`, cancelled on exit), with a
custom `FsmTimers` that turns `create`/`cancel` into requests; but that
service has no access to the context's effect buffer, so `ArmRetry` would
lose its place among the other effects.

## 4. Compile-time guarantees

Tally under "Transitions and forging": 0 reject, 32 runtime, 4 accept, no
forge cases (36 probes).

- Runtime (P1, P3, P5, P6, W1–W5, K1–K6, T1–T6, L1–L5, L7, I1–I5): the
  operation needs a transition from the right state, and finny's dispatch
  matches `(state, event)` and returns `Err(NoTransition)` otherwise (P1, P5,
  K1 and T5 assert the `Err` from the typed `dispatch`). The rows that enter
  a state fill its data (P6: each entry into an owner state sets its owner;
  W1: `Pending` only by a print landing in the last column or a pending
  restore; I4: the deferred route's `Evaluated` rows check the room). Nothing
  in the types stops an action from emitting the forbidden effect or a row
  from leaving its target's data unset: those compile.
- Accept (P2, P4, I6, L6): every state's struct and every sub-machine's
  context exists for the machine's life, readable through the state accessor
  `get_state::<S>()` in any state. P2: `Pressed.owner` names the drag source
  after the program started a drag; P4: `DragsOnlyPressed`'s press point and
  `beneath` in `Hovering`; I6: `Cache.closure_mib` (0, never built) on the
  deferred route; L6: `Locked`'s `shake` still 1 (a running shake) while
  `Unlocked` and locking again (section 2, lock).
- One structural guarantee finny does give: a transition action sees only its
  `from` and `to` structs and the context (P3: `DragsOnlyDragging`'s release
  can't read `DragsOnlyPressed.beneath`), though a guard sees every state.

No forging means stays public, so there are no forge cases. The
encapsulation is the wrappers', not finny's: each keeps its finny frontend in
a private field (only `&` reaches outside, through `fsm()`), offers a typed
`dispatch` instead, and every context type (including pointer's `RouterCtx`)
has private fields and no public constructor, so outside code can't build a
machine with `FsmFactory::new_with` either. I checked both routes with
throwaway cases: building a `Router` from a `Config` (E0308), building a
`RouterCtx` (E0451), writing `current_states` through `fsm()` (E0596).

finny itself gives anyone who holds a machine `&mut` (the frontend, which is
how finny is normally used: `dispatch` takes `&mut self`) public fields to set
`current_states`, any state struct and the context, and public generated
transition types whose `execute_transition` runs any row from any current
state without its guard (`fsm/transitions.rs:116`; the `#[cfg(test)]` module
in `src/pointer.rs` runs `Pressed`'s release from `Hovering` and gets an `Up`
for a press nobody owned). A maintainer using finny directly would have all
of that, and every runtime refusal above is one field write away; phase 3
should weigh it under maintenance risk. The transition types could also be
made unnameable with a private module around each `#[finny_fsm]`; I didn't,
since no `&mut` backend is reachable.

The typed `dispatch` also accepts every level's events, including the
re-queue helpers (`Enter`, polkit's `End`, `LockscreenEvents::LockConfirmed`
addressed to `Lockscreen` directly), which skip what the parent or `handle`
would do (by the generated dispatch, `codegen.rs:883-889`: an
`Enter(Capture)` captures from `Outside`; the direct confirmation leaves the
delay region in `SleepPending`). That is transition API, and none of it
commits a listed probe.

The compile errors finny does produce are good. Sending a sub-machine's event
at the top level:

```text
error[E0277]: the trait bound `RouterEvents: From<impl_finny::pointer::Motion>` is not satisfied
  |     let _ = m.dispatch(Motion { at: Point::new(0, 0), hit: Hit::NOTHING }, &mut out);
  = help: `RouterEvents` implements trait `From<T>`:
            From<InsideEvents>
            From<impl_finny::pointer::DragEnded>
            ...
```

It points at the event and lists the ones the level accepts. Graph checks
point at the builder call: "This transition can never be taken: an earlier
transition of this state for the same event has no guard. ...", "A transition
into the same state. Use `self_transition()` ...", "Unreachable state!".

## 5. Errors while writing it

Few, short, and mostly well placed. Closure bodies keep their spans, so type
and borrow errors point into the builder code (`expected u32, found &str`
under the closure's expression). The ones that cost time:

- `error: State not found.` under `transition_to::<Cancelled>()`: every target
  state must also have its own `fsm.state::<Cancelled>();` statement. A
  minute; the message doesn't say what to add.
- `E0502: cannot borrow *ctx as immutable because it is also borrowed as
  mutable` for `ctx.out.push(Effect::Refused { needed_mib: x + ctx.reserve_mib, .. })`:
  `ctx` is an `EventContext` that derefs to the context, so two field accesses
  borrow it twice. Hoist the read. Hit three times.
- DSL shape: `Expected 4 closure arguments, actually have 3.` (transition vs
  internal action arity), `Unsupported closure input.` for `|ev: &E, ..|`,
  `Unsupported statement.` for a `let` in the builder function. All point at
  the right token.
- clippy's `redundant_closure` on `with_context(|c| InsideCtx::new(c))`
  can't be applied (finny's parser wants a closure literal: "Missing
  closure!"); the call now reads `InsideCtx::new(ctx.config)`.

The design problems (no bubbling, no parent events, region skipping, drain
order) produced no compiler errors at all; they showed up as reasoning about
the generated dispatch (`codegen.rs:864-972`), which I had to read.

## 6. Ergonomics

Concision: the most verbose slot, 2311 lines (pointer 336, wrap 330, polkit
408, transfer 462, lock 344, install 397, shared 34; enum 1195, statig 1596).
Each (state, event, transition) is its own builder statement: 35, 34, 36, 27,
29, 54 statements for 34, 32, 32, 26, 27, 35 transitions, plus one state and
one event struct per name. Shared superstate behaviour is duplicated across
substates (pointer's `Removed`, polkit's `PamError`/`PamInfo` thrice, wrap's
every event twice) unless it's lifted into a parent transition on the
sub-machine state.

Readability: each statement reads as a row of a transition table, which is
good for the linear install and the transfer table; the hierarchical targets
read worse, because what the spec writes as one superstate is spread over a
parent machine, a sub-machine, `handle`'s routing, and queued helper events.
A maintainer has to learn: which level each event is addressed to, that
sub-machines don't bubble and can't signal their parent, that forwarded events
skip other regions, transition order (stop sub-machine, exit, action, entry,
start sub-machine), the guard/target-before-action rule, and the per-target
buffer drain order.

Debugging: the state is visible (`get_current_states()` per level,
`get_state::<S>()`), and `Inspect`/`InspectTracing` can log every dispatch,
guard, entry and exit. Stepping into the generated code works but lands in
macro-expanded trait impls (`InsideTransition14` and friends, numbered by
declaration order).

## 7. Build

Edition 2024 crate; no `rust-version` declared. The dependency tree has 34
crates: two `syn` majors (derive_more 0.99 on syn 2, finny_derive on syn 3),
petgraph 0.5 with its own indexmap 1.9/hashbrown 0.12, heapless 0.7 (spin,
lock_api, hash32), tracing by default. No proc-macro warnings. Incremental
rebuilds of the slot took 1–5 s (debug); compile times are phase 3's.
`Cargo.lock` unchanged.

## 8. Spec questions

- lock, `OutputAdded(o)` in `Lockscreen` when `o` is already present: "if `o`
  is new: add it; then `OpenSurface(o)` if in `Lockscreen`" could open again.
  I followed the reference: no surface for a known output.
- polkit, the conversation numbers of `Starting`/`Prompted`/`Checking`: the
  spec gives each a `conv`; I used `last_conv` (equal by invariant 1).
- wrap: the spec's hierarchy (pending states inside the mode states, "pending
  kept" on `SetAutowrap`) is two orthogonal flags, which I modelled as
  regions; same behaviour, different chart.

## 9. Verdict

I wouldn't choose finny for these targets. It expressed all six and passed
everything, and its strengths are real: compile-time checks of the
transition graph, transition actions that see their two states, entry/exit
ordering that made install's inhibitor and stage runs fall out, orthogonal
regions, a fixed-size queue, and good error spans. But its hierarchy fights
statecharts like pointer, polkit and lock: sub-machines don't bubble, can't
signal or leave their parent, don't see the parent's data, and break region
fan-out, so each superstate turns into routing in `handle`, re-queued helper
events and guards that look into a child's current state. Its state data is
not scoped (every state struct lives always), so it makes none of the illegal
states unrepresentable (0 rejects; 4 probes accepted by reading stale or
placeholder data), and it isn't protected from whoever holds the machine
`&mut` (public fields, transition types runnable from any state): only a
wrapper like mine hides that. It was the most verbose slot (1.9× the enum
baseline's lines), and in one unpinned smoke pass it took 1.6–2.7× the enum's
time per event (indicative only; phase 3 measures). Per the survey, its maintenance is one author's two-day revival.
It would fit a flat, table-like machine with entry actions and few levels
(install's executing steps, a protocol handshake), where the builder rows
are the spec's table and the queue is useful. For Sekka's hot paths (pointer,
wrap) and its hierarchical shell machines, the plain enum, or a crate whose
superstates take the events their substates ignore (statig's worked
`pointer.rs`), stays much closer to the spec.
