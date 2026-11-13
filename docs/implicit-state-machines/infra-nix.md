# INX: Nix integration, carried patches and live-media modules

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `nix, modules, templates, flake.nix`. Audited at `8e0acbe5`; line numbers refer to that commit.

128 files, 0 pending. 46 implicit machines: 5 High, 4 Medium, 37 Low.

## Machines

<a id="inx-01"></a>

### INX-01 — (test harness) Deadline-poll helpers and compositor-readiness waits shared by the VM test scripts
- **Where:** `nix/activity-vm-test.nix:124` (`wait_action`), `:138` (`wait_paused`); also `nix/aizome-greeter-desktops-vm-test.nix:58` (`until`), `nix/background-validation-vm-test.nix:66` (`wait`), `nix/capture-cursor-vm-test.nix:106` (`Compositor.wait_for`), `nix/display-recovery-vm-test.nix:103` (`wait_for`), `:167` (`wait_for_dpi`), `nix/displays-cursor-vm-test.nix:47` (`as_alice`), `:68` (`wait_for`), `nix/displays-fit-vm-test.nix:51`, `nix/genkan-keyboard-test.py:32` (`until`), `nix/hyprlauncher-software-test.nix:74` (`until`), `nix/network-dns-vm-test.nix:62`, `nix/network-fit-vm-test.nix:46`, `:102` (OCR `text` loop), `nix/screenshare-sessions-vm-test.nix:115`, `nix/screenshot-notifications-vm-test.nix:70`, `:79` (`instances`), `nix/screenshots-aizome.nix:123` (`wait_window`), `nix/screenshots.nix:188`, `:196`, `:205`, `nix/tatami-recording-test.nix:66`, `:80`, `nix/window-pop-vm-test.nix:46`
- **Encoding:** per-test copies of a `deadline = time.monotonic() + N` loop over a predicate, plus fixed `time.sleep` settle periods between steps; the Hyprland instance `signature` captured once from `basename /run/user/1000/hypr/*` (e.g. `nix/activity-vm-test.nix:178`, `nix/network-dns-vm-test.nix:179`, `nix/window-pop-vm-test.nix:35`, `nix/screenshots.nix:175`)
- **States:** per wait `Polling` → `Satisfied` | `TimedOut` (screenshot/diagnostics, raise); session readiness `NoSocket` → `SocketPresent` → `InstancesAnswered` (`wl_socket`, `pid`) → `SessionServicesActive` (`tatami-waybar`)
- **Transitions:** socket appears before instance metadata, so tests poll `hyprctl -j instances` (`nix/capture-cursor-vm-test.nix:81-93`, `nix/screenshot-notifications-vm-test.nix:79-86`, `nix/tatami-recording-test.nix:66`); `genkan-keyboard-test.py` treats an exception as "not yet" (`nix/genkan-keyboard-test.py:38-40`); `network-fit` waits for the watcher's recentring, not just the scale change (`nix/network-fit-vm-test.nix:61-68`); `window-pop` drives tiled → popped (`pop` tag, floating, pinned) → returned (`nix/window-pop-vm-test.nix:80-107`)
- **Hazards:** the signature is taken once with a glob; if Hyprland's first start failed and the getty restarted it (as `nix/displays-cursor-vm-test.nix:45-46` says can happen), two instance directories exist, `basename` prints two lines and every later `hyprctl` call gets a malformed `HYPRLAND_INSTANCE_SIGNATURE` (only `displays-cursor` picks the newest with `ls -t | head -1`); `genkan-keyboard`'s `until` swallows every exception, so a persistent error looks like a timeout; fixed sleeps stand in for unobservable settle states
- **Reify:** Low — test helpers; a shared helper module would remove the copies

<a id="inx-02"></a>

### INX-02 — (test harness) Activity: stand-in power meter and pause/resume probes
- **Where:** `nix/activity-vm-test.nix:14` (`standIn`); also `:184-191`, `:218-235`
- **Encoding:** a Python Unix-socket server holding `start`/`base` from startup (`:32-33`), the socket path `/run/tatami/power.sock`, and the socket unit `tatami-power-meter.socket` it replaces
- **States:** `SocketActivatedService` (real meter, no RAPL) → `Stopped` → `StandInListening` → per client `Accepted` → `Answered` → `Closed`; app side `Running` ↔ `Paused` (SIGSTOP, `ps` stat `T`)
- **Transitions:** socket activation (`:185-190`); stop socket and service, start the stand-in (`:233-235`); per accept send cumulative energy (`:41-47`); inspector button pauses/resumes and the test polls `ps` and the action's enabled state (`:223-229`, `:138-146`, `:124-136`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** Tatami Activity and `tatami power-meter` (desktop/tatami/activity, desktop/tatami)

<a id="inx-03"></a>

### INX-03 — (test harness) Greeter session cycle with remembered last session
- **Where:** `nix/aizome-greeter-desktops-vm-test.nix:35` (inspection directory via `AIZOME_KIT_INSPECT`); also `:70` (`greeter_up`), `:83` (`choose`), `:90` (`log_out`)
- **Encoding:** the kit's inspection JSON files in `/run/greeter-inspect`, which the test deletes at each logout (`:93`), and the greeter's persisted `/var/lib/aizome-greeter/state.toml`
- **States:** `GreeterUp` → `MenuOpen(Sessions)` → `SessionChosen` → `LoggedIn(Tatami | Xfce | Aizome)` → `LoggedOut` → `GreeterUp` showing the previous session
- **Transitions:** greeter up (`:71-73`); F2 menu, type, Return (`:84-88`); password → session processes, greeter gone (`:114-116`); `loginctl terminate-user`, wait for no `demo` processes, clear inspection files (`:91-93`); remembered session shown (`:123`, `:134`) and persisted (`:140-141`)
- **Hazards:** inspection files from a dead greeter would satisfy `shows()` if `log_out` did not delete them; the deletion is the only thing separating greeter generations
- **Reify:** Low — fixture
- **Related:** Aizome greeter (desktop/aizome greeter, state file `/var/lib/aizome-greeter/state.toml`)

<a id="inx-04"></a>

### INX-04 — (test harness) AccountsService session seed, readiness window and activation fixtures
- **Where:** `nix/aizome-ibus-test.nix:51` (`system.activationScripts.ibusTestSession`); also `nix/aizome-ibus-test.nix:66-69`, `nix/session-environment-vm-test.nix:29-48`
- **Encoding:** `/var/lib/AccountsService/users/<user>` written only if absent; an `ExecStartPost=sleep 3` drop-in on `org.freedesktop.IBus.session.GNOME`; a `specialisation.updated` changing the `calamares-session-env` unit and an `original` node with `restartIfChanged = true`, `RemainAfterExit = false`
- **States:** seed `Absent` → `Seeded` → `UpdatedByGDM` (activation never overwrites); IBus unit `Activating(PID visible)` → `Active` (window widened to 3 s); session-env snapshot `Base` → `Switched(updated)` → `RolledBack`
- **Transitions:** create-once seed (`nix/aizome-ibus-test.nix:53-61`, `nix/session-environment-vm-test.nix:31-39`); readiness delay (`nix/aizome-ibus-test.nix:68`); negative-control service settings (`nix/session-environment-vm-test.nix:45-48`); the drivers are `tests/aizome-ibus.py` and `tests/session-environment.py`
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `tests/aizome-ibus.py`, `tests/session-environment.py`; `installer/genkan/rust/system` (`calamares-session-env`, `input-method.nix`)

<a id="inx-05"></a>

