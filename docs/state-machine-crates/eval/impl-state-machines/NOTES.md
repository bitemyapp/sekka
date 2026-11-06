# impl-state-machines: notes

## 1. Crate

**state-machines 0.34.0**, with state-machines-macro 0.34.0 and
state-machines-core 0.14.0. It is pinned and built with no features: each
machine says `dynamic: true`, and I don't use `async`, `runtime`, `serde`
or `inspect`. No reserve crate was needed: the dev shell's rustc is 1.99.0
(b940084d7 2026-09-28), and all three crates declare
`rust-version = "1.99"`.

One `state_machine!` generates:

- a marker type per state and superstate;
- the typestate machine `Name<S>`, with consuming transition methods. Each
  returns `Result<Name<Target>, (Self, GuardError)>`, or, when an event has
  several candidates from one state, a generated
  `Name<Source><Event>Outcome` enum;
- with `dynamic: true`, the runtime wrapper `DynamicName`:
  `handle(NameEvent) -> Result<(), DynamicError>` and
  `current_state() -> NameState`.

`Machine::handle` maps the spec event to a `NameEvent`, calls the wrapper,
ignores its `Err`, and then drains the context's effect buffer (§3).

Each target puts its macro, callbacks and facade in a private
`mod machine` and re-exports the typed API: the machine, the state types,
every outcome enum, `Ctx` and the facade. It does not re-export
`DynamicName`, `NameEvent` or `NameState` (see "Hiding" in §4).

### Can the typestate and dynamic machines drift apart?

No. `DynamicName` isn't a second transition engine. It holds
`Option<AnyNameState>`, an enum of the typed machines, and
`__sm_handle_one` matches `(state, event)` to call that state's typed
transition method, or `__sm_select_<event>` for an event with several
candidates (macro `codegen/dynamic.rs:386-396`). So guards, callbacks,
hooks and factories run in the same generated code.

What the wrapper adds is bookkeeping: an epoch, a visit counter per state
and superstate, and a completion queue. It also offers `stabilize` for
automatic edges, which I don't use.

The one difference is in what can be reached.
`new_init_state(ctx, NameState::X)` builds any state, with its data absent
and no entry hooks run, and `into_x()` turns that into a typed `Name<X>`.
I keep that type unexported (§4).

### The survey against the source

**Correct:**

- MSRV 1.99, edition 2024, syn 3.
- The typestate signatures.
- `Err(InvalidTransition)` in dynamic mode.

**Imprecise: "dynamic-mode state data may be None unless a `data:`
factory is declared."**

- Data is `None` after construction (`new`, `new_init_state`) or a
  restore, in either mode, whatever factories exist (`typestate.rs:285-301`,
  `dynamic.rs:295-305`).
- Any transition into a data state sets it: to `Default::default()`, or to
  the factory's value (`typestate.rs:373-404, 464-485`).
- The typed per-state accessors return `Option` too
  (`typestate.rs:1061-1070`). The README's "no Option wrapper!" (README:577)
  is out of date.

**Wrong in the README:**

- *"Callbacks take `&self`, not `&mut self`" (README:1327).* Transition
  methods take `mut self` and call `self.cb(..)` or `new_machine.cb(..)`
  (`typestate.rs:826, 639, 851`). So before/after callbacks, lifecycle hooks
  and `data:` factories may take `&mut self`, and every target here does.
  Guards are called as `self.g(&self.ctx, &payload)` (`typestate.rs:600`),
  so they take `&self`.
- *"Under the Hood".* The `impl<C, S: SubstateOf<Flight>>` transition it
  shows isn't generated. A superstate source is expanded into one inherent
  method per leaf (`typestate.rs:1105-1110`). `SubstateOf` impls are
  generated, but nothing uses them.

**Needs the `runtime` feature:**

