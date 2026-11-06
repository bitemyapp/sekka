# Buck2 storage-first qualification, 2026-11-06

One run of the harness's storage plan (`scripts/shared-build-bench.py`,
`"plan": "storage"`) on `wx-workstation` (host `theophrastus`, 128 logical
CPUs, one Btrfs filesystem mounted `compress=zstd:1`). It ran 31 stages, each
once: cold, no-op, one edit of each kind, revert, four fresh worktrees,
divergent edits in five worktrees, service eviction, post-eviction rebuild,
restart, primed output loss, capacity restore, release and its fresh worktree,
the nine package test groups, Soroban and a final original build.
[Why this plan](../../../bazel-build-plan.md#workstation-exit-gates).

| | |
| --- | --- |
| Source | `aaf232ea82004c0c485b571c73ca1879abc23dae` |
| Engine | `/nix/store/5dhs21giw29lynlas4qpag9aykwqr3ds-sekka-buck2-2026-10-15-local-content-store/bin/buck2` |
| Client | `/nix/store/ldkvq2i0hii4ahynpi2zgrap20hkyl33-sekka-buck2` |
| Service | fresh `shared-build-service-5ce137e61808`, NativeLink 1.8.3, 64 GiB CAS, 1 GiB AC, 32 worker slots |
| Client mode | `--cache cache --cache-write` (local execution, writer cache), explicit 10 GiB / 100,000-entry content store, Cargo dev incremental state |
| Storage roots | each worktree's build state (`untracked:` roots), run state, content store, client bootstrap, repository, service directory, TMP, external buckd state; the active scope adds the service's image, containers and retained Nix closure |

[config.json](config.json) is the frozen configuration; [summary.json](summary.json)
is the harness's summary. There are no Cargo-baseline comparisons: edit times
are reported, not gated.

## Gates

| Gate | Result | Observed |
| --- | --- | --- |
| Correctness | pass | 31 observations, none failed; no failed build commands or native retries |
| Fresh worktrees reuse everything | pass | 0 compiler actions in each of four fresh worktrees (706 cached) |
| Physical sharing | pass | per added worktree −1,054,140,231, +167,828,194, +46,091,000, +23,742,390 bytes; limit 896,881,168 (5% of the 17,266,534,719-byte immutable payload + 32 MiB) |
| No-op | pass | 0.16 s |
| Restart, eviction recovery, primed output loss | pass | |
| Warm worktree ≤20% of cold | **fail** | 23.2%, 17.0%, 16.1%, 15.4% of the 89.5 s cold build; only the first fresh worktree misses |
| Edits (reported) | — | leaf 1.7 s, shared 16.5 s, icon 17.3 s |
| Churn budget, automatic total-state bound | not measured | not in this plan |

The first fresh worktree's union fell because the content store, already at
its 10 GiB bound, evicted stale edit objects while serving it. The other three
added 168, 46 and 24 MB. Each fresh worktree's own build state was 8 MiB of
single-referenced extents beside 3.47 GB reflinked from the content store.

## Storage by stage

Physical bytes are compsize unions (compressed). The CAS and content store
columns are uncompressed extents from `btrfs filesystem du`. All figures are
in [storage.json](storage.json), with every root's attribution.

| Stage | Active union | Immutable union | CAS | Content store |
| --- | --- | --- | --- | --- |
| dev-cold | 13.23 GB | 8.69 GB | 7.1 GiB | 7.2 GiB |
| dev-revert (after three edits) | 21.84 GB | 17.27 GB | 12.9 GiB | 10.1 GiB |
| dev-fresh-b | 20.78 GB | 16.35 GB | 12.9 GiB | 10.0 GiB |
| dev-fresh-c | 20.95 GB | 16.37 GB | 12.9 GiB | 10.0 GiB |
| dev-fresh-d | 21.00 GB | 16.38 GB | 12.9 GiB | 10.0 GiB |
| dev-fresh-e | 21.02 GB | 16.40 GB | 12.9 GiB | 10.0 GiB |
| dev-divergent-j4 | 32.83 GB | 28.12 GB | 23.7 GiB | 10.2 GiB |
| service-eviction (4 GiB CAS) | 19.50 GB | 14.78 GB | 4.0 GiB | 10.2 GiB |
| dev-primed-output-loss | 21.27 GB | 16.56 GB | 4.0 GiB | 10.2 GiB |
| release-fresh | 26.16 GB | 21.61 GB | 11.0 GiB | 10.2 GiB |
| final-original | 28.52 GB | 23.76 GB | 12.9 GiB | 10.2 GiB |

Identical worktrees cost almost nothing. Growth comes from new build
generations: three edits added 8.6 GB and five divergent edits 11.8 GB, mostly
in the CAS, which keeps every generation until its LRU bound. Each divergent
worktree kept about 550 MB of private state: its rebuilt outputs and
incremental state. Cargo incremental state is private per worktree (717 MiB in
tree a after the cold build). Content-store eviction can turn extents that a
worktree shared with the store into single-referenced ones without writing
anything; per-root exclusive bytes therefore explain growth, and only the
union measures it.

## CAS and content-store deduplication

After the run, `build-service --local dedupe` (controller from `f3f4dc9c`,
`/nix/store/ry5df77qg0h0nymwbiasvg36kiw03787-sekka-build-service`) shared
extents between the CAS and the content store. Of 12,746 objects present in
both, it deduplicated 9,678 (10,691,260,436 logical bytes) in 17.3 seconds and
skipped 3,068 smaller than 4 KiB; there were no mismatches or errors
([dedupe.json](dedupe.json)). A second pass found 9,675 already shared
([dedupe-repeat.json](dedupe-repeat.json)). Remeasuring `final-original`'s
exact roots:

| Union | Before | After | Change |
| --- | --- | --- | --- |
| Active | 28,522,398,373 | 24,292,815,246 | −4,229,583,127 (−14.8%) |
| Immutable | 23,763,878,916 | 19,533,408,427 | −4,230,470,489 (−17.8%) |

The raw receipts are the four `final-*.out.gz` files: a JSON receipt line,
then compsize's output. [SHA256SUMS.decompressed](SHA256SUMS.decompressed) lists
their decompressed digests. Later runs measure this in the plan's
`service-dedupe` stage.

## Remote execution

A fresh worktree at the same source, with an unbuilt edit to `aizome-theme`,
built the dev graph with `--cache remote --cache-write`: 706 commands, 705
executed on the service's workers, in 2 min 1 s. The aizome-sys and both Hako
test targets that failed on 2026-11-05 passed (seven remote build actions;
libtest runs on the local executor). Remote actions carry the worker-runtime
platform property, so they do not reuse local-execution action-cache entries;
identical output blobs are still one CAS object.

## Open findings

- **Lost cache entries after a service restart.** `release-first` ran seconds
  after `service-restore-capacity` restarted the backend. All 742 of its cache
  uploads reported success, yet `release-fresh` executed 85 actions locally
  (60 build-script runs, 25 compilations), 57 with action digests identical to
  release-first's. 53 of the 85 had finished in release-first's first 30
  seconds, while the client retried HTTP/2 errors from the restarted service.
  The rebuilds were correct; the cost is repeated work.
- **First fresh worktree latency.** 20.8 s against 13.8–15.2 s for the others,
  while the content store was evicting at its bound.

Three earlier starts on this day were discarded before producing evidence, and
their run roots and services destroyed: a configuration error of mine (the
harness owns `--write-build-id`), a change to measure worktree build state apart
from tracked source, and an observer that refused symlinked store paths in the
retained closure (fixed in `aaf232ea`). The run directory, quarantined output
evidence and the service's state remain on `wx-workstation` under
`/home/callen/sekka-work/buck2-storage-20261106` and
`/home/callen/sekka-work/shared-build-service-5ce137e61808`.

These are workstation build and storage observations. They do not qualify an
image, an installer or hardware.
