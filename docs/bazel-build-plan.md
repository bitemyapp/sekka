# Bazel and Nix build plan: workstation first

Status: implementation plan, 2026-11-03, with its first review unit and
most of its second implemented ([Implementation status](#implementation-status)).
Bazel is not a supported build entry point yet, and no service is installed.
The initial investigation used Sekka commit
`41fdd11dd5b457b29649d0f5a5e0cf80c06fc2ce`.

## Decision and order of work

Use `ssh wx-workstation` as the shared Bazel cache and execution host. First
run the Bazel client, compilation, tests and worktree-sharing experiments on
that workstation. Establish acceptable build times and persistent storage
there before evaluating the laptop. The laptop should then reuse workstation
results, offload eligible cache misses, and share downloaded artifacts among
its own worktrees.

Nix remains responsible for pinned tools, native dependencies, worker
environments, packaging, NixOS systems and images. Bazel should own the
compilation/test action graph for migrated components. Start with
`rules_rust`, `crate_universe` and `rules_nixpkgs`; preserve the existing Cargo
workspace boundaries and Cargo manifests/locks as dependency inputs.

The storage objective is to retain each reusable immutable artifact once in
a shared store, rather than once per worktree. Each machine may have one
bounded local cache; execution scratch must have a bounded lifetime. A
central CAS (content-addressable store) plus copies in every `bazel-out`
directory does not meet this objective. Different profiles, features,
targets and toolchains legitimately produce different artifacts.

Delivery order:

1. Capture a current baseline and pin the experiment environment.
2. Build real Aizome targets with Bazel locally on `wx-workstation`.
3. Prove cross-worktree cache reuse, then physical sharing using execution
   workers and a shared output service on the same workstation.
4. Pass the workstation gates in this document and publish the evidence.
5. Test laptop downloads and local deduplication, then workstation execution
   of laptop-initiated misses.
6. Integrate packaging and CI after their separate correctness gates pass.

Keep the release workflow inhibited. This project does not authorize image
publication. Component benchmarks and fixtures do not certify an ISO.

## Implementation status

Review units (1), pinned toolchain/rules and harness, and (2), real local
Aizome targets, are implemented except for most of the reload contract and
the shipping profiles' full verification. Units (3)–(6) have not started: there
is no cache, worker or output service, and the laptop, packaging and CI are
unchanged. Phase 1's local trials are in
[test-results/bazel/2026-11-03-phase1](test-results/bazel/2026-11-03-phase1/README.md).

| Piece | Where | State |
| --- | --- | --- |
| Pins | `.bazelversion`, `MODULE.bazel`, `MODULE.bazel.lock`, `.bazelrc`, `REPO.bazel` | Bazel 9.2.0 (the root lock's `bazel_9`), rules_rust 0.75.0, rules_cc 0.2.17, rules_nixpkgs 58151f37 |
| Tools and C libraries | `nix/bazel/`, `build/bazel/nix.bzl`, `build/bazel/native.bzl`, `devShells.x86_64-linux.bazel` | From the root lock's Nixpkgs; Bazel's repository rules and the shell build the same store paths. `checks.bazel-native-inputs` fails if the Aizome package's inputs or whisper.cpp settings and Bazel's differ |
| crates.io and Git crates | `crate_universe` over `build/bazel/aizome-cargo/`, locked in `build/bazel/aizome-crates.lock.json` | All 644 packages at Cargo.lock's versions; features equal `cargo tree --target x86_64-unknown-linux-gnu` for 463 of 464 compiled crates (exceptions below) |
| Sekka's crates | A generated `BUILD.bazel` in each of the 48 path packages (`build/bazel/generate.py`) | Libraries, binaries, build scripts and the members' tests. Nix package sources leave them out (`nix/bazel/without-build-files.nix`): with them added, the derivations of `aizome`, `hotaru`, `genkan`, `uta`, `yukimi` and `checks.sukumo` were unchanged. `checks.tuning`, which copies all of `desktop/aizome`, does change |
| Builds | `bazel build --config=dev //desktop/aizome/... //apps/hotaru/... //apps/yukimi/...` | All 341 targets build on wx-workstation, compositor dylibs and Koe's whisper.cpp included. The compositor also builds with `release`; `release-lto` hasn't been tried |
| Compositor | `bazel run --config=release //desktop/aizome/crates/aizome-comp:aizome -- --build-info` | Runs, and reports the same core ABI built from two trees at different paths with separate output bases |
| Tests | The Aizome package's check set (below) | 9 targets, 548 tests pass under Bazel |
| Harness | `scripts/bazel-bench.py`, `scripts/bazel-storage.py` | Phase 1 local trials for Cargo and Bazel, 44 of 44 checks passed |
| Configurations | `.bazelrc` | `dev`, `release`, `release-lto` and `wx-local`. `wx-cache`, `wx-exec`, `wx-shared` and the laptop's follow with their services |

Working with it, in `nix develop .#bazel` (whose `bazel` reads only the
checked-in `.bazelrc`, with the Nix JDK):

```sh
bazel build --config=dev //desktop/aizome/apps/soroban:aizome-soroban
bazel test --config=dev //desktop/aizome/apps/yukimi:lib_test \
  //desktop/aizome/apps/yukimi:headless //desktop/aizome/apps/yukimi:look \
  //desktop/aizome/apps/yukimi:golden //desktop/aizome/crates/aizome-kit:lib_test \
  //desktop/aizome/crates/aizome-sys:lib_test //desktop/aizome/crates/aizome-shell:lib_test \
  //desktop/aizome/apps/hako:lib_test //desktop/aizome/apps/hako:file_manager
# After changing a Cargo manifest, Cargo.lock or MODULE.bazel's annotations:
python3 build/bazel/repin.py
python3 build/bazel/generate.py --check
```

Those nine test targets are the package's check phase
(`desktop/aizome/nix/package.nix`): Yukimi's library, `headless`, `look`
and `golden` tests; the kit's, `aizome-sys`'s, the shell's and Hako's
libraries; Hako's `file_manager`. They run with the package's test
environment (fonts, fontconfig, time zones, the libraries opened at run
time, a D-Bus daemon: `testEnv` in `nix/bazel/toolchains.nix`).

Phase 1's trials (`dev` profile, the workspace's libraries and binaries,
16 jobs, no cache) give medians of 124 s cold under Cargo and 115 s under
Bazel; 0.21 and 0.26 s for a no-op; 0.67 and 1.52 s for an edit to
Soroban; 5.4 and 14.5 s for an edit to aizome-theme; 7.5 and 13.1 s for a
new icon read by aizome-kit's build script. The last two miss the latency
gate's `1.5 ×` Cargo; in both the compositor core's 10.5 s compilation is
the critical path.

What implementing it found, which the remaining units must account for:

- **Splicing.** crate_universe copies a Cargo workspace before resolving it.
  Aizome's path dependencies leave the workspace (Yukimi's and Hotaru's
  crates) and come back into it (`yukimi-system` depends on `aizome-ipc`),
  so in the copy Cargo sees two `aizome-ipc` packages. `generate.py` writes
  a manifest-only mirror with one root above every path package, the same
  relative layout, Aizome's members' dependencies (dev-dependencies only for
  members, as Cargo resolves them) and its `[patch]` table. A repin resolves
  the mirror against Aizome's own `Cargo.lock` and fails if a byte of it
  changes; ordinary builds never splice.
- **Features.** Bazel builds one configuration: members' dev-dependency
  features are on in every build (`wayland-protocols-misc` gains `client`),
  and `jiff-core` gains `default`. The package's
  `--features aizome-koe/openblas,aizome-koe/vulkan` is not selectable per
  configuration yet, so Koe builds as the development shell's `cargo build`
  builds it.
- **Compilation.** Cargo compiles workspace members incrementally and
  pipelines dependents on metadata. rules_rust 0.75 has no incremental mode,
  and with its pipelining on, crates failed (rustc's JSON from
  `wayland-protocols` unparsed, then `.rmeta` missing for `hayro` and
  Shiori's library): it is off. Edits to a large crate pay its whole
  compilation, which is most of the gap above. Files a crate includes through
  `concat!` count as their whole directory: a new kit icon recompiles the
  compositor core, which includes some of them.
- **Linking.** `rules_nixpkgs_cc` links with `-fuse-ld=gold` when it finds
  gold; `nix/bazel/toolchains.nix` leaves gold out, so Bazel links with GNU
  ld like the package, and `dev` links with mold like `nix develop .#aizome`. It still adds `-Wl,-no-as-needed -lstdc++ -lm`:
  Bazel's binaries list `libstdc++.so.6` and `libm.so.6` as needed where
  Cargo's don't. Every link sees the native libraries' directories, as
  `NIX_LDFLAGS` gives them to every link in the Nix build: crates such as
  `input-sys` name a library without saying where it is.
- **C flags.** Build scripts get `rules_nixpkgs_cc`'s compile flags
  (`-U_FORTIFY_SOURCE -fstack-protector -Wall`, and `-x c++ -std=c++0x` for
  C++), which Cargo's builds in Nix don't have. whisper-rs-sys appends
  `-x none`, without which CMake links its objects as C++ source.
- **Build scripts.** `MODULE.bazel` declares the native environment of the
  crates that run pkg-config, bindgen, rustc, CMake or make, and two fixes
  that keep Cargo's behaviour: `khronos_api` names its XML through its
  manifest directory (`build/bazel/patches/`), and `libspa-sys` takes its
  manifest directory when it runs rather than when it was compiled. The
  compositor core's ABI fingerprint script reads every path package's
  manifest and sources.
- **Hermeticity.** The Linux sandbox still shows actions the host's `/usr`:
  before its environment was declared, whisper-rs-sys's bindgen loaded
  Ubuntu's libclang. Phase 2's workers, which see only declared store
  closures, are where that becomes enforced; a hermetic local sandbox is
  worth trying first.
- **The core's ABI.** Its fingerprint hashed RUSTFLAGS, which under
  rules_rust carry `--sysroot=` the action's sandbox: every build had
  another ABI. `aizome-comp-core/build.rs` now leaves `--sysroot` out, as
  it leaves out other path-only flags; `rustc -vV` already identifies the
  toolchain, and Cargo's builds pass none, so the package's fingerprint
  means what it did. This changes Aizome's source and rebuilds its package.
- **Running outside Bazel.** The compositor needs its runfiles to find
  `libstd-*.so` (run it with `bazel run`), and its run path names the
  output base's absolute execroot. The packaging gate must give it the
  package's layout (`lib/aizome`) and no execroot path.
- **Left out.** cargo-auditable's dependency data; `aizome-comp`'s `reload`
  test, which rebuilds the logic library with Cargo; examples and benches;
  tests of crates outside Aizome's workspace (Hotaru's, Yukimi's, Sukumo's),
  which Cargo doesn't run from Aizome either.
- **Disk.** wx-workstation had 208–232 GiB free during this work, less than
  the 30 GiB pilot envelope above the 200 GiB floor. A full `dev` output
  base held 8.9 GiB of outputs and 3.5 GiB of repository cache, so
  development and the Phase 1 trials used tmpfs (`/dev/shm`), one tool at a
  time. Persistent-disk trials, and five worktrees, wait for headroom.

Next, in order: the rest of the reload contract (mismatch rejection, and
a logic edit reloaded into a disposable session or VM, never the user's),
`release` and `release-lto` verified against the package's behaviour, the
editor configuration, the edit latency (pipelining's failures, finer
`concat!` inputs), then Phase 2's cache service.

## Repository entry points

Read [DEVELOPERS.md](../DEVELOPERS.md) and the root [AGENTS.md](../AGENTS.md)
before implementation. Use a topic branch from current `main` and separate
worktrees for the experiment; preserve other agents' and users' changes.

| File | Why it matters |
| --- | --- |
| [flake.nix](../flake.nix), [flake.lock](../flake.lock) | Authoritative first-party sources, dependency pins and public outputs |
| [nix/components.nix](../nix/components.nix) | Local component adapters and source boundaries |
| [Aizome package](../desktop/aizome/nix/package.nix) | Native inputs, two compilation profiles, checks, install layout and fixups |
| [Aizome Cargo manifest](../desktop/aizome/Cargo.toml) | Workspace members, features and profiles |
| [Reload documentation](../desktop/aizome/docs/reload.md) | Rust dylib ABI and packaged/development compatibility contract |
| [Compositor build script](../desktop/aizome/crates/aizome-comp-core/build.rs) | Transitive source, lock and compiler inputs to the ABI fingerprint |
| [rustc wrapper](../desktop/aizome/scripts/rustc-wrapper) | Core-specific `prefer-dynamic` behavior |
| [aizome-dev](../desktop/aizome/crates/aizome-ipc/src/bin/aizome-dev.rs) | Cargo target discovery, build environment and reload workflow |
| [runner definition](../nix/runner.nix) | Existing job isolation and shared Nix store/daemon access |
| [CI retention](../scripts/ci-retain.py) | Existing bounded Nix GC roots; does not manage Bazel storage |
| [PR workflow](../.github/workflows/pr.yml), [cache workflow](../.github/workflows/cache.yml) | Current trust boundaries and Nix checks |

Do not merge Cargo workspaces, broaden Nix source filters, replace local
first-party source with remote forks, or change GUI/helper privilege
boundaries to make the migration easier.

## Evidence already available

The [recorded probe](test-results/bazel-nix-probe-2026-11-03.json) is a compact,
checked-in extract of the investigation. It used actual `aizome-theme` source
with a synthetic dependent library and executable, a build script reading a
data file, and serde/proc-macro dependencies. It is not a full Aizome Bazel
benchmark.

Pins used: Bazel `9.3.1`, `rules_rust` `0.75.0`, Rust `1.99.0`, and
`rules_nixpkgs` commit `58151f3745a1abe09277a6a8d0c58449018d2baa`, with
Nixpkgs from Sekka's root lock. These are reproduction pins, not an instruction
to upgrade the host or use moving upstream branches.

| Probe | Seconds | Relevant result |
| --- | ---: | --- |
| Cargo, identical second worktree with an independent target | 10.80 | Compiled again |
| Bazel, identical second worktree, shared disk cache | 6.63 | 107 cache hits; no executed compilation actions |
| Bazel, identical second worktree, HTTP cache | 9.15 | 107 remote hits |
| Bazel, application-only edit | 0.70 | One executed action |
| Bazel, shared-library edit in a fresh worktree | 6.84 | 104 hits and three executed actions |
| Bazel, top-level downloads followed by a local application edit | 0.73 | One executed action; local output storage grew |
| Bazel, build-script data-file edit | 2.07 | Three executed actions; changed output verified |
| Cross-machine HTTP cache read | 33.40 | 107 hits; includes first-start/repository setup; executable verified |

On `wx-workstation`, full downloads materialized 98,086,912 allocated bytes
in a fresh `bazel-out`; top-level downloads used 741,376 bytes; minimal
downloads used 167,936 bytes. The top-level worktree grew to 10,817,536 bytes
after one local compilation. All 63 matching Rust output files compared
between full-download worktrees had identical content and different inodes.
The shared HTTP cache itself used 99,270,656 allocated bytes. Repository
sources, server state and other output-base contents were additional.

The small Rust dylib/shared-core/`dlopen` smoke test passed. Actual Aizome
hot reload, native dependencies, remote execution, `bb_clientd`, production
cache operation and laptop physical deduplication remain unqualified.

Limitations: these were single observations on a busy workstation. Cargo,
Bazel and per-crate Nix compiler profiles were not equivalent. Bazel's cold
43.28-second run included bootstrap tools, versus Cargo's 11.16 seconds.
Do not extrapolate throughput or full-workspace speedups. One follow-up run
outside the pinned shell invalidated the graph; it is excluded from the
controlled measurements and retained in the raw evidence.

Raw evidence and exploratory scripts were retained at:

- Workstation: `~/sekka-work/bazel-nix-eval-20261103/{evidence,tools,fixture}`.
- Original local checkout: `.work/build-eval-20261103/` under
  `/home/callen/work/sekka`.
- Earlier, separate full-Cargo/Nix Aizome benchmark:
  `~/sekka-work/aizome-build-bench-20261102/report/REPORT.md`, at commit
  `b13988e986628502d7b80136b2ab6dac233934e9`.

The exploratory scripts are diagnostic records, not an idempotent harness;
several assume prepared directories. The HTTP fixture server was stopped,
and generated output directories, private caches and temporary GC roots were
removed, reclaiming about 4 GiB. Its old port is not a running service. The
checked-in record is sufficient to understand the baseline if those external
directories disappear; rebuild a maintained harness in this repository.

The older full-workspace report found roughly 5.6 GiB per Cargo target tree
and a stale compiled theme value when naively sharing a mutable target
directory across source versions. Do not use a common `CARGO_TARGET_DIR` as
the artifact-sharing implementation.

## Architecture on wx-workstation

"Bazel server" here means the workstation hosts the shared cache, scheduler
and execution workers. Bazel also has a client/JVM server for each output
base; those remain separate across worktrees. A common `output_user_root`
does not create one shared output base. Do not force concurrent worktrees
through a single `--output_base`.

```text
worktrees A/B/C on wx-workstation
    -> pinned Bazel clients and independent analysis state
    -> shared action cache + CAS on wx-workstation
    -> scheduler + isolated workers on wx-workstation
    -> one shared output service/cache on wx-workstation

later: laptop worktrees
    -> same toolchain/action identities
    -> cache and workers on wx-workstation over SSH
    -> one bounded output service/cache on the laptop

root flake -> tools/native libraries/worker environment
Bazel outputs + provenance -> Nix packaging -> existing system/VM checks
```

### Service choice and layout

Start with a pinned Buildbarn deployment: storage/action cache first, then
its scheduler/workers and `bb_clientd`. This keeps the cache and output
filesystem investigation in one ecosystem. Adapt the
[upstream deployments](https://github.com/buildbarn/bb-deployments) to this
single host; pin commits and image digests and document the actual generated
configuration. Do not use the earlier Python HTTP probe as a persistent
cache service.

NativeLink is the fallback if Buildbarn operation proves disproportionate.
Record that decision and rerun the same storage gates. A simple cache-only
service can be a diagnostic step, but is not the completion criterion.

Proposed paths, all to be created by the implementation:

| Path | Lifetime and owner |
| --- | --- |
| `~/sekka-work/bazel-lab/repo` | Dedicated checkout; record upstream and exact revision |
| `~/sekka-work/bazel-lab/worktrees/{a,b,c,d,e}` | Disposable measured source trees |
| `~/.local/state/sekka-bazel/services` | Pinned service config, bounded logs, PID/container identity |
| `~/.cache/sekka-bazel/cas` | Shared immutable artifact storage, with explicit byte limit |
| `~/.cache/sekka-bazel/repos` | Shared repository download cache; extracted copies accounted separately |
| `~/.cache/sekka-bazel/clientd` | One output-service cache per host, not per checkout |
| `~/.cache/sekka-bazel/scratch` | Bounded worker and local-action writable space |
| `~/.cache/sekka-bazel/bazel` | Separate output bases; reclaim inactive ones without evicting the shared CAS |
| `~/sekka-work/bazel-lab/results/<run-id>` | Commands, metrics, manifests and logs |

Use an isolated service/project name. Avoid the existing runner containers,
daemon configuration and other agents' directories. Bind service endpoints
to loopback initially. Proposed host ports are `19850` for cache/CAS and
`19851` for execution; check availability and record the actual endpoints.
The output service should use a private local socket and an unprivileged
FUSE mount where supported. Check `/dev/fuse` and user permissions before
choosing that deployment. Do not require privileged containers as a default.

Use a common cache namespace for matching trusted builds across worktrees;
do not include checkout path or Git branch in it. Preserve a separate trust
boundary for untrusted builds. Client flags alone are not cache write
authorization. Workers must not inherit SSH keys, developer secrets or the
host Nix daemon socket; provision declared store closures before execution.

### Resource budget

Recheck disk and concurrent workloads at the start of each phase. The
2026-11-03 documentation preflights found about 229–235 GiB free on the
workstation's 3.7 TiB filesystem as other work continued. Those are dated
observations, not reserved space. Existing CI retention reacts around 200
GiB free.

Start with four build jobs and bounded native sub-build parallelism. For Nix
preparation, start with `--max-jobs 2 --cores 4`. Count worker CPU reservations
and compiler-internal concurrency together; do not equate 128 logical CPUs
with 128 safe parallel Rust links or shader generators.

Propose up to a 30 GiB pilot disk envelope: 16 GiB central CAS/action-cache data,
8 GiB writable scratch, 2 GiB output-service cache and 4 GiB repository/server
state. Configure actual limits and verify allocated bytes; preallocated
storage and logs count. These are initial limits, not evidence that a full
Aizome graph fits. New Nix closures also consume host space outside this
envelope. Keep at least 200 GiB host headroom; reduce the initial allocation
when 30 GiB plus preparation costs cannot fit, and stop new heavy trials when
that floor would be crossed. Run comparison configurations sequentially and
retire their owned scratch after preserving measurements.

If the working set exceeds the budget, report its measured size and choose
an explicit revised allocation before claiming a cache-performance failure.
Do not reclaim unrelated targets, VMs or Nix roots. No global GC is part of
this experiment. Cache eviction must remove unreferenced content safely;
output-service reads of evicted content must refetch or produce a clear
rebuildable failure, never silently use stale bytes.

## Phase 0: baseline and reproducible harness

A read-only workstation preflight can start with:

```sh
ssh wx-workstation 'bash -s' <<'REMOTE'
set -eu
if ! command -v nix >/dev/null 2>&1; then
  . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
fi
hostname
uname -m
nix --version
df -h / /nix/store
findmnt -T "$HOME/sekka-work"
ss -ltn
REMOTE
```

The SSH alias currently resolves to a host reporting the hostname `callen`;
use the alias rather than inventing a new endpoint. Nix was not in the
noninteractive SSH PATH during investigation. Resolve the active profile or
enter the root-flake development shell; do not copy an old ephemeral
`/nix/store/.../bin/nix` path from the logs.

Create the dedicated checkout and five detached worktrees at one recorded
implementation commit. Keep a clean source manifest and the exact patch for
each deliberate edit. If the laptop supplies uncommitted sources, transfer
tracked changes and explicitly selected new files into a separate staging
checkout, hash that content, and exclude `.env`, caches and build outputs.
Never synchronize over another active checkout.

Implement these proposed files before relying on commands named later:

| Proposed file/output | Contract |
| --- | --- |
| `.bazelversion`, `MODULE.bazel`, lock files and `.bazelrc` | Pinned tools/dependencies and explicit profiles |
| `nix/bazel/` and root-flake `devShells.<system>.bazel` | Toolchain/native closure and service/worker definitions derived from the root lock |
| `build/bazel/` | Rust/native rule adapters, platform definitions and packaging metadata |
| `scripts/bazel-bench.py` | Idempotent scenario runner; isolated paths, timeouts, correctness checks, JSON metrics and cleanup of its own artifacts |
| `scripts/bazel-storage.py` | Cross-root allocated-byte accounting and content/inode manifests without following unrelated symlinks |
| `docs/test-results/bazel/<run-id>/` | Small reviewed results and provenance; large logs archived separately with checksums |

The harness must refuse to overwrite a non-owned directory and record any
failed trial. Record wall time, action CPU time where available, peak memory,
cache hits/misses, executed mnemonics, analysis time, downloaded/uploaded
bytes, action digests, output digests and semantic results. Capture Bazel
Build Event Protocol JSON, profile data and execution logs using the flags
supported by the pinned version. Count executable actions separately from
internal/runfiles/metadata actions.

Run the existing Cargo builds in `nix develop .#aizome` at the same revision
as the Bazel comparison. Reproduce both development and shipping behavior
from `desktop/aizome/nix/package.nix`. Record all features, target triples,
environment and wrapper choices. Capture the exact Nix derivations and
closure manifest. Also record the current monolithic `.#aizome` build as
context; do not mix its packaging/check time with Cargo compilation time.

## Phase 1: real local Bazel builds on the workstation

First run Bazel with local sandboxed execution and no shared action cache.
This establishes correctness and cold/incremental costs before adding
service effects. All heavy compilation in this phase runs on
`wx-workstation`, even when initiated through SSH from the laptop.

Start with theme plus a small real application such as Soroban. Then add the
compositor/core/logic boundary and a native-heavy target such as Koe. Finish
with the Aizome workspace's actual build and test target set. The small probe
alone is not the gate for starting laptop experiments.

Implementation requirements:

- Import Rust and C/C++ tools and native libraries from the root flake.
  Explicitly configure both toolchains; the initial probe's Nix Rust toolchain
  does not prove the C/C++ environment is ready for remote execution.
- Pin Bzlmod dependencies and dependency-generation tools. Check generated
  rules against all affected Cargo locks/manifests. Do not repin dependencies
  during a timed trial. Map each existing Cargo workspace separately; support
  Aizome's Yukimi/Hotaru path dependencies without copying a different source
  revision or flattening workspace feature boundaries.
- Match features, panic mode, optimization, debug information, codegen units,
  explicit target and linker configuration. Disable host-native CPU tuning
  for artifacts intended for the laptop. Preserve `GGML_NATIVE=OFF` and the
  package's relevant CPU/BLAS/Vulkan choices.
- Declare build-script inputs, generated outputs, environment reads and
  proc-macro inputs. Cover `include_*`, assets, bindgen headers, pkg-config,
  shader tools, fonts and libraries opened by `dlopen`. A successful build
  outside a sandbox is not proof that inputs are declared.
- Model heavy native compilation explicitly where beneficial. Keeping a
  whole CMake build inside one Cargo build-script action gives that action's
  granularity, not object-level cache reuse. Account for native `.o`/`.a`
  files and generated trees rather than measuring only Rust `.rlib` files.
- Preserve the `release` compositor build versus `release-lto` applications.
  Audit cargo-auditable behavior and core-specific rustc flags. A Bazel-built
  logic library must not be assumed compatible with a Cargo-built running
  core merely because the source and rustc version match.
- Qualify a matched Bazel host/core/logic set, mismatch rejection, and a real
  logic edit/reload in a disposable session or VM. Adapt `aizome-dev` and
  packaged `build-env.json` deliberately. Preserve the current path until
  this contract is established; never reload an experimental library into
  the user's active compositor as a benchmark shortcut.
- Add an editor configuration using the pinned Rust toolchain and Bazel
  project information. Verify proc macros and build-script generated code.
  Any retained Cargo check fallback must have an explicit bounded scratch
  policy; hidden editor `target` trees count against the storage objective.

Repository fetching/analysis may need network access. Compilation actions
must operate from declared inputs and provisioned tools. Persist dependency
downloads separately so an action-cache cold run does not accidentally
measure the internet or a Rust toolchain rebuild.

## Phase 2: workstation cache reuse, then physical artifact sharing

### A. Cache-only baseline

Start the shared cache on `wx-workstation`. Run the client in worktree A on
that same host with local compilation and cache uploads. Run B with identical
sources, locks, flags and environment but its own output base. Compare full,
top-level and minimal output downloads in separate fresh output bases.

Use `--nosystem_rc --nohome_rc` and the checked-in configuration. Endpoints
and output roots may vary; action inputs must not. Avoid inherited PATH,
temporary directory, home, timestamp or worktree names in action keys.
Investigate misses with action/command/environment diffs, not just hit-rate
percentages. Do not normalize away a path that changes actual program or
build-script semantics.

### B. Workers on the same host

Add isolated execution workers on `wx-workstation` and run workstation Bazel
clients against them. This is the remote-execution protocol exercised on
one machine, not a move of the benchmark to the laptop. Build worker images
or store closures from the root flake, including the required `/nix/store`
paths. Include an environment/closure identity in the execution platform so
worker changes cannot reuse incompatible action results. Do not let workers
fetch undeclared tools or call the host Nix daemon during an action.

Measure local execution versus same-host worker execution with equal CPU
and memory budgets. Disable dynamic local racing/fallback for the measured
remote-execution configuration so zero-local-compilation claims are real.
Explicitly list unavoidable local actions, repository rules and tests.
Virtual machines, devices and live-session checks stay with the existing
Nix/test infrastructure; remote compilation does not make them ordinary
remote unit tests.

### C. Shared output service

Add one `bb_clientd` output service/cache for the workstation's experiment
user. Each worktree retains a separate output namespace backed by that
shared service. Use the pinned Bazel version's output-service flags; verify
their names with `bazel help build` and test compatibility before committing
configuration. The service must make output files available to normal file
reads, debuggers, `bazel run`, tests and dynamic loaders.

The output service's mutable file pool is separate from its immutable CAS
cache. Measure both. Locally executed actions may still create distinct
writable files; remote execution plus retirement of scratch must address
that growth. Sharing a download cache alone is insufficient. Consult the
[output-service design](https://blog.bazel.build/2024/07/23/remote-output-service.html)
and [bb_clientd](https://github.com/buildbarn/bb-clientd) for the interfaces;
actual byte sharing is a benchmark result to establish here.

On the workstation, prefer output-service reads from the central CAS without
a second persistent full copy on that same host. If a separate read cache is
needed for latency, bound and report that replication explicitly. On the
laptop, one local read cache is expected; all its worktrees must share it.
Do not describe intentional copies between stores as complete physical
deduplication.

If a virtual output service cannot support the required workflow, test a
bounded immutable materialization store with supported hardlink/reflink
semantics as an explicit alternative. Never hardlink writable compiler or
Cargo target files across worktrees. Reflinks require filesystem support;
the workstation's ext4 setup must not be assumed to provide them. A
cache-only result cannot be promoted as the final storage solution.

### Configuration contract

Create named configurations with the following behavior. They are proposed
interfaces, not configurations available in this documentation PR. Keep the
same compilation profile and action environment when comparing them.

| Proposed config | Execution and cache behavior |
| --- | --- |
| `wx-local` | Local sandboxed actions; no disk/remote action cache; baseline output materialization |
| `wx-cache` | Local actions, shared cache at the workstation loopback endpoint; test `all`, `toplevel` and `minimal` downloads separately |
| `wx-exec` | Cache plus workstation execution endpoint; no dynamic local racing or silent local fallback |
| `wx-shared` | `wx-exec` plus the workstation output service; final workstation storage candidate |
| `laptop-cache` | SSH-forwarded cache, local-result uploads disabled, one local output service/cache |
| `laptop-exec` | `laptop-cache` plus workstation execution of eligible misses; fallback disabled for measurement |

The cache/execution settings are `--remote_cache=grpc://127.0.0.1:19850`
and `--remote_executor=grpc://127.0.0.1:19851` for the proposed loopback
deployment. `--remote_download_outputs=toplevel` is the cache-only starting
point, not the physical-sharing solution. Inspect actual action mnemonics
and strategy selection when enforcing remote-only compilation; a nominal
executor URL does not establish where each action ran. Check output-service
and download-flag interactions against the pinned version's help.

After the root `.#bazel` shell, config and a real target exist, the command
shape on the workstation is:

```sh
# Run from a measured workstation worktree; set target to an implemented label.
target='//desktop/aizome:qualified-target'
nix develop .#bazel --command bazel \
  --nosystem_rc --nohome_rc \
  --output_user_root="$HOME/.cache/sekka-bazel/bazel" \
  build --config=wx-shared "$target"
```

`qualified-target` is a placeholder; replace it with the label in the
implementation's recorded target manifest. The harness must expand and
record all effective options rather than relying on this example as evidence.

### Workstation scenario matrix

Run each relevant scenario for development and shipping profiles, with a
fixed target set and recorded expected outputs. Distinguish first-ever
setup, cold action cache, warm shared cache with fresh analysis state, and
warm no-op in an existing output base. Use at least five repetitions for
warm/edit trials; report median and range. Do not call the maximum of five
runs a meaningful p99. Record concurrent host load and repeat contaminated
trials without hiding their results.

| Scenario | Required observation |
| --- | --- |
| Cold A | Correct full build; action, memory, CPU and complete disk costs |
| A no-op | No compiler/linker action; analysis/startup separated |
| Identical B, then C/D/E | All eligible actions hit; stable shared artifact bytes |
| Application-only edit in B | Only the expected leaf/link actions execute; changed behavior verified |
| Shared-library edit in C | Correct reverse dependency rebuild; unrelated actions reuse results |
| Build-script data/header/environment edit | Generated/native changes reach the binary; no stale output |
| Revert edits | Original artifact reuse despite mtimes or checkout path changes |
| Feature/profile/toolchain change | Deliberate, explained misses; incompatible outputs never alias |
| Concurrent B and C with divergent sources | Both results correct; record duplicate in-flight work and locking |
| Repeated edits and many worktrees | Plateau/bounded persistent growth; scratch reclaimed |
| Client/service restart and cache eviction | Correct recovery, fetching and error handling |
| Debug/run/test/editor/hot reload | Correct outputs on demand without rebuilding private artifact trees |

For storage accounting, record CAS, action-cache metadata, output-service
cache/file pool, all output bases, worker scratch, repository downloads and
extractions, Nix store delta, logs, editor artifacts and packaged outputs.
Keep source checkout size separate, but report generated/extracted build
repositories rather than omitting them. Sample peak disk during execution
and steady state after quiescence. Do not sum separate `du` calls as proof of
physical use: hardlinks can be counted repeatedly. Deduplicate `(device,
inode)` identities across measured roots; record file size, allocated blocks,
SHA-256 and link count for outputs. Use filesystem extent accounting where
reflinks are claimed, and backing-store/cache counters for virtual outputs.

Nix already shares identical store paths across worktrees. If different
derivations retain byte-identical files, measure that duplication separately
and evaluate Nix's supported store optimization as a maintenance task. Do
not manually hardlink store files or run a whole-store optimization during a
timed build; its I/O cost and any changed host policy need their own record.

### Workstation exit gates

These are proposed acceptance thresholds, not achieved measurements. Freeze
them before trials; any revision requires a written rationale and preserved
results, rather than weakening a threshold to hide a failure.

1. **Correctness:** all selected real Aizome builds/tests pass; build-script
   edits, concurrent variants and the real reload contract pass. Every
   deliberate semantic edit has an independent observable check.
2. **Reuse:** an identical second worktree executes zero eligible
   compiler/linker/build-script actions after prerequisite tools are primed.
   Explain exceptions individually. Internal Bazel actions are not misses.
3. **Latency:** warm identical-worktree median is at most 20% of the cold
   action build; steady no-op is at most 2 seconds. Leaf and shared-edit
   medians are no worse than `max(2 seconds, 1.5 * matching Cargo median)`.
   Compare equivalent flags and actual behavior; report cold overhead too.
4. **Persistent sharing:** after five identical worktrees exercise builds,
   runs and tests, identical compiler outputs have one shared immutable
   backing representation per intended store. No persistent private
   multi-gigabyte intermediate trees remain. Added total build-state storage
   is at most 5% of the one-worktree payload plus 32 MiB metadata per extra
   worktree. Report intentional Nix-package/CAS copies separately and count
   them in total capacity; do not hide them behind this metric.
5. **Bounded operation:** a repeated-edit/eviction run respects configured
   limits and host headroom; worker scratch is reclaimed; a service restart
   preserves correctness. Retiring an output base must not delete shared
   artifacts still needed by other worktrees.

Publish a pass/fail table and the raw-data manifest. If either build latency
or physical sharing fails, fix the workstation implementation first. Do not
start laptop optimization on the strength of cache-hit counts alone.

## Phase 3: laptop reuse and local deduplication

Begin only after the workstation report passes the gates above. Identify
the actual laptop through its local environment; record OS, architecture,
filesystem, free space and CPU. Do not assume that a machine which can SSH
to the builder can run the same target binaries. Use matching target/exec
platforms or document cross-compilation requirements.

### Copy artifacts through the appropriate store

Use Bazel's cache protocol for Bazel artifacts and Nix closure copying or a
trusted Nix substituter for tools, native dependencies and packaged results.
Do not `rsync` the workstation's `bazel-out`, output bases or Cargo targets:
their action metadata, absolute paths and writable state are not a portable
shared artifact store.

First use read-only cache consumption for laptop local-build results
(`--remote_upload_local_results=false`, backed by service authorization where
needed). Match the workstation's source manifest, locks, rules, toolchain
closure, target, features and profiles. Copy only declared required Nix
closures; never disable store signature/integrity checks to make this work.
For a path obtained from the recorded toolchain/package manifest, the Nix
operation is `nix copy --from ssh://wx-workstation "$store_path"`. Ensure the
remote Nix store command is available in SSH's environment or configure its
documented remote-program setting. Root actively needed closures with
bounded lifetimes. [Nix copy reference](https://nix.dev/manual/nix/2.34/command-ref/new-cli/nix3-copy).

Once the implementation has bound the proposed ports, this tunnel provides
loopback-only access from the laptop:

```sh
ssh -N -o ExitOnForwardFailure=yes \
  -L 127.0.0.1:19850:127.0.0.1:19850 \
  -L 127.0.0.1:19851:127.0.0.1:19851 wx-workstation
```

The ports are proposed deployment defaults, not live services today. Keep
the tunnel foreground-managed by its owner or a named user service. Record
the PID/service so cleanup stops only this connection. Configure Bazel's
cache endpoint to the first port and, in the later execution test, its
executor endpoint to the second. A single remote-API frontend may instead
serve both; document the selected deployment. Use TLS/authentication if
exposing endpoints outside the SSH tunnel.

### Laptop tests, in order

1. Prime the selected targets on `wx-workstation`. From a fresh laptop
   output base, retrieve and run them. Record download/setup time separately
   from repeated builds, bytes transferred and local compiler CPU. All
   eligible primed actions should hit; inspect misses before expanding scope.
2. Configure one laptop-wide bounded output-service cache, with separate
   worktree namespaces. Repeat the five-worktree physical-sharing test,
   including launching binaries, tests, debugger and editor. Start with a
   2 GiB cache and 4 GiB scratch limit only if local headroom permits; size
   the final limits from the measured working set. Full downloads into five
   separate output trees do not satisfy local deduplication.
3. Submit a deliberate laptop edit to execution workers on `wx-workstation`.
   Verify the changed result and worker execution records; with fallback
   disabled, zero eligible Rust/C/C++ compile/link actions should run on the
   laptop. Repository resolution and Bazel analysis may still consume local
   CPU, and need separate measurement. A second laptop worktree should reuse
   the result without creating another persistent payload copy.
4. Disconnect/reconnect the tunnel and restart the output service. Define
   the offline policy: previously materialized runnable outputs remain usable
   where supported; missing remote outputs fail clearly. An explicit local
   fallback may be offered, with bounded scratch, but must not silently make
   a remote-only benchmark look successful by compiling on the laptop.

Laptop acceptance: correct executable/runtime closures, zero eligible local
compilation on fully primed builds and remote-only misses, at least 90%
reduction in compile/link CPU versus a matching uncached local Cargo build,
and the workstation's physical-sharing gate applied locally. Report latency,
network cost, local analysis cost and background service memory even if CPU
savings pass. End-to-end repeated edits should beat the measured laptop
local-build baseline; report any interactive regression explicitly. Central
CAS plus one bounded laptop cache is acceptable. One physical copy across
both machines is not required to claim this local-deduplication milestone.

## Phase 4: packaging and CI integration

This is a separate gate, not an assumption that successful local Bazel
commands automatically accelerate `nix flake check`.

Ordinary sandboxed Nix builds cannot reach an arbitrary remote cache or
execution service. Putting a monolithic Bazel invocation in
`buildBazelPackage` preserves that boundary and does not expose action reuse
by itself. Bazel repository rules that invoke Nix also need explicit
provisioning for an offline Nix build. Do not disable the sandbox or expose
the daemon socket inside ordinary compilation actions to work around this.

Implement and review an explicit packaging boundary:

- Bazel produces an installable artifact bundle and a manifest binding source
  content (including dirty-tree patches), locks, rules, toolchain/worker
  closure, target/profile/features, action configuration and artifact digest.
- Nix packaging consumes a declared, hash-verified immutable bundle or cache
  snapshot. Retain the existing public flake outputs, runtime dependencies,
  licenses, fonts, service files, wrapper behavior and fixups. Validate ELF
  interpreters, RPATHs and closure references; no executable may depend on a
  vanished Bazel execroot or undeclared workstation library.
- Provide an offline from-source path using the same Bazel graph with all
  repositories, tools and native inputs declared in advance. Define how the
  bundle hash is supplied for local edits without pretending a mutable cache
  URL is a reproducible Nix input. Record which build path was exercised.
- Test packaged compositor/core/logic identity and `aizome-dev` against those
  packaged outputs, then existing component checks. Do not treat a manifest's
  claimed Git SHA as proof of source provenance or bypass hash verification.
- In CI, perform eligible compilation through the same service and graph,
  then package and run existing checks. Preserve trusted/untrusted PR
  separation and cache write rules. Containers must use a reachable service
  endpoint: their loopback is not the host's loopback. Keep Nix roots and
  Bazel retention separate, bounded and observable.

The first packaging PR should make this boundary concrete and reviewable
before CI defaults change. Production migration is not complete while CI
silently recompiles the entire component through a second Cargo graph.

If installation, ISO or nightly behavior is exercised, follow the mandatory
[ISO qualification skill](../.agents/skills/sekka-iso-qualification/SKILL.md)
and its coordinator checklist. Disk-mutating tests belong in disposable VMs.
Keep component checks, image builds, VM installation and hardware validation
as separate evidence categories. Publication remains inhibited.

## Evidence, review units and handoff

Use small reviewable changes: (1) pinned toolchain/rules and harness,
(2) real local Aizome targets, (3) workstation services and sharing evidence,
(4) laptop reuse/deduplication evidence, (5) packaging bridge, (6) CI adoption.
Do not start the laptop unit until the workstation unit passes. Keep the
existing Cargo/Nix entry points available for comparison and rollback until
their replacements are qualified.

For each unit, retain:

- Exact source commit/patch hashes, lock hashes, tool/service versions,
  Nix derivation/closure paths, worker image digest and configuration hash.
- Invocation argv, selected environment inputs, host load and resource
  limits, target/profile matrix, trials including failures, and correctness
  assertions. Redact credentials; do not archive entire environments.
- Build event/profile/execution logs, cache counters and byte transfers;
  source/output content manifests; peak and steady physical storage by owner.
- Median/range comparisons against same-revision baselines, gate decisions,
  exceptions, unresolved risks and the exact next command or task.
- Named services, endpoints, PIDs/container IDs, GC roots and retained paths;
  how to restart them and retire only their owned scratch. Preserve results
  before cleanup and state what is still consuming space.

Use one machine-readable result per trial with `source`, `host`, `scenario`,
`profile`, `toolchain`, `argv`, `wall_seconds`, `action_cpu_seconds`,
`executed_actions`, `cache_hits`, `download_bytes`, `storage_peak_bytes`,
`storage_steady_bytes`, `correctness`, and raw-log checksums. Unknown values
are null with a reason, not zero. Commit compact summaries under
`docs/test-results/bazel/`; large archives need a recorded accessible location
and checksum. A future agent must be able to resume from repository evidence
without the original conversation or an active `/tmp` worktree.

## Upstream references

- [Bazel remote caching](https://bazel.build/remote/caching) and
  [remote execution](https://bazel.build/remote/rbe).
- [Bazel output directory layout](https://bazel.build/remote/output-directories).
- [rules_rust](https://bazelbuild.github.io/rules_rust/),
  [Rust dylibs](https://bazelbuild.github.io/rules_rust/rust_dylib_library.html),
  [rust-analyzer integration](https://bazelbuild.github.io/rules_rust/rust_analyzer.html).
- [rules_nixpkgs](https://github.com/tweag/rules_nixpkgs).
- [Buildbarn deployments](https://github.com/buildbarn/bb-deployments) and
  [bb_clientd](https://github.com/buildbarn/bb-clientd).
- [NativeLink](https://github.com/TraceMachina/nativelink), the service fallback.
- [Nix sandbox settings](https://nix.dev/manual/nix/2.34/command-ref/conf-file.html#conf-sandbox).

Check these interfaces against the pinned versions when implementing; online
documentation may describe a newer version than the reproduction pins.
