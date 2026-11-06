# Phase 3b: measurements of the eleven implementations

Measured commit: **`f603466695ae8c3b8214493fa17848dbb6b86a22`** ("Phase 2: crate
implementations") of the scratch workspace (copied here as `../`), a clean tree apart from
`results/`. Measured on 2026-11-04. `run.sh` reruns everything in order; every cell below traces to
a file in `raw/` (the section texts name them). `tables.md` is the generated tables alone
(`tools/tables.py`); this file adds the notes in `tools/notes.md` (`tools/assemble.py`).

**Environment** (`raw/env.txt`):

| Item | Detail |
|---|---|
| Toolchain | rustc 1.99.0 (b940084d7 2026-09-28), LLVM 21.1.8; cargo 1.99.0 (5f94df478 2026-08-27) |
| Host | runnymede, Linux 7.3.1 (NixOS), AMD Ryzen 7 9800X3D, 8 cores / 16 threads, boost on, governor `performance` (left as found) |
| Tools | valgrind 3.27.1, gungraun(-runner) 0.20.0, criterion 0.8.2, proptest 1.12.0, trybuild 1.0.122 |
| Builds | the dev-shell wrapper: `CARGO_BUILD_JOBS=8`, dev without debuginfo, `CARGO_INCREMENTAL=0` (`sme.py compile` sets 1); a scratch target dir, `sme-eval` |
| Wall-clock | `taskset -c 3` (SMT sibling: CPU 11), under `flock -o ~/.cache/hotaru-runnymede-benchmark.lock` |
| Other load | other sessions: a `llama-server` (half a core, later one), `qemu` VMs (more started during criterion run 2), a large cargo build at the end of run 2, browsers (`raw/*-load.log`) |

The instruction counts are the primary performance number. Read them together with "Build
sensitivity" below. The ratios to enum move by up to 1.45x with how the bench crate is composed,
and one slot (sml on wrap) moves by 2.5x with the LTO settings Sekka ships.

## Summary

The LTO columns come from the second instruction build ("With Hotaru's release settings" below):
`lto = "thin"`, `codegen-units = 1`, as Sekka's `apps/hotaru/Cargo.toml` ships.

## Correctness

No correctness failure: every slot passes all 105 traces, all six differential tests at 20 000
cases (`raw/proptest-cases.txt` confirms 20 000 cases per test; up to 200 events each), its
trybuild cases, and every smoke row says `same as enum` on both the full and the
instruction-sized workloads. `cargo test --locked --release` passed for all eleven crates
(`raw/exit-codes.txt`).

- impl-enum is the reference, so it has no differential test; impl-implicit has none either (it
  keeps the confirmed bugs). impl-implicit fails exactly its 16 confirmed-bug traces and passes all
  105 with `--features fixed`.
- impl-implicit's smoke digests differ from enum's on six of nine workloads ("differs (bugs kept)",
  which the smoke binary expects). Its speed rows measure a machine that does other work on those
  workloads, so they are not comparable; the tables label it "bugs kept".
- impl-sml's 19 tests include its six `tests/crate_findings.rs` pins of crate behaviour, and
  impl-finny's 14 one unit test (`the_pressed_release_transition_runs_from_hovering`, the
  unguarded `execute_transition`).

## Compile-fail matrix

The classes follow IMPLEMENTING.md's "Transitions and forging": a probe's class is what the
transition API allows (`handle` and the typed transitions or generated dispatch), and forging
(building a machine in a chosen state, writing state or context data, taking an edge without its
guard and action) never decides it. "A transition stands for its event" makes a typed
"X without E" probe `reject` when every typed path to X goes through a transition named for E or
demanding E's result. The forge column holds one case per target in which a forging means stays
public because the crate generates it public. The reviews are in
`../../reviews/<slot>.md`
(section 2); each slot's `impl-<slot>/NOTES.md` section 4 records what it hid.

Cells that need explaining:

