# Relax compiler follow-up

This follows the [initial pinned experiment](../2026-11-11/README.md).
It fixes the inner-macro resolution and enum-cast failures that blocked the
compositor check, plus the process-heap test failure. Focused probes also found
cached macro-scope loss, incorrect file-module macro ordering, and a scheduling
test that assumed privileges its process did not have.

## Revisions and environment

- Final Relax integration: `d09dcd17fa766b42e2f304fcdecfb489b2d00078` on
  [codex/sekka-compat-followup](https://github.com/bitemyapp/relax/tree/d09dcd17fa766b42e2f304fcdecfb489b2d00078).
  The base is `ba98a6c859fb06950b9939a222ed39fb268d30ba`, with the earlier dylib
  source-graph and unnamed-mapping fixes carried forward alongside these fixes.
- Sekka Rust sources and lockfiles remain at
  `7dcf88e3b7f3391ec498a53a28555d02ba8fb14a`. The experiment branch adds only
  development-shell, harness, and evidence/documentation changes.
- Final combined checks run on `wx-workstation`, Linux x86-64, NixOS 26.11,
  kernel 7.3.1, 128 logical CPUs and approximately 250 GiB RAM. A brief host
  outage interrupted the first independent cache test; that proof was completed
  locally, and final combined validation resumed on the workstation after reboot.
- Commands use the root-locked `nix develop --no-update-lock-file .#relax`
  environment and Rust 1.99.1. Cargo jobs and the final experiment's Relax CPU
  budget are limited to eight. Independent tests explicitly identify their host
  boundary in the PR and accompanying logs.

The remote follow-up directory is `$HOME/sekka-work/relax-followup-20261111`.
The [evidence index](evidence/index.json) records raw and stored SHA-256 hashes.
Logs are normalized for workspace paths and compressed with deterministic gzip;
JSON log links point to the stored files. The packaging includes completed
independent proofs and final results, excluding build artifacts and caches.

## Atomic fixes

Each fix was prepared in a private Makepad source checkout and then carried to
the Relax projection. These are separate reviewable PRs, not merges.

| PR | Cause and fix | Focused validation |
| --- | --- | --- |
| [#15](https://github.com/makepad/relax/pull/15) | Enum-to-integer casts read the enclosing enum's `non_exhaustive` attribute instead of the variant's | Cross-crate signed/unsigned casts pass; a foreign non-exhaustive variant still rejects with E0606 |
| [#16](https://github.com/makepad/relax/pull/16) | Packed demand metadata discarded module declaration positions, visibility, and outer `macro_use` flags | The new round-trip scope test fails on the baseline and passes with the fix; 101 frontend tests pass |
| [#17](https://github.com/makepad/relax/pull/17) | The scheduling test assumed UI promotion from inherited `SCHED_IDLE` always succeeds | All 26 CPU-budget tests pass; explicit normal/idle runs pass, while the old test fails under idle |
| [#18](https://github.com/makepad/relax/pull/18) | Lazy process-wide JIT worker startup occurred inside the domain heap measurement | Independent exact-branch test passes; patched remote runs pass at CPU budgets 1, 8, and 128 |
| [#19](https://github.com/makepad/relax/pull/19) | Inner `#![macro_use]` was missing from module scans, demand imports, and persisted metadata | Independent runner and 101 frontend tests pass; child-only edits match cold-cache diagnostics; positive and rejection fixtures pass cold and warm |
| [#20](https://github.com/makepad/relax/pull/20) | File-module macro lookup ignored declaration order and shadowing across files | Independent runner and 100 frontend tests pass; direct/expanded calls, nested export ties, earlier-call rejection, and cold/warm repeats match rustc |

The two independent demand-cache fixes use distinct formats: v10 for module
items and v11 for inner attributes. The combined integration uses v12 because
its payload contains both; the version and hash domain change together. This
prevents reuse of a different branch's incompatible metadata. Crate-image
format v27 is shared by the inner-attribute variants because that layout is
identical.

The heap failure was a baseline error, not evidence of retained application
domains. Allocation-site diagnostics identify the one-time worker-pool budget
and Rust native thread setup allocations. The existing per-domain reclamation
checks already passed. The fix waits for worker thread registration before
sampling process heap; it keeps all 20 cycles and the 256 KiB tolerance and is
compiled only for Linux tests.

## Final results

| Check | Result | Evidence |
| --- | --- | --- |
| Complete Relax default workspace suite, release profile | 2,255 passed, 0 failed, 11 ignored | [Log](evidence/followup-remote/final-workspace.log.gz) |
| Sekka component tests | All 184 tests in 12 packages pass under both rustc and Relax | [Results](evidence/followup-remote/final-components/results.json) |
| Reduced regressions | 9 executed tests per compiler; all 5 expected rejections agree | [Results](evidence/followup-remote/final-regressions/results.json) |
| Compositor check, cold then warm | Both rustc and Relax pass both invocations | [Results](evidence/followup-remote/final-compositor/results.json) |
| Relax eager compositor check | Passes with `MAKEPAD_RUN_CHECK_EAGER=1` | [Results](evidence/followup-remote/final-compositor-eager/results.json) |
| Harness against the initial compiler pin | Original 6 executed tests and 3 rejection fixtures still pass | [Results](evidence/followup-remote/backcompat-regressions/results.json) |

The final native workspace run includes all 376 compiler, 102 frontend, and
815 type-system tests. In particular, the domain teardown test and the
inherited-idle scheduling test both pass. The 11 pre-existing ignored tests
are not counted as passed. The [metadata](evidence/followup-remote/final-metadata.log.gz)
records the exact runner, harness, and generated compiler lockfile hashes;
each comparison also records the runner hash and unchanged Sekka lockfiles.
The [workspace count](evidence/followup-remote/workspace-summary.json) uses each
Cargo target's final summary, excluding 18 nested isolated-test summaries that
would otherwise count the same tests twice.

No dependency-source workarounds or socket-test exclusions are applied.
`--allow-c` still uses Cargo/rustc and native tools for build dependencies.
A successful compositor check establishes Rust analysis for that target, not
native executable emission, a running desktop, an ISO build, VM installation,
or physical hardware qualification. The normal Sekka build and release
publication settings are unchanged.

## Buck2 comparison

The compatibility boundary has moved: the compositor now passes Relax's check.
The earlier statement that macro imports and enum casts block that check applies
only to the initial pinned compiler.

The [historical Buck2 comparison](../2026-11-11/README.md#comparison-with-the-buck2-experiment)
still measures a different workload: native builds, edits, cross-worktree cache
reuse, storage, and recovery. Those measurements remain stronger evidence for
Sekka's whole-build workflow. Relax now covers the component test set and
compositor analysis, but this follow-up does not measure full desktop execution,
native build outputs, cross-worktree storage, or edit performance. The earlier
tiny warm-test timing table remains tied to its original compiler revision; no
new speed ordering is claimed for these fixes.

The subsequent [cache and worktree comparison](../2026-11-11-cache-comparison/README.md)
measures this compiler revision against Buck2 + NativeLink on the same component
tests, including empty caches, warm reruns, relocated worktrees, executed source
edits, and compression-aware physical cache storage. It keeps the historical
whole-native-build figures separate from those JIT test measurements.

## Reproduce

Use a fresh output directory for every run. The harness accepts both original
and upstream-relocated rejection-fixture paths, keeping the initial compiler
pin reproducible too. It includes the new macro fixtures when present in the
selected compiler checkout and records their source hashes. The original pin
still passes its original regression set with this harness.

```sh
git -C /path/to/relax checkout d09dcd17fa766b42e2f304fcdecfb489b2d00078
nix develop --no-update-lock-file .#relax
cargo build --release --manifest-path /path/to/relax/Cargo.toml -p relax-runner --bin relax -j8
MAKEPAD_CPU_CORES=8 cargo test --release --workspace --no-fail-fast --manifest-path /path/to/relax/Cargo.toml -j8
python3 scripts/relax-experiment.py regressions --relax-root /path/to/relax --output /path/to/new/regressions
python3 scripts/relax-experiment.py components --relax-root /path/to/relax --output /path/to/new/components
python3 scripts/relax-experiment.py check --package aizome-comp --repeat 2 --relax-root /path/to/relax --output /path/to/new/compositor
MAKEPAD_RUN_CHECK_EAGER=1 python3 scripts/relax-experiment.py check --package aizome-comp --engine relax --relax-root /path/to/relax --output /path/to/new/compositor-eager
```

The individual PRs contain the reduced positive/rejection fixtures and their
independent validation commands. Their results distinguish an expected
diagnostic from a crash or an empty successful test run.
