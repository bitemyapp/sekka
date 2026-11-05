# Why Aizome is lighter and quicker than GNOME and Plasma in the small VM

[desktops.md](desktops.md) measured the same Sekka system with three
desktops in a 2-vCPU, 4 GiB VM with no 3D: Aizome needed 221 MiB over a
bare system against GNOME's 567 and Plasma's 875, logged in in 1.2 s
against 2.0 and 5.2 s, and put a key press on screen in 0.53 frames at the
median under load against 0.97 and 1.01. This page takes those numbers
apart: where each desktop's memory goes, what each does between the Enter
and a settled desktop, where the time goes between a key press and its
frame, and what each wakes for at idle.

It uses three kinds of evidence, kept apart below:

- **The VM trials' own records**, re-analysed: per-process memory split
  into anonymous, file-backed and shared pages, `/proc/meminfo`, the guest
  journals and screendump timelines of each login, per-thread CPU and
  wakeups, and every key press's commit and presentation times
  (`docs/performance/data/desktops/trials/`). Measured.
- **Headless runs on the host**: GNOME Shell, KWin with plasmashell, and
  Aizome's compositor with its shell, each in a sandbox on two CPUs and
  rendering on the CPU as in the VM (llvmpipe; pixman for Aizome), for
  memory by mapping, idle work, and the CPU an update and a window move
  cost. Measured, but not a VM and not a login: what they can and cannot
  stand in for is said where they are used.
- **The source** of Mutter 50.5, GNOME Shell 50.4, KWin 6.8.2,
  plasma-workspace 6.8.2 and Aizome (`d7ecbac`), for the mechanisms.
  Where a mechanism is read from code rather than seen in a measurement,
  it is marked *inferred*.

Everything here is about this configuration: Sekka's systems, one small
VM without GPU acceleration, one machine (runnymede, Ryzen 7 9800X3D,
Linux 7.3). On a real GPU, GNOME Shell and KWin composite on the GPU and
several of the costs below (llvmpipe's code, threads and buffers, the
time to composite) shrink or move; no photon was timed.

## The answer, ranked

Each gap is Aizome against the other desktop, in the units desktops.md
reports. "Design" is what the desktop does as its developers ship it;
"configuration" is what Sekka's system (through NixOS's desktop modules,
which Sekka uses unchanged) adds or turns on. Ranks are by size.

### Memory in use, 190 s after login (Aizome 221 MiB over a bare system)

"In use" is MemTotal − MemAvailable, the figure desktops.md reports. It is
almost all anonymous memory (heaps, private data), shared memory and the
kernel's own; file-backed pages (library code, fonts, caches) are page
cache the kernel can drop, so they barely count. The rows are the
anonymous part of each group's PSS, then shared memory and kernel memory
from `/proc/meminfo`; they add up to within 4% of the measured gap
([data/why-desktop/memory-gap.json](data/why-desktop/memory-gap.json)).

