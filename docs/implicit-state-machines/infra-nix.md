# INX: Nix integration, carried patches and live-media modules

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `nix, modules, templates, flake.nix`. Audited at `8e0acbe5`; line numbers refer to that commit.

128 files audited. 46 implicit machines: 5 High, 4 Medium, 37 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="inx-18"></a>

### INX-18 — Copy-to-RAM of the live media in the initrd
- **Where:** `nix/iso-copytoram.nix:42` (`stage`); also `nix/iso-copytoram.nix:22-40`, `:44-50`, `:68-84`, `modules/installer.nix:80-86`, `nix/iso-copytoram-test.nix:27-56`, `nix/iso-copytoram-initrd-test.nix:15-17`, `nix/iso-copytoram-initrd-test.py:107-108`
- **Encoding:** mounts at `/sysroot/iso`, `$stage` (private bind), `$stage/ram` (tmpfs sized to the media), `$stage/media` (rollback bind); the EXIT trap; the unit's exit status (upstream unit is `requiredBy = initrd.target`)
- **States:** `Probe` → `SizeUnknown` | `InsufficientRAM` (continue from media) → `Staged` → `RAMAllocated` → `Copied` → `RollbackHeld` → `MediaUnmounted` → `Switched(RAM at /sysroot/iso)` | `Restored(media back)` | `Stranded(media kept at $stage/media, exit 1)`
- **Transitions:** size probe (`:22-26`); RAM check with 4 GiB reserve (`:27-40`); staging (`:42-56`); tmpfs (`:57-60`); copy (`:61-64`); rollback bind (`:68`); unmount media (`:69-72`); move RAM in (`:73`); restore or strand (`:74-82`); success (`:84`); trap unmounts leftovers (`:44-50`)
- **Hazards:** `:68` is the one unguarded step: under `writeShellApplication`'s errexit a failure exits 1, failing the initrd even though the media are intact, unlike every other branch; a failure inside `cleanup` stops the trap and leaves `/run/sekka-copytoram.*`
- **Reify:** High — boot-critical, six mount phases in one linear script, one unguarded step
- **Suspected bug:** the media size is the size of the block device holding `/sysroot/iso` (`:23`), not of the ISO. If that device is a whole USB stick (a raw-written hybrid ISO found by label on the disk rather than its partition), `bytes` is the stick's capacity, so a 64 GB stick on a 32 GB machine is reported "insufficient RAM" and copytoram is silently skipped though the image would fit (confidence: low)
- **Reproduction:** Partly confirmed (unit): [summary](reproductions.md#inx-18)

<a id="inx-26"></a>

### INX-26 — Hyprland managed screencopy session lifecycle (patched)
- **Where:** `nix/patches/hyprland-screenshare-sessions-0.57.patch:173` (`SManagedSession`: `m_session`, `stoppedListener`, `m_idleTimer`, `m_lastUsed`, `m_clientDestroy`); also `nix/patches/hyprland-screenshare-sessions.patch:216-229`, `nix/overlays/hyprland.nix:19-29`, `nix/screenshare-sessions-vm-test.nix:154-206`
- **Encoding:** membership in `m_managedSessions`, the session's `m_stopped`, `m_lastUsed`, an event-loop timer, a late client-destroy listener, pending frames in `m_pendingFrames`, and the region identity `m_requestedBox` (logical) vs derived `m_captureBox` (pixels)
- **States:** `Created` → `InUse` (each lookup bumps `m_lastUsed`) ↔ `Idle` → `Stopped` (idle ≥ 5 s with no pending frame | client destroyed | evicted as LRU at 32 per client | monitor gone) → `Erased`; invalid-but-representable `m_stopped && still in m_managedSessions` (lookups and timer skip it)
- **Transitions:** lookup skips stopped, region by requested box (`nix/patches/hyprland-screenshare-sessions-0.57.patch:28`, `:36`); LRU eviction preferring idle (`:44-64`); timer armed at creation (`:80-85`); bump (`:88`); timer re-arms or stops (`:93-106`); client destroy stops (`:112-116`); ctor/dtor register and cancel (`:127-143`); erase by address (`:73-78`); pixel box re-derived on constraint changes (`:236-237`); older variant (`hyprland-screenshare-sessions.patch:69-158`, `:169-185`, `:266-271`)
- **Hazards:** each `stop()` synchronously destroys the object being called through (comments `:63`, `:105`, `:115`); safe only because nothing touches it afterwards; when all 32 sessions are busy the eviction stops one with a frame pending; a client polling slower than every 5 s gets a fresh session each time
- **Reify:** High — five fields, four end conditions in four handlers, self-destroying callbacks
- **Related:** INX-39; clients: portal region shares (xdg-desktop-portal-hyprland), `tests/screencopy-sessions.c`

<a id="inx-29"></a>

### INX-29 — IBus Wayland frontend keymaps, seat and system-keymap mode (patched)
- **Where:** `nix/patches/ibus-wayland-system-keymap.patch:68` (`key_user`, `key_sys`); also `:24` (`has_compositor_keymap`), `:417` (`use_sys_keymap`), `nix/patches/ibus-wayland-system-keymap-dependencies.patch:21-76`, `:165-220`
- **Encoding:** two `IBusXkbKeymap` structs (keymap, state, eleven modifier masks), `seat->has_compositor_keymap`, `seat` itself (NULL after removal), `seat->active`, `priv->use_sys_keymap` set from the panel's GSettings signal
- **States:** `NoKeymaps` (keys forwarded raw) → `SystemOnly` → `UserAndSystem`; seat `Absent` | `Present(active?, compositorKeymap?)`; mode `EngineKeymap` | `SystemKeymap` (no text commits for unhandled printable keys); invalid-but-representable `use_sys_keymap && !key_sys.state`
- **Transitions:** engine change rebuilds `key_user` (`nix/patches/ibus-wayland-system-keymap.patch:155-160`, `:425-428`); compositor keymap forwarded to the virtual keyboard and flags set (`:165-172`); then **returns early if both states exist** (`:175-178`), else fills `key_sys` and, if absent, `key_user` (`:181-194`); activate clears `has_compositor_keymap` (`:363-364`); active keymap chosen by mode (`:437-441`, `:486-489`); no unsolicited commit in system mode (`:448-459`); raw forward when no states (`:467-469`); property and panel wiring (`:504-510`, `:519-521`, `:555-556`, `:587-592`); NULL-seat guards and cleanup (`nix/patches/ibus-wayland-system-keymap-dependencies.patch:21-76`, `:157`, `:165-220`)
- **Hazards:** `key_sys` is set once per frontend process; later compositor keymaps (live layout changes) reach the virtual keyboard but not IBus's own state, while system mode reads keysyms and modifier masks from `key_sys`
- **Reify:** High — mode, two keymap structs and seat flags across six handlers, a documented user-visible limit
- **Suspected bug:** change the Aizome layout (e.g. US → German) while `aizome-ibus` runs with `use-system-keyboard-layout` on: the early return at `:175-178` keeps the old `key_sys`, so engines get keysyms and modifiers from the old layout. This is the probable mechanism of the known limit in DEVELOPERS.md:368-373 (confidence: medium)
- **Related:** `installer/genkan/rust/system/input-method.nix` (`aizome-ibus.service`), Aizome compositor's input-method/virtual-keyboard handling (desktop/aizome), `tests/aizome-ibus.py`
- **Reproduction:** Partly confirmed (unit): [summary](reproductions.md#inx-29)

<a id="inx-34"></a>

### INX-34 — Binary-cache publication pipeline
- **Where:** `nix/push-cache.nix:52` (`stage`); also `:42-48`, `:61-66`, `:67-86`
- **Encoding:** shell variables `outputs`, `closure`, `own`; the staging directory; the bucket's objects (NARs before narinfos) and `nix-cache-info`
- **States:** `KeyChecked` → `Built(optionally rooted)` → `SignedLocally` → `Filtered(own paths)` → `NARsUploaded` → `NarinfosUploaded` → `CacheInfoWritten`; an interrupted run leaves NARs without narinfos, never the reverse
- **Transitions:** key check (`:22-25`); build through `ci-retain.py` or plain Nix with command substitution (`:43-48`); sign everything (`:54`); closure and per-upstream filter by process substitution (`:61-66`); hard-link (`:67-74`); ordered syncs (`:78-81`); cache info (`:84-85`)
- **Hazards:** `--size-only` skips a same-size narinfo whose contents changed; the filter depends on three external caches answering correctly at publication time
- **Reify:** High — persisted remote state with an ordering invariant; error handling differs between stages
- **Suspected bug:** `:61` and `:64` use process substitution, whose exit status `errexit`/`pipefail` ignore (the comment at `:40-41` fixed this only for the build). If `nix-store --store https://cache.nixos.org --check-validity` fails part-way (network error, 5xx), `own` keeps only the paths checked before the failure, the script uploads that subset or nothing, prints "Published N of M" and exits 0: the nightly reports success while the cache lacks Sekka's packages and installs compile them (confidence: medium)
- **Related:** `scripts/ci-retain.py`, the cache and nightly workflows (`.github/workflows/cache.yml`, `.github/workflows/nightly.yml`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#inx-34)

<a id="inx-36"></a>

### INX-36 — Self-hosted runner supervisor and per-job admission
- **Where:** `nix/runner.nix:157` (supervisor `while true`); also `nix/runner.nix:72` (`admit`), `:87` (`job`), `:123`
- **Encoding:** the Docker container named `sekka-runner-job`, files under `$secrets` (`headers`, `job.env`), the token file, the Git object cache, the CI roots mount; per job the runner's `ACTIONS_RUNNER_HOOK_JOB_STARTED` hook
- **States:** supervisor `WaitPrevious` → `RefreshGitCache` → `NoToken(sleep 60)` | `Registering` → `RegisterFailed(sleep 60)` | `JobRunning` → `JobEnded` → `Cooldown(5 s)` → loop; job `Admitting` → `Rejected(kill -KILL -1)` | `Admitted` → `Pruned` → `Seeded` → `Steps`
- **Transitions:** wait for a job from an earlier supervisor (`:159`); refresh only between jobs (`:162-164`); token check (`:165-169`); JIT registration (`:176-186`); `docker run --rm` (`:188-205`); drop the registration file and cool down (`:206-207`); admission (`:75-79`), prune and seed (`:82-83`)
- **Hazards:** a JIT registration is spent on every loop whether or not the container starts; Git cache refresh is bounded by `timeout 180`, but `docker wait` is not
- **Reify:** High — long-running loop with remote side effects, several retry paths and no backoff on one of them
- **Suspected bug:** if `docker run` fails to start (image `sekka-runner:latest` not loaded, missing bind source `/var/cache/sekka-ci-git`), the loop registers a new JIT runner with GitHub every ~5 s plus refresh time (`:176-207`) with no backoff, leaving a trail of never-connected runner registrations and API calls until someone notices (confidence: medium)
- **Related:** `scripts/runner-admission.sh`, `scripts/runner-git-cache.sh`, `scripts/ci-retain.py`, `scripts/runner-host-setup.sh`; GitHub's JIT runner API
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#inx-36)
