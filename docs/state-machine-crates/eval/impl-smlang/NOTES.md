# impl-smlang: notes

Revised after the phase-2 review (`crates/review/smlang.md`) and the
"Transitions and forging" rule: pointer's hover moved into state data (P5
is now rejected); install's `Cancel` rows collapsed into one; each
generated machine is now kept in a private module, so no forging means
stays public (section 4); the four NOTES statements the review found wrong
are corrected.

## 1. Crate

smlang 0.8.0 (macro crate smlang-macros 0.8.0, on syn 1.0.109). No reserve
substitution; no `Cargo.lock` change.

**Survey check** (survey section "smlang 0.8.0", against
`smlang-macros-0.8.0/src/{codegen.rs,parser/*.rs,validation.rs}`):

- Right: nested `match state { match event }` dispatch; guards and actions
  are methods of a generated `StateMachineContext` trait with no default
  bodies; actions return the target state's data; `#![no_std]`, no
  allocation; `Err(InvalidEvent)` / `Err(TransitionsFailed)` /
  `GuardFailed` / `ActionFailed`; `_ + Ev` and `A | B + Ev`; internal
  transitions; no hierarchy, regions, history, timers or deferral; edition
  2018, syn 1.
- Missing or wrong:
  - **Reuse is keyed by signature.** A guard or action name can be reused
    only where the source-state data and event data types (and, for
    actions, the target data type) are all equal (`validation.rs`). An
    `A | B` pattern and a `_` wildcard are sugar for one row per state, and
    each expanded row is checked by the same rule. This, more than the lack
    of hierarchy, is what makes superstate emulation expensive (section 6).
  - **Entry/exit hooks take only `&mut self`**: no state data, no temporary
    context (so no sink), no knowledge of the transition. They don't run
    on internal transitions, on any transition whose target equals its
    source (there are no external self-transitions), or for the initial
    state.
  - Every mention of a data state must repeat its type (`Pressed(Owned)`).
    A data state's internal transition needs an action that returns the
    data again (by value from `&Data`): no in-place update.
  - `PartialEq` for `States` and `Events` compares the discriminant only
    (issue #21): `States::Pressed(1) == States::Pressed(99)` is `true`
    (checked in a scratch crate). No code or probe here compares data
    states with `==`; they use `matches!`.
  - `temporary_context` may be `&mut impl Trait`: the macro pastes the type
    into every guard, action and `process_event`, which makes them generic.
  - **Bug:** an external transition runs `on_exit_<source>` *before* the
    action. If the action returns `Err`, `process_event` returns
    `ActionFailed` with the state unchanged and the exit hook already run;
    each retry runs it again (checked in a scratch crate). A failed action
    also ends the row search: later guarded rows are not tried. No
    implementation here is affected (the only hooks are install's, whose
    actions never fail).
  - The README is stale against 0.8.0's codegen: `log_guard` takes
    `result: bool` (not `&Result`), `log_state_change` is
    `transition_callback(old, new)`, and `smlang::async_trait` (in the async
    example's `use`) isn't exported.
  - Generated public on the machine type: `StateMachine::new_with_state(
    context, any state)`, `context_mut()`, and every guard, action and hook
    (pub trait methods). They can be hidden (section 4).

## 2. Per target

Counts are rows as written, rows after `|`/`_` expansion, distinct guards,
distinct actions. In every target the `statemachine!` sits in a private
`mod fsm` (pointer.rs:49); the target re-exports `States`, `Events` and
`Error` and forwards `process_event`, `state_enum` and (where probes read
it) a read-only `context` (pointer.rs:323).

**pointer** (35 rows, 44 expanded, 7 guards, 30 actions). `Inside`'s hover
rides in every substate's data: `Hovering(Hover)`, `Pressed(Owned)` and
`DragsOnlyDragging(Owned)` with `Owned { hover, owner }` (one type, so they
share `A | B` rows), and `DragsOnlyPressed(EdgePress { hover, .. })`.
`Outside` and `NativeDrag` are unit variants; the context `Router` holds
only the threshold (pointer.rs:174). Cost of the superstate: `Inside`'s
five rows (Leave, StartDrag, Capture, Uncapture, Removed) are 16 rows, once
per data type, with three "exit Inside" actions (`exit_hovering`,
`exit_owner`, `exit_edge`, all calling `Hover::exit`, pointer.rs:126) and
three capture actions that only rebuild `Owned`. Entering `Inside` from
`Outside` needs its own five rows (Move, Press ×3, Release) with their own
action and guard names, because `Outside` has no data and `Hovering` has;
those actions call the `Hovering` ones with `&Hover::default()`. Every
hover update is copied out of the action into the target's data. The press
target is chosen by guards (`no_target`, `edge_press`); the action
recomputes it, because guards can't hand values to actions. Easy: the
table reads like the spec's, and the order of effects falls out of the
actions.

**wrap** (31 rows, 64 expanded, 7 guards, 15 actions). Four dataless
states; cursor, saved cursor, `previous` and links are the context
`Cursor` (wrap.rs:72), which is the spec's "machine data (all states)". A
row's target is fixed, so where a print may or may not leave a wrap
pending, a guard predicts the action's outcome: three `fills_*` guards over
`fills` (wrap.rs:121), which duplicates the arithmetic of `print`
(wrap.rs:135). `Repeat(n)` can't be a row (its end state depends on `n`
prints): `handle` feeds it as `n` `Print` events (wrap.rs:278). Without
autowrap the print from `Normal` and `Pending` is the same, so one pattern
pair covers both. Wildcard internal rows for `Tab`, `ReverseIndex` at the
top (guarded, and placed before the specific rows, which is legal) and
`FullReset`.

**polkit** (33 rows, 57 expanded, 14 guards, 30 actions). `Active`'s
request rides in the state data: `Run { req, conv }` for
Starting/Prompted/Checking, `Request` for RetryWait/Unavailable; `Idle`
has none. The context `Agent` holds the config and `last_conv`
(polkit.rs:88). Cost of the superstate: `Active`'s rows are 20 (Begin from
Active 8, Dismiss 3, AuthorityChanged 3, CancelRequest 3, ChooseIdentity
3), with 17 actions that differ only in what is running (EndHelper /
CancelRetry / nothing), and the Begin/Cancel/Choose guards doubled for the
two data types (10 guards for 5 predicates). A same-cookie `Begin` is
ignored because no guard passes (`TransitionsFailed`). The dialog's and the
conversation's entry/exit effects are in the actions: the hooks can't see
`conv` or `cookie`.

