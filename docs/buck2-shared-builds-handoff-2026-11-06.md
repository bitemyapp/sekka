# Buck2 shared-build handoff — 2026-11-06

This is the resume point for [Sekka PR #291](https://github.com/bitemyapp/sekka/pull/291),
`feat/buck2-shared-builds`. The user wants the **full PR qualified** and has
explicitly required a **linear history with no merge commits**. Rebase when
updating from `main`; do not merge `main` into this branch. Keep the PR draft
until its evidence and CI are ready for review. Publication remains inhibited.

## State at handoff

- Checkout: `/home/callen/work/sekka-shared-builds`; clean branch, pushed to
  `origin/feat/buck2-shared-builds`. The pre-handoff HEAD is `0305fe44`.
- PR #291 is draft. Its latest observed trust check succeeded; `build` and
  `result` were skipped. That is not a passing CI build.
- After fetching `main` on November 6, `origin/main` is two commits ahead of
  this branch. `origin/main..HEAD` has zero merge commits. Rebase before
  freezing the source for qualification; use force-with-lease to update the PR.
- The last three pushed implementation/evidence commits are `3c5cd788`
  (physical inventory and Buck2 metadata hardening), `a4b171ad` (raw
  disposable Btrfs inventory receipt), and `0305fe44` (regenerated Buck graph
  after a root-lock change). This handoff adds documentation only.

## What works, and what is still unproved

`scripts/sekka-buck2` uses the root flake's pinned client, Rust toolchain,
native tools and Cargo-locked vendor closure. It roots the realized client
once; ordinary source edits do not reevaluate Nix. Cargo manifests and lock
remain authoritative, and `generate` turns resolved units into separate
compiler/build-script actions. Nix shares identical store paths across
worktrees. Buck2 keeps mutable daemon state per worktree and has a per-user,
bounded immutable content store. On Btrfs it can reflink store objects into
worktree outputs; an ext4 client does not establish that behavior. Shipping
Nix packages remain separate from this development build path. See
[`DEVELOPERS.md`](../DEVELOPERS.md#cargo-workspaces-through-buck2),
[`nix/buck2/`](../nix/buck2/) and [`scripts/sekka-buck2`](../scripts/sekka-buck2).

**The cross-worktree storage goal is unproved.** Focused Btrfs store tests
passed before the most recent root-lock update, and LAN probes established
fresh-root cache hits and mutation isolation. No completed five-worktree
Btrfs extent-union report shows the actual total bytes saved by Buck2 plus
Nix. Cache hits are not a physical-sharing measurement. The observer now
works, but its checked-in receipt measured a disposable service, not build
outputs from five worktrees.
No frozen full-matrix configuration or completed full-matrix report is
checked in. Local `shared-build-buck-crosshost-*` summaries under
`/home/callen/sekka-work` are incomplete/failed cross-host slices and do not
measure the total physical union.

| Observation | Host/filesystem | Scope and limit |
| --- | --- | --- |
| Cargo cold 123.8 s vs Bazel 114.8 s at 16 jobs; no-op 0.21 vs 0.26 s; physical 5.99 vs 7.55 GiB | `wx-workstation`, 128 logical CPUs, worktrees and outputs on `/dev/shm` tmpfs | Single-worktree Cargo/Bazel Phase 1; no Buck2 or Btrfs reflink result. [Receipt](test-results/bazel/2026-11-03-phase1/README.md) |
| Buck2 no-op theme builds 0.45–0.56 s, with zero Nix invocations | `wx-workstation` | Five traced small frontend runs, not full-workspace or throughput measurements. [Notes](../DEVELOPERS.md) |
| `aizome-kit` build: 211 local actions, zero cache hits | Local ext4 checkout, current rebased graph | A build correctness smoke, not a timing or Btrfs sharing result. |
| `physical-inventory --scope active`: 285 roots, 4 containers, 2 referenced images, 1,035,043,103 physical bytes, no unresolved roots | `wx-workstation` Btrfs | Disposable service/observer smoke only; no five-worktree payload. [Raw stdout and scope](test-results/build-service/2026-11-06-physical-smoke/README.md) |

`wx-workstation` currently reports 128 logical CPUs and Btrfs for
`/home/callen/sekka-work`. The old Cargo/Bazel trials deliberately used tmpfs
because persistent-disk headroom was inadequate. Do not convert those inode
figures into a claim about Btrfs extents or Buck2 deduplication.

## Completed since the interrupted security/storage work

The physical inventory now discovers owned service/container/image roots,
retained Nix closures and supplied workload roots, then measures one
compression-aware Btrfs extent union through `compsize`. It validates the
allowlist, filesystem identity and Docker mount view. `--scope immutable`
provides the separate matched payload denominator. A real disposable service
smoke passed both the active inventory and a small immutable probe; an
out-of-allowlist `/tmp` probe was refused. The service, images, manifest and
observer containers were cleaned up afterward. The checked-in raw stdout is
[`active.out.gz`](test-results/build-service/2026-11-06-physical-smoke/active.out.gz);
no credentials were committed.

The Buck2 local-store index uses FULL-synchronous WAL and rejects aliased
index sidecars (symlinks/hardlinks) before touching their targets. Output
preparation remains bounded, and the focused 525-file native smoke checked
writable single-link views and mutation isolation. These are component and
security/correctness checks, not full-workspace performance results. Relevant
code: [`nix/build-service/inventory.py`](../nix/build-service/inventory.py),
[`scripts/build-service`](../scripts/build-service),
[`nix/buck2/engine.nix`](../nix/buck2/engine.nix), and the patches in
[`nix/buck2/patches/`](../nix/buck2/patches/).

Before the latest rebase/lock update, the focused Nix builds for `.#buck2`,
`.#checks.x86_64-linux.buck2-backend`, `.#build-service` and
`.#checks.x86_64-linux.build-service-inventory` passed. On the current branch,
`scripts/sekka-buck2 check` and `scripts/sekka-buck2 -p aizome-kit build`
passed. The current-pin Btrfs engine test and the broad flake check have not
been completed. One earlier `nix flake check --no-build` attempt encountered
an invalid cached Nix store source path; investigate/retry rather than
recording a pass.

## Work remaining for this PR

1. **Refresh and freeze.** Rebase onto `origin/main` without merge commits;
   regenerate/check the Buck graph if the new base changes inputs. Rerun the
   current-pin Buck2, backend, build-service and inventory checks, including
   the Btrfs-gated engine tests on `wx-workstation`. Record the exact frozen
   commit, tool/closure identities, config and host headroom. Resolve the
   broad flake-check result.
2. **Prepare comparable evidence.** Freeze a matching Cargo baseline for
   source, features and profiles. Configure the service/clients and an owned
   Btrfs workload root with private credentials. Record all roots that matter:
   CAS, output trees, daemon state, worker scratch, generated repositories,
   retained Nix closures and packages. Do not add separate `du` figures or
   active/immutable unions together.
3. **Run the full workstation matrix.** The 85 stages in
   [`scripts/shared-build-bench.py`](../scripts/shared-build-bench.py) cover
   cold/no-op/leaf/shared/icon/revert, four fresh worktrees, divergent and
   identical concurrent edits, 4/16/32-job trials, ten churn observations,
   eviction/output-loss/restart, release and release-LTO, nine test groups,
   Soroban's 11 tests and reuse, and private headless startup. The harness
   retains failed/interrupted original attempts; do not replay a failed cold
   run as a new cold observation. Its `run` command requires a frozen JSON
   config and `--go`; `smoke` is a targeted preflight, not qualification.
4. **Prove physical sharing.** Measure one active Btrfs extent union after the
   one-tree edit/revert state and after each of four fresh worktrees. Measure
   the matched one-tree immutable union separately. The gate requires each
   added tree to cost at most 5% of that payload plus 32 MiB, with no hidden
   private multi-GiB intermediate tree. Count intentional Nix-package/CAS
   copies in total capacity. Verify zero eligible compiler actions in each
   fully primed fresh tree, warm builds at most 20% of cold, no-op at most 2 s,
   and edit medians at most `max(2 s, 1.5 × matched Cargo median)`. Publish
   raw receipts and the gate table. See
   [`docs/bazel-build-plan.md`](bazel-build-plan.md#workstation-exit-gates).
5. **Exercise failure and resource bounds.** Verify correctness through
   repeated edits, eviction, lost output state and service restart; observe
   scratch reclamation and retained-state growth. The harness explicitly
   marks *automatic total-state enforcement* unmeasured; a finite churn run
   alone cannot close that claim. Fix any failed gate before claiming the
   workstation result.
6. **Finish cross-machine and review evidence.** After workstation gates
   pass, prove laptop reuse, remote execution and local deduplication under
   the plan's separate gates. Check current PR CI, retain a source/identity
   manifest and raw results, and make PR #291 ready for review only when its
   required checks pass. Shipping Nix packaging/production migration is a
   separate boundary: do not describe this PR's development client as having
   replaced existing package derivations. [Packaging plan](bazel-build-plan.md#phase-4-packaging-and-ci-integration)

The earlier [`docs/bazel-build-plan.md`](bazel-build-plan.md) contains useful
acceptance criteria, but its opening implementation-status snapshot predates
the service, Buck2 and recent CI work. Use the current code and this handoff
for present state; use the plan's gates for acceptance.

## Safe resume commands and pointers

Run from the checkout after providing Python 3 for the benchmark harness:

```sh
git status --short --branch
git fetch origin main
git log --oneline HEAD..origin/main
git rev-list --min-parents=2 --count origin/main..HEAD
scripts/sekka-buck2 check
scripts/shared-build-bench.py plan
```

After inspecting the two newer base commits, rebase with `git rebase
origin/main`, resolve any conflicts, run the relevant checks, and push with
`git push --force-with-lease origin feat/buck2-shared-builds`. Do not put
credentials in a benchmark config, action argv, log, or commit; use private
0600 files as the controller/client commands require. Keep all build-service
measurement roots in one Btrfs filesystem UUID. A full run uses
`scripts/shared-build-bench.py run CONFIG.json --go` and later
`scripts/shared-build-bench.py summarize RUN_ROOT`; first satisfy the
configuration contract documented at the top of that script. An isolated
`smoke` result cannot be promoted to a full-matrix result.
