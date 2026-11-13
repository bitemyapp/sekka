# Review: rust-fsm slot (rust-fsm =0.8.0, rust-fsm-dsl 0.8.0)

Paths: workspace `../eval/`; crate source
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rust-fsm-0.8.0/src/lib.rs`
and `…/rust-fsm-dsl-0.8.0/src/{lib,parser}.rs`. Line numbers below come
from single-file `cat -n` reads.

What I ran (component checks only, own target dirs, no workspace edits):

- `cargo test --locked -p impl-rust-fsm`: 12/12 spec tests and the
  compile-fail suite pass. Conformance: pointer 21/21, wrap 14/14, polkit
  16/16, transfer 20/20, lock 18/18, install 16/16.
- `SME_PROPTEST_CASES=5000 cargo test --locked --release -p impl-rust-fsm --test spec`: 12/12.
- smoke, three runs: every rust-fsm row says `same as enum`. `bin/sizes`
  gives the same numbers as NOTES.
- `metrics/sme.py compile-fail`: the rust-fsm row matches NOTES (32 T, 4 A, 0 R).
- A scratch crate outside the workspace
  (`rfsm-probe`, not kept; one bin per claim) to test the DSL claims and
  model struct layouts.

## 1. Idiom fidelity

What the DSL can hold: unit states, unit inputs, and one unit output per
row. The macro emits `(Self::State::X, Self::Input::Y)` arms
(dsl `lib.rs:83–95`), fieldless enums (`:124`, `:137`), and an output that
is a single `Ident` (`parser.rs:15–16`). Because of that, I call a target
*idiomatic* when every state change goes through `consume` on a table row
and the hand code covers only what the DSL can't hold: payloads, data,
guards and effect code. No source file calls `from_state`. No shadow
state enum exists: the table's state is the only state, and the data
fields beside it are not a second state.

The pattern is the same in every file. A table, then `input()` (guards as
split inputs), then `consume`, then `act()` (a `match (Output, Event)` with
a catch-all `unreachable!`).

| target | grade | in the table | hand code beside it |
|---|---|---|---|
| pointer | idiomatic (minor under-use, S1) | `pointer.rs:22–91`: all six leaves; the topology of all 27 spec lines (55 rows; `Inside`'s five rows repeated ×4) | guards for 4 of 27 lines in `input` `:165–184` (threshold `:167`, press target `:169–173`, owner `:177`, `:181`). Hover, owner and press live in `Data` `:98–106`. "Enter `Inside` with no hover" relies on the `Exit` action clearing it (`:228–235`). Effects in `act` `:187–246` |
| wrap | idiomatic (minor under-use, S1) | `wrap.rs:23–93`: both flags as the four states. This is the only target whose whole spec state is in the table. The Mealy output carries the pending bit for DECSC (`Save`/`SavePending`, `:39`, `:55`) | guards (RI at top, EL `m>2`, a pending restore) for 3 of 13 lines in `input` `:175–195`. Whether a print leaves a wrap pending is decided in `act` and fed back as `Reached` (`:283–287`). `Repeat` expands into n table steps in `handle` (`:300–306`), and each print still goes through `consume` |
| polkit | idiomatic, one recorded workaround | `polkit.rs:25–87`: the topology of all 19 spec lines; `Active`'s five rows repeated ×5 | ~13 of 19 lines are guarded in `input` `:123–163`: current conv, cookie, identities, screen, back-off limit, identity range. Exit actions are keyed on the pre-step state (`from`, `:266`; `exit_phase` `:171–179`, `end` `:182–188`) rather than on per-state outputs; NOTES records the trade-off (~12 more outputs). `back_off` (`:197–208`) re-derives the limit that `input` already split on (`:126`): two copies of one guard that must agree |
| transfer | idiomatic | `transfer.rs:23–62`: all ten states in 23 rows, with little repetition | the item guard on every response, plus the `can_merge` and `then` splits (`input` `:111–143`). The frame stack is spec data, so it is data in every implementation. `next` picks `Probing` or `Finished` from the frames and feeds `Exhausted` (`:166`): on the ~10 lines that end in **next**, the job-end decision is hand code, but `consume` still makes the transition |
| lock | partial (honest, recorded) | two tables, one per region (`screen` `:27–64`, `delay` `:66–84`); the hierarchy is flattened | the region crossings are wired by hand: `settle` (`:168–172`) runs from the enter-`Locked` and leave actions, and `delay_input` reads the `screen` table (`:157`). Guards in `screen_input` `:125–147`. Entry and exit actions are row outputs. The 8-row `delay` table holds its whole region |
| install | idiomatic | `install.rs:23–137`: the topology of all 30 lines, with `deferred` held as four duplicate states (`:88–116`) that `state()` folds back (`:271–275`) | the room checks and the phrase are split inputs (`:175–193`); the sizes live in `Data` `:143–147` |

**Share of the spec in the crate's mechanism.**

- **Next-state topology:** about 100% of spec lines are table rows, with
  three exceptions where hand code decides the moment and `consume`
  executes it: transfer's job end, wrap's pending-after-print, and lock's
  cross-region release.
- **Guards:** about 0%. All of them are in `input()`; only their outcomes
  appear in the table, as named inputs.
- **Effects:** 0%. The table only names them.
- **State data:** about 0%, except wrap's two flags and install's
  `deferred`.

That is the most the DSL can hold.

## 2. Compile-fail audit

All 36 probes have a case (P1–P6, W1–W5, K1–K6, T1–T6, L1–L7, I1–I6).
There are no rejects, so no `.stderr` files are needed. I disagree with
none of the classes:

- **0 reject is correct for this crate.** `State`, `Input` and `Output`
  are fieldless `pub` enums. `from_state` (`lib.rs:291`) builds any state,
  and `consume` (`lib.rs:298`) takes any input in any state. Custom
  alphabet types don't change this: they must be unit variants (E0533 for
  a tuple variant, reproduced).
- **The accepts (P2, P4, P6, I6) are genuine.** In each, the stale field
  is readable through `data()`. Finny, which also keeps data beside the
  machine, has P2, P4 and P6 as accepts too.
- **W1, W5 and T3** are runtime here but accepts in impl-enum, only
  because of private fields. NOTES says so, and statig has the same
  classes.
- **W1, T6, L1 and I4** go through `handle`, because each predicate
  involves data the table doesn't have. NOTES states that a bare table
  would take them.
- **The cases test what they claim.** Each runtime case asserts the
  refusal (`Err`, an unchanged state, or an absent effect), and each
  accept asserts that the illegal value is observable.
- **One cosmetic point:** P6's table half (`P6-owner-mismatch.rs:11–12`,
  `from_state(State::Pressed)`) proves nothing, since the table never has
  an owner. The `handle` half (`:14–20`: `Pressed` with a stale
  `beneath`) carries the case. No change needed.

## 3. NOTES.md claims

### Verified

I checked these against the code, the crate source and the scratch crate:

- **Unit-only alphabet.** The table holds only unit states, inputs and
  outputs, and an output is one bare identifier. A tuple variant in a
  custom input type gives `error[E0533] … found tuple variant
  <Self::Input>::Go`, spanning the whole macro (reproduced verbatim).
- **Guards as split inputs.** `input()` never reads the table state
  except in lock's `delay_input` (`:157`). Checked in all six files.
- **`consume`** calls `transition` and then `output`, both on the old
  state; `output` runs only when the transition exists
  (`lib.rs:302–303`). `TransitionImpossibleError` is a ZST.
- **Duplicate rows** compile with no diagnostic, and the first row wins
  in both `transition` and `output`. I reproduced this in a private
  module and with a custom state type; no `unreachable_patterns` lint
  fires.
- **A misspelt state** in generated types gets only a `dead_code`
  warning, and only when the module is private (reproduced). For a
  misspelt *target* state, the warning names the correct state: "variant
  `B` is never constructed".
- **Syntax errors:** `A(Go | Back)` gives "unexpected token, expected
  `)`" and `_(Go)` gives "expected identifier, found keyword `_`"
  (reproduced).
- **Table sizes:** I counted every rows/states/inputs/outputs figure and
  every repeat count ("about 25", "20 extra", "about 40").
- **Numbers:** the LoC table matches `metrics/sme.py loc`, and the size
  numbers match `bin/sizes`. Every `:line` reference in NOTES points at
  the code it describes.

### Wrong or overstated

1. **"Misspelt states … a typo gets no warning at all"** (§1). The same
   claim feeds the verdict's "two silent DSL hazards" (§5, §9). It is
   overstated in two ways:
   - The crate's **custom alphabet types**
     (`#[state_machine(state(crate::S), input(crate::I))]`, README
     "Custom alphabet types") make a misspelt state or input an `E0599`
     error at the row, with a "there is a variant with a similar name"
     hint (reproduced).
   - In **this implementation**, every `state()` is an exhaustive match
     over `fsm::State`: `pointer.rs:264`, `wrap.rs:312`, `polkit.rs:274`,
     `transfer.rs:269`, `lock.rs:275` and `:281`, `install.rs:264`. A
     misspelt state therefore already fails with `E0004 non-exhaustive
     patterns: &State::Bb not covered` (reproduced).

   What stays silent:
   - duplicate rows;
   - a misspelt input whose correct spelling another row uses (the row
     is dead and the event is ignored);
   - a misspelt output whose correct name another row uses. `act()`'s
     catch-all `unreachable!` makes that a runtime panic (`pointer.rs:244`,
     `wrap.rs:281`, `polkit.rs:254`, `transfer.rs:248`, `lock.rs:245`,
     `install.rs:245`).
