# Buck2 shared-build handoff — 2026-11-06

This is the resume point for [Sekka PR #291](https://github.com/bitemyapp/sekka/pull/291),
`feat/buck2-shared-builds`. The user wants the PR converged and qualified,
with a **linear history and no merge commits**: rebase onto `main`, never merge
it in. Keep the PR draft until its evidence and CI are ready. Publication stays
inhibited.

**Priorities, as the user set them on 2026-11-06:** storage sparing comes
first; build times only need to stay reasonable. Prefer few, targeted runs
over long benchmark matrices. Builds, tests and measurements go to
`ssh wx-workstation` (Btrfs, 128 CPUs, 250 GB RAM, ~2 TB free).

## State

- Checkout `/home/callen/work/sekka-shared-builds`, rebased onto `origin/main`
  `2711942e` with no merge commits, pushed to `origin/feat/buck2-shared-builds`.
- Since the storage run, three engine patches:
  - `presence-consistent-hits.patch`: cache uploads ask NativeLink about every
    output; no more action results naming evicted blobs.
  - `content-store-unlocked-views.patch`: views are cloned, verified and synced
    after the store locks are released.
  - `content-store-advisory-writes.patch`: LRU and hint writes are buffered
    instead of each costing a synchronous commit.
- `scripts/sekka-buck2` now fingerprints every engine patch by glob.
- On wx, the engine's check phase, `checks.buck2-backend`,
  `checks.build-service-inventory` and `.#build-service` pass at these
  commits. No broad flake check was run.
- PR CI: the trust check passes; the Nix `build` job is skipped, so CI does
  not exercise Buck2.

## What is proved

**[The storage-plan run](test-results/buck2/2026-11-06-storage-plan/README.md)**
covered 31 stages on the current engine. Local execution used a writer cache,
the reflinking content store and Cargo incremental state:

- **Gates:** every storage, reuse and correctness gate passed. Four fresh
  worktrees executed no compiler actions. Each added −1.05 GB to +168 MB to the
  physical union against an 897 MB limit, and kept 8 MiB of its own build
  state.
- **Recovery:** eviction, restart, primed output loss, the nine test groups and
  Soroban all passed, including the tests that failed on 2026-11-05.
- **Remote execution:** a separate check ran 705 of 706 actions on the
  service's workers.
- **Dedupe:** the new command shared 10.7 GB of CAS blobs with the content
  store and cut the final union by 15% (−4.23 GB).

**The full 85-stage matrix of 2026-11-05** used an older engine and remote
execution. Its data is under
`/home/callen/sekka-work/shared-build-qualification-coOjat/runs/buck2-final-v2`
on wx. Its storage figures counted each fresh worktree's source. It missed
edit-latency gates that are now reported rather than gated. Its five
correctness failures do not recur on the current engine.

Where storage goes once worktrees share: every new build generation (edits,
release, tests) lands in the CAS until its LRU bound. The content store holds
the working set at its 10 GiB bound; dev plus release outputs already exceed
it. Incremental state is private per worktree (~0.7 GB after a dev build).

## Open work, in order

1. **Option 2, on its own branch `feat/buck2-single-cas`:** one physical copy
   of each output on a host.
   - Phase 1 (Buck2 only): the content store reads NativeLink's CAS
     directory, through a digest index because names carry a generation, and
     reflinks from it. Uploaded outputs are deduplicated against the CAS inline.
   - Phase 2: a NativeLink built from source with stable digest paths, local
     reads counted in its LRU, a free-space floor, and casita-style retention.
     That retention means roots that keep their whole graph, leases for
     in-flight uploads and reads, and eviction of action-cache entries before
     the blobs they name.
   - The casita investigation found casita itself unsuitable today: it stores
     chunks with no reflinks, speaks no REAPI or substituter protocol, and is
     pre-release.
2. **Writers under concurrency.** Four worktrees materializing at once take
   68–93 s against 37 s with the store off. A single worktree is at its
   baseline. The cost is ingesting new outputs, mostly incremental state,
   under the exclusive lock. Option 2 replaces that ingest path; revisit after it.
3. **Make dedupe routine** until option 2 makes it unnecessary: a timer on the
   deployment host or a hook after large builds.
4. **Bounds.** The content store's 10 GiB default is below the dev plus
   release working set. The 64 GiB CAS default has no free-space awareness.
5. **CI.** Decide how PR CI's `build` job should exercise this work.
6. **When PR 333 (per-application Aizome packages) lands**, or this branch
   lands after it:
   - move `nix/bazel/without-build-files.nix` into its `package-sources.nix`;
     otherwise BUCK/BUILD edits rebuild Nix packages;
   - rerun `scripts/sekka-buck2 generate` for the lock change;
   - point `checks.bazel-native-inputs` at `components.compositor`.

   Measured combination on wx:
   - Buck2: PR 333 invalidates 0 of 706 actions.
   - A Soroban edit rebuilds only Soroban in Nix: 24 s and +17 MiB, against
     7:23 and +573 MiB.
   - Full check evaluation: 72.8 s against 68.8 s.

   Details are in `/home/callen/sekka-work/pr333-combo-20261106/results/` on wx.
7. **Scope decisions for the user:** whether Bazel stays beside Buck2, and
   when to start the laptop phase. Btrfs on runnymede (ext4 today) would save
   ~89 GiB of its ~140 GiB of build state by compression alone (2026-11-06
   estimate). Shipping Nix packages remain separate from this development path.

## Resume

From the checkout (Python is not on PATH; use the pinned interpreter or `nix
develop`):

```sh
git status --short --branch
git rev-list --min-parents=2 --count origin/main..HEAD
scripts/sekka-buck2 check
scripts/shared-build-bench.py plan --kind storage
```

On wx, `/home/callen/sekka-work/buck2-storage-20261106` holds the run:

- `make_config.py` writes the frozen config;
- `reset_run.sh` stops the run and destroys its service;
- `post_dedupe.sh` and `remote_check.sh` hold the post-run checks;
- `gc/` holds the rooted tools.

For a new run:

1. Push the commit to that directory's `repo`.
2. Build `gc/build-service-*` from it.
3. Run `build-service --local … init --domains trusted --port 17891 --jobs 32`,
   then `up`.
4. Write the writer client with `client --access writer --format buck2` to a
   0600 file.
5. Regenerate the config.
6. Start `shared-build-bench.py run CONFIG --go` with `setsid`.

Older qualification services from 2026-11-05 are still running there, one
serving CI jobs; leave them unless the user asks. Never put credentials in a
config, argv, log or commit.
