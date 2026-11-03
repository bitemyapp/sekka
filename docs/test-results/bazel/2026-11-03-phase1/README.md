# Phase 1: Cargo and Bazel builds of Aizome's workspace, 2026-11-03

Local trials on `wx-workstation` (Threadripper PRO 9985WX, 128 threads,
Ubuntu 24.04, Determinate Nix 3.24.0), [docs/bazel-build-plan.md](../../../bazel-build-plan.md)'s
Phase 1. No cache, worker or output service took part: Bazel ran with
`--config=dev --config=wx-local`, Cargo in `nix develop .#aizome`. These are
component build measurements; they qualify no package, image or hardware.

## What ran

- **Source.** Cargo at `bcfbe754`, Bazel at `6a6b9e0e`, in fresh worktrees
  of a dedicated checkout (`scripts/bazel-bench.py`). The commits differ only
  in Bazel's files, documentation and the harness: the Aizome shell is the
  same store path at both (`/nix/store/kq9illgzm4bf42f4bf1b53xc1jxsca32-nix-shell-env`).
- **Targets.** Cargo: `cargo build --locked --offline --workspace -j 16` for
  `desktop/aizome/Cargo.toml`. Bazel: every `rust_binary`, `rust_library` and
  `rust_dylib_library` under `//desktop/aizome/...`, `//apps/hotaru/...` and
  `//apps/yukimi/...` (`bazel/targets.txt`), `--jobs=16`. Neither built tests.
- **Profile.** Cargo's `dev`, and Bazel's emulation of it (`.bazelrc`): members
  at opt-level 0 with line tables, everything else at opt-level 2 without
  debug information, both linked with mold.
- **Storage.** Worktrees, target directory, output base and repository cache
  on tmpfs (`/dev/shm`): the root filesystem had 230 GiB free, too close to
  the plan's 200 GiB floor for persistent trials. One tool at a time.
- **Load.** Other users' work ran throughout: the one-minute load average
  was 5–17 before each trial (in each record).

Each edit trial added a fresh marker and checked for it independently:
Soroban's binary (leaf), aizome-theme's rlib (shared), aizome-kit's generated
`icons.rs` (build script); the revert's binary had to equal the cold
build's. All 44 checks passed.

## Results

Wall seconds; five trials for each edit and no-op, one cold and one revert.

| Scenario | Cargo median (range) | Bazel median (range) | Executed Rust actions, Cargo / Bazel |
| --- | ---: | ---: | ---: |
| Cold | 123.8 | 114.8 | 512 crates / 744 `Rustc`, 88 build scripts |
| No-op | 0.21 (0.21–0.21) | 0.26 (0.26–0.31) | 0 / 0 |
| Leaf: Soroban's `main.rs` | 0.67 (0.67–0.71) | 1.52 (1.47–1.72) | 1 / 2 |
| Shared: aizome-theme | 5.37 (5.32–5.42) | 14.54 (14.29–14.64) | 30 / 54 |
| Build script: a kit icon | 7.53 (7.48–9.23) | 13.13 (12.94–14.44) | 25 / 52 |
| Revert | 7.78 | 13.34 | 25 / 52 |

Cargo prints one `Compiling` line per package; Bazel counts each library,
binary and build-script compilation (`Rustc`) and each build-script run.

Bazel's setup before the cold build (module resolution, toolchains, crate
downloads into an empty repository cache) took 16.6 s and is not in the cold
figure; its cold build includes compiling rules_rust's own tools.

Storage after the cold build, allocated bytes (`scripts/bazel-storage.py`):

| | Cargo | Bazel |
| --- | ---: | ---: |
| Build state | 7.58 GiB target directory | 3.83 GiB output base, 3.36 GiB repository cache |
| Physical, each inode once | 5.99 GiB | 7.55 GiB |

Both include the 0.36 GiB worktree. Cargo hard-links its outputs, which is
why its physical figure is lower than its sum.

## Reading it

Against the plan's latency gate (the edit medians no worse than
`max(2 s, 1.5 × Cargo's)`, no-op at most 2 s), no-op and the leaf edit pass;
the shared and build-script edits don't. In both, the compositor core is
the critical path: about 10.5 s to compile `aizome_comp_core`, while all of
Cargo's rebuild took 5.4 s. Cargo compiles workspace members incrementally;
rules_rust 0.75 has no incremental mode, and its pipelining (dependents
compiling from metadata) failed on this graph and is off. The build-script
edit also recompiles the core because it includes Lucide icons from the
kit through `concat!`, which generate.py declares as the whole icon
directory; Cargo's dep-info names only the icons used.

Bazel's revert recompiled rather than reusing the original results: with
`wx-local` there is no disk or remote cache, and the local action cache
keeps only the latest result per output. Reuse across edits, reverts and
worktrees is Phase 2's cache question, not measured here.

`bazel-bcfbe754/` keeps the first Bazel run, before two fixes: its leaf edit
took 14.5 s because the core's ABI script was given every path package's
sources (now only its closure's), and it linked with GNU ld. It is retained
as a failed configuration, not a result.

## Fields to read with care

- Cargo's `action_cpu_seconds` is the CPU of `cargo` and its children
  (`getrusage`); Bazel's in these records is `timingMetrics.cpuTimeInMs`,
  Bazel's own CPU rather than the actions'. They are not comparable, and the
  harness no longer reports the latter as action CPU.
- `peak_rss_kib` (Cargo) is the largest child since the harness started, not
  each trial's; the harness now names it `run_peak_child_rss_kib`.
- Peak storage during builds was not sampled.

## Raw data

Per-trial JSON is here (`cargo/`, `bazel/`, `bazel-bcfbe754/`, with
`summary.json` from `scripts/bazel-bench.py summarize`). Build logs, Build
Event Protocol files and Bazel profiles are on wx-workstation:
`~/sekka-work/bazel-lab/results/phase1-logs.tar.gz`, SHA-256
`fa017ee7f74c36da817c2980f5faf28e4c017bad8d4067f4a83f3040f8b0c52f`; each
trial record lists its own logs' checksums.
