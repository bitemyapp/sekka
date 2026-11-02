# Aizome, GNOME and KDE Plasma in a small VM

The same Sekka system three times, differing only in the desktop, booted in
a deliberately small virtual machine: 2 vCPUs, 4 GiB of RAM, and a virtual
GPU without 3D, so every desktop draws on the CPU, as desktops do in most
VMs and cloud desktops. A fourth VM with no desktop at all gives the
operating system's own share. Each was booted three times, the four
alternating, and measured from QEMU's start to the login screen, through
the login, at idle, while starting its apps, and for input-to-screen
latency with a probe client, at idle and under load.

Everything here is component evidence from disposable VMs on one machine
([README.md](README.md)), not physical hardware: no photon was timed, and
on a real GPU the numbers would be different, for all three desktops.

## Summary (4 GiB, median of three boots)

<!-- summary:begin -->
| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Boot: QEMU start → login screen settled | **6.4 s** | 7.6 s | 8.6 s | 5.1 s (console) |
| Login: Enter → desktop settled | **1.2 s** | 2.0 s (overview) | 5.2 s | – |
| Memory in use at the desktop (MemTotal − MemAvailable, 190 s after login) | **591 MiB** | 937 MiB | 1,245 MiB | 370 MiB |
| &nbsp;&nbsp;the desktop's own share (over no desktop) | **+221 MiB** | +567 MiB | +875 MiB | – |
| Idle: guest CPU busy, % of one vCPU | 0.17% | 0.26% | 0.19% | 0.17% |
| Idle: thread wakeups per second | 100 | **43** | 60 | 56 |
| Key → screen, idle, p50 / p99 | **6.5 / 13.6 ms** (0.49 / 1.02 frames) | 10.5 / 17.0 ms (0.78 / 1.28) | 9.6 / 26.0 ms (0.72 / 1.95) | – |
| Key → screen, two busy threads, p50 / p99 | **7.0 / 13.5 ms** (0.53 / 1.02) | 13.0 / 23.6 ms (0.97 / 1.77) | 13.5 / 26.2 ms (1.01 / 1.97) | – |
| Refreshes missed drawing every frame, under load | 0% | 0.84% | 0% | – |
| Apps: launch → window settled, warm | **0.11–0.17 s** (photo viewer 0.32) | 0.47–0.79 s (Loupe 3.19, as its controls hide) | 0.27–0.63 s (Konsole 1.22) | – |
| App memory (PSS), warm | 15–57 MiB (photo viewer 326) | 143–253 MiB (436) | 34–177 MiB (166) | – |
| System closure on disk | 8.49 GB | 8.52 GB | 10.78 GB | 3.55 GB |

