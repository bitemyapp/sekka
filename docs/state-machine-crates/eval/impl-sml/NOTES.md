# impl-sml: the six targets with stateforward-sml 2.0.0

## 1. Crate

`stateforward-sml = "=2.0.0"` (imported as `sml`), with its proc-macro crate
`stateforward-sml-macros` 2.0.0; no features, no reserve substitution. 2.0.0
was published 2026-11-02, two days after 1.5.0; docs.rs has no 2.0 build, so
this is written from the tarball (`README.md`, `docs/dsl.md`,
`docs/migrating-to-2.0.md`, `macros/src/*`, `tests/*`).

One `sml!` invocation holds one table, or adjacent tables that form a
composite tree (`state<Child>` names a child table). Three separate
generators produce the code: `codegen.rs` (flat), `orthogonal_codegen.rs`
(several `*` rows, no children), `composite_codegen.rs` (one child:
`generate_code`; anything bigger: `generate_multi_code`). They differ in
small ways (see 6).

Every conformance trace passes (pointer 21/21, wrap 14/14, polkit 16/16,
transfer 20/20, lock 18/18, install 16/16), all six differential tests pass
(256 and 5000 cases), and every smoke row is `same as enum`.
`NOT_EXPRESSED` is empty and nothing is ignored.

### Crate findings (immature or broken in 2.0.0)

`tests/crate_findings.rs` pins each of these with a test that asserts what
the release does:

1. **Any composite-to-composite transition skips the child's exit, reset
   and entry** (single-child and tree generators). A self-transition
   `state<Kid> + Again = state<Kid>`, or `state<Ka> + Swap = state<Kb>` in
   a tree, runs the parents' `state<..>` exit and entry hooks, but not the
   source child's exit, nor the target child's reset to its initial state
   and initial entry: `Swap` logs only `exit Ka, enter Kb`, and swapping
   back finds `Ka` still in its old state (history without `(H)`). The
   docs say external self-transitions run exit and entry and that
   re-entering a child resets it. Cause: `composite_lifecycle`
   (`macros/src/composite_codegen.rs:4559-4576`, and the same gate inline
   for completion rows at 4770-4785 and 4849-4864) emits the child's exit
   only for `composite -> non-composite` and its entry only for
   `non-composite -> composite`; the tree generator's per-child exit and
   entry code is right, only the gate is wrong. Polkit's "Begin for
   another cookie" first used a self-transition and lost the new request's
   `StartHelper`; it now goes to `Idle` and `defer`s the `Begin`
   (polkit.rs:62). No target now has a composite-to-composite edge.
2. **`process(E)` is not queued in synchronous machines.** It recurses into
   the dispatcher inside the current step (`let _ =
   self.__sml_process_event_unlocked(E)?;`, `orthogonal_codegen.rs:1671`,
   `composite_codegen.rs:4423`, `codegen.rs:999`), so in an orthogonal
   machine the later regions see `E` before the event that caused it, and
   in a flat machine `A + E / process(P) = B` handles `P` in `B` before
   `B`'s anonymous row moves on to `C`. sml.cpp queues it until the step
   ends, the async generators here do too, and `docs/sml-cpp-parity.md`
   claims "post-transition iterative dispatch in sync/async flat,
   orthogonal, parent, and child tables".
3. **An unhandled processed event aborts the current step**: the `?` above
   returns `Err(InvalidEvent)` from the outer `process_event`, after the
   transition's effects but before the remaining regions see the event and
   before completion transitions (the deferred-event drain, by contrast,
   tolerates `InvalidEvent`). In lock a sleep that finds no delay held
   processes an event nobody takes; nothing was left to do in those steps,
   so the spec's behaviour holds either way, but `process_event` would
   return `Err` for an accepted `PrepareForSleep`. Two defensive no-op rows
   keep it `Ok` (lock.rs:71-72).
