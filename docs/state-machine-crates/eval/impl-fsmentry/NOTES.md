# impl-fsmentry: notes

## 1. Crate

`fsmentry` 0.4.0 (proc macro; `fsmentry-core` 0.4.0 does the parsing and
code generation). No reserve substitution. Cargo.lock unchanged.

What it is, from the source (`fsmentry-core-0.4.0/src/lib.rs`): a graph DSL
(`A -> B(T) -> C`, source groups `A & B -> C`, named edges `A -name-> B`,
also from a source group) that expands into:

- a plain `pub enum` of the states;
- an `XEntry<'state>` enum;
- one handle struct per state **with outgoing edges**. A handle holds
  `&'state mut X` in a private tuple field. It has one method per outgoing
  edge (`mem::replace`: it takes the destination's data and returns the
  left state's data) and, if the state has data, `AsRef`/`AsMut` impls.

There are no events, guards, actions, entry/exit hooks or context.

Where the survey was wrong or incomplete:

- "per-state handle structs": only states with outgoing edges get one.
  For a sink or isolated state, the `Entry` variant holds `&'state mut Data`
  directly, or nothing (`ExecEntry::Flush`, `PhaseEntry::Locked(&mut LockedData)`).
- "`as_ref`/`as_mut` for the state's data": these are `AsRef<T>`/`AsMut<T>`
  impls, and their borrow is tied to the handle, not to `'state`. A helper
  can't return `&'state mut Data` out of a handle, so lock's in-state data
  updates match the enum directly (`lock.rs:98`). Handles of states with
  the same data type do share these traits (transfer's `Probing`, `Placing`
  and others all implement `AsRef<ItemId>`); the transitions share nothing.
- "match plus `mem::replace` at zero cost": the generated methods aren't
  `#[inline]`. Called from `handle::<Digest>` monomorphized in `bench`,
  they stay out-of-line calls through the GOT, each re-checking the
  discriminant with a panic arm ("entry struct was instantiated with a
  mismatched state"). Section 3 has the measured cost.
- The README says handle structs "expose their mutable reference"; the
  field has no `pub`, so it is visible only in the macro's module and its
  children. Each graph sits in its own `pub mod`, as the README recommends.
- "~5 lines of graph plus ~20 lines of match": the graphs are 4–16 lines
  per target, the matches 60–150 (LoC in section 6).
- Not in the survey:
  - one edge per ordered pair of states (`duplicate edge definition`): an
    edge shared by several events can stand for none of them alone. That
    is the cause of W3 and L1 (section 4).
  - An edge can demand evidence only through its destination's data type
    (a proof token, install's `Route` and `Steps`): the proof belongs to
    the node, not the edge.
  - Transition methods aren't `#[must_use]`.
  - A misspelled node name silently defines a new state (section 5).

## 2. Per target

All six: every conformance trace passes, the differential test passes (256
cases, and 5000 in release), `NOT_EXPRESSED` is empty, nothing is ignored.
Every edge is named for its event where it has one event. Shared edges are
named for what they share: wrap's `settle`, polkit's `restart`/`retry`,
lock's `release`, transfer's `next`/`finish`.

- **pointer** (21/21). Two graphs, nested as the README does hierarchy.
  `Window { Outside, Inside(Over), NativeDrag }` (`enter`, `leave`,
  `start_drag`, `drag_ended`) and
  `Press { Hovering, Pressed(Node), DragsOnlyPressed(EdgePress), DragsOnlyDragging(Node) }`
  (`press`, `edge_press`, `drag`, `release`, `capture`).
  - `Over` (the hover and the `Press` machine) has private fields and
    isn't `Copy`.
  - Exit actions come from the data a transition returns:
    `i.leave().hover.clear(out)`. The release's `Up` goes to the owner
    returned by `p.release()`.
  - Capture "from any press state" is four arms, one per handle type.
- **wrap** (14/14). One graph of four unit states (`pend`, `settle`,
  `autowrap_off`, `autowrap_on`); the cursor, links, saved cursor and
  `previous` are private fields beside it. RIS rebuilds the machine
  (`wrap.rs:214`).
- **polkit** (16/16). `Agent { Idle, Active(Session) }` (`begin`, `end`)
  nests `Phase` (`prompt`, `submit`, `restart`, `back_off`, `give_up`,
  `retry`, `choose`).
  - ChooseIdentity takes `choose` from `Starting`/`Prompted`/`Unavailable`,
    but `restart` from `Checking` and `retry` from `RetryWait`, because
    those pairs already have their edge.
  - `Request`/`Session` aren't `Copy`.
  - Back-off and the conversation check are plain guards; `last_conv`
    and the config sit in a sibling `Ctx`.
- **transfer** (20/20). One per-item graph; the merge frames are a
  `Frames(Vec<Frame>)` beside it, as in the reference.
  - A conflict is `AskingFolders` or `AskingFiles` (only folders have
    `merge`). A placement is `Placing` (at the target, from `free`) or
    `PlacingCopy` (a free name, from `keep_both`). This is fix 6.
  - `Disposing` and `Restoring` leave only by `disposed_next`/`_finish`
    and `restored_next`/`_finish`; the shared **next** is `next`/`finish`.
  - `Replaced`/`Restore` have private fields with read accessors and
    aren't `Copy`.
  - Each state that finishes an item ends in a two-armed `match` over its
    own handle.
- **lock** (18/18). The regions are two graphs side by side, `Screen`
  (`lock`, `unlock`) and `Delay` (`take_delay`, `sleep`, `release`,
  `resume`). `Lockscreen ⊃ Locked ⊃ Checking` is nested three deep:
  `LockscreenData` holds `Phase` (`confirm`), and `LockedData` holds
  `Check` (`submit`, `refused`).
  - Both data structs have private fields, aren't `Copy`, and expose
    `phase_entry()`/`check_entry()`, so the inner graphs are reachable on
    the live machine (fix 8).
  - Entry and exit actions are hand-written around the edges.
  - Deep exits (PAM accepted) look at the inner state first, then take the
    outer edge (`lock.rs:250`).
- **install** (16/16). `Session` (`begin`, `prepared`, `refused`,
  `cancel`, `cancelled`, `confirm`, `complete`, `fail`) nests `Prep`
  (`checked`…`built`) in `Preparing` and `Exec` in `Executing`.
  - The nested machines are sealed. `Preparation(Prep)` is built only at
    `Checks`. `Steps(Exec)` is built only by `Steps::confirmed(&Route, phrase_ok)`
    (at `Erase`, and `None` for a wrong phrase).
  - `Prepared(Route)` holds a token whose field is private; only
    `handle`'s room checks build one (`Route::deferred_fits`, `Route::in_memory`).
  - Superstate rows are one edge each. Stages that end a superstate are
    decided on the outer handle after reading the inner state.

## 3. Effects and dispatch

`out: &mut impl Sink<Effect>` goes straight through my handler functions:
no `dyn`, no buffer, and no allocation (transfer's frame `Vec` grows only
on merges, as in the reference).

Per event, the crate adds:

- building the `Entry` (one `match`);
- an out-of-line call per transition and per `as_ref`/`as_mut`, each with
  a panic arm (section 1).

Ignored events are a `_ => {}` arm, with no `Err`. `unsafe(true)` would
make the panic `unreachable_unchecked`: generated `unsafe` (banned), and it
wouldn't change the inlining.

Smoke (one pass each, ns/event, fsmentry against enum; every row
`same as enum`; taken at load average ~10):

| Workload | fsmentry | enum |
|---|---|---|
| pointer-stream | 7.05 | 7.02 |
| wrap-80 | 4.20 | 4.11 |
| wrap-200 | 3.86 | 4.38 |
| replay-pointer | 19.06 | 17.50 |
| replay-wrap | 8.47 | 8.06 |
| replay-polkit | 19.89 | 12.34 |
| replay-transfer | 14.79 | 13.06 |
| replay-lock | 10.23 | 7.95 |
| replay-install | 10.90 | 11.89 |

Criterion `replay-polkit` is the number that stands, not the smoke pass:

- before the review fixes, unloaded: fsmentry 549.40 ns against enum
  505.38 ns (+9%);
- after them, three runs at load average ~10:
  - fsmentry 704.66 / 874.89 / 721.21 ns;
  - enum 504.79 / 665.26 / 526.05 ns.

Those three runs are too noisy to tell whether removing `Copy` changed
anything; phase 3's pinned runs should decide. The out-of-line calls are
amplified by the harness: `handle` is instantiated in `bench`, so nothing
from `impl-fsmentry` inlines without LTO.

Sizes (bytes, fsmentry against enum):

| Target | fsmentry | enum |
|---|---|---|
| pointer | 28 | 28 |
| wrap | 32 | 32 |
| polkit | 28 | 28 |
| transfer | 40 | 40 |
| lock | 36 | 28 |
| install | 24 | 24 |

- transfer: the earlier 48 came from my `Ctx` struct's tail padding. It
  wasn't forced (disjoint field borrows work); `Frames` beside `config`
  has none.
- lock: each nesting level is its own struct, with its own padding and tag.

## 4. Compile-time guarantees

Under "Transitions and forging", 36 probes: 23 reject, 10 runtime, 3
accept, plus 5 forge cases.

| Target | Reject | Runtime | Accept | Forge |
|---|---|---|---|---|
| pointer | P2–P6 | P1 | | F-as-mut |
| wrap | | W2, W4, W5 | W1, W3 | none |
| polkit | K1, K3, K4, K5, K6 | K2 | | F-as-mut |
| transfer | T1, T3, T4, T6 | T2, T5 | | F-as-mut |
| lock | L3, L6, L7 | L2, L4, L5 | L1 | F-as-mut |
| install | I1–I6 | | | F-as-mut |

Every transition probe calls a handle on the live machine (`entry()`,
`phase_entry()`, `check_entry()`, `Steps::entry()`). Shape probes (P2–P6,
L6, I6) use variant literals or standalone values, which the rule allows.

What the reject errors name:

- No such edge (E0599):
  - K1: `starting.submit`;
  - K3: `starting.restart`;
  - K5: `starting.retry`;
  - K6: `unavailable.retry`;
  - T1: `restoring.placed`;
  - T3: `disposing.next`;
  - T4: `asking_files.free`;
  - T6: `asking_files.merge`;
  - L3: `locking.submit`;
  - L7: `held.take_delay`;
  - I2: `files.bootloader`;
  - I3: `preparing.confirm`;
  - I5: `executing.cancel`.
- No such data: P2, P5, L6 (no `as_ref`/`as_mut`), P4 (E0609), P3 (E0308,
  the edge returns `Node`), I6 (E0023).
- Missing destination data: P6 (E0061).
- Consumed handle: K4 (E0382, `active` moved by `end()`).
- Proof tokens, built only by the room checks or a confirmation: I4
  (E0451, `Route`'s private field) and I1 (E0423, `Steps(..)`'s private
  field).

What it can't reject:

- **W1**: the cursor is context outside the graph. A ZST proof token
  `PendingWrap(AtEdge)` wouldn't help either: a token built while the
  cursor sits at the edge stays valid after a `CursorTo` through `handle`.
- **W3**: `settle` is the single `PendingWrap -> Normal` edge for eight
  causes, so an HT arm can take it.
- **L1**: `SleepPending -release-> NotHeld` is also LogindVanished's edge,
  and the delay graph can't see the lock region.

The runtime cases are event data (K2, T5, L2), an event that must take an
edge (L4), or effects (P1, W2, W4, W5, T2, L5).

**Forging means** (all crate-generated except where noted as hidden):

| Means | Where | Status |
|---|---|---|
| `AsMut<T>` on every data state's handle | all but wrap | public with the handles: a forge case per target |
| `&'state mut Data` in a sink state's `Entry` variant | lock's `Locked`, install's `Cache` | public; named in the forge comments |
| every exit returns the left state's data, which another machine's edge can take (a `Route`, a `Steps`, a `Session`) | all | crate behaviour; named in the forge comments |
| `Copy` on data carrying private state (`Over`, `Request`/`Session`, `LockscreenData`/`LockedData`, `Replaced`/`Restore`, and the enums holding them) | implementation | **hidden** (fix 1) |
| nested machines as bare edge data (`Preparing(Prep)`, `Executing(Exec)`) | implementation | **hidden**: `Preparation`/`Steps` newtypes (fix 2) |
| pub fields on transfer's data | implementation | **hidden**: private, with accessors (fix 2) |
| state enums' pub variants | crate | the live machines' state fields are private, so only standalone values are affected |

The forge cases (`compile-fail/cases/fsmentry/<target>/forge/F-as-mut.rs`)
each commit one probe:

- pointer P1: swap two machines' `Over`, and `Up` reaches node 1;
- polkit K2: `*starting.as_mut() = 1`;
- transfer T5: `*probing.as_mut() = ItemId(1)`;
- lock L2: swap two lock screens, and a stale `PamDone` unlocks;
- install I2: swap two machines' `Steps`, and step 4 is skipped.

wrap has no forge case: `Mode` has no data states and every field is
private.

A typical error:

```text
error[E0599]: no method named `retry` found for struct `impl_fsmentry::polkit::phase::Unavailable<'state>` in the current scope
  |         unavailable.retry(2);
  |                     ^^^^^ method not found in `impl_fsmentry::polkit::phase::Unavailable<'_>`
```

## 5. Errors while writing it

- In my code: E0308 "expected `()`, found `u32`" when an arm ended in a
  transition. A transition returns the left state's data, so arms that
  leave different states differ in type (`polkit.rs:148`); the fix (`_ =`)
  took seconds.
- While writing the probes: an edge name that matches a prelude trait
  method gives a misleading error when the edge is missing. `Delay`'s
  `-take->` made `held.take()` fail with "`Held<'_>` is not an iterator"
  (`Iterator::take`), so I renamed the edge `take_delay`.
- DSL errors, from a scratch crate outside the workspace:
  - a duplicate edge: "duplicate edge definition between Closed and Open",
    pointing at the arrow;
  - no edges: "must define at least one edge `A -> B`";
  - a data type named like its node: E0428, then a misleading E0107.
- A misspelled node in a transition (`Open -> Lokced`) silently defines a
  new state with its own method. The exhaustive `state()` match catches it.

## 6. Ergonomics

LoC (`metrics/sme.py loc`), against the previous round:

| | pointer | wrap | polkit | transfer | lock | install | total |
|---|---|---|---|---|---|---|---|
| fsmentry now | 212 | 195 | 220 | 293 | 271 | 290 | 1487 |
| fsmentry before | 213 | 195 | 220 | 257 | 261 | 246 | 1398 |

Today's run gives the enum 1195 (it was 1165 in the previous round).

What the fixes cost:

- **Fix 6** (node splits): about +20 in transfer. The `AskingFiles` arm is
  20 lines and the `PlacingCopy` arm 6; the `Ask` struct and the
  `can_merge` guard went.
- **Fix 2** (sealing): +19 in transfer for the `Replaced`/`Restore`
  accessors.
- **Fixes 2 and 7** (install's newtypes and the `Route` token): +39 for
  the newtypes and their impls, plus a few handler lines; +44 in install.
- **Fix 8**: +10 in lock for the accessors.
- **Event names** (fix 5): about 0, the DSL renames, plus 4 lines for
  `Disposing`/`Restoring`'s own exits.
- `Frames::placed` saved about 6 lines in transfer.

**Fix 10 not applied.** The review says a local trait over the handle types
would fold transfer's two-armed `match`es into fewer lines. Measured: I
applied it (a `Leave` trait with ten impls, `s.leave(frames.done(..))` at
each site), ran rustfmt and `sme.py loc`, and transfer came to 314 lines
against 293. Each handle type needs its own impl, so it costs 21 lines; I
reverted it.

Readability:

- The graph blocks are a checked edge list named for the events, and
  probes read like typestate's.
- The rest is plain `match` code. The state is an ordinary enum, so you
  can step through dispatch.

Costs:

- "from any substate" rows need one arm per handle type;
- superstate exits need peek-then-act;
- data carried forward must be copied before the call;
- node and edge names occupy namespaces (a struct per node; edge names can
  collide with prelude traits);
- proof tokens and seals are plain Rust around the graph, not crate
  features.

## 7. Build

- Edition 2021 crate; works unchanged from this edition 2024 / Rust 1.99
  workspace.
- No runtime dependencies. Build-time: `fsmentry-core`, `syn` 2, `quote`,
  `proc-macro2`.
- rustc and clippy (`--all-targets`) are clean.
- A crate-only rebuild took about 0.3 s (debug) and 0.6 s (release).

## 8. Spec questions, and where I departed from the review

Spec choices:

- install's `Cache` carries only the closure.
- lock's `ShakeTick` acts in `Checking` too (the shake is `Locked`'s data).
- polkit checks the duplicate `Begin` before the `identities == 0` refusal.

Departures from the review:

- **T3 is `reject`**; the review's expected table kept it `accept`. Under
  the confirmed reading, naming `Disposing`'s and `Restoring`'s exits for
  their events makes every way out stand for the disposal or restore
  result, as typestate's `restored` does. It costs 4 DSL lines. The
  returned aside is still droppable after that result.
- **Fix 10** is measured and not applied (section 6).
- **W1** stays `accept` even with a proof token (section 4).
- **Install's tokens can travel between machines.** An exit hands back
  its `Route` or `Steps`, and another machine's `prepared`/`confirm` will
  take it. That is a two-machine transplant like the `AsMut` swaps, so it
  is named in install's forge comment and doesn't decide I1 or I4.

## 9. Verdict

With the review's fixes, fsmentry's legal-edge handles give
typestate-class guarantees here: 23 of 36 probes reject, against
typestate's 32 in today's table. The costs:

- about 6% more lines than before the fixes (1487);
- sizes equal to the enum's except lock's nesting (36 vs 28);
- the crate's out-of-line transitions.

Still hand-written: guards, events, effects, entry/exit order, and the
seals and tokens. The `AsMut` on every data handle is a forging means the
crate won't let you hide without hiding the handles themselves.

I'd choose it for linear transactions and protocol states where the edge
list and the data per state are the main thing to get right (install,
transfer, the delay region). For hierarchical, effect-heavy machines
(pointer, polkit, lock), a crate with superstates and entry/exit actions
does more, though fsmentry's nesting worked for all three.
