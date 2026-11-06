# impl-statig: notes

## 1. Crate

statig 0.4.1 (runtime; `no_std`, no required dependencies) with its macro
crate statig_macro 0.4.0 (`#[state_machine]` on an impl block; the survey
names only 0.4.1). No reserve substitution, no lock-file change. All six
targets implemented; every trace passes and every differential test passes
(256 cases, and 5000 in release). An independent review found no must-fixes;
its five should-fixes are applied (event in the context instead of flags,
polkit's cost attributed to the crate, superstate `local_storage`, softer
maintenance claims, the source of the 0 accepts).

Survey claims checked against the source:

- Right: `match` dispatch over a generated `State` enum and a borrowing
  `Superstate<'sub>` enum, `Super` bubbling to an implicit `Top` that ignores
  everything, no allocation, no timers/queue/history/regions, hooks,
  `&Event` and `&mut` state-local data, a concrete context, declared MSRV
  1.66 (the README badge says 1.65), `proc-macro-error2` in the macro.
- Confirmed by running it: the unreleased fix the survey mentions is real.
  In 0.4.1 `before_transition` gets its arguments swapped
  (`blocking/inner.rs:42` passes `(&target, &self.state)` to a
  `(source, target)` hook). A scratch machine going x → y saw `source = y`.
  `after_transition` (`inner.rs:58`) is right.
- Incomplete. The "shortest path" is computed from enum discriminants
  (`blocking/state.rs:68-69`, `superstate.rs:51-52`, which also uses
  `unsafe` `transmute`). A transition to the same leaf is an external
  self-transition of the leaf only. A target that shares a superstate never
  exits it, even when it carries different superstate data (see polkit).
- Missing: state-local parameters marked `#[default]` (left out of the
  generated constructor), `#[state(local_storage("x: T"))]` (fields the
  handler doesn't take), `#[superstate(initial = "…")]` (a
  `Superstate::name()` constructor), `state(name = …)`, `visibility`.
  I used the first two throughout.
- Misleading. "State-local data is only reachable from that state and its
  superstates" holds inside handlers. But the generated `State` enum is
  `pub` (`visibility` defaults to `pub`), its variant fields are public, and
  `state()` returns it, so outside code can read any state's data. The
  generated constructors are private. `UninitializedStateMachine::state_mut`
  is safe (`blocking/state_machine.rs:729`); only the initialized machine's
  is `unsafe` (`:475`). See §4.
- Concision: "about 40–50 lines for press/drag/hover" describes a toy. The
  full `pointer` is 274 counted lines (impl-enum 203).

## 2. Per target

Lines (`metrics/sme.py loc`): pointer 274, wrap 229, polkit 221, transfer
259, lock 275, install 318, shared 20; total 1596 (impl-enum 1165).

**pointer** (the phase-1 worked example, reviewed). I kept the model: leaves
`outside` and `native_drag`, and superstate `inside` over `hovering`,
`pressed`, `edge_pressed` and `edge_dragging`. Each leaf carries the
state-local `hover`, which the superstate borrows. `exit_inside` emits the
`UnderOff`/`HoverOff` pair. It matched the spec and passes everything, so
the changes are small:

- the context became the shared `crate::Out<'_, Effect>` (`lib.rs`);
- the machine initializes into `Nowhere`, a sink that panics if an initial
  entry action ever emits;
- P1 now also releases from `Outside`, the spec's second case.

Workaround still in place: `outside` (`pointer.rs:92`) repeats
`hovering`'s move/press/release for the entering event. A handler can't
transition and then let the new state handle the same event. Polkit
re-dispatches instead (below); here the duplication is local, so I left it.