4. **A flat orthogonal table with no row reaching `X` doesn't compile**:
   the generated code names `States::X` (`no variant ... named X`, spanned
   over the whole macro). Tree generators are fine (lock's root has no `X`).
5. Internal (processed) events are variants of the public events enum.
   Declared as `Name` or `Name(bool)` rows, anyone can build them: my first
   version let a caller send install's `Ready(true)` to skip the space
   check, and lock's `SleepWhileLocked` while `Held` and `Locking` to drop
   the delay before confirmation. The crate lets you seal them: `process`
   takes any expression, evaluated in the implementation's module, so the
   events are now `event<T>` with a private-field payload
   (`pub struct Ready(bool);`, install.rs:28-29; lock.rs:29-30), which
   outside code can't build (E0423). Cost: lock needs its own
   `release_for_sleep(&SleepWhileLocked)`, since one callback can't take two
   payload shapes.
6. Smaller rough edges: in a flat table an action takes a `Name(T)` event's
   payload by value (its guards by reference), while `event<T>` payloads
   and every composite callback take a reference; a wildcard source (`_ +
   E`) only fills states with no row for `E` yet, so it can't be followed
   by a second wildcard row for `E` ("Wildcard has no effect"), and its
   position decides precedence; generated `States` enums derive nothing (no
   `Debug`); when every guard fails, a flat machine returns
   `TransitionsFailed` (as the error enum documents) but a composite
   returns `InvalidEvent`.

### Where the survey was wrong or incomplete

- "Data in states: yes, produced by the action": true, but a payload is
  read-only afterwards (guards, actions and exit hooks get `&T`); changing
  it means a self-transition whose action returns a new value
  (pointer.rs:71). Composite states (`state<Child>`) and a child's
  initial state can't carry data from the transition that enters them, so
  superstate data lives in the context.
- "Outside code reads state through a visitor closure": only root states
  are visited; a child state's payload can't be read from outside at all,
  and `is(...)` ignores payloads, so asking "is the child in `Pressed`?"
  needs a dummy payload (`is(&PointerInsideStates::Pressed(Node(0)))`).
- "Dispatch and allocation: ... bounded defer/process queues": the defer
  queue is a 16-slot array of the events enum in every machine that defers
  (pointer 440 bytes, polkit 248); process events use no queue at all in
  sync machines (finding 2).
- "Concision: about 25-30 lines": the tables alone are 21-54 rows here,
  and each needs its callbacks (21-40 per target); see 6.
- The rest (licence, MSRV 1.90, edition 2018, `no_std`, `forbid(unsafe)`,
  single maintainer, 6 releases since 2026-07-11, no getters in 2.0)
  checks out against the tarball.

## 2. Per target

Every target: the machine wraps the generated `XStateMachine` (pub field
`machine`, its `process_event`/`is`/`visit_current_states` are the typed
API). `handle` maps the spec event to the crate's event and ignores the
`Err` of an event no row takes. The context is the struct implementing the
generated `XStateMachineContext`; the machine owns it and there are no
getters, so `state()` uses only `is`/`visit_current_states`.

### pointer (pointer.rs)

`Pointer` table {`Outside`, `state<Inside>`, `NativeDrag`}; child `Inside`
{`Hovering`, `Pressed(Node)`, `DragsOnlyPressed(EdgePress)`,
`DragsOnlyDragging(Node)`}. `Inside`'s entry resets the hover and its exit
emits `UnderOff`/`HoverOff` (pointer.rs:79-80); `Leave` and `StartDrag` are
parent rows on `state<Inside>`. "Enter `Inside`, then as `Hovering`" is
`"outside"_s + event<Move> / defer = state<Inside>` (pointer.rs:75-77): the
deferred event is re-dispatched once the target is entered, which is
exactly the spec's wording. Child-first dispatch with bubbling gave
`Removed(n)` for free: an owner row takes it when `n` is the owner, else
the parent row forgets `n` from the hover. Workarounds: the hover is
context data (a composite state has no payload); `Pressed` and
`DragsOnlyDragging` share the payload type `Node` so their rows share
callbacks (pointer.rs:58-63), since a callback's signature includes the
source payload and event types. 30 rows, 29 callbacks.

### wrap (wrap.rs)

A flat table of the four states. The spec's chart (pending inside
`Autowrap`/`NoAutowrap`, "pending kept" when switching) can't be
composites: re-entering a child resets it, and shallow history restores
the child's last state, not the current pending flag; orthogonal regions
can't consult each other in guards. Guards choose the row before the
action runs, so a print's target needs a guard that computes where the
print leaves the cursor (`after`, wrap.rs:151; `repeat_lands_pending` loops
it for REP): the print's outcome is computed twice. Each source state needs
its own print guard and action (four of each for `Print`, four for
`Repeat`): callbacks can't see the source state. `_ + event<E>` wildcards
cover the states a specific row doesn't (wrap.rs:57-86). 47 rows, 34
callbacks.

