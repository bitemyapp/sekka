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

## Free-space sizing

`build-service --local … autosize --dry-run` on the qualification service
reported 1.83 TB free, a 383 GB floor (10% of the filesystem) and 68.6 GB of
logical CAS. It would have raised the bound to the 256 GiB maximum. It was a
dry run; nothing was resized.

## Checks

On wx, at the branch's commits:
- `checks.x86_64-linux.buck2-backend` passed. It now runs
  `build/buck2/test_cas_dedupe.py`, which executed on the Btrfs builder:
  shared, mismatched, journaled, changed and repeated cases.
- `checks.x86_64-linux.build-service-inventory` passed, with the autosize
  tests.
- The engine's check phase passed, including the new content-store,
  presence and local-CAS tests.

No broad flake check was run. These are build-system observations, not image,
installer or hardware qualification.
