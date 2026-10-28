> **Historical pre-monorepo evidence.** These results apply to the named predecessor revisions, not a released Sekka image. See the [migration record](migration.md) for the migration checks.

# Fast installation, tuning and Hyprland handoff

Updated October 9, 2026, America/Chicago. The previous SanDisk/filesystem
handoff is archived at [archive/agent-handoff-sandisk-2026-10-04.md](archive/agent-handoff-sandisk-2026-10-04.md).

## User requirements for this round

1. Make the installer more attractive.
2. Port portable CachyOS performance and reliability ideas (no recompiled
   packages or custom kernel). Instead of zram, create a **swap partition
   matched to RAM with zswap**.
3. Make installation faster: under one minute from clicking Install, after all
   inputs are supplied, on real hardware. Precache in RAM without exceeding it.
4. Add **vanilla Hyprland** and an **Omarchy-flavored Hyprland**.
5. **All new code in Rust.** Approved exceptions: declarative Nix (modules,
   flake wiring, tests); declarative config and CSS files; Hyprland
   `hyprland.lua` limited to declarative `hl.*` calls.
6. Build, test and run VMs on `ssh wx-workstation`.
7. Capture screenshots of potential final candidates for the user.

## October 9: merged into stable and the default branches; Yukimi for any NixOS

Pinned: Calamares `52251cce5fc8657809b9d36d922173e92aec3361`, Tatami
`723c3ab5c029517dd7cd15f6234e295730c607c0`, Yukimi
`9f42aaad4ce43457b057b9a135ac130e53cf9457`, all on `stable`. No image has
been built from them yet.

