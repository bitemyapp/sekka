# impl-rust-fsm: notes

## 1. Crate

rust-fsm 0.8.0 with rust-fsm-dsl 0.8.0 (the `state_machine!` DSL), default
features (`std`, `dsl`). No reserve substitution, no lock-file change.

### The pattern

All six targets use the DSL. Each file has the same pieces, and nothing
checks that they agree:

- **the table** (`state_machine! { pub fsm(Initial) … }`): unit states, unit
  inputs, and at most one unit output per row;
- **the state type:** five targets use the crate's custom state type
  (`#[state_machine(state(spec::<target>::StateName))]`; lock uses
  `LockPhase` and `DelayPhase`), so `state()` is `*fsm.state()`. Install
  keeps a generated `State`, because its `…Deferred` copies have no
  `StateName` twin;
- **`input()`**: turns the spec's `Event` into a table input. It reads only
  the event and the data, never the table's state (lock's `PrepareForSleep`
  is the one exception). Guards become split inputs here (`PressEdge`,
  `FailureLast`, `EvaluatedNoRoom`). It returns `None` for events the spec
  ignores in every state;
- **`Data`**: the table, and everything the table can't hold, as plain
  fields. They share one struct so the table's byte packs into the data's
  padding (section 3). Each data field is meaningful in some states and
  stale in the others;
- **`act()`**: a `match` on the output, exhaustive. Each arm takes its
  event's payload with `let Event::… = event else { unreachable!() }`,
  because the table pairs every output with one kind of event. A new or
  misspelt output is therefore a compile error (`E0004`) rather than a run-
  time panic.

`handle` is `input` → `consume` → `act`. `consume`'s `Err` (no row) is
ignored, as are `Ok(None)` rows (no output).

### Why not a hand-written `StateMachineImpl`

The README suggests one for states with data, and the trait allows it: the
associated types `Input`, `State` and `Output` have no bounds
(`lib.rs:248–252`). So a hand impl could use a data-carrying `State` enum,
and an output that is a fixed-capacity effect list or a descriptor `handle`
expands (as `Repeat` already is here).

Such an impl would get impl-enum's 7 compile-time rejects. But those
guarantees would come from the user's enum, not from the crate. The price:

- every step's logic runs twice, because `consume` calls `transition` and
  then `output` on the same `(state, input)` pair (`lib.rs:302–303`). One
  shared step function could write it once, but both calls still evaluate
  it;
- the functions take no `self` or context, so config (threshold, reserve,
  cols, `delete_replaced`) would ride in every state or input;
- `transition` borrows the state and returns a new one, so transfer's frame
  stack would be cloned on every event.

That is impl-enum behind a two-function trait, so it adds nothing to the
comparison. I used the DSL, which is what the crate actually provides.

### Checking the survey against the source

The survey is right on these points (checked in
`rust-fsm-0.8.0/src/lib.rs` and `rust-fsm-dsl-0.8.0/src/{lib,parser}.rs`):

- no hierarchy, guards, entry/exit actions, context, timers or history;
- no data in DSL states;
- Mealy outputs;
- Mermaid diagrams behind the `diagram` feature;
- `Err(TransitionImpossibleError)`, which is a ZST;
- nothing allocated, and `no_std` without `std`;
- MIT, no MSRV, edition 2021, syn 2.

It is wrong or imprecise on these:

- **"Compile-time guarantees: only the declared (state, input) pairs
  exist."** This is a run-time property: any input can be passed to
  `consume` in any state and gets `Err` there. Nothing about it is checked
  at compile time. The DSL doesn't check its own rows either:
  - two rows for the same `(state, input)` compile with no diagnostic (the
    first row wins in both `transition` and `output`; no
    `unreachable_patterns` lint fires through the macro). Reproduced in a
    scratch crate;
  - with generated types, a misspelt state name in a row becomes a new
    state. In a private module that gets a `dead_code` warning, and in a
    `pub` module nothing.