**wrap**. Four data-less leaves under the superstates `autowrap` and
`no_autowrap`. The superstates handle the cursor controls
(`Screen::control`, `wrap.rs:83`, returns `Some(pending)` or `None`, mapped
to the region's own leaf) and `FullReset`. The leaves handle `Print`,
`SetAutowrap` (it crosses regions and needs the leaf's pending flag) and
`SaveCursor` (it saves the pending flag). Cursor, saved cursor, `previous`
and links are shared storage (`&mut self`). The hot path is a `Print` in
`normal` that doesn't reach the edge: `Handled`, no transition.

- Easy: the per-mode print rules sit in four short leaves.
- Workaround: `Repeat(n)` is unrolled in `Wrap::handle` into `n` `Print`
  dispatches (`wrap.rs:248`), reading `previous` through the machine's
  `Deref` to its storage. A handler returns one `Outcome`, and the state can
  change between the prints.
- A settling control in a settled leaf is a self-transition
  (normal → normal): cheap, but a transition.
- No entry or exit actions (the spec has none).

**polkit**.

- States: `idle`; superstate `active` (state-local `request: Request`;
  entry `open_dialog`, exit `close_dialog`, which emits `Reply` then
  `CloseDialog`); inside it, superstate `conversing` (adds
  `conv: u32`; exit `end_helper`) over `starting` (entry `start`
  assigns the next conversation number to its `#[default]` field and emits
  `StartHelper`), `prompted` and `checking`; and `retry_wait` (entry arms,
  exit cancels) and `unavailable` (entry shows the note), both directly in
  `active`.
- Effect order falls out of statig's sequence: handler effects, then exits
  bottom-up, then entries top-down. `Success` gives `EndHelper`, `Reply`,
  `CloseDialog`; back-off gives `EndHelper`, `ShowNote`, `ArmRetry`.
- An action's signature has no event or target, but the context is my own
  type and reaches every action, so `handle` puts the (`Copy`) event in it
  (`Cx::event`). The spec's event-dependent exits read it: `end_helper`
  skips `EndHelper` after `Unreachable` (`polkit.rs:147`), `cancel_retry`
  skips `CancelRetry` after `RetryTimer` (`:226`), and `close_dialog`
  replies `authorized: true` only after `Success` (`:116`). My first
  version used three state-local flags set by the handlers instead
  (`Conversation::helper`, `retry_wait`'s `armed`, `Request::authorized`);
  the review showed they were avoidable, and removing them cut 26 lines.
  The cost of this form: an exit action now names the events that end its
  state, so a new way out must be checked against those `matches!`.
- Workaround: restarts inside `conversing` (a `Failure` while checking,
  `ChooseIdentity`) don't exit it, because the target shares the
  superstate. So those handlers emit `EndHelper` themselves.
  `ChooseIdentity` does it in `conversing` and returns `Super` to `active`
  (`polkit.rs:136`).
- Workaround: `Begin` for another cookie must exit and re-enter `active`.
  Starting(A) → Starting(B) has the same discriminants, so statig would run
  neither `close_dialog` nor `open_dialog`. The handler goes to `idle` and
  sets `Cx::again`, and `Polkit::handle` dispatches the event a second
  time (`polkit.rs:94`, `:263`).

**transfer**. One leaf per awaited response, with `item`, `can_merge`,
`aside` and `then` as state-local storage. Every leaf's entry action sends
its request (`Probe`, `Ask`, `PlaceAt`, `SetAside`, `Trash`/`Delete`,
`RestoreAside`, `List`), so every way into a state asks for what it awaits.
The self-transition probing → probing re-runs `probe` for the next item.
`placing` gained a `name` (`local_storage`) so its entry action can send
`PlaceAt`. Guards on the current item are plain `if i == *item` in each
handler.

- Not expressed as hierarchy: the merge frames. They are recursive, and
  statig's hierarchy is fixed at compile time, so the stack is shared
  storage. `Job::next` pops frames and emits `RemoveEmptySource`/`Outcome`
  (`transfer.rs:44`), as the enum does.
- No superstates: no event is handled the same way by several states.
- `Vec` only grows on `Start`/`Listed`.

**lock**. statig has no orthogonal regions, so each region is its own
machine (`Screen` and `delay::Inhibitor`, in separate modules: two
`#[state_machine]`s in one module clash, see §5).

- Dispatch: `Lock::handle` (`lock.rs:333`) gives every event to the lock
  region, then to the delay region. The delay region's context carries the
  lock region's phase after that step, which stands in for the spec's
  guards on the other region. `sleep_pending` releases the delay once the
  lock region is no longer `Locking`, which comes right after
  `LockedHint(true)` or the lockscreen exit, as the spec orders it.
- Lock region: `unlocked`; superstate `lockscreen` (entry `RequestLock` and
  surfaces, exit `ReleaseLock?` then `LockedHint(false)`) over `locking` and
  superstate `locked` (entry `LockedHint(true)`; exit `CancelShake` if
  shaking; state-local `shake`, mutated in place by its `ShakeTick`
  handler) over `idle` and `checking`.
- Delay region: `not_held`; superstate `holding` (entry `TakeDelay`,
  handles `LogindVanished`) over `held` and `sleep_pending`.
- `LockEnded` must not release the lock: the lock region's context carries
  the event (`lock::Cx`), and `leave_lockscreen` emits `ReleaseLock` unless
  it was `LockEnded` (`lock.rs:131`). My first version used a `release` flag
  in `lockscreen`'s state-local data, set by the handlers before leaving;
  the review showed it was avoidable.
- Deviation: `has_text` is `idle`'s own data, not `Lockscreen`'s. Only
  `Locked.Idle` reads or writes it.
- Each *leaf* must declare every field of its superstate chain, used or
  not (the leaf's arm builds `Some(Superstate::X { fields })` from its own
  bindings, `lower.rs:216`). An intermediate superstate needn't take an
  unused parent field as a parameter:
  `#[superstate(local_storage("f: &'sub mut T"))]` declares it (the reviewer
  validated this on the old `release`). With the flag gone, no state here
  has an unused field, and the `#[allow(unused_variables)]`s are gone.

**install**. Superstates `preparing` (`Cancel` → `Interrupt`, an early
`Confirm` → `Interrupt` + fail, `StageFailed`) and `executing` (entry
`StayAwake`, exit `AllowSleep`, `StageFailed` past the irreversible point).

- Each stage's entry action runs it (with `Progress(n)` first for numbered
  steps). Each terminal state's entry action reports the end (`failed` and
  `refused` carry their report as `local_storage`).
- The entry actions are forced where order matters: the `StayAwake` before
  `Progress(1), Run(Erase)`, and the `AllowSleep` before `Complete`/`Failed`
  can only be produced this way. I used them for every stage for
  consistency. That costs 13 named one-line actions: 52 of the 318 lines,
  most of which emitting `Run` from the handlers would save.
- `Prepared { closure }` is emitted by the handler, so `prepared` keeps only
  the route (I6).

## 3. Effects and dispatch

- `IntoStateMachine::Context<'ctx>` is a generic associated type with only a
  lifetime, so the context can't be generic over the sink.
  `crate::Out<'a, E>(&'a mut dyn Sink<E>)` (`lib.rs`) is the context of
  pointer, wrap, transfer and install. Polkit's `Cx` adds the event and
  `again`; the lock region's `lock::Cx` adds the event; the delay region's
  `delay::Cx` adds the lock phase. That is one dynamic call per effect.
- No allocation per event: all state data is `Copy`, and transfer's frame
  `Vec` grows only on `Start`/`Listed`.
- Per-event costs statig adds:
  - leaf → superstate dispatch, building a borrowing `Superstate` value per
    level;
  - two no-op dispatch hooks per level;
  - on a transition, `depth()` and `common_ancestor_depth()` recursion,
    then an exit match and an entry match per level, plus the transition
    hooks.
- Ignored events cost nothing extra (no `Err`).
- Workaround costs: lock dispatches twice per event; polkit dispatches twice
  for a `Begin` that replaces a request (2 of the 94 replay events); wrap's
  `Repeat(n)` is `n` dispatches (impl-enum loops `n` prints too).
- Smoke run after the review fixes (`SME_SMOKE=1`, one pass, noisy host,
  ns/event, both columns from the same run):

  | workload | statig | impl-enum |
  |---|---|---|
  | pointer-stream | 10.92 | 6.80 |
  | wrap-80 | 7.63 | 3.87 |
  | wrap-200 | 8.17 | 3.87 |
  | replay-pointer | 26.35 | 20.62 |
  | replay-wrap | 18.06 | 15.83 |
  | replay-polkit | 41.91 | 11.91 |
  | replay-transfer | 22.15 | 23.06 |
  | replay-lock | 30.98 | 10.30 |
  | replay-install | 21.39 | 10.34 |

  Every row says `same as enum`. (The first run, before the fixes, gave the
  same pattern: replay-polkit 31.91 against 8.62.)
- One quick criterion group (`replay-polkit`, 94 events, before the fixes):
  enum 482.24 ns, statig 1.6364 µs (×3.4).
- Polkit's ×3.4 is the crate's per-transition machinery, not this
  implementation. The reviewer's callgrind run (polkit replay: enum 10,105
  Ir, statig 34,203) attributes 26% of statig's cost to
  `StateExt::transition_path`: it calls `depth()` and
  `common_ancestor_depth()` recursively, rebuilding the borrowed
  `Superstate` values at every level, O(depth²). Superstate bubbling takes
  9% and the exit/entry traversal 9%. Polkit has the deepest hierarchy
  (three levels) and transitions on most events. Removing the `again`
  re-dispatch saves only about 3% (33,180 Ir), and buffering effects in the
  context instead of the `dyn Sink` is slower (34,604 Ir). On wrap
  (×1.83), a buffer costs 11–17% more than `dyn`, so the `dyn` context is
  not a distortion either.