- *"Orthogonal regions: yes".* They are available only through `runtime`
  (`regions.rs:538`, a `compile_error!` without it). Its `regions:`
  composition and `runtime::Parallel` dispatch through `pub async fn handle`
  (`regions.rs:391`, `runtime/parallel.rs:139`), and they have no
  cross-region guards. There's no async runtime here, so `lock` composes two
  machines by hand.
- *"Timers: state-scoped deadlines".* These need `runtime` too, plus a
  `Runner`, a host `Clock` and an async `drain`. They would cancel on exit,
  matching CancelRetry and CancelShake, but they emit no effects. Not used.

**Dates I couldn't check:** "23 versions, 0.9 → 0.34 within 2026" comes from
the survey. The count of 23 published versions (0.1.0 to 0.34.0) agrees
with the index cache, but the dates aren't there.

**Not in the survey:**

- There is no **public** context accessor. `ctx` is a private field of the
  typed machine (`typestate.rs:161`), the wrapper's `inner` is private
  (`dynamic.rs:928`), and `AnyNameState` is a private enum
  (`dynamic.rs:136`).
- Because the macro expands in my module, module privacy lets code there
  reach `machine.inner` and `m.ctx`. The callbacks and the facades rely on
  this (§3).
- `initialize()` is generated on every state type, not only the initial
  one (`typestate.rs:189, 333`), and it re-runs the active entry hooks.
  - `Locker<Idle>::initialize()` re-emits RequestLock, OpenSurface(0) and
    LockedHint(true).
  - `Installer<Erase>::initialize()` re-emits StayAwake, Progress(1) and
    Run(Erase).

  I checked both with a throwaway test.
- The macro's generic parameter names aren't hygienic: `ctx_generics`
  emits a bare `<C>` (`utils.rs:98`).

## 2. Per target

All traces pass:

| target | traces |
|---|---|
| pointer | 21/21 |
| wrap | 14/14 |
| polkit | 16/16 |
| transfer | 20/20 |
| lock | 18/18 |
| install | 16/16 |

All six differential tests pass, at 256 cases (debug) and at 5000 cases
(release). `NOT_EXPRESSED` is empty, and nothing is ignored.

### pointer (`src/pointer.rs`)

**Model** (`pointer.rs:100`): `Outside`, `NativeDrag`, and the superstate
`Inside(Hover)` {`Hovering`, `Pressed(Node)`, `DragsOnlyPressed(EdgePress)`,
`DragsOnlyDragging(Node)`}.

- `Inside`'s exit hook, `clear_hover` (`pointer.rs:116`), emits UnderOff
  and HoverOff on Leave and StartDrag. The generated code resets `Inside`'s
  data on entry and carries it between substates.
- Leaf data comes from `data:` factories (`edge_press`, `press_target`,
  `captured`, `keep_edge`), so `Node` and `EdgePress` need no `Default`.
- `moved` and `removed` use `hierarchical: true` (the child first, then the
  `from: Inside` row). `press` uses `branching: true` for the three-way
  choice.

**What the crate made easy:** the spec's effect order.

- Moves and presses from `Outside` act in `after` callbacks, because
  `Inside`'s data exists only in the new machine.
- A release from an owner state acts in `before`, because the transition
  clears the owner.

So "track before delivering, except on a release" falls out of the
before/after split.

**Workarounds:** none.

### wrap (`src/wrap.rs`)

**Model:** the superstates `Autowrap` {`Normal`, `PendingWrap`} and
`NoAutowrap` {`NormalNoAutowrap`, `PendingNoAutowrap`}. The context holds
the cursor, the saved cursor, REP's memory and the links.

- A trait `Mode` on the generated state types (`wrap.rs:209`) lets one
  `print_cell` and one `ends_pending` serve all four states.
- Guards run before callbacks, so `ends_pending` predicts what `print_cell`
  will do: the logic is duplicated.

**Workarounds:**

- **REP:** `handle` dispatches `print(None)` up to n times
  (`wrap.rs:358`), because each printed cell may change the state.