- **Misspelt names are caught in two ways the survey doesn't mention.**
  - The crate's custom alphabet types make a misspelt state an error at the
    row. Tried on `pointer.rs`: `error[E0599]: no variant … named Hovring
    found for enum spec::pointer::StateName`, with "there is a variant with
    a similar name".
  - Any exhaustive `match` over a generated enum catches a new variant.
    Install's `state()` does this for its generated `State`; the exhaustive
    `act()` does it for outputs (tried: `E0004 non-exhaustive patterns:
    Output::Hovr not covered`).

  What stays silent:
  - duplicate rows;
  - a misspelt input whose correct spelling another row uses: the row is
    dead and the event is ignored;
  - a misspelt output that names another existing output: the wrong action
    runs, or its `let … else` panics.
- **Custom alphabet types can't carry run-time data either.** The macro
  writes `Self::Input::Go` as a pattern, so a tuple variant fails with
  `error[E0533]: expected unit struct, unit variant or constant, found tuple
  variant <Self::Input>::Go`. The error spans the whole macro invocation. (A
  custom output type may name associated constants with fields, so constant
  payloads are possible, since outputs are only constructed.)
- **One output per transition, and only a bare identifier.** `consume`
  computes the output on the *old* state, and only when the transition
  exists.
- **`StateMachine<T>` is only `Debug + Clone`** (not `Copy` or `PartialEq`).
  Comparing states goes through `*t.state()`, and `from_state` builds any
  state.
- I couldn't check the maintenance, star and download figures offline.

## 2. Per target

Table sizes are rows / states / inputs / outputs. "Repeated" rows are those
a superstate, wildcard or or-pattern would have saved. The DSL has none of
the three: `A(X | Y)` is "unexpected token, expected `)`", and `_(X)` is
"expected identifier, found keyword `_`".

### pointer: 55 rows / 6 / 13 / 11 (`src/pointer.rs:24`)

**Model.** The six leaves are the table, with `StateName` as its state type.
`Inside`'s rows (`Leave`, `StartDrag`, `Capture`, `Removed`,
`RemovedOwner`) are repeated in all four substates. The hover pair, owner,
press point, clicks and `beneath` are `Data` fields beside the table
(`:103`), so they exist in every state. "Entering `Inside` starts with no
hover" holds only because the `Exit` action clears the hover.

**Guards** (`input`, `:172`):

- the press target: `PressNothing`, `PressEdge`, `PressNode`;
- the threshold: `MoveFar`, measured against the last press point;
- "names the owner": `UncaptureOwner`, `RemovedOwner`, compared against the
  `owner` field.

Because `input` never reads the state, the owner and press point can be
stale. Wherever a guard doesn't matter, its rows are repeated, so a stale
value is harmless only because those rows exist. Examples: `MoveFar` in four
states, three `Press…` rows in each owner state, `RemovedOwner` in
`Hovering`. About 25 of the 55 rows are repeats.

**What worked well:** the effect order (track before delivery, except on a
release) is ordinary action code, and all 21 traces pass.

**Not expressed:** nothing.

### wrap: 58 rows / 4 / 16 / 15 (`src/wrap.rs:25`)

**Model.** The four (pending × autowrap) states are the table, so the two
flags are fully held. That's the DSL at its best. The cursor, saved cursor,
REP memory and links are `Data` (`:110`).

**Workarounds:**

- **Pending after a print.** Whether a print leaves a wrap pending depends
  on the column it ends at, which is known only after the action runs. The
  print actions feed a completion input, `Reached`, back into the table: a
  second `consume` per print that reaches the last column (`:305`).
- **`Repeat(n)`** changes state between its prints, so it never reaches the
  table. `handle` expands it into n `Print` steps (`:320`).
- **`SaveCursor`** stores the state's pending bit, which the output names
  (`Save` / `SavePending`).
- **Data guards:** `ReverseIndexTop` and `RestorePending` are split inputs.
- **`FullReset`** resets the data fields; the table's row has already moved
  to `Normal`, so the table itself is never replaced.

**Cost:** every event is listed in every state, 58 rows for four states.
This is the largest target in lines (282, against the enum's 185).

**Not expressed:** nothing.

### polkit: 46 rows / 6 / 14 / 16 (`src/polkit.rs:27`)

**Model.** `Active`'s rows (`BeginNobody`, `BeginNoScreen`, `Begin`,
`Dismiss`, `ChooseIdentity`) are repeated in its five substates, 20 extra
rows.

