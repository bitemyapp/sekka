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

- `feat/buck2-shared-builds` (PR #291, draft) is pushed with the lost-writes
  and contention fixes, the paused-gateway restart (`251253d7`) and stale
  cleaning (`1356dfba`). Its CI trust check passes; the build job is
  skipped.
- `feat/buck2-single-cas` is rebased onto it and pushed; it has no PR yet.
  It makes NativeLink's CAS the only store on the CAS host:
  - hits are cloned from it (`local-cas-downloads.patch`);
  - produced outputs skip the content store, are journaled, and have their
    extents shared with their CAS blobs after each build
    (`local-cas-outputs.patch`, `build/buck2/cas_dedupe.py`);
  - the client opts in with a `build-service client --local-cas` file;
  - the post-build pass scans the CAS directory once, 0.6 s at 500,000
    blobs. It had scanned twice at 3.2–4.5 s each;
  - both copies of a locally built content-based output (placeholder and
    published content path) share the CAS blob's extents (`54ff881a`). One
    had kept its own: 1.1 GB of a cold dev worktree;
  - the engine's index of the CAS directory rescans outside its lock, and
    misses wait for a scan in progress (`4f41ec88`);
  - Buck2's daily stale cleaning is on in `.buckconfig` (7 days, or 12
    hours under 10% free), with the deferred write actions it requires. It
    landed on PR #291. An edit's superseded content-based outputs otherwise
    stayed in buck-out for good. Cleaned artifacts come back from a cache,
    or are rebuilt without one.
- `build-service autosize` sizes the CAS to free disk space above a floor,
  and `dedupe` shares the CAS with a content store. `autosize` waits for an
  idle service before resizing, unless the disk is under half its floor.
- A backend restart by `resize` or `runtime` no longer fails builds in
  flight: the gateway is paused until the backend listens, so clients wait.
  That takes up to 2.6 s at 500,000 CAS entries, beyond Buck2's 1.5 s of
  retries. In-flight requests get a retryable `UNAVAILABLE` instead of an
  HTML 502. PR #291 carries the same fix (`251253d7`).
- [The single-CAS evidence](test-results/buck2/2026-11-06-single-cas/README.md)
  holds the targeted repros and a full storage-plan run.
- On wx, at these commits: the engine's check phase, `checks.buck2-backend`
  (now with the dedupe/journal test) and `checks.build-service-inventory`
  (now with autosize and dispatch tests) pass. No broad flake check was run.
- PR CI: only the trust check runs; the Nix `build` job is skipped.

## What is proved

**[The final storage-plan run](test-results/buck2/2026-11-06-final/README.md)**
repeated the 31 stages at the single-CAS tip (`7fef66a8`), with a
`--local-cas` writer client. Every gate passed, eviction recovery included:
- each added worktree cost +14.0 to +14.8 MB;
- a warm worktree took 4.6–5.3% of the cold build's time;
- the final union was 20.16 GB, 29% below the first storage run's 28.52 GB.

The review fixes in `bdc406ca` came after it.

**[The first storage-plan run](test-results/buck2/2026-11-06-storage-plan/README.md)**
covered the same 31 stages on PR #291's engine. Local execution used a writer cache,
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

1. **Review and land.** Land PR #291 first. Then open a PR for
   `feat/buck2-single-cas` or fold it in; the user decides.
   - Its storage-plan run passed every gate except eviction recovery. That
     failure was a controller bug, fixed in `84954a1a`, and a targeted
     eviction repro passed.
2. **Make `autosize` routine** on the deployment host: DEVELOPERS.md has the
   timer recipe. It resizes only on a 5% shrink or a 25% growth, and only
   when idle. On wx, the user's service manager has run since 2026-11-03,
   before callen joined `docker`, so the recipe needs its `sg docker -c`
   form there; that form ran `autosize` cleanly. No timer is installed:
   the only long-lived services on wx are 2026-11-05's, which are not this
   work's to manage.
3. **NativeLink fork**, only if needed: stable digest paths (dropping the
   client's index) and casita-style retention (roots, and action-cache-first
   eviction). Cloned blobs are already kept recent: `has()` touches each key
   in 1.8.3. Its evicting map also leases keys, but only the worker's input
   staging takes those leases. Free-space sizing is already covered by
   `autosize`.
4. **Laptop phase.** On a host without the CAS, the content store and
   downloads work as before. runnymede is ext4, so it gets no reflinks.
5. **CI.** PR CI's `build` job is skipped because #291 is a draft:
   `scripts/check-pr-trust.py` admits no drafts. The workflow runs on
   `pull_request_target`, so it comes from `main`, and `main`'s copy has
   none of the shared-build jobs this branch adds to `pr.yml`. Marking #291
   ready would run `main`'s build job (Uta and components); the shared-build
   jobs first run on PRs after #291 merges. Both runner labels they need
   (`sekka-builder` on wx, `sekka-build-client`) are online.
6. **When PR 333 (per-application Aizome packages) lands:**
   - move `nix/bazel/without-build-files.nix` into its `package-sources.nix`;
   - rerun `scripts/sekka-buck2 generate`;
   - point `checks.bazel-native-inputs` at `components.compositor`.

   It invalidates none of the 706 Buck2 actions. Details are in
   `/home/callen/sekka-work/pr333-combo-20261106/results/` on wx.
7. **Scope decisions for the user:**
   - Does Bazel stay beside Buck2?
   - Does runnymede move to Btrfs? About 89 GiB of its 140 GiB of build state
     would be saved by compression alone.

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
4. Write the writer client with `client --domain trusted --access writer
   --format buck2 --local-cas` to a 0600 file.
5. Regenerate the config.
6. Start `shared-build-bench.py run CONFIG --go` with `setsid`.

Both of 2026-11-06's services were destroyed with their state, so a rerun
starts a new one (steps 3–4) with an empty CAS.

Older qualification services from 2026-11-05 are still running there, one
serving CI jobs; leave them unless the user asks. Never put credentials in a
config, argv, log or commit.
