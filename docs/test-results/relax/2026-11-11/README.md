# Relax compiler experiment on wx-workstation

This continues the recovered ChatGPT experiment on the workstation with Sekka's
native dependencies available. It exercises component tests and compiler
regressions. It does not establish a working Relax-built compositor, desktop,
distribution image, installation, or physical hardware result.

## Recovered inputs and reproducibility

The `sekka-relax-experiment.zip` attachment came from the ChatGPT conversation
“Relax compile experiment”. It was downloaded to
`$HOME/Downloads/sekka-relax-experiment.zip` and unpacked
under `$HOME/work/sekka-relax-import`; all entries in its `SHA256SUMS`
passed verification. Its ZIP SHA-256 is
`92347c04d0d047e4d5980de5927763b9d9d92e4178d577c19b33042ce6e73f24`.

The archive's last verified compiler was `db020cc34744b3247fc22b05484cde208e65da68`
on upstream `7e48bc68b320c280d2d801a0af58e7c2d73889dc`. Its latest source,
`a5e17b498cbd0dbc5adff63ad67d95b6b5513372`, included two follow-ups that had not
been rebuilt end to end. The original host ran 126 tests in nine packages with
four socket tests excluded; memory and missing native dependencies prevented
the larger toolkit/compositor attempts from completing.

This continuation uses:

- Sekka `7dcf88e3b7f3391ec498a53a28555d02ba8fb14a`, also the archive's source
  revision. Component Rust source and lockfiles remain unchanged.