**Workarounds:**

- **Exit actions** (end the helper, cancel the retry, reply and close) have
  no place in the DSL. `handle` copies the state before `consume` (`:284`),
  and `exit_phase` and `end` (`:175`, `:186`) match on it. The alternative
  was output names per exit flavour (`EndConvRefuse`, `EndRetryOpen`, …),
  about 12 outputs.
- **Guards** go in `input` (`:127`). A stale conversation, a duplicate
  cookie, another request's cancel and an out-of-range identity all become
  `None`. The back-off limit becomes `Failure`/`FailureLast`.
- **The back-off limit is written twice.** `back_off` (`:202`) re-derives
  the limit that `input` already split on (`:130`), because the table can't
  pass the guard's result to the action. The two must agree.
- **`cookie`** is `Some` exactly in `Active`, kept so by the actions. That
  lets `input` drop a duplicate `Begin` without looking at the state.
- The running conversation's number always equals `last_conv`, so only the
  counter is stored.

**What worked well:** the table reads almost like the spec's transition
table.

**Not expressed:** nothing.

### transfer: 23 rows / 10 / 20 / 17 (`src/transfer.rs:25`)

**Model.** The ten states are the table.

**Workarounds:**

- **Merge frames** (a nested sub-machine per folder) can't be in the table.
  They are a `Vec<Frame>` in `Data` (`:88`), pushed and popped by actions.
  The job's config is in `Data` too.
- **next** decides between `Probing` and `Finished` from the frames. The
  item-done rows go to `Probing`, and `next` feeds `Exhausted` into the
  table when the stack empties (`:170`). The job's last step therefore
  passes through `Probing` inside one `handle`.
- **Guards** go in `input` (`:115`). It checks that the response names the
  current item, splits by payload (`ProbedFree`, `PlacedCancel`, …), and
  splits by data (`Merge` only with `can_merge`, `RestoredCancel` after a
  cancel).
- **The aside** is a stale field outside its three states.

**What worked well:** little repetition; the table is compact and readable.

**Not expressed:** nothing.

### lock: 25 + 8 rows (`src/lock.rs:29`, `:69`)

The `screen` table (state type `LockPhase`) has 4 states, 14 inputs and 13
outputs. The `delay` table (`DelayPhase`) has 3 states, 6 inputs and 2
outputs.

**Model.** The two orthogonal regions are two tables, both in `Data`
(`:95`). `handle` gives each event to `screen` first, then to `delay`, which
matches the spec's effect order. The DSL can't express the region crossings,
so I wired them by hand:

- entering `Locked`, and leaving the lock screen, feed `Settled` into the
  `delay` table (`settle`, `:173`);
- `PrepareForSleep`'s delay row depends on the screen region, so
  `delay_input` reads the `screen` table's state (`:162`), an `In(state)`
  guard. This is the one place an input function looks at a state.

**Hierarchy.** `Lockscreen ⊃ Locked ⊃ Checking` is flat. The `Lockscreen`
rows are repeated three times and `ShakeTick` twice.

**Entry/exit actions** are the outputs of the rows that enter or leave.
`shake` is reset on leaving, so that a later `leave` from `Locking` can't
cancel a stale shake. That is another data invariant kept by hand.

**What worked well:** the `delay` table (8 rows, no data) is the cleanest
code in this slot.

**Not expressed:** nothing.

### install: 70 rows / 24 / 14 / 23 (`src/install.rs:23`)

**Model.** `deferred` is a bool, so the table holds it as a second copy of
`Prepared`, `Erase`, `Format` and `Files` (`…Deferred`, `:88`). `state()`
folds the copies back (`:278`). This is the only state data any target keeps
in the DSL. `root_mib` and `closure_mib` are `Data` (`:145`), and the room
checks are split inputs (`EvaluatedNoRoom`, `BuiltNoRoom`).

**Repetition.** Without superstates:

- each preparation stage repeats `Cancel`, `Confirm`, `ConfirmWrong` and
  `StageFailed`;
