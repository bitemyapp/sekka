# ICI: CI workflows, websites and repository tests

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `.github, site, aizome-site, tests, docs`. Audited at `8e0acbe5`; line numbers refer to that commit.

77 files, 0 pending. 27 implicit machines: 3 High, 5 Medium, 19 Low.

## Machines

<a id="ici-01"></a>

### ICI-01 — Main-branch cache publication run (coalesced queue → checks → binary cache)
- **Where:** `.github/workflows/cache.yml:16` (`concurrency.group: sekka-cache-${{ github.ref }}`, `cancel-in-progress: false`); stages at `:38`, `:72`, `:81`, `:89`, `:103`
- **Encoding:** GitHub's per-group run queue (one running run, at most one pending); the step order of the single `cache` job; ci-retain generations `main-uta`/`main-checks` (with a `success` marker) on the builder's GC-root mount; the contents of cache.sekka.computer
- **States:** `Pending` (newest only) → `SecretsChecked` → `UtaBuilt` → `Checked` → `Cached` → `CoverageVerified`; any step → `Failed`; older pending runs → `Cancelled` (never cached)
- **Transitions:** push or dispatch enqueues and cancels the older pending run (`cache.yml:16-18`); secrets gate (`:38-53`); `ci-retain.py main uta` (`:80`); `ci-retain.py main checks flake check --keep-going` + `git diff --exit-code` (`:87-88`); `nix run .#push-cache` (`:102`); `check_cache_coverage.py` (`:107`)
- **Hazards:** in a burst of merges the intermediate main revisions are never cached, so `--early` systems (`?ref=main`) that update in that window compile; coverage is checked only after the push, so a revision that fails coverage is still in the cache (no branch names it); the nightly's own push to main uses the workflow token and starts no cache run (comment `:27-28`), so caching that revision depends on the nightly doing it; every builder job shares one runner container (`nix/runner.nix:157`), so cache runs queue behind nightly, PR and release jobs.
- **Reify:** Low — a linear step list whose ordering is the only invariant, and the YAML states it.
- **Related:** `scripts/ci-retain.py:137` (generation and retention machine); `nix/runner.nix:157` (supervisor that runs one job at a time); ICI-02

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

<a id="ici-04"></a>

### ICI-04 — Manual release: tag → draft prerelease → published
- **Where:** `.github/workflows/release.yml:14` (`concurrency.group: sekka-release`); stages `:37`, `:64`, `:71`, `:76`
- **Encoding:** the `v*` Git tag (input `RELEASE_TAG`, `:30`); a detached checkout of the tag's commit (`:44-46`); the `result-iso` out-link; the `dist/` assets; the GitHub release object's `draft`/`prerelease` flags
- **States:** `Dispatched` → `SourceVerified` → `Checked` → `ISOBuilt` → `AssetsPrepared` → `Draft(uploading)` → `Published(prerelease)`; any failure stops the run; `Draft` survives a failed upload or edit
- **Transitions:** the tag resolves and is an ancestor of `origin/main` (`release.yml:43-46`); `nix flake check` + `nix build .#iso` + lock unchanged (`:68-70`); `prepare-release.py` (`:75`); `gh release create --draft --prerelease` (`:82-84`); `gh release edit --draft=false` (`:85`)
- **Hazards:** a failure between `:82` and `:85` leaves a draft holding the tag, which must be cleared by hand before a re-dispatch. The tag is resolved at `:44` and resolved again by `gh release create --verify-tag` at `:82`, so a tag moved mid-run publishes assets built from the old commit under the new one. There is no qualification gate between the ISO build and publication (AGENTS.md requires VM qualification for published ISOs). The file itself encodes no inhibition beyond its dispatch-only trigger.
- **Reify:** Medium — the flow is linear, but the draft/published split is the only rollback point and nothing cleans it up.
- **Related:** `scripts/prepare-release.py` (asset split, covered by `tests/test_release.py`)

<a id="ici-05"></a>

### ICI-05 — Per-site publish decision keyed on the revision the live site serves
- **Where:** `.github/workflows/sites.yml:89` (`plan` step); `:33` (`concurrency.group: sekka-sites`); matrix `sources` `:57`, `:70`
- **Encoding:** the first `github.com/bitemyapp/sekka/commit/<sha>` link on the live site (`:103-105`), which `revision_html` writes into every page (`site/src/main.rs:150`, `aizome-site/src/main.rs:252`); `steps.plan.outputs.publish`, `steps.secrets.outputs.deploy`; the hand-kept `sources` lists and `on.push.paths` (`.github/workflows/sites.yml:9-24`)
- **States:** per site, `Unknown` → publish; `LiveIncludesHead` → skip; `LiveCurrentForSources` → skip; `LiveStale` → publish → `Built` → `NotDeployed` (no secrets) | `Deployed`
- **Transitions:** a dispatch forces publication (`:96-99`); an unknown or unparsable live revision → publish (`:106-108`); `merge-base --is-ancestor HEAD live` → skip (`:109-111`); `git diff --quiet live HEAD -- sources` → skip (`:112-114`); otherwise publish (`:115-118`); revision stamp check (`:137`); secrets gate (`:150-155`); `wrangler pages deploy` (`:166`)
- **Hazards:** three hand-kept copies of "what a site is built from" (`paths`, `sources`, the Nix filesets) must agree. `flake.lock` is in none of them, and the fonts come from nixpkgs (`nix/site.nix:140`), so a font or toolchain update republishes neither site until a listed path changes. A run without secrets, or a failed deploy, leaves the live revision old and recovers only on the next qualifying push. A curl failure falls through to publishing.
- **Reify:** Medium — the persisted state lives in the deployed HTML; the logic is compact, but its inputs are duplicated in three places.
- **Related:** `nix/site.nix`, `nix/aizome-site.nix` (filesets and `--revision`)

<a id="ici-06"></a>

