# One CAS per host: engine fixes and targeted repros, 2026-11-06

Targeted repros on `wx-workstation` (128 logical CPUs, Btrfs `compress=zstd:1`)
after the [storage-plan run](../2026-11-06-storage-plan/README.md), on branch
`feat/buck2-single-cas`. Each is one observation, not a benchmark matrix. The
engine changes are patches in [`nix/buck2/patches/`](../../../../nix/buck2/patches/):

| Patch | What it changes |
| --- | --- |
| `presence-consistent-hits.patch` | Cache uploads ask the CAS about every output instead of trusting remembered expirations |
| `content-store-unlocked-views.patch` | Views of stored objects are cloned, verified and synced after the store locks are released |
| `content-store-advisory-writes.patch` | LRU accesses and view hints are buffered rather than each costing a synchronous commit |
| `local-cas-downloads.patch` | On the CAS host, cache hits are cloned from NativeLink's file before any store lookup |
| `local-cas-outputs.patch` | On the CAS host with cache writes, produced outputs skip the content store and are journaled for the client |

## Lost cache entries after eviction

Repro: one daemon builds dev; the CAS is shrunk to 1 GiB and restored; the
same daemon builds release; a fresh worktree builds release. A disk probe
then checks every release action result against the CAS files it names.

| Engine | Results naming missing blobs | Fresh release worktree |
| --- | --- | --- |
| Storage-run engine | 79 of 81 build-script results (1,032 files), 1 compile | 646 cached, 96 executed |
| With `presence-consistent-hits.patch` | none | 742 cached, 0 executed |

The storage run's 85 lost entries had the same cause. The restart before
`release-first` was coincidental.

## Concurrent fresh worktrees

Repro: worktrees primed from the cache, each then rebuilding a different
`aizome-theme` edit at `-j4` (55 actions); a fresh worktree materializes about
22,500 files for this, 18,768 of them incremental state.

| Engine | Four at once, each | One alone | Content store afterwards |
| --- | --- | --- | --- |
| Storage-run engine | 222 s (five at once) | — | — |
| + unlocked views | 125–137 s | 43 s | — |
| + advisory writes | 68–93 s (two runs) | 35.5 s | — |
| + local CAS downloads | 88 s, all in lockstep | 10 s for a cached build | 10.8 GB |
| + local CAS outputs (this branch) | 40 s | — | 109 MB |
| Content store off, for comparison | 37–38 s | — | — |

A worktree that needs no materialization takes 32–36 s.

The lockstep finishes were queues on the store's exclusive lock:
- first, lookups recovering CAS-only blobs from peers' view hints;
- then, ingesting produced outputs. These went through two paths, the
  declaration and the output pipeline; the pipeline path was the one
  that had been missed.

Shared lookups with writer preference made no difference (86–88 s) and were
dropped. With outputs kept in the CAS, each of the four worktrees kept 23 MB
of its 6.3 GB `buck-out` unshared with the CAS.

## Fresh worktree on the CAS host

A fresh worktree whose 706 dev actions were all cached:

| Client | Wall time | Content store left unshared with the CAS |
| --- | --- | --- |
| Downloads, then the post-build extent pass | 28 s | 46 MB of 8.6 GB |
| Clones from the CAS (`local-cas-downloads.patch`) | 10 s | 9 MB of 54 MB |

## Copies the journal missed

Trace: a cold dev build on a fresh service. The content store's objects were
matched to `buck-out` files by digest, and every file's first extent was
compared with its CAS blob's.

The content store still held 1.15 GB after the build: 1,054 objects that the
CAS also held. They were the published copies of content-based outputs. A
locally executed content-based output is written to a placeholder
(`…/output_artifacts/…`) and published as a copy at its content path. The
copy was ingested into the store, and only the content path was journaled.
After both passes, one of the two buck-out files kept the extents the action
wrote:

| Engine and client | buck-out files apart from the CAS | Worktree's exclusive bytes | Content store |
| --- | --- | --- | --- |
| Before | 1,066 files, 1,114 MB (the content paths) | 1.10 GB of 6.64 GB | 1.15 GB |
| Copies listed instead of ingested | 1,066 files, 1,114 MB (the placeholders) | 1.09 GB | 28.7 MB |
| Placeholders listed too, store pass first (`54ff881a`) | 68 dependency files, 0.4 MB | 0.02 GB | 28.7 MB |

In the first row, the client's store pass, which moves CAS blobs onto store
objects' extents, ran after the journal pass. It left the journaled content
paths on extents nothing else used. The fix lists the placeholder at
publication and runs the store pass first. Fresh worktrees were never
affected, because their hits are cloned straight to the content path. The
storage-plan runs' cold and release stages predate the fix.