- **SetAutowrap:** it keeps the pending wrap through four explicit
  transitions. Shallow history remembers the target region's last child,
  not the source's.

### polkit (`src/polkit.rs`)

**Model:** `Idle`, and the superstate `Active(Request)` {`Starting(u32)`,
`Prompted(u32)`, `Checking(u32)`, `RetryWait`, `Unavailable`}. `Active`'s
exit hook is `close_dialog`.

- Conversation numbers come from factories: starting the helper is what
  yields the number.
- A Begin for another cookie is `from: Active, to: Starting, kind: external`
  (`polkit.rs:109`).
- `current` is an event-level guard, and back-off is `branching`. A marker
  trait `Phase` (`polkit.rs:176`) gives `stop` its rule for CancelRetry.

**Workarounds:**

1. **No factory for superstate data.** It can only be `Default`-initialized
   on entry (`validation.rs:275-285`). So `Request` has a placeholder
   `Default` (cookie 0), which `take_request` (an `after` callback)
   overwrites.
2. **OpenDialog can't be an entry hook.** Hooks get no payload and run in a
   fixed order, so the `open_and_start` factory (`polkit.rs:223`) emits
   OpenDialog and then StartHelper.
3. **EndHelper and CancelRetry are transition callbacks.** The spec's exits
   are conditional on the edge, and exit hooks can't see the edge.

### transfer (`src/transfer.rs`)

**Model:** `Idle`, the superstate `Running(Frames)`, and `Finished`.

- The frame stack is `Running`'s data, so a cancel drops it by leaving
  `Running`.
- **next** is the `next_item` factory, and every "item done" is a pair of
  candidates: `[has_next] → Probing`, or `fallback → Finished`.
- Merges push frames, because `invoke_child` needs the runtime and async.

**`Asking` is split by the conflict's `can_merge`** (`transfer.rs:130`):
`superstate Asking {AskingMergeable(ItemId), AskingUnmergeable(ItemId)}`.
`probed` branches into one leaf or the other, and only `AskingMergeable`
has the `Merge → Listing` edge (`transfer.rs:171`). This is what makes T6 a
reject.

- It cost about 4 DSL lines (two leaves, one more `probed` candidate) and an
  `is_mergeable` guard.
- It saved the `Asked` struct, its factory and the `merges` guard.

**Workaround:** `Start` from `Idle`. The factory probes item 0, and an
`after` callback pushes the top frame at `next: 1` (`transfer.rs:359`),
because `Running`'s data exists only in the new machine.

### lock (`src/lock.rs`)

**Model:** two machines.

- `Locker`: `Unlocked`, and the superstate `Lockscreen(Field)` {`Locking`,
  superstate `Locked(Shake)` {`Idle`, `Checking(u32)`}}, with lifecycle
  hooks.
- `Delay`: `NotHeld`, `Held`, `SleepPending`.

**Composition:** `Lock::handle` (`lock.rs:423`) routes each event through
`dispatch_locker` and `dispatch_delay` (`lock.rs:401, 409`). Each one
dispatches and then drains its own machine's buffer, so effects come out in
call order.

- The cross-region guard becomes `sleep(LockPhase)`'s payload.
- "Lock confirmed, or lock screen left" is relayed as `lock_settled` after a
  `Locker` transition that left `Locking` or `Lockscreen`
  (`lock.rs:443-451`).

**Workaround (effect order):** CancelShake and ReleaseLock are `before`
callbacks, and only LockedHint(false) is an exit hook. Exit hooks run after
the `before` callbacks and can't see the edge.

### install (`src/install.rs`)

**Model:** the superstates `Preparing` and `Executing`, with
`Executing {enter: stay_awake, exit: allow_sleep}`.

- One generic `run` is the entry hook of every stage, with the stage and
  step number taken from a `Step` trait on the state types
  (`install.rs:179`).
- StageFailed is `hierarchical`, so `Record`'s warning overrides
  `Executing`'s failure.

**Friction:**

