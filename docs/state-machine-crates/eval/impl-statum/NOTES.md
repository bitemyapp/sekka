# impl-statum: notes

## 1. Crate

statum 0.9.0 (`statum-core` and `statum-macros` 0.9.0), default features
(no `introspection`, no `#[validators]`). No reserve substitution, no
`Cargo.lock` change. All six targets are implemented; `NOT_EXPRESSED` is
empty and nothing is ignored.

statum generates a *typed* API (`Machine<State>`, one method surface per
state), not an event dispatcher. Each target is a typestate API in statum's
idiom: `#[state]` enum, `#[machine]` struct, one `#[transition]` block per
source state. Each target's `Machine::handle` matches the generated
`<machine>::SomeState` per event, the same shape as impl-typestate's `Any`.
The designs follow impl-typestate method for method, so the probes compare
like with like.

Seals shared by every target. statum generates forging means that are
public by default; I hid every one it lets an implementation hide:

- **Builders.** `#[machine]` generates a public builder for every state
  (`Router::<Pressed>::builder()`), with the machine's own visibility
  (`statum-macros/src/machine/emission/builders.rs:34`). Each machine has
  a `seal: Seal` field (`src/lib.rs:26-33`): `Seal` is public, but only
  this crate can construct it, so no builder can be finished outside:
  `error[E0599]: no method named build found for struct HelperPreparedBuilder<_, _>`.
- **Data payloads.** Each data state is a tuple variant holding its own
  hand-written payload struct with private fields, neither `Copy` nor
  `Clone` (`Pressed(PressedData)`, `Prepared(PreparedData)`, …), plus
  getters where `handle` or the cases read one (`PressedData::owner`,
  `CheckingData::check`, `item()`, `conversation()`). Outside code can't
  build a payload for `transition_with_data` (`E0451: field deferred of
  struct PreparedData is private`), and it can't write a payload's fields.
- **Unit states get tokens.** Each state without data holds a token only
  its module makes (`tokens!`, `src/lib.rs:6-17`; `Outside(OutsideTok)`,
  `Copy(CopyTok)`, …). This turns the otherwise unconditional
  `CanTransitionTo<S>` into `CanTransitionWith<STok>`.
- **Not hideable.** The `pub state_data` field
  (`statum-macros/src/machine/emission/support.rs:328`) and the
  `CanTransition*` / `transition_map` impls themselves. See §4 for what
  stays open.

Sealing cost 143 lines (2185 → 2328), mostly payload structs and token
lists.

## 2. Per target

LoC from `metrics/sme.py loc`, sealed (statum / typestate): pointer 418 /
409, wrap 390 / 383, polkit 368 / 341, transfer 361 / 304, lock 363 / 313,
install 413 / 383, shared 15 / 6, total **2328 / 2139** (unsealed: 2185).
Conformance passes on every trace (21/21, 14/14, 16/16, 20/20, 18/18, 16/16). The differential tests pass
with 256 and with 5000 cases.

- **pointer** (`src/pointer.rs`): six leaves with sealed payloads
  (`Hovering(HoveringData)`, `Pressed(PressedData)`, …, and tokens for
  `Outside` and `NativeDrag`).
  `Router { threshold2, seal }` is the machine. statum has no superstates, so
  every inside leaf carries `hover`, and `inside!` (`:152`) generates
  `Inside`'s shared exits (`leave`, `start_drag`) as one `#[transition]`
  block per leaf. A press from `Hovering` has three outcomes; `Branch` takes
  two and can't be nested, so `press` is a plain method returning a
  hand-written `Pressing` enum (`:170`) that composes declared edges
  (`capture`, a private `press_edge`). An edge press moving returns
  `::statum::Branch<EdgePressed, EdgeDragging>`.
- **wrap** (`src/wrap.rs`): four token states; the cursor and links are the
  `Screen` machine's fields. The shared cursor logic is a generic plain
  `impl<S: WrapStateTrait> Screen<S>`. `Ready`/`ReadyNoWrap` are in-module
  aliases of `::statum::Branch` (`:72`), which statum follows. This is the
  closest fit of the six: no data, two-way branches.
