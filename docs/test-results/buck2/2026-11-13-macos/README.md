# Mac Buck2 / NativeLink probe — 2026-11-13

This records the pre-implementation baseline. The subsequent
[APFS implementation and qualification](../2026-11-13-apfs/README.md)
supersedes the storage limitations and proposed storage work below.

The Sekka-patched Buck2 engine and NativeLink 1.8.3 run natively on this
Apple M5 Max. Cache correctness and invalidation passed a small Rust fixture.
Sekka's complete Cargo/Nix frontend and its single-CAS extent sharing are
still Linux-only. This is a bounded feasibility test, not an Aizome build,
Linux performance comparison, ISO test, or native Mac support certification.

## Exact inputs and method

Tested `feat/buck2-single-cas` at `665f9522`, the fetched head of draft PR
[#334](https://github.com/bitemyapp/sekka/pull/334), stacked on #291. Main did
not contain this work. Changes are on `codex/buck2-macos` based on that tip;
there was no merge, deployment, or publication change.

The engine used upstream `9a1876786f32806f6b65fc51f4c1c3077efc38a5` and all
20 patches listed in `nix/buck2/engine.nix`, applied in order. It compiled
natively with the pinned nightly of 2026-07-05, Cargo's release profile and
incremental compilation disabled in 3m57s. This was a Cargo bootstrap, not
the Nix derivation: this Mac has no Nix. [fingerprint.json](fingerprint.json)
records patch hashes, the resulting binary hash, compiler, hardware and
commands. NativeLink came from its official
[1.8.3 release](https://github.com/TraceMachina/nativelink/releases/tag/v1.8.3).

Hardware: macOS 27.0.1, Apple M5 Max, 18 logical CPUs, 128 GiB RAM, APFS.
Normal interactive host, no power/governor/cache tuning. Final engine timings
were taken after engine compilation finished, with warm OS caches. Earlier
upstream-binary/harness trial runs are excluded. This small run set does not
support tail percentiles or claims about large real-workspace build times.

`scripts/buck2-macos-probe.py` creates a private loopback NativeLink cache
with hash/size verification and action-result completeness checking, then
compiles a 19-action synthetic Rust DAG: base → middle → 16 leaves → binary.
Each leaf has 250 functions. All 19 outputs are explicitly requested,
including in the fresh workspace, to avoid confusing lazy materialization
with storage savings. The fixture uses rustc 1.98.1, opt-level 1, no debug
info, no Cargo incremental state, and a separate macOS platform/cache identity.
It checks executable results and exact action counts after every change.
This deliberately bypasses the Linux-only Sekka Cargo action wrapper.

## Build results

| Scenario | Runs | Wall time (median; range) | Compiler actions |
| --- | ---: | ---: | ---: |
| Empty private CAS and new daemon | 1 | 0.621 s | 19 local |
| Zero changes | 5 | 0.017 s; 0.016–0.017 s | 0 |
| Small edit to final binary | 3 | 0.237 s; 0.232–0.272 s | 1 local |
| Edit shared base, three levels below binary | 3 | 0.492 s; 0.490–0.520 s | 19 local |
| Revert to original sources | 1 | 0.023 s | 19 cache hits, 0 local |
| Fresh workspace, all outputs materialized | 1 | 0.218 s | 19 cache hits, 0 local |

The dominant measured cost is compilation after invalidation; small edits
cost much more than no-op graph checks. This is action-level evidence, not a
CPU profile or a reason to alter Linux scheduling. Cache reuse worked across
roots without recompilation. Remote execution, eviction/restart recovery,
authentication, proc macros, dynamic libraries and compiler incremental-state
reuse were outside this fixture.

## Storage results

| Stage | NativeLink CAS logical bytes | Buck artifact logical bytes |
| --- | ---: | ---: |
| Cold | 2,719,005 | 5,400,688 |
| Five zero-change builds | 2,719,005 | 5,400,688 |
| After three small edits | 4,131,111 | 6,812,368 |
| After three deep edits | 12,285,795 | 14,913,400 |

No-change builds added no CAS objects or artifact bytes. Logs/database state
did grow, and are counted separately in [summary.json](summary.json).
Locally produced outputs have both placeholder and published paths, and old
generations remain; a fresh workspace materialized 19 outputs totalling
2,700,344 bytes, all SHA-256-identical to blobs already in the CAS.

These are **logical** sizes. Summing `st_blocks`/`du` counts shared APFS blocks
more than once, so neither measure establishes physical duplication or savings.
The explicit NativeLink-local cloning/content-store paths are compiled out on
Darwin, and `cas_dedupe.py` uses Linux FIEMAP/FIDEDUPERANGE. The ordinary gRPC
cache works; this run does not establish Sekka single-CAS physical sharing on
APFS. Upstream file-copy behavior is not treated as proof of CAS sharing.

A separate real-filesystem probe used a disposable 2 GiB APFS sparse disk
image to avoid unrelated host writes contaminating free-space accounting:

- One random 64 MiB donor, eight clones: 512 MiB of logical views, **0 bytes
  additional allocation reported by `statvfs`** after sync, at its accounting
  granularity. This is not a claim that APFS metadata is free.
- Eight explicit byte copies: **536,879,104 bytes** additional allocation.
- Overwrite 1 MiB of one clone: **1,048,576 bytes** additional allocation;
  the donor and other clones retained their exact hashes.
- All clones had distinct inodes with link count one; they survived donor
  deletion. Thus hardlink aliasing was not mistaken for copy-on-write.
- Median clone plus file fsync: 0.218 ms; byte copy plus file fsync: 44.9 ms.
  Neither includes directory fsync/F_FULLFSYNC. Engine linking overlapped this
  separate storage probe, so these timings are indicative, not a latency gate.

[apfs-storage.json](apfs-storage.json) holds all samples. The disk image was
detached and probe data removed. APFS cloning is viable; integrating it safely
into the current engine and CAS publication lifecycle remains work.

## Portability findings and changes

1. `scripts/sekka-buck2 targets` fails at missing `flock` on this Mac. Installing
   that utility alone is insufficient: the root flake exposes x86_64-linux,
   and `nix/buck2/engine.nix` selects mold and Linux-specific checks/metadata.
2. `action.py` rewrites execution paths to `/proc/self/cwd` and selects mold;
   `generate.py` assumes `.so`, and `run.py` uses Linux runtime library paths.
   Sparse imported-state fallback invokes GNU `cp --reflink --sparse`.
   Merely disabling reflinks would not produce a working native frontend.
3. The existing relocation regression failed six assertions because macOS
   resolves `/var` through `/private/var`. Fixed it by checking canonical
   ownership bases after the existing lexical fast path. Added a checkout
   alias regression that also runs on Linux and verifies external symlinks
   remain external. Ordinary Linux matching takes the same fast path.
4. Corrected the dedupe-test skip message: an unavailable Linux ioctl does
   not mean the filesystem has no cloning API.

Six focused Python tests pass on macOS and in a disposable Linux/aarch64
Python 3.13 container. The Linux extent test correctly skips on this Mac;
there was no new Btrfs or XFS qualification. No engine patches, Linux storage
operations, scheduler settings or release workflows changed. Only the action
source fingerprint in the graph lock was refreshed; Cargo resolution and
generated targets are unchanged. The root-flake checks were not runnable here.

## Recommended next work

Keep the proven Linux path intact. For Linux/Aizome builds today, invoke the
existing Linux checkout/build service from the Mac. A native Darwin frontend
needs its own root-flake toolchain and platform identity, correct dynamic
library naming/loading, and a stable execution-root strategy that does not
embed checkout paths into cached runtime behavior. Keep the Cargo workspaces
and narrow source inputs.

For native storage, introduce an OS-specific clone primitive: preserve Linux
FICLONE/FIDEDUPERANGE unchanged, and use Darwin `fclonefileat` for independently
owned destinations at publication/materialization time. Darwin requires a
nonexistent destination, so simply widening the current Linux `cfg` guards
is wrong. Preserve digest checks, byte/entry bounds, eviction recovery and
durability ordering. Leave the Linux inotify preparation pipeline untouched.

Do not translate the post-build dedupe pass into an unlocked clone-and-rename
of live files. Linux's kernel compares and shares existing extents without
replacing the inode; cloning a new APFS inode and replacing a pathname can
lose concurrent writes or leave open handles referring to old data. Arrange
sharing during immutable publication or under ownership/quiescence guarantees,
then test corruption, concurrent readers/writers, eviction, and crash recovery.
Keep the existing Linux Btrfs/XFS acceptance checks as regression gates. This
work is a separate port, not a quick switch to turn on after this probe.

## Reproduction and evidence

Run the native graph fixture with explicit binaries:

```sh
python3 scripts/buck2-macos-probe.py --buck2 /absolute/path/to/patched/buck2 \
  --nativelink /absolute/path/to/nativelink --rustc /absolute/path/to/rustc \
  --output /absolute/path/to/new-private-results
```

For storage, create and mount a private APFS disk image, run
`python3 scripts/buck2-apfs-probe.py /absolute/path/to/image-mount`, then detach
it. The script creates and removes only its own temporary directory.
Apple's installed `clonefile(2)` manual describes the tested clone semantics.

`events.tar.gz` contains the final per-build events, action inventories and
stderr; `engine-build.log.gz` records the exact native engine compilation.
Raw local artifacts and binaries remain under the directory in the fingerprint.
The retained engine is `~/.cache/sekka/mac-probe-20261113/buck2`; its 2.8 GiB
of compilation intermediates and the detached disk image were removed.
The installed engine-build nightly remains available through rustup.
Services and Buck daemons created for the probes were stopped.
