# Memory investigation handoff — 20 October 2026

Continue on Sekka's `codex/migrate-aizome-22`, migrated from
[Aizome #22](https://github.com/bitemyapp/aizome/pull/22). Recorded measurements
and revision IDs below refer to the original Aizome repository. The strongest next experiment is
**jemalloc with background purging in the shell alone**, on the final
source, while leaving the compositor and apps on glibc. This is a proposed
experiment, not an implemented production policy. No allocator, linkage or
codegen default was changed, and nothing was deployed into the live desktop.

Read [the complete results](memory.md) for methodology and exclusions,
[the machine-readable results](memory-results.json) for values and ranges,
and [the profiling guide](profiling.md) for working commands. This document
records the decisions, remaining work and transfer requirements.

## Resume on another machine

In a fresh parent directory, get the branch and its pinned Sidestep submodule:

```sh
git clone --branch codex/migrate-aizome-22 --recurse-submodules https://github.com/bitemyapp/sekka.git
cd sekka/desktop/aizome
git rev-parse HEAD
nix develop .#aizome-profiling
```

The checked-in `sidestep` symlink points to `../../vendor/sidestep`. The
submodule and Sekka's root flake input both pin `dae957e`. Update them together
only deliberately; do not update inputs as an incidental setup step.

Inside the development shell:

```sh
nix build .#aizome --out-link result
nix build .#aizome-debug --out-link result-debug
nix build .#aizome-fonts --out-link result-fonts
export AIZOME_FONT_DIR="$(readlink -f result-fonts)"
export AIZOME_DEBUG_DIRS="$(readlink -f result-debug)/lib/debug"
scripts/aizome-memory --help
scripts/aizome-profile --help
```

These build the current branch, including upstream changes integrated after
the measurements. For the exact package revision that passed the full build
and installed smoke checks, use a separate checkout at
`f88ac0eb9e908f26f8e7201af2311d749c428851` in the original Aizome repository.
New builds do not imply the original machine's measured totals.

Use explicit PIDs with `scripts/aizome-memory --json PID ...` for an isolated
experiment; its default discovery also includes any real Aizome desktop.
Record the destination machine's CPU, RAM, GPU/backend, kernel, compiler,
fonts, display geometry and background activity before establishing a new
same-machine control.

## Results and decisions

PSS values below use MiB. Heaptrack's allocation figures use decimal MB.
Frame figures are offscreen CPU work, not input-to-display latency.

| Experiment | Accepted result | Decision |
|---|---|---|
| Prepared-picture ownership | After eight palette cycles, retained offscreen pixels fell 44.70 → 5.59 MB; total live heap 73.10 → 34.02 MB. Final-source, matched-feature current-cache PSS was 98.91 → 96.80 MiB, with overlapping ranges in the larger-cache repetitions. | Keep the fix. It prevents live cache accumulation; glibc retention limits immediate resident savings. |
| RGB JPEG resizing before RGBA expansion | Peak allocation 198.73 → 174.51 MiB; median 264.20 → 203.99 ms, about 23% faster in seven alternating pairs. Both tested wallpapers produced identical pixels. | Keep. Other image formats retain their original path. |
| Fresh index entry reservation | Reserve the already-known number of records during construction. | Keep; no extra scan or shrink during loading. |
| Four → one codegen units, thin LTO | Three-app incremental PSS 81.81 → 80.40 MiB, overlapping ranges. Fresh shell 64.11 → 65.13 MiB, also overlapping. CPU changes were modest/mixed. | Keep four by default. |
| Dynamic Rust standard library only | 81.67 MiB versus 81.28 MiB for its no-LTO static control. | No useful saving. |
| Native Rust Kit dylib, no C-FFI wrapper | Three apps 81.28 → 73.00 MiB; lone Soroban 20.30 → 25.87 MiB. No large CPU regression in the measured cases. | Keep experimental; investigate shell-plus-app amortization before accepting ABI coupling. |
| Shell jemalloc decay, frozen intermediate source | After 60 s idle: glibc 215.90 MiB, jemalloc 176.66 MiB, same jemalloc binary with background purging 71.49 MiB. Four purge threads observed. These are single-run curves. | Strong shell-only follow-up, not an adopted policy or a guaranteed final-source saving. |
| Jemalloc in three small apps | Settled increment: glibc 82.01 MiB, default jemalloc 90.06 MiB, background jemalloc 86.04 MiB. Background policy added twelve threads across the apps. | No blanket app switch. |
| Allocator responsiveness | Settei pane-change p95 CPU: glibc 1.098 ms, jemalloc 2.240 ms, background jemalloc 2.031 ms. Some smaller updates and shell opening improved. | Existing budgets passed, but relative regressions matter. |
| Compositor jemalloc preload | With background purging, idle compositor PSS was 67.15 MiB versus glibc 65.83 MiB in one comparison. | No demonstrated idle-compositor win. No whole-system allocator policy was tested or changed. |
| Fixed glibc mmap threshold, 128 KiB | Final-source scaled shell reached 62.59 MiB PSS / 37.34 MiB anonymous PSS, stable for 60 s. Offscreen allocation-heavy paths regressed roughly 2–3×. | Do not apply globally. Explicit 128 KiB disables adaptive threshold adjustment. |
| Loaded-index shrinking | Saved 2.84 MiB live capacity but increased retained anonymous memory in worker-load/UI-drop stress: 378.52 → 390.62 MiB, same 19.79 MiB settled live allocation. | Removed. Do not restore based on container capacity alone. |
| Streaming index reader / exact-size prescan | Streaming reduced peak 43.78 → 23.76 MiB but was about 9% slower; a prescan was about 30% slower. | Helper experiments only; not adopted. |
| Release builds and symbols | `aizome-dev` now uses normal optimized profiles. Optional matching split symbols, raw Heaptrack capture and process-memory tooling are available. | Keep. DWARF was already stripped from installed apps and normally is not mapped at runtime; this is a profiling/distribution improvement, not a live-heap saving. |

The picture fix retains the image owner through the displayed window's
lifetime, while consuming the first-frame flag only once. Previously its
destruction queued `ForgetImages` before the pending `Paint`, which then
reintroduced ownerless pixels. Hide/replacement/close still releases ownership.

Small-app anonymous PSS was only about 4–6 MiB per client. Settei's sampled
client PSS included 19.25 MiB of shared memory and 11.94 MiB file-backed
memory. Allocator tuning cannot remove those costs; Wayland buffers and
shared code/fonts deserve separate treatment.

## First investigation: shell-only jemalloc with background purging

### Rebuild matched controls from current source

The measured allocator matrix froze the image fixes **and a subsequently
rejected index shrink**, identically in every arm. Its package identities
and shared dependency features were checked, so the allocator comparison
is useful, but the absolute curve is not a final-source result. Rebuild
the current source before making a policy decision:

```sh
scripts/memory-allocator-build "$HOME/aizome-shell-allocator-trial" 2
```

Use a new output directory outside Aizome, Sidestep and any warm target.
The helper snapshots the working tree, builds a glibc control first, adds
`tikv-jemallocator = 0.7.0` with
`override_allocator_on_supported_platforms`, and verifies C allocator
exports. It produces shell/Soroban/Sumi/Settei binaries in both `glibc/`
and `jemalloc/`, plus a process-scoped preload library. It preserves source,
lock files, build logs and feature listings alongside them.

For this experiment, launch **only its shell**. Keep the installed package's
compositor and app binaries on glibc. Compare three arms: matched glibc,
jemalloc with default configuration, and the **same jemalloc executable**
with `MALLOC_CONF=background_thread:true`. Leave decay and arena settings
unchanged in the first comparison. Clear inherited allocator overrides,
including `LD_PRELOAD`, `GLIBC_TUNABLES`, `MALLOC_CONF`, and `MALLOC_*`
tuning variables, before applying each arm's settings.

The current helper injects the experimental allocator through Kit. That is
convenient for comparisons but is **not shell-only production integration**.
If the shell trial succeeds, put allocator selection in the shell binary
or its own optional package feature, not a Kit-wide feature shared by all
apps. Verify the actual Nix workspace build: Cargo feature unification and
grouped-versus-shell-only builds previously confounded measurements. Keep
package selections, features and flags identical between each control.

The previous run verified both ELF exports and real C bindings from
Fontconfig, FreeType and PulseAudio. Repeat a separate `LD_DEBUG=bindings`
diagnostic on the new shell. `#[global_allocator]` alone does not prove C
coverage. Do not enable binding logging during measured runs. Private
allocators and direct mappings inside dependencies remain outside malloc
interposition.

### Isolate the session and inputs

Use the [profiling guide's isolated compositor/shell setup](profiling.md#the-compositor-and-shell),
a private D-Bus, and fresh private HOME/XDG directories. Do not replace the
real shell or share its notification service. Set `AIZOME_SESSION=0`, keep
the test compositor's session services disabled, and launch the experimental
shell with its allocator environment applied only to that command. Do not
export a preload into the user session or system services.

Copy the **same** config, wallpaper and index fixture into each arm's private
directories. The index cache lives at `$XDG_CACHE_HOME/aizome/files.idx`;
record its hash and entry count and prevent a background rebuild from
silently changing it. Test both a normal-size cache and the 230,772-entry
synthetic scale. The old synthetic fixture/generator is in the local archive,
not Git. If regenerating it, record the generator, paths, root, timestamps,
cache format and hash; a similar entry count is not an identical workload.

Begin with headless Pixman at 2880×1800@2, then repeat on the destination's
real graphics backend. Pin `AIZOME_FONT_DIR` for every new arm. The old
memory matrix used host Fontconfig while its CPU tests pinned fonts, so new
fixed-font totals form a new baseline.

### Close the missing responsiveness and efficiency evidence

1. Run at least three alternating repetitions per arm, preferably five.
   Open and dismiss the palette eight times with the same actions and timing,
   including Files search and its worker/index lifetime. Also test real
   palette typing, wallpaper replacement and multiple outputs.
2. Record shell and compositor separately: PSS, anonymous/file/SHM PSS,
   RSS, SwapPss, threads, CPU, wakeups and page faults. Sample after startup,
   each cycle, and 5/10/30/60/300 seconds idle. Extend the successful case to
   a long session rather than extrapolating an eight-cycle curve.
3. Reopen after 10/60/300 seconds idle. Measure first-frame and interaction
   tails, not just steady-state averages. Record CPU event/layout/raster
   stages separately from compositor presentation or input-to-display
   timing. The existing offscreen harness allocates/copies damage buffers;
   its allocator sensitivity is useful but is not exact live-render latency.
4. Coordinate `/tmp/aizome-benchmark.lock` with other agents/developers, and
   inspect unrelated CPU/GPU work too. The lock is cooperative. The old
   long-idle reopen observations all overlapped an inference workload and
   were excluded from latency conclusions; they must not be treated as a pass.
5. Measure background purge cost during idle and concurrent interaction,
   especially on a laptop. Only then vary thread/arena limits one at a time,
   checking the compiled allocator's actual settings. Four purge threads
   were observed in the first experiment; fewer threads might reduce cost
   but can change reclamation speed. Shorter decay is another separate arm.

Define acceptance before collecting results: a repeatable resident reduction
on final source, no meaningful interaction-tail or first-reopen regression,
and acceptable idle CPU/wakeup/power cost. Report absolute costs and ranges
as well as percentages. A lower idle PSS or a passing frame budget alone is
insufficient. Do not use swap or memory pressure to disguise a regression;
stress under pressure only in a disposable test environment.

If accepted, add an opt-in shell-only Nix/package setting with a straightforward
glibc rollback, then validate a sustained real session before considering a
default. Keep C overrides inside that shell process. Jemalloc's direct Rust
`mallocx`/`sdallocx` paths may bypass ordinary Heaptrack interception; use
uninstrumented process metrics for comparisons and a separate allocator
profiling build if detailed jemalloc heap attribution is needed.

## Other investigations, in priority order

1. **Wayland SHM retention.** Measure grow/shrink, maximize/restore and scale
   changes per app, including both client and compositor mappings. Sidestep's
   pool grows geometrically and retains touched backing storage. Any reclaim
   change belongs where SCTK/Sidestep knows buffer ownership and release:
   retain a warm reserve, reclaim only unused storage after sustained idle,
   and test the next frame. Blind `madvise(DONTNEED)` or shrinking from Kit
   is not an ownership-safe implementation.
2. **Shell plus shared Kit.** If more code sharing is needed, co-build the
   shell and varying app counts against the same native Rust dylib, including
   a persistent shell plus one app. The original solo test had no shell
   sharing Kit, so it cannot establish that configuration's first-app cost.
   Preserve the complete compiler/target/flags/features/dependency graph and
   paired artifacts in one Nix closure; matching a compiler version string
   alone is insufficient. Check startup/relocation costs and loaded mappings
   alongside PSS and interaction CPU. Keep third-party compatibility separate.
3. **Index retention without regressions.** Profile worker allocation/free
   lifetimes on the final code before proposing a new parser or retention
   policy. Rejected shrinking and fixed-mmap experiments showed why live
   byte reductions can trade badly against reuse or cross-thread frees.
4. **Complete the development reload roundtrip.** The installed package's
   logic ABI check passed, but no separately rebuilt checkout-to-package
   logic reload was performed. Run that isolated roundtrip before relying
   on the changed build defaults in daily development. Preserve panic
   unwinding and test old metadata / `--optimised` compatibility.
5. **Lower-priority compiler tuning.** Keep four codegen units and current
   LTO defaults unless a new controlled workload shows a useful benefit.
   Use explicit Cargo config for `profile.release-lto.codegen-units`; the
   hyphenated profile makes `CARGO_PROFILE_RELEASE_LTO_*` ambiguous.

## Provenance and validation boundary

| Item | Revision or evidence |
|---|---|
| Compiler/linkage baseline | `e75c905dd04535796792ff0862af0210ee175e01` |
| Source fixes | `557ad1a` |
| Runtime/profiling implementation and fully validated Nix package | `f88ac0eb9e908f26f8e7201af2311d749c428851` |
| Experiment snapshot/cache hardening | `a0f0eab` |
| Full report and committed summary data | `4810fc4` |
| Upstream integrated for this handoff | `0e93d4c`, merge `71cd651e18f165afbdc76a75a5229dc0d448f8b5` |
| Sidestep | `dae957e913376a26e642d6abee533eb755fb9092` |
| Measured compiler / host | Rust 1.98.1 (`48a229cea`), LLVM 21.1.8; Ryzen 7 9800X3D, 16 logical CPUs, approximately 64 GiB RAM |
| Memory-matrix compositor | Installed `fcdb02f`, private headless Pixman, 2880×1800@2 |
| Allocator | `tikv-jemallocator 0.7.0`, `tikv-jemalloc-sys 0.7.1`, bundled jemalloc 5.3.1 fork; statistics/profiling off |

Before the upstream merge, all 35 focused correctness tests and 105 accepted
CPU timing invocations passed. Both Nix outputs built successfully. All 23
Aizome ELF/debug pairs matched build IDs and `.text` addresses; runtime
DWARF was absent, split line tables present and unwind tables retained.
GDB listed Soroban source from the debug output's exact retained source.
The runtime closure excluded debug output, retained source and compiler.
The upstream prebuilt `libstd` already lacked DWARF/build ID and therefore
has no extracted source symbols.

An isolated installed compositor/Soroban launch and close passed with no
`LD_LIBRARY_PATH`; installed logic reported “would reload; core unchanged.”
`aizome-dev` selected `release` logic and `release-lto` standalone builds,
with identical settings for the compatibility `--optimised` flag. Script
fixtures also covered deletions, unusual filenames, dangling symlinks and
independent copies of warm build caches.

The upstream merge was clean and keeps those measured commits available;
the PR diff excludes the already-merged Yukimi work. **The historical full
build/tests and performance values describe their recorded revisions, not
the merged branch as a freshly benchmarked whole.** Rebuild and establish
fresh controls before testing on the next machine.

At merge `71cd651`, offline Nix evaluation with builds disabled passed for
both x86_64-linux and aarch64-linux. It preserved the profiles, split debug
output and profiling tools alongside upstream's libsecret/network-agent
integration. Full `cargo metadata --locked --offline` passed; all 518 locked
package identities/checksums matched upstream, with only the retained
`aizome-face` → `image` dependency edge differing. No post-merge native
package build, network-auth VM run or performance run was performed.

## What travels with Git, and what does not

Git contains all retained source changes, allocator/linkage build helpers,
the memory/profiling tools, the full report, its figures, and the 129 KB
`memory-results.json` with accepted summaries, ranges, exclusions and package
validation. That is sufficient to understand the results and start new
experiments without the original host.

The original host retains approximately 20 GiB under
`/home/callen/work/aizome-memory-results`. This is **not pushed**: it contains
raw Heaptrack traces, frozen binaries, compiler targets, screenshots,
synthetic/current index fixtures, feature/lock manifests, measurement
runners and full logs. Several runners embed machine-specific paths.
`evidence:/...` references in the committed JSON resolve relative to that
directory; they are provenance, not downloadable GitHub files.

Useful local archive entries include:

- `heap/publishable-summary.json`, `heap/final-notes.json`, the `heap/before-raw/`
  and `heap/after-raw/` capture directories, and `heap/scaled-files.idx`.
- `memory-summary.json`, `latency-summary.json`, `shell-cgu-summary.json`,
  `allocator-memory-summary.json`, `allocator-matched-decay.json`, and
  `allocator-latency/summary.json`.
- `allocator-controls/` for matching locks/features; `allocator-reopen-summary.json`
  for the explicitly excluded long-idle timing observations.
- `measure_variants.py`, `latency_matrix.py`, `allocator_latency_matrix.py`,
  the `heap/` workload scripts, and `assemble_report_data.py`; adapt paths
  deliberately, and retain new scripts/fixtures with the next result set.
- `nix-package-validation.json`, plus rooted `nix-package-out` and
  `nix-package-debug` symlinks for the verified package.

If exact raw reanalysis is needed, copy the archive separately from the
original host using its SSH hostname, for example:

```sh
rsync -a --info=progress2 ORIGINAL_HOST:/home/callen/work/aizome-memory-results/ \
  ../aizome-memory-results/
```

That preserves symlinks but does **not** transfer their Nix store targets or
every frozen binary's runtime dependencies. The verified package outputs
and their closures can be copied separately:

```sh
nix copy --from ssh://ORIGINAL_HOST \
  /nix/store/yj2697z1yf2p8hq0rppbi1r14w7f78pq-aizome-0.1.0-f88ac0e \
  /nix/store/cs6ibbgdk012axf5yj5p46qvcv924bz2-aizome-0.1.0-f88ac0e-debug
```

These package outputs do not replace the different experimental binaries.
The copied package and frozen experimental binaries target x86_64-linux;
rebuild to execute on aarch64. Cross-analysis of recordings is separate
and depends on the analysis tool's support.
Raw Heaptrack interpretation needs the exact captured executables/libraries
and matching symbols, with paths resolved on the destination. Transfer the
paired experimental DSOs and required store dependencies too if reanalyzing
them; do not substitute a new checkout's symbols. Otherwise regenerate a
fresh experiment using the committed helpers and keep its own manifests.

No original experiment processes remain running. Other developers' builds
and the real desktop were left alone.