- Relax upstream `deaacb9c62a17fd6d932ecb784a9dbc22f0c630f` and integration
  `5339a68b8c3e7e9aadfce8deb7e9367c8b436e18` on
  [bitemyapp/relax, codex/sekka-compat](https://github.com/bitemyapp/relax/tree/5339a68b8c3e7e9aadfce8deb7e9367c8b436e18).
  This is eight independent changes carried onto that upstream base.
- `ssh wx-workstation` (host `theophrastus`), Linux x86-64, 128 logical CPUs,
  approximately 250 GiB RAM; Cargo and Relax jobs limited to eight for the
  component experiment. The root flake's Rust compiler is
  `rustc 1.99.1 (58cf8f9c3 2026-10-13)`.
- Root-locked `nix develop .#relax`: Aizome's native libraries plus OpenSSL
  for the runner, Python for the harness, and Node for the WebAssembly backend
  tests. No flake lock update, normal build routing, or publication change.

The remote experiment directory is
`$HOME/sekka-work/relax-experiment-20261111`. It retains full build
artifacts, caches, and logs; the checked-in evidence contains results and logs,
not binaries or compiler source snapshots. Evidence paths are normalized to
`$EXPERIMENT`, `$CARGO_HOME`, and `$IMPORT` when copied into this report.
Logs are gzip-compressed; JSON log links use the stored `.log.gz` names.
[evidence/index.json](evidence/index.json) records raw and stored file hashes.
The runner's generated [Cargo.lock](evidence/compiler-Cargo.lock) contains only
local workspace packages and is retained for build provenance.

The recovered CLI shim was not carried: the newer upstream has a standalone
`relax-runner`. Each compiler change was prepared in a private Makepad source
checkout and then carried to the Relax projection as its contributor guidance
requests. Each PR was built and tested independently on the pinned base before
the combined experiment.

## Atomic upstream changes

| PR | Failure and bounded change | Independent validation |
| --- | --- | --- |
| [#4](https://github.com/makepad/relax/pull/4) | Seven value-only SSE2 byte operations were incorrectly unsafe; match Rust's signatures while keeping pointer loads unsafe | All 256 byte values in 16 lanes; unguarded load still rejected with E0133 |
| [#5](https://github.com/makepad/relax/pull/5) | Generated `write!` calls selected a later unrelated local macro; resolve at the textual invocation position | Nested expansion and legitimate local shadowing both pass |
| [#6](https://github.com/makepad/relax/pull/6) | A function re-export falsely collided with an exported macro; resolve the imported macro namespace, including leading `::` | Positive macro/value calls pass; both real built-in collisions still reject with E0255 |
| [#7](https://github.com/makepad/relax/pull/7) | Importing a public macro accidentally imported a private same-named module, hiding a dependency's attribute macro | Dependency-free three-crate fixture passes both tests, including a valid public module alias |
| [#11](https://github.com/makepad/relax/pull/11) | Config/generated-source-only build scripts repeated Cargo preparation because `linked_libs` existed but was empty | 100 frontend unit tests; observed Cargo checks go from cold/warm 1/1 to 1/0 |
| [#12](https://github.com/makepad/relax/pull/12) | Launcher tests required FHS paths absent on NixOS; find `sleep`/`env` through PATH | All 67 remote library tests pass, versus 64 passes and three ENOENT failures before |
| [#13](https://github.com/makepad/relax/pull/13) | Rust `dylib` dependencies disappeared from the source graph | Dependency-free check, `--also`, JIT execution, app test, and library test all pass |
| [#14](https://github.com/makepad/relax/pull/14) | Allocator test treated unavailable mapping labels as measured zero RSS | Survivor data and accounting pass; unsupported RSS comparison emits a diagnostic |

During this run, the maintainer incorporated and closed six PRs:
#4 ([`385052c3`](https://github.com/makepad/relax/commit/385052c347a0eac00bc9ef4080f8220657cc71c7)),
#5 ([`f66b3c07`](https://github.com/makepad/relax/commit/f66b3c07cf55a5caf67d0594c58aa273302d9263)),
#6 ([`133bf3f4`](https://github.com/makepad/relax/commit/133bf3f4dee7030c517591dd59b887f0f3f65d4e)),
#7 ([`570a81ec`](https://github.com/makepad/relax/commit/570a81ecd09147fb3145f613f0833b25c47c5e6f)),
#11 ([`d5525cdc`](https://github.com/makepad/relax/commit/d5525cdc26e9109d7ecb4768b29ac367e8039427)), and
#12 ([`1cba9969`](https://github.com/makepad/relax/commit/1cba99699884e7d4282f18aefee9824e09df8a5b)).
The upstream versions include further changes (notably a broader intrinsic
audit); this report tests the pinned integration, not the moving upstream tip.
PR #13 checks/JIT-compiles Rust source; it does not implement native DSO
emission/loading or change `cdylib`/`staticlib` semantics.

The cache fix validates complete build-script metadata, existing output
directories, and package/feature variants. Malformed or incomplete metadata and
real native link requirements remain conservative. Deleting generated output
causes another Cargo attempt and an explicit failure in the probe: Cargo does
not automatically regenerate that deleted output here. Successful recovery is
not claimed.

## Component results

All **184 tests in 12 packages pass under both compilers** on the final
integration revision. [results.json](evidence/final-components/results.json)
records matching counts, unmodified lockfiles, and the runner SHA-256.

| Package | rustc release | Relax |
| --- | ---: | ---: |
| hotaru-sprites | 13 | 13 |
| hotaru-core | 42 | 42 |
| calamares-session-env | 5 | 5 |
| aizome-ipc | 5 | 5 |
| aizome-theme | 2 | 2 |
| aizome-config | 31 | 31 |
| aizome-shell-proto | 4 | 4 |
| respin-tools | 19 | 19 |
| hotaru-link-regex | 9 | 9 |
| hotaru-io | 3 | 3 |
| aizome-kit | 40 | 40 |
| aizome-soroban | 11 | 11 |
| **Total** | **184** | **184** |

The harness compares unit/integration targets explicitly, not doctests. Socket
tests run without exclusions. `--allow-c` invokes Cargo/rustc and native tools
for native build dependencies; these are not wholly rustc-free builds.
Successful tests establish execution for covered paths, while Relax checking
does not imply every function was emitted as machine code. Component wall times
include different dependency preparation and are not used as performance claims.

The final compiler also passes six positive reduced regression tests and three
expected-rejection fixtures against rustc
([results](evidence/final-regressions/results.json)), all five dylib fixture
commands, and the native-cache probe
([results](evidence/final-cache-probe/results.json)). Focused frontend and type
suites pass **100** and **815** tests respectively.

The complete Relax workspace suite is **not green**. It passes the remote
launcher, WebAssembly backend, and allocator groups, then stops in
`relax-compiler`: **374 passed, one failed**. The failing
`domain::teardown_tests::teardown_fixture_cycles_and_retiring_thread` sees a
roughly 579 KiB step in process-wide heap usage, exceeding its 256 KiB tolerance.
The same test fails in an isolated rerun and on unmodified upstream `deaacb9c`.
Its domain-reclamation assertions pass before the process-heap assertion fails;
the cause of the retained process memory remains unresolved. No tolerance was
relaxed and no later workspace test groups are claimed as run.
See [full run](evidence/logs/integrated-build-tests.log.gz),
[focused tests](evidence/logs/integrated-focused-tests.log.gz), and
[upstream reproduction](evidence/logs/upstream-teardown-test.log.gz).

This host has `CONFIG_ANON_VMA_NAME` disabled. The separate burst-survivor test
now verifies survivor data and accounting while explicitly reporting that its
RSS comparison is unavailable. It is not evidence for the RSS assertion on a
kernel supporting named mappings.

## Remaining compositor boundary

The native `cargo check --locked --release -p aizome-comp --bin aizome` passes.
The Relax equivalent remains blocked. The Rust `dylib` source-graph fix removes
one reduced failure, but cgmath uses `#![macro_use]` inside a module. Relax's
parent prescan does not publish those macros to the parent in the required
textual scope. Both the pinned unmodified and patched compiler reproduce it.

[fixtures/inner-macro-use](fixtures/inner-macro-use/probe.rs) contains the small
reproducer, including inline and file-module variants. Rustc accepts it; Relax
cannot resolve the generated calls. Moving the attribute to the parent module
declaration works in the probe. No dependency-source workaround is applied to
Sekka. A compiler fix needs consistent handling across prescans, serialized
imports, demand loading, inline scopes, and child-only edit invalidation, so it
is left as an explicit next investigation.

The final [paired check](evidence/final-compositor/results.json) records rustc
exit 0 and Relax exit 1. Relax reports 561 unresolved imports (the first shown
are cgmath macros) and five E0606 enum-cast diagnostics: four for Wayland
`handle::State` and one for `ClockId`. Those additional diagnostics have not been
reduced to independent causes. Its partial count of 8,243 checked body owners is
not a successful compositor result.

## Comparison with the Buck2 experiment

The systems address different costs. Buck2 uses rustc and shares content-addressed
build outputs across invocations and worktrees. Relax performs its own Rust
analysis and JIT compilation, with a native Cargo bridge where required.
This experiment validates a useful component subset under Relax, not parity with
the native desktop build.

The historical Buck2 reference is the
[31-stage final storage run at `7fef66a8`](https://github.com/bitemyapp/sekka/blob/7fef66a8ae5b3f592194dd2104b1afa354ca6dfb/docs/test-results/buck2/2026-11-06-final/README.md),
on the same workstation, with all gates passing. Its full workload recorded:

| Buck2 observation | Historical result |
| --- | ---: |
| Development cold build | 83.6 s |
| Warm no-op | 0.21 s |
| Added warm worktree | 4.6–5.3% of cold time |
| Added physical storage per worktree | 14.0–14.8 MB |
| Leaf / shared / icon edits | 1.6 / 15.8 / 16.3 s |
| Final compressed physical union | 20.16 GB |

Those full-build and compressed storage measurements cannot be compared to the
small Relax test totals or ordinary `du` output. Relax cross-worktree sharing,
edit invalidation performance, full-build storage, cache eviction/recovery, and
remote execution were not measured here. Buck2's historical evidence covers
edits, cross-worktree reuse, storage, and recovery much more broadly; it also is
not ISO qualification.

A smaller paired probe uses `aizome-theme` at the two pinned Sekka revisions.
Its manifest, all Rust sources, inherited workspace metadata, and transitive
locked dependency records match exactly. Each command runs the same two tests.
After one priming invocation per engine, five interleaved fresh-process
invocations give the following elapsed wall times:

| Warm theme test invocation | Median | Range |
| --- | ---: | ---: |
| Cargo, release profile | 0.090 s | 0.089–0.090 s |
| Relax, normal JIT test command | 0.116 s | 0.110–0.131 s |
| Buck2, dev profile, local cache | 0.276 s | 0.273–0.282 s |

[Comparison results](evidence/warm-comparison-exact-wait/results.json) include
commands, hashes, revisions, every sample, and test-count validation. These are
warm command-to-completion latencies, mostly driver overhead on a tiny test
target. Profiles and compiler execution models differ. They measure neither
cold compilation nor source edits, whole-workspace throughput, or runtime
performance. Cargo is quickest for this specific warmed invocation; Relax is
also below Buck2's invocation cost here. That does not establish a general build
speed ordering.

The final timing harness blocks until child exit with a watchdog for timeouts;
metadata validation happens outside the timed interval. An earlier probe used
Python's polling timeout wait, which quantized short runs by up to 50 ms; those
preliminary timings are retained remotely and excluded from this table.

For the current Sekka workflow, Buck2 has the stronger evidence for whole-build
reuse and multi-worktree storage. Relax is a viable experimental path for the
passing component tests, with real compiler compatibility gaps still preventing
the compositor check. These results do not support replacing the native build
with Relax or claiming a general compilation speedup.

## Run the experiment again

From the Sekka root, use the pinned integration commit:

```sh
git clone https://github.com/bitemyapp/relax.git /path/to/relax
git -C /path/to/relax checkout 5339a68b8c3e7e9aadfce8deb7e9367c8b436e18
nix develop .#relax
cargo build --release --manifest-path /path/to/relax/Cargo.toml -p relax-runner --bin relax -j 8
python3 scripts/relax-experiment.py regressions --relax-root /path/to/relax --output /path/to/new/regressions
python3 scripts/relax-experiment.py components --relax-root /path/to/relax --output /path/to/new/components
python3 scripts/relax-experiment.py check --package aizome-comp --relax-root /path/to/relax --output /path/to/new/compositor
python3 scripts/relax-native-cache-probe.py /path/to/relax/target/release/relax /path/to/relax/libs/relax/compiler/std /path/to/new/cache-probe
```

The compositor command is expected to return nonzero until the remaining
compiler gap is fixed. The dylib fixture has its own runner:
`bash /path/to/relax/libs/relax/compiler/tests/rust_dylib/run.sh /path/to/relax/target/release/relax`.
Use a fresh output directory for each compiler revision. The harness records
commands, revisions, binary hashes, lockfile hashes, exit codes, test counts,
timeouts, and logs; failures stay failures. It cleans up only subprocess groups
that it creates. The original download and full remote evidence remain available
for investigating the imported experiment separately.

For the warm theme probe, supply a separate Buck2 checkout at `7fef66a8` and its
pinned engine. `scripts/relax-compare.py --sekka /path/to/sekka --relax /path/to/relax
--buck2 /path/to/buck2-checkout --buck2-bin /path/to/pinned/buck2 --output /path/to/new/comparison`
checks theme sources, inherited workspace metadata, and locked dependencies
before comparing. It uses its own daemon isolation and content store and stops
that daemon when finished.