### INX-05 — (test harness) Aizome login → session → logout environment lifecycle
- **Where:** `nix/aizome-login-vm-test.nix:95`; also `:106-120`, `:164-183`
- **Encoding:** process presence (`aizome-greeter`, `aizome`, `aizome-shell`), the session's `/proc/<pid>/environ`, `AIZOME_ENVIRONMENT_KEYS`, the lingering user manager's `show-environment`, the login keyring's `Locked` property
- **States:** `Greeter` → `PasswordTyped` → `SessionRunning(keys exported to user manager, keyring unlocked)` → `SessionTerminated` → `Greeter` with the keys removed from the still-running manager
- **Transitions:** QMP key events typed one key at a time (`:70-92`, `:107`); session socket and shell appear, greeter exits (`:108-110`); keyring `b false` (`:123-128`); `loginctl terminate-session`, compositor and session gone, `default.target` still active (`:175-178`); keys cleared (`:180-182`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** Aizome session environment handling (desktop/aizome session, `aizome-session`)

<a id="inx-06"></a>

### INX-06 — (test harness) Wallpaper menu-panel driver with reject-keeps-state invariant
- **Where:** `nix/background-validation-vm-test.nix:97` (`panel`); also `:111` (`state`), `:114` (`choose_path`), `:106` (`compositor`)
- **Encoding:** walker `--dmenu -p <prompt>` in the user's process list plus a mapped `walker` layer as readiness; the triple (status, `~/.local/state/tatami/background` symlink, `tatami-swaybg` MainPID); compositor PID plus start time
- **States:** `MenuClosed` → `MenuOpen` → `Panel(prompt)` … → `PathEntered` → `Applied` | `Rejected(notice)`; settings `Before` → unchanged on reject → `Valid` on recovery
- **Transitions:** panel readiness poll (`:101-105`); menu walk (`:115-137`); rejection leaves `state()` and pixels unchanged (`:157-164`); valid image changes the symlink (`:165-173`); compositor identity unchanged (`:174`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `tatami background` and `tatami-swaybg` (desktop/tatami)

<a id="inx-07"></a>

### INX-07 — (test harness) Boot-splash hold gate and Plymouth mode driver
- **Where:** `nix/boot-splash-test.nix:25` (`systemd.services.splash-hold`); also `:38` (`slow-stop`), `:60` (`splash_up`)
- **Encoding:** a oneshot ordered `before` `plymouth-quit*.service` that loops until `/run/splash-release` exists (`TimeoutStartSec = 10min`); a `RemainAfterExit` unit whose `ExecStop` sleeps 20 s
- **States:** `SplashHeld(splash-hold activating)` → `MessageShown` → `AskingPassword(ask.service)` → `Answered` → `HintShown` ↔ `Details(Esc)` → `Released` → `LoginPrompt`; then manual `reboot`/`shutdown` modes; `PoweringOff(slow-stop)`
- **Transitions:** hold observed as `activating` (`:62`); display/hide message (`:77-80`); password unit and answer (`:84-104`); Esc toggles (`:112-116`); release marker (`:120-124`); `plymouthd --mode` (`:130-137`); poweroff (`:141-144`)
- **Hazards:** a failure before `:120` leaves boot held until the 10-minute start timeout
- **Reify:** Low — fixture
- **Related:** `installer/genkan/rust/system/boot-splash`

<a id="inx-08"></a>

### INX-08 — (test harness) Capture-cursor compositor driver
- **Where:** `nix/capture-cursor-vm-test.nix:73` (`class Compositor`); also `:205-218`, `:227-236`, `:270-300`, `:311-315`
- **Encoding:** `Compositor.wayland`/`.pid` filled once instances answer, a `fixture` user unit (foot), a `record` user unit (wf-recorder), a headless `CAPTURE-1` output and mirror configuration, bounded 30-iteration DPMS polls
- **States:** `Starting` → `InstanceReady` → `FixtureFullscreen` → per protocol/scale/transform `Configured` → `Captured`; recorder `Started` → `SIGINT` → `Inactive(success)`; mirror `Created` → `Mirrored` → `DPMSOff(fading)` → `DPMSOn` → `Removed`
- **Transitions:** readiness (`:83-98`); monitor reconfigure + wait (`:176-180`); recorder unit start/stop/result (`:211-218`); mirror create (`:229-236`); DPMS fade/wake loops (`:282-300`); mirror removal (`:311-315`); PID identity at the end (`:349-350`)
- **Hazards:** the wake loop (`:295-299`) ends after 30 tries without failing; only the assertion after it catches a non-recovery
- **Reify:** Low — fixture
- **Related:** INX-25 (the patch under test)

<a id="inx-09"></a>

### INX-09 — ChatGPT executor plugin copy serialized per destination
- **Where:** `nix/chatgpt/copy-plugin.cjs:5` (`pending: Map`); also `nix/chatgpt/copy-plugin.cjs:24` (`copyPlugin`), `nix/chatgpt/patch-asar.py:38` (splices the helper into the ASAR)
- **Encoding:** `pending` maps destination → tail promise; file modes in the copy (store modes vs `u+w`)
- **States:** per destination `Idle` → `Copying` (+ queued successors) → `Idle`; per copy `RepairExisting` → `Copied(without .mcp.json)` → `RepairedNew`
- **Transitions:** chain after the previous copy, ignoring its failure (`copy-plugin.cjs:45-46`); publish tail (`:47`); clear only if still the tail (`:48-50`); repair, copy filtered, repair (`:32-39`); refuse a destination owned by another user (`:13-15`) or not a directory (`:29-31`)
- **Hazards:** serialization is per process: two app processes copying the same destination still race; chained copies run even after the previous one failed
- **Reify:** Low — small, local and correct
- **Related:** upstream ChatGPT app's executor (patched in `nix/chatgpt/patch-asar.py`); `tests/test_chatgpt_plugins.cjs`

<a id="inx-10"></a>

### INX-10 — (test harness) CI GC-root retention driver
- **Where:** `nix/ci-cache-test.nix:77` (`root = /nix/var/nix/gcroots/sekka-ci`); also `:83-85`, `:88-131`
- **Encoding:** per-run root directories (`main-checks`, `pr-uta`, `pr-checks`) with `success` markers, `SEKKA_CI_ROOTS`, `SEKKA_CI_MIN_FREE_BYTES`
- **States:** `NoRoots` → `Rooted(main, success)` → reused without building (`--max-jobs 0`) ; `PRFailed(no success, inputs of passing checks kept)` → `PrunedForSpace(PR roots dropped first)` → `Expired(dir removed, paths collectable)`; `PlainNix(no roots mount)`
- **Transitions:** rooted check (`:89`), GC keeps it (`:93-95`), rerun from roots (`:98`); failure never succeeds (`:101-103`); keep-going partial (`:106-111`); plain mode (`:115-117`); low space prune (`:120-125`); expiry (`:128-131`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `scripts/ci-retain.py` (the machine itself), INX-34, INX-36

<a id="inx-11"></a>

### INX-11 — (test harness) Display recovery: fake lid and dock/undock/lid driver
- **Where:** `nix/display-recovery-vm-test.nix:17` (`fakeLid`); also `:74-76`, `:112` (`apply`), `:140-160`, `:191-223`, `:225-276`
- **Encoding:** a uinput `SW_LID` device fed from FIFO `/run/fake-lid` (reopened after each writer closes, `:25-29`); headless `eDP-1`/`DP-1` outputs; Tatami's persisted `~/.local/state/tatami/displays.json`/`displays.lua` and runtime `monitors.lua` (`:61-62`); the `display-recovery` user unit running `tatami displays watch`
- **States:** `Main` | `Extended` | `Mirrored` | `PanelOff` → `Undocked(no outputs → panel back)` → `Reloaded` → `Redocked(panel off again)`; lid `Open` ↔ `Closed`; window `OnScreen` → `Stranded` → `Rescued`
- **Transitions:** apply (`:131-139`); two undock/reload/redock cycles (`:147-158`); Xft.dpi follows main scale (`:178-188`); lid closed/open with logind (`:200-212`); undocked behind closed lid → DPMS off → lid open wakes (`:215-220`); window rescue after unplug and after an app moves it off-screen (`:244-261`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `tatami displays watch` / `apply` (desktop/tatami/displays)

<a id="inx-12"></a>

### INX-12 — (test harness) Genkan keyboard-step driver over the kit's inspection files
- **Where:** `nix/genkan-keyboard-test.py:91` (`class Genkan`); also `nix/genkan-keyboard-test.nix:31` (debug preview build), `nix/genkan-keyboard-test.py:60` (`launch`), `:142` (`choose`)
- **Encoding:** `AIZOME_KIT_INSPECT` JSON documents per window/menu (`:101-108`), cleared before launch (`:95`); Genkan's `listen` button and `swap-alt-super` checkbox state; `/etc/xdg/aizome/hotaru-shell` and `~/.config/aizome/settings.toml` edited mid-test
- **States:** keyboard step `Sample` → `Waiting` → `SpaceRefused` → `AltDetected(swap recommended)` → `Declined` → `Waiting` → `Stopped(Esc)` → `ControlDetected(no swap)`; Hotaru panes follow the shell file per new pane
- **Transitions:** press again (`:260-261`); space (`:263-264`); Alt (`:265-272`); decline (`:275-277`); Esc (`:280-283`); Control (`:286-293`); shell file swap and new panes (`:222-229`); options cleared by settings and `reload-config` (`:241-253`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** Genkan's keyboard step (installer/genkan/rust), the kit's inspection output (desktop/aizome/crates/aizome-kit), Hotaru's shell choice

<a id="inx-13"></a>

### INX-13 — (test harness) Live-media guard mount-state fixture
- **Where:** `nix/genkan-live-media-test.nix:82` (`fixture`); also `:98` (`iso`), `:101` (`ram_media`), nodes `:54-68`
- **Encoding:** a `live-media-fixture` transient unit with `PrivateMounts=yes` whose mount namespace all commands enter by MainPID (`:72-74`, `:90`); mounts at `/iso` and `/nix/.ro-store`; kernel `copytoram`; root filesystem type
- **States:** `RoStoreUnmounted` → `ISOMounted` | `RAMMedia(tmpfs /iso + squashfs /nix/.ro-store)` | `StoreFromUnrelatedImage` | `ImageRenamed`; guard `Accepted` | `Refused(reason)`
- **Transitions:** unmount loop (`:92`); build fixture ISO (`:93-96`); each subtest moves mounts and checks the probe (`:107-156`)
- **Hazards:** mounting in PID 1's namespace would race systemd's asynchronous teardown of `nix-store.mount` (comment `:85-89`); hence the private namespace
- **Reify:** Low — fixture
- **Related:** `installer/genkan/nix/live-guard-probe.rs` and Genkan's live-media guard (installer/genkan/rust/src)

<a id="inx-14"></a>

### INX-14 — (test harness) Genkan helper preflight stage boundary
- **Where:** `nix/genkan-preflight-test.nix:71` (`configuration_refused`); also `:59-66`, `:101-107`
- **Encoding:** the helper's JSONL output (`"stage"`, `"kind"`) and its human stage messages; the request's disk identity and `confirmation`; the disk's SHA-256 before and after
- **States:** helper `IdentityCheck` → `FilesystemSupport` → (template missing) `Refused`; never `InstallationFiles`, `verify-live-store`, `verify-system`, `progress`, `prepared`; stale identity refused before `FilesystemSupport`
- **Transitions:** good, corrupt and missing store paths all stop at configuration (`:83-99`); stale serial refused earlier (`:102-107`); disk unchanged (`:109`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** Genkan's privileged helper `calamares-nixos-helper` (installer/genkan/rust)

<a id="inx-15"></a>

### INX-15 — Googlebook vboot kernel-partition attempt attributes
- **Where:** `nix/googlebook-iso.nix:35` (`cgpt add -i 2 -P 15 -T 0 -S 1`); also `nix/googlebook-iso-test.nix:85-88`, `:105-108`
- **Encoding:** GPT attributes of the kernel partition (priority, tries, successful) that Depthcharge reads and decrements
- **States:** firmware's `Priority>0, Tries>0` → `Tried` → `Successful` | `Exhausted`; the image starts pinned at `Successful` (P=15, T=0, S=1), so attempts never exhaust (comment `:34`); `Disabled(P=0)` is rejected
- **Transitions:** set at build (`googlebook-iso.nix:35`); verified with `load_kernel_test` (`:37-42`); disabled and locked-mode rejections (`googlebook-iso-test.nix:87-88`, `:105-108`)
- **Hazards:** none observed; the outputs are disabled (`flake.nix:118-119`)
- **Reify:** Low — one command pins a firmware machine

<a id="inx-16"></a>

### INX-16 — (test harness) Hyprlauncher open/cancel/reopen with a resident daemon
- **Where:** `nix/hyprlauncher-software-test.nix:138` (`open_launcher`), `:143` (`cancel`); also `:85` (`layer`), `:165`, `:177-193`, `:198-206`
- **Encoding:** the `hyprlauncher` layer surface (at most one, `:89`), the daemon PID kept across cycles, `Reconfigured a swapchain` log lines, coredumps
- **States:** `Closed(daemon resident)` → `Open(layer)` → `Searching` → `Launched` | `Cancelled` → `Closed`; per scale `Rescaled` → swapchain reconfigured; negative control `Crashed(SEGV)`
- **Transitions:** Super+R opens (`:139-141`); Esc closes, daemon still alive (`:144-146`); same PID after reopen (`:187`); swapchain sizes logged (`:192-193`); unpatched crash (`:200-203`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** INX-27 (the patch under test)

<a id="inx-17"></a>

### INX-17 — Installed flake lock: as-installed pin, then following `nightly`
- **Where:** `nix/installed-lock.nix:26` (`locked`); also `nix/installed-lock.nix:70-79` (`sekka` node, `original.ref = "nightly"`), `templates/flake.nix.in:6-12`, `modules/installer.nix:233`, `:236-238`, `flake.nix:847-855`
- **Encoding:** `/etc/nixos/flake.lock` on an installed system (written from `etc."calamares-nixos/flake.lock"`): the `sekka` node locked to the media's commit (`type = github`, `rev`) or, for dirty images, its store path (`type = path`); the root's inputs all `follows` Sekka's (`:59-69`); Sekka's own nodes rebased under it (`:43-54`)
- **States:** `AsInstalled(rev | path)` → `Updated(nightly head)`; every followed input moves with Sekka in one step
- **Transitions:** generated at evaluation (`installed-lock.nix:56-81`); written to the media (`modules/installer.nix:233`); moved by an update (Yukimi / `sekka-update`, outside this slice); template/lock contract checked (`flake.nix:847-855`)
- **Hazards:** a dirty image's `type = path` lock is only resolvable while that store path exists on the installed system (the media carry it, `modules/installer.nix:236-238`); after a GC before the first update, rebuilding from the lock cannot refetch it; adding a Sekka input changes the installed root only through the fixed list at `:17-24`
- **Reify:** Low — persisted, two-phase, one writer
- **Related:** `apps/yukimi` (`sekka-update`), Genkan's flake writer (installer/genkan/rust)

<a id="inx-18"></a>

### INX-18 — Copy-to-RAM of the live media in the initrd
- **Where:** `nix/iso-copytoram.nix:42` (`stage`); also `nix/iso-copytoram.nix:22-40`, `:44-50`, `:68-84`, `modules/installer.nix:80-86`, `nix/iso-copytoram-test.nix:27-56`, `nix/iso-copytoram-initrd-test.nix:15-17`, `nix/iso-copytoram-initrd-test.py:107-108`
- **Encoding:** mounts at `/sysroot/iso`, `$stage` (private bind), `$stage/ram` (tmpfs sized to the media), `$stage/media` (rollback bind); the EXIT trap; the unit's exit status (upstream unit is `requiredBy = initrd.target`)
- **States:** `Probe` → `SizeUnknown` | `InsufficientRAM` (continue from media) → `Staged` → `RAMAllocated` → `Copied` → `RollbackHeld` → `MediaUnmounted` → `Switched(RAM at /sysroot/iso)` | `Restored(media back)` | `Stranded(media kept at $stage/media, exit 1)`
- **Transitions:** size probe (`:22-26`); RAM check with 4 GiB reserve (`:27-40`); staging (`:42-56`); tmpfs (`:57-60`); copy (`:61-64`); rollback bind (`:68`); unmount media (`:69-72`); move RAM in (`:73`); restore or strand (`:74-82`); success (`:84`); trap unmounts leftovers (`:44-50`)
- **Hazards:** `:68` is the one unguarded step: under `writeShellApplication`'s errexit a failure exits 1, failing the initrd even though the media are intact, unlike every other branch; a failure inside `cleanup` stops the trap and leaves `/run/sekka-copytoram.*`
- **Reify:** High — boot-critical, six mount phases in one linear script, one unguarded step
- **Suspected bug:** the media size is the size of the block device holding `/sysroot/iso` (`:23`), not of the ISO. If that device is a whole USB stick (a raw-written hybrid ISO found by label on the disk rather than its partition), `bytes` is the stick's capacity, so a 64 GB stick on a 32 GB machine is reported "insufficient RAM" and copytoram is silently skipped though the image would fit (confidence: low)
- **Reproduction:** Partly confirmed (unit): [test and output](reproductions.md#inx-18)

<a id="inx-19"></a>

### INX-19 — (test harness) Kakehashi helper write through pkexec with progress and cancel files
- **Where:** `nix/kakehashi-vm-test.nix:74` (`write_as_alice`); also `:51-53`, `:63` (`write`), `:92-118`
- **Encoding:** a private job directory with `plan.json`, the JSONL `progress` file (`stage`, `message`, `touched`) and the `progress.cancel` marker; the hot-plugged stick image with a stale tail
- **States:** `Planned` → `Refused(system disk | identity changed, touched=false)` | `Stopped(before writing, touched=false)` | `Writing` → `Verifying` → `Finished`
- **Transitions:** hot-plug (`:51-53`); refusals (`:93-99`); cancel marker before start (`:78-79`, `:102-106`); full write and read-back, tail cleared (`:109-118`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** Kakehashi window ⇄ privileged helper (apps/kakehashi/helper, apps/kakehashi/core)

<a id="inx-20"></a>

### INX-20 — (test harness) Keep-awake inhibitor lifecycle
- **Where:** `nix/keep-awake-test.nix:38` (`ready`); also `:15-29`, `:40-69`
- **Encoding:** `tatami awake status` (`protected`), `systemd-inhibit --list`, the `tatami-keep-awake` user unit, `tatami indicator idle` class; a stubbed `tatami-hypridle` and a `false` suspend
- **States:** `Off` → `On(lock held)` → `HolderKilled` → `ReAcquired`; `LogindRestarted` → `ReAcquired`; `Rebooted` → `Restored(saved intent)` → `Off`; `Masked` → `Warning(on fails)`
- **Transitions:** on (`:41-46`); kill holder (`:48-49`); restart logind (`:50-51`); reboot (`:53-57`); off (`:58-61`); mask (`:63-65`); unmask/on/off (`:66-69`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `desktop/tatami/keep-awake.nix`, `tatami awake` (desktop/tatami)

<a id="inx-21"></a>

### INX-21 — (test harness) Network DNS/IPv6 page driver with settle detector and mirrored UI mode
- **Where:** `nix/network-dns-vm-test.nix:82` (`custom`, `selection` globals); also `:84` (`settle`), `:100` (`combo`), `:126` (`dns`), `:148` (`open_details`), `:161` (`set_metadata`)
- **Encoding:** a harness-side `custom` flag mirroring whether the page shows the Custom row (it moves the selectors), a screenshot counter, NetworkManager's `eth0` state snapshot, and the profile's opaque `user.data` key `org.sekka.tatami.ipv6-dns` (`:250`)
- **States:** settle `Changing` → `Stable(3 s)`; selector `Opened` → `Located(two OCR reads agree)` → `Chosen`; profile DNS `Automatic` | `Cloudflare` | `Google` | `Custom`, IPv6 `Auto` ↔ `Off(DNS suspended in backup)`; backup `Valid` | `Malformed` | `ExternallyChanged` (refused)
- **Transitions:** settle loop (`:91-98`); OCR agreement (`:110-123`); `custom` set by `dns()` (`:129`), reset on reopen (`:157`), forced true after reopen (`:233`, `:252`); NM restart (`:230-231`); corrupt backup (`:250-255`); external change (`:261-265`)
- **Hazards:** `custom` must be kept equal to the app's real mode by hand; a reopen while the profile is Custom (`:157` then `:233`) is only right because the test sets it again, otherwise the clicks land one row off
- **Reify:** Low — fixture
- **Related:** Tatami Network's IPv6 DNS backup in the NM profile (desktop/tatami/network); `network_profile_transactions` tests (`:9-17`, `:280`)

<a id="inx-22"></a>

### INX-22 — (test harness) Wi-Fi join flows over simulated access points
- **Where:** `nix/network-vm-test.nix:96` (`wait_active`); also `:13-32`, `:110-150`
- **Encoding:** NetworkManager's active and saved connection lists; `tatami launch tatami-network --join NAME`; the simulated APs from INX-41
- **States:** `Scanning` → `PasswordAsked` → `Activating` → `Activated`; wrong password `ProfileCreated` → `GaveUp` → `ProfileRemoved` → previous network `Reactivated`; a second start hands over to the open window (`:124-126`)
- **Transitions:** join Home (`:111-119`); hidden network (`:126-132`); wrong password (`:135-146`); no password files left (`:149`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** INX-41; Tatami Network's join flow (desktop/tatami/network)

<a id="inx-23"></a>

### INX-23 — Aquamarine DRM async-commit-event flush during teardown (patched)
- **Where:** `nix/patches/aquamarine-null-connector.patch:16` (`flushAsyncCommitEvents`); also `nix/overlays/aquamarine.nix:5-8`, `nix/aquamarine-test.nix:19-27`
- **Encoding:** per-output `asyncCommitEventPending` flag; the backend's `connectors` list, whose slots can be null while it is torn down
- **States:** output `Idle` → `CommitEventPending` → `Emitted`; connector `Live` → `Null(teardown)`
- **Transitions:** flush skips null connectors (`aquamarine-null-connector.patch:19`)
- **Hazards:** none observed; the negative control must still crash (`nix/aquamarine-test.nix:22-27`)
- **Reify:** Low — upstream backport, one guard

<a id="inx-24"></a>

### INX-24 — Avahi startup SIGHUP window (patched)
- **Where:** `nix/patches/avahi-startup-sighup.patch:11` (`ignore_signal(SIGHUP)`); also `nix/overlays/avahi.nix:4-7`, `modules/avahi-fixes.nix:5`, `nix/avahi-startup-test.nix:9-14`
- **Encoding:** SIGHUP's disposition across daemon startup: default (terminate) → ignored → the real handler that `run_server` installs (outside the patch)
- **States:** `Starting` → `SIGHUPIgnored` → `PIDPublished` → `HandlerInstalled` → `InitialStateRead` → `Running`
- **Transitions:** ignore before `daemon_pid_file_create` (`nix/patches/avahi-startup-sighup.patch:11-13`); resolvconf may signal as soon as the PID file exists (comment `:8-10`)
- **Hazards:** a SIGHUP in the window is dropped, not deferred; correctness relies on `run_server` installing its handler before reading resolver/host/service state, which the patch's comment asserts but its context does not show
- **Reify:** Low — one line closes a race
- **Related:** `tests/check_avahi_startup.py` (gdb-synchronized at PID publication)

<a id="inx-25"></a>

### INX-25 — Hyprland cursor-free capture image and its per-frame zoom snapshot (patched)
- **Where:** `nix/patches/hyprland-capture-cursor-0.57.patch:116` (`CMonitorResources.m_mirrorFBOffset`, `.m_mirrorFBScale`, with upstream `m_mirrorFBValid`, `m_mirrorFBNeedsFullRefresh`); also `nix/patches/hyprland-capture-cursor.patch:79-82`, `nix/overlays/hyprland.nix:19-29`
- **Encoding:** the mirror framebuffer's validity flags plus the zoom transform snapshotted when it was saved; `CTexPassElement.excludeFromMirror` (`nix/patches/hyprland-capture-cursor-0.57.patch:308`); the GL draw-buffer mask toggled per excluded element; the frame's `ZOOMFACTOR`
- **States:** mirror FB `Invalid` → `Valid(offset, scale)` → `Stale(damage)` → `Valid`; per draw `BothAttachments` ↔ `DisplayOnly(cursor)`; per capture `CursorExcluded` | `CursorOverlaid(projected through snapshot)`
- **Transitions:** copy FB forced whenever a copy is needed (`nix/patches/hyprland-capture-cursor-0.57.patch:63-71`); snapshot on save (`:81-89`, `:190-195`); projection (`:91-93`, `:164-166`); software-cursor screencopy skip removed (`:149-152`); mirrored displays draw the source cursor with DPMS opacity (`:206-222`); zoom sampled after `frameTick` (`:239-244`); clears initialise both attachments (`:253-271`); draw buffers narrowed and restored (`:279-296`); older variant without the zoom/orientation parts (`hyprland-capture-cursor.patch:46-59`, `:108-128`, `:166-182`, `:221-238`)
- **Hazards:** `invalidateMirrorFB` does not reset the snapshot, and `projectMirrorBox` never checks validity, so a capture overlaid before the first save after a zoom change projects through the previous transform (defaults 0/1.0 before any save); the draw-buffer restore assumes the mask was `{0,1}` before; the upstream `zoomLock` at `nix/patches/hyprland-capture-cursor-0.57.patch:242` is a function-static shared by every monitor
- **Reify:** Medium — validity, snapshot and GL mask are separate fields kept consistent by call order
- **Related:** INX-08; grim and wf-recorder clients; screenshare frames (INX-26)

<a id="inx-26"></a>

### INX-26 — Hyprland managed screencopy session lifecycle (patched)
- **Where:** `nix/patches/hyprland-screenshare-sessions-0.57.patch:173` (`SManagedSession`: `m_session`, `stoppedListener`, `m_idleTimer`, `m_lastUsed`, `m_clientDestroy`); also `nix/patches/hyprland-screenshare-sessions.patch:216-229`, `nix/overlays/hyprland.nix:19-29`, `nix/screenshare-sessions-vm-test.nix:154-206`
- **Encoding:** membership in `m_managedSessions`, the session's `m_stopped`, `m_lastUsed`, an event-loop timer, a late client-destroy listener, pending frames in `m_pendingFrames`, and the region identity `m_requestedBox` (logical) vs derived `m_captureBox` (pixels)
- **States:** `Created` → `InUse` (each lookup bumps `m_lastUsed`) ↔ `Idle` → `Stopped` (idle ≥ 5 s with no pending frame | client destroyed | evicted as LRU at 32 per client | monitor gone) → `Erased`; invalid-but-representable `m_stopped && still in m_managedSessions` (lookups and timer skip it)
- **Transitions:** lookup skips stopped, region by requested box (`nix/patches/hyprland-screenshare-sessions-0.57.patch:28`, `:36`); LRU eviction preferring idle (`:44-64`); timer armed at creation (`:80-85`); bump (`:88`); timer re-arms or stops (`:93-106`); client destroy stops (`:112-116`); ctor/dtor register and cancel (`:127-143`); erase by address (`:73-78`); pixel box re-derived on constraint changes (`:236-237`); older variant (`hyprland-screenshare-sessions.patch:69-158`, `:169-185`, `:266-271`)
- **Hazards:** each `stop()` synchronously destroys the object being called through (comments `:63`, `:105`, `:115`); safe only because nothing touches it afterwards; when all 32 sessions are busy the eviction stops one with a frame pending; a client polling slower than every 5 s gets a fresh session each time
- **Reify:** High — five fields, four end conditions in four handlers, self-destroying callbacks
- **Related:** INX-39; clients: portal region shares (xdg-desktop-portal-hyprland), `tests/screencopy-sessions.c`

<a id="inx-27"></a>

### INX-27 — Hyprtoolkit window buffer allocation fallback ladder (patched)
- **Where:** `nix/patches/hyprtoolkit-buffer-allocation.patch:44` (`swapchain`); also `:103` (`CWaylandPlatform::primaryAllocator`), `:146` (`m_primaryAllocator`), `nix/overlays/hyprtoolkit.nix:4-7`, `nix/hyprlauncher-software-test.nix:12-15`
- **Encoding:** `m_waylandState.swapchain` (its current format persists), `m_waylandState.wlBuffers`, the lazily created and cached primary-node allocator
- **States:** `ExistingReconfigured` | `NewMainPreferred` → `NewMainARGB` → `NewPrimaryPreferred` → `NewPrimaryARGB` → `Failed(buffers cleared)`; allocator `Unopened` → `Cached` | `Unavailable(render node only)`
- **Transitions:** no formats → clear (`:38-42`); reconfigure existing with its format (`:44-50`); ladder (`:53-74`); fail clears buffers, keeps the old swapchain (`:77-81`); per-buffer check (`:83-91`); commit both (`:92-93`); primary node opened once and cached (`:103-127`)
- **Hazards:** after a failure `wlBuffers` is empty while the previous `swapchain` stays, so the next resize retries it; a failed primary open is retried on every resize; once ARGB is chosen it stays chosen for that window
- **Reify:** Medium — retry ladder whose outcome lives in two fields and a cache
- **Related:** INX-16; Aquamarine allocator

<a id="inx-28"></a>

### INX-28 — IBus Wayland input-method commit serial (patched)
- **Where:** `nix/patches/ibus-wayland-serial.patch:17` (`priv->im_serial = 0` in the IMv2 branch); also `:23`, `:15` (removed unconditional reset), `nix/ibus-wayland-serial-test.nix:15-21`
- **Encoding:** `priv->im_serial`, counted from the compositor's `done` events and sent with each commit
- **States:** per input-method binding `Bound(serial 0)` → `Counting(n)`; before the patch, any new global (e.g. a new `wl_output`) reset it to 0 mid-session
- **Transitions:** reset only when an IM manager or IMv1 interface is bound (`:17`, `:23`)
- **Hazards:** none observed after the patch
- **Reify:** Low — single counter, two reset sites
- **Related:** the compositor's input-method-v2 commit-serial check (Aizome compositor, desktop/aizome); `tests/check-ibus-wayland-serial`

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
- **Reproduction:** Partly confirmed (unit): [test and output](reproductions.md#inx-29)

<a id="inx-30"></a>

### INX-30 — PipeWire X11 bell module display/source teardown (patched)
- **Where:** `nix/patches/pipewire-x11-bell-disconnect.patch:10` (`impl->source == NULL` guard); also `:20-24`, `nix/overlays/pipewire.nix:4-6`, `modules/pipewire-fixes.nix:3-4`, `nix/pipewire-x11-disconnect-test.nix:7-10`
- **Encoding:** `impl->source` (the X connection's loop source; NULL once destroyed) and `impl->display`
- **States:** `Connected(source)` → `IOError(source destroyed)` → `Destroyed(display closed)`; Xlib may report the I/O error again during `XCloseDisplay`
- **Transitions:** handler returns if already handled (`nix/patches/pipewire-x11-bell-disconnect.patch:10-11`); handler destroys the source (`nix/patches/pipewire-x11-bell-disconnect.patch:15`); module destroy destroys and nulls it (`nix/patches/pipewire-x11-bell-disconnect.patch:21-24`), then closes the display (`nix/patches/pipewire-x11-bell-disconnect.patch:26-27`)
- **Hazards:** the guard relies on the I/O-error handler also setting `impl->source = NULL` after `nix/patches/pipewire-x11-bell-disconnect.patch:15`, which is outside the patch context
- **Reify:** Low — one nullable field
- **Related:** `tests/check_pipewire_x11_disconnect.py`

<a id="inx-31"></a>

### INX-31 — (test harness) Uta MPRIS test waits for bus-name ownership before its caching proxy
- **Where:** `nix/patches/uta-mpris-readiness.patch:8` (`until`); also `:11-18`, `nix/uta.nix:91`
- **Encoding:** a 5 s deadline now shared by both waits (moved from `:26`), `name_has_owner` polling
- **States:** `NameUnowned` → `NameOwned` → `ProxyCreated` → `Playing`
- **Transitions:** poll every 20 ms (`:12-18`); then the existing `PlaybackStatus` loop
- **Hazards:** zbus permanently caches an initial `GetAll` failure (comment `:9-10`); any client that builds a caching proxy before a player owns its name has the same problem
- **Reify:** Low — test-only patch
- **Related:** Uta's MPRIS server (github bitemyapp/uta); Aizome's MPRIS client (desktop/aizome aizome-sys)

<a id="inx-32"></a>

### INX-32 — XApp portal backend bound to the graphical session (patched unit)
- **Where:** `nix/patches/xapp-session-lifecycle.patch:7` (`PartOf=graphical-session.target`); also `:8`, `:11`, `nix/overlays/xapp.nix:7-9`, `modules/portal-fixes.nix:5`
- **Encoding:** systemd unit ordering and stop propagation for a `Type=dbus` user service
- **States:** `Inactive` → `Active(with session)` → `Stopped(with graphical-session.target)`
- **Transitions:** stop or restart propagated from the target (`nix/patches/xapp-session-lifecycle.patch:7`); ordered after it (`nix/patches/xapp-session-lifecycle.patch:8`)
- **Hazards:** `PartOf`/`After` do not stop D-Bus activation; a request arriving after the session target stopped (with a lingering user manager) can start the backend again without a display; `Requisite=` would prevent it
- **Reify:** Low — two unit lines

<a id="inx-33"></a>

### INX-33 — XApp Settings theme-change notification coalesced to idle (patched)
- **Where:** `nix/patches/xapp-settings-theme-idle.patch:12` (`static guint theme_changed_source`); also `:15-23`, `:36-40`, `nix/overlays/xapp.nix:6`, `nix/xapp-settings-test.nix:20-24`
- **Encoding:** a process-global idle source ID holding a reference to the settings object
- **States:** `Idle(0)` → `Scheduled(id)` → `Emitted` → `Idle`
- **Transitions:** notify schedules once (`nix/patches/xapp-settings-theme-idle.patch:36-40`); idle clears and emits accent colour (`:19-22`)
- **Hazards:** the source is never removed if the object is finalised first (it holds a ref, so it only delays finalisation); one static slot per process
- **Reify:** Low — classic coalescing flag
- **Related:** `tests/check_xapp_settings.py`

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#inx-34)

<a id="inx-35"></a>

### INX-35 — (test harness) Runner cancellation-order fixture
- **Where:** `nix/runner-protocol-test.nix:9` (`marker`); also `:17-31`, `:81-90`
- **Encoding:** a marker file `/tmp/runner-cancel-<label>` written by the slow build and polled (100 × 0.1 s) by the failing one; `max-jobs = 2`
- **States:** `SlowStarted(marker)` → `FailBuildFails(exit 23)` → `SlowCancelled`; `SlowNeverStarted(exit 24)` is the fixture's own failure
- **Transitions:** marker write (`:18`); poll and fail (`:22-30`); harness checks the log and waits for the sleeper to die (`:86-89`); upstream client shows the masked status, the pinned one reports the failure (`:92-100`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** INX-36 (`tools`, `nixPackage`)

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#inx-36)

<a id="inx-37"></a>

### INX-37 — AI-harness and background-job cgroup placement
- **Where:** `nix/scheduling/agent-workloads.py:142` (`retry`); also `nix/scheduling/agent-workloads.py:67` (`scopes`), `:92` (`attach`), `nix/scheduling/background.nix:6-12`
- **Encoding:** each process's cgroup path, transient scopes named `app-sekka-{agent|work}-<root pid>-<start time>.scope` (`nix/scheduling/agent-workloads.py:87`) with `PartOf` the first member's unit and `CollectMode=inactive-or-failed`, the per-scope `retry` deadlines, the poll period; for `sekka-background` the marker `/etc/sekka/background-scheduling`, the user manager's socket and current slice
- **States:** per process `InLauncherUnit` → `InWorkScope(app-background.slice)`; nested harness `InWorkScope` → `InAgentScope(app.slice)`; per scope `Unattached` → `Attached` | `RetryAfter(5 s)`; daemon `Busy(0.25 s poll)` ↔ `Quiet(2 s)`
- **Transitions:** classify roots (`nix/scheduling/agent-workloads.py:67-89`); skip processes already placed or delegated (`nix/scheduling/agent-workloads.py:97-110`); start-time recheck (`nix/scheduling/agent-workloads.py:112-115`); `StartTransientUnit`, else `AttachProcessesToUnit` (`nix/scheduling/agent-workloads.py:128-135`); timeout is a failure (`nix/scheduling/agent-workloads.py:137-138`); backoff and pruning (`nix/scheduling/agent-workloads.py:148-157`); poll period (`nix/scheduling/agent-workloads.py:158`); explicit `sekka-background` scope unless already in the slice (`background.nix:6-12`)
- **Hazards:** new children run in the interactive unit until the next poll (up to 0.25 s); a nested harness's scope is `PartOf` the work scope it came from, so stopping that job stops the nested harness too; the agent-name set is duplicated in C++ (INX-38, `ananicy-processes.patch:141-156`) and must be kept in sync by hand
- **Reify:** Medium — polling reconciler; state is the cgroup tree plus a retry map
- **Related:** INX-38, INX-43; `installer/genkan/rust/system/tuning.nix` (runs it as `sekka-agent-workloads`), `tests/test_agent_workloads.py`; Aizome compositor's spawn (desktop/aizome/crates/aizome-comp-core/src/session.rs)

<a id="inx-38"></a>

### INX-38 — ananicy-cpp process-event classification (patched)
- **Where:** `nix/scheduling/ananicy-processes.patch:20` (name re-read at dequeue); also `:48` (`for_each_thread`), `:112-117`, `:123-133`, `:177-182`, `nix/scheduling/ananicy.nix:2-3`
- **Encoding:** the process queue fed by BPF and netlink exec/fork events; the removed `prev_pid` dedupe (`nix/scheduling/ananicy-processes.patch:112-115`, `nix/scheduling/ananicy-processes.patch:126-133`); the BPF handler's remaining `prev_pid`/`prev_ts` (`nix/scheduling/ananicy-processes.patch:182`); per-thread scheduler and I/O class
- **States:** per event `Queued(pid)` → `Named(at dequeue: cmdline, then exe)` → `RuleMatched` | `NoRule` → `Applied(every existing thread)`
- **Transitions:** every exec queued, including a wrapper's second exec under the same PID (`nix/scheduling/ananicy-processes.patch:115`, `nix/scheduling/ananicy-processes.patch:129-131`); fork events classify the child (`nix/scheduling/ananicy-processes.patch:181`); name re-read, `exe` fallback (`nix/scheduling/ananicy-processes.patch:20-35`); interpreted harness and Chromium names (`nix/scheduling/ananicy-processes.patch:141-167`); per-thread `ioprio_set` and `sched_setscheduler`, ignoring ESRCH (`nix/scheduling/ananicy-processes.patch:59-63`, `nix/scheduling/ananicy-processes.patch:78-80`, `nix/scheduling/ananicy-processes.patch:97-99`)
- **Hazards:** with fork and thread creation now queued, a build that spawns many processes floods the queue; a PID reused before dequeue is classified by the new process's name; harness identification duplicates INX-37's list
- **Reify:** Medium — queue semantics changed in three places (BPF, netlink, worker)
- **Related:** INX-37, INX-43; `installer/genkan/rust/system/tuning.nix` (rules)

<a id="inx-39"></a>

### INX-39 — (test harness) Screenshare session counting driver
- **Where:** `nix/screenshare-sessions-vm-test.nix:81` (`class Compositor`); also `:123-140`, `:142-145`
- **Encoding:** counts of `Created`/`Destroyed screenshare session` lines in the Hyprland log, `screencastv2>>1,` events captured by `socat` from `.socket2.sock`, a unit counter for capture clients, `--hold` to keep a client connected
- **States:** per capture `Started` → `Captured` → `Holding` → `Exited(success)`; per scenario expected `(created, destroyed)` deltas
- **Transitions:** start and wait for "captured" or exit (`:132-140`); finish (`:143-145`); scenarios (`:154-206`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** INX-26; `tests/screencopy-sessions.c`

<a id="inx-40"></a>

### INX-40 — (test harness) One-shot screenshot notification action owner
- **Where:** `nix/screenshot-notifications-vm-test.nix:92` (`start`); also `:10-15`, `:104-153`
- **Encoding:** the `tatami screenshot-notify` user unit that owns the notification's actions, Mako's visible list and history, Mako's config (`history=1` for the negative control)
- **States:** `Shown(owner alive)` → `Invoked(Satty)` → `OwnerExited`; or `Expired(10 s)` → `OwnerExited` → `InHistory` (original) | `NotInHistory` (fixed); restoring the original yields a notification whose Edit action has no owner
- **Transitions:** fresh invoke and owner exit (`:110-123`); expiry and owner exit (`:126-130`); history check (`:131-134`); restore (`:135-150`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `tatami screenshot-notify` (desktop/tatami), `desktop/tatami/config/mako/config`

<a id="inx-41"></a>

### INX-41 — (test harness) Simulated Wi-Fi access-point bring-up
- **Where:** `nix/simulated-wifi.nix:82` (`start_access_points`); also `:51-71`
- **Encoding:** two hwsim radios, network namespace `ap`, hostapd's multi-BSS config, dnsmasq with per-BSS ranges, NM's `scanRandMacAddress = false`
- **States:** `RadiosLoaded` → `APRadioInNamespace` → `HostapdRunning` → `LastBSSUp` → `Addressed` → `DHCPServing`
- **Transitions:** wait for phy (`:83`); netns, move, hostapd (`:85-89`); wait for last BSS device (`:91`); addresses and dnsmasq (`:92-98`)
- **Hazards:** with random scan MACs, NM takes `wlan1` down mid-scan and wpa_supplicant then treats the next results as scan-only (comment `:65-69`); the fixture avoids rather than exercises it (INX-45 exercises it)
- **Reify:** Low — fixture
- **Related:** INX-22, INX-45, `nix/screenshots.nix:36-53`

<a id="inx-42"></a>

### INX-42 — (test harness) Tatami screen recording lifecycle with marker-file fault injection
- **Where:** `nix/tatami-recording-test.nix:86` (`start`); also `:9-16`, `:92` (`clean`), `:119-180`
- **Encoding:** wrappers that fail when `/tmp/cancel-capture` or `/tmp/fail-recorder` exist, the `record-test` user unit (`KillMode=process`), wf-recorder processes, `tatami_recording_` PipeWire nodes, default sink/source snapshot
- **States:** `Idle` → `Recording(wf-recorder + mixer graph)` → `Stopped` | `Cancelled(picker)` | `RecorderFailed` | `SupervisorKilled` | `MixerKilled` | `MicGone` → `Clean(no recorder, no graph, defaults unchanged)`; `MicUnavailable` refuses to start
- **Transitions:** markers (`:133`, `:137`); kill main only (`:147`); unload microphone (`:151`, `:169`); kill `pw-cli` (`:162`); every path ends in `clean()` (`:93-94`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `tatami screenrecord` (desktop/tatami)

<a id="inx-43"></a>

### INX-43 — (test harness) Tuning process-tree stand-ins with nice-value handshake
- **Where:** `nix/tuning-test.nix:10` (`waitNice`); also `:15-26`, `:136` (`all_threads`), `:152-223`
- **Encoding:** each stand-in waits, unbounded, until ananicy has set its own nice value before starting the next (`:11-13`); signals USR1/USR2 to the probe add children or threads; ananicy stopped, an unpatched daemon run as control, then restarted
- **States:** `TerminalAt-4` → `ShellAt0` → `CompilerAt3`; Nix workers `Normal` → `Idle(all threads)`; late thread `Classified`; agent `Interactive` with job `InWorkScope`; nested harness `InAgentScope`
- **Transitions:** tree start (`:111-117`); stop/control/start (`:153-169`); late threads (`:172-174`); retitled child (`:177-179`); harness and jobs (`:193-211`); nested (`:214-223`)
- **Hazards:** `waitNice` has no timeout; if ananicy never matches, the stand-ins block and only the outer 120 s waits fail
- **Reify:** Low — fixture
- **Related:** INX-37, INX-38, `tests/scheduling-probe.c`, `nix/scheduling/aizome-launch-probe.nix`

<a id="inx-44"></a>

### INX-44 — (test harness) Yukimi update completion: activation survives its client, switch vs boot
- **Where:** `nix/update-completion-test.nix:86` (`system.activationScripts.yukimiTest`); also `:71-83`, `:104-106`, `:115-169`
- **Encoding:** marker `/run/test-slow-activation` → `/run/test-activation-started`; Yukimi's `/var/lib/yukimi/update.json` (`state`), `operations/*/result.json`, `progress.log`; `sekka-update --status` (`action`, `reasons`); profile links `system-10x-link`
- **States:** `None` → `Activating(client killed)` → `Activated` → `None`; session-list change `Switched(live)`; session-entry change `StagedForBoot` → `action=reboot` → after reboot `None`
- **Transitions:** target-side refusal before stopping the display manager (`:111-113`); slow activation and client kill (`:116-122`); switch (`:130-137`); boot (`:139-151`); reboot (`:163-169`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** Yukimi helper and update state (apps/yukimi)

<a id="inx-45"></a>

### INX-45 — (test harness) wpa_supplicant dropped-scan reproduction
- **Where:** `nix/wpa-supplicant-scan-test.nix:79` (subtest); also `:20-21`, `:32`, `:67-77`, `:91-96`
- **Encoding:** wpa_supplicant paused with SIGSTOP so a D-Bus `Scan` and a link-down queue behind it; its radio-work log lines; the interface `State`
- **States:** `ScanQueued` → `ScanRemovedBeforeStart` → `SelectNetwork` → `Completed`; unfixed: `ScanOnlyResults` → joins nothing
- **Transitions:** STOP, scan, link down, CONT (`:81-86`); removal logged (`:88`); join (`:92-95`); no scan-only line (`:96`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** `installer/genkan/rust/system/wpa-supplicant/clear-scan-res-handler.patch`; INX-41

<a id="inx-46"></a>

### INX-46 — (test harness) Yukimi under X11: nested compositor and single-instance lifecycle
- **Where:** `nix/yukimi-x11-test.nix:63` (nested `weston` wait); also `:71-88`
- **Encoding:** the native Yukimi process, a nested `weston` per instance, runtime files `/run/user/1000/yukimi-x11.*`
- **States:** `Launched(Yukimi + weston)` → `RelaunchHandedOver(same PIDs)` → `Quit(both gone, runtime files removed)` → `Reopened` → `OuterClose(Alt+F4 ends both)`
- **Transitions:** launch from the Run dialog (`:47-54`); relaunch keeps PIDs (`:72-75`); Ctrl+Q (`:77-79`); reopen (`:80-81`); Alt+F4 (`:85-88`)
- **Hazards:** none observed
- **Reify:** Low — fixture
- **Related:** Yukimi's X11 launcher (apps/yukimi)

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `flake.nix` | 918 | none |  | output wiring and eval-time assertions |
| `modules/avahi-fixes.nix` | 6 | machines | [INX-24](#inx-24) | selects patched avahi daemon |
| `modules/googlebook.nix` | 31 | none |  | live-media config overrides |
| `modules/hyprland-fixes.nix` | 7 | none |  | overlay import |
| `modules/installer.nix` | 302 | machines | [INX-18](#inx-18), [INX-17](#inx-17) | copytoram unit override and installed lock; rest config |
| `modules/pipewire-fixes.nix` | 5 | machines | [INX-30](#inx-30) | selects patched pipewire daemon |
| `modules/portal-fixes.nix` | 6 | machines | [INX-32](#inx-32), [INX-33](#inx-33) | overlay for patched XApp portal |
| `nix/activity-vm-test.nix` | 321 | machines | [INX-01](#inx-01), [INX-02](#inx-02) | stand-in power meter; pause/resume polls |
| `nix/ai-packages.nix` | 11 | none |  | package set override |
| `nix/aizome-greeter-desktops-vm-test.nix` | 143 | machines | [INX-03](#inx-03), [INX-01](#inx-01) | greeter login/logout cycle |
| `nix/aizome-ibus-policy-test.nix` | 98 | none |  | eval assertions |
| `nix/aizome-ibus-test.nix` | 90 | machines | [INX-04](#inx-04) | AccountsService seed and readiness delay; driver in tests/ |
| `nix/aizome-login-vm-test.nix` | 185 | machines | [INX-05](#inx-05) | login to logout environment lifecycle |
| `nix/aizome-site.nix` | 93 | none |  | static site build |
| `nix/applications.nix` | 95 | none |  | catalog evaluation |
| `nix/aquamarine-test.nix` | 42 | machines | [INX-23](#inx-23) | negative-control check of the patch |
| `nix/avahi-packages-test.nix` | 42 | none |  | eval assertions |
| `nix/avahi-startup-test.nix` | 15 | machines | [INX-24](#inx-24) | wrapper for tests/check_avahi_startup.py |
| `nix/background-validation-vm-test.nix` | 177 | machines | [INX-06](#inx-06), [INX-01](#inx-01) | wallpaper menu panels and state invariant |
| `nix/boot-splash-choice-test.nix` | 92 | none |  | eval assertions |
| `nix/boot-splash-test.nix` | 146 | machines | [INX-07](#inx-07) | splash hold gate and Plymouth modes |
| `nix/calamares.nix` | 18 | none |  | package import |
| `nix/capture-cursor-vm-test.nix` | 353 | machines | [INX-08](#inx-08), [INX-25](#inx-25), [INX-01](#inx-01) | compositor driver for cursor patch |
| `nix/chatgpt/copy-plugin.cjs` | 51 | machines | [INX-09](#inx-09) | per-destination promise chain |
| `nix/chatgpt/package.nix` | 9 | none |  | package override |
| `nix/chatgpt/patch-asar.py` | 65 | none |  | pure byte patch, single match within budget |
| `nix/chatgpt/test.nix` | 5 | none |  | tests |
| `nix/ci-cache-test.nix` | 134 | machines | [INX-10](#inx-10) | CI GC-root retention driver |
| `nix/components.nix` | 76 | none |  | component wiring |
| `nix/display-recovery-vm-test.nix` | 287 | machines | [INX-11](#inx-11), [INX-01](#inx-01) | fake lid FIFO and dock/undock driver |
| `nix/displays-cursor-vm-test.nix` | 103 | machines | [INX-01](#inx-01) | newest-signature readiness and polls |
| `nix/displays-fit-vm-test.nix` | 146 | machines | [INX-01](#inx-01) | scale cycling polls |
| `nix/fish-default-test.nix` | 128 | none |  | eval assertions |
| `nix/fish-vm-test.nix` | 110 | none |  | tests: tty login shell checks |
| `nix/fixture.nix` | 19 | none |  | test binary build |
| `nix/genkan-keyboard-test.nix` | 86 | machines | [INX-12](#inx-12) | VM and debug preview for the .py driver |
| `nix/genkan-keyboard-test.py` | 298 | machines | [INX-12](#inx-12), [INX-01](#inx-01) | keyboard-step driver over inspection files |
| `nix/genkan-live-media-test.nix` | 158 | machines | [INX-13](#inx-13) | private-namespace mount fixture |
| `nix/genkan-preflight-test.nix` | 113 | machines | [INX-14](#inx-14) | helper preflight stage boundary |
| `nix/googlebook-iso-test.nix` | 109 | machines | [INX-15](#inx-15) | vboot acceptance and rejection checks |
| `nix/googlebook-iso.nix` | 47 | machines | [INX-15](#inx-15) | pins vboot priority/tries/successful |
| `nix/googlebook-kernel.nix` | 20 | none |  | kernel image build |
| `nix/googlebook-vboot-loader.nix` | 33 | none |  | host test utility build |
| `nix/hyprland-packages-test.nix` | 64 | none |  | eval assertions |
| `nix/hyprlauncher-software-test.nix` | 208 | machines | [INX-16](#inx-16), [INX-27](#inx-27), [INX-01](#inx-01) | launcher open/cancel/reopen driver |
| `nix/ibus-wayland-serial-test.nix` | 23 | machines | [INX-28](#inx-28) | before/after serial harness wrapper |
| `nix/installed-lock.nix` | 81 | machines | [INX-17](#inx-17) | installed flake lock pin then nightly |
| `nix/iso-copytoram-initrd-test.nix` | 18 | machines | [INX-18](#inx-18) | runs helper inside real initrd |
| `nix/iso-copytoram-initrd-test.py` | 113 | machines | [INX-18](#inx-18) | cpio parse; unavailable-media branch |
| `nix/iso-copytoram-test.nix` | 58 | machines | [INX-18](#inx-18) | drives the three copy outcomes |
| `nix/iso-copytoram.nix` | 86 | machines | [INX-18](#inx-18) | initrd copy-to-RAM mount phases |
| `nix/iso-desktop-references-test.nix` | 44 | none |  | eval and inventory comparison |
| `nix/iso-integrity-test.nix` | 77 | none |  | tests: squashfs gate rejections |
| `nix/kakehashi-vm-test.nix` | 120 | machines | [INX-19](#inx-19) | pkexec helper write with progress/cancel |
| `nix/kakehashi.nix` | 92 | none |  | package and shell build |
| `nix/keep-awake-test.nix` | 71 | machines | [INX-20](#inx-20) | inhibitor lifecycle driver |
| `nix/libsecret-dh-padding.nix` | 17 | none |  | version-gated patch selection |
| `nix/libsecret-dh-test.nix` | 49 | none |  | tests: control and fixed probes |
| `nix/live-diagnostics.nix` | 23 | none |  | tool build |
| `nix/mpv-software-test.nix` | 92 | none |  | tests: playback and option checks |
| `nix/network-dns-vm-test.nix` | 284 | machines | [INX-21](#inx-21), [INX-01](#inx-01) | DNS/IPv6 driver, settle detector |
| `nix/network-fit-vm-test.nix` | 191 | machines | [INX-01](#inx-01) | scale and OCR polls |
| `nix/network-vm-test.nix` | 152 | machines | [INX-22](#inx-22), [INX-41](#inx-41) | Wi-Fi join flows |
| `nix/overlays/aquamarine.nix` | 9 | machines | [INX-23](#inx-23) | adds patch |
| `nix/overlays/avahi.nix` | 8 | machines | [INX-24](#inx-24) | adds patch |
| `nix/overlays/default.nix` | 8 | none |  | overlay list |
| `nix/overlays/hyprland.nix` | 31 | machines | [INX-25](#inx-25), [INX-26](#inx-26) | version-gated patch variants |
| `nix/overlays/hyprtoolkit.nix` | 8 | machines | [INX-27](#inx-27) | adds patch |
| `nix/overlays/pipewire.nix` | 7 | machines | [INX-30](#inx-30) | adds patch |
| `nix/overlays/xapp.nix` | 12 | machines | [INX-32](#inx-32), [INX-33](#inx-33) | adds patches |
| `nix/patches/aquamarine-null-connector.patch` | 25 | machines | [INX-23](#inx-23) | null connector in async commit flush |
| `nix/patches/avahi-startup-sighup.patch` | 14 | machines | [INX-24](#inx-24) | SIGHUP ignored before PID publication |
| `nix/patches/hyprland-capture-cursor-0.57.patch` | 311 | machines | [INX-25](#inx-25) | cursor-free mirror FB and zoom snapshot |
| `nix/patches/hyprland-capture-cursor.patch` | 252 | machines | [INX-25](#inx-25) | pre-0.57 variant |
| `nix/patches/hyprland-screenshare-sessions-0.57.patch` | 240 | machines | [INX-26](#inx-26) | managed session idle/LRU/client end |
| `nix/patches/hyprland-screenshare-sessions.patch` | 273 | machines | [INX-26](#inx-26) | pre-0.57 variant |
| `nix/patches/hyprtoolkit-buffer-allocation.patch` | 149 | machines | [INX-27](#inx-27) | allocation fallback ladder |
| `nix/patches/ibus-wayland-forward-keycode.patch` | 44 | none |  | stateless IMv2 keysym-to-keycode routing |
| `nix/patches/ibus-wayland-serial.patch` | 26 | machines | [INX-28](#inx-28) | im_serial reset only on IM bind |
| `nix/patches/ibus-wayland-system-keymap-dependencies.patch` | 223 | machines | [INX-29](#inx-29) | NULL seat guards and cleanup |
| `nix/patches/ibus-wayland-system-keymap.patch` | 595 | machines | [INX-29](#inx-29) | user/system keymaps and mode |
| `nix/patches/libsecret-gnutls-dh-padding.patch` | 78 | none |  | pure DH secret padding |
| `nix/patches/pipewire-x11-bell-disconnect.patch` | 27 | machines | [INX-30](#inx-30) | idempotent X I/O error teardown |
| `nix/patches/plasma-login-config-order.patch` | 46 | none |  | config source precedence only |
| `nix/patches/uta-mpris-readiness.patch` | 29 | machines | [INX-31](#inx-31) | test waits for bus name |
| `nix/patches/xapp-session-lifecycle.patch` | 11 | machines | [INX-32](#inx-32) | PartOf/After graphical-session.target |
| `nix/patches/xapp-settings-theme-idle.patch` | 43 | machines | [INX-33](#inx-33) | idle-coalesced theme notify |
| `nix/pipewire-packages-test.nix` | 46 | none |  | eval assertions |
| `nix/pipewire-x11-disconnect-test.nix` | 11 | machines | [INX-30](#inx-30) | wrapper for tests/check_pipewire_x11_disconnect.py |
| `nix/plasma-login-config-test.nix` | 63 | none |  | tests: config loader |
| `nix/plasma-login-session-test.nix` | 48 | none |  | tests: driver in tests/plasma-login-session.py |
| `nix/pr-ci-test.nix` | 25 | none |  | tests: unit tests and linters |
| `nix/prebuilt-binaries-vm-test.nix` | 123 | none |  | tests: loader checks |
| `nix/publish-nightly.nix` | 15 | none |  | wrapper of scripts/publish-nightly.py |
| `nix/push-cache.nix` | 89 | machines | [INX-34](#inx-34) | staged cache publication |
| `nix/reference-systems.nix` | 64 | none |  | eval of prebuilt systems |
| `nix/runner-protocol-test.nix` | 106 | machines | [INX-35](#inx-35) | marker-file cancellation fixture |
| `nix/runner.nix` | 241 | machines | [INX-36](#inx-36) | supervisor loop and job admission |
| `nix/scheduling/agent-workloads.py` | 162 | machines | [INX-37](#inx-37) | polling cgroup placement reconciler |
| `nix/scheduling/aizome-launch-probe.nix` | 45 | none |  | test probe build |
| `nix/scheduling/ananicy-processes.patch` | 184 | machines | [INX-38](#inx-38) | event queue and per-thread policy |
| `nix/scheduling/ananicy.nix` | 4 | machines | [INX-38](#inx-38) | adds patch |
| `nix/scheduling/background.nix` | 13 | machines | [INX-37](#inx-37) | marker-gated background scope |
| `nix/scheduling/policy-test.nix` | 40 | none |  | eval assertions and unit tests |
| `nix/screenshare-sessions-vm-test.nix` | 211 | machines | [INX-39](#inx-39), [INX-26](#inx-26), [INX-01](#inx-01) | session counting driver |
| `nix/screenshot-notifications-vm-test.nix` | 158 | machines | [INX-40](#inx-40), [INX-01](#inx-01) | action owner lifecycle |
| `nix/screenshots-aizome.nix` | 231 | machines | [INX-01](#inx-01) | capture script waits |
| `nix/screenshots.nix` | 327 | machines | [INX-01](#inx-01), [INX-41](#inx-41) | capture script waits; simulated Wi-Fi |
| `nix/session-environment-vm-test.nix` | 71 | machines | [INX-04](#inx-04) | seed and activation fixture; driver in tests/ |
| `nix/simulated-wifi.nix` | 100 | machines | [INX-41](#inx-41) | access-point bring-up fixture |
| `nix/site.nix` | 160 | none |  | static site build |
| `nix/sound-test.nix` | 60 | none |  | tests: wrapper for tests/test_sound.py |
| `nix/sukumo-test.nix` | 93 | none |  | tests: cargo test runner |
| `nix/tatami-recording-test.nix` | 183 | machines | [INX-42](#inx-42), [INX-01](#inx-01) | recording lifecycle fault injection |
| `nix/tools.nix` | 22 | none |  | package build |
| `nix/tuning-test.nix` | 242 | machines | [INX-43](#inx-43) | process-tree stand-ins |
| `nix/update-completion-test.nix` | 172 | machines | [INX-44](#inx-44) | Yukimi update completion driver |
| `nix/uta-installation-test.nix` | 44 | none |  | eval assertions |
| `nix/uta.nix` | 98 | machines | [INX-31](#inx-31) | applies test readiness patch; build |
| `nix/verified-iso.nix` | 11 | none |  | adds verification to ISO build |
| `nix/verify-squashfs.nix` | 30 | none |  | pure verification script |
| `nix/window-pop-vm-test.nix` | 125 | machines | [INX-01](#inx-01) | pop/return toggle polls |
| `nix/wpa-supplicant-scan-test.nix` | 98 | machines | [INX-45](#inx-45), [INX-41](#inx-41) | dropped-scan reproduction |
| `nix/xapp-packages-test.nix` | 60 | none |  | eval assertions |
| `nix/xapp-settings-test.nix` | 25 | machines | [INX-33](#inx-33) | wrapper for tests/check_xapp_settings.py |
| `nix/yukimi-discover.nix` | 87 | none |  | tests: driver in tests/yukimi-tatami.py |
| `nix/yukimi-x11-test.nix` | 91 | machines | [INX-46](#inx-46) | nested compositor lifecycle |
| `templates/flake.nix.in` | 35 | machines | [INX-17](#inx-17) | installed inputs follow sekka nightly |