### ICI-06 — Night/Day theme switch remembered in localStorage
- **Where:** `aizome-site/theme.js:7` (`key = "aizome-theme"`); `site/theme.js:7` (`"sekka-theme"`), same logic
- **Encoding:** `localStorage[key]` (`"day"`, or absent/`"night"`); `<html data-theme="day">`; the `checked` state of the `#site-theme` checkbox (`site/src/html.rs:93`, `aizome-site/src/html.rs:133`); the CSS treats either as Day (`site/src/tokens.rs:276`, `aizome-site/src/theme.rs:367`)
- **States:** `Night` | `Day`, persisted and mirrored in the attribute and the checkbox; browsers that refuse storage keep only the page-local copy
- **Transitions:** `show(remembered())` while the head loads (`aizome-site/theme.js:30`); resync at `DOMContentLoaded` (`aizome-site/theme.js:37`); `change` → `setItem` (`aizome-site/theme.js:41`) + `show` (`aizome-site/theme.js:45`); cross-tab `storage` event (`aizome-site/theme.js:51-55`); resync on `pageshow` (`aizome-site/theme.js:56-58`)
- **Hazards:** the page state has two writers (the user's change and the resync from storage), and storage is the authority; when `setItem` throws, every resync reverts to the stored value.
- **Reify:** Low — two states and one key.
- **Suspected bug:** where `setItem` throws (quota or private mode) or storage is blocked, switching to Day before the `load` event is undone by the initial `pageshow`, which fires after `load` and calls `show(remembered())` → Night (confidence: low)

<a id="ici-07"></a>

### ICI-07 — Plymouth local-console ownership (carried patch)
- **Where:** `docs/upstream/plymouth/console-owner.patch:33` (adds the release in `free_devices_from_device_path`); accessor `:50-54`
- **Encoding:** upstream `ply_device_manager_t.local_console_managed` (bool) + `.local_console_terminal`; the owning renderer is implicit: it is the one whose `renderer->terminal` equals the console terminal
- **States:** `Free` → `Owned(by renderer R)` → `Free`
- **Transitions:** a renderer created while the console is `Free` takes the terminal (upstream, unchanged); the patch releases it when the owning renderer is freed: `local_console_managed = false` (`console-owner.patch:33-37`); upstream also clears it once no keyboards are left (commit message `:6-7`)
- **Hazards:** the owner is found by comparing pointers rather than stored. Releasing the console does not hand it to an existing terminal-less renderer, so input recovers only if another renderer is created afterwards (the simpledrm→DRM order the patch targets). Two independent paths clear the flag (owner freed, last keyboard gone).
- **Reify:** Medium — upstream C, but this bool/owner split is the class of bug the patch fixes.
- **Related:** ICI-08; applied by `installer/genkan/rust/system/boot-splash/default.nix`

<a id="ici-08"></a>

### ICI-08 — Plymouth splash keyboard binding across renderer replacement (carried patch)
- **Where:** `docs/upstream/plymouth/keyboard-removal.patch:51` (`keyboard_removed` in `plymouthd-input.c`); `:84` (`ply_boot_splash_unset_keyboard`); `:97` (device manager); `:146-179` (script plugin `set_keyboard`/`unset_keyboard`); `:201` (`script_lib_plymouth_set_keyboard`)
- **Encoding:** four copies of "the current keyboard": `splash->keyboard`, the script plugin's `plugin->keyboard`, `script_lib_plymouth_data_t.keyboard`, and whether `on_keyboard_input` is attached (gated on `plugin->is_animating`). Also the `manager->keyboards` list, and `manager->keyboard_removed_handler`, which is nulled during manager teardown
- **States:** `NoKeyboard` → `Bound(K)` → (K's renderer freed) `Unbound` → `Bound(K')` | `NoKeyboard`; `ManagerTearingDown` (removals not reported)
- **Transitions:** removal reported before the free (`:97-100`); the daemon detaches, unsets the splash keyboard (`:68`), then rebinds each remaining keyboard in turn, so the last one wins (`:70-72`); unset clears `splash->keyboard` (`:84`); the script plugin moves the input handler and the library pointer (`:150-163`, `:170-178`); the caps-lock query tolerates a missing keyboard (`:191`); manager teardown silences the handler (`:108-110`)
- **Hazards:** the four copies stay consistent only because every path goes through set/unset. Handler attachment is keyed on `is_animating`, not on whether the handler is attached, so a set or unset while the splash is shown but not animating moves the pointer without the handler (not checked against upstream's show/hide paths). `attach_remaining_keyboard_to_splash` walks a list that still holds the removed keyboard, and rebinds even when the removed keyboard was not the splash's.
- **Reify:** Medium — upstream code; the patch funnels transitions through set/unset, but the four copies remain.
- **Related:** plymouthd `src/daemon/plymouthd-input.c` and the script splash plugin (upstream); ICI-07; applied by `installer/genkan/rust/system/boot-splash/default.nix`

<a id="ici-09"></a>

### ICI-09 — Download page's nightly build list: edge-cached index with fallback
- **Where:** `site/download-worker/src/lib.rs:63` (`list`); TTL `site/src/shared.rs:12` (`BUILDS_TTL = 300`); classification `site/download-worker/src/render.rs:33`
- **Encoding:** a Cloudflare Cache API entry keyed by `<base>/builds.json` (per data centre, `max-age=300`); the static page's `<!--builds-->…<!--/builds-->` fallback (`render.rs:26-27`); the `x-builds-cache: HIT|MISS` header
- **States:** per request, `Hit` | `Miss→Cached(200|404)` | `Miss→Uncached` (other status or fetch error); rendered as `List` | `NoneYet` (404 or empty list) | `Fallback` (None)
- **Transitions:** cache hit (`lib.rs:67-74`); fetch (`:76-85`); only 200/404 are stored (`:88-97`); classify (`render.rs:34-41`); splice into the page (`lib.rs:41-45`)
- **Hazards:** a 200 body that fails to deserialize (for example after a publisher schema change drops a field) is cached for 300 s and shows the "unavailable" fallback. The publisher deletes expired builds right after swapping the index (`scripts/publish-nightly.py:360-364`), so a data centre still holding the previous index links to deleted ISOs for up to `BUILDS_TTL`. A cached 404 shows "No qualified ISO" for up to 300 s after the first publication.
- **Reify:** Low — the worker is stateless; its only state is the TTL cache.
- **Related:** `scripts/publish-nightly.py:300` (writes `nightly/builds.json`); `site/src/pages.rs:1138` (fallback markup); ICI-24

<a id="ici-10"></a>

### ICI-10 — Home-page video asset loading (lazy, single-flight, retried after failure)
- **Where:** `site/video.js:22` (`loading`), `:21` (`video`)
- **Encoding:** `loading` (Promise | null), `video` (object | null), `seek.disabled`/`seek.max`; the once-only `pointerenter` listener and the idle callback
- **States:** `NotLoaded` (`loading=null, video=null`) → `Loading` (`loading` pending) → `Loaded` (`video` set) | back to `NotLoaded` on rejection
- **Transitions:** `load()` runs `loading ??= open()` (`:42`); success sets `video` and enables seeking (`:44-47`); failure resets `loading` (`:51`); idle/`load` trigger (`:429-434`); `pointerenter` (`:435`); load started by Play (`:198-205`)
- **Hazards:** a retry after a partial failure re-runs all of `open()`. It fetches the Wasm again (the `fetch` at `:62` runs before the glue's already-initialised check), makes a new bridge, and adds every FontFace to `document.fonts` again. A failure during a load started by Play is terminal (`fail`); a background failure is silent and can be retried.
- **Reify:** Low — small, and the promise guards it.
- **Related:** `site/video/hype.js:1006` (ICI-14)

<a id="ici-11"></a>

### ICI-11 — Video playback state (poster/loading/playing/paused/ended/failed) with seek-drag resume
- **Where:** `site/video.js:183` (`setState` → `player.dataset.state`); fields `:23` (`audio`), `:24` (`gain`), `:25` (`source`), `:26` (`start`), `:27` (`shown`), `:28` (`frame`), `:29` (`origin`), `:30` (`resume`)
- **Encoding:** partially explicit. One function sets the `data-state` string, but "playing" really means `source != null`; the position lives in `frame` when stopped and in `start`/`origin` plus the audio clock when playing; a seek drag is `resume=true` with `source=null` while `data-state` stays `playing`
- **States:** `poster` → `loading` → `playing` ⇄ `paused` → `ended` → `playing`; any load error → `failed` (terminal). `Dragging` (`resume && !source`, with data-state `playing`) is a hidden sub-state. Invalid combinations that can be represented: `data-state=playing && source==null` outside a drag; `resume && data-state=paused`
- **Transitions:** `play()` guard (`:193`), `loading` (`:199`), `fail` (`:203`, `:304-311`); `begin` (`:206`, `:209-226`: new `source`, `onended=finish`, `origin`, `playing`); `pause` → `halt` (`:229-236`) + `paused` (`:241`); `finish` (`:244-252`) from `tick` (`:178`) or `onended` (`:218`); `seekTo` restarts or goes to `paused` (`:259-270`); drag start `halt` + `resume=true` (`:345-351`); drag input (`:352-358`); `release` → `begin` (`:340-344`); keyboard (`:376-421`)
- **Hazards:** `release()` calls `begin()` without `halt()`, assuming nothing restarted playback during the drag. Pausing during a drag does not clear `resume`, so playback restarts on pointer-up. The `{ once: true }` `pointerup`/`pointercancel` pair leaves one listener registered after the other fires; this is harmless only because `resume` is false by then.
- **Reify:** High — six handlers write the state, and the drag sub-state is encoded by an extra flag and a `data-state` that misreports it.
- **Suspected bug:** while dragging the seek thumb during playback, pressing K twice pauses and then calls `play()`, whose `begin()` creates source S1. On pointer-up, `release()` sees `resume` still true and calls `begin()` again, creating S2 without stopping S1. S1 is orphaned: it plays over S2, cannot be paused, and its `onended=finish` later jumps the player to `ended` (confidence: medium)

<a id="ici-12"></a>

### ICI-12 — Player controls auto-hide and touch-tap gating
- **Where:** `site/video.js:315` (`sleep` timer), `:322` (`pointer`)
- **Encoding:** the `player-awake` class; the `sleep` timeout id (2.5 s); the last `pointerType`
- **States:** `Asleep` ⇄ `Awake`; a touch tap while `Asleep && playing` only wakes the controls
- **Transitions:** `wake()` adds the class and re-arms the timer (`:316-320`) on `pointermove` (`:326`), a screen click (`:332`) and keydown (`:380`); the timer removes the class (`:319`); `pointerdown` records the pointer type (`:323-325`); tap gating (`:333`)
- **Hazards:** none observed
- **Reify:** Low — one local timer.

<a id="ici-13"></a>

### ICI-13 — Mute and volume level
- **Where:** `site/video.js:31` (`level`), `:32` (`muted`)
- **Encoding:** `level` (0–1) and `muted` (bool); `setLevel` derives `gain.gain.value`, the slider value, the `player-muted` class and the Mute/Unmute label
- **States:** `Audible(level>0, !muted)` | `Muted(muted)` | `Silent(level==0, !muted)`, which displays as muted
- **Transitions:** `toggleMute` (`:281-289`) unmutes and restores `level=1` from `Silent`, or mutes; slider input sets `level` and clears `muted` (`:361-365`); `setLevel` applies the result (`:274-279`), first when `gain` is created (`:210-214`)
- **Hazards:** none observed
- **Reify:** Low — two fields and one projection function.

<a id="ici-14"></a>

### ICI-14 — wasm-bindgen glue: one-shot instance and closure refcounts (generated)
- **Where:** `site/video/hype.js:935` (`wasmModule`, `wasmInstance`, `wasm`); `:821` (`makeMutClosure` state `{a, b, cnt}`); `:3` (`durations`)
- **Encoding:** module-level `wasm` (undefined until init); per-closure `state.cnt`, and `state.a` (0 while the closure runs or after destruction); `FinalizationRegistry` backstops (`:684-708`); the `durations` Map, which `load_audio_wasm_callback` reads (`:4`)
- **States:** glue `Uninitialised` → `Initialised` (no in-flight state); closure `Live(cnt≥1)` → `Running(a=0)` → `Destroyed(cnt=0)`
- **Transitions:** `__wbg_init` returns early once initialised (`:1007`); `__wbg_finalize_init` sets `wasm`, clears the memory-view caches and calls start (`:936-944`); a closure call increments and decrements the count (`:828-835`) and destroys at zero (`:838-843`); `site/video.js:104` must set `durations` before `bridge.prepare` (`site/video.js:105`)
- **Hazards:** concurrent `__wbg_init` calls would both instantiate, because there is no pending state; only `video.js`'s `loading ??=` prevents this. The code is generated: regenerate it rather than edit it.
- **Reify:** Low — generated by wasm-bindgen.
- **Related:** `site/video.js:41` (ICI-10)

<a id="ici-15"></a>

### ICI-15 — (test harness) IBus session-switch driver: Aizome → Xfce → GNOME → Aizome
- **Where:** `tests/aizome-ibus.py:27` (`until`), `:178` (`logged_out`), `:199` (`login`), `:254` (`manager_pid`)
- **Encoding:** the VM's session phase (AccountsService session and type, display-manager restarts); `aizome-ibus.service` MainPID/NRestarts; the ibus-daemon count; the PID of the persistent user manager; `/tmp/ibus-exit-one`, an injected copy of the wrapper
- **States:** `AizomeSession` → `FrontendKilled` → `Restarted` → (Sumi, Chrome, GTK composition) → `LoggedOut` → `XfceSession` → `LoggedOut` → `GnomeSession` → `LoggedOut` → `AizomeSession` → `LoggedOut`
- **Transitions:** frontend exit injection and the kill/restart checks (`:62-113`); `logged_out` waits for the shell to exit, the unit to go inactive, the daemon to go, and the same manager to remain (`:178-196`); `login` stops the DM, sets the AccountsService session and starts the DM (`:199-215`); subtests (`:255-368`)
- **Hazards:** fixed sleeps gate focus in GNOME and Xfce (`:48`, `:231-239`, `:324-328`); the injected wrapper (`:71-73`) assumes the second line of the ExecStart script starts the frontend.
- **Reify:** Low — a linear test script.
- **Related:** `installer/genkan/rust/system/input-method.nix:29` (the wrapper's session guard), `:88` (the `aizome-ibus` unit, `Restart` at `:102`)

<a id="ici-16"></a>

### ICI-16 — (test harness) gdb-scripted Avahi startup signal window
- **Where:** `tests/check_avahi_startup.py:16` (`run_case`); gdb script `:39-79` (breakpoints at `:49`, `:53`, `:64`, `:68`)
- **Encoding:** gdb breakpoints and the markers they print (`PID_PUBLISHED`, `HANDLER_SETUP_REACHED`, `MAIN_LOOP_REACHED`, `QUEUED_SIGNAL`, `RELOAD_RETURNED`); the PID file `/run/avahi-daemon/pid` as the persisted marker; `quit 77` when the inferior has died
- **States:** `Started` → `PidPublished` → (early HUP) `Killed(pid file left)` | `HandlersInstalled` → `MainLoop` → `ReloadQueued` → `Reloaded` → `Terminated(pid file removed)`
- **Transitions:** break at `daemon_pid_file_create`, then finish (`:49-52`); early `signal SIGHUP` or `continue` (`:38`); exit 77 for a dead inferior (`:57-60`); main-loop break (`:64-66`); HUP and `daemon_signal_next` (`:68-71`); SIGTERM (`:77`); expectations (`:120-139`)
- **Hazards:** the only bound is a 20 s subprocess timeout (`:107`); the script relies on symbol names exported by the packaged daemon.
- **Reify:** Low — a fixture.
- **Related:** `nix/patches/avahi-startup-sighup.patch` (the daemon publishes its PID file before installing signal handlers)

<a id="ici-17"></a>

### ICI-17 — (test harness) Spawn → wait-ready → inject → tear-down daemon drivers
- **Where:** `tests/check_pipewire_x11_disconnect.py:43` (`case`); also `tests/check_xapp_settings.py:43`, `tests/test_launcher_search.py:70` (`Elephant`)
- **Encoding:** `subprocess.Popen` handles; log files polled for markers; D-Bus name ownership; socket existence; deadlines from `time.monotonic()`
- **States:** `Spawned` → `Ready` (displayfd, log line, bus name, or a socket that answers) → `Injected` (X server stopped or killed, xsettingsd HUP, queries) → `Verified` → `Stopped` (terminate, then kill after 5–10 s)
- **Transitions:** PipeWire: Xvfb `-displayfd` handshake (`check_pipewire_x11_disconnect.py:58-67`); readiness from the log (`:77-78`; `wait_for_log` `:18-26` fails early if the daemon exits); X server kill and the survival window (`:80-88`); teardown in `finally` (`:94-99`, `stop` `:29-36`). XApp: wait for the bus name (`check_xapp_settings.py:60-69`), monitor ready (`:77`), crash or SettingChanged count (`:85-99`), teardown in reverse order (`:104-112`). Elephant: readiness loop on the socket and a query (`test_launcher_search.py:111-116`, `:120`), then `close` (`:130-134`)
- **Hazards:** `Elephant.close` waits 10 s after `terminate` with no kill fallback (`test_launcher_search.py:131-132`); XApp's monitor readiness depends on the text "owned by" in gdbus output (`check_xapp_settings.py:77`).
- **Reify:** Low — fixtures.
- **Related:** `nix/patches/pipewire-x11-bell-disconnect.patch`, `nix/patches/xapp-settings-theme-idle.patch`, `desktop/tatami/elephant/package.nix`

<a id="ici-18"></a>

### ICI-18 — (test harness) Chrome IBus probe: CDP request ids and a held-modifier tab cycle
- **Where:** `tests/ibus-chrome.py:48` (`CDP`, `sequence` `:51`), `:122` (`tab_cycle`)
- **Encoding:** the CDP `sequence` counter; replies matched by id, with other messages dropped (`:56-60`); Ctrl held in the compositor between `key("ctrl", "down")` and `key("ctrl", "up")`; the dconf `use-system-keyboard-layout` key and the IBus engine, session-wide state the probe changes
- **States:** `Launched` → `PortKnown` (DevToolsActivePort) → `Attached` → per trial `Cleared` → `CtrlHeld` → `TabOpened` → `TabClosed` → `CtrlReleased` → `Typed`; finally `Restored` (dconf reset, engine back to xkb)
- **Transitions:** launch (`:141-162`); port (`:163-165`); CDP connect (`:173-176`); `clear` (`:108-116`); `tab_cycle` (`:123-131`); preference flips (`:196`, `:212`); restore (`:262-275`)
- **Hazards:** an exception between `:123` and `:131` leaves Ctrl held in the compositor, because `finally` restores dconf and the engine but not the key; `CDP.call` discards events while it waits, bounded only by a 20 s timeout per receive.
- **Reify:** Low — a fixture.
- **Related:** `nix/aizome-ibus-test.nix` (runs it inside ICI-15's VM)

<a id="ici-19"></a>

### ICI-19 — (test harness) Plasma Login greeter → session → logout driver (OCR-gated)
- **Where:** `tests/plasma-login-session.py:137` (`login`), `:200` (`logout_xfce`), `:61` (`wait_word`), `:41` (`frame_number`)
- **Encoding:** the VM's session phase, observed through OCR of screenshots, `pgrep`, user-unit ActiveState and the user manager's PID; the global `frame_number`, which orders the evidence files
- **States:** `Greeter` → `PasswordVerified` → `XfceSession` → `LoggedOut` → `XfceSession` → `LoggedOut` → `PlasmaSession` (on the unpatched VM: `XfceWithoutPortals`)
- **Transitions:** wait for the greeter and wake it (`:143-146`); choose the session (`:155-165`); password check loop (`:103-116`); submit (`:172`); Xfce readiness (`:183-197`); logout (`:200-214`); sequence (`:235-269`)
- **Hazards:** each OCR call can take up to `OCR_TIMEOUT` = 120 s (`:15`), so the real waits run far past the nominal timeouts; the fixed click coordinates assume the 1280×800 layout.
- **Reify:** Low — a linear script.

<a id="ici-20"></a>

### ICI-20 — (test harness) Responsiveness measurement: probes in systemd units, stop-file protocol
- **Where:** `tests/responsiveness/run.sh:138` (`measure`); also `tests/responsiveness/probe.rs:55` (`Stop`), `tests/responsiveness/terminal.sh:11`
- **Encoding:** per-scenario marker files `$dir/stop` and `$dir/terminal.stop`; transient user units `sekka-resp-{wake,frame,frame-short,exec,read,fsync,terminal}`; the outer/inner phase, via `RESPONSIVENESS_INSIDE` (`run.sh:273`); artefacts prepared once in `$state` (probe binary `:33`, calibrated `work` `:35`, `devenv.sh` `:37`, data files `:38-43`); `load.failed`, `load.times`, `psi.before`/`psi.after`
- **States:** `Outer` → `Inner(inhibited)` → `Prepared` → per scenario `ProbesStarted` → `Loading` → `Stopping` → `Stopped` → `Summarised`
- **Transitions:** re-execs itself under `systemd-run` + `systemd-inhibit` (`run.sh:273-280`); `rm -f` the markers (`:142`); `start_probes` (`:75-90`); the sampler loops until the stop file exists (`:111`); load (`:150`); `stop_probes` touches the stop file, sends fio SIGINT and waits for each unit (`:92-99`); the probe checks the stop file at most every 100 ms (`probe.rs:61-67`), then writes JSON (`probe.rs:193`); an EXIT trap stops the terminal probe (`terminal.sh:11`)
- **Hazards:** `run.sh` has no trap, so if it exits under `set -e` or `sekka-responsiveness.service` is stopped between `start_probes` and `stop_probes`, nothing touches `$dir/stop`. `stop_probes` waits on `is-active` with no timeout. Prepared artefacts are never invalidated: a changed `probe.rs` or flake keeps the old binary and `devenv.sh`, and the first run's 2 ms calibration is reused forever.
- **Reify:** Medium — a harness, but its marker files, unit names and cached artefacts form a cross-process protocol with no cleanup path.
- **Suspected bug:** stopping a measurement early (`systemctl --user stop sekka-responsiveness`) leaves the `sekka-resp-*` probe units running indefinitely. Each frame probe burns about 24% of a core (2 ms of work per 8.3 ms frame), and fio runs for up to an hour. The next `run.sh measure` then fails at `start_probes`, because `systemd-run --unit=sekka-resp-wake` finds the unit still present (confidence: medium)

<a id="ici-21"></a>

### ICI-21 — (test harness) wlr-screencopy / toplevel-export frame lifecycle client
- **Where:** `tests/screencopy-sessions.c:37` (`struct frame`: `buffer_info`, `buffer_done`, `ready`, `failed`)
- **Encoding:** four bools per frame, set by protocol events; one frame in flight at a time over one connection
- **States:** `Requested` → `BufferInfo` → `BufferDone` → `Copying` → `Ready` | `Failed` (from any state); afterwards `--hold` keeps the connection idle
- **Transitions:** capture request (`:159-161`, `:176-177`); `buffer` (`:91`); `buffer_done` (`:104`, `:126`); `dispatch_until(buffer_done, failed)` (`:162`, `:178`); copy (`:166`, `:182`); `dispatch_until(ready, failed)` (`:167`, `:183`); destroy (`:170-171`, `:186-187`); hold and roundtrip (`:240-245`)
- **Hazards:** `dispatch_until` (`:150-154`) has no timeout, so a compositor that never answers a frame hangs the client until the VM test's own timeout; every failure exits through `die` without destroying the frame.
- **Reify:** Low — a fixture that mirrors xdg-desktop-portal-hyprland's sequential capture.
- **Related:** `nix/screenshare-sessions-vm-test.nix:135` (runs it against Hyprland)

<a id="ici-22"></a>

### ICI-22 — (test harness) Session-environment baseline across activation and desktop switches
- **Where:** `tests/session-environment.py:172` (per-machine loop); `:9` (`RECORD = /run/user/1000/calamares-session-env`)
- **Encoding:** the baseline record file and the user manager's Environment property; the manager's PID; the InvocationID of `calamares-session-env.service`; the AccountsService session selection
- **States:** `Tatami` → `Switched(unchanged)` → `Switched(updated)` → `Rollback` → `LoggedOut` → `Xfce` → `Gnome` → `Tatami` → `ManagerRestarted` (the unpatched VM stops after `Xfce`, with the record corrupted)
- **Transitions:** `password_login` (`:119-125`); `select` (`:128-136`); `switch-to-configuration` (`:146-150`); wait for the greeter to retire (`:153-169`); sequence (`:189-255`)
- **Hazards:** none observed; every OCR loop has a deadline, and each tesseract call times out after 10 s (`:33`).
- **Reify:** Low — a linear script.
- **Related:** `installer/genkan/rust/system/session-env/package.nix`, `installer/genkan/rust/system/desktops.nix` (the `calamares-session-env` record it checks)

<a id="ici-23"></a>

### ICI-23 — (test harness) FakeHyprland: stateful command and event sockets for Tatami's display recovery
- **Where:** `tests/test_display_recovery.py:46` (`FakeHyprland`); event socket `:246` (`watch`)
- **Encoding:** monitor and client state in `/state/monitors` and `/state/clients`, replaced atomically (`:83-86`); behaviour switches as marker files `hang`, `refuse`, `reject-saved` (`:103`, `:118`, `:120`); the lid in `/state/lid`, read by the fake busctl; the call log `/state/calls`; the `stop` Event
- **States:** per request `Answered` | `Hung` (until close) | `RefusedAck` | `RejectedMode`; each monitor's `disabled`, `mirrorOf` and `dpmsStatus` change as eval and dispatch requests are applied (`:125-135`, `:138-161`)
- **Transitions:** accept loop (`:68-74`); `reply` dispatches on the request (`:100-136`); `evaluate` merges Lua fields (`:138-161`); `watch` hands the test the event connection, and the watcher stops when the test closes it (`:246-266`)
- **Hazards:** `close` joins only the accept thread; a `hang` handler waits on `stop` for up to 60 s in a daemon thread.
- **Reify:** Low — a fixture.
- **Related:** `desktop/tatami/tool/src/display_policy.rs` (state files `monitors-off`, `monitors.lua`, `displays-session.json`, `displays.lock`)

<a id="ici-24"></a>

### ICI-24 — (test harness) RecordingR2: conditional-write object store for nightly publication
- **Where:** `tests/test_publish_nightly.py:32` (`RecordingR2`)
- **Encoding:** the `current` index and its `etag`; the `objects` dict; failure switches `fail_upload`, `fail_index`, `conflict_claim`; the ordered `calls` log
- **States:** index `Original(etag)` → `Swapped` (only with a matching etag); prefix `Free` → `Claimed` (`publication.json`, if-none-match) → `Uploaded` → `Indexed` → `Pruned`
- **Transitions:** `put_small` claim or 412 (`:48-51`), index compare-and-swap (`:52-55`); `upload` with checksum check (`:58-64`); `delete` (`:66-68`); the order is asserted at `:305-307`
- **Hazards:** none observed in the fixture. It shows that a failed run leaves its prefix claimed (`:314`), so a same-day retry of the same revision needs manual cleanup.
- **Reify:** Low — a fixture.
- **Related:** `scripts/publish-nightly.py:300` (`publish`: claim `:352`, uploads, index CAS `:360`, prune `:362-364`); ICI-09 (the worker's cached view of the index)

<a id="ici-25"></a>

### ICI-25 — (test harness) Fake system commands steered by marker files in bwrap namespaces
- **Where:** `tests/test_sekka_update.py:46` (`FAKE`); also `tests/test_tatami_update.py:13`, `tests/test_tatami_weather.py:17`, `tests/test_tatami_background.py:155`
- **Encoding:** files under `/state` that steer and record the fakes: `building`, `fail`, `wait`, `fail-profile`, `activating`, `slow-activation`, `fail-activation`, `<unit>` PID files, and the `commands`/`calls` logs; the fake system-profile and `/run/current-system` symlinks; Tatami's `awake` and `fail-reload`; the weather fake's `offline` countdown and `delay`; the background test's queue of menu selections
- **States:** fake Yukimi system `Idle` → `Building` (→ `Waiting`) → `ProfileSwitched` → `Activating` → `Activated` | `Failed`; worker units `Running` (PID file present) → `inactive` (zombie or gone); weather network `Offline(n)` → `Online`
- **Transitions:** `nix build` touches `building` and honours `fail`/`wait` (`test_sekka_update.py:62-64`); `nix-env` relinks the profile (`:68-72`); `systemd-run` spawns the worker and records its PID (`:80`); `systemctl kill` sends it SIGINT (`:87`); `show` maps a zombie or missing PID to inactive (`:88-93`); `switch-to-configuration` marks and relinks (`:100-110`); Tatami fakes (`test_tatami_update.py:19`, `:29`); curl countdown (`test_tatami_weather.py:24-26`); the fake walker pops the queue (`test_tatami_background.py:159-160`)
- **Hazards:** none observed
- **Reify:** Low — fixtures.
- **Related:** `apps/yukimi/crates/yukimi-system/src/completion.rs` (update receipt `update.json`, states `activating`/`activated`); `desktop/tatami/tool/src/update.rs` (`update-session.json`); `desktop/tatami/tool/src/weather.rs` (`weather-refreshing`)

<a id="ici-26"></a>

### ICI-26 — (test harness) Isolated PipeWire graph driving the Tatami Sound window, including a service restart
- **Where:** `tests/test_sound.py:186` (scenario); `:14` (`start`/`processes`); `:29` (`wait`); `:117` (`choose`)
- **Encoding:** spawned daemons in `processes`; PipeWire graph objects created with pactl; AT-SPI widget states; the retry loop in `choose` (3 attempts, `:130`)
- **States:** `DaemonsUp` → `DefaultsSet` → `AppReady` → (volume, mute, balance, defaults, streams, hotplug) → `PulseKilled` → `ServiceError` → `PulseRestarted` → `Reconnected` → `SingleInstance` → `Closed`
- **Transitions:** start the daemons and wait for WirePlumber's defaults (`:187-204`); start the app (`:208`); kill pipewire-pulse (`:366`), see the error, restart it (`:369`); reconnect (`:373`); Escape and the pactl-outlived check (`:385-389`); teardown (`:403-410`)
- **Hazards:** `wait` swallows every exception its check raises for 25 s (`:29-38`), hiding real errors behind the timeout message.
- **Reify:** Low — a fixture.
- **Related:** `desktop/tatami/sound/src/audio.rs` (the window's connection and reconnect logic)

<a id="ici-27"></a>

### ICI-27 — (test harness) Yukimi in Tatami: OCR-gated page walk, polkit cancel and rescale
- **Where:** `tests/yukimi-tatami.py:57` (`capture_until`), `:119` (`click_word`), `:116` (`SCALE`)
- **Encoding:** on-screen text read by OCR; Hyprland clients/layers/monitors JSON; whether `pkexec`/`yukimi-helper` processes exist; the hash of `configuration.nix`; the module-global `SCALE`, which maps OCR pixels to compositor coordinates
- **States:** `SessionReady` → `Scaled(2)` → `LauncherOpen` → `Overview` → pages … → `Discover` → `SteamDetails` → `InstallSheet` → `AuthDialog` → `Cancelled(nothing changed)` → `Scaled(1.25)` → `Scaled(1, 1280×800)` → `Quit`
- **Transitions:** apply the scale (`:97-99`); launcher (`:101-106`); page walk (`:162-174`); install, authentication and cancel (`:205-231`); rescale loop that rebinds `SCALE` (`:234-251`); quit (`:253-257`)
- **Hazards:** `SCALE` is a module global rebound mid-run (`:237`) and read by `click_word`; `capture` presses Shift before every frame (`:51`).
- **Reify:** Low — a linear script.
- **Related:** `apps/yukimi` (helper behind pkexec); Tatami's polkit agent

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `.github/actionlint.yaml` | 3 | none |  | runner label config |
| `.github/workflows/cache.yml` | 107 | machines | [ICI-01](#ici-01) | main cache publication stages |
| `.github/workflows/kakehashi.yml` | 96 | none |  | build matrix on hosted runners; artifacts only; no gating state |
| `.github/workflows/nightly.yml` | 150 | machines | [ICI-02](#ici-02) | input update and branch promotion |
| `.github/workflows/pr.yml` | 152 | machines | [ICI-03](#ici-03) | trust gate and commit status lifecycle |
| `.github/workflows/release.yml` | 85 | machines | [ICI-04](#ici-04) | tag to draft to published release |
| `.github/workflows/sites.yml` | 167 | machines | [ICI-05](#ici-05) | publish decision vs live revision |
| `aizome-site/src/apps.rs` | 836 | none |  | data tables and static HTML |
| `aizome-site/src/color.rs` | 85 | none |  | pure colour math |
| `aizome-site/src/glyphs.rs` | 180 | none |  | pure glyph outlining |
| `aizome-site/src/html.rs` | 161 | machines | [ICI-06](#ici-06) | page shell; hosts theme switch markup |
| `aizome-site/src/main.rs` | 741 | machines | [ICI-05](#ici-05) | static generator; revision stamp read by sites.yml |
| `aizome-site/src/pages.rs` | 1023 | none |  | static page content |
| `aizome-site/src/theme.rs` | 417 | machines | [ICI-06](#ici-06) | theme parser; Day CSS mirror of the switch |
| `aizome-site/theme.js` | 59 | machines | [ICI-06](#ici-06) | Night/Day switch in localStorage |
| `docs/brand/generate.py` | 201 | none |  | pure SVG generator |
| `docs/upstream/plymouth/console-owner.patch` | 70 | machines | [ICI-07](#ici-07) | local console ownership release |
| `docs/upstream/plymouth/keyboard-removal.patch` | 302 | machines | [ICI-08](#ici-08) | splash keyboard rebinding |
| `site/download-worker/pages-shim.js` | 19 | none |  | one-time Wasm instantiation at module load |
| `site/download-worker/src/lib.rs` | 105 | machines | [ICI-09](#ici-09) | edge-cached builds index |
| `site/download-worker/src/render.rs` | 285 | machines | [ICI-09](#ici-09) | status classification of the index; pure rendering |
| `site/src/brand.rs` | 176 | none |  | SVG inlining; id counter only |
| `site/src/color.rs` | 84 | none |  | pure colour math |
| `site/src/glyphs.rs` | 105 | none |  | pure glyph outlining |
| `site/src/html.rs` | 121 | machines | [ICI-06](#ici-06) | page shell; hosts theme switch markup |
| `site/src/main.rs` | 631 | machines | [ICI-05](#ici-05) | static generator; revision stamp read by sites.yml |
| `site/src/pages.rs` | 1210 | machines | [ICI-09](#ici-09) | static content; download page fallback markers |
| `site/src/shared.rs` | 31 | machines | [ICI-09](#ici-09) | BUILDS_TTL constant |
| `site/src/tokens.rs` | 309 | machines | [ICI-06](#ici-06) | token CSS; Day mirror of the switch |
| `site/theme.js` | 59 | machines | [ICI-06](#ici-06) | Night/Day switch in localStorage |
| `site/video.js` | 436 | machines | [ICI-10](#ici-10), [ICI-11](#ici-11), [ICI-12](#ici-12), [ICI-13](#ici-13) | video load, playback, controls, volume |
| `site/video/hype.js` | 1034 | machines | [ICI-14](#ici-14) | generated wasm-bindgen glue |
| `tests/aizome-ibus.py` | 371 | machines | [ICI-15](#ici-15) | VM session-switch driver |
| `tests/application-matrix.nix` | 110 | none |  | tests (eval assertions) |
| `tests/applications-runtime.sh` | 117 | none |  | tests (in-VM functional checks) |
| `tests/aquamarine-null-connector.cpp` | 19 | none |  | tests (regression fixture) |
| `tests/check-ibus-wayland-serial` | 23 | none |  | tests (before/after compile check) |
| `tests/check_avahi_startup.py` | 156 | machines | [ICI-16](#ici-16) | gdb-driven daemon phases |
| `tests/check_config.rs` | 11 | none |  | dispatcher only |
| `tests/check_googlebook_image.py` | 61 | none |  | tests (image layout assertions) |
| `tests/check_installed_template.py` | 47 | none |  | tests (eval assertions) |
| `tests/check_pipewire_x11_disconnect.py` | 111 | machines | [ICI-17](#ici-17) | spawn/ready/teardown driver |
| `tests/check_wallpapers.py` | 62 | none |  | tests (file assertions) |
| `tests/check_xapp_settings.py` | 112 | machines | [ICI-17](#ici-17) | spawn/ready/teardown driver |
| `tests/desktop-matrix.nix` | 203 | none |  | tests (eval assertions) |
| `tests/ibus-chrome.py` | 279 | machines | [ICI-18](#ici-18) | CDP probe with held modifier |
| `tests/ibus-gtk.c` | 36 | none |  | tests (GTK entry probe) |
| `tests/ibus-wayland-serial.c` | 89 | none |  | tests (fixture for nix/patches/ibus-wayland-serial.patch) |
| `tests/libsecret-dh-padding.c` | 51 | none |  | tests (deterministic padding check) |
| `tests/measure_yukimi.py` | 72 | none |  | read-only sampling loop |
| `tests/plasma-login-config.py` | 81 | none |  | tests (config precedence generator) |
| `tests/plasma-login-session.py` | 274 | machines | [ICI-19](#ici-19) | OCR-gated login/logout driver |
| `tests/process_cleanup.rs` | 52 | none |  | tests (respin-tools Process drop) |
| `tests/qmp_input.rs` | 10 | none |  | dispatcher only |
| `tests/responsiveness/probe.rs` | 194 | machines | [ICI-20](#ici-20) | stop-file polling probes |
| `tests/responsiveness/run.sh` | 294 | machines | [ICI-20](#ici-20) | measurement phases and unit protocol |
| `tests/responsiveness/terminal.sh` | 22 | machines | [ICI-20](#ici-20) | terminal probe with EXIT trap |
| `tests/scheduling-probe.c` | 71 | none |  | signal-driven fixture; no phase |
| `tests/screencopy-sessions.c` | 248 | machines | [ICI-21](#ici-21) | frame capture lifecycle |
| `tests/session-environment.py` | 260 | machines | [ICI-22](#ici-22) | baseline across switches driver |
| `tests/terminal-input-probe.c` | 50 | none |  | raw-mode echo fixture |
| `tests/test_agent_workloads.py` | 62 | none |  | tests |
| `tests/test_cache_coverage.py` | 302 | none |  | tests (disposable caches) |
| `tests/test_chatgpt_plugins.cjs` | 104 | none |  | tests |
| `tests/test_ci_cache.py` | 249 | none |  | tests of scripts/ci-retain.py (see ICI-02) |
| `tests/test_display_recovery.py` | 548 | machines | [ICI-23](#ici-23) | FakeHyprland stateful fake |
| `tests/test_launcher_search.py` | 227 | machines | [ICI-17](#ici-17) | Elephant spawn/ready/teardown |
| `tests/test_pr_trust.py` | 170 | none |  | tests of scripts/check-pr-trust.py (see ICI-03) |
| `tests/test_publish_nightly.py` | 444 | machines | [ICI-24](#ici-24) | RecordingR2 conditional-write fake |
| `tests/test_release.py` | 64 | none |  | tests |
| `tests/test_sekka_update.py` | 546 | machines | [ICI-25](#ici-25) | marker-file fake commands |
| `tests/test_sound.py` | 410 | machines | [ICI-26](#ici-26) | PipeWire graph driver with restart |
| `tests/test_tatami_background.py` | 167 | machines | [ICI-25](#ici-25) | fake walker selection queue |
| `tests/test_tatami_update.py` | 144 | machines | [ICI-25](#ici-25) | marker-file fake commands |
| `tests/test_tatami_weather.py` | 198 | machines | [ICI-25](#ici-25) | fake curl offline countdown |
| `tests/test_usb_wake.py` | 48 | none |  | tests (fake sysfs) |
| `tests/yukimi-tatami.py` | 257 | machines | [ICI-27](#ici-27) | OCR-gated Yukimi UI driver |