### polkit (polkit.rs)

`Polkit` {`Idle`, `state<Active>`}; child `Active` {`Starting`, `Prompted`,
`Checking`, `RetryWait`, `Unavailable`}. Entry actions: `Active` opens the
dialog, `Starting` starts a conversation, `RetryWait` arms the timer
(polkit.rs:41-42,59). The spec's exits depend on the target (no
`EndHelper` to `Prompted`/`Checking` or after `Unreachable`; no
`CancelRetry` after the timer), and an exit action can't know its target
(exit callbacks get the temporary context and the source payload only; the
context's `transition_callback(&self, old, new)` does see both root
states, but runs after the action, with `&self` and no temporary context,
so it can't emit effects), and `CloseDialog` must follow the `Reply` that a
transition action emits, while exit actions run before transition
actions: so these are transition actions. `ChooseIdentity` has one row per
kind of source (polkit.rs:50-52), so the table says what it ends
(`end_conversation` or `cancel_timer`). The parent's end-of-request rows
can't see the child state, so the context mirrors two facts of it: the
running conversation (`helper: Option<u32>`, polkit.rs:87) and the armed
timer (polkit.rs:89). "End the request, then as in `Idle`" is `/
(end_for_another, defer) = Idle` (polkit.rs:62), after finding 1 broke the
self-transition. Events are declared `Failure(Conv)` rather than
`event<Failure>` so the five helper events share `current`/`exhausted` and
the back-off actions. 25 rows, 30 callbacks.

### transfer (transfer.rs)

Flat: states carry the awaited item (`Probing(ItemId)`, `Asking(Ask)`,
`PlacingReplacement(Aside)`, `Restoring(Restore)`, ...), guards compare the
response's item with the payload. "Item done, then next" goes to `Next`, a
choice state whose anonymous rows take the next item or finish
(transfer.rs:56-57; the guard `more` checks the frames before the action
pops them). The merge frames are a `Vec` in the context: a table can't
nest itself, so a folder's sub-machine isn't a child table, and the spec's
"popping a frame" effects are plain code, not exit actions. One guard per
(payload, event, outcome) because guards can't be shared across payload
types (`for_item` had to become `aside_reported`, `placed_for_item`, ...).
21 rows, 40 callbacks.

### lock (lock.rs)

The root table has two orthogonal regions (lock: `Unlocked`,
`state<Lockscreen>`; delay: `NotHeld`, `Held`, `SleepPending`);
`Lockscreen` {`Locking`, `state<Locked>`}; `Locked` {`Idle`,
`Checking(u32)`}. Entry/exit: `Lockscreen` requests the lock and opens
the surfaces; `Locked`'s entry sends the hint, its exit cancels a running
shake. Leaving the lock screen keeps the spec's order (CancelShake,
ReleaseLock, LockedHint(false), ReleaseDelay) because exits run
innermost-first, then the action, then the next region: `UnlockRequested`
is a root row with `(release_lock, hint_unlocked)`, and a PAM acceptance
reaches `X` in `Locked`, whose completion `state<Locked> / release_lock = X`
and then `state<Lockscreen> / hint_unlocked = Unlocked` unwind it
(lock.rs:48,59). The delay region learns the phase a sleep found from
sealed events the lock region processes (`process(SleepWhileLocked(()))`,
finding 5), the sml.cpp way for regions to talk. `has_text` and `shake`
are context data (composite states carry no payload). 34 rows, 22
callbacks: the crate's best fit here.

### install (install.rs)

