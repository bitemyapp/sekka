# ICI: CI workflows, websites and repository tests

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `.github, site, aizome-site, tests, docs`. Audited at `8e0acbe5`; line numbers refer to that commit.

77 files audited. 27 implicit machines: 3 High, 5 Medium, 19 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="ici-02"></a>

### ICI-02 — Nightly input update and main/nightly branch promotion
- **Where:** `.github/workflows/nightly.yml:12` (`concurrency.group: sekka-nightly`); stages `:59`, `:92`, `:101`, `:117`, `:131`, `:139`
- **Encoding:** GitHub refs `main` and `nightly` (persisted; installed systems follow `nightly`); the job's local `HEAD`, which is either an update commit on the checked-out main tip or that tip itself when `flake.lock` did not change; `$RUNNER_TEMP/check.log` (failed-check list); ci-retain generations `main-uta`/`main-checks` (a failed generation roots the checks that passed); the binary cache contents
- **States:** `Dispatched` → `InputsUpdated(commit | unchanged)` → `UtaBuilt` → `Checked(attempt 1)` | `Retrying` → `Checked(attempt 2)` → `Cached` → `CoverageVerified` → `MainAdvanced` | `MainRefused` → `NightlyMoved`; a failure before `:148` leaves both refs alone. Combinations the workflow can produce: main advanced but nightly not (job ends between `:148` and `:150`); nightly on a commit not reachable from main (`MainRefused`); the cache holding a revision that neither ref names (failure after `:130`)
- **Transitions:** `nix flake update`, plus a commit when `flake.lock` differs (`nightly.yml:63-72`); first check attempt (`:110`), failed-list scrape and full retry (`:112-114`); `git diff --exit-code` (`:116`); `push-cache` (`:130`); coverage (`:138`); `git push HEAD:main || warning` (`:148-149`); `git push --force HEAD:nightly` (`:150`)
- **Hazards:** the `||` at `:148-149` treats every failed push to main (token, network, branch protection) as "main moved" and still force-moves nightly, which can leave nightly on a commit outside main's history until the next run force-pushes over it. The retry re-runs the whole flake check and depends on the first attempt's failed generation staying rooted. ci-retain keeps one failed main generation, and none below `MIN_FREE` (`scripts/ci-retain.py:86`), so on a full disk the retry rebuilds everything. An evaluation error before any `❌ checks.` line runs the full check twice for nothing. `timeout-minutes: 720` (`:31`) covers both attempts, the cache push and the coverage check on the single shared runner.
- **Reify:** High — installed systems follow the branch it moves, and where the refs end up depends on two push commands with different failure handling and on retention state kept by another script.
- **Related:** `scripts/ci-retain.py:137` (generations, `success` marker, prune at `:164` and `:187`); `apps/yukimi/crates/yukimi-system/src/updates.rs:65` (installed systems compare nightly's `lastModified` to classify Newer/Older); `scripts/check_cache_coverage.py`

<a id="ici-03"></a>

### ICI-03 — Trusted PR admission, recheck and commit-status lifecycle
- **Where:** `.github/workflows/pr.yml:19` (`concurrency.group: sekka-pr-<number>`, `cancel-in-progress: true`); jobs `trust` `:24`, `build` `:61`, `result` `:130`
- **Encoding:** `trust` job outputs `allowed`/`head`/`merge` (`:32-35`); the GitHub commit status `Trusted PR / build and tests` on the head SHA (`pending` at `:57`, final at `:149-151`); recheck outputs `allowed`/`merge` (`:90-92`); `$RUNNER_TEMP/ci-retain.py`, a copy of main's wrapper (`:100`); ci-retain `pr-*` generations
- **States:** `Event` → `Denied` (no status posted) | `Admitted(head, merge)` + status `pending` → `Rechecked` → `Building` → status `success` | `failure` | `error` (build cancelled or skipped); a superseding event → `Cancelled`
- **Transitions:** gate `check-pr-trust.py` (`pr.yml:49`); pending status (`:50-59`); recheck after waiting for the builder (`:81-87`); same-merge guard (`:94-95`); merge checkout (`:103`); `ci-retain.py pr checks` (`:127`); `result` maps `needs.build.result` to a state (`:144-147`) and posts it (`:149`)
- **Hazards:** the recheck requires the exact merge admitted earlier, and `select_merge` accepts only a merge whose parents are the *current* main tip and the head (`scripts/check-pr-trust.py:73`). Any push to main while the PR waits for the single builder therefore fails the build job. `result` runs under `always()` (`.github/workflows/pr.yml:132`) and posts `error` for a cancelled run; when the superseding run has the same head (a reopen, or a `workflow_dispatch` for the same PR), that late `error` can overwrite the new run's `pending`. A run the gate denies posts nothing, so the head keeps whatever status an earlier run left.
- **Reify:** High — the status lifecycle is spread across three jobs and two gate invocations, and its results are user-visible checks.
- **Suspected bug:** a PR queued behind the nightly (up to 12 h) or behind another merge: main moves, the recheck returns a new merge (or none), `test "$CURRENT_MERGE" = "$EXPECTED_MERGE"` fails, and the head is marked `failure` ("Build and tests: failure") even though nothing was built or tested. Nothing re-admits the PR (confidence: medium)
- **Related:** `scripts/check-pr-trust.py:45` (`select_merge` retry loop); `scripts/runner-admission.sh` (event admission, tested at `tests/test_pr_trust.py:144`); `nix/runner.nix:157`
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ici-03)