- **Baselines.** In phase 2, enum's W1, W5, T3, I4 and I5 and implicit's W1, W5, K1, T3 and I5
  moved from A to T: they had been accepted only by writing public fields, which is forging. Their
  remaining 7 R (enum) are data shape only (P2-P6, L6, I6). typestate's L4 moved from T to R
  (`resumed` consumes the `SleepPending`). implicit's 15 A are the current code's shape.
- **statig 7/29/0, no forge.** Data shape only. The 0 A and 0 forge rest on the implementation's
  private storage types: statig's safe `UninitializedStateMachine::state_mut` starts a machine in
  any state, and each target would have a forge case if its storage could be built from outside
  (NOTES §4; review §2).
- **smlang 7/29/0, no forge.** P5 moved A to R once the hover became state data (review §2,
  fix 1). W1, T3, I4 and I5 moved A to T: they went through the generated `new_with_state` and
  `context_mut`, which is forging. Those stay public in smlang. Here they are hidden in a private
  module per target, at about 11 lines each (NOTES §4).
- **sml 5/31/0, no forge.** P5, L6 and W1 are T because composite states carry no payload, so the
  data is context. I4 moved A to T, and the L1 hole closed, once the internal events got
  private-field payloads (review §2). The state setters need `TestingPolicy` and a context value,
  and every context's fields are private.
- **finny 0/32/4, no forge.** P5, P6, W1, W5, T3, I4 and I5 moved A to T once the wrappers stopped
  exposing `fsm_mut()` (review §2, fixes M1 and M2). The 4 A (P2, P4, I6, L6) are state structs
  that outlive their state, read through `get_state::<S>()`. The "no forge" is the wrappers'
  doing. A finny frontend held directly, which is finny's normal use, exposes `current_states`,
  every state struct, the context and an unguarded `execute_transition` per row (NOTES §4).
- **fsmentry 23/10/3, forge in 5 targets** (`F-as-mut`: `AsMut` on every data state's handle;
  wrap has no data states). W3 moved T to A: one edge per state pair, so `settle` stands for eight
  causes (review §2, fix 4). Event-named edges, split nodes and proof tokens (review fixes 5-7)
  moved K3, K6, T1, T4, T6, I1 and I4 from A to R. K5 and T3 also came out R, beyond the review's
  expected table (NOTES §4). W1 and L1 stay A.
- **statum 32/4/0, forge in all 6** (`F-transition-with`). The 12 former A went through builders or
  `CanTransition*`, which is forging, and are R against the `#[transition]` methods (review §2).
  L1 (its own `SleepPending` state) and L4 are R, as in typestate. The seals close fresh
  construction and data writes at a cost of 143 lines. A sealed payload can still be moved out of a
  donor machine's `pub state_data` and passed to `CanTransitionWith`/`transition_map` (NOTES §1, §4).
- **state-machines 26/10/0, forge in 5 targets** (`*_data_mut`/`with_*_data`; wrap has no data
  states). P6 and W1 moved A to T: they had gone only through `new_init_state`, which is forging and
  is now hidden in a private module. P1, L1 and I4 moved to R under "a transition stands for its
  event", and T6 moved to R after splitting `Asking` (review §2; NOTES §4).
- **rust-fsm 0/32/4, forge in all 6** (`F-from-state`). The DSL's states and inputs are fieldless
  enums and `consume` takes any input in any state, so there is nothing to reject. W1, W5 and T3 are
  T only because the implementation's fields are private (review §2). P2, P4, P6 and I6 are A:
  the data beside the table is readable, and stale. `from_state` sits on the type that carries
  `consume` and can't be hidden without hiding the table (NOTES §4).

## Lines of code

`cargo fmt --all --check` passed, so the slots were counted as they are (no formatted copy).
statum's seals are 143 of its 2 328 lines (NOTES §1); finny's 34 shared lines are its frontend
type aliases (array and null queues) and the fixed effect buffer.

## Sizes

finny holds every state struct at once plus per-level effect buffers (NOTES §3).
state-machines holds the wrapper's bookkeeping, an `Option` slot per data state and a `Vec`
effect buffer. sml's pointer and polkit carry a one-slot defer queue (440 and 248 bytes with the
default 16-slot queue). smlang's install carries its buffer (24 bytes).

