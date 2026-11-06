# Raw outputs of the evaluation

Every command's output from `../run.sh`, as recorded on 2026-11-04 for commit `f603466` of the
scratch workspace (copied here as `../../`). `../tools/tables.py` reads these files and nothing
else. Each step of `run.sh` names the files it writes.

The files are the run's own output. Paths inside them (target directories, the workspace, the
wrapper, benchmark binaries) are the original run's scratch paths. They are kept as recorded and
don't exist in this repository.

## Run records

| File | What it is |
|---|---|
| `env.txt` | date, commit, toolchain, kernel, `lscpu`, governor and boost, valgrind and gungraun-runner versions |
| `steps.log` | start and end time of every `run.sh` step, in the order they ran |
| `exit-codes.txt` | the exit status of every command `run.sh` checks |
| `*-load.log` | load average, top 10 processes and `/proc/stat` lines for CPU 3 and CPU 11, before and after each locked command (`compile`, `smoke`, `instructions`, `criterion-run1`, `criterion-run2`, `criterion-lto`) |
| `tables-stderr.txt` | stderr of `tables.py` and `assemble.py` (empty) |

## Correctness

| File | What it is |
|---|---|
| `conformance.txt`, `conformance.jsonl` | `sme.py conformance`: summary, and one line per trace and implementation |
| `conformance-fixed.txt`, `conformance-fixed.jsonl` | the same with `--fixed` (impl-implicit with its bugs off) |
| `proptest-<impl>.txt` | `SME_PROPTEST_CASES=20000 cargo test --locked --release -p impl-<impl>` |
| `proptest-cases.txt` | the case counts the differential tests ran (the same tests with `--nocapture`) |
| `compile-fail.txt` | `sme.py compile-fail --run`: the R/T/A matrix and each slot's trybuild result |
| `compile-fail-cases.txt` | the list of compile-fail case files |
| `smoke.txt`, `smoke-small.txt` | `bench --bin smoke` on the full and the instruction-sized (`SME_SMOKE=1`) workloads; `smoke-build.txt` is its build |

## Static metrics

| File | What it is |
|---|---|
| `fmt-check.txt` | `cargo fmt --all --check` (empty: formatted) |
| `loc.json`, `loc.md` | `sme.py loc --json` and `sme.py loc` |
| `sizes.tsv`, `sizes.md` | `bench --bin sizes` and `sme.py sizes` |
| `counts.tsv` | workload sizes: events per workload and printed cells for wrap (`../tools/counts`) |
| `compile.jsonl` | `sme.py compile`, every repetition (`rep`), dev and release |
| `compile-<profile>-<rep>.txt` | what `sme.py compile` printed for that repetition |

## Instruction counts (gungraun, callgrind)

| File | What it is |
|---|---|
| `instructions.txt` | the all-slot build's terminal output: the primary measurement |
| `instructions-stub.txt` | every crate slot off (the harness overhead) |
| `instructions-solo-<slot>.txt` | the three baselines and one crate slot |
| `instructions-lto-all.txt`, `instructions-lto-stub.txt` | all slots and the stub with thin LTO and one codegen unit |
| `instructions-*-build.txt` | the build of each of those |
| `gungraun-<build>.jsonl` | every `summary.json` of that build's gungraun directory, one line each: `{"path": "bench/instructions/machines/<bench>.<impl>", "summary": {...}}`, the object unchanged |
| `callgrind/<build>.<bench>.<impl>.out` | the callgrind profiles results.md and the report cite: `all.pointer_stream.enum_`, `stub.pointer_stream.enum_`, `all.wrap_80.sml`, `lto-all.wrap_80.sml`, `lto-all.wrap_80.enum_`, `lto-all.wrap_80.typestate`; read them with `callgrind_annotate` |

`<build>` is `all`, `stub`, `solo-<slot>`, `lto-all` or `lto-stub`.

## Wall-clock (criterion) and the perf cross-check

| File | What it is |
|---|---|
| `criterion-build.txt`, `criterion-lto-build.txt` | the bench builds (default, and thin LTO with one codegen unit), ending with the binary's path |
| `criterion-run1.txt`, `criterion-run2.txt`, `criterion-lto.txt` | criterion's terminal output for each suite |
| `criterion-<run>.jsonl` | every `<group>/<impl>/new/estimates.json` of that suite, one line each: `{"group": ..., "impl": ..., "estimates": {...}}`, the object unchanged |
| `perf-check.tsv` | `perf stat -e instructions:u` of one run of each stream in the criterion binaries |

## What was consolidated

The run left a criterion directory per suite and a gungraun directory per instruction build in
`raw/`: 5 940 files, 39 of its 41 MB on disk. When the results were copied into this repository,
`../tools/consolidate.py` folded each directory into the JSON Lines file above, and the directories
were dropped. Dropped with them: criterion's `base/` estimates (identical to `new/` in all 297
benchmarks), `sample.json`, `tukey.json` and `benchmark.json`, and the callgrind `.out` and `.log` files
other than the six in `callgrind/`. `run.sh` now writes the consolidated files directly; the
profiles it doesn't keep stay in its target directories.
