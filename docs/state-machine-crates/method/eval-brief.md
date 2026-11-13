# Evaluation brief (phase 3b): measure every implementation

The workspace `../eval/` holds six of Sekka's state machines
implemented three ways by hand (`implicit`, `enum`, `typestate`) and with
eight crates (`statig`, `smlang`, `sml`, `finny`, `fsmentry`, `statum`,
`state-machines`, `rust-fsm`). The implementations are finished and reviewed.
Your job is to measure them all, reproducibly, and write the results. You
are the only agent running in the workspace now.

Read first: `../eval/IMPLEMENTING.md` (especially "For
phase 3"), `metrics/sme.py`'s docstring, `bench/benches/*.rs`, and each
`impl-<slot>/NOTES.md`.

## Commands

All cargo and Python through the wrapper, with a target dir of your own:
`CARGO_TARGET_DIR=<scratch>/t/sme-eval ../eval/isolate default <scratch> bash -c 'cd docs/state-machine-crates/eval && …'`.
Python inside it: `python3`, from the root flake's default dev shell.
Always `--locked`.

## Steps

1. **Snapshot.** `git -C ../eval status`; commit the
   finished implementations in the workspace's own repository (it is
   scratch, not Sekka) as "Phase 2: crate implementations", so results name
   a commit. Record `rustc -vV`, `cargo -V`, the kernel and CPU.
2. **Correctness.** `sme.py conformance` (all slots) and
   `sme.py conformance --fixed`; `SME_PROPTEST_CASES=20000 cargo test --locked --release`
   for every `impl-*` crate; `sme.py compile-fail --run`. Record failures
   verbatim.
3. **Static metrics.** `sme.py loc` (after `cargo fmt --check`; if it isn't
   formatted, say so and format a copy, don't edit the slots), `sme.py sizes`.
4. **Compile time.** `sme.py compile` for all slots, debug and `--release`,
   under the benchmark lock (below), 3 repetitions; report medians and
   spread. Note the crate-only and incremental columns matter most to a
   developer; clean includes the proc-macro dependencies.
5. **Smoke.** `cargo run --locked --release -p bench --bin smoke`: every row
   must say `same as enum`. Any `DIFFERS FROM ENUM` is a correctness result:
   record it and leave that row out of the speed comparison.
6. **Instruction counts** (deterministic; the primary performance number,
   because the host is shared):
   `PATH=<scratch>/valgrind/bin:$PATH GUNGRAUN_RUNNER=<scratch>/gungraun/bin/gungraun-runner cargo bench --locked -p bench --features iai --bench instructions`.
   Subtract the stub overhead IMPLEMENTING.md describes, and normalise per
   event (or per printed cell for wrap).
7. **Wall-clock** criterion benches, the secondary number:
   `cargo bench --locked -p bench --bench machines`, built first, then run
   pinned and serialized:
   `flock -o ~/.cache/hotaru-runnymede-benchmark.lock taskset -c 3 <bench binary> --bench …`
   (CPU 3; its sibling is 11). Before and after each group record
   `/proc/loadavg` and the top 10 processes by CPU (`ps -eo pid,pcpu,comm --sort=-pcpu | head`).
   If the load average exceeds 4 when you start, wait (poll every few
   minutes, up to an hour) and record that you waited. Run the whole
   criterion suite twice; report both runs and flag any result whose two
   runs differ by more than 10%.
   Do not change CPU governors or anything system-wide.

## Output

Write in `../eval/results/`:

- `results.md`: environment; correctness table (conformance per target per
  implementation, property tests, smoke); compile-fail matrix (R/T/A/- per
  probe, and totals per implementation: rejected at compile time, rejected
  at runtime, accepted, not applicable); LoC per target and shared; sizes;
  compile times; instruction counts per event per target with the ratio to
  `enum`; criterion medians with confidence intervals and the ratio to
  `enum`; anomalies. Tables first, prose short. Name the commit measured.
- `raw/`: every command's raw output (criterion's `target/criterion`
  estimates JSON copied, gungraun output, sme.py JSON with `--json` where
  supported, logs of loads), so each table cell can be traced to a file.
- `run.sh`: the exact commands, in order, so someone can rerun everything.

Final message to me, under 400 words: the headline numbers per
implementation (instructions/event on pointer and wrap, ratio to enum;
compile-fail R/T/A totals; LoC total; crate-only compile time;
conformance), any correctness failure, and anything that makes a number
untrustworthy.