## Compile time

Spreads are small: across three repetitions at most 0.13 s on crate-only (sml, dev), 0.19 s on
incremental and 0.32 s on clean (finny, dev). Crate-only (the crate alone, its dependencies
built) and incremental (a touch, then a rebuild from the incremental cache) are what a developer
pays while editing a machine. Clean adds the crate's proc macros and their `syn`. The baselines
depend only on `spec`, so their clean time is close to their crate-only time. The load average
stayed between 1.5 and 2.1 during the six repetitions.

## Instruction counts, the primary number

How the stub was measured: with every slot built, no slot is a stub. So the harness overhead was
taken from a second build with every crate slot off (`--no-default-features --features iai`).
There each crate row is a placeholder that returns at once and only drops its input. All eight
placeholders agree to the instruction (190 for one stream; 984-1 415 for a replay set, which drops
one `Vec` per trace).

**Build sensitivity.** The same source measures differently in differently composed bench crates.
Callgrind shows why (`raw/callgrind/<build>.<bench>.<impl>.out`, `callgrind_annotate`):

- enum's pointer-stream is 2 445 417 Ir in the all-slot build. There
  `<impl_enum::pointer::Pointer as Machine>::handle::<Digest>` and `Digest::push` stay out of line,
  called from `drive::pointer::run` (1.22 M + 0.44 M + 0.14 M Ir). In the stub build both are
  inlined into `run`, giving 1 894 090 Ir: 29% fewer for the same code.
- Across the ten default-profile builds, enum's own count per unit ranges 29% on pointer-stream
  (94.7-122.3), 15% on replay-pointer, 21% on replay-polkit, 20% on replay-lock and 10% on
  replay-transfer. typestate's ranges are similar (31% on pointer, 30% on polkit). Neither moves
  on the wrap streams, replay-wrap or replay-install. implicit's counts move by up to 36%, its wrap
  streams included.
- The slots' own counts are steadier: within 9% of the all-slot build, except statum replay-lock
  (+21% solo) and smlang replay-lock (-12%). So the ratio to enum moves mostly with the reference.
  It shifts by up to 1.45x (statum replay-lock 1.88 in the all-slot build against 2.73 solo;
  statig pointer 1.25 against 1.62; fsmentry pointer 1.24 against 1.51).
- With thin LTO and one codegen unit (Hotaru's release settings), cross-crate calls can inline.
  enum drops to 85.6 Ir per pointer event. **sml's wrap falls from 2.46x to 0.98x enum.** In the
  default build its cost is out-of-line guard and action callbacks plus a virtual `Digest::push`
  per effect through its `&mut dyn Sink` context: `Digest::push` alone is 7.1 M of 32 M Ir. In the
  LTO build all of it inlines into `run` (12.7 M Ir). fsmentry's out-of-line edge methods (its
  NOTES §1) matter less: 1.24x against 1.31x on pointer, about 1.0x on wrap in both builds. Every
  other slot's wrap ratio stays within 0.2x between the two builds (1.8x to 5.5x).

The default-profile all-slot build is the brief's measurement and the tables above report it
first. For a Sekka app built like Hotaru, the LTO table is the closer estimate.

## Wall-clock (criterion), the secondary number

Run 1 started at once (load 2.84). Run 2 waited one 3-minute poll (load 6.87, then 2.03), and
then other sessions' load rose under it. A VM started, and during replay-lock and replay-install a
large cargo build ran: the load went from 6 to 23, and CPU 11, the sibling, was 61% and 100%
busy. 40 of the 99 results differ by more than 10% between the runs: 7 of the 33 stream results
and 33 of the 66 replay results.

- The streams are usable, with care:
  - pointer-stream agrees within 4% between the runs for every implementation except sml (+16%).
  - wrap-80 agrees within 10% except rust-fsm (-16%).
  - Run 1's wrap-200 ran at load 7-9 with CPU 11 54% busy. Five of its results are 11-23% faster
    in run 2.
