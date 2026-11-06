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
  `2711942e` with no merge commits. Local commits since the rebase:
  - `cc20783f`: the harness's storage plan.
  - `75d98d79`: build state measured apart from tracked source.
  - `aaf232ea`: the observer accepts symlinked store paths.
  - `f3f4dc9c`: CAS/content-store `dedupe`.
  - A final commit with these docs and the evidence.
- The remote branch still has the pre-rebase `d6a83edc` until a
  `--force-with-lease` push.
- The Buck graph is current after the rebase (`scripts/sekka-buck2 check`).
- `checks.build-service-inventory` and `.#build-service` build at `f3f4dc9c` on
  wx. No broad flake check was run.
- PR CI: only the trust check has ever run green on #291; `build` was skipped.

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

1. **Push** the rebased branch with `--force-with-lease` (lease on
   `d6a83edc9ee30407af8ab114cb7bdcf11c9d4878`). Then check PR CI and decide
   how its `build` job should cover this work.
2. **Lost cache entries after a restart.** Uploads made right after a backend
   restart reported success but 85 of 742 later missed (see the results'
   open findings). Reproduce it in isolation: restart, build cold, build a
   fresh worktree. Then check whether NativeLink's filesystem store loses
   blobs that are written while it rescans its content directory at startup.
3. **Make dedupe routine.** Today it is a manual command. Decide between a
   timer on the deployment host and a hook after large builds. Rerun the
   storage plan once to record its `service-dedupe` stage.
4. **Bounds.** Consider raising the content store's 10 GiB default to the dev
   plus release working set (~14 GiB physical, ~20 GiB logical). Consider
   whether the 64 GiB CAS default suits a storage-first workstation.
5. **Warm latency.** The first fresh worktree took 23% of cold (limit 20%),
   probably store eviction under its bound. It is low priority under the
   current guidance.
6. **Scope decisions for the user:** whether Bazel stays in this PR beside
   Buck2, and when to start the laptop phase. Measured on 2026-11-06, Btrfs
   on runnymede (ext4 today) would save ~89 GiB of its ~140 GiB of build state
   by compression alone; the scripts are in the session scratchpad.
   Shipping Nix packages remain separate from this development path.

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