<a id="ici-11"></a>

### ICI-11 — Video playback state (poster/loading/playing/paused/ended/failed) with seek-drag resume
- **Where:** `site/video.js:183` (`setState` → `player.dataset.state`); fields `:23` (`audio`), `:24` (`gain`), `:25` (`source`), `:26` (`start`), `:27` (`shown`), `:28` (`frame`), `:29` (`origin`), `:30` (`resume`)
- **Encoding:** partially explicit. One function sets the `data-state` string, but "playing" really means `source != null`; the position lives in `frame` when stopped and in `start`/`origin` plus the audio clock when playing; a seek drag is `resume=true` with `source=null` while `data-state` stays `playing`
- **States:** `poster` → `loading` → `playing` ⇄ `paused` → `ended` → `playing`; any load error → `failed` (terminal). `Dragging` (`resume && !source`, with data-state `playing`) is a hidden sub-state. Invalid combinations that can be represented: `data-state=playing && source==null` outside a drag; `resume && data-state=paused`
- **Transitions:** `play()` guard (`:193`), `loading` (`:199`), `fail` (`:203`, `:304-311`); `begin` (`:206`, `:209-226`: new `source`, `onended=finish`, `origin`, `playing`); `pause` → `halt` (`:229-236`) + `paused` (`:241`); `finish` (`:244-252`) from `tick` (`:178`) or `onended` (`:218`); `seekTo` restarts or goes to `paused` (`:259-270`); drag start `halt` + `resume=true` (`:345-351`); drag input (`:352-358`); `release` → `begin` (`:340-344`); keyboard (`:376-421`)
- **Hazards:** `release()` calls `begin()` without `halt()`, assuming nothing restarted playback during the drag. Pausing during a drag does not clear `resume`, so playback restarts on pointer-up. The `{ once: true }` `pointerup`/`pointercancel` pair leaves one listener registered after the other fires; this is harmless only because `resume` is false by then.
- **Reify:** High — six handlers write the state, and the drag sub-state is encoded by an extra flag and a `data-state` that misreports it.
- **Suspected bug:** while dragging the seek thumb during playback, pressing K twice pauses and then calls `play()`, whose `begin()` creates source S1. On pointer-up, `release()` sees `resume` still true and calls `begin()` again, creating S2 without stopping S1. S1 is orphaned: it plays over S2, cannot be paused, and its `onended=finish` later jumps the player to `ended` (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ici-11)