**transfer** (27 rows, 26 guards, 27 actions). One state per awaited
response with its data (`ItemId`, `Asked`, `Aside`, `Restore`); the frame
stack and **next** are context code in `Job` (transfer.rs:107), as in
impl-enum. **next** ends in `Probing` or `Finished` depending on the
frames, so every "item done" is two rows, `[.. && more_*] -> Probing` and
`-> Finished`: seven `more_*` guards (six are `Ok(self.more())`,
transfer.rs:122; `more_listed` also counts the frame the action is about to
push), `has_items` for `Start`, and eight `_next`/`_last` action pairs. The
"response is for my item" check is eight guards too (one per state data ×
event data). The `_next` actions return `self.next(out).ok_or(())`: if a
`more_*` guard ever mispredicted, the action would fail after pushing
effects and `handle` would swallow the error (the 5000-case differential
run found no case). A choice pseudostate (a transient state plus a
follow-up event from `handle`) would have saved about 16 methods; I kept
the table literal.

**lock** (two machines, 34 rows, 7 guards, 30 actions). No regions: the
delay region is its own machine (`name: Delay`, private `mod delay`,
lock.rs:23) living in the lock region's context `Screen` (lock.rs:140).
Lock actions send it `Settled` (lock confirmed or lock screen left) and
`Sleep(locked)` at the point the spec's effect order needs (lock.rs:179-192,
289); its own events (SessionStarted, LogindAppeared, Resumed,
LogindVanished) go to it from `handle` (lock.rs:407). Outside the crate
only its state is visible (`Screen::delay_state`, lock.rs:163).
`Lockscreen ⊃ Locked ⊃ Checking`: `Locked`'s data is
`Field { has_text, shake }`, `Checking`'s `Check { shake, check }`;
`Lockscreen`'s rows (LockEnded 3, UnlockRequested 3, OutputAdded 4,
OutputRemoved 4, PrepareForSleep 4) are 18 rows with 18 thin actions. The
entry/exit actions are in the actions (the hooks can't see `shake`, the
sink, or which event leaves). `PrepareForSleep` without
`lock_before_sleep` reaches the delay region from `Locking`/`Locked`,
which ignores it because no delay is ever held then.