Aizome is ahead on boot, login, memory, input latency (it stays at about
half a frame even with the CPUs busy) and app start-up in this
configuration; it loses on idle wakeups (its shell alone wakes about 47 times a
second, more than all of GNOME's guest together), its terminal uses more memory than
Konsole and its photo viewer more than Gwenview, and its system closure is
no smaller than GNOME's. All three are near the no-desktop system in idle
CPU. 2 GiB results are [below](#2-gib).
<!-- summary:end -->

## What was compared

Four installed Sekka systems, built from this repository at `d7ecbac` by
[tests/performance/desktops/default.nix](../../tests/performance/desktops/default.nix)
through this checkout's flake (its lock and first-party source). They
differ only in the desktop the installer would have chosen:

| Subject | Installer choice | Login screen (as Sekka pairs it) |
|---|---|---|
| Aizome | `calamares.desktops = [ "aizome" ]` | Aizome's own, `aizome-greeter` on greetd |
| GNOME | `calamares.desktops = [ "gnome" ]` | GDM |
| Plasma | `calamares.desktops = [ "plasma" ]` | Plasma Login Manager |
| No desktop | none: Sekka's system modules without `desktops.nix` | the text console (multi-user.target) |

Everything else is the installer's reference system
(`installer/genkan/rust/reference.nix`, which the installation media
prebuild): `calamares.tuning.enable` (CachyOS-derived sysctls, ananicy
rules, `preempt=full`, systemd-oomd), zswap in front of a swap device the
size of RAM, the latest kernel, NetworkManager, CUPS, PipeWire, US English
in UTC, and one person, "Demo", who logs in with a password (no automatic
login). The application catalog module is imported with nothing chosen, so
there is no Firefox; each desktop brings what Sekka's desktop module gives
it (for Aizome that includes fish as the terminal's shell, as on every
Aizome install).

The same for all four, and different from an installation: no Plymouth
boot splash (its quiet console settings are kept); stock Nix rather than
Determinate Nix; the Nix store on a read-only erofs image rather than the
root ext4; QEMU boots the kernel directly (no boot loader); and the
benchmark's own small additions (`instrument.nix`: a root shell on a
virtio-serial port, the probe client, uinput), which the disk footprint
figures leave out.

| | Version |
|---|---|
| Linux | 7.3 (`linuxPackages_latest`), systemd 262, Mesa 26.2.6 (llvmpipe) |
| Aizome | Sekka `d7ecbac`: `/nix/store/sg59lw6c0y4xx39kbv0ad56c08k00h11-aizome-0.1.0` (pixman renderer on this GPU) |
| GNOME | GNOME Shell 50.4, Mutter 50.5, GDM 50.2 |
| Plasma | Plasma 6.8.2 (plasma-workspace, KWin, Plasma Login Manager 6.8.2), Frameworks 6.31, Qt 6.12.0 |
| Nixpkgs | `1c89083e451e891152365b799595cf7790a43325` |

([data/desktops/versions.json](data/desktops/versions.json).)

## The VM

QEMU/KVM on the machine described in [README.md](README.md):

- **2 vCPUs** (`-cpu host`), **4 GiB RAM**, a swap disk the size of RAM
  with zswap in front, as the installer sets it up. The QEMU process (every
  thread) was pinned to host CPUs 2, 3, 10 and 11 (two cores and their SMT
  siblings) with `taskset`, the same for every run.
- **virtio-gpu without 3D** at 1920×1080 (`virtio-vga`: QEMU's `-vga
  virtio`, libvirt's default video for Linux guests; no virgl). There is no
  GPU acceleration in the guest: GNOME Shell and KWin composite with Mesa's
  llvmpipe (OpenGL on the CPU), GTK 4 apps render with it too, and Aizome
  uses pixman, its software renderer. The mode is 75 Hz, and the kernel
  paces page flips to it.
- Disks on virtio-blk with `cache=none` (O_DIRECT): the host's page cache
  is bypassed, so every boot reads the store from the NVMe SSD, not from
  host RAM.
- No display window (`-display none`): the screen is read through QMP.
  User-mode networking with no route out.
- The guest's clock starts at 15:00 UTC on the day of the setup:
  Aizome's wallpaper and theme follow day and night, and its reference
  frames are the daytime ones. The first boots (all of round 1, and GNOME's
  and Plasma's second) ran before this was fixed, with the host's clock
  (15:31–18:05 UTC); only Aizome's appearance depends on it, and its one
  such boot was in daytime.

Each measured boot starts from the same "golden" root disk, through a qcow2
overlay thrown away afterwards. The golden disk was made by a first boot
that went through the first login, dismissed the first-run windows and
opened every app once, so a measured boot is a used system's later boot
and login, and an app's first launch after boot (cold) finds the caches a
used system has, but nothing in memory.

## Results

<!-- tables:begin -->
Medians over the boots with the minimum–maximum in brackets; generated by `summarize.py` into [data/desktops/tables.md](data/desktops/tables.md) from [data/desktops/summary.json](data/desktops/summary.json).

### Boot and login (4096 MiB; median, min–max over trials)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Trials | 3 | 3 | 3 | 3 |
| Boot: QEMU start → login screen settled (s) | 6.36 (6.27–6.36) | 7.56 (7.46–7.67) | 8.56 (8.47–8.56) | 5.07 (4.96–5.86) |
| &nbsp;&nbsp;kernel + initrd + userspace (systemd-analyze, s) | 0.20 + 2.73 + 2.07 | 0.20 + 2.74 + 2.23 | 0.20 + 2.75 + 1.97 | 0.20 + 1.84 + 1.88 |
| &nbsp;&nbsp;display manager started (host timeline, s) | 5.24 (5.24–5.31) | 5.48 (5.42–5.57) | 5.21 (5.21–5.24) | – |
| Login: Enter → desktop settled (s) | 1.23 (1.13–1.24) | 2.04 (2.04–2.04) | 5.24 (5.14–5.34) | – |
| &nbsp;&nbsp;Enter → first change on screen (s) | 0.63 (0.63–0.64) | 1.74 (1.04–1.74) | 0.13 (0.13–0.14) | – |
| &nbsp;&nbsp;Enter → PAM session opened (guest, s) | 0.10 (0.09–0.10) | 0.03 (0.03–0.03) | 0.10 (0.09–0.10) | – |

### Memory (4096 MiB guest; MiB, median and range)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| At the login screen: used (MemTotal − MemAvailable) | 531 (516–543) | 691 (688–705) | 1022 (1018–1052) | 375 (366–377) |
| &nbsp;&nbsp;over the no-desktop VM | +156 | +315 | +646 | +0 |
| &nbsp;&nbsp;swap in use | 1 (0–2) | 4 (4–4) | 2 (2–2) | 2 (1–2) |
| Desktop, 60 s after login: used (MemTotal − MemAvailable) | 602 (588–602) | 961 (960–968) | 1249 (1237–1251) | 374 (358–383) |
| &nbsp;&nbsp;over the no-desktop VM | +227 | +587 | +875 | +0 |
| &nbsp;&nbsp;swap in use | 2 (0–2) | 5 (5–6) | 4 (4–5) | 2 (1–2) |
| Desktop, 190 s after login: used (MemTotal − MemAvailable) | 591 (580–594) | 937 (937–948) | 1245 (1241–1245) | 370 (357–370) |
| &nbsp;&nbsp;over the no-desktop VM | +221 | +567 | +875 | +0 |
| &nbsp;&nbsp;swap in use | 2 (0–2) | 5 (5–6) | 4 (4–5) | 2 (1–2) |
| PSS at 190 s: system services | 85 (85–87) | 90 (90–90) | 80 (80–95) | 67 (67–68) |
| PSS at 190 s: login screen (still running) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) |
| PSS at 190 s: the user's session | 276 (276–276) | 698 (696–698) | 1115 (1115–1122) | 0 (0–0) |

### Idle cost over 120 s at the desktop (4096 MiB)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Guest CPU busy (% of one vCPU) | 0.17 (0.17–0.25) | 0.26 (0.19–0.27) | 0.19 (0.17–0.78) | 0.17 (0.08–0.19) |
| Context switches / s | 182 (181–188) | 82 (81–83) | 118 (115–184) | 106 (106–106) |
| Thread wakeups / s (voluntary switches) | 100 (100–101) | 43 (42–43) | 60 (59–80) | 56 (56–56) |
| Interrupts / s | 186 (185–194) | 102 (100–104) | 141 (137–178) | 189 (187–189) |
| Host: QEMU CPU (% of one core) | 0.89 (0.85–0.89) | 0.80 (0.79–0.88) | 0.92 (0.83–1.42) | 0.83 (0.83–0.88) |
| Host: PPT over host idle (W) | 2.37 (1.27–2.53) | 2.49 (0.66–5.97) | 1.71 (-0.58–1.79) | 3.42 (2.47–3.50) |

### Input to screen and frame pacing (4096 MiB)

| | Aizome | GNOME | Plasma |
|---|---|---|---|
| Refresh the compositor reports (ms) | 13.33 | 13.33 | 13.33 |
| Key → presented, idle: p50 / p95 / p99 / max (ms) | 6.5 / 13.0 / 13.6 / 13.7 | 10.5 / 16.4 / 17.0 / 24.0 | 9.6 / 23.2 / 26.0 / 29.4 |
| &nbsp;&nbsp;in frames: p50 / p99 | 0.49 / 1.02 | 0.78 / 1.28 | 0.72 / 1.95 |
| &nbsp;&nbsp;presses (boots) | 660 (3) | 660 (3) | 660 (3) |
| Continuous frames, idle: fps | 75.0 (75.0–75.0) | 74.9 (74.9–75.0) | 75.0 (75.0–75.0) |
| &nbsp;&nbsp;interval p99 / max (ms) | 13.3 / 13.3 | 13.3 / 26.7 | 13.3 / 13.3 |
| &nbsp;&nbsp;refreshes missed (%) | 0.00 (0.00–0.00) | 0.09 (0.04–0.13) | 0.00 (0.00–0.00) |
| Key → presented, load: p50 / p95 / p99 / max (ms) | 7.0 / 13.0 / 13.5 / 15.1 | 13.0 / 20.0 / 23.6 / 55.4 | 13.5 / 24.2 / 26.2 / 36.2 |
| &nbsp;&nbsp;in frames: p50 / p99 | 0.53 / 1.02 | 0.97 / 1.77 | 1.01 / 1.97 |
| &nbsp;&nbsp;presses (boots) | 660 (3) | 660 (3) | 660 (3) |
| Continuous frames, load: fps | 75.0 (75.0–75.0) | 74.4 (74.2–74.5) | 75.0 (74.9–75.0) |
| &nbsp;&nbsp;interval p99 / max (ms) | 13.3 / 13.3 | 13.3 / 40.0 | 13.3 / 13.3 |
| &nbsp;&nbsp;refreshes missed (%) | 0.00 (0.00–0.00) | 0.84 (0.62–1.02) | 0.00 (0.00–0.09) |

### App start-up (4096 MiB): launch → window settled (s); first change on screen (s); PSS (MiB)

| Desktop | App | Settled, cold | Settled, warm | First change, cold | First change, warm | PSS cold | PSS warm | Other new processes, cold (MiB) |
|---|---|---|---|---|---|---|---|---|
| Aizome | Hotaru (terminal) | 0.12 (0.11–0.17) | 0.17 (0.11–0.22) | 0.11 (0.11–0.12) | 0.12 (0.11–0.13) | 57 (57–57) | 57 (57–57) | 0 (0–0) |
| Aizome | Hako (files) | 0.12 (0.12–0.16) | 0.12 (0.11–0.12) | 0.12 (0.11–0.12) | 0.12 (0.11–0.12) | 21 (20–21) | 23 (22–23) | 0 (0–0) |
| Aizome | Settei (settings) | 0.11 (0.11–0.12) | 0.12 (0.11–0.17) | 0.11 (0.11–0.12) | 0.12 (0.11–0.12) | 25 (25–26) | 26 (23–26) | 0 (0–0) |
| Aizome | Sumi (editor) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 16 (16–16) | 16 (16–17) | 0 (0–0) |
| Aizome | Soroban (calculator) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 15 (15–15) | 15 (14–15) | 0 (0–0) |
| Aizome | Shashin (image) | 0.32 (0.32–0.33) | 0.32 (0.28–0.34) | 0.12 (0.11–0.12) | 0.12 (0.11–0.13) | 323 (323–326) | 326 (324–327) | 0 (0–0) |
| Aizome | Shiori (pdf) | 0.17 (0.12–0.17) | 0.12 (0.11–0.17) | 0.12 (0.11–0.12) | 0.12 (0.11–0.12) | 38 (38–39) | 38 (37–39) | 0 (0–0) |
| GNOME | Console (terminal) | 0.57 (0.56–0.57) | 0.47 (0.46–0.49) | 0.57 (0.56–0.57) | 0.47 (0.46–0.49) | 143 (143–144) | 143 (142–143) | 0 (0–0) |
| GNOME | Files (Nautilus) (files) | 0.67 (0.67–0.68) | 0.57 (0.52–0.58) | 0.52 (0.52–0.52) | 0.47 (0.47–0.52) | 256 (256–257) | 253 (228–255) | 8 (8–8) |
| GNOME | Settings (settings) | 0.57 (0.57–0.62) | 0.52 (0.50–0.53) | 0.57 (0.57–0.57) | 0.47 (0.45–0.47) | 144 (141–144) | 143 (140–143) | 0 (0–0) |
| GNOME | Text Editor (editor) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 144 (144–144) | 144 (141–144) | 0 (0–0) |
| GNOME | Calculator (calculator) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 146 (145–147) | 146 (143–147) | 0 (0–0) |
| GNOME | Image Viewer (Loupe) (image) | 3.19 (1.14–3.20) | 3.19 (1.14–3.20) | 0.52 (0.52–0.77) | 0.68 (0.57–0.77) | 440 (439–467) | 436 (433–441) | 0 (0–0) |
| GNOME | Document Viewer (Papers) (pdf) | 0.82 (0.73–0.83) | 0.79 (0.78–0.84) | 0.42 (0.42–0.42) | 0.42 (0.37–0.43) | 255 (233–256) | 252 (248–256) | 0 (0–0) |
| Plasma | Konsole (terminal) | 1.27 (1.17–1.27) | 1.22 (1.17–1.28) | 0.27 (0.27–0.27) | 0.22 (0.21–0.27) | 39 (37–39) | 37 (37–40) | 0 (0–0) |
| Plasma | Dolphin (files) | 0.63 (0.63–0.63) | 0.63 (0.58–0.65) | 0.32 (0.27–0.37) | 0.27 (0.26–0.32) | 96 (94–96) | 93 (91–94) | 0 (0–0) |
| Plasma | System Settings (settings) | 0.87 (0.87–0.87) | 0.63 (0.57–0.68) | 0.77 (0.77–0.77) | 0.53 (0.52–0.58) | 178 (178–178) | 177 (176–178) | 0 (0–0) |
| Plasma | KWrite (editor) | 0.27 (0.26–0.27) | 0.27 (0.26–0.32) | 0.22 (0.22–0.26) | 0.26 (0.21–0.28) | 35 (33–35) | 34 (32–35) | 0 (0–0) |
| Plasma | Gwenview (image) | 0.58 (0.57–0.58) | 0.58 (0.57–0.63) | 0.32 (0.27–0.32) | 0.27 (0.26–0.32) | 166 (159–168) | 166 (157–168) | 13 (13–13) |
| Plasma | Okular (pdf) | 0.53 (0.53–0.54) | 0.48 (0.47–0.53) | 0.37 (0.37–0.37) | 0.27 (0.26–0.32) | 82 (82–83) | 72 (69–72) | 0 (0–0) |

### 2 GiB

The same VMs with 2 GiB of RAM (1,962 MiB visible to the guest) and a 2 GiB
swap device, two boots each, measured through boot, login, memory and idle
cost only. All three desktops boot and log in as they do with 4 GiB, in about
the same times (Plasma's login took 5.8 s against 5.2 s). The
kernel keeps less for itself with less RAM, so every figure of memory in
use is about 80–110 MiB lower, but each desktop's share over the
no-desktop system is about the same: 206 MiB for Aizome, 560 for GNOME and
842 for Plasma at 190 s (221, 567 and 875 with 4 GiB). Plasma left 825 MiB available and
had pushed 25 MiB to swap, all of it compressed in zswap's pool (6 MiB),
none written to the swap disk; its memory pressure stall total over the
boot was 0.3 s, against 0.01–0.02 s for Aizome and GNOME. 2 GiB is enough
to log in to any of the three in this configuration, with nothing opened
yet.

#### Boot and login (2048 MiB; median, min–max over trials)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Trials | 2 | 2 | 2 | 2 |
| Boot: QEMU start → login screen settled (s) | 6.26 (6.26–6.26) | 7.41 (7.36–7.46) | 8.11 (7.66–8.56) | 5.81 (5.66–5.96) |
| &nbsp;&nbsp;kernel + initrd + userspace (systemd-analyze, s) | 0.19 + 2.75 + 2.14 | 0.20 + 2.74 + 2.27 | 0.19 + 2.31 + 1.97 | 0.19 + 2.74 + 1.97 |
| &nbsp;&nbsp;display manager started (host timeline, s) | 5.35 (5.33–5.37) | 5.51 (5.45–5.57) | 4.76 (4.33–5.19) | – |
| Login: Enter → desktop settled (s) | 1.23 (1.13–1.33) | 2.04 (2.04–2.04) | 5.79 (5.64–5.94) | – |
| &nbsp;&nbsp;Enter → first change on screen (s) | 0.63 (0.63–0.64) | 1.74 (1.64–1.84) | 0.13 (0.13–0.14) | – |
| &nbsp;&nbsp;Enter → PAM session opened (guest, s) | 0.10 (0.10–0.10) | 0.03 (0.03–0.03) | 0.08 (0.08–0.09) | – |

#### Memory (2048 MiB guest; MiB, median and range)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| At the login screen: used (MemTotal − MemAvailable) | 426 (422–430) | 617 (616–618) | 946 (941–951) | 292 (292–292) |
| &nbsp;&nbsp;over the no-desktop VM | +134 | +325 | +654 | +0 |
| &nbsp;&nbsp;swap in use | 2 (1–2) | 4 (4–5) | 2 (2–2) | 1 (1–1) |
| Desktop, 60 s after login: used (MemTotal − MemAvailable) | 501 (496–506) | 862 (860–863) | 1135 (1129–1142) | 289 (288–290) |
| &nbsp;&nbsp;over the no-desktop VM | +212 | +572 | +846 | +0 |
| &nbsp;&nbsp;swap in use | 3 (2–4) | 5 (4–6) | 25 (25–26) | 1 (1–1) |
| Desktop, 190 s after login: used (MemTotal − MemAvailable) | 499 (495–502) | 853 (849–857) | 1135 (1134–1137) | 293 (288–297) |
| &nbsp;&nbsp;over the no-desktop VM | +206 | +560 | +842 | +0 |
| &nbsp;&nbsp;swap in use | 3 (2–4) | 5 (4–6) | 25 (25–26) | 1 (1–1) |
| PSS at 190 s: system services | 85 (85–86) | 90 (89–91) | 67 (66–68) | 68 (68–68) |
| PSS at 190 s: login screen (still running) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) |
| PSS at 190 s: the user's session | 278 (277–280) | 695 (691–699) | 1093 (1093–1093) | 0 (0–0) |

#### Idle cost over 120 s at the desktop (2048 MiB)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Guest CPU busy (% of one vCPU) | 0.27 (0.22–0.31) | 0.24 (0.21–0.28) | 0.24 (0.20–0.29) | 0.09 (0.09–0.09) |
| Context switches / s | 182 (178–187) | 82 (81–82) | 116 (115–117) | 106 (106–106) |
| Thread wakeups / s (voluntary switches) | 100 (98–101) | 42 (42–42) | 59 (59–60) | 56 (56–56) |
| Interrupts / s | 187 (183–192) | 102 (102–102) | 138 (137–140) | 188 (187–189) |
| Host: QEMU CPU (% of one core) | 0.96 (0.95–0.97) | 0.81 (0.78–0.85) | 0.92 (0.92–0.92) | 0.91 (0.90–0.92) |
| Host: PPT over host idle (W) | -0.55 (-2.05–0.94) | -0.66 (-0.81–-0.51) | 3.30 (3.07–3.54) | 2.75 (1.81–3.70) |
<!-- tables:end -->

## What it shows

**Boot.** All four systems reach their display manager at about the same
point, 5.2–5.5 s after QEMU starts (kernel 0.2 s, initrd 2.7 s, userspace
2.0–2.2 s); the console-only VM shows its login prompt at 5.1 s. What
differs is how long each login screen takes to draw itself after that:
about 1.1 s for Aizome's, 2.1 s for GDM's (GNOME Shell in greeter mode) and
3.3 s for Plasma Login's (KWin, the greeter and a separate wallpaper
process), so the login screen is up and settled at 6.4, 7.6 and 8.6 s. The
spread between boots is 0.2 s or less for every desktop.

**Login.** From Enter to a settled desktop: Aizome 1.2 s, GNOME 2.0 s (to
its overview), Plasma 5.2 s. PAM opened the session within 0.1 s of the
Enter everywhere; the rest is the desktop starting. In the journal,
Aizome's compositor unit started 0.21 s after PAM and its session target
was reached at 0.61 s; GNOME's session target at 1.19 s; Plasma started
KWin and plasmashell at 0.37 and 0.44 s and reached its workspace target at
1.92 s, and its last elements (the wallpaper fading in, the panel) were on
screen at 5.2 s. Plasma's login screen reacts first (its own feedback,
0.13 s after Enter); GNOME's screen first changes 1.0–1.7 s after Enter.

**Memory.** At the desktop after 190 s, the guest has 591 MiB in use with
Aizome, 937 MiB with GNOME and 1,245 MiB with Plasma, against 370 MiB for
the same system with no desktop: each desktop's own cost is about 221, 567
and 875 MiB. At the login screen it is already 156, 315 and 646 MiB.
In PSS, Aizome's whole session is 276 MiB, of which the compositor and the
shell (one unit, `aizome.service`) are 154 MiB. GNOME's is 698 MiB: GNOME
Shell 366 MiB, and Evolution's data server, calendar, address book and
alarm services about 90 MiB, there for the shell's calendar. Plasma's is
1,115 MiB: plasmashell 354 MiB, KWin 216 MiB, and Akonadi (KDE's PIM
store: its server, a MariaDB server and fourteen agents and resources,
mostly for mail, started with the session) 330 MiB. On the login screen,
GDM's GNOME Shell holds 251 MiB, Plasma Login's KWin, greeter and wallpaper
processes 371, 199 and 198 MiB, and Aizome's greeter session 136 MiB; none
of the login screens keeps running once the desktop is up. On this display
the OpenGL compositors (and GTK 4 apps) also map Mesa's llvmpipe and LLVM,
which Aizome's pixman renderer does not need. None of the three used more
than a few MiB of swap at 4 GiB.

**Idle.** None of the three costs much CPU at idle here: the guest is busy
0.17%, 0.26% and 0.19% of one vCPU (Aizome, GNOME, Plasma) against 0.17%
with no desktop, and QEMU's own CPU time on the host is 0.8–0.9% of a core
for all four (Plasma's first boot ran at 0.78% in the guest while something
in its session was still busy). Aizome loses on wakeups: its threads woke
100 times a second, against GNOME's 43 and Plasma's 60 (the console VM's
56 are mostly kernel workers). Most are `aizome-shell`, at
47 a second: nine `aizome-sys` threads (seven watching `org.*` D-Bus
services, MPRIS and a status thread) each wake four times a second, an
rfkill thread and two blocking-pool threads twice; the compositor's
watchdog wakes four times a second; and each of Aizome's helpers (Koyomi's
agent, Hako's file-manager service, the network agent, the portal) has a
thread waking twice a second. They cost little CPU in this VM, but each
wakeup keeps a laptop's core out of its deepest idle state, and this is the
clearest thing here for Aizome to fix. In Plasma, PowerDevil (14 a second)
and MariaDB (3) lead; in GNOME, nothing in the session wakes more than
about once a second. The host's power during the idle windows moved by a
few watts between boots of the same desktop, more than between desktops,
so it shows no difference.

**Input to screen.** At idle, a key press reached the screen in 6.5 ms at
the median with Aizome (0.49 frames), 10.5 ms with GNOME (0.78) and 9.6 ms
with Plasma (0.72); at the 99th percentile in 13.6, 17.0 and 26.0 ms (1.02,
1.28 and 1.95 frames). Aizome's distribution is what a compositor that
repaints for the next vblank gives with presses at random phases: half a
frame on average and never more than one. GNOME usually adds part of a
frame, and Plasma, at the 95th and 99th percentiles, nearly a whole one. With two threads
busy in the user's apps on the 2-vCPU guest, Aizome stayed where it was
(7.0 ms median, 13.5 ms p99), while GNOME moved to 13.0 / 23.6 ms (one
press at 55 ms) and Plasma to 13.5 / 26.2 ms: about a frame at the
median for both. The probe received the keys about 0.1 ms after the press
(median; at most 6 ms, under load): the rest is the compositor. Under load the three are protected
differently by Sekka's tuning: ananicy gives GNOME Shell and KWin nice −12,
and Aizome's compositor has a CPU weight of 1000 inside a session slice
weighted 400 against the apps' 100.

**Frame pacing.** All three kept 75 frames a second for a client drawing a
new full-window frame on every frame callback. Aizome missed no refresh, idle
or loaded; Plasma none at idle and 0.09% in one of three loaded runs; GNOME missed 0.09% at
idle and 0.84% under load, as single dropped frames (intervals of 26.7 and
40 ms).

**Apps.** Aizome's apps were on screen and settled 0.11–0.17 s after the
launch command, cold or warm, which is this harness's floor (`systemd-run`
itself takes about 60 ms in the guest, and screendumps come every 50 ms);
Shashin needed 0.32 s to show the 20-megapixel photo in full. GNOME's GTK 4
apps were up at about 0.47 s and settled at 0.47–0.8 s; Loupe had its
window up at 0.5–0.8 s, the photo at about 1.4 s, and settled at 3.2 s in
most runs, when its on-image controls hide. They hold 143–256 MiB each,
much of it llvmpipe's, since GTK 4 renders with OpenGL, which here runs on
the CPU. Plasma's Qt apps had their windows up at 0.22–0.53 s and settled at
0.27–0.87 s, except Konsole, whose window appeared at 0.22 s and which
changed once more, by about 3,700 pixels, a second later (consistent with
the shell's prompt being drawn), settling at 1.2 s; they hold 34–178 MiB.
Aizome's apps hold 15–57 MiB, except Shashin, which keeps the decoded
photo (326 MiB, against Gwenview's 166 and Loupe's 436). Hotaru (57 MiB
with its fish shell) uses more than Konsole (37 MiB with bash). First
launches after boot were at most a quarter of a second slower than later
ones (Plasma's System Settings, 0.87 against 0.63 s; most differ by 0.1 s
or less): the golden disk had every app's caches, and the store is on an
SSD.

**Disk.** The no-desktop system's closure is 3.55 GB. Every desktop first
adds the same 3.33 GB (the graphics stack, with 64- and 32-bit Mesa and
LLVM, speech-synthesis voices, fonts, and the services desktops need); on
top of that Aizome adds 1.63 GB of its own, GNOME 1.66 GB and Plasma
3.92 GB, for totals of 8.49, 8.52 and 10.78 GB
([data/desktops/footprint.json](data/desktops/footprint.json)). Aizome's
installation is no smaller than GNOME's.

## Method

### Observing the screen

The harness polls QMP `screendump` (a 1920×1080 PPM in `/dev/shm`) every
100 ms during boot and login and every 50 ms during app launches, and
compares each frame with the one before. A pixel has changed when any
channel moved by more than 24 of 255; a frame has changed when more than
2,500 pixels did (0.12% of the screen). That leaves out a blinking text
cursor and a clock's minute digits, and keeps a window opening, a menu
appearing or a wallpaper fading in.

- **Settled** is when the final picture was up: the last change before
  the screen stayed unchanged for a quiet window (3 s for boot and login,
  2 s for apps), or, if later, the first frame matching the reference
  (below), since a slow fade changes little between two polls but a lot in
  all. It includes every fade, slide and late element (a panel filling in,
  a prompt appearing).
- Boot and login also require the frame to match a **reference** within
  2% of its pixels: the login screen and the desktop as a separate setup
  boot captured them (the no-desktop VM's console within 0.1%). A black
  console that pauses for 3 s is not "the login screen".
- App launches require the settled frame to differ from the frame before
  the launch by at least 20,000 pixels: a window appeared. **First change**
  is the first frame that differed from the one before by more than the
  threshold: usually the window appearing.
- Resolution is the polling interval: ±100 ms for boot and login, ±50 ms
  for apps. Screendumps run on QEMU's main thread, on the same pinned CPUs;
  none were taken during the memory, idle or latency measurements.

### One measured boot

Each trial is one boot, under the shared measurement lock (no other
measurement or build ran meanwhile), with the host's 1-minute load average
below 1.5 at the start:

1. **Host idle power**: 30 s of PPT with no VM, the floor for that
   trial's power delta.
2. **Boot**: QEMU is started (time zero) and the screen watched until the
   login screen (for the no-desktop VM, the console's login prompt) is
   visible and settled. Inside the guest: `systemd-analyze`, the critical
   chain and `blame`, and when the display manager became active, mapped
   onto the host's timeline through the guest's CLOCK_MONOTONIC (read over
   the command channel, best of five round trips of about 4 ms).
3. **Memory at the login screen**, 30 s after it settled.
4. **Login**: the password is typed through QMP's keyboard the way each
   login screen takes it (GDM: Enter on the listed user, wait 2 s, the
   password; Plasma Login: a key to wake its controls, which is then
   cleared, the password; Aizome's: the password). Time zero is the final
   Enter; the screen is watched until the desktop is up and settled. The
   journal gives the guest's side: Enter → PAM session opened, and PAM
   session → the desktop's key units started. GNOME opens its overview at
   login, and that is what its login time ends on; the harness then closes
   it with Esc, so that the idle and memory figures are for the empty
   desktop, as the other two show it.
5. **Memory** 60 s after the Enter, **idle cost** from 65 s to 185 s,
   **memory** again at 190 s.
6. **Responsiveness**: the probe, at idle and under load (below).
7. **App start-up**: each app's first launch since boot (cold), then five
   more rounds of every app (warm).
8. Poweroff; the overlay is discarded.

The no-desktop VM has no login: its time zero for steps 5 on is 10 s after
the console settled.

### Memory

`MemTotal − MemAvailable` from `/proc/meminfo` (memory the kernel could
not give a new program without reclaiming or swapping), swap in use and
zswap's pool, and per-process PSS (proportional set size: shared pages
divided among the processes that map them) from `/proc/*/smaps_rollup`,
grouped by where systemd put each process: **system** services
(`system.slice`, PID 1), the **login screen** (its user: `greeter`,
`gdm-greeter`, `plasmalogin`, with their user managers) and the
**session** (Demo's `user-1000.slice`: the login scope and Demo's user
manager with everything it started). The command channel's shell is left
out. The no-desktop VM, measured at the same times, is the operating
system's own share, and each desktop's cost is the difference.

### Idle cost

Over 120 s with nothing happening on the desktop: guest CPU busy from
`/proc/stat` (as a share of one vCPU), context switches and interrupts per
second, and per-thread voluntary context switches (wakeups) and CPU ticks
from `/proc/*/task/*`, to name the busiest; on the host, the QEMU
process's CPU time (all threads) and the PPT power meter's mean, also as a
difference from the trial's host-idle floor.

### App start-up

Each app is started inside the session as a launcher would, as a
transient systemd user unit in `app.slice` with the user manager's
environment (which every one of these sessions imports:
`WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP`, the D-Bus address):
`systemd-run --user -M demo@ --slice=app.slice --
/run/current-system/sw/bin/<app> [file]`. The image viewers open the same
6000×3375 JPEG (`hintersee.jpg`, one of Tatami's wallpapers, 2.7 MB) and
the PDF viewers the same 12-page text PDF; the others start empty (the
terminal with the user's shell, the file manager in the home folder, the
editor on a new document). The time runs from the `systemd-run` command to
the screen settling with a window up. A second later, the PSS of the app's
processes (its unit's, and new processes in scopes it made under
`app.slice`, as Konsole does) and, separately, of other new processes it
caused to start (D-Bus-activated helpers). The app is then stopped (its
unit, and its processes by name) before the next. Cold is each app's first
launch after boot; warm, the next five. Plasma ships no calculator in
Sekka's configuration (KCalc is not part of NixOS's Plasma), so it has
six apps.

### Responsiveness: latency in frames

[`wl-probe.c`](../../tests/performance/desktops/wl-probe.c) is a plain
Wayland client (wl_shm, xdg-shell, wp_presentation) with an 800×600 window.
Its two colours' buffers are filled once, so drawing costs it nothing: the
numbers are the compositor's and the guest's scheduling.

- **Input to screen**: a root helper
  ([`uinput-keys.c`](../../tests/performance/desktops/uinput-keys.c))
  creates a uinput keyboard and presses A 220 times per run, 120–250 ms
  apart at random, so presses don't lock to the display's phase, recording
  CLOCK_MONOTONIC just before each press. The probe flips its whole window
  to the other colour on each press and asks for presentation feedback.
  Latency is the presentation time minus the press time: the kernel's
  evdev, the compositor's input handling, the client, the compositor's
  repaint and the page flip. All three compositors report presentation
  times in CLOCK_MONOTONIC (`clock_id` 1, checked each run) with
  microsecond resolution, and every press arrived and was presented. In
  frames: the latency divided by the refresh period the compositor
  reported with each presentation (13.33 ms: 75 Hz).
- **Frame pacing**: the probe draws a new full-window frame on every frame
  callback for 30 s; intervals between presented frames give the frame
  rate and the refreshes missed (an interval of k refreshes misses k − 1).
- Both at idle and **under load**: two threads that only compute
  (`perf-util spin 2`, nice 0) as a transient unit in the user's
  `app.slice`, as a build started from a terminal would be, on the 2-vCPU
  guest.
- If the compositor showed the probe's window without giving it keyboard
  focus, the harness clicked it (found by its colour), as a person would.
  None of the measured runs needed it.

On virtio-gpu a page flip completes when QEMU has taken the frame, on a
75 Hz vblank the kernel emulates. "Presented" here means handed to the
virtual display, not lit on a panel.

### First login and first-run windows

The setup boot (not measured) logged in for the first time and recorded
what each desktop showed ([data/desktops/screens](data/desktops/screens)):

- **Aizome** opened Annai's "Welcome to Aizome" pages over the desktop, its
  first-run guide ("Show at login" is off, so it appears once). The harness
  closed it.
- **GNOME** opened the overview with GNOME Shell's "Welcome to NixOS
  26.11" dialog offering the tour (Skip / Take Tour). gnome-initial-setup
  did not run for this account (created by the system, not by the setup
  assistant). The harness marked the dialog as shown
  (`org.gnome.shell welcome-dialog-last-shown-version`) and
  `~/.config/gnome-initial-setup-done`, and closed it with Esc.
- **Plasma** showed its desktop and panel; no Welcome Center appeared.

Every measured login is a returning person's, with no first-run window:
Aizome's and Plasma's desktops, and GNOME's overview.

### What changed while building the harness

Smoke runs, not in the results, found two things worth knowing:

- **GDM waits for a boot VGA device.** With the GPU as `virtio-gpu-pci`
  (not VGA-class, so no `boot_vga`), GDM logged "It appears that your
  system does not have a primary GPU! Proceeding with any GPU" 10 s after
  starting, and GNOME's login screen took 17.4 s from QEMU's start instead
  of 7.5 s; Aizome's and Plasma's were unaffected (6.2 and 8.4 s). Every
  measured run uses `virtio-vga`, which is what libvirt and `-vga virtio`
  give. The three `virtio-gpu-pci` boots are kept in
  `data/desktops/trials-virtio-gpu-pci/` as evidence.
- **Aizome's wallpaper follows the time of day.** Its login screen and
  desktop show a day and a night wallpaper. Aizome's second boot, started
  after sunset by the host's clock, never matched the daytime reference
  frames; it was stopped, discarded and run again with the guest clock
  fixed to 15:00 UTC, as every boot from then on was.

## Limits

- **One machine, one VM shape.** 2 vCPUs pinned to two host cores and
  their SMT siblings, 4 GiB (and 2 GiB) of RAM, virtio-gpu without 3D. On
  real GPUs GNOME Shell and KWin composite on the GPU, Aizome uses GLES, and
  the gaps in rendering cost, memory (llvmpipe's buffers and code) and
  latency will be different, in either direction. Nothing here is a
  physical-display or real-hardware measurement; no photon was timed.
- **Presented is not lit.** virtio-gpu has no scanout: the compositors
  pace themselves to the mode's 75 Hz and the kernel's emulated vblank,
  and "presented" means QEMU took the frame. The latency figures measure
  input handling, scheduling and composition up to the flip; a monitor's
  own latency would add the same amount to each.
- **What each desktop ships.** Sekka's configuration, not upstream
  defaults: Sekka's tuning (ananicy rules that give GNOME Shell and KWin
  nice −12, Aizome's cgroup weights), the services each desktop module
  enables, and the apps each brings. A GNOME or Plasma configured
  differently (Akonadi off, Evolution's data server gone, no Tracker) would
  use less memory; the same is true of Aizome.
- **The harness's resolution.** App and login times are polled screendumps
  every 50/100 ms; `systemd-run` alone takes about 60 ms inside the guest,
  so ~0.11 s is the floor of what an app launch can show here, and
  Aizome's apps are at that floor.
- **The second login, not the first.** Measured logins are a returning
  person's; first-run windows were dismissed once in the setup boot.
- **Power is noisy.** The host's PPT sensor sees the whole socket; other
  work on the machine outside the measurement lock (the user's desktop,
  other sessions) moves it by watts. The idle windows' power deltas are reported, but QEMU's CPU
  time is the steadier measure of what an idle desktop costs the host.
- **Few boots.** Three boots per desktop at 4 GiB and two at 2 GiB, the
  subjects alternating; spread is shown as minimum–maximum over boots, and
  latency percentiles pool 660 presses per condition. Differences smaller
  than the spread are not differences.
- **Login ends where each desktop ends.** GNOME's login time ends on its
  overview, which is what it shows at login; Aizome's and Plasma's on the
  desktop.

## Reproduce

All from this repository's root, on x86_64 Linux with KVM and Nix with
flakes, with about 40 GB free for the VM images (each VM's closure as an
uncompressed erofs image, 3.6–10.8 GB, and a golden root disk, under
`$TMPDIR/aizome-perf/desktops/img`; set `DESKBENCH_ROOT` to put them
elsewhere, and pass `summarize --runs` the matching `runs` directory). The harness never touches the
host's devices: every disk is an image file, the display is headless.

```sh
# Build the four systems (no compiling: all from Sekka's and NixOS's caches)
nix build --impure -f tests/performance/desktops --no-link \
  baseline.toplevel aizome.toplevel gnome.toplevel plasma.toplevel host tools

# Prepare each golden disk and reference frames (two boots each, unmeasured)
for v in aizome gnome plasma baseline; do tests/performance/desktops/run.sh setup $v; done

# One measured boot (repeat, alternating subjects; use a measurement lock
# on a shared machine). Pin with DESKBENCH_CPUS=2,3,10,11 (the default).
tests/performance/desktops/run.sh trial aizome r1 --png
tests/performance/desktops/run.sh trial gnome r1
tests/performance/desktops/run.sh trial plasma r1
tests/performance/desktops/run.sh trial baseline r1
# 2 GiB: boot, login, memory and idle only
tests/performance/desktops/run.sh trial gnome m1 --mem 2048 --phases hostidle,idle

# Or a queue of trials, several per hold of the lock:
#   lock-measure tests/performance/desktops/batch.sh QUEUE 44 13

# Summaries, tables and per-trial data into docs/performance/data/desktops
tests/performance/desktops/run.sh summarize --keep-raw
# Closure sizes and versions
tests/performance/desktops/footprint.sh
```

| File | What it is |
|---|---|
| `default.nix` | The four systems from this checkout's flake, the probe tools, the host environment |
| `base.nix` | What all four share: reference.nix's installed-system settings, VM disks |
| `instrument.nix` | The benchmark's additions to each VM (root shell on a virtio-serial port, tools, uinput) |
| `tools.nix`, `wl-probe.c`, `uinput-keys.c`, `perf-util.c` | The latency/pacing probe, the uinput key injector, a clock and CPU-load helper |
| `bench.py` | Runs the VMs: setup boots, measured trials (QEMU, QMP screendumps, the guest shell) |
| `summarize.py` | Raw runs → per-trial summaries, `summary.json`, `tables.md` |
| `batch.sh`, `run.sh`, `footprint.sh` | Queue runner under the lock, environment wrapper, closure sizes |