- `step_done` must be `branching`, so 8 of its 9 candidates say
  `fallback: true` (`install.rs:155`).
- The state named `Copy` shadows `core::marker::Copy` in the module.

**An unused option:** `final_states` could declare install's four terminal
states and transfer's `Finished`. The macro would then reject outgoing
edges from them. But each successful `handle` into one would also push a
`CompletionEvent` into the wrapper's `Vec`: an allocation per machine, and a
queue that grows unless something drains it (`scopes.rs`,
`__sm_record_completions`). I left it out.

## 3. Effects and dispatch

### How effects get out

**Each context owns a `Vec<Effect>`** (`Vec::with_capacity(16)`, shared
code in `src/out.rs`), and the callbacks push to it. The facade drains it
into the caller's sink after each event. To get at it, the facade uses a
small `ctx()` accessor on the wrapper, with one match arm per leaf on the
private `inner`. That is 4 to 20 arms per machine, for example
`pointer.rs:331`. Module privacy allows it because the macro expanded in the
same module.

I wrote `ctx()` myself, so it depends on undocumented generated names:
`inner` and `Any<Name>State`.

The typed machine gets a public `take_effects()` (for example
`pointer.rs:346`), which the compile-fail probes use. Wrap's FullReset
keeps the buffer (`Ctx::with_buffer`, `wrap.rs:50`).

**Allocation:** one per context when the machine is built (lock has two
contexts, so two). The `Vec` keeps its capacity, so there are none per
event.

**Why not lend the caller's sink?** The crate leaves no way. The context
type has no lifetime parameter. A generic `C` gets no bounds in the
generated `impl<C> Name<C, Leaf>`, so callbacks can't use it. And a payload
can't carry `&mut` either, because callbacks see only `&payload`.

### What was my choice and what is the crate's

- **The effect buffer was my choice.** The first version used a shared
  `Rc<RefCell<Vec<Effect>>>`. It cost two allocations per machine (the `Rc`
  box and the `Vec`) and a `RefCell` borrow per effect. That penalized the
  crate unfairly: the reviewer measured new and drop of a `Pointer` at
  26.4 ns, against 0.2 for enum. The context-owned buffer saved about 15% on
  pointer-stream and about 25% on replay-pointer in the reviewer's runs;
  wrap didn't change.
- **The wrapper's bookkeeping and the per-state `Option` slots are the
  crate's.** I can't change either while using `DynamicName` (below).

### Per-event costs the crate adds

- `inner.take()`, then a match on (state, event).
- The typed method, which destructures and rebuilds the typed machine. That
  moves the context and every state's data slot: 32 to 144 bytes.
- `__sm_commit`, which updates the epoch and the per-scope counters.
- `__sm_record_completions`, which only compares epochs here, since no
  final states are declared.
- `inner = Some(..)`.
- For an ignored event, an
  `Err(DynamicError::InvalidTransition | GuardFailed)` with static strings
  (no allocation), after the machine is put back.

On top of these, my facade drains the buffer once per event.

**Panics poison the wrapper.** A panicking callback leaves `inner` taken.
`ctx()` then returns `None`, and every later `handle` returns
`Err(Poisoned)`.

**The typed API could be driven directly,** with no wrapper. The reviewer
tried this: a hand-written enum of `Screen<S>` values dispatching wrap's 48
(state, event) pairs to the typed methods, plus `IntoSt` impls for the 16
outcome enums, about 110 lines. It saved about 16% on wrap (23.2 → 19.4
ns/event). I didn't adopt it: IMPLEMENTING asks for the dynamic wrapper,
and that is the crate's runtime path.

### Smoke

Best of five unpinned runs (other agents were loading the machine), in
ns/event. Every row says `same as enum`.