| | Contributor | GNOME over Aizome | Plasma over Aizome | Kind |
|---|---|---:|---:|---|
| 1 | **The compositor and shell's own heaps**: gnome-shell 220 MiB anonymous; KWin 107 + plasmashell 231; Aizome's compositor 11 + shell 35. About 79 MiB of GNOME Shell's is Sekka's 4928 × 3264 wallpaper, which Mutter keeps as a full-size texture (llvmpipe keeps it in RAM); Aizome's wallpaper costs it about 24 | **+175 MiB** (+120 without the wallpapers) | **+292 MiB** | design (the wallpaper's size: configuration) |
| 2 | **PIM and search services started with the session**: Akonadi's server, MariaDB and 14 agents and resources, Baloo (Plasma, 231 MiB anonymous, 336 PSS); Evolution's data server and alarm notifier, GNOME Shell's calendar server, Online Accounts, LocalSearch (GNOME, 61 MiB, 122 PSS) | +61 MiB | **+231 MiB** | configuration (below) |
| 3 | **The desktop's helper daemons**: 18 `gsd-*` settings daemons, the shell's GJS notification and screensaver services, its portal backend (GNOME); kded6, PowerDevil, KAccess, the polkit agent, the activity manager, ksmserver, the KDE portal, Discover's notifier (Plasma); Aizome's four helpers | +41 MiB | +74 MiB | design (Discover's notifier: configuration) |
| 4 | **Kernel memory** for more processes and threads (unreclaimable slab, page tables, kernel stacks) | +28 MiB | +35 MiB | follows from the rest |
| 5 | **The input method**: IBus's daemon, GTK extension (emoji and Unicode panels), X11 bridge and portal | +28 MiB | – | GNOME's default |
| 6 | **X11 support**: Mutter runs X11 window frames in a separate GTK process (`mutter-x11-frames`, 25 MiB anonymous) | +24 MiB | +3 MiB | design |
| 7 | **Shared memory** (window buffers; llvmpipe's display buffers) | −8 MiB | +26 MiB | design |
| 8 | Common services (PipeWire, portals, keyring, D-Bus) | +10 MiB | +10 MiB | – |
| | Sum of the rows / measured gap | +359 / **+346 MiB** | +670 / **+653 MiB** | |
| | (File-backed pages mapped: library code and data, not in use) | (+82 MiB) | (+231 MiB) | |

**Design or configuration, in MiB.** Taking out the services that are
there because of how Sekka's system (through NixOS's modules) is
configured, rather than because the desktop needs them: Akonadi, MariaDB
and Baloo (Plasma; NixOS's Plasma module sets `programs.kde-pim.enable =
mkDefault true`, which installs KMail, Kontact, Merkuro and Akonadi, and
Akonadi is then D-Bus-activated 7–9 s into every session) and
Discover's notifier, the gap to Aizome is still about 410 MiB for Plasma.
GNOME's EDS and LocalSearch are different: NixOS enables them because they
are GNOME core services (GNOME Shell's own calendar server activates EDS,
0.8 s into the login), so they are GNOME's design as shipped, but a
distribution could leave them out; without them and IBus, GNOME's gap is
about 255 MiB. Sekka's wallpapers are configuration too: GNOME's
4928 × 3264 one costs GNOME Shell about 79 MiB, Aizome's about 24 (row 1).
The compositor and shell is the largest part of both gaps either way.

### Login: Enter → settled desktop (Aizome 1.2 s)

| | Contributor | GNOME over Aizome | Plasma over Aizome | Evidence | Kind |
|---|---|---:|---:|---|---|
| 1 | **What gates the first picture.** Plasma's splash stays up until plasmashell's desktop is "UI ready" (4.64 s), then fades into the desktop and panel (5.3 s). GDM keeps its greeter on screen until the user's GNOME Shell has finished its whole startup, startup animation included, and registered with GDM (1.70 s); then the greeter is torn down and the overview appears (2.04 s). Aizome's compositor draws as soon as it is up (0.63 s), and the desktop is complete when the shell's wallpaper lands (1.13–1.23 s) | +0.8 s | +4.0 s | measured: journal + screen timeline; code: `shell/shellcorona.cpp`, `ksplash/ksplashqml/splashapp.cpp`, `js/ui/main.js:343-373`, `js/ui/layout.js:693-707` | design |
| 2 | **CPU burned on two vCPUs while that happens**: 8.4 s of session CPU by 65 s after the Enter in Plasma (KWin 3.9 s, 3.6 s of it llvmpipe drawing the animated splash; plasmashell 2.1 s; KSplash 0.9 s; Akonadi and MariaDB 1.3–1.5 s, started after the desktop settled), 3.1 s in GNOME (IBus's GTK extension 1.35 s, GNOME Shell 1.0 s), 0.6 s in Aizome | +2.5 s CPU | +7.8 s CPU | measured: per-thread CPU at the idle window's start | design, except IBus (default) and Akonadi (configuration) |
| 3 | **Services started in the login's critical window**: Plasma reaches `plasma-workspace.target` only when PowerDevil is up (2.01 s; it spends 0.75 s initialising libddcutil and helpers), then starts portals, Baloo, Discover's notifier, KUnifiedPush; GNOME starts EDS, 18 `gsd-*` units and IBus between 0.8 and 1.4 s | – | – | measured | design (and Discover: configuration) |

### Key → screen under load (Aizome 0.53 frames, 7.0 ms)

All three hand the key to the client in about 0.1 ms and the probe commits
2 µs later, so the whole gap is in one question: does the commit make the
next vblank, or wait a refresh (13.3 ms) longer?

| | Contributor | GNOME over Aizome | Plasma over Aizome | Evidence | Kind |
|---|---|---:|---:|---|---|
| 1 | **How late a commit can come and still make the next vblank** (the latch point). Aizome draws the moment a commit arrives and submits it, 0.2 ms before the vblank at the latest. Mutter paints on llvmpipe, then its KMS thread holds the update until vblank − (its measured worst KMS dispatch + 0.8 ms + the vblank's duration): 3.4 ms at idle, 4.5–9 ms under load. KWin composites at once when late and makes it up to about 1.3 ms before the vblank, about 4 ms under load | +0.44 frames (GNOME misses the first vblank for 48% of presses, Aizome for 3%) | +0.29 frames after each run's opening seconds (11.3 against 7.4 ms; 34% missed) | measured: commit phase against outcome, 660 presses per condition; code: `clutter-frame-clock.c`, `meta-kms-crtc.c`, `renderloop.cpp`, Aizome's `state.rs`, `backend/udev.rs` | design |
| 2 | **A render-time estimate that stays high**: every Plasma run began with 28–69 presses in a row presented a whole refresh late, at any phase: the first 6–15 s of presses after the probe's window appeared; thereafter 9% (idle) and 34% (load) of presses missed | – | the remaining +0.19 frames of the median | measured (the runs of misses); mechanism inferred from code: KWin's estimate is average + 2 × variance with the variance decaying over 6 s, and an estimate over one refresh turns on triple buffering, held until ten frames come in under 0.95 refresh | design |
| 3 | **Competing for two vCPUs**: llvmpipe's rasteriser threads in GNOME Shell and KWin run as ordinary threads beside the load; Aizome's pixman composite is on its main thread | (in rank 1) | (in rank 1) | inferred | design, with Sekka's tuning (ananicy gives GNOME Shell and KWin nice −12; Aizome's compositor CPU weight 1000) |

### Idle (Aizome loses: 100 wakeups a second against 43 and 60)

| | Contributor | Wakeups a second | Evidence | Kind |
|---|---|---:|---|---|
| 1 | Aizome's shell: `aizome-sys` threads waking every 250 ms to check for a stop request (seven D-Bus name watchers 28/s, the status and MPRIS threads 4/s each, rfkill 2/s), two blocking-pool threads 2/s each, the settings watcher 2/s | 47 | measured (VM and host) | design (fixable) |
| 2 | Aizome's compositor: its watchdog 4/s, main loop 1.75/s, settings watcher 1/s; each Aizome helper (calendar agent, file-manager service, network agent, portal) 2/s | 7 + 8 | measured | design (fixable) |
| 3 | Plasma: PowerDevil 14/s (libddcutil's display watch and recheck threads, by its log), MariaDB 3/s; the shell and KWin about 1.7/s together | 19 | measured | design; MariaDB is configuration |
| 4 | GNOME: nothing in the session above about once a second (GNOME Shell 1.3/s) | ~5 | measured | design |

None of this is visible as CPU time in the VM (all three are within
0.1 percentage points of the bare system); it matters on a laptop, where
each wakeup keeps a core out of its deepest idle state.

## Memory

### What "in use" is made of

Each desktop's VM against the no-desktop VM, 190 s after login (MiB,
medians of three boots, `/proc/meminfo`):

| | Aizome | GNOME | Plasma |
|---|---:|---:|---:|
| In use (MemTotal − MemAvailable) | +221 | +567 | +875 |
| Anonymous pages (heaps, stacks, private data) | +119 | +464 | +729 |
| Shared memory | +35 | +26 | +60 |
| Unreclaimable kernel slab | +12 | +28 | +30 |
| Page tables | +6 | +15 | +20 |
| Kernel stacks | +3 | +6 | +5 |
| *File pages mapped by processes (page cache; reclaimable)* | *+174* | *+256* | *+406* |

Anonymous memory is about 80% of GNOME's and Plasma's cost and half of
Aizome's, whose shell keeps its surfaces in shared memory. Library code,
fonts, icon caches and other files mapped into processes show up only as
page cache, which MemAvailable counts as available: GNOME and Plasma map
82 and 231 MiB more of it than Aizome, and almost none of that is in the
figure desktops.md reports. So "per-process library overhead" matters
here in a narrower form than it sounds: not the size of the libraries, but
the private memory each process makes when it loads them (relocated data,
its heap at start-up, its threads' stacks), counted below.

### By process: the session in the VM

PSS of the user's session 190 s after login, by role, with its anonymous
part (MiB; medians of three boots;
[data/why-desktop/memory-gap.json](data/why-desktop/memory-gap.json),
[memory-split.json](data/why-desktop/memory-split.json)):

| Role | Aizome | GNOME | Plasma |
|---|---|---|---|
| Compositor and shell | 120 (anon 45): `aizome` 62 (11), `aizome-shell` 58 (35) | 301 (220): `gnome-shell` | 533 (337): `plasmashell` 345 (231), `kwin_wayland` 189 (107) |
| PIM and search | – | 122 (61): EDS's four processes 90, calendar server 8, Online Accounts 10, LocalSearch 14 | 336 (231): MariaDB 75; Akonadi's control process, server and 14 agents and resources 255; Baloo 6 |
| Desktop helpers | 26 (5): calendar agent, network agent, file-manager service, portal (5–7 each) | 76 (45): 18 `gsd-*` (1–2 each), two GJS services 25, portal backend 16, the global-shortcuts provider | 123 (79): kded6 28, PowerDevil 14, portal 13, Discover's notifier 12, KAccess, polkit agent, activity manager, KIO workers, ksmserver (4–9 each) |
| Input method | – | 42 (28): IBus extension 18, X11 bridge 17, daemon 4 | – |
| X11 support | 34 (10): Xwayland 30, xwayland-satellite 4 | 66 (35): `mutter-x11-frames` 44, Xwayland 21 | 29 (13): Xwayland 24, the XEmbed tray proxy 4 |
| Common (PipeWire, portals, keyring, D-Bus, systemd) | 97 (41) | 91 (51) | 95 (51) |
| **Session** | **276 (102)** | **697 (441)** | **1,116 (712)** |

Three things stand out:

- **A process that links big libraries pays for them privately, even
  idle.** Each of Akonadi's idle mail agents (archive, filter, follow-up,
  send-later, unified mailbox, mail-merge) holds 27–28 MiB PSS, 18 of it
  anonymous, and the host runs show what that is: 14.4 of the mail filter
  agent's 18.5 MiB is *relocated library data*, the pages of each shared
  library's data and vtables that the dynamic linker rewrites at load and
  that therefore become private to the process. 5.5 MiB of it is Qt
  WebEngine's (Chromium's), which the agent links; ffmpeg, OpenSSL and Qt
  Quick follow. Evolution's source registry is the same: 10.4 of its
  12.7 MiB, with WebKitGTK (3.2 MiB) and flite's speech voices (0.5–0.6
  MiB each) at the top. kded6: 6.5 of 12.9. This is the real form of
  "per-process library overhead" in the in-use figure; the libraries'
  code itself is shared page cache. Most `gsd-*` daemons hold 1–2 MiB.
  Aizome's helpers hold 0.7–1.4 MiB anonymous each: they are Rust
  binaries with few shared libraries, and Aizome's compositor and shell
  have 1.7 and 0.5 MiB of relocated data against GNOME Shell's 8.9,
  KWin's 11.2 and plasmashell's 14.0. The session has 59 processes with
  Plasma, 70 with GNOME, 30 with Aizome.
- **GNOME pays twice for X11 window frames and input methods**: Mutter
  draws the decorations of X11 windows in a separate GTK process
  (`mutter-x11-frames`, 44 MiB) and IBus brings an X11 bridge (17 MiB),
  both running with no X11 app open.
- **Aizome's whole session is smaller than GNOME Shell alone.**

### Inside the compositor and shell

The VM records only each process's totals, and its disk images had been
deleted, so the split by mapping comes from the headless host runs (method
[below](#the-headless-runs-on-the-host)). They reproduce the VM's
compositor and shell sizes closely where it matters, in anonymous memory:
GNOME Shell 240 MiB on the host against 220 in the VM, plasmashell 230
against 231, KWin 97 against 107, Aizome's compositor 12.5 against 11 and
its shell 34 against 35 (MiB; medians of two clean runs on the host; the
host's GNOME Shell has run for 40 s rather than 190, and never opened its
overview). Their file-backed and shared parts differ more (Aizome's
compositor maps less without its DRM backend), which matters little to
memory in use.

#### Compositor and shell memory by mapping, headless on the host (MiB PSS; median of clean runs)

| Mapping | Aizome compositor | Aizome shell | GNOME compositor+shell | Plasma compositor | Plasma shell |
|---|---:|---:|---:|---:|---:|
| other anonymous mmap (large malloc blocks, engines' own heaps, driver buffers) | 8.1 | 1.2 | 128.4 | 23.0 | 72.4 |
| malloc: main heap (brk) | 2.5 | 3.9 | 82.4 | 55.0 | 97.2 |
| libraries: code | 13.6 | 0.2 | 54.7 | 39.1 | 52.6 |
| malloc: thread arenas | 0.0 | 27.8 | 10.8 | 7.1 | 30.9 |
| libraries: read-only data | 8.3 | 0.3 | 23.6 | 17.1 | 29.6 |
| executable | 0.0 | 18.2 | 0.0 | 1.8 | 1.1 |
| libraries: relocated data (private) | 1.7 | 0.5 | 8.9 | 11.2 | 14.0 |
| JS GC heap (Qt's QML engine, named memfd) | 0.0 | 0.0 | 0.0 | 0.6 | 13.5 |
| llvmpipe display buffers (memfd shared with the compositor) | 0.0 | 0.0 | 0.0 | 10.6 | 10.6 |
| JS GC heap (1 MiB-aligned chunks) | 0.0 | 0.0 | 8.7 | 0.0 | 0.0 |
| caches, schemas, locale, resources | 0.0 | 0.0 | 1.3 | 4.7 | 5.0 |
| other files | 0.3 | 0.3 | 4.2 | 0.3 | 0.4 |
| shared memory (window buffers, wl_shm pools) | 0.5 | 1.5 | 0.0 | 0.0 | 1.3 |
| fonts (files mapped) | 0.3 | 0.6 | 0.8 | 0.0 | 0.2 |
| **PSS total** | **35.6** | **54.7** | **324.7** | **171.2** | **329.6** |
| of which anonymous | 12.5 | 34.3 | 239.7 | 97.4 | 229.8 |

#### The wallpaper's share (MiB PSS of the compositor and shell; medians)

| | Sekka's wallpaper | With it | Against | Difference | of it anonymous |
|---|---|---:|---:|---:|---:|
| Aizome | Tokyo at dusk, 6000 × 3750 | 90.3 | 65.3 (no picture) | +24.9 | +24.5 |
| GNOME | Foggy forest, 4928 × 3264 | 324.7 | 245.8 (the same picture at 1920 × 1080) | +78.9 | +78.4 |
| Plasma | Tatami ocean wave (look-and-feel) | 500.8 | 507.6 (Plasma's default look and feel and its wallpaper) | -6.8 | -2.8 |

Set the wallpapers aside and GNOME Shell still holds 150–160 MiB of
anonymous memory, and KWin and plasmashell together about 330, against
about 22 for Aizome's compositor and shell. Most of it is malloc heap:
82 MiB in GNOME Shell's main heap, 55 in KWin's, 97 in plasmashell's
plus 31 in its threads' arenas, and 23–72 MiB more in large anonymous
blocks (big allocations, Mesa's buffers and textures). Aizome's
compositor has 2.5 MiB of heap and 8 MiB of mapped blocks, its shell 4 MiB
of main heap.

What the mappings show, by the categories the question asked about:

- **The JavaScript engines are small.** GNOME Shell's SpiderMonkey heap
  (its 1 MiB-aligned GC chunks) is 6–12 MiB; plasmashell's QML engine
  keeps its GC heap in a named memfd (`JSGCHeap:QtQml`), 13.5 MiB.
  Neither shell's size is its JavaScript: the bulk is the C and C++ heap
  underneath (Clutter, Mutter, Cogl and GObject state; Qt Quick's scene
  graph, items and caches) and what the renderer allocates.
- **GPU driver state, which is llvmpipe here.** In GNOME Shell and both
  Plasma processes llvmpipe maps libLLVM (19–34 MiB of PSS each, file-backed)
  and Mesa's libgallium (7–12 MiB), JIT-compiles its shaders into
  anonymous executable memory (under 1 MiB), keeps textures in ordinary
  heap memory and, in Plasma, shares 10.6 MiB of display buffers between
  plasmashell and KWin (`memfd:mesa-shared`). Aizome's compositor
  composites with pixman (0.25 MiB of library) and its shell draws with
  tiny-skia inside its own binary: no driver at all.
- **The wallpaper is the biggest single item in GNOME Shell.** Mutter
  uploads the background at the image's own size
  (`meta-background-image.c:257-270`), and Sekka's GNOME wallpaper is
  4928 × 3264: 61 MiB of RGBA, 82 with mipmaps, which llvmpipe keeps in
  RAM. Against the same picture cropped to 1920 × 1080, GNOME Shell held
  78 MiB more anonymous memory. Aizome's shell decodes its 6000 × 3750
  wallpaper on a thread, scales it to cover the output and keeps only that
  (`crates/aizome-face/src/wallpaper.rs:1-4`): 8 MiB at 1920 × 1080. Yet
  with it the shell held 24 MiB more than with none, all of it in the
  thread malloc arenas; that the rest is heap kept after decoding the
  23-megapixel original is likely but was not established (design.md saw
  the live shell's arenas grow too). Plasma's look-and-feel wallpaper
  made no difference against Plasma's default one: both are drawn at the
  screen's size.
- **Fonts, icons and theme caches are file-backed and small**: under
  1 MiB of mapped fonts and 0–5 MiB of caches, schemas and resources in
  any of the three (fontconfig caches, GSettings schemas, KDE's `.kcache`
  files and Plasma's SVG caches), all page cache. The icons and glyphs the
  shells have drawn live in their heaps, not in these files.
- **Threads**: llvmpipe adds two rasteriser threads per process, Qt Quick
  a render thread per window and QML loader threads; glibc gives busy
  threads their own malloc arenas, which is where much of plasmashell's
  heap sits.

### Design or configuration

Of the 654 MiB between Aizome and Plasma, about 240 MiB is configuration:
Akonadi, MariaDB and Baloo (and Discover's notifier), there because
NixOS's Plasma module turns on `programs.kde-pim` by default, which Sekka
keeps; Akonadi is D-Bus-activated 7.1–8.8 s after every login. The rest,
about 410 MiB, is Plasma's design in this VM: KWin and plasmashell (292
MiB more anonymous memory than Aizome's pair), the helper daemons (74),
more kernel memory and llvmpipe's shared buffers. Of the 346 MiB between
Aizome and GNOME, about 90 MiB is services a distribution could leave
out (EDS, Online Accounts and LocalSearch, 61 MiB; IBus, 28 MiB), though
GNOME treats them as core and its own calendar starts EDS; the rest,
about 255 MiB, is GNOME Shell (175 MiB more than Aizome's compositor and
shell), the settings daemons and `mutter-x11-frames`. Of GNOME Shell's
share, about 79 MiB comes from the two together: Sekka's choice of a
4928 × 3264 wallpaper and Mutter's keeping it at full size; with a
screen-sized picture GNOME's gap would be about 55 MiB smaller (Aizome
pays 24 for its own).

On a real GPU some of this moves: textures and llvmpipe's buffers go to
video memory, and libLLVM is replaced by the GPU driver's own code (which
is not small either; on runnymede's RTX 5090 the NVIDIA libraries map
about 114 MiB into Aizome's compositor, design.md section 4).

## Login

Times are from the Enter, on the guest's clock, from the journal and the
screendumps (every 100 ms) of three boots each; the per-boot timelines
are in [data/why-desktop/login-timeline-*.txt](data/why-desktop/).
PAM opens the session and systemd starts the user's manager within
0.1–0.25 s in all three; the rest is the desktop. The tables give the
first boot's times; the other two agree within about 0.25 s (Plasma's
splash ended at 4.64, 4.39 and 4.59 s; GNOME Shell finished starting at
1.70, 1.73 and 1.70 s and GDM's greeter shut down at 1.89, 1.94 and
1.83 s).

**Aizome, 1.2 s.**

| s | |
|---:|---|
| 0.10 | PAM session opened (greetd) |
| 0.30 | `aizome.service` starting: the compositor, which starts the shell and xwayland-satellite itself |
| 0.53 | the compositor ready: it notifies systemd once it is listening and has exported the session's environment (`crates/aizome-comp-core/src/session.rs:217-231`) |
| 0.63 | its first frame on screen: the whole screen changes |
| 0.72–0.83 | the shell's notification server and polkit agent registered |
| 0.63–1.13 | the desktop shows its colour while a thread decodes the wallpaper and scales it to the output (`crates/aizome-face/src/wallpaper.rs:1-4`) |
| 1.13–1.23 | the wallpaper lands: the desktop is complete |

The session's processes had used 0.6 s of CPU in all by 65 s after the
Enter (the shell 0.28 s, the compositor 0.07 s).

**GNOME, 2.0 s.**

| s | |
|---:|---|
| 0.03 | PAM session opened (GDM) |
| 0.20 | gnome-session's systemd targets starting |
| 0.24 | GNOME Shell starting: Mutter opens the GPU (falls back to `kms_swrast`, llvmpipe) at 0.30–0.37 s, has its Wayland socket at 0.53–0.55 s |
| 0.84–0.90 | the shell's JavaScript loaded: "Registering session with GDM", the unit reports ready |
| 0.79–1.40 | the shell's calendar server (0.79 s) activates EDS (0.83 s); 18 `gsd-*` daemons, IBus, LocalSearch and the portals start (0.90–1.40 s); IBus's GTK extension alone takes 1.35 s of CPU |
| 1.70 | the shell's startup sequence completes: background loaded, startup animation run, "GNOME Shell started" |
| 1.70 | the shell registers its display with GDM (`js/ui/main.js:343-373`: only in the `startup-complete` handler) |
| 1.83–1.94 | GDM shuts its greeter's GNOME Shell down (that GDM waits for the registration is inferred from this order; GDM's code was not read) |
| 2.04 | the user's overview on screen and settled |

Until 1.74 s the greeter is what is on screen (in one boot the whole
screen changed once at 1.04 s, to neither the greeter nor the desktop):
GNOME shows nothing of the user's session until
its shell has started completely, then switches in one step. The session
had used 3.1 s of CPU by 65 s: IBus's extension 1.35 s, GNOME Shell 1.0 s
(0.7 s on its main thread, 0.3 s in llvmpipe).

**Plasma, 5.2 s.**

| s | |
|---:|---|
| 0.09 | PAM session opened; Plasma Login's own feedback on screen at 0.13 s |
| 0.39–0.41 | startplasma makes systemd reload its units |
| 0.44 | KSplash and KWin starting |
| 0.53 | ksmserver ready; plasmashell starting |
| 1.13 | KWin's Xwayland up (KWin starts it with the session) |
| 1.14 | KWin's first frame: the splash, the whole screen |
| 1.17 | plasmashell on D-Bus |
| 1.26 | kded6 ready, `plasma-core.target` |
| 1.27–2.01 | PowerDevil starting: libddcutil initialising (0.22 s) and its helpers; `plasma-workspace.target` waits for it |
| 2.02–3.43 | autostart: Discover's notifier, Baloo, KUnifiedPush, geoclue, keyring; xdg-desktop-portal takes 1.4 s to come up with its KDE and GTK backends |
| 1.24–4.44 | the splash animates: the screen changes on every 100 ms poll |
| 4.64 | KSplash exits: its sixth stage, "desktop", sent by plasmashell when every desktop containment is UI-ready (`shell/shellcorona.cpp:1520-1536`, `ksplash/ksplashqml/splashapp.cpp:110-114`) |
| 4.74–5.34 | the desktop fades in, then the panel |

Plasma's critical path is plasmashell's own start-up, 0.53 → 4.64 s on two
vCPUs, while KWin composites the animated splash with llvmpipe the whole
time: by 65 s KWin had used 3.9 s of CPU, 3.6 s of it in llvmpipe's two
rasteriser threads, against plasmashell's 2.1 s (1.5 s on its main
thread). The session had used 8.4 s of CPU by then, 1.3–1.5 s of it
Akonadi's and MariaDB's, which started after the desktop had settled.
That the splash's animation is what costs KWin so much is inferred from
the timing (the screen changed on every poll while it was up; KWin's
llvmpipe time has nowhere else to come from in that window); with
software rendering KWin itself turns its own effects' animations off
(`scene/workspacescene.cpp:855-858`), but the splash is a client.

**Before the login: the login screens.** desktops.md found the three
display managers starting at about the same time and the login screens
taking 1.1, 2.1 and 3.3 s to draw after that. The same pattern applies:
GDM's greeter is a full GNOME Shell in greeter mode (251 MiB), Plasma
Login runs KWin, a QML greeter and a separate wallpaper process (371, 199
and 198 MiB), Aizome's greeter session 136 MiB in all. This page does not
time their start-ups step by step.

## Key to screen

### Where the time goes

Per press, from the probe's own records (660 presses per desktop and
condition; [data/why-desktop/latency-phase.json](data/why-desktop/latency-phase.json)):

| ms, median | Aizome idle | Aizome load | GNOME idle | GNOME load | Plasma idle | Plasma load |
|---|---:|---:|---:|---:|---:|---:|
| Key press → the probe has the key | 0.08 | 0.05 | 0.12 | 0.08 | 0.11 | 0.07 |
| → the probe commits its new frame | 0.002 | 0.002 | 0.002 | 0.002 | 0.002 | 0.002 |
| Commit → presented | 6.41 | 6.99 | 10.35 | 12.88 | 9.52 | 13.38 |
| of which: commit → the next vblank | 6.19 | 6.50 | 6.51 | 6.31 | 7.16 | 6.73 |
| Presses that missed that vblank | 2.4% | 2.9% | 27.6% | 47.7% | 24.2% | 50.5% |
| Key → presented | **6.5** | **7.0** | **10.5** | **13.0** | **9.6** | **13.5** |

Presses come at random phases of the 75 Hz refresh, so the wait for the
next vblank is the same for all three (half a refresh, 6.7 ms, on
average). Everything else is the share of presses whose commit missed
that vblank and was shown a whole refresh (13.3 ms) later. That share is
set by how close to the vblank a commit can arrive and still make it.

### The latch point, measured

The share of commits that missed the next vblank, by how long before it
they arrived (presses 75–219 of each run, past Plasma's opening seconds;
0.5 ms bins):

| Commit arrived before the vblank | 0–0.5 ms | 0.5–1 | 1–1.5 | 1.5–2 | 2–3 | 3–3.5 | 3.5–4 | 4–5 | 5–6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Aizome, idle | 10/21 | 0/21 | 0/16 | 0/16 | 0/34 | 0/13 | 0/14 | 0/41 | 0/33 |
| Aizome, load | 11/19 | 0/19 | 1/17 | 0/20 | 0/25 | 0/18 | 0/15 | 0/28 | 0/35 |
| GNOME, idle | 16/16 | 21/21 | 6/6 | 20/20 | 28/31 | 18/24 | 3/18 | 1/30 | 0/27 |
| GNOME, load | 15/15 | 18/18 | 20/20 | 27/27 | 37/37 | 23/23 | 6/7 | 22/32 | 16/28 |
| Plasma, idle | 8/8 | 19/19 | 12/16 | 2/20 | 0/39 | 0/16 | 0/17 | 0/30 | 0/26 |
| Plasma, load | 23/23 | 14/14 | 16/16 | 19/19 | 30/33 | 15/17 | 14/19 | 13/28 | 2/29 |

Aizome makes the vblank if the commit comes at least 0.2 ms before it
(the latest commit that made it came 0.19 ms before, the earliest that
missed 0.20 ms before), idle or loaded. GNOME needs about 3.4 ms at idle
and 4.5–9 ms under load; Plasma about 1.3 ms at idle and about 4 ms
under load.

### Why: each compositor's rule after a commit

- **Aizome draws at once and submits at once.** At the end of every pass
  of its event loop the compositor draws each output whose redraw is
  queued, unless a page flip is still pending
  (`crates/aizome-comp-core/src/state.rs:605`, `backend/udev.rs:420-451`);
  the vblank event clears the pending flip
  (`backend/udev.rs:1087`). There is no predicted deadline and no safety
  margin: pixman composites the damaged 800 × 600 and the atomic commit
  goes to the kernel, which shows it at the next vblank. In the VM that
  took 0.2 ms. (`pacing.rs` only releases FIFO and commit-timing
  barriers, which the probe doesn't use.)
- **Mutter dispatches at once, then waits for a deadline.** An idle frame
  clock updates immediately (`clutter-frame-clock.c:898-923`, `:1038`);
  the shm buffer is applied synchronously on commit and the stage
  scheduled. GNOME Shell then paints with llvmpipe, and the KMS thread
  holds the commit until vblank − (evasion + the vblank's duration),
  where evasion is the longest KMS dispatch it has measured (including
  how late its timer woke) plus 800 µs; a frame that can't make that goes
  to the next vblank (`src/backends/native/meta-kms-crtc.c:31`, `:739-762`,
  `:891-897`, `:1080-1090`). The
  3.4 ms latch is that paint time plus that margin; under load both grow,
  and the margin follows the worst recent lateness, so the latch moves
  out to 9 ms. (The split between paint time and margin was not measured:
  `CLUTTER_DEBUG=frame-timings` in the VM would show it.)
- **KWin composites at a predicted slot, or at once when late.** Its
  render loop aims to start compositing at target vblank − E, where E is
  the render-time estimate plus the DRM backend's safety margin plus 1 ms
  (`core/renderloop.cpp:54-142`, `backends/drm/drm_commit_thread.cpp`);
  a commit that arrives after that slot is composited at once and makes
  the vblank if llvmpipe and the commit finish in time, which in the VM
  meant arriving 1.3 ms or more before it. The estimate is an average plus
  twice a variance that jumps to any overshoot and decays over about 6 s
  (`core/renderjournal.cpp:25-43`); once it reaches a whole refresh, KWin
  (with atomic modesetting) allows a second frame in flight and aims every
  frame two vblanks out, until ten frames in a row come in under 0.95 of
  a refresh (`renderloop.cpp:59-95`, `backends/drm/drm_output.cpp:41-52`).

### Plasma's opening seconds

Plasma's medians are not its steady state. Every one of its six runs
began with a stretch in which every press, whatever its phase, was shown
a refresh late; marking a late press `X` (or `x` if it came within 4 ms
of the vblank) and an on-time one `.`, the start of the first idle run:

```
plasma idle r1  XXXxXXXXXXXXXxXXXXXxXXXxXxXx...............x.....x.....x.......x.x.x......
gnome  idle r1  .X..x.x....xx...........x...xx.xx......xx...xxx...xx....xxx....xx...x....x
```

The stretches lasted 28–46 presses at idle (6–10 s from the first press)
and 44–69 under load (10–15 s), counted to the first three on-time
presses in a row; a late press with time to spare was
followed by another 96% of the time in Plasma, never in GNOME
([data/why-desktop/latency-streaks.json](data/why-desktop/latency-streaks.json)).
After it, Plasma's median was 8.1 ms at idle (0.61 frames; p99 14.9 ms)
and 11.3 ms under load, against Aizome's 6.5 and 7.4 and GNOME's 10.4
and 13.0 over the same presses. The pattern fits KWin's estimator after
the slow first frames of a new window: a variance that takes seconds to
decay and a triple-buffering mode that needs ten fast frames to leave,
when an idle probe gives it about five frames a second. That this is the
mechanism is inferred from the code; `KWIN_LOG_PERFORMANCE_DATA=1` in the
VM would confirm it.

### What the benchmark favours

The probe updates one window, now and then, on an otherwise still
desktop: the case of typing. Aizome's rule (draw as soon as anything
changes) is the best possible rule for that, as long as drawing is fast,
and pixman at 800 × 600 is. It is not free in every case (inferred from
the code, not measured here): a second client committing just after a
frame went out waits for that flip and then for the next vblank, up to
two refreshes, where Mutter and KWin would have batched both into the
frame they were about to start. For a client drawing continuously all
three kept 75 frames a second (desktops.md, frame pacing).

## Idle

desktops.md measured the guests over 120 s: guest CPU within 0.1
percentage points of the bare system for all three, and wakeups of all
threads 100/s with Aizome against 43 with GNOME and 60 with Plasma. By
process ([data/why-desktop/session-cpu.json](data/why-desktop/session-cpu.json)):

| Wakeups a second at idle | Aizome | GNOME | Plasma |
|---|---:|---:|---:|
| Compositor | 6.8 | 1.3 (one process) | 0.3 |
| Shell | 47.1 | | 1.4 |
| Desktop helpers | 8 (four helpers, 2 each) | under 2 in all | 14 (PowerDevil) |
| Configuration services | – | 0.3 (EDS's alarm notifier) | 3.3 (MariaDB) |

Per thread, Aizome's shell wakes for `aizome-sys`'s threads, each of
which checks for a stop request every 250 ms (`crates/aizome-sys/src/bus.rs`,
`recv_timeout`): the seven D-Bus name watchers (28/s), the status and MPRIS
threads (4/s each), rfkill (2/s), plus two blocking-pool threads and the
settings watcher (2/s each). The compositor's are its watchdog (4/s), its
main loop (1.75/s) and its settings watcher (1/s). GNOME Shell wakes
about once a second (its main loop and SpiderMonkey's helper threads),
KWin less than that, plasmashell one to three times (its main and
Wayland event threads). The host runs below show the same threads waking
at the same rates.

None of the three costs measurable CPU at idle in the VM, and on the host
none used more than 1.2 ms of CPU a second at idle. The wakeups are the
one place the comparison goes against Aizome, and they come from its own
timers (polling loops that could block instead), not from how it draws.

## The headless runs on the host

**What they are.** `tests/performance/why-desktop/headless.py` starts one
desktop at a time on the host: GNOME Shell 50.4 (`--headless
--virtual-monitor 1920x1080@75`), KWin 6.8.2 (`--virtual`, 1920 × 1080)
with plasmashell, or Aizome's compositor (`--backend headless`, pixman)
with its shell, each with its VM's system path for plugins, QML modules,
schemas and Sekka's wallpapers (GNOME's foggy forest, Plasma's ocean-wave
look-and-feel, Aizome's Tokyo at dusk). GNOME and Plasma came from this
checkout's Nixpkgs (`0bb2eed`, a day newer than the VMs' `1c89083e`, with
the same GNOME Shell 50.4, Mutter 50.5, KWin and Plasma 6.8.2, Qt 6.12.0,
Frameworks 6.31 and Mesa 26.2.6); Aizome's compositor was the VMs' own
build (`sg59lw6c…`), and its shell and helpers the same source (Aizome is
unchanged since `d7ecbac`) built against the newer Nixpkgs, which the
compositor found on PATH; the harness now pins those too. Each runs in a bubblewrap sandbox: a minimal `/dev` with no
GPU, input or NVIDIA nodes (KWin is given `/dev/udmabuf`, which its
software renderer allocates through), its own PID and network
namespaces, `/proc`, `/tmp` and runtime directory, a private session bus
and an empty stand-in for the system bus, pinned to two host CPUs, with
llvmpipe limited to two threads. So GNOME Shell and KWin render with
llvmpipe and Aizome with pixman, as in the VM.

**What they are not.** They are not a login and not a full session: no
logind, NetworkManager, UPower or PipeWire, so indicators that depend on
them stay empty; services are D-Bus-activated as they are asked for (EDS,
IBus, kded6, Akonadi and MariaDB, the portals all started), but what
only systemd starts (GNOME's `gsd-*` daemons, PowerDevil, ksmserver) does
not run.
Headless outputs have no real vblank: Mutter's virtual monitor reports
presentations without a vblank flag, so its frame clock takes its
fallback path, and KWin's virtual output has no DRM margin or triple
buffering. The runs are therefore used for memory by mapping, idle
wakeups and CPU per update and per window move, and not for latency.

**Phases.** 40 s to settle; every process's `smaps`; 30 s untouched (per
thread wakeups and CPU); a probe client
([commit-probe.c](../../tests/performance/why-desktop/commit-probe.c),
the desktop comparison's probe driven by a timer instead of keys) maps an
800 × 600 window and flips it 120 times, 120–250 ms apart; then the
window is moved 300 times, 16 ms apart, by each desktop's own scripting
(a GLib timer in GNOME Shell through `Shell.Eval`; a KWin script; `aizomectl
move`). Two clean runs of each desktop, alternating, under the shared
measurement lock (the host's 1-minute load average under 0.7 at the start
of each, but one: 6.3 before Aizome's second run, the minute after another
job released the lock; its figures match the first run's); one run of Aizome and of Plasma with no wallpaper and
Plasma's default look and feel, and two of GNOME with the same picture
cropped to 1920 × 1080 (with no picture at all, GNOME Shell never
finishes starting: its startup waits for the background); one run of
each under `perf record` for attribution only. Every figure is the
median of the clean runs, which agreed within 5% in memory.

### Results

#### The whole headless session (MiB PSS; median of clean runs)

| | Compositor + shell | Everything else the session started | Total | Processes |
|---|---:|---:|---:|---:|
| Aizome | 90.3 | 45.5 | 135.8 | 12 |
| GNOME | 324.7 | 170.1 | 494.8 | 22 |
| Plasma | 500.8 | 434.1 | 934.9 | 35 |

#### Largest other processes of the headless sessions (MiB PSS, anonymous; one clean run)

- Aizome: Xwayland 17 (10); xdg-desktop-portal-gtk 13 (6); xdg-desktop-portal 6 (3); aizome-portal 4 (1); xwayland-satellite 4 (1); xdg-permission-store 1 (1); dconf-service 1 (1); dbus-daemon 1 (1)
- GNOME: evolution-source-registry 36 (13); ibus-extension-gtk3 18 (14); xdg-desktop-portal-gnome 16 (9); gnome-control-center-global-shortcuts-provider 15 (9); gjs 15 (6); evolution-addressbook-factory 14 (6); evolution-calendar-factory 12 (5); gnome-shell-calendar-server 10 (4)
- Plasma: mariadbd 77 (57); akonadi_mailfilter_agent 28 (18); akonadi_archivemail_agent 28 (18); akonadi_followupreminder_agent 28 (18); akonadi_unifiedmailbox_agent 27 (18); akonadi_sendlater_agent 27 (18); akonadi_mailmerge_agent 27 (18); kded6 25 (13)

#### Work, headless on the host (two CPUs; median of clean runs)

| | Idle: wakeups/s | Idle: CPU ms/s | One update: CPU ms | One update: wakeups | One move step: CPU ms |
|---|---:|---:|---:|---:|---:|
| Aizome compositor | 6.8 | 0.122 | 0.50 | 2.5 | 0.33 |
| Aizome shell | 47.7 | 0.518 | 0.10 | 10.2 | 0.11 |
| GNOME compositor+shell | 2.0 | 0.595 | 2.10 | 6.1 | 2.21 |
| Plasma compositor | 0.2 | 0.032 | 2.31 | 13.1 | 1.82 |
| Plasma shell | 3.6 | 1.171 | 0.34 | 1.5 | 0.07 |

#### Where an update's and a move step's CPU goes, by thread (ms; mean of clean runs)

| | One update | One move step |
|---|---|---|
| Aizome | aizome: main thread 0.49 | aizome: main thread 0.33; aizome-shell: main thread 0.09 |
| GNOME | gnome-shell: main thread 1.06; gnome-shell: llvmpipe-1 0.70; gnome-shell: llvmpipe-0 0.32 | gnome-shell: llvmpipe-1 1.08; gnome-shell: llvmpipe-0 0.86; gnome-shell: main thread 0.27 |
| Plasma | kwin_wayland: llvmpipe-1 1.00; kwin_wayland: llvmpipe-0 0.80; kwin_wayland: main thread 0.51; plasmashell: main thread 0.21; plasmashell: llvmpipe-1 0.06 | kwin_wayland: llvmpipe-1 0.98; kwin_wayland: llvmpipe-0 0.68; kwin_wayland: main thread 0.16; plasmashell: main thread 0.05 |

#### Who wakes at idle, headless (wakeups/s by thread; one clean run)

- Aizome compositor: aizome-watchdog 4.0; main thread 1.8; aizome-config-w 1.0
- Aizome shell: aizome-sys org. (7 threads) 28.0; aizome-sys audi 5.0; aizome-sys mpri 4.0; aizome-sys stat 4.0; blocking-1 2.0; aizome-config-w (2 threads) 2.0; aizome-sys back 1.0; meters 0.5
- GNOME compositor+shell: JS Helper (4 threads) 1.2; main thread 0.7; gmain 0.2
- Plasma shell: main thread 3.3; WaylandEventThr (2 threads) 0.3; QThread 0.2; plasmas:disk$0 0.1; QSGRenderThread 0.1
- Plasma compositor: main thread 0.2; llvmpipe-1 0.2; llvmpipe-0 0.1
What the host runs add:

- **One update costs Aizome a quarter of what it costs the others.** The
  probe's full-window flip cost Aizome's compositor 0.5 ms of CPU, all on
  its main thread (pixman copies the damaged 800 × 600 into the output);
  GNOME Shell 2.1 ms, half on its main thread and half in llvmpipe's two
  threads; KWin 2.3 ms, 1.8 of it in llvmpipe, and plasmashell another
  0.3 ms for an update that isn't its own. That rendering time, on a VM's
  slower and shared vCPUs, is part of what sits inside GNOME's 3.4 ms and
  KWin's 1.3 ms latch points in the VM (inferred: the VM's own split
  wasn't measured).
- **Moving a window**: 0.4 ms a step for Aizome (compositor and shell,
  not counting `aizomectl`'s own process for each step), 2.2 for GNOME
  Shell and 1.9 for KWin with plasmashell, again mostly llvmpipe
  rasterising what the window uncovers and covers. pixman only copies.
- **Idle on the host matches the VM**: Aizome's shell 48 wakeups a
  second (with one artefact: an audio thread retries 5 times a second
  because the sandbox has no PipeWire; the VM's shell had none), its
  compositor 7; GNOME Shell 2, KWin under 1, plasmashell 4. None of them
  drew a frame at idle.
- **The sessions D-Bus starts are already most of the difference.** With
  nothing but the compositor, the shell and what they ask for over D-Bus,
  the headless sessions came to 136 MiB of PSS for Aizome, 495 for GNOME
  and 935 for Plasma, Plasma's including Akonadi and MariaDB, which were
  D-Bus-activated within 40 s here too.

**Where the CPU goes, by profile** (one `perf record -e task-clock`
run per desktop, the whole sandbox under perf, each phase reported by its
time window; attribution only, its CPU figures are not used above;
[data/why-desktop/headless/](data/why-desktop/headless/)
`*-perf/perf-summary.txt`):

| | One update | A window moving |
|---|---|---|
| Aizome | pixman 54% (39% in `store_scanline_a8b8g8r8`), Aizome's compositor core and smithay 26%, libc 8% | pixman 55%, the compositor core 17%, the shell 18% |
| GNOME | llvmpipe's rasteriser threads 48% (JIT-compiled shaders, `lp_rast_shade_tile`); on the main thread libgallium 23% (17.6% in `convert_ubyte`, converting the client's buffer into a texture), libLLVM 7%, GLib, SpiderMonkey, Mutter, Clutter and Cogl 2–4% each | llvmpipe's threads 87% |
| Plasma | llvmpipe's threads in KWin 66%; KWin's main thread copying the buffer (`memmove`) 4%, libkwin 5%, libgallium 5%; plasmashell 5% | llvmpipe's threads in KWin 88% |

Two things follow. On the CPU, the cost of an update in GNOME Shell and
KWin is llvmpipe emulating a GPU: uploading the client's pixels into a
texture, then rasterising textured triangles over the damaged area; the
compositors' own code is a few percent. Aizome's pixman path does one
copy, and on the host even that was slower than it needs to be:
`store_scanline_a8b8g8r8` is pixman's generic converter, used because the
headless backend's buffer is ABGR8888
(`crates/aizome-comp-core/src/backend/headless.rs:139`) and the client's
is XRGB8888. On DRM, as in the VM, Aizome gives pixman XRGB8888 or
ARGB8888 buffers (`backend/udev.rs:60`), so there the copy is likely
pixman's fast one and an update cheaper than the 0.5 ms above (inferred,
not profiled in the VM).


## What could not be determined

- **Memory by mapping inside the VM.** The VM trials kept each process's
  totals, not its mappings, and the disk images had been deleted; the
  mapping split comes from the host runs, which match the VM's totals
  closely for the compositors and shells but are not the VM.
- **GNOME Shell's JavaScript heap exactly.** GNOME Shell 50 no longer has
  `global.get_memory_info()` (only its struct remains), so the
  SpiderMonkey heap is counted as its 1 MiB-aligned anonymous chunks, a
  heuristic that can include other 1 MiB-aligned allocations.
- **The split of Mutter's 3.4 ms latch** into paint time and KMS
  margin, and **the confirmation that KWin's opening stretch is triple
  buffering**: both need a VM run with `CLUTTER_DEBUG=frame-timings` or
  `KWIN_LOG_PERFORMANCE_DATA=1`, which would mean rebuilding the golden
  disks. The measured facts (where each latch sits, that Plasma's late
  presses come in one opening stretch per run) don't depend on it.
- **Latency on the host.** Neither Mutter's headless backend nor KWin's
  virtual one schedules against a vblank the way their DRM backends do,
  so a host latency figure would measure a different code path.
- **What activates Akonadi** 7–9 s into a Plasma session: the journal
  shows the D-Bus activation but not the caller.
- **`perf` inside the VM** was not used: the attribution above is from the
  host runs and the VM's per-thread counters.
- **Why Aizome's wallpaper costs its shell 24 MiB** where the picture it
  keeps is 8: heap kept by the decoding thread is the likely reason, not
  checked.
- **Aizome's per-update cost in the VM.** The host profile shows pixman's
  slow format-converting path because the headless buffer is ABGR; the
  DRM backend in the VM uses the client's format, so its cost there is
  probably lower, but it wasn't profiled.
- **The login screens' start-up**, which desktops.md times (1.1, 2.1 and
  3.3 s after the display manager starts), was not taken apart here.
- **Physical hardware.** Everything here is llvmpipe or pixman in a VM or
  a sandbox. With a GPU, GNOME Shell's and KWin's composite and texture
  upload move to the GPU, and their latch points should move closer to
  the vblank (inferred); Aizome would composite with GLES.

## Reproduce

From this repository's root:

```sh
# Analyses of the desktop comparison's VM data → docs/performance/data/why-desktop
tests/performance/why-desktop/run.sh analyse

# The headless host runs: pin the packages, then two clean rounds, the
# wallpaper variants and the profiled runs, and the summary tables
# (LOCK and LOCK_BUILD as on a shared machine)
tests/performance/why-desktop/run.sh pins /tmp/why-pins
LOCK=tests/performance/lock-measure tests/performance/why-desktop/run.sh headless /tmp/why-pins /tmp/why-runs 2
```

The tables here were made from the copies in
[data/why-desktop/headless/](data/why-desktop/headless/) (each run's
`record.json.xz`, the probe's records, the session log and, for the
profiled runs, `perf-summary.txt`): `headless-summary.py
docs/performance/data/why-desktop/headless OUT.json` reproduces
`summary.md` there.

**A caution for anyone running compositors in sandboxes.** The first
version of the harness gave the sandbox its own PID namespace but shared
the host's `/tmp` and network namespace. Xwayland display numbers are
chosen through lock files in `/tmp` and abstract sockets: GNOME Shell's
Mutter, inside the sandbox, could not see the live session's Xwayland
(its PID was outside the namespace), took `/tmp/.X1-lock` for a stale
lock and deleted it (`meta-xwayland.c:210-219`), and the sandboxes left
lock files of their own. The live session kept working (its socket was
still bound); the lock file was restored and the stray ones removed. The
harness now gives every sandbox its own `/tmp`, `/proc` and network
namespace. Two more things a sandbox needs: its own `/proc` (without it
glycin's nested bubblewrap test hangs and GNOME Shell never loads its
background, so never finishes starting), and clients started inside its
namespaces (`nsenter`): smithay's Wayland server panics on a client whose
credentials it can't read. With Yama's `ptrace_scope` at 1, `perf` can
only profile its own descendants, so the profiled runs start the whole
sandbox under `perf record`.

| File | What it is |
|---|---|
| `latency-phase.py`, `latency-streaks.py` | Each press's commit phase against its outcome; runs of late presses |
| `memory-split.py`, `memory-gap.py` | Session memory by role and type; the gap in "in use" terms |
| `session-cpu.py`, `login-timeline.py` | CPU and wakeups per process; login timelines from journal and screen |
| `headless.py`, `commit-probe.c`, `default.nix` | The host runs and their probe |
| `headless-summary.py`, `perf-summary.sh`, `run.sh` | Tables from the host runs; perf attribution; the driver |

Data: [data/why-desktop/](data/why-desktop/).
