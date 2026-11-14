# APFS NativeLink CAS implementation — 2026-11-13

Native APFS sharing is implemented in `apfs-local-cas.patch`, on the
Sekka-patched Buck2 engine used by draft PR [#334](https://github.com/bitemyapp/sekka/pull/334).
The complete Sekka Cargo/Nix development frontend remains Linux-only. These
are engine and build-system component tests, not an Aizome build or ISO qualification.

## Implementation and ownership

On macOS, local-CAS cache hits use `fclonefileat` into private download stages;
the existing download digest check still runs before publication. Successful
local actions clone and verify private upload snapshots, upload through
NativeLink's authorized API, refresh the local CAS index, and clone the CAS
back into their still-owned outputs. This runs before materializer declaration,
under the executor's output lease and host-sharing permit. Placeholder-to-content
copies use verified clones too. Published views have distinct writable inodes;
NativeLink's blob permissions and bytes are never modified by the engine.

APFS cannot deduplicate existing extents in place. There is deliberately no
unlocked post-build clone-and-rename pass on Darwin. Linux keeps its existing
FICLONE/FIDEDUPERANGE implementation, journal and preparation pipeline. The CAS
index moved to a shared module with the Linux lookup/scan behavior unchanged;
only Darwin publishers use the new explicit post-upload refresh.

Local-CAS opt-in, action/platform upload permissions, size limits, successful
completion and SHA-256 are required for early output sharing. Cache readers
can clone existing hits. Unsupported cloning or a separate CAS volume uses
the ordinary cache path. Network failures in the optional early upload defer
to the normal uploader; integrity failures fail the build. Snapshot batches
are bounded to 16 files/64 MiB, with a larger file alone, and four concurrent
batches. No detached publication task can continue after its owner is cancelled.

Private capture directories have per-capture locks and a parent lock covering
creation/scavenging. Stale cleanup preserves active captures and unrecognized
entries. Publication syncs the file, renames it and syncs its parent. This is
not sudden-power-loss qualification; stale-stage tests model abandoned captures.

## Physical allocation

An isolated 4 GiB APFS sparse image held private NativeLink 1.8.3 services and
64 MiB random output fixtures. Free-space differences use `statvfs` after
`sync`, including metadata and build logs. They do not sum `du`/`st_blocks`,
which count shared APFS extents repeatedly. Baseline and enabled runs were
sequential; baseline used the preceding 20-patch engine and enabled used the
final 21-patch engine. Both binaries are fingerprinted.

| Measurement | Baseline | APFS sharing |
| --- | ---: | ---: |
| Cold physical allocation, 64 MiB artifact | 128.38 MiB | 64.39 MiB |
| Additional fresh workspace | 64.31 MiB | 316 KiB |
| Additional allocation after overwriting 1 MiB of fresh view | 4 KiB | 1 MiB |
| Cold build, one sample | 0.493 s | 0.644 s |
| Fresh workspace, one sample | 0.295 s | 0.186 s |

The baseline overwrite updates already-private extents. The shared overwrite
allocates new extents, while the original output and CAS blob retain their
SHA-256. Distinct inodes and survival after donor deletion rule out hardlink
aliasing. Fresh materialization used about 99.5% less additional physical
storage in this fixture. Metadata is not free, and this is not an extrapolation
to a full desktop build.

Both end-to-end runs passed concurrent fresh materialization, per-action and
platform read-only policy, upload size limits, failed and cancelled producers,
CAS deletion/restart recovery, same-size corrupted-blob rejection and repair.
Existing clones survived CAS loss and corruption; no private stages leaked.
Fault injection modified only the disposable CAS while its service was stopped.

## Build times

The same final engine ran a 19-action Rust DAG, first with sharing disabled,
then enabled: base → middle → 16 leaves → binary. Each leaf contains 250
functions. All outputs are requested, and every run checks executable results
and exact local/cache action counts. Rustc 1.98.1 uses opt-level 1, no debug
information or Cargo incremental state. Each mode starts with an empty private
cache and daemon; OS caches are warm. Native engine compilation had finished.

| Scenario | Runs per mode | Disabled median | APFS median | Work |
| --- | ---: | ---: | ---: | --- |
| Cold | 1 | 0.627 s | 0.999 s | 19 local actions |
| Zero changes | 5 | 0.0167 s | 0.0166 s | 0 actions |
| Small final-binary edit | 3 | 0.249 s | 0.251 s | 1 local action |
| Deep shared-base edit | 3 | 0.497 s | 0.919 s | 19 local actions |
| Revert | 1 | 0.0226 s | 0.0225 s | 19 cache hits |
| Fresh workspace, every output | 1 | 0.247 s | 0.238 s | 19 cache hits |

The storage saving has a measured cold/deep-rebuild cost on this small graph.
Deep rebuilds ranged 0.495–0.511 s disabled and 0.877–0.991 s enabled. Extra
snapshot verification, transport and durable publication are in the enabled
path; this run does not attribute time among them. No-op and small-edit times
were similar. These few runs do not establish tail latency or large-workspace
performance. Linux does not execute the Darwin upload/publication path.

## Validation and limits

- Native release engine built using pinned nightly `2026-07-05`.
- Nine native APFS Rust tests passed: integrity, writable/executable modes,
  donor deletion, snapshot write isolation, later output changes/replacements,
  symlink/FIFO rejection, stale cleanup, actual punched sparse holes, CAS
  generation refresh, eight concurrent capture workers and cross-volume fallback.
- Existing download verification/atomic-publication regression passed on macOS.
- Eight focused Python regressions passed on macOS and Linux. Nix's narrow
  backend-test source includes the newly imported client module.
- All 21 patches apply cleanly to the pinned upstream archive and reproduce
  the tested edited sources and Cargo manifest/lock. Generated graph input and
  output hashes match; no dependency or target regeneration was needed.
- The exact final engine derivation was built on `wx-workstation` using the
  root-lock-pinned Nixpkgs/compiler, with its complete check phase on Btrfs.
  Linux dedupe tests passed sharing, mismatch, journal, repeat, interrupted-pass
  leftovers and late appends. See `linux-build.log.gz` and `linux-python.log`.
- No fresh XFS qualification, full Linux workload performance comparison,
  authenticated gateway test, remote execution, whole-frontend Darwin Nix
  build or sudden-power-loss test was performed. Existing Linux operations
  remain in place; earlier Btrfs workload evidence remains in the PR.

## Reproduction and artifacts

With the pinned upstream source and all `engine.nix` patches applied:

```sh
CARGO_PROFILE_RELEASE_INCREMENTAL=false cargo +nightly-2026-07-05 build --locked --release --bin buck2 -j 12
CARGO_PROFILE_RELEASE_INCREMENTAL=false cargo +nightly-2026-07-05 test --locked --release -p buck2_execute_impl --lib -- materializers::apfs::tests
python3 scripts/buck2-macos-probe.py --buck2 /absolute/buck2 --nativelink /absolute/nativelink --rustc /absolute/rustc --local-cas --output /absolute/new-graph-results
python3 scripts/buck2-apfs-integration.py --buck2 /absolute/buck2 --nativelink /absolute/nativelink --local-cas --output /absolute/disposable-apfs-mount/new-results
```

Repeat the Python fixtures without `--local-cas` for the baseline. Set
`SEKKA_APFS_TEST_OTHER_VOLUME` to a disposable mounted image when running Rust
tests to exercise EXDEV; otherwise that assertion is explicitly skipped.
The integration script requires a new directory directly under a mount; it
must never be pointed at a production cache. The disposable image was detached
and removed after recording evidence; all probe services/daemons were stopped.

`fingerprint.json` records patch, engine and script hashes and host details.
`graph-*.json` and `storage-*.json` retain samples; corresponding event archives
retain per-build logs. Engine build/unit-test logs are compressed separately.
The native engine is retained at `~/.cache/sekka/mac-probe-20261113/buck2-apfs`.
The Mac has no Nix; its engine was bootstrapped with Cargo using the same source
and pinned nightly, rather than claiming a Darwin Nix derivation build.

## Standalone extraction follow-up

The Shikka Cargo importer exposed rustc hardlinked incremental session outputs.
Replacing one alias changes the old inode ctime and falsely tripped another
output's publication guard. APFS now detaches verified aliases while Buck2 owns
the outputs, before recording the later guards. Linux storage code is unchanged.

All 10 APFS unit tests passed (the optional second-volume case was not exercised
in this run). The real NativeLink fixture also passed all 10 integrity/lifecycle
checks on a new disposable APFS image: a 64 MiB cold output used 67,592,192 bytes,
a fresh clone used 323,584 bytes, and a 1 MiB write allocated 1,048,576 bytes.
Evidence: `incremental-hardlinks-tests.log.gz` and
`incremental-hardlinks-storage.json`. The new patch was reapplied over its
preceding engine patches and compared byte-for-byte with the tested fork.
