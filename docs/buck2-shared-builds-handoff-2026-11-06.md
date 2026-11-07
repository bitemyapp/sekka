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
  and contention fixes (`5baedd7c`).
- `feat/buck2-single-cas` branches from it and is pushed; it has no PR yet.
  It makes NativeLink's CAS the only store on the CAS host:
  - hits are cloned from it (`local-cas-downloads.patch`);
  - produced outputs skip the content store, are journaled, and have their
    extents shared with their CAS blobs after each build
    (`local-cas-outputs.patch`, `build/buck2/cas_dedupe.py`);
  - the client opts in with a `build-service client --local-cas` file.
- `build-service autosize` sizes the CAS to free disk space above a floor,
  and `dedupe` shares the CAS with a content store.
- [The single-CAS evidence](test-results/buck2/2026-11-06-single-cas/README.md)
  holds the targeted repros and a full storage-plan run.
- On wx, at these commits: the engine's check phase, `checks.buck2-backend`
  (now with the dedupe/journal test) and `checks.build-service-inventory`
  (now with autosize and dispatch tests) pass. No broad flake check was run.
- PR CI: only the trust check runs; the Nix `build` job is skipped.

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

1. **Review and land.** Land PR #291 first. Then open a PR for
   `feat/buck2-single-cas` or fold it in; the user decides.
   - Its storage-plan run passed every gate except eviction recovery. That
     failure was a controller bug, fixed in `84954a1a`, and a targeted
     eviction repro passed.
   - Rerun the plan once if a full eviction-stage record is wanted.
2. **Make `autosize` routine** on the deployment host, for example from a
   user timer every 15 minutes. It resizes only on a 5% shrink or a 25%
   growth, because a resize restarts the backend.
3. **NativeLink fork**, only if needed: stable digest paths (dropping the
   client's index), local reads counted in its LRU, and casita-style
   retention. That retention means roots, leases, and action-cache-first
   eviction. Free-space sizing is already covered by `autosize`.
4. **Laptop phase.** On a host without the CAS, the content store and
   downloads work as before. runnymede is ext4, so it gets no reflinks.
5. **CI.** Decide how PR CI's `build` job should exercise this work.
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
4. Write the writer client with `client --access writer --format buck2` to a
   0600 file.
5. Regenerate the config.
6. Start `shared-build-bench.py run CONFIG --go` with `setsid`.

Older qualification services from 2026-11-05 are still running there, one
serving CI jobs; leave them unless the user asks. Never put credentials in a
config, argv, log or commit.
