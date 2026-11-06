# Crate agent brief (phase 2)

You are one of eight agents, each implementing six of Sekka's state machines
with one Rust state-machine crate, so the crates can be ranked by expected
value (performance, correctness, expressiveness, concision, maintenance
risk). Your result feeds a comparison, so honesty matters more than a green
run: a workaround or an inexpressible trace you record is a finding, not a
failure.

Workspace: `../eval/` (its own Cargo workspace, edition 2024,
committed on `main`). **Start by reading `../eval/IMPLEMENTING.md`
in full**; it is the contract: slot rules, build commands, what each target
must export, compile-fail case layout, NOTES.md sections, LoC rules. Then read
each target's `spec/src/<target>/SPEC.md` before implementing it, and the
worked example `impl-statig/src/pointer.rs` plus `impl-enum/src/<target>.rs`
(the reference).

Background on your crate: the survey at
`../survey.md`
(find your crate's `###` section). The survey was written from docs and
source reading, not from use; check its claims against the crate source in
`~/.cargo/registry/src/*/<crate>-<version>/` and record where it was wrong.

Rules that matter most (IMPLEMENTING.md has the rest):

- Edit only `impl-<slot>/` and `compile-fail/cases/<slot>/`. Seven other
  agents are editing their own slots in the same workspace right now. Do not
  touch `spec/`, the baselines, `bench/`, `compile-fail/src`, `metrics/`, the
  workspace `Cargo.toml`, `Cargo.lock` (except via the shared `flock`
  procedure in IMPLEMENTING.md), or other slots. Do not commit.
- Every cargo command goes through the wrapper with your own target dir:
  `CARGO_TARGET_DIR=<scratch>/t/sme-<slot> ../eval/isolate default <scratch> bash -c 'cd docs/state-machine-crates/eval && CARGO_BUILD_JOBS=4 cargo … --locked …'`.
  Python: `python3`, from the root flake's default dev shell.
- Use the crate's idiom (its states, superstates, guards, actions,
  entry/exit, state data, generated handles). A hand-written enum behind a
  thin crate facade defeats the comparison. Where the crate cannot express
  something, do the smallest honest workaround and record it.
- No `unsafe`, no extra dependencies, no threads/async/timer runtimes, no
  full benchmarks (smoke and sizes only). Never weaken a test.
- If another slot's breakage stops your build (a manifest that no longer
  parses), wait a minute and retry; build `bench` with
  `--no-default-features --features <slot>`.

Finish with, in this order:

1. `cargo fmt -p impl-<slot>`; `cargo test --locked -p impl-<slot>` (all
   tests); `SME_PROPTEST_CASES=5000 cargo test --locked --release -p impl-<slot>`;
   the bench smoke and sizes for your slot.
2. `impl-<slot>/NOTES.md` with every section IMPLEMENTING.md lists.
3. A final report to me (under 400 words): per target, conformance N/M and
   whether the differential test passes (or which are ignored/not
   expressed, and why); compile-fail tally (reject/runtime/accept/n.a.);
   smoke result per workload; sizes; the three most important findings
   about the crate (what it did well, where it fought you, any bug in the
   crate); and your verdict. Quote exact numbers from command output, not
   estimates.