2. **The size attribution** (§3: "The rest is `Data`, which holds every
   state's fields at once: pointer's owner and press point, … wrap's
   `u128` alignment") is wrong for pointer, wrap and transfer.
   - The excess comes from layout, not from the data model. The 1-byte
     table sits beside a separately aligned `Data` struct, and Rust
     doesn't reuse a nested struct's tail padding. Pointer also stores
     `threshold2: i64` (`pointer.rs:111`).
   - I modelled the same field types: nested layouts give 40, 48 and 48
     bytes (the reported sizes); flattened, they give 28, 32 and 40
     (impl-enum's sizes).
   - Only install's extra 8 bytes come from keeping the data beside the
     table, because both sizes are held at once.
3. **The case against a hand-written `StateMachineImpl`** (§1) has three
   problems:
   - "The output would have to be a `Vec`" is wrong. `Output` is an
     unbounded associated type (`lib.rs:246–264`). A fixed-capacity
     output, or a descriptor that `handle` expands (as `Repeat` already
     is here), would work.
   - "Every guard would be written (and run) twice" is half right. Each
     guard runs twice, because `consume` calls both functions
     (`lib.rs:302–303`), but one shared step function would write it
     once.
   - The paragraph omits the main point. Because the associated types are
     unbounded, a hand impl can use a data-carrying `State`. That is the
     README's own advice when "a state cannot carry any additional
     data". Such an impl would get impl-enum's 7 rejects, but those
     guarantees would come from the user's enum, not from the crate. The
     price is every step evaluated twice, config carried in each state
     or input, and transfer's frames cloned per event.

   The conclusion ("adds nothing to the comparison") stands, but the
   reason should be this one.
4. **The verdict's "1.2 to 1.9 times slower" (smoke)** rests on a single
   noisy wrap-80 run; NOTES itself reports 6.3–6.4 ns on reruns. My two
   reruns (ns/event, rust-fsm vs enum):

   | workload | rust-fsm | enum | ratio |
   |---|---|---|---|
   | pointer-stream | 8.2 | 6.8 | 1.2× |
   | wrap-80 | 5.9 | 3.9 | 1.5× |
   | wrap-200 | 5.7–5.9 | 3.8–4.6 | 1.3–1.5× |
   | replay-lock | 13.0–13.3 | 8.6–8.7 | 1.5× |
   | replay-transfer | 17.0–17.4 | 12.8–13.5 | 1.3× |

Minor, no change needed: "custom alphabet types can't carry data" is
right for runtime data. A custom output type can name associated consts
that have fields (reproduced), so constant payloads are possible.

## 4. Test integrity

- After substituting the crate name, `tests/spec.rs` and
  `tests/compile_fail.rs` are identical to impl-statig's, and to smlang's,
  sml's, finny's, fsmentry's, statum's and state-machines'.
- `NOT_EXPRESSED` is empty and there are no `#[ignore]`s; nothing needs
  them. The 5000-case release run passes.

## 5. Benchmark distortions

- **None that would skew the benchmarks.** There is no per-event
  allocation, no cloning, no `dyn` and no queue. `out` is passed
  generically. An ignored event costs a `None` from `input` or a ZST
  `Err` after one `transition` match.
- **Costs the crate requires:**
  - two matches per `consume`;
  - `act` re-matching the event to get its payload;
  - completion inputs: wrap's `Reached` once per print that reaches the
    edge, transfer's `Exhausted` once per job;
  - lock running two input functions and two tables per event.
- **Small costs the crate doesn't require:**
  - pointer's `input` computes `dist2` on every `Move` in every state
    (`:167`), because it doesn't read the state; impl-enum computes it
    only in `Edge`;
  - polkit copies a 1-byte state.

  Pointer also precomputes threshold², which favours rust-fsm slightly.
- **Costs the crate doesn't require that do affect a metric:** the
  nesting padding in the size metric (M1).

## 6. Required fixes

1. **M1 (must): sizes.** In pointer, wrap and transfer, keep the table
   and the data in one struct: either move `fsm` into `Data` (then
   `table()` returns `&self.data.fsm` and `data()` keeps working for the
   probes) or flatten `Data`'s fields into the machine.
   - Store pointer's threshold as the `i32` from `Config`, and square it
     per event as impl-enum does (`impl-enum/src/pointer.rs:95`).
   - Re-run `bin/sizes`. Expect pointer 28, wrap 32, transfer 40.
   - Rewrite the NOTES §3 size paragraph: only install's extra 8 bytes
     are the cost of data beside the table.
2. **M2 (must): the typo hazard.** Correct the misspelt-state claim in
   NOTES §1, §5 and §9 as in finding 3.1:
   - custom alphabet types make misspelt states and inputs `E0599`;
   - this implementation's exhaustive `state()` matches already make a
     misspelt state `E0004`;
   - the silent hazards are duplicate rows, and misspelt inputs or
     outputs that have a correctly spelt twin (for outputs, a runtime
     panic through `act`'s catch-all).

   Rephrase the verdict's "two silent DSL hazards" to match.
3. **S1 (should): use the crate's custom state types.** This changes LoC
   by ~40 lines (~3%).
   - Add `#[state_machine(state(spec::pointer::StateName))]`, and the
     same for wrap, polkit and transfer. Lock's tables take
     `state(spec::lock::LockPhase)` and `state(spec::lock::DelayPhase)`.
   - `state()` becomes `*self.fsm.state()`; for lock,
     `StateName { lock: *self.screen.state(), delay: *self.delay.state() }`.
   - This removes the five mapping matches (`pointer.rs:263–272`,
     `wrap.rs:311–318`, `polkit.rs:273–282`, `transfer.rs:268–281`,
     `lock.rs:273–287`) and makes a misspelt state an error at the row.
   - Install keeps its generated `State`, because of the deferred copies.
   - The probes then name `StateName`, `LockPhase` and `DelayPhase`
     variants.
4. **S2 (should): rewrite the hand-impl paragraph** in §1 as in finding
   3.3: data-carrying associated types are allowed, and a hand impl would
   inherit impl-enum's 7 rejects at the price of double evaluation,
   config in each state or input, and cloning transfer's frames. Drop
   "would have to be a `Vec`" and "written twice". In §4, say that the
   0 rejects belong to the DSL.
5. **S3 (should): qualify the speed ratio.** Mark the verdict's figure as
   smoke-only, about 1.2–1.6×, or replace it with the phase-3 numbers.
6. **S4 (should): `act()`'s catch-all `unreachable!`.** Record in NOTES
   §5 or §6 that it turns a misspelt or newly added output into a runtime
   panic. Matching exhaustively on `action`, with `let Event::… = event
   else { unreachable!() }` in each arm, would make it a compile error.
   Either do that or record the trade-off.

## 7. Overall

The implementation represents rust-fsm fairly, with two corrections. It
uses the DSL the way the crate intends: every state change is a table row
driven by `consume`, outputs name actions, guards are split inputs, and no
shadow enum does the work. All six targets pass conformance and the
5000-case differential runs, and every smoke digest matches impl-enum. Hand
code covers what the DSL can't hold: payloads, data, guards, hierarchy,
regions, and entry and exit actions. Each file's header and NOTES label it
plainly, and the compile-fail split (21 refused by the table, 11 by the
implementer's code, 4 accepts) is honest. Two places are too harsh on the
crate in ways that matter for the ranking:

- The size metric for pointer, wrap and transfer includes nesting
  padding (M1).
- The "misspelt states are silent" hazard ignores the crate's custom
  alphabet types and the implementation's own exhaustive matches (M2).

Once M1 and M2 are fixed, the slot can be ranked. S1 is a small under-use
worth taking for concision.