A cold release build (742 actions) on the fixed engine kept 0.03 GB of its
16.67 GB `buck-out` to itself. Its 4,636 placeholder and published files
(12.2 GB) all shared their CAS blobs, and the content store held 36.1 MB. The
journal pass shared 10.0 GB in 15.8 s, one pair at a time. On up to 8 threads
(`1cfc5199`), a release shared-crate edit's pass (55 actions in 60.5 s)
shared 6.6 GB in 3.5 s. The worktree then kept 0.03 GB of its 19.96 GB
`buck-out` to itself.

## Stale outputs in a worktree

An edit gives each changed content-based output a new content-path
directory and leaves the old one. Buck2's stale cleaning was off, and it
needs deferred write actions, without which `clean --stale` printed
"Skipping clean". With both on (`.buckconfig`), on a fresh service:

| Step | Result | buck-out (exclusive) |
| --- | --- | --- |
| Release cold build | 742 actions in 110.7 s | 10.06 GB (0.01 GB) |
| Shared-crate edit | 55 actions in 56.5 s | 13.35 GB (0.01 GB) |
| `buck2 clean --stale=1m` | 4,457 paths, 12.4 GiB, in 0.9 s | 0.01 GB |
| Release build again | 742 cached, 5.0 s | 5.02 GB (0.01 GB) |

The configured policy is a daily pass for artifacts unused for 7 days, or
12 hours under 10% free. Soroban's tests passed with deferred write actions
(242 actions, local execution).

## Free-space sizing

`build-service --local … autosize --dry-run` on the qualification service
reported 1.83 TB free, a 383 GB floor (10% of the filesystem) and 68.6 GB of
logical CAS. It would have raised the bound to the 256 GiB maximum. It was a
dry run; nothing was resized.

## A resize under running builds

Repro: the resize `autosize` would make (same bounds; workers and backend
restarted, gateway kept) lands a few seconds into a build of a different
`aizome-theme` edit. The local-CAS writer client was used, and the restart took
1.0–1.2 s on a 20–24 GB CAS.

| Gateway | Build under the resize | Result |
| --- | --- | --- |
| HTML 502 while the backend is down | primed worktree, 55 actions executed and uploaded | pass, 18.1 s; uploads retried on GOAWAY |
| HTML 502 while the backend is down | fresh worktree, 3 s in | **fail**: 30 blob reads (`BatchReadBlobs`) got the 502, which gRPC clients cannot retry |
| `UNAVAILABLE`, before the pause below (`9bd7a3ef`) | fresh worktree, twice, 3 s in | pass: 706 actions, 651 cached; in the second, 15 requests retried, reaching attempt 3 of 5 |