`Install` {`Idle`, `state<Preparing>`, `Prepared(bool)`, `Cancelling`,
`state<Executing>`, terminals}; `Preparing` {`Checks`,
`Configuration(u64)`, `Evaluation(u64)`, `Build(u64)`, `Cache(u64)`};
`Executing` {`Erase` .. `Flush`}. Each stage's entry runs it; `Executing`'s
entry/exit hold and release the inhibitor; `Complete` and `Cancelled`
report themselves on entry. A failed executing step bubbles to
`state<Executing> + StageFailed`, which exits (`AllowSleep`) before
`Failed { erased: true }`: the spec's order with no extra code.
Workarounds: a child can't target its parent's states, so preparation ends
by processing the sealed `Ready(deferred)` or `Stop` (install.rs:28-29,
38-45), which only the parent takes; the route `Erase`..`Files` carry is
the context's (`deferred`), because a child's initial state takes
`Default` data and the codegen never builds a payload for a composite
target; `Cancelling` needs one row per stage result (install.rs:80-86;
rows can't list several events). 54 rows, 39 callbacks.

## 3. Effects and dispatch

The context can't be generic per call, so every table declares
`temporary_context: &mut dyn Sink<Effect>`: `out` is passed to each
`process_event` and on to every guard, action and entry/exit callback
(one virtual call per effect, as in impl-statig). No allocation per event;
transfer's frame `Vec` grows on merges only, like the reference's.

Per-event costs the crate adds: conversion to the events enum, the policy
hooks (no-ops for `NoPolicy`), a `match` on (state, event) per table level
with child-first fall-through, a `Result` from every callback, an
`Err(InvalidEvent)` for each ignored event, and a completion pass
(`stabilize`) after every handled event. Pointer's `Outside` entries and
polkit's replaced request go through the defer queue (push, then pop and
re-dispatch); lock's and install's processed events are a nested dispatch.
Wrap computes each print's outcome twice (guard, then action).

Sizes (`bench --bin sizes`): pointer 80, wrap 48, polkit 72, transfer 48,
lock 36, install 40 bytes (enum: 28, 32, 28, 40, 28, 24). Pointer and
polkit use a one-slot defer queue (`DeferOne`, lib.rs:17-27, through the
crate's `sml_policies!`); with the default 16-slot queue they are 440 and
248 bytes.

Smoke (`SME_SMOKE=1`, one pass, noisy: other agents were building), ns per
event, sml vs enum, after the review fixes: pointer-stream 7.42 vs 6.80,
wrap-80 6.64 vs 4.04, wrap-200 6.54 vs 4.20, replay-pointer 22.60 vs
15.94, replay-wrap 11.94 vs 7.15, replay-polkit 18.30 vs 9.15,
replay-transfer 17.85 vs 12.23, replay-lock 19.17 vs 10.91, replay-install
12.95 vs 8.11; every digest `same as enum`.

## 4. Compile-time guarantees

Tally (36 probes): 5 rejected (P2, P3, P4, P6, I6), 31 refused at run time
(P1, P5, W1-W5, K1-K6, T1-T6, L1-L7, I1-I5), 0 accepted, none not
applicable, no forge case. (The first version said 31 runtime and 1 accept,
37 in all: it had 30 runtime cases; sealing the internal events moved I4
from accept to runtime and closed the L1 hole.)

The rejections all come from the generated states enums' payload shapes:
`NativeDrag` and `Prepared` have no owner or closure, `Pressed` and
`DragsOnlyDragging` hold a `Node`, not an `EdgePress`. The errors point at
the real mistake and, helpfully, at the table row that declared the
variant:

```text
error[E0308]: mismatched types
  |     let _ = PointerInsideStates::Pressed(press);
  |             ---------------------------- ^^^^^ expected `Node`, found `EdgePress`
note: tuple variant defined here
 --> src/pointer.rs
  |          DragsOnlyPressed(EdgePress) + event<Capture> / capture_edge = Pressed(Node),
```

Those are shape guarantees only: the states enums are public and anyone
can build a value, which writes nothing into a machine. P2 has a nuance:
the inactive child keeps its last state, so after a drag from
`Pressed(n)` `is(&PointerInsideStates::Pressed(_))` still answers true in
`NativeDrag`; `state()` tests the root first (pointer.rs:342-357), the
owner can't be read from outside, and the child resets on re-entry.

No forging means stays public, so there is no forge case. The generated
state setters (`set_current_states`, `set_child_state`,
`set_<child>_state`, `new_with_state_data`) exist only when the policy's
testing slot implements the sealed `TestingAccess` trait, which
`TestingPolicy` does; `TestingPolicy` itself is public, but every machine
here uses `NoPolicy` or `OneDeferred`, and building a `TestingPolicy`
machine needs a context value, and all six contexts have private fields
and no public constructor. 2.0 has no context getter. Writing the public
`machine` field can only move in another machine, itself reached through
events. `initialize()` re-runs only the root initial state's entry hook,
and no root initial state has one. The internal events are sealed
(finding 5). So the class is what the events allow: rows and guards refuse
everything else at run time (an event no row takes is an `Err`, which the
runtime cases assert). Data the crate makes context data (pointer's
hover, lock's shake, wrap's cursor) gets no type-level protection (P5,
L6, W1 are runtime). Under "a transition stands for its event", no probe
here becomes `reject`: the crate's typed entry is `process_event` with an
event value, not one method per transition.

## 5. Errors while writing it

- `Guard `for_item` can only be reused when all input states and events
  have the same data` (flat generator), spanned over the whole `sml!`
  invocation with no row: one guard name used with two payload types.
  Two minutes; the composite generator says `callback `release_delay` has
  incompatible uses` and points at the callback.
- `no method named `clone` found for struct `pointer::Move``, spanned over
  the macro: deferred events must be `Clone` (documented). One minute.
- `Wildcard has no effect` on the second of two `_ + event<ReverseIndex>`
  rows: the parser (`parser/mod.rs:958`) only fills states with no row for
  the event yet. Five minutes in the parser.
- Finding 1 gave no error at all: the polkit trace caught a missing
  `StartHelper`, and finding it took ten minutes in `composite_codegen.rs`.
- Callback signature mistakes are ordinary trait errors (E0046/E0053 with
  the expected signature), which are easy.

## 6. Ergonomics

The tables read like the spec's transition tables, and child-first
bubbling, entry/exit hooks, anonymous (choice) rows, `defer` and
orthogonal regions matched the spec's wording directly: lock (244 lines,
enum 211) says in its table where each effect comes from, where the enum
reference spreads that over helper calls. The cost is the callbacks: every guard
and action is a trait method whose signature is fixed by the source
payload, event payload and target payload, so small behaviours shared by
rows with different events need a callback each (a one-line wrapper of a
helper); payload sharing (`Failure(Conv)`, `Pressed(Node)`) is the
workaround. Guards run before actions, so an outcome the action computes
(wrap's pending, transfer's next item) needs a predicting guard or a choice
state. The total is 1729 lines (enum: 1195; statig: 1596).