| workload | state-machines | enum |
|---|---|---|
| pointer-stream | 15.08 | 6.54 |
| wrap-80 | 22.73 | 3.87 |
| wrap-200 | 22.77 | 3.84 |
| replay-pointer | 26.98 | 15.31 |
| replay-wrap | 27.29 | 7.57 |
| replay-polkit | 28.62 | 10.85 |
| replay-transfer | 44.38 | 11.82 |
| replay-lock | 23.79 | 8.48 |
| replay-install | 22.62 | 7.62 |

### Sizes

From `bin/sizes`, in bytes. Each facade now holds only its wrapper (lock
holds two).

| target | state-machines | enum |
|---|---|---|
| pointer | 168 | 28 |
| wrap | 176 | 32 |
| polkit | 184 | 28 |
| transfer | 296 | 40 |
| lock | 256 (152 + 104) | 28 |
| install | 336 | 24 |

Where the bytes go (`size_of`, from a throwaway test):

| target | typed machine | context |
|---|---|---|
| pointer | 64 | 32 |
| wrap | 64 | 64 |
| polkit | 80 | 40 |
| transfer | 144 | 32 |
| lock: Locker | 56 | 40 |
| lock: Delay | 32 | 32 |
| install | 112 | 32 |

- **The wrapper's part** is 40 bytes (epoch, completion `Vec`, last state),
  plus 8 per state and superstate (`scope_epochs`, present even without
  `runtime`), plus the state tag. For pointer that is 104 of 168; for
  install, 224 of 336.
- **The typed machine** holds every data-carrying state's slot at once, each
  as an `Option`. The context's `Vec` header (24 bytes) replaced the 8-byte
  `Rc`.
- **Transfer** gained two scopes from the split `Asking`.

## 4. Compile-time guarantees

### Tally

From `metrics/sme.py compile-fail`: **reject 26, runtime 10, accept 0, plus
5 forge cases** (36 probes). The first version had 22/10/4 under the old
rules.

| target | reject | runtime | forge |
|---|---|---|---|
| pointer | P1 P2 P3 P4 P5 | P6 | F-pressed-data-mut |
| wrap | W3 | W1 W2 W4 W5 | none |
| polkit | K1 K3 K4 K5 K6 | K2 | F-starting-data-mut |
| transfer | T1 T3 T4 T6 | T2 T5 | F-probing-data-mut |
| lock | L1 L3 L4 L6 L7 | L2 L5 | F-checking-data-mut |
| install | I1 I2 I3 I4 I5 I6 | | F-with-prepared-data |

### The rules ("Transitions and forging", "a transition stands for its event")

**Every probe goes through the typed transition API.**

**These moved under the new rules:**

- **P6 and W1 → runtime.** Each was accepted only through `new_init_state`,
  which is forging.
  - Every typed edge into an owner state declares a `data:` factory, and
    the probe checks the owner each one sets. The accessor returns
    `Option`, so the factories rule out "no owner", not the types.
  - `PendingWrap` is entered only under `ends_pending` or `saved_pending`,
    or kept across SetAutowrap/HT/RI/DECSC. The probe shows that only the
    print writing the last column sets it.
- **P1 → reject.** Every typed transition that delivers an `Up` is an owner
  state's `release`, and only `press`, `capture` or `moved` build one. So
  `Router::<Pressed>::release(hovering, ..)` fails with E0308.
- **L1 → reject.** The one typed release of a pending sleep's delay is
  `Delay<SleepPending>::lock_settled`, which stands for "lock confirmed or
  lock screen left". The other release, `Delay<Held>::sleep`, fails with
  E0308 on a pending sleep.
  - No type ties `lock_settled` to the `Locker`: only `Lock::handle`'s
    relay does (`lock.rs:443-451`).
- **I4 → reject.** A deferred `Prepared` is reached only by `evaluated`'s
  `fits_unpacked` candidate. `cached` doesn't exist on
  `Installer<Evaluation>`, so it fails with E0599.
- **T6 → reject**, thanks to the split `Asking` (§2): there is no `Listing`
  variant in `JobAskingUnmergeableAnsweredOutcome`.