- `Cancelling` lists all ten stage-result inputs one row at a time ("any
  result" becomes 10 rows);
- each executing step repeats `StageFailed`.

About 40 of the 70 rows are repeats.

**Entry/exit actions** (`Run`, `StayAwake`, `AllowSleep`) are row outputs.

**Not expressed:** nothing.

## 3. Effects and dispatch

**How `out` reaches the actions.** `out` is passed generically
(`&mut impl Sink<Effect>`) to the action functions, which are outside the
crate. The crate never sees an effect; it only returns an output symbol. No
`dyn` and no buffer.

**Allocation.** None per event. Transfer's frame `Vec` grows on `Start` and
`Listed`, as in the reference.

**Per-event costs the crate adds:**

- `consume` runs two matches over the `(state, input)` pair: `transition`,
  then `output`. It returns `Result<Option<Output>, ZST>`;
- after it, `act` matches the output and destructures the event again,
  because event payloads don't pass through the table;
- an extra `consume` for each completion input: wrap's `Reached` per print
  reaching the edge, transfer's `Exhausted` per job end;
- lock runs two input functions and two tables per event.

**Costs the crate doesn't require:**

- pointer's `input` squares the threshold and computes `dist2` on every
  `Move` in every state, because it doesn't read the state. impl-enum
  computes `dist2` only in `Edge`;
- polkit copies the 1-byte state before `consume`.

**Ignored events** cost either a `None` from `input` (the table isn't
called) or an `Err`. The `Err` is a ZST, with no allocation; its cost is the
`transition` match.

**Smoke** (ns/event, one pass, `SME_SMOKE=1`, two runs each), all rows
`same as enum`:

| workload | rust-fsm | enum |
|---|---|---|
| pointer-stream | 6.79, 6.73 | 6.10, 6.40 |
| wrap-80 | 6.04, 6.08 | 4.15, 3.90 |
| wrap-200 | 5.95, 5.93 | 3.88, 3.85 |
| replay-pointer | 18.54, 17.92 | 17.60, 16.46 |
| replay-wrap | 13.40, 13.54 | 8.47, 8.68 |
| replay-polkit | 16.70, 17.23 | 10.85, 10.53 |
| replay-transfer | 16.36, 16.86 | 12.64, 12.81 |
| replay-lock | 13.94, 12.42 | 7.73, 7.80 |
| replay-install | 12.95, 13.20 | 7.70, 7.70 |

A third run, after the forge cases (same code), gave pointer-stream 8.36
vs 6.70, wrap-80 6.54 vs 3.93 and wrap-200 6.36 vs 3.86. Smoke on this
shared machine moves by about 20%.

**Sizes** (bytes, rust-fsm vs enum): pointer 28 vs 28, wrap 32 vs 32,
polkit 24 vs 28, transfer 40 vs 40, lock 20 vs 28, install 32 vs 24.

- **The earlier 40, 48 and 48** for pointer, wrap and transfer were nesting
  padding, not the data model. The 1-byte table sat beside a separately
  aligned `Data` struct, and Rust doesn't reuse a nested struct's tail
  padding (pointer also stored an `i64` threshold²). With the tables inside
  `Data`, in all six targets, the byte packs into the data's padding.
- **Only install's extra 8 bytes are the cost of data beside the table.**
  Its `root_mib` and `closure_mib` are both stored at once, where
  impl-enum's variants hold one or the other.
- **Polkit and lock come out smaller than impl-enum**, because their flat
  fields pack tighter than impl-enum's variant layout.

## 4. Compile-time guarantees

**None of the 36 probes is rejected:** 32 runtime, 4 accept, 0 reject,
plus 6 forge cases (one per target). The enum baseline has 7 rejects.

The 0 rejects belong to the DSL. Its generated (or custom unit) types are
fieldless enums. `StateMachine::from_state` builds any state, and `consume`
takes any input in any state. A "no such field" error on a state variant
would be vacuous, since no state has fields, so I wrote no reject cases. A
hand-written `StateMachineImpl` over a data-carrying enum could reject what
impl-enum rejects, but through the user's enum, not the crate (section 1).

**Runtime, refused by the generated table** (21): P1, P3, W2, W3, W4, K1,
K3, K4, K5, K6, T1, T3, T4, L3, L4, L6, L7, I1, I2, I3, I5. These are probes
the table can attempt on its own. Each case starts from
`StateMachine::new()`, reaches the state it needs through `consume`, and
then tries the illegal step with `consume`. The table answers `Err` or a
different target or output. No runtime or accept case uses `from_state`.

**Runtime, refused by my code** (11): P5, W1, W5, K2, T2, T5, T6, L1, L2, L5,
I4. These involve data the table doesn't hold (conversation, item, column,
sizes, check id, the other region), so they go through `handle`. My
`input()` or actions refuse them. A bare table would take four of them
unchecked: `Reached` in any column (W1), `Merge` in any `Asking` (T6),
`Settled` while `Locking` (L1), `EvaluatedDeferred` with no room (I4).