A maintainer must learn the DSL (two event spellings with different
callback rules, `state<T>` vs `Name(T)`, wildcard precedence, `defer`,
`process`, completion rows, child-first dispatch), the generated names
(`PointerInsideStates`, `XStateMachineContext`), and that callbacks can't
see the current state or context fields outside them. Debugging: no
`Debug` on generated states (add it with `states_attr`, if every payload
has it), `StateName::name()` gives static names, and a `Logger` policy
traces every guard, action and state change by name (no payloads). The
generated code is only visible through macro expansion.

## 7. Build

Edition 2018 crates with MSRV 1.90; no warnings in impl-sml. Dependencies:
`proc-macro2`, `quote`, `syn` 2 (with `extra-traits`, `visit`),
`string_morph`. A touch of `lib.rs` rebuilds impl-sml in 0.57 s (debug) and
0.98 s (release) here; phase 3 measures clean builds. The `graphviz`
feature (unused) writes diagrams at build time.

## 8. Spec questions

None needed a choice. Two readings worth stating: lock's `SleepPending +
LockConfirmed` row releases the delay without checking the lock region,
relying on invariant 1 (`SleepPending` only while `Locking`); polkit's
replaced request ends and then re-runs the `Begin` in `Idle`, literally
"end the request, then as in `Idle`".

## 9. Verdict

For hierarchical, protocol-shaped machines whose effects follow entry/exit
and child-first order (lock, install, polkit, pointer), sml's tables
follow the spec closely, with real orthogonal regions, composites,
choice states and `defer`. I would not choose it for Sekka now: 2.0.0 is two days
old from one maintainer, and I hit two semantic bugs (finding 1, findings
2-3) in documented features, one of which silently dropped an effect.
Where its cost shows (wrap: a hot path whose outcome the action computes;
transfer: recursive frames), it is slower and wordier than a plain enum,
and the per-event overhead (1.4-2x the enum's time on replays) matters for pointer
and wrap. Revisit for lock-screen and installer machines once the
composite self-transition and `process` ordering are fixed upstream.