- **T1 stays reject, with a new probe:** "the placement failed, now dispose
  of the aside". `Job<Restoring>` has no `placed`.

**These stayed `runtime`: K2, L2 and T5.** Their forge cases defeat them.

### Hiding

The macro, its impls and the facade sit in a private `mod machine`, and
`DynamicName`, `NameEvent` and `NameState` aren't re-exported. Naming
`impl_state_machines::pointer::DynamicRouter` fails with E0433, so
`new_init_state` can't be called from outside. I removed the facades'
`pub fn machine()`.

### What the crate generates public and can't be hidden

- On every typed machine: `with_<state>_data`, `<state>_data_mut`, and the
  generic `state_data_<state>_mut` (`typestate.rs:997, 1054`).
- On the wrapper that `into_dynamic()` returns, which is still a value you
  can call methods on: `set_<state>_data` and `<state>_data_mut`
  (`dynamic.rs:690`).
- `initialize()` on every state type, which re-runs the entry hooks.

Each target with state data has one forge case, built from one of these
means, with the others named in a comment:

- **pointer:** rewrite `Pressed`'s owner, and the release reaches a node
  that never took the press.
- **polkit:** rewrite the conversation, and a stale prompt counts.
- **transfer:** rewrite the probed item, and another item's response
  counts.
- **lock:** rewrite the check id, and a stale Accepted unlocks.
- **install:** `with_prepared_data(true)` on the in-memory route reaches
  `BuildOnTarget` with no unpacked-room check.

Lock's and install's comments add what `initialize()` re-emits (§1).

**Wrap has no forge case.** It has no data states, no entry hooks (so
`initialize()` does nothing), and a private context, so no forging means
stays public. My compound data types also have private fields, so
`*_data_mut` can't forge their contents. Primitive leaf data (u32, ItemId,
Node, bool) remains forgeable.

**The generic accessors also return `None`.** `state_data_<state>()`
compiles on every state type and returns `None` where the data doesn't
live. That's why P2, P4 and I6 also have a runtime refusal path; they
remain rejects, as in the baselines.

### Typical error (P2)

```text
error[E0599]: no method named `pressed_data` found for struct `Router<impl_state_machines::pointer::NativeDrag>` in the current scope
  = note: the method was found for `Router<impl_state_machines::pointer::Pressed>`
```

It points at the real mistake, and it names the state that does have the
method. The other errors read as well:

- Outcome enums: "no variant … named `Listing` found for enum
  `JobAskingUnmergeableAnsweredOutcome`" (T6).
- Wrong state types: "expected `Router<Pressed>`, found `Router<Hovering>`"
  (P1).

**One trap:** an event named `take` gave "`Delay<Held>` is not an
iterator" for L7, so I renamed it `acquire`.

## 5. Errors while writing it

**The worst: one callback shared by a payload event and a payload-less
one.** Local callbacks get `&payload` exactly when their event has a
payload. `failed_unerased`, used by both `confirm(bool)` and `stage_failed`,
gave:

```text
error[E0061]: this method takes 1 argument but 0 arguments were supplied
157 |               transition: { from: Preparing, to: Failed, after: [failed_unerased] }
    |  ________________________________________________________________^^^^^^^^^^^^^^^-
help: provide the argument
157 +             transition: { from: Preparing, to: Failed, after: [failed_unerased(/* _ */)
```

The span runs to the end of the macro, and the suggested fix isn't DSL
syntax. The fix was pairs of `x()` and `x_for<P>(&P)` methods.

**The other errors were fine:**

- Validation errors are spanned at the event, for example "branch
  candidates require transition guards; a guardless `fallback: true` must be
  last".
- A misspelt callback gets E0599 at the DSL identifier, with a suggestion.
- A missing `Default` gets E0277 at the type, though it doesn't mention
  `data:` factories as the alternative.

**Unhygienic generics:** naming a machine `C` produces cascades of E0574 and
E0109.