**Accepted** (4): P2, P4, P6, I6. The data beside the table is readable, and
stale, in states the spec doesn't give it:

- P2: the owner after `NativeDrag`;
- P4: the press point in `Pressed`;
- P6: `Pressed` with a `beneath` from an earlier edge press;
- I6: `closure_mib` on the deferred route.

These are data-shape probes, and each goes through `handle` (the transition
API) plus the read-only `data()`, with no forging, so under "Transitions and
forging" they stay `accept`. P6 used to include
`from_state(State::Pressed)`, which counts as forging; I removed it.

**Forging.** One means stays public in every target:
`StateMachine::from_state`, an inherent method of `rust_fsm::StateMachine`.
It sits on the same generated type that carries `consume`, the crate's
transition API, which the typed probes need. The crate has no seal, and a
private generated module would hide `consume` too, so `from_state` can't be
hidden without hiding the table.

It builds a generated table in any state, or replaces a table already
driven by `consume`. It can't reach the private table inside `Pointer`,
`Wrap` and the others: no field, setter or constructor exposes it, and
`data()` and `table()` are read-only. Each target's
`forge/F-from-state.rs` uses it to commit one illegal operation at the
table level:

- pointer: P1, `Up` from a forged `Pressed`;
- wrap: W1, `WrapPrint` from a forged `PendingWrap`;
- polkit: K1, `WriteAnswer` from a forged `Prompted`;
- transfer: T3, leaving `Restoring` by replacing the table;
- lock: L4, a forged `SleepPending` after `Resumed`;
- install: I5, a forged `Cancelling` after `Erase`.

The other means, named in those cases' comments: a bare table's `consume`
takes any declared edge without `input()`'s guards or `act()`'s actions.
That is how a bare table would take W1, T6, L1 and I4, which stay `runtime`
because `handle` refuses them.

Nothing else is public. `Data`'s fields are `pub` but behind `&` only, and
its private table field means outside code can't build one.
`table().clone()` copies the table out but can't write it back.

**Privacy, not the crate.** The fields are private and exposed read-only
(`data()`, `table()`). That is why W1, W5 and T3 come out "runtime" here,
while impl-enum, with `pub` fields, has them as "accept". The refusal comes
from privacy and my actions, not from rust-fsm.

**A typical compile error.** There are no reject cases to quote, so here is
the DSL's E0533 (data in a custom input type). It points at the whole macro
invocation, not at the variant:

```text
error[E0533]: expected unit struct, unit variant or constant, found tuple variant `<Self::Input>::Go`
  --> src/main.rs:12:1
12 | / state_machine! {
13 | |     #[state_machine(input(crate::Ev))]
   ...
   | |_^ not a unit struct, unit variant or constant
```

## 5. Errors while writing it

**Syntax errors are good.** The DSL's errors are syn's and point at the
token:

- `A(Go | Back)`: "unexpected token, expected `)`" at the `|`;
- a missing comma between rows: "expected `,`" at the next row;
- `_(Go)`: "expected identifier, found keyword `_`";
- naming an input in `input()` that no row uses: E0599 "no variant … named
  `Back` found for enum `Input`";
- a misspelt state, with custom state types: E0599 at the row, with a
  similar-name hint;
- a misspelt or new output: E0004 at `act()`'s exhaustive match.

**The worst error** was the E0533 above: one minute, because I knew the
expansion. The real implementation hit no DSL errors (one unrelated
`Node: Default`).