**install** (38 rows, 53 expanded, 5 guards, 21 actions, 16 entry hooks).
The best fit: one state per stage; `Run(stage)` (after `Progress(n)`,
`StayAwake` at Erase), `Interrupt`, `Complete` and `Cancelled` come from
smlang's `on_entry_*` hooks (install.rs:233-285), which is what the spec
says ("each stage's entry runs it"). Newtypes `Root`, `Closure`, `Route`
label the positional state data. `Cancel` during preparation is one
action-less row over all five stages (install.rs:49; action-less rows may
span data types) into `Cancelling`, whose entry interrupts. `Confirm` and
`StageFailed` during preparation still need 3 rows each (Checks, the `Root`
group, Cache) with 6 thin actions, since their `Failed` effect isn't the
same for every way into `Failed`. `Executing`'s StageFailed is 2 rows;
`Cancelling`'s 7 rows have no actions.

## 3. Effects and dispatch

- pointer, wrap, polkit, transfer, lock: `temporary_context: &mut impl
  Sink<Effect>`. Every guard and action is generic over the sink: static
  dispatch, no `dyn`, no buffer, no allocation. Every guard and action
  carries the parameter, used or not.
- install: the entry hooks get no temporary context, so the context owns a
  `Vec<Effect>` (capacity 4, allocated once in `new`); actions and hooks
  push to it and `handle` drains it (install.rs:332). One extra copy per
  effect, and 24 bytes in the machine. install's smoke overhead against
  enum (replay-install 13.03 vs 7.05 ns/event, about +85% in the final run;
  +55% in the review's run) is the largest of the six. Most of it is
  probably this buffer, which smlang's argument-less hooks force: an
  action-only variant (`Run(stage)` in the actions, effects straight to
  `out`) would avoid it, at the cost of the hooks. I did not measure that
  variant.
- Per event: a `match` from the spec `Event` to the generated `Events`
  (struct fields become tuples); the no-op `log_process_event`; ignored
  events return `Err(InvalidEvent)` (undeclared pair) or
  `Err(TransitionsFailed)` (no guard passed), small values `handle`
  discards. Guards run in row order and shared sub-guards are re-evaluated
  per row (transfer's `answered` up to six times for one `Answered`;
  `more()` walks the frames per item done). Data states' internal
  transitions copy the data out of the action and back in (pointer now
  copies the hover on every move). wrap does the print arithmetic twice
  (guard and action); `Repeat(n)` is `n` dispatches.
- Smoke (`SME_SMOKE=1`, one short pass, other agents running: noisy),
  final run, ns/event smlang vs enum: pointer-stream 7.88 vs 7.35, wrap-80
  4.51 vs 3.98, wrap-200 4.45 vs 3.84, replay-pointer 17.08 vs 16.35,
  replay-wrap 10.90 vs 8.12, replay-polkit 15.96 vs 10.11, replay-transfer
  15.54 vs 12.40, replay-lock 12.05 vs 8.03, replay-install 13.03 vs 7.05.
  Rows vary by up to a third between runs (replay-pointer 20.31, then
  15.10, then 17.08); wrap's gap (13–18% in all three runs) and install's
  buffer look real. Every digest is "same as enum".
- Sizes (bytes, smlang / enum): pointer 32/28, wrap 48/32, polkit 28/28,
  transfer 48/40, lock 32/28, install 48/24 (the buffer). wrap's 16 extra
  bytes are crate layout: the generated `StateMachine { state, context }`
  keeps the state tag outside the context, which the `u128` links align to
  16 (`[u64; 2]` links would give 40).

## 4. Compile-time guarantees

Probes, classified by the transition API (`Machine::handle` and the
generated `process_event` with typed `Events`): **reject 7** (P2, P3, P4,
P5, P6, L6, I6), **runtime 29**, **accept 0**, **forge 0**, not applicable
0.

- **Reject:** only state-data shapes: a variant that carries no data
  (`States::NativeDrag`, `States::Outside`, `LockStates::Locking`), or
  carries a type without the field (`DragsOnlyDragging(Owned)` has no
  `beneath`, `Pressed(Owned)` no `at`, `Prepared(Route)` no `closure`), or
  a data struct missing its owner. The errors are plain rustc errors at the
  probe's line and name the real mistake; P2's also points into the DSL
  row where the variant was declared:

  ```text
  error[E0532]: expected tuple struct or tuple variant, found unit variant `States::NativeDrag`
    |     if let States::NativeDrag(owner) = m.state_enum() {
    |            ^^^^^^^^^^^^^^^^^^^^^^^^^ help: use this syntax instead: `States::NativeDrag`
   ::: src/pointer.rs
    |             NativeDrag + DragEnded = Outside,
    |             ---------- `States::NativeDrag` defined here
  ```
- **Runtime:** everything the table decides. The refusal is a value, so
  probes assert `Err(Error::InvalidEvent)` or `Err(Error::TransitionsFailed)`
  from `process_event`. `process_event` takes any `Events` in any state;
  nothing is checked at compile time about which events a state takes.
  W1, T3, I4 and I5 were `accept` through `new_with_state`/`context_mut`;
  by the transition API they are refused at run time (the `fills_*` guards;
  `Restoring` declares only `Restored`; the `room_unpacked` guard; `Cancel`
  undeclared after `Erase`).
- **Forging: no forge case, because no means stays public.** smlang
  generates `new_with_state`, `context_mut` and the trait's guards, actions
  and hooks as `pub` items of the module that invokes the macro. Here that
  module is a private `mod fsm` (and `mod delay`), the targets re-export
  only `States`, `Events`, `Error` (and `DelayStates`), forward
  `process_event`, and keep the contexts' constructors and fields private.
  A scratch crate outside the workspace confirmed it: naming
  `wrap::StateMachine` (E0603 private struct), `wrap::fsm` (private
  module), `Cursor::new` (E0624 private function),
  `wrap::StateMachineContext` (private trait) and `lock::delay` (private
  module) all fail to compile. The cost is three forwarding methods and a
  module wrapper per target (about 11 lines each). A maintainer who uses
  the generated `StateMachine` directly, as smlang's examples do, gets all
  three forging means in public.

## 5. Errors while writing it

Few, because I read the parser and validation first. The one I hit: I
named a guard and an action both `accepted` (lock). The macro doesn't
catch it; rustc reports four errors in generated code, all spanned on the
whole macro, the first:

```text
error[E0428]: the name `accepted` is defined multiple times
  --> impl-smlang/src/lock.rs:91:1
91 | statemachine! {
   | ^^^^^^^^^^^^^ `accepted` redefined here
   |               previous definition of the value `accepted` here
```

The last of the four (E0308, expected `&(u32, PamOutcome)`, found `(u32,
PamOutcome)`) is misleading. Fixed in a minute. The macro's own validation
errors use `Span::call_site()`: they underline the whole `statemachine!`
block, not the row. Reproduced in a scratch crate:
`Action `exit_inside` can only be reused when all input states, events, and
output states have the same data` (which rows?), and, for a data state
mentioned once without its type, `This event's type Pressed does not match
its previous definition` (it's a state). Row order matters too: an
unguarded wildcard before an unguarded specific row for the same event
fails with "State and event combination specified multiple times", and a
guarded specific row after an unguarded wildcard with "guarded transition
is unreachable"; a guarded wildcard first is legal (wrap's `ReverseIndex`
relies on it). Hook names keep the state's case (`string_morph`): a state
`A` gets `on_exit_A`.

## 6. Ergonomics

- The DSL table is the spec's transition table, readable at a glance; guard
  expressions (`[current && retries_left]`) read well.
- Concision suffers wherever the spec has hierarchy: the signature rule
  turns one superstate row into one row and one action *name* per substate
  data type, so the context impl is a long list of 1–3 line methods
  (`exit_hovering`/`exit_owner`/`exit_edge`, `more_probed`/`more_placed`/…).
  LoC (`metrics/sme.py loc`): pointer 313, wrap 266, polkit 328, transfer
  387, lock 367, install 299, shared 6, total 1966 (enum 1195, statig
  1596 in the same run).
- **Why wildcards don't collapse the superstate rows** (checked against the
  parser and the review's scratch crate):
  - `_ + Ev` expands to every state with no row for `Ev` *yet* (states
    outside the superstate included, unless an explicit row comes first),
    and each expansion is signature-checked like a hand-written row. A
    wildcard with a guard or an action fails as soon as the covered states'
    data types differ (`Action exit_inside can only be reused ...` for
    `_ + Leave / exit_inside` over `Hovering(Hover)`/`Pressed(Owned)`).
  - Action-less wildcards and patterns over mixed data types are legal
    (install's `Cancel` row uses one), but then the effects must come from
    entry hooks, which see neither the source's data (`hover`, `conv`,
    `cookie`, `shake`) nor the event (`ReleaseLock`) nor a sink.
  - `transition_callback(&self, old, new)` sees both states with their
    data, but takes `&self`, gets no sink, and runs only on non-self
    transitions: effects from it would need interior mutability and a
    buffer.
  So the duplication is a crate cost, not a choice.
- Boilerplate per method: `Result<bool, ()>` / `Result<T, ()>` and the
  unused `_: &mut impl Sink<Effect>`. rustfmt doesn't format inside the
  macro.
- A maintainer must learn: row order (guarded before unguarded; specific
  rows before unguarded wildcards), the reuse-by-signature rule,
  internal-transition semantics (no hooks, data re-returned), hooks without
  arguments, the discriminant-only `PartialEq`, that a guard can't pass a
  value to its action, and (to keep forging private) wrapping the macro in
  a private module.
- Debugging: `derive_states: [Debug]` gives a printable state;
  `log_process_event`, `log_guard`, `log_action` and `transition_callback`
  hooks are there for tracing; `process_event` tells why an event did
  nothing. Stepping lands in macro-generated code with no source.

## 7. Build

Edition 2018 crate, syn 1, no MSRV declared. The workspace lock already
holds syn 2.0.119 and syn 3.0.9, so smlang-macros (syn 1's only user) adds
a third syn major version. Used from edition 2024 with no rustc or clippy
warnings in the generated code. Crate-only rebuild after `touch`: 0.42 s
debug, 0.58 s release (wrapper's own timing; phase 3 measures properly).
The slot's first build, smlang-macros and syn 1 included: 2.75 s. No
proc-macro warnings.

## 8. Spec questions

None blocking. Readings that rely on equivalences rather than literal
rows: lock's `PrepareForSleep` without `lock_before_sleep` from
`Locking`/`Locked` reaches a delay region that holds nothing; polkit's
same-cookie `Begin` is ignored by "no guard passed".

## 9. Verdict

For flat machines smlang is decent: a declarative table, generic sink
plumbing via `temporary_context`, `Err` values for ignored events, close to
enum speed, nothing allocated. install (linear stages, entry hooks) and
wrap (four dataless states) fit it. It fights the hierarchical targets
(pointer's `Inside`, polkit's `Active`, lock's regions and
`Lockscreen ⊃ Locked`, transfer's choice of next): row and method counts
multiply with each substate data type. Superstate data can stay typed (P5
is rejected now), but only by repeating it in every substate's data, with
separate entry rows. The compile-time guarantees are the state-data shapes;
everything else is runtime. Forging means are generated public, though a
private module hides them. Add the maintenance risk (last release 2024-08,
syn 1, the exit-before-failed-action bug, a stale README). I would use it
only for small flat protocol machines like install, not for Sekka's
pointer, polkit or lock machines.