- Sizes in bytes (statig / enum): pointer 32/28, wrap 48/32, polkit 28/28,
  transfer 48/40, lock 24/28, install 32/24.

## 4. Compile-time guarantees

Tally (IMPLEMENTING.md's "Transitions and forging" rule): 7 rejected
(P2–P6, L6, I6), 29 refused at run time, 0 accepted, 0 forge cases, 0 not
applicable. The transition API is `Machine::handle` alone: statig's
handlers, actions and generated constructors are private to the impl block,
so callers can only send events.

Forging means statig offers, and how each is closed for all six targets:

- `UninitializedStateMachine::state_mut` is safe
  (`blocking/state_machine.rs:729`). Set any state before `init`, which then
  runs every entry action down to it: e.g. `erase`'s `Run(Erase)` with no
  confirmation (I1), `restoring` dropped for `probing` (T3), `pending_wrap`
  at column 0 (W1); its `inner_mut` (`:699`, also safe) writes the storage.
  Both need a storage value (the `serde` feature's `Deserialize` would be
  another source; it is off). Every storage type (`Router`,
  `Screen`, `Agent`, `Job`, `Screen`/`delay::Inhibitor`, `Helper`) has
  private (or `pub(super)`) fields and derives neither `Default` nor `Clone`,
  so outside code can't build one.
- `InitializedStateMachine::state_mut`/`inner_mut` are `unsafe` (`:475`,
  `:442`) and out of reach anyway: each wrapper's `machine` field is
  private.
- The generated `State`/`Superstate` enums are public with public variant
  fields, and the `statig::blocking::State` trait methods (`call_handler`,
  `call_entry_action`) are public. But variant literals can only be
  inspected, and the trait methods need `&mut` storage (closed above).
- Contexts: `crate::Out`'s sink field is public, but a context only reaches
  a machine through `handle`.

No forging means stays public, so there is no forge case. The 0 accepts and
the 0 forge cases rest on this implementation's private fields, not on
statig: with a constructible storage type, each target would get a forge
case through the safe `state_mut` (the reviewer reproduced
`*m.state_mut() = State::Q {}; m.init()` running the superstate's and Q's
entry actions). Rank statig's own guarantee as the 7 data-shape rejections.
Re-checked every case against the rule: the 7 rejects inspect variant
shapes only, and all 29 runtime cases drive `handle`.

- What it rejects: state-local data shape only. Each rejection is a field
  missing from (or required by) a generated `State` variant: `NativeDrag`
  has no `owner`, `Locking` no `shake`, `Prepared` no `closure`.
- Why the rest are runtime: there is no transition table. Any handler may
  return `Transition(any State)`, actions may push any effect, and the
  order of effects is the handler's; from outside, only `handle`'s
  behaviour refuses them.
- L6's rejection is data shape only: `State::Locking` has no `shake`, but a
  `locking` handler may still push `ArmShake`.
- Typical error, L6; it points at the real mistake:

  ```text
  error[E0559]: variant `impl_statig::lock::State::Locking` has no field named `shake`
    |     let _ = State::Locking { shake: 1 };
    |                              ^^^^^ `impl_statig::lock::State::Locking` does not have this field
  ```

- Note: with several `State` enums in the crate, rustc now prints full
  paths. That is why pointer's P5/P6 `.stderr` were regenerated, with the
  same errors.

## 5. Errors while writing it

I hit few errors. Reading `codegen.rs` first showed the pitfalls, and I
provoked the following in a throwaway example to see the messages:

- A leaf without a field its superstate takes. This is the worst one: it
  names the superstate's parameter, never the offending leaf, and suggests
  renaming it to the storage type:

  ```text
  error[E0425]: cannot find value `n` in this scope
  7 |     fn sup(n: &mut u8) -> Outcome<State> { let _ = n; Handled }
    |            ^
  help: a unit struct with a similar name exists
  7 +     fn sup(M: &mut u8) -> Outcome<State> { let _ = n; Handled }
  ```

  An action input that no state has gives the same E0425 on the action.
- `state(name = "AState")` with `initial = "State::x()"` gives "expected a
  type, found a trait" and suggests `<dyn "State::x()">`. The prelude's glob
  import brings in the `State` trait.
- Two `#[state_machine]`s in one module give E0252 "the name `state` is
  defined multiple times", because each expansion emits
  `use statig::{state, superstate, action};` (`codegen.rs:21`). Hence lock's
  `delay` submodule.
- A misspelled superstate or action name is reported well: "superstate not
  found", pointing at the string.
- My own one real mistake, constructing a type alias as a tuple struct
  (E0423), took a minute.

## 6. Ergonomics

- Concision: 1.37× impl-enum's lines (1596 against 1165). The cost is the
  attribute strings and one named method per entry or exit action (there
  are no inline actions), plus the repeated superstate fields in every
  leaf's signature.
- Readability: good. A state is a plain function with a `match`, and
  `Super` reads as "ask the parent".
- What a maintainer must learn:
  - handler parameters bind by name (`event` and `context` are reserved;
    other names become state-local fields; the same name in a superstate
    borrows the leaf's);
  - the ordering: handler effects, then exits bottom-up, then entries
    top-down;
  - entry and exit actions see neither the event nor the other end of the
    transition, unless the event is routed through the context (as polkit
    and lock do here);
  - a transition whose target shares a superstate never re-enters it;
  - two machines can't share a module.
- Debugging: `state()` returns the generated enum (derive `Debug`), and
  hooks can log dispatches (mind the swapped `before_transition`).
  Stepping works: the generated code is matches calling your methods.
- Regions, self-posted events and timers are not in the crate. Timers are
  effects here (`ArmRetry`, `ArmShake`), so nothing of statig's was needed
  for them.

## 7. Build

- statig 0.4.1 and statig_macro 0.4.0 are edition 2021 with MSRV 1.66, and
  work unchanged from edition 2024 code (let-chains in handlers).
- Every build prints cargo's future-incompat warning for
  `proc-macro-error2` 2.0.1.
- Generated impls carry `#[allow(unused)]`, but a leaf that must declare a
  superstate field it doesn't use warns (`unused_variables` on the handler
  parameter). None is left after the review fixes.
- Debug build with dependencies warm (syn already built): statig_macro +
  statig + impl-statig in 0.80 s; impl-statig alone 0.30 s. Phase 3
  measures properly.

## 8. Spec questions

- lock `OutputAdded(o)`: "if `o` is new: add it; then `OpenSurface(o)` if in
  `Lockscreen`". I read the surface as conditional on the output being new
  too, as impl-enum does; traces and the differential agree. The same goes
  for `OutputRemoved`.
- polkit `ChooseIdentity(i)`: the spec says `start(i)`. I also store `i` as
  the request's identity, so a later `RetryTimer` or `TryAgain` restart uses
  it, as impl-enum does.
- install `Cache` data: the spec lists `root_mib` and `closure_mib`. I keep
  only the closure: nothing reads the root after `Build`'s check.
- lock `has_text` on `Lockscreen` and transfer's `Placing` `name`: modelling
  choices (§2), not ambiguities.

## 9. Verdict

statig is a competent, small, allocation-free runtime hierarchical state
machine. Its superstates with borrowed state-local data and its
entry/exit ordering paid off in pointer, polkit, install and the lock
screen. The ordering of `Reply`/`CloseDialog`, `StayAwake`/`AllowSleep` and
`LockedHint`/`ReleaseDelay` falls out of the hierarchy instead of being
hand-sequenced.

It fought me in four places:

- no orthogonal regions (lock is two machines and a coordinator);
- no run-to-completion re-dispatch (polkit's `again`, wrap's `Repeat`,
  pointer's duplicated entry);
- exit and entry actions blind to the transition. Routing the event
  through the context handles that; my first version used flags, which the
  review showed were avoidable;
- discriminant-only least-common-ancestor computation (no external
  re-entry of a superstate).

It adds no compile-time protocol guarantees beyond data locality (7 of 36
probes rejected, all data shape). It costs about 1.5–2× impl-enum per
event on the hot workloads and about 3.4× on polkit's replay. The 3.4×
comes from its transition path computation and hierarchy traversal (§3),
not from the workarounds.

I'd choose it for Sekka's protocol-like, event-driven machines with nested
"while held" resources and few events per second: the polkit agent and the
install helper. I would not choose it for per-cell or per-pointer-event hot
paths (wrap, pointer), or where the point is to reject illegal operations at
compile time (typestate wins there).

Maintenance risk is moderate. What I could observe offline:

- 0.4.1 still has the swapped `before_transition` arguments (reproduced),
  so no release carries a fix;
- every build prints cargo's future-incompat warning for `proc-macro-error2`
  2.0.1;
- it uses `unsafe` internally (`superstate.rs:51-52`).

The survey adds a single maintainer, an unreleased upstream fix for the
hook and an advisory calling `proc-macro-error2` unmaintained; I could not
check those offline, and the advisory I know of (RUSTSEC-2024-0370) is for
`proc-macro-error` 1.x, not `proc-macro-error2`.