**The remaining silent hazards** are duplicate rows, and a misspelt input or
output that names a correctly spelt twin (section 1). I checked my tables
for duplicate `(state, input)` pairs with a script; there are none.

**`act()` used to end in a catch-all `unreachable!`** over
`(output, event)`, which turned a new or misspelt output into a run-time
panic. It now matches exhaustively on the output, with a `let … else
unreachable!()` per payload. That costs about 30 lines and moves the error
to compile time.

## 6. Ergonomics

**Concision.** 1496 LoC against the enum's 1165 (+28%). Per target:

| target | rust-fsm | enum |
|---|---|---|
| pointer | 245 | 203 |
| wrap | 282 | 185 |
| polkit | 240 | 171 |
| transfer | 234 | 184 |
| lock | 236 | 206 |
| install | 253 | 210 |

The custom state types removed five `state()` mapping matches, and the
exhaustive `act()` added its `let … else` lines. The table rows are short
and readable, but they are repeated, and the actions and guards come on top
of them.

**Readability.** Each table reads like the spec's transition list and could
render as a Mermaid diagram. But one transition is spread over three places:
the row, the `input()` arm that selects it, and the `act()` arm that does
its work. The data invariants are kept by hand:

- pointer's hover is clear outside `Inside`;
- polkit's `cookie` is `Some` exactly in `Active`;
- lock's `shake` is 0 outside `Locked`.

**What a maintainer must learn.** The two row forms; the
`#[state_machine(state(...))]` attribute; that unlisted pairs are `Err`;
that the output is computed on the old state. Plus the conventions used
here: split inputs for guards, completion inputs, `from` for exit actions,
and stale data guarded by rows. The crate itself is small (`lib.rs` 340
lines, most of it docs; DSL about 400).

**Debugging.** `state()` is the spec's own `Copy` enum, and `consume` is two
small matches, easy to step through. A debugger shows the stale data fields
next to the state, with nothing saying which ones are live.

## 7. Build

- rust-fsm and rust-fsm-dsl are edition 2021, with no MSRV.
- Dependencies: proc-macro2, quote, syn 2 (default features; no `full`). No
  build script.
- No warnings from my code or the macro; clippy is clean.
- In a scratch crate, rust-fsm, the DSL and syn built from clean in 1.69 s
  (debug, 4 jobs).
- The `#[derive(...)]` attributes in the DSL apply to every generated type,
  including the `Impl` marker, but not to a custom state type.

## 8. Spec questions

None that changed behaviour. Two modelling notes:

- polkit's running `conv` and lock's running `check` always equal the
  machine-wide counters (`last_conv`, `last_check`), so only the counters
  are stored.
- lock's "`CancelShake` if in `Locked`" is read as `Locked` or `Checking`
  (`Checking ⊂ Locked`), as the reference does.

## 9. Verdict

Not for these targets. Every one has state data, guards, and a superstate or
entry/exit actions, and the DSL holds none of them. The table ends up a
third of each implementation, beside a hand-written guard function, a data
struct with stale fields, and an action match. Against the enum baseline:

- 28% more code;
- slower on the smoke runs only (phase 3 has the real benchmarks): about
  1.05–1.25× on the pointer stream, 1.5–1.65× on wrap, 1.05–1.8× on the
  ~100-event replays;
- no compile-time guarantee from the crate (0 rejects against the enum's
  7);
- the same size or smaller in every target but install (+8 bytes);
- one silent DSL hazard: duplicate rows (and a misspelt input or output that
  hits a correctly spelt twin). Custom state types and exhaustive matches
  catch the other typos at compile time.

**What it did well:** small, readable tables with no allocation and no
unsafe. The custom state type lets the table run directly on the spec's
`StateName`. The bool data that does fit (wrap's flags, install's
`deferred`) is fully checked by the table. Lock's 8-row `delay` table shows
the case it suits: a flat, payload-free protocol table, where the Mealy
table doubles as documentation (with the `diagram` feature). In Sekka that
means small connection, inhibitor or retry-state tables whose events carry
no data and whose transitions have no guards. For everything else here,
plain `enum` + `match` is shorter, faster and stricter.