- The replays are not. Even enum's replay-pointer moved by 43% between runs (12.9 against
  7.4 ns/event).
- criterion's 95% intervals come from a single run's samples. They are far narrower than the
  run-to-run differences, so they understate the uncertainty.

The LTO suite ran once, after a 12-minute wait for the load to fall: 4 polls, from 30.7 to 1.7.
Its streams ran at load 1.7-3.9 with CPU 11 at most 10% busy, the quietest of the three suites.
It confirms the LTO instruction counts on wrap: sml at 1.03x enum and fsmentry at 1.00x, against
1.79x and 1.07x for wrap-80 in default run 1. The other slots keep their run-1 wrap-80 ratios
within 0.3x, except rust-fsm: 1.98x in run 1, 1.55x in run 2 and 1.49x here, against 1.95x in
instructions in both builds. Its replays share the replay caveats.

The wall-clock and instruction rankings differ in places for two reasons: the timed binary inlines
differently from the counted one, and IPC differs between implementations (see "Cross-check" and
the anomalies).

## Load during the measurements

`ps`'s `%CPU` in the logs is each process's average over its lifetime, not its current use. The
`/proc/stat` busy shares are the measured interference: CPU 3 near 100% while a benchmark runs on
it, and CPU 11 (its sibling) at the share shown.

In run 1 the load ranged 1.4-8.8 and CPU 11 was 1-56% busy per group. In run 2 the load ranged
2.0-23 and CPU 11 was 9-100% busy. The compile repetitions and the default instruction runs saw
loads of 1.5-3.4. The solo and LTO instruction builds ran between run-1 groups under the same lock
and never overlapped them. Instruction counts don't depend on load.

## Anomalies

1. **Build sensitivity of the primary number** (see "Instruction counts"). Ratios to enum on
   pointer-stream, replay-pointer, replay-polkit and replay-lock move by up to 1.45x with the
   composition of the bench crate, mostly because the reference's `handle` is inlined into the
   driver in some builds and not in others. With thin LTO and one codegen unit, sml's wrap moves
   from 2.46x to 0.98x enum. Rankings within about 1.3x of each other on those workloads aren't
   robust. The wrap streams are robust for every crate slot except sml.
2. **The timed binary isn't the counted binary** (see "Cross-check"). In criterion's binary statum
   runs 132.8 instructions per pointer event, against 180.0 in the instruction bench. finny
   (226.8 against 245.8), fsmentry (143.5 against 151.5) and implicit (79.5 against 107.5) differ
   too. The wrap streams agree within 2%.
3. **Instruction throughput differs by 2x between implementations** on the same workload: 12.5 to
   27.0 instructions per ns (IPC times clock) on pointer-stream. So wall-clock ratios can invert instruction ratios: statum runs
   pointer-stream at 0.72x enum's time with 1.08x its instructions in the same binary.
4. **Replay wall-clock** times one replay set (94-144 events, 0.4-5 µs) thousands of times on
   identical input, so branch predictors can learn it. Its ratios disagree with the instruction
   counts (statum replay-pointer: 0.51x and 0.87x enum in the two wall-clock runs, 1.39x in
   instructions). Use the
   instruction counts for the replays.
5. **impl-implicit** keeps its confirmed bugs, and its effects differ from enum's on 6 of 9
   workloads. Its performance rows are not a like-for-like comparison.
6. **Host load.** The host was shared throughout (see the load table). Wall-clock is pinned, but
   CPU 11, CPU 3's SMT sibling, was not reserved, and boost was on. Run 2's last two replay groups
   ran beside another session's large build (CPU 11 61% and 100% busy).
7. **Replays include construction.** Each replay builds a fresh machine per trace and drops the
   trace's events, so finny's and state-machines' larger construction cost (NOTES §3 of each) is
   in their replay numbers. The harness's own drop is subtracted (the stub).
8. **Compile time** was measured with 8 jobs, unpinned, and dev builds without debuginfo (the
   wrapper's `CARGO_PROFILE_DEV_DEBUG=0`). Debuginfo builds would be slower for every slot.