- **polkit** (`src/polkit.rs`): sealed payloads carry `Request` and a
  `Conversation` token with a private field (`conversing_data!`). `Session { config, last_conv,
  seal }` is the machine. `conversing!` (`:154`) puts the three conversing
  states' shared edges (`end`, `succeed`, `choose`) into *the same block* as
  each state's own edges, with the own methods passed in as tokens. A
  separate block would fail: `Checking.refused` and `Checking.choose` both
  target `Starting`, and statum rejects one edge declared in two blocks
  (E0119; verified with a macro block plus a state's own block).
  `back_off` (`:107`) is generic over the public
  `statum::CanTransitionWith<RetryWaitData/UnavailableData>` bounds, so
  `Starting` and `Prompted` share it. Non-`Copy` payloads (the token) move
  with `transition_map(|data| …)`.
- **transfer** (`src/transfer.rs`): each state has its own payload type
  (`item_data!`). `Probing(ItemId)`, `Placing(ItemId)`, … would collide
  (E0119), because statum keys `transition_with` by payload type, not by
  target state.
  `Job { mode, delete_replaced, frames, seal }` is the machine (no `kind`).
  "next" (`:101`), "done" and "cancel" are generic functions bounded on
  `CanTransitionWith<ProbingData> + CanTransitionWith<FinishedTok>`; each state
  declares those edges by returning `Next = ::statum::Branch<Probing,
  Finished>`. A probe has four or five outcomes, so it returns a hand-written
  `Probed` enum (`:170`) built from private edge methods. The merge frames are
  a machine field (a `Vec`, as in the baselines); statum has no nested
  machines.
- **lock** (`src/lock.rs`): statum has no orthogonal regions, so there are
  two machines, `LockScreen` (outputs, last check, motion) and `Inhibitor`
  (`NotHeld`, `Held`, `SleepPending`). L1 follows impl-typestate: once held
  for a pending sleep the delay is `Inhibitor<SleepPending>`, whose `release`
  is private (`src/lock.rs`, the `SleepPending` block). Only `settle`
  consumes it, called from `confirm`, `leave` and `accepted`. A transition
  can't return a tuple, so
  the cross-region transitions (`confirm`, `leave`, `accepted`) take
  `&mut inhibitor::SomeState` and settle it in place (`settle`, `:91`). The
  hierarchy `Lockscreen ⊃ Locked ⊃ Checking` is flattened into variants, and
  entry/exit actions are written out in the transitions, as in
  impl-typestate.
- **install** (`src/install.rs`): 20 states (terminal states included), the
  reserve as the machine field. `preparing!` and `executing!` stand in for
  the two superstates. `Route` (three ways) is hand-written, and `Step4` is a
  `Branch` alias.

## 3. Effects and dispatch

`out: &mut impl Sink<Effect>` is passed straight into the transitions:
statum constrains only the receiver and the return type, so they can be
generic. There is no buffer, no `dyn` and no allocation (transfer's frame
`Vec` is the baselines' too). statum has no event model: unhandled
(state, event) pairs fall through `(s, _) => s` and produce no `Err`.

The runtime boundary, compared with impl-typestate:

- **Moves:** both move the whole state out of `self` and back in on every
  event. statum also moves the machine's shared fields, which live inside
  every `SomeState` variant (typestate keeps pointer's threshold outside
  `Any`).
- **Placeholder:** typestate uses a constant placeholder for the
  `mem::replace`. statum has no documented way to make one: the builder
  isn't `const`, and a struct literal depends on the undocumented generated
  `marker` field. My first version built a placeholder with the builder on
  every event. Its non-generic, non-`#[inline]` setters made wrap cost
  13.1–15.9 ns/event against typestate's 10.8–11.6 in smoke. A `const`
  literal with `marker` brought it to 10.3–10.9, which confirmed the cause.
  The shipped version holds `Option<SomeState>` and uses
  `take().expect(..)` (documented API, one branch; the niche keeps the
  size): 10.1–11.1.
- **Size:** `bench --bin sizes` gives statum / typestate: pointer 32/32, wrap
  64/64, polkit 28/28, transfer 48/48, lock 24/32, install 24/24.
- **Speed:** final smoke (one pass, this machine is noisy):
  - Sealed code, statum vs typestate: pointer-stream 8.35 vs 10.49
    ns/event; wrap-80 10.45 vs 11.17; wrap-200 10.27 vs 10.88; replay-lock
    13.11 vs 8.03; replay-install 7.38 vs 7.46. The sizes are unchanged by
    sealing: the tokens are zero-sized.
  - Every row says `same as enum`.
  - Short criterion runs while diagnosing (statum / typestate):
    replay-lock 889.27 / 508.65 ns, replay-polkit 958.24 / 769.45 ns,
    replay-install 331.72 / 492.10 ns.
  - I didn't find lock's gap. Building the delay placeholder with a literal
    instead of the builder only recovered 7%. After review, `handle` holds
    the delay region as an `Option` too. `settle` keeps a placeholder: it
    works through the typed transitions' `&mut inhibitor::SomeState`.
    Re-measured (sealed code; both rows about 7% slower on this run):
    replay-lock 949.28 / 561.50 ns. The gap remains, so phase 3 should
    look at it.

## 4. Compile-time guarantees

Tally under "Transitions and forging" (36 probes): **32 reject, 4 runtime,
0 accept, 6 forge cases** (typestate: 32 / 4 / 0). The per-probe classes
are typestate's:

| target | reject | runtime | forge |
|---|---|---|---|
| pointer | P1–P6 | | `F-transition-with` (P5) |
| wrap | W1–W5 | | `F-transition-with` (W1) |
| polkit | K1, K3–K6 | K2 | `F-transition-with` (K3) |
| transfer | T1–T4, T6 | T5 | `F-transition-with` (T3) |
| lock | L1, L3, L4, L6, L7 | L2, L5 | `F-transition-with` (L3) |
| install | I1–I6 | | `F-transition-with` (I4) |

Changes from the first version:
- The 12 former accepts are rejects against the transition API, for
  example `Session::<Prompted>::answer(starting, ..)`,
  `Job::<SettingAside>::set_aside(asking, ..)`, `Route::Deferred(evaluation)`
  and `locking.submit(..)`.
- **L1** is `reject` as the coordinator decided: `SleepPending` is a state
  of its own with a private `release`, so
  `Inhibitor::<Held>::release(pending, ..)` is E0308.
- **L4** moved from runtime to `reject`, as impl-typestate's current L4 did:
  `resumed` is the Resumed event and consumes the `SleepPending`, so keeping
  it is E0382 "use of moved value". The review expected L4 to stay runtime;
  I followed the baseline's newer reading.

**Hiding.** For every edge a `#[transition]` method's return type declares,
statum emits a public `impl statum::CanTransitionWith<NextData>` and
`CanTransitionMap<Next>` (data targets), or `CanTransitionTo<Next>` (unit
targets), plus the machine's `pub fn transition_map`
(`transition/emit.rs:80-181`, `machine/emission/support.rs:333`). This
happens even for private transition methods, for example install's private
`deferred` and transfer's `ask`. They take the edge without the method's
body. No statum option turns them off: statum treats a declared edge as
legal for whoever can name the machine. The review quotes
`docs/generated-builder-reference.md` "Visibility" for this; `docs/` isn't
in the published crate, so I checked the code instead.

What the seals (§1) close:
- **Fresh construction is closed:** sealed builders, payloads and tokens.
- **Data writes are closed:** private payload fields.
- **One-call forging is closed:** a caller can't make the data for
  `transition_with_data`, or return it from a `transition_map` closure,
  for any edge.

**What stays open: harvesting.** The review and the coordinator expected
sealed payloads to "close every edge into a data state" and polkit and lock
to keep no forge case. That is wrong, and I checked it against the crate
source:

- `state_data` is always a `pub` field
  (`machine/emission/support.rs:328`), and the machine has no `Drop` impl.
  So a caller can **move** a sealed payload or token out of a second
  machine it drove legitimately into the target state, then hand it to
  `CanTransitionWith` or `transition_map` on the first machine. Non-`Copy`
  and non-`Clone` don't prevent a move.
- I can't block the move: adding `impl Drop for Helper<S>` breaks statum's
  own generated transitions, which move fields out of `self` (`E0509:
  cannot move out of type Helper<Idle>, which implements the Drop trait`,
  reported at `#[transition]`).

So every declared edge stays forgeable, at the price of a donor machine. One
forge case per target shows it, each moving a donor's payload:
- **pointer:** P5, an `OutsideTok` from a fresh machine, leaving the hover on.
- **wrap:** W1, a `PendingTok`.
- **polkit:** K3, a `RetryWaitData` from a session that failed, which
  restarts without back-off, ten times.
- **transfer:** T3, a `FinishedTok` from an empty job, which drops the aside.
- **lock:** L3, a `LockedData` from a confirmed screen, which starts a check
  while `Locking`.
- **install:** I4, a `PreparedData` from a helper whose 400 GiB root passed,
  used on a 39 GiB root.

The other means (`transition_map`, `CanTransitionMap`, and the target's
other probes the same way) are named in each case's comment.

Typical error, which points at the real mistake:

```
error[E0609]: no field `owner` on type `NativeDragTok`
  |     let _ = dragging.state_data.owner;
```

`no method named retry found for struct Session<Starting>` comes with
`note: the method was found for Session<RetryWait>`, which helps. When a
method exists on no state, rustc prints the type as `Job<TransferState>`:
statum requires the generic parameter to be named after the state enum, so
it reads like a concrete type.

## 5. Errors while writing it

- **Shared payload types collide** (about 10 minutes, reading
  `transition/emit.rs`). Two targets of one state with the same payload type,
  even a self-loop beside another target, give an unexplained E0119 at the
  attribute:
  ```
  error[E0119]: conflicting implementations of trait `TransitionWith<Hover>` for type `exp::Router<exp::Hovering>`
    --> impl-statum/src/exp.rs:35:1
  35 | #[transition]
     | ^^^^^^^^^^^^^
     | |
     | first implementation here
     | conflicting implementation for `exp::Router<exp::Hovering>`
  ```
  The fix is a payload type per state (first named variants, which generate
  `XData`; now hand-written sealed structs and tokens). The same E0119 appears when one edge is declared in two
  `#[transition]` blocks, for example a shared-edge macro block plus the
  state's own.
- **Wrapper paths must be absolute** (about 5 minutes). Bare `Result<…>` and
  `statum::Branch<…>` are refused with a message whose fix is wrong, and
  whose `Found:` line drops the method's parameters:
  `Found: fn r(self) -> Result<Router<Native>, Router<Outside>>` …
  `Fix: return Router<NextState> directly.` The real fix is
  `::core::result::Result` / `::statum::Branch`. Nested `Branch` is
  refused the same way.
- **`self.transition()` needs an annotation.** It is generic over the
  target, so `self.transition().print(..)` is E0034 "multiple applicable
  items". Write `Screen::<Normal>::print(self.transition(), ..)`.
- **Generic source states are refused:**
  `source state S in #[transition] target Router<S> is not a variant`.
  Shared edges need `macro_rules!`, which works.

## 6. Ergonomics

LoC is 9% above hand-written typestate once sealed (2328 / 2139; 2% unsealed).
statum removes the per-state type definitions, but sealing puts payload
structs and tokens back, and it adds the `#[transition]` blocks, absolute
wrapper paths, `SomeState` wrapping (no `From` impls are generated) and
placeholder handling. A maintainer has to learn rules that appear in no type signature:

1. Every edge to a given target is declared in one block per source state.
2. No two targets of one state may share a payload type.
3. Only `::core::result::Result`, `::core::option::Option` and two-way
   `::statum::Branch` are accepted, or in-module aliases of them. No
   nesting, tuples or custom enums.
4. Non-`Copy` payloads need `transition_map`.
5. The builders are public (seal them).
6. Every declared edge, even from a private method, can be taken without
   its body: seal payloads and tokens, and accept that a donor machine still
   reopens each edge.

Debugging: `state()` and `SomeState::is_*()` show the state, and
`#[derive(Debug)]` below `#[state]`/`#[machine]` works (`SomeState` itself
has no derives). Transition bodies are ordinary methods you can step
through; `transition()` and `transition_with()` step into
macro-generated impls.

## 7. Build

- MSRV 1.93; statum is edition 2021. No warnings in this crate.
- `statum-macros` is 13,258 lines on syn 2 (`full`, `visit-mut`). It enables
  `proc-macro2`'s `span-locations` feature, which unifies onto every
  proc macro in the build.
- The macros **read the source files from disk**
  (`Span::local_file()` plus `fs::read_to_string`, `source/parser/mod.rs:45`)
  to find the `#[state]`/`#[machine]` items and type aliases. They keep a
  process-global registry (`static LOADED_MACHINES`,
  `machine/registry.rs:6`) and emit a different builder under rust-analyzer
  (`RUST_ANALYZER_INTERNALS`). For rustc this is benign: the files read are
  the crate's own module files, and proc macros re-run with the crate. The
  real risks are these:
  - rust-analyzer sees different generated code;
  - the long-lived RA proc-macro server relies on an mtime/length cache
    (`source/syntax.rs:70-82`);
  - items that aren't on disk degrade (`include!`, other macros' output;
    `macro_rules!` blocks work);
  - under `introspection`, the hashed file path becomes symbol names.
- A clean build of statum, statum-core, statum-macros and impl-statum (syn
  already built) took 2.09 s (dev).
- The README's install snippet still says `0.8.10`.

**Survey corrections:**

- "Strongest compile-time guarantees": the typed API is exactly as strong
  as hand-written typestate (32 / 4 / 0, the same probe classes). But statum
  also generates public forging means: builders, `CanTransition*` /
  `transition_map` (even for private methods) and `pub state_data`.
  Builders, fresh payloads and one-call unit edges can be sealed by hand;
  moving a payload out of a donor machine can't be prevented.
- "Branching: returns are an enum or `Result`": custom enums are refused.
  Only fully qualified `Result`/`Option`/two-way `Branch` work, unnested.
- `SomeState` is generated by `#[machine]` and doesn't need `#[validators]`.
  There's no conversion into it.
- "40–60 lines for the drag machine": pointer is 390.
- Not mentioned in the survey:
  - payload-type collisions (E0119);
  - one block per edge;
  - no generic source states;
  - no tuple returns, which matter for orthogonal regions;
  - source-file reading.
- Confirmed: MSRV, edition, macro-crate size, no hierarchy, entry/exit or
  timers, `linkme` only with `introspection`.

## 8. Spec questions

None beyond the baselines'. I followed impl-enum's readings (for example
`Begin` with `identities == 0` while a request is open: reply false, the
request goes on).

## 9. Verdict

statum's typed API is as strong as hand-written typestate: the same 32 / 4
/ 0 probe classes, with the typestate's shape. It is as fast on the hot
paths. replay-lock and replay-polkit are slower and unexplained; replay-install
is faster. It costs 9% more lines once sealed.

The difference is forging. Every declared edge gets a public, guard-free
`CanTransition*` / `transition_map`, even from a private method, and no
switch turns them off. Hand sealing (builders, private payloads, tokens)
removes one-call and fresh forging. Because `state_data` is public, a caller
can still move a payload out of a legitimately driven donor machine, and a
`Drop` impl to stop that breaks statum's own code. Hand-written typestate
has no such hole.

So the edge-skipping finding is narrower than my first verdict claimed. It
takes deliberate, two-machine effort. A reviewer would see it as plainly as
a `mem::transmute`. And it can't be committed by an ordinary event loop
through `handle`.

For Sekka I would still choose hand-written typestate, or the enum
baseline, over statum. statum adds source-reading proc macros, rules you
learn by hitting E0119, and per-state sealing boilerplate. It buys less than
nothing in guarantees and roughly nothing in concision. The churn adds risk
(0.8.x to 0.9 in months, a "Lite" rewrite proposed).

Where it could fit: a persisted workflow record rebuilt from storage
(`#[validators]`, unused here) inside one module, whose typed API never
leaves it. It doesn't fit event-driven GUI machines (pointer, wrap, lock).
