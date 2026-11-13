# Typestate wrap layouts

The evaluation's `impl-typestate` wrap costs 2.1× `impl-enum`'s instructions
per printed cell. This experiment finds out why and measures the
alternatives. Each variant replaces `impl-typestate/src/wrap.rs` and is
checked by that crate's wrap conformance and differential tests, then counted
with the baselines-only instruction bench (`--no-default-features --features iai`).

| File | Layout |
|---|---|
| `wrap-v0.rs` | The evaluation's: every state owns the `Screen` by value (`Normal(Screen)`); the wrapper `Any` is 48 bytes |
| `wrap-boxed.rs` | States own `Box<Screen>`; the machine holds `Option<Any>` |
| `wrap-tokens.rs` | Zero-sized state tokens; every transition takes `&mut Screen`; the screen lives beside the state |
| `wrap-borrowed.rs` | States hold the screen by `&'s mut` (`Normal<'s>(&'s mut Screen)`); the machine stores a one-byte `Tag` and rebuilds the typed state per event |
| `wrap-borrowedfast.rs` | `wrap-borrowed.rs` with `handle` routing `Print` first, plus `with_typed` (typed access for code outside the module) |

## Results

Instructions per printed cell (wrap-80, wrap-200) and per event of a replay
set (replay-wrap), as a ratio to `impl-enum` in the same build. "default" is
the bench profile, "LTO" is `lto = "thin"` with one codegen unit, as
`apps/hotaru` ships. From `logs/<variant>.log` (`parse.py`).

| Variant | wrap-80 default / LTO | wrap-200 default / LTO | replay-wrap default / LTO | Guarantees |
|---|---:|---:|---:|---|
| v0 | 2.16 / 2.10 | 2.17 / 2.11 | 1.90 / 1.84 | all |
| boxed | 1.27 / 1.24 | 1.27 / 1.23 | 1.41 / 1.32 | all |
| tokens | 1.14 / 1.14 | 1.14 / 1.14 | 1.13 / 1.13 | weaker: a token from one machine works with another machine's screen |
| borrowed | 1.13 / 1.15 | 1.13 / 1.15 | 1.12 / 1.13 | all |
| borrowedfast | 1.00 / 1.00 | 1.00 / 0.99 | 1.08 / 1.08 | all |

Every variant passes wrap's 14 conformance traces and its differential test
against `impl-enum`.

## Why v0 is slow

`Screen` is 32 bytes, including a `u128` of row links, and `Any` is 48.
Each printed cell moves it about four times:

1. `mem::replace` copies it out and writes a placeholder in.
2. The consuming transition takes it by value and returns a differently
   laid-out `Ready` enum.
3. `.into()` re-tags it as `Any`.
4. The result is written back.

LLVM can't keep it in registers through those enum conversions. Under LTO,
callgrind counts 37 data reads and 35 writes per cell against the enum's 14
and 9 (`../../results/raw/callgrind/`). In the borrowed layouts a state is
pointer-sized and the machine's state is one byte. The remaining 13% in
`wrap-borrowed.rs` was `handle` classifying every event before matching it;
routing `Print` first removes it.

## Guarantee checks

`probes/` holds programs compiled against `wrap-borrowedfast.rs` through
`with_typed`:

| Probe | Result |
|---|---|
| `ok_drive.rs` | compiles and runs: a print through the typed API is committed back |
| `w1_pending_from_normal.rs` | E0308: a `Normal` can't be relabelled `Pending` |
| `w4_normal_print_on_pending.rs` | E0308: `Normal::print` doesn't take a `Pending` |
| `w5_soft_wrap.rs` | E0599: no soft wrap outside the print transition |
| `forge_literal.rs` | E0423: a `Pending` can't be built from outside |
| `pair_other_machine.rs` | "lifetime may not live long enough": machine A's state can't be handed back to machine B |

The instruction counts were taken before `with_typed` was added to
`wrap-borrowedfast.rs`. It isn't called on the measured path.

## Rerunning

From the repository root, with a scratch directory outside it:

```sh
SCRATCH=/tmp/wrap-scratch LOGS=/tmp/wrap-scratch/logs \
  docs/state-machine-crates/eval/experiments/typestate-wrap/run.sh v0 boxed tokens borrowed borrowedfast
SCRATCH=/tmp/wrap-scratch docs/state-machine-crates/eval/experiments/typestate-wrap/run.sh \
  --probe pair_other_machine borrowedfast
```

The comment at the top of `run.sh` lists the settings: the valgrind and
gungraun-runner paths, the CPUs to pin to, `--conformance-only` and
`--probe`. It swaps each variant into the evaluation's
`impl-typestate/src/wrap.rs` and restores the file on exit. `LOGS` keeps a
rerun from overwriting the recorded `logs/`.