- **Branches.** Each repository's work since October 5 is one squash commit
  on `stable` (Calamares adds one fix commit after it). Default branches:
  `codex/graphical-installer` here fast-forwards to `stable`; Calamares's
  `calamares` (which had stayed with upstream's GitHub mirror) merges
  `stable` with a merge commit; Yukimi's and Tatami's `main` equal `stable`.
  Yukimi had no `stable` branch before, so installed systems' `yukimi` input
  (which follows it) couldn't update until now.
- **Yukimi** works on any NixOS system (flake or channels, any layout), and
  updates without root downloading again; see its README. Its module turns
  on the setuid `pkexec` wrapper: recent NixOS has it only when something
  asks (Xfce does), so on Plasma-, GNOME- or Hyprland-only systems every
  change through Yukimi failed with "pkexec must be setuid root". The
  installer's catalog reaches Yukimi through `programs.yukimi.catalogs`
  (Calamares `rust/src/applications.nix`), registered only where Yukimi's
  module is imported. Each source on Yukimi's Updates page can follow another
  branch, so pushing to `main` of Tatami, Yukimi or Calamares and pressing
  Update installs it, with no `nix` commands.
- **Checked:** `nix flake check --no-build`; the desktop matrix (31 cases)
  and the application matrix (`all`, `none`) against the pinned inputs; in
  QEMU, Yukimi on this image's flake system and on a channel-based system
  (installing and removing for everyone, removing hand-written entries,
  updating, following a branch).

## October 8 round: GitHub step and README screenshots

Calamares was pinned at `0d36f90424b27ff4922aa504671f61403ca82710` (the
GitHub step and the login-screen fixes below).

- **GitHub step** (calamares `rust/src/github.rs`, `ui.rs`, `ui/pages.rs`,
  `config.rs`, `plan.rs`), after Account and before Desktop, as in Ubuntu's
  server installer: a GitHub username's public keys
  (`https://github.com/USER.keys`, shown with `ssh-keygen -l` fingerprints)
  become `users.users.USER.openssh.authorizedKeys.keys`; a switch enables
  `services.openssh` (key-only when keys are authorized, passwords otherwise),
  kept out of the applications list; Git's name and email go to
  `programs.git.config.user`. The helper parses all of it again. Tested: unit
  tests, the fixture's `application-configurations` "all" case evaluated
  through `tests/application-matrix.nix` (new assertions), and a GUI install
  in local QEMU that looked up `bitemyapp` (14 keys) and then logged in over
  SSH with one of them; password login was refused. The VM fixture takes the
  GitHub options by default (`CALAMARES_TEST_GITHUB=0` turns them off) and
  `verify` checks the key, sshd and `git config --system`.
- **README screenshots** (`docs/images/tour/`): a development image
  (`RESPIN_CALAMARES_SRC`, `respin.cacheApplications = false`) installed in
  QEMU on the laptop with all five desktops, driven over QMP (screendump,
  `send-key`, usb-tablet clicks); apps were started over SSH with
  `systemd-run --user` and placed with a KWin script in Plasma. The login
  screen shot was retaken under GDM after the fix below.
- **Xfce had no keyboard or mouse under Plasma Login Manager.** The nixpkgs
  module sets only `X11.ServerPath` to the bare `xorg-server/bin/X`, without
  NixOS's `xserverArgs` (`-config xserver.conf -xkbdir …`), so Xorg never
  finds `xf86-input-libinput` (`Failed to load module "libinput"`). The
  upstream module has the same bug. `desktops.nix` now points `ServerPath` at a
  `plasmalogin-xserver` wrapper that passes those arguments, less the fixed
  display, `-terminate` and `-logfile /dev/null` (the Xorg log is in
  `~/.local/share/xorg`).
- **GNOME warned at login that screen locking needs GDM**, and had no lock
  screen: gnome-shell 50 enables its screen shield only when GDM's
  `org.gnome.DisplayManager` D-Bus service is present. GDM is now the login
  screen whenever GNOME is installed (before: only when GNOME was the only
  desktop). GDM's greeter lists every session in the system `XDG_DATA_DIRS`,
  so each unchosen one gets a `Hidden=true` entry in `/etc/X11/sessions`,
  which GDM reads first. The session-environment reset moved to
  `gdm-password` with it. `tests/desktop-matrix.nix` asserts all of this.
- Both checked in QEMU on the README image, rebuilt with the patched module
  (`--override-input calamares`): with all five desktops, GDM listed exactly
  GNOME, Hyprland (uwsm), Plasma (Wayland), Tatami and Xfce; GNOME showed no
  warning, Super+L locked (`LockedHint=yes`) and unlocked; Xfce took keys and
  clicks; Tatami and Plasma started. With GNOME removed, Plasma Login Manager
  came back, Xfce took keys and clicks (Xorg log: libinput for every device),
  X exited on logout and Plasma started after it.

## October 6 round: Acer reinstall feedback

The Plasma Login Manager / Tatami image installed and booted on the Acer
Predator Helios Neo 14; Tatami and Plasma (Wayland) worked. Reports and fixes:

| Report | Cause | Fix |
|---|---|---|
| The live installer, left unattended, showed a black screen that no key or VT switch woke; a hard power-off was needed | Plasma's laptop defaults in the live session: lock after 5 min, screen off after 10, **sleep after 15 on AC** (5 on battery). PowerDevil skips auto-sleep in VMs, so VM tests never hit it. The live ISO also lacked NVIDIA power management, so video memory was not preserved across sleep | graphical `modules/installer.nix`: `/etc/xdg/powerdevilrc` and `kscreenlockerrc` turn off idle dim, screen-off, lock and sleep in the live session; `hardware.nvidia.powerManagement.enable` as on installed systems. calamares `rust/src/power.rs`: the helper holds a logind `sleep:idle` block lock from before the first disk write to completion (Plasma mirrors it); `install.json` records `kept_awake`. The `live-profiles` check asserts the settings |
| Tatami had two network UIs: nm-applet in the tray and nmtui in a terminal | nm-applet is installed by Xfce's NixOS module and autostarted in Tatami; nmtui was Tatami's own | calamares `tatami/tool/src/network.rs`: a Walker Wi-Fi list (`tatami network`) for the bar, Super+Ctrl+W and Setup › Wi-Fi; nm-applet hidden from Tatami (`NotShowIn` + `Tatami;`). Tested against mac80211_hwsim radios with hostapd (WPA2 and open APs): new network with password, wrong password (profile removed), saved network, disconnect, radio off/on |
| Clean-up pass toward Omarchy Quattro | Audit by a subagent against Omarchy 4.0.4 | Session keeps `XDG_CURRENT_DESKTOP=Hyprland:Tatami` (with `misc.disable_xdg_env_checks`); printer applet hidden; power key opens System (logind `handle-power-key` lock while the session runs; before, it powered off); `tatami bluetooth` unblocks the radio and reports a missing adapter; Elephant built with the 8 providers used; Mako `Type=dbus`; Setup › Hyprland writes a user file that `dofile()`s the defaults; the browser key accepts only real browsers |
| (found in the audit) Links and Office files could open in the ChatGPT app in every desktop | No default applications; chatgpt.desktop claims http(s) and Office types | calamares `rust/src/applications.nix`: the first selected browser and LibreOffice are the defaults in `/etc/xdg/mimeapps.list`; the application matrix asserts them |

Later on October 6, from the user's questions about first-boot time and
interference between desktops (calamares `498778f`, `8b9336b`, `7830d5b`):

- **Desktop lineup:** the installer offers Plasma, GNOME, vanilla Hyprland,
  Tatami and Xfce (the one X11 desktop). MATE, LXQt and Cinnamon are gone
  from the installer; `desktops.nix` still accepts them for existing systems.
  A probe of every desktop in an all-desktop install showed why: their
  session managers export their environment, each brings a notification
  daemon, polkit agent, applets and a settings daemon, and Xfce's `xfconfd`
  and GNOME's IBus ran in the others.
- **Login screen:** its `XDG_DATA_DIRS` points at a `linkFarm` with one
  session per chosen desktop, hiding Plasma (X11), Hyprland without uwsm and
  Cinnamon's variants.
- **Keyring:** GNOME Keyring everywhere; Plasma's kwalletd stores into it
  (`[KSecretD] Enabled=false`, a KWallet 6.30 option), and kwallet-pam is
  off. Existing KWallet contents are not migrated.
- **First boot:** the user manager of a lingering user is ready only once its
  `default.target` is, which includes rootless Docker; nixpkgs' unit also ran
  for system users (`ConditionUser=!root`), so the login screen's user
  started and retried it. Now `!@system`. Whether Docker's own start should
  stop gating the user manager awaits the user's first-boot timings from the
  Acer. VM first boot: user managers 0.1–0.7 s, NetworkManager 69 ms,
  wait-online not run at boot.
- **Regression fixed:** `Type=dbus` + `BusName` on `tatami-mako` (from the
  audit) made systemd refuse to load it after another desktop's notification
  service had claimed the name in that boot.

Not done from the audit (low value or needs hardware): Elephant's Bluetooth
picker instead of bluetui (pairing needs an agent), launcher clutter from other
desktops' settings apps, KDE's push-notification service running in Tatami,
hiding hardware-specific menu entries.

## October 5 round: hardware feedback fixes

The user installed the final image on a ThinkPad X1 Carbon Gen 14 with every
desktop except Cinnamon plus many applications, then a later image on an Acer
Predator Helios Neo 14. Their reports and the fixes:

| Report | Cause | Fix |
|---|---|---|
| Ctrl+Alt+F*n* does nothing | ananicy-cpp's `cgroup_realtime_workaround` (forced on by nixpkgs) moved realtime compositors to the root cgroup; polkit then denied logind `Seat.SwitchTo` | calamares `rust/system/tuning.nix`: ananicy cgroup features off |
| Plasma/GNOME failing or slow to start after switching desktops | SDDM's Wayland greeter (enabled by the Plasma module) races kwallet-pam when starting a session ([sddm#1443](https://github.com/sddm/sddm/issues/1443)): 30 s stall, then an inactive session on a black VT. Reproduced once in nine VM logins | `rust/system/desktops.nix`: Plasma Login Manager instead of SDDM (GDM when GNOME is the only desktop). Its Wayland greeter is stopped only after the session has started, so it cannot race the VT switch. An interim switch to SDDM's X11 greeter caused the Acer black screen below |
| Omarchy "not quite as nice as the official one" | — | Conformed to Quattro (v4.0.4) with the Waybar/Walker/Mako stack and the Rust helper, then renamed **Tatami** so it is not mistaken for Omarchy. `omarchy` still works in existing configurations; see `docs/hyprland.md` |
| Black screen instead of the login screen on an Acer Predator Helios Neo 14 (RTX 4070 Laptop, MUX set to the discrete GPU) | nouveau on the earlier image; then SDDM's X11 greeter with PRIME offload configured while the panel was wired to the NVIDIA GPU (`NVIDIA(G0): Setting mode "NULL"`) | NVIDIA's driver (615, open modules) on the media and installed systems; calamares `src/graphics.rs` picks the primary GPU from the connected displays (internal panel first) and enables offload only when they are on the integrated GPU; the Wayland login screen lights whichever GPU drives them |
| Timezone should be a map with a sensible default | — | calamares `src/zonemap.rs`, `src/ui/zonemap.rs`, `src/timezone.rs`: clickable Natural Earth map, CLDR names, detection live → geoip.kde.org/ipinfo.io → hardware clock → New York. ipapi.co was rate-limiting (HTTP 429) |

Also fixed: Xfce's polkit-gnome agent and GNOME's IBus autostart no longer
start in other desktops, and Tatami's dark GTK defaults are a dconf profile
selected only in its session (`DCONF_PROFILE` through uwsm's `env-tatami`).
LXQt asked for a window manager at the first login because the other desktops
install KWin, Marco and Xfwm4; `/etc/xdg/lxqt/session.conf` now names Openbox.
Its file manager then wrote Home/Trash/Computer/Network launchers into
`~/Desktop`, which Plasma showed with warning badges; with another icon desktop
installed, `/etc/xdg/pcmanfm-qt/lxqt/settings.conf` is the packaged file
without them. Both were found in the Plasma Login Manager rotation, as was
the worst one: desktops export their environment into the user's systemd
manager, which outlives each session (the `ksecretd` that pam_kwallet starts
keeps old sessions "closing"). After an X11 desktop, Plasma's services got
`QT_QPA_PLATFORM=xcb` and LXQt's Qt theme, and plasmashell ran on X11 without
its panel. calamares `rust/system/session-env` (Rust) records the manager's
environment as it starts and restores it before each login, from the
`plasmalogin` PAM session stack (`pam_exec`, after `pam_systemd`).

Verified on candidate `4c236fb0…` (impure build of both working trees, before
the SDDM change):

- All-inclusive install passed the harness: Plasma, GNOME, Xfce, MATE, LXQt,
  Hyprland and Omarchy with all applications, 108 s from click to completion
  for a 34.2 GB closure. That is VM disk-bound: 104 s copying.
- GUI-driven install through the new Location page: detection by
  geoip.kde.org, map clicks (London → BST, back to Chicago), and the installed
  `time.timeZone = "America/Chicago"`. The harness check failed only because
  my click on Btrfs missed, so it installed the default ext4 against
  `--filesystem btrfs`.
- VT switching to tty3 and back works in Plasma, GNOME and Omarchy.
- The SDDM change was applied with `nixos-rebuild boot` in the installed
  all-inclusive VM and exercised with the login rotation above.

Screenshots: `.work/candidate-screens/` and `.work/timezone-screens/`.
These changes are committed and pushed: calamares `1e14e4d`, pinned here. The
final image built from them is described under "Candidate images".

## Where things are

Both repositories use the branch `claude/fast-install-hyprland`, created from
`codex/filesystem-install-reliability` and pushed to GitHub.

| Area | Location |
|---|---|
| Two-phase install (prepare → confirm → execute), timing record | calamares `rust/src/install.rs`, `helper.rs`, `session.rs` |
| RAM-sized swap, exact layout, chosen identities | calamares `rust/src/memory.rs`, `disk.rs` (`Layout`), `filesystem.rs` (`Identities`) |
| Page-cache warming | calamares `rust/src/precache.rs` |
| Nix progress parsing, dry-run sizes | calamares `rust/src/nixlog.rs` |
| Installed-system modules (desktops, tuning, Hyprland, Tatami and its Rust tool, NVIDIA) | calamares `rust/system/`, exported as `nixosModules` by calamares `flake.nix` |
| GPU detection (NVIDIA, integrated GPU, which one drives the displays) | calamares `rust/src/graphics.rs`; probe with `cargo run --example detect-graphics` |
| Reference system for prebuilt media closures | calamares `rust/reference.nix` (sync test in `config.rs`) |
| Genkan's window, on Aizome's kit (the libadwaita GUI it replaced was calamares `rust/src/ui.rs`) | `installer/genkan/ui/` |
| Media integration (reference systems, closure lists, zram, options) | graphical `modules/installer.nix`, `nix/reference-systems.nix`, `nix/calamares.nix` |
| VM harness changes (local fixture, sizing, USB throttle, timing) | graphical `rust/src/vm.rs`, `rust/src/builder.rs` |
| User-facing docs | graphical `README.md`, `docs/fast-install.md`, `docs/tuning.md`, `docs/hyprland.md` |

`nix/calamares.nix` accepts `RESPIN_CALAMARES_SRC=/abs/checkout` with
`--impure` for development builds. Pure builds use the pinned revision in
`nix/calamares-source.json` and the `calamares` flake input: Calamares
`0cbb39e2c42ab4b0a05690a7727dd69dea051545`. `respin-tools pin-calamares REV`
updates both, and the `calamares-pin` check keeps them in agreement. The input
tracks `stable`, as installed systems do.

Branches, in both repositories: `claude/fast-install-hyprland` is this work,
pushed and not merged. `stable` is what installed systems follow (calamares)
and what release images are built from (graphical). Move it only to verified
revisions. The default branches (`calamares`, `codex/graphical-installer`) are
for reviewed, merged work.

Installed systems import the installer's modules from the `calamares` input
(`nixosModules.default`) instead of copies in `/etc/nixos`. Verified:
`nix flake update calamares` then a rebuild on a new-layout VM, and the
documented conversion of an old-layout VM. Both produced the identical system.

## Candidate images

**Final image, with Yukimi** (application cache, the five offered desktops
prebuilt with Yukimi, and NVIDIA's driver for both kernels): a pure build of
this repository at its pinned installer `41b2e17a`, Tatami `b925d7a` and
Yukimi `af29b80` ([bitemyapp/yukimi](https://github.com/bitemyapp/yukimi)),
without overrides.
`nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`,
11,839,500,288 bytes, SHA-256
`fe5efe48d39f1f053be752d653d6c9264c2c938a4867a671d2fe8a12479b4513`.
It is on the workstation at `~/work/dng/pure-iso14/iso/` and on the Mac at
`artifacts/yukimi-oct7/`. `.work/flash-sandisk-yukimi.command` writes it to
the SanDisk.
- Everything in the wallpapers image below, plus Yukimi on every desktop:
  installed systems get a ninth flake input, `yukimi`, tracking Yukimi's
  `stable` branch (which `calamares` follows), calamares enables
  `programs.yukimi` on every system with a desktop, the reference systems
  prebuild it (the install downloaded 0 bytes), and its polkit action names
  its own helper. `respin-tools pin-yukimi REVISION` pins it.
- Verified: `nix flake check` 9/9; 31 desktop and 34 application
  configurations with Yukimi asserted on every desktop; all-inclusive VM
  install, 100.5 s click-to-complete; 8 of 8 logins through Plasma Login
  Manager. On the installed system: Yukimi from the Plasma menu and in
  Tatami; Check for updates per input (calamares and tatami "would go back",
  yukimi "couldn't check"); removing Zed edited `calamares.applications`
  and rebuilt in 60 s; returning to generation 1 brought Zed and the
  configuration back in 3 s.
- Yukimi's repository has no `stable` branch yet (creating it was declined
  as a production deploy, so it is left to the user). Until it exists,
  `nix flake update` of the `yukimi` input fails; Yukimi's update check marks
  only that input "couldn't check".

**Previous image** (wallpapers, weather): pinned installer `0d10e385` and
Tatami `b925d7a`, SHA-256
`6ea87f16c16d62f59bb10c4e61e5910a5cee09417d2247b3f544178d76b2fd7a`, on the
Mac at `artifacts/fast-install-oct7/` (`.work/flash-sandisk-fast-install.command`).
- Everything in the split-Tatami image below, plus Tatami's 23 wallpapers in
  every desktop from one copy of each image (Plasma's wallpaper settings,
  GNOME's Appearance, Xfce's backdrop folders, Tatami's Style › Background),
  each desktop with its own default: the ocean wave in Plasma (a Breeze
  Global Theme with that wallpaper, which the login screen also shows), the
  foggy forest in GNOME, the mountains at dusk in Xfce, Da Nang at night in
  Tatami. Tatami also gained the weather beside the clock and the Omarchy
  alignment work (menus, keybindings, top-right panels with DNS, screen
  recording, OCR and QR capture, a dark authentication prompt).
- `nix flake check` passed; 31 desktop and 34 application configurations
  passed, with each desktop's wallpaper default asserted; all-inclusive VM
  install 100.2 s from click to completion; 8 of 8 logins through Plasma
  Login Manager, each desktop showing its default wallpaper.
- Evidence: `docs/test-results/fast-install-*.json` and
  `docs/test-results/application-configurations.json`.

`33720cfd…` (installer `f9e1698`, Tatami `e26e503` from its own repository,
GNOME Keyring everywhere, 99.6 s install, 8 of 8 logins) is in
`artifacts/fast-install-oct6b/`.
`aca2aa73…` (installer `7b6d2a2`, installed on the Acer on October 6) is in
`artifacts/fast-install-oct6/`.
`eb4e2d08…` (installer `70fa021`, the image installed on the Acer on October
5) is in `artifacts/fast-install-tatami/`.
`7753ef75…` (installer `0cbb39e`, SDDM's X11 login screen, PRIME offload
whenever an Intel GPU was present) gave the Acer a black login screen; it is
in `artifacts/fast-install-nvidia/`.

Earlier images: `4f7621f0…` (installer `1e14e4d`, old layout, nouveau) in
`artifacts/fast-install-oct5/`, and `69d989e3…` (installer `cd054b9`, the one
installed on the X1 Carbon) in `artifacts/fast-install/`.

`bce81f55…`, which carried the GUI screenshots and the other VM runs, differs
from this image only by a storage-test ordering fix (test code). That fix
changes the source hash, and therefore the image hash.

The previous candidate `201a7c40…` (11,912,892,416 bytes) passed the first
GUI-driven install. The final candidate adds: zram hidden from the disk list,
the `/var/log/calamares-nixos/install.json` timing record, and `nix copy` in
place of the removed direct copier.

## Verified

- Calamares library: 57 unit tests, 4 GUI tests and 6 compile-fail doctests;
  Clippy with `-D warnings` and rustfmt are clean. Omarchy tool: 19 tests,
  clean Clippy. Harness: 16 tests.
- Configuration matrices: all 191 supported desktop combinations and all 34
  application selections evaluate against pinned NixOS. They assert the
  swap/zswap/resume/tuning wiring and that the default session exists.
- VM installs that booted the installed disk and passed the fixture's checks:
  Plasma (UEFI/NVMe/ext4), Xfce with swap and tuning off (BIOS/VirtIO/Btrfs),
  Omarchy + Hyprland (UEFI/VirtIO/Btrfs), and a **GUI-driven** install on the
  release candidate.
  - The GUI install used UEFI/NVMe/Btrfs with Plasma, vanilla Hyprland and
    Omarchy, Omarchy as the default.
  - It reported "Installed in 57 seconds" for an 11.4 GiB system, with 8 vCPUs
    and ISO reads throttled to 200 MB/s.
  - Login through SDDM into Omarchy worked automatically. Manual logins worked
    into vanilla Hyprland and Plasma.
  - Screenshots are in `docs/images/candidate-*.png`, and the full set is in
    `.work/final-screens/` (local, ignored).
- The final candidate passed a second GUI-driven install: UEFI/NVMe/XFS, Plasma
  plus vanilla Hyprland (default), and Install clicked before preparation
  finished. The installer waited, then installed. The timing record showed
  storage 1.1 s, copy 40.6 s for 9.8 GiB and bootloader 0.9 s. The harness
  logged into vanilla Hyprland. Screenshots are in
  `.work/final-screens/candidate-1e1253a4/`.
- Copier A/B on a quiet host with otherwise identical images: `nix copy` 30.5 s
  click-to-complete against 48.0 s for the direct copier, which was removed.
- Hyprland/Omarchy component behavior in standalone VMs: see `docs/hyprland.md`.

## Not verified / open

- **Physical hardware**, including the ThinkPad X1 Carbon Gen 14 and the
  one-minute target on real NVMe and USB. VM disk emulation limits copy
  throughput to about 250–450 MB/s.
- Hibernation and resume, brightness, Bluetooth, battery and hyprlauncher on
  real hardware.
- The deferred path is VM-verified with binary-cached applications on a 6 GiB
  VM. With applications that must be compiled from source (the AI apps
  without the media cache), a source build failed in the VM; release media
  carry those packages.
- Measuring the hardware result: after installation, boot the installed system
  and read `/var/log/calamares-nixos/install.json` for stage timings. This is
  present only in images built after that change.
- Upstream problems seen in the rotation, not fixed here:
  - Plasma Login Manager 6.7.5 does not restart a greeter that crashed: the
    screen stays black while `plasmalogin.service` is active
    (`src/daemon/Greeter.cpp`, `onHelperFinished` ignores
    `HELPER_OTHER_ERROR`). A fix is a small C++ patch, which needs the user's
    approval under the Rust-only rule.
  - A Plasma session ended with `loginctl terminate-session` leaves its user
    units (kwin, plasmashell) running, so GNOME then refuses to start ("A
    graphical session is already running!"). Normal logouts are clean.
  - DrKonqi's coredump launcher aborts when no display is reachable, and each
    abort produces a coredump that starts another launcher (about 670 in five
    minutes after a forced termination).
  - pam_kwallet's `ksecretd` keeps sessions "closing" in GNOME and the uwsm
    sessions, so the user manager outlives logouts.
- Test harness: `/tmp/plm-rotation.sh` on the workstation drives the rotation.
  Its readiness probes match wrapped process names (`^\.?plasmashell`), and the
  Plasma probe waits for queued `plasma-*` jobs, because a logout sent during
  the splash screen half-tears the session down.

## Workstation notes

- Nix is at `/nix/var/nix/profiles/default/bin`. The user is not a trusted Nix
  user and has no direct `/dev/kvm` access. Run QEMU in Docker with
  `--device /dev/kvm --group-add 993 -v /nix:/nix:ro`; the env file is
  `~/work/dng/vm.env`.
- Working copies: `~/work/dng/{calamares,determinate-nixos-graphical}`, rsynced
  from the Mac. `~/work/dng/dev-iso/flake.nix` is a scratch flake overriding
  `respin.referenceDesktops` and `respin.cacheApplications`.
- `/home/callen` is itself a Git repository. Use `path:` for scratch flakes.
- Never use `path:` for the graphical repository once VM artifacts exist: it
  copies the ignored `.work/` (tens of GB of VM disks) into the store. Use
  `git+file://` and `git add -A` in the workstation copy so new files are
  included.
- Delete a scratch flake's `flake.lock` before rebuilding. A stale lock keeps
  an old snapshot of a `path:` input.
- Nix inside Alpine containers needs a `HOME` owned by the container user.
- Matching `pgrep -f`/`pkill -f` patterns also match the invoking shell's own
  command line; prefer PIDs.