**Clippy** flagged two of my own names (`to_column_0`, `to_locker`), and I
renamed them. It flagged nothing in the generated code.

**Time lost was small.** Each target passed conformance on its first
compiling version.

## 6. Ergonomics

**Concision:** 2162 lines (`sme.py loc`: pointer 318, wrap 331, polkit 336,
transfer 416, lock 361, install 385, shared 15). Enum has 1165 and
typestate 2139.

The 135 lines added since the first version (the T6 split nets out roughly even):

- the per-target `ctx()` accessors, one arm per leaf;
- `take_effects`;
- the private-module wrapping and its re-export lists.

The DSL is declarative and close to the spec's tables. The rest is many
small named methods, and `fallback: true` on every unguarded branching
candidate.

**What reads well:**

- superstates with data;
- lifecycle hooks;
- `kind: external`;
- `hierarchical`;
- `data:` factories;
- typed outcome enums;
- marker traits on the state types (`Mode`, `Phase`, `Step`).

**What a maintainer must learn:**

- the fixed callback order: around, guards, before, exit hooks, factory,
  entry hooks, after;
- guards run before callbacks;
- a callback's arity follows its event's payload;
- superstate data is only ever `Default` on entry;
- the generated names, and the private `inner` that the drain relies on;
- the reserved event names.

**Debugging:**

- `current_state()` is a `Copy` enum with `Display`, and `Debug` prints
  every data slot.
- Callbacks are ordinary methods, so breakpoints work.
- The transition code is macro-generated and has no source lines.

**`Option`-typed data:** 13 sites `expect` data that transitions always set,
or fill it with `get_or_insert_default`. Now that `new_init_state` is
hidden, outside code can't produce a state without its data.

**Churn:** 23 versions, a single author, and a README that contradicts the
code in places.

## 7. Build

- **Toolchain:** rustc 1.99.0 meets MSRV 1.99, and the edition is 2024.
- **Dependencies:**
  - state-machines-core 0.14.0;
  - state-machines-macro 0.34.0;
  - harel-machines 0.2.0, which does statechart exit/entry resolution
    during expansion;
  - syn 3.0.9, the third syn in this lock (1.0.109, 2.0.119, 3.0.9).

  `Cargo.lock` didn't change.
- **Warnings:** none from the generated code, under rustc or clippy.
- **Build times:** the crate chain builds in about 3 s (dev, 4 jobs).
  After touching `lib.rs`, the slot rebuilds in 1.12 s (dev) and 1.87 s
  (release).

## 8. Spec questions

- **lock:** I relay "lock confirmed or lock screen left" after a `Locker`
  transition that leaves `Locking` or ends in `Unlocked`. By invariant 1,
  only leaving `Locking` can find `SleepPending`, so both readings give the
  same effects.
- **polkit:** for "Begin … checked in this order", `Active` with the same
  cookie wins over `identities == 0`.

## 9. Verdict

**Not for these targets as a whole.**

**What it did well:**

- It expressed all six, each in its own idiom: hierarchy with superstate
  data, entry/exit hooks, `data:` factories, child-first and ordered guarded
  choices.
- One definition gives a typestate API that rejects 26 of 36 probes, with
  clear errors.
- That API comes with a runtime wrapper that can't drift from it.

**What counts against it:**

- **Speed:** 1.8–5.9× the enum's ns/event in smoke.
- **Size:** 5.5–14× the enum's. The wrapper's 40 + 8·scopes bytes, and an
  `Option` slot for every state's data, are the crate's.
- **No context access:** the drain goes through undocumented private
  fields.
- **Forgeable by default:** the state-data mutators and `initialize()` are
  generated public, which gives five forge cases.
- **Regions and timers** are async-only.
- **Maintenance:** fast churn and a single author.

**Where it could fit in Sekka:** low-rate, protocol-like machines (install,
polkit), where the typed API can be used directly and the DSL makes the
transition table reviewable. Not the hot paths (pointer, wrap), and not
machines with orthogonal regions (lock).