All 225 502s fell within one second, as the gateway's log shows. Buck2
retries `UNAVAILABLE` for about 1.5 s (100–800 ms backoff), so a markedly
slower restart would still fail reads. `autosize` therefore also waits for
`--idle-seconds` without gateway requests. A dry run over the last 15 minutes
of these builds counted 30,279 requests and deferred. The gateway also stops
retiring connections after 1000 requests (nginx's default), which had caused
GOAWAY retries in builds with no restart at all.

**At the CAS's maximum size.** A disposable service was given synthetic CAS
entries (startup reads only names and metadata). A probe through the gateway
every 50 ms timed what clients saw during a `resize`:

| CAS entries | Clients see `UNAVAILABLE` for |
| --- | --- |
| 0 | 0.48 s |
| 42,000 (today's 24 GB) | 0.54–0.60 s |
| 500,000 (about the 256 GiB maximum) | 1.67–2.57 s |

NativeLink loads its whole index before it listens, so a full cache outlasts
Buck2's 1.5 s of retries. `resize` and `runtime` now pause the gateway during
the restart and resume it once the backend accepts connections. At 500,000
entries, eight concurrent probe loops through two resizes got no errors in
1,573 probes, and the longest stall was 1.74 s. On the same service, a cold
prime (706 actions, 90.8 s) was followed by two fresh worktrees, each with a
held resize landing 3 s and 7 s in. Both built all 706 actions from the cache
(8.6 s and 9.6 s). The first saw six requests retried once.

**The post-build pass at that size.** The prime's pass, which shared 5.4 GB
of outputs with their CAS blobs, took about 6.7 s. The kernel's byte
comparison was not the cost: 2.73 GB of fresh pairs shared in 0.20 s. The
cost was the client's index of the CAS directory, 3.2–4.5 s at 500,000
names, which each pass built twice. One `os.scandir` pass now serves both and
takes 0.62–0.66 s. On another disposable service with 500,000 synthetic
entries, the client's own reports gave:

| Build | Journal pass | Content-store pass |
| --- | --- | --- |
| Cold, 706 actions in 83.5 s | 1.96 s: 11,785 outputs, 5.4 GB shared | 0.25 s: 1,054 objects, 1.1 GB |
| Shared-crate edit, 55 actions in 19.0 s | 1.40 s: 443 outputs, 3.0 GB shared; 15,375 already shared | 0.09 s |

Fresh worktrees whose hits are clones journal nothing.

Two things this repro turned up were not faults:
- A primed build missed the cache entirely because its isolation directory
  differed from the earlier repro's. Buck2 puts
  `buck-out/ISOLATION/` into every action's paths.
- The journal pass reported 108 outputs' blobs missing. All were empty files,
  whose blob a CAS never stores. The pass now skips sub-block outputs before
  looking them up.

A transient `systemd-run --user` timer for `autosize` failed on wx because
its user service manager predates the user's `docker` group. The same timer
ran `autosize` and exited cleanly through `sg docker -c`.

## Storage-plan run on this branch

The same 31-stage plan as the storage run, at `c1508843` on a fresh service
(`shared-build-service-cadfe642c2e5`). The client was generated with
`--local-cas`: local execution, writer cache, hits cloned from the CAS,
produced outputs journaled and shared with it. [config.json](config.json),
[summary.json](summary.json) and [storage.json](storage.json) hold the frozen
configuration, the harness summary and every stage's unions and attribution.

| Gate | Result | Storage-run engine |
| --- | --- | --- |
| Correctness | pass: 31 observations, no failed builds or native retries | pass |
| Physical sharing, per added worktree | pass: +21.8, +20.5, +20.5, +21.0 MB (limit 498 MB) | pass: −1,054 to +168 MB |
| Fresh worktrees reuse everything | pass: 0 compilers each | pass |
| Warm worktree ≤20% of cold | **pass**: 5.8–7.2% (6.3, 5.7, 5.9, 7.1 s of 98.8 s) | fail: 15–23% |
| No-op | pass: 0.22 s | pass: 0.16 s |
| Restart, primed output loss | pass | pass |
| Eviction recovery | **fail**: `resize` crashed, see below | pass |
| Edits (reported) | leaf 2.0 s, shared 19.4 s, icon 16.8 s | 1.7, 16.5, 17.3 s |

| Stage | Active union | Storage-run engine |
| --- | --- | --- |
| dev-cold | 9.34 GB | 13.23 GB |
| dev-revert (after three edits) | 13.70 GB | 21.84 GB |
| dev-fresh-e | 13.79 GB | 21.02 GB |
| dev-divergent-j4 (five worktrees at once, 42.5 s each) | 23.83 GB | 32.83 GB (222 s each) |

`release-fresh` reused all 742 release actions in 6.3 s, where the storage run
re-executed 85 in 96 s.

The eviction and capacity-restore stages failed because the controller's
`resize` crashed. Moving it into a helper for `autosize` let a subparser
variable shadow it. Nothing was evicted, so the later stages ran on an
unevicted cache, and this run's final union cannot be compared with the
storage run's. The bug is fixed in `84954a1a`, and a controller check now
fails if `main()` rebinds a module function.

**Eviction repro with the fixed controller** (earlier service, local-CAS
client, a fresh isolation directory):
1. Tree A built cold.
2. The CAS was resized to 1 GiB and back.
3. A's no-op afterwards ran nothing in 0.2 s: its outputs outlived their
   evicted blobs.
4. A fresh tree B rebuilt all 706 actions correctly in 79 s, because the
   evicted blobs made every cache entry incomplete.
5. Soroban's tests passed in B, and B's pass shared 1,551 rebuilt outputs
   (788 MB) with the CAS again.

## Checks

On wx, at the branch's commits:
- `checks.x86_64-linux.buck2-backend` passed. It now runs
  `build/buck2/test_cas_dedupe.py`, which executed on the Btrfs builder:
  shared, mismatched, journaled, empty, changed and repeated cases.
- `checks.x86_64-linux.build-service-inventory` passed, with the autosize,
  deferral and dispatch tests.
- At `1cfc5199`, both checks passed again, and the engine's check phase
  passed at `54ff881a` (engine patches unchanged since).
- The gateway configuration passed `nginx -t` in the pinned nginx image.
- The engine's check phase passed, including the new content-store,
  presence and local-CAS tests.

No broad flake check was run. These are build-system observations, not image,
installer or hardware qualification.

Both services used here were destroyed with their state afterwards. The run
directory stays on wx under `/home/callen/sekka-work/buck2-singlecas-20261106`.
