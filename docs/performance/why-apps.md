# Why Aizome's apps start faster and hold less, taken apart

[design.md §1–2](design.md#1-the-cost-of-a-window) measured it: the same
hello window maps in 19 ms in Aizome's kit, 43 ms in Qt Widgets, 87 ms in
Qt Quick, 93 ms in GTK 4 and 115 ms in libadwaita, and the kit holds a
third of their private memory. The page's explanation is "one toolkit
drawing on the CPU, almost nothing to load". This write-up measures where
each toolkit's time and memory actually go, down to the library, function
and syscall, and how much of each gap each part explains. Everything was
measured on runnymede on 2 November 2026
([the machine](README.md#the-machine)), with the programs, harness and
environment of design.md.

## The answer

Four things explain the gaps, and drawing on the GPU is only one of them.
Ranked by what they cost summed over the four toolkits:

1. **Starting a GPU API, mostly the Vulkan loader probing every installed
   driver** (166 ms over the four). GTK 4 and libadwaita start Vulkan, and
   the Vulkan loader loads all 14 drivers installed on this machine to ask
   what they offer. Among them, NVIDIA's driver library is loaded and
   unloaded four times at about 8 ms a time, and lavapipe pulls in LLVM.
   That probing alone is 48–52 ms of GTK's 97–123 ms, and the whole Vulkan
   renderer is 65–70 ms. Qt Quick's OpenGL scene graph costs it 32 ms. Qt
   Widgets and the kit start no GPU API.
2. **The toolkit's own set-up** (52 ms), chiefly parsing its theme's CSS:
   6 ms of CPU in GTK 4, 21 ms in libadwaita and 28 ms in GNOME Text
   Editor. On top of that come icon themes, the QML engine and app UI
   built from XML.
3. **Plug-ins the session's environment switches on** (49 ms). Sekka's
   session sets `QT_QPA_PLATFORMTHEME=gtk3`, so every Qt app loads Qt's
   GTK 3 platform theme, which links and initialises GTK 3. That is 23 ms
   of Qt Widgets' 45 ms and of Qt Quick's 93 ms, and 29 ms of KWrite's.
   Without it, Qt Widgets maps in 21.6 ms, within 2.4 ms of the kit.
   (design.md says its runs had this variable unset; they didn't. See
   [the correction](#a-correction-to-designmd).)
4. **Loading and linking 100–200 shared libraries** (43 ms). After exec,
   reaching `main()` takes the kit 0.3 ms and GTK 4 12.1 ms. Most of that
   is the dynamic linker's symbol lookups, plus the kernel's page faults
   for the pages it relocates. libadwaita takes 15.9 ms and KWrite
   26.3 ms.

One thing runs the other way. **The kit spends 12 of its 19 ms on fonts**:
at start it asks fontconfig about all 1,623 font faces installed here,
eagerly and on the main thread. GTK does its font work on a thread of its
own, and Qt only lists the fonts. With one font package installed, the
kit's window is up in 6.8 ms.

Memory follows the same lines. GTK's Vulkan path adds 9.4 MiB of private
memory, most of it relocated pages of GPU drivers, LLVM's above all. Qt
Quick's GL scene graph adds 20.6 MiB, the GTK 3 theme 4.7 MiB in Qt
Widgets, and Qt's font database about 4 MiB. Of the kit's own 4.7 MiB,
2.6 MiB of live heap is eight font files that it reads whole from the
session's font directory. Using the system's copies instead saves
1.9 MiB.

### Startup: contributors to each gap over the kit

Medians of 9–10 runs each, ms from launch to the window mapped, from
[ablate](data/why-apps/ablate.md) and [fonts](data/why-apps/fonts.md).
Each row is measured as noted. "Everything else" is the remainder, split
in [§4](#4-toolkit-set-up-css-icons-qml) by profile.

| Contributor to the gap | GTK 4 | libadwaita | Qt Widgets | Qt Quick | How it was measured |
|---|---:|---:|---:|---:|---|
| First frame (the kit: 19.2) | 96.9 | 123.4 | 44.6 | 92.9 | clean runs |
| **Gap over the kit** | **77.7** | **104.2** | **25.4** | **73.7** | |
| 1. Starting a GPU API | 65.0 | 69.5 | 0 | 31.9 | ablation: `GSK_RENDERER=cairo`, `QT_QUICK_BACKEND=software` |
| … of which the Vulkan loader probing 14 drivers | 48.3 | 52.0 | – | – | ablation: `VK_DRIVER_FILES` = RADV's alone |
| 2. Everything else: toolkit set-up, Wayland, layout, first draw | 10.0 | 27.3 | 1.7 | 12.6 | remainder (each toolkit's, less the kit's 5.8 ms) |
| 3. Plug-ins from the session's environment | ≈0 | ≈0 | 23.7 | 25.0 | ablation: `QT_QPA_PLATFORMTHEME` and `QT_IM_MODULE` unset; for GTK, a11y, portals and gvfs off, each within ±3 ms |
| 4. Loading and linking libraries before `main()` | 11.8 | 15.6 | 5.5 | 9.9 | stamps: `main()` at 12.6, 16.4, 6.3 and 10.7 ms against the kit's 0.8 |
| 5. Font discovery that grows with the installed fonts | −9.1 | −8.2 | −5.5 | −5.7 | ablation: one font package; the kit saves 12.1 ms, the others 3.0–6.6 |

In the real apps (§9) the same parts account for the gaps. GNOME Text
Editor's Vulkan renderer costs 74.4 ms of its 168.9 and KWrite's GTK 3
theme 29.2 of its 111.1. Sumi maps in the kit hello's 19.2 ms and shows
its text about 5 ms later.

### Memory: contributors to each gap over the kit

Private memory (anonymous pages, `Pss_Anon`, the "private" column of
design.md) 3 s after the first frame, medians of 3 runs from
[memablate](data/why-apps/memablate.md).

| Contributor to the gap, MiB | GTK 4 | libadwaita | Qt Widgets | Qt Quick | How it was measured |
|---|---:|---:|---:|---:|---|
| Private memory (the kit: 4.67) | 16.79 | 19.30 | 11.59 | 36.85 | |
| **Gap over the kit** | **12.12** | **14.63** | **6.92** | **32.18** | |
| GPU API: drivers' relocated pages and allocations | 9.36 | 9.66 | 0 | 20.58 | ablation (cairo, software) |
| … of which the 13 Vulkan drivers not used | 4.67 | 5.01 | – | – | ablation (RADV alone) |
| GTK 3 platform theme | – | – | 4.68 | 3.22 | ablation |
| Everything else: the lean run less the kit | 2.68 | 5.22 | 2.21 | 7.37 | lean runs: 7.35, 9.89, 6.88, 12.04 |
| Font database of 1,623 faces (overlaps the rows above) | 0.45 | 0.43 | 4.16 | 3.27 | ablation (one font package) |

The rows add up to the gap within 0.3 MiB, except Qt Quick's (1 MiB
short: its ablations overlap).

[§8](#8-memory-mapping-by-mapping) breaks the same memory down mapping by
mapping: heap, relocated library pages, thread arenas, stacks.

## What is common, and set aside

- **The harness.** Python forks and execs the program in about 0.5 ms
  before its first instruction. Sway then takes 0.4–0.9 ms from the
  client's first commit of a buffer to the IPC event that ends the
  measurement (WAYLAND_DEBUG, [§6](#6-wayland-and-the-first-frame)).
  About 1 ms of every figure is the harness, the kit's included.
- **The compositor round trip.** Every toolkit commits an empty surface
  and waits for sway's first `configure` (about 0.5 ms) before it draws.
  None waits for a frame callback or vblank before its first frame, so
  the 19 ms isn't sway's frame timing.
- **Damage tracking, frame callbacks and the compositor's composition**
  play no part in a first frame.
- **Warm caches.** The page cache holds every file, and Mesa's shader
  cache was filled by each set's warm-up run, as on a desktop in use. A
  first launch after a reboot or a driver update would cost the GPU
  toolkits more, but that wasn't measured.
- **No keyboard.** Headless sway had no input devices, so no toolkit
  compiled a keymap. In a session, each one compiles the compositor's
  keymap at start (xkbcommon for GTK and Qt, kbvm for the kit).
- **fontconfig itself.** Every toolkit here, the kit included, loads
  fontconfig and parses the same 28 configuration files (about 2 ms).
  What differs is how much each asks of it afterwards ([§5](#5-fonts-the-kits-largest-cost)).

## How it was measured

Five instruments, from cheapest to most invasive. Every timing quoted as a
time comes from the clean or ablation runs. Profiles are used only for
shares and for attribution.

- **Clean runs and ablations** (`probe.py`, sets `ablate`, `fonts`):
  design.md's harness and environment (a private headless sway on the
  Radeon, a private D-Bus, the session's environment otherwise), each
  program alternating with the others, 9–10 runs after a warm-up. An
  ablation runs the same program with one variable changed. The
  difference in medians is that part's net cost. Also recorded: every
  thread's CPU time at the first frame (schedstat) and the shared objects
  mapped then.
- **Stamps** (`stamps.so`, set `stamps`): an `LD_PRELOAD` library that
  time-stamps the boundaries between libraries:
  - its constructor (which glibc runs after relocating everything);
  - `main()`;
  - every `dlopen` of a path;
  - EGL, Vulkan, fontconfig, xkbcommon and D-Bus calls;
  - the xdg toplevel's creation and every `wl_surface` attach and commit;
  - waits in `poll` of 0.2 ms or more.

  It intercepts calls made through the PLT and those looked up with
  `dlsym` (libepoxy, fontique, QtDBus). It adds 0.3 ms to the kit and
  2–7 ms to GTK, so stamps give the order and length of phases, not
  headline times.
- **`perf stat`** (set `pstat`): user and system time and page faults to
  the first frame. Programs using libwayland `_exit` right after their
  first commit (`STAMPS_EXIT=1`). The kit is stopped at the window event.
  The kernel samples user and system time at 1 kHz, and the medians are
  over 6–7 runs.
- **Profiles** (`perf record -e cpu-clock:u`, a sample every 100 µs of
  CPU, DWARF call chains, set `perf`). Samples are cut at the window
  event and given a purpose: the nearest frame on the call chain that a
  rule recognises, so a `malloc` inside CSS parsing counts as CSS. They
  also get a mechanism: the library the sample landed in. perf slows a
  start by 15–55% and sees user time only (`perf_event_paranoid=2`); the
  kernel's share comes from `perf stat`. Shares are per run, averaged
  over 3 runs. Also: `strace -f` to the first frame (set `strace`),
  `WAYLAND_DEBUG=1` (set `wldebug`) and `LD_DEBUG=statistics` (set
  `lddebug`).
- **Memory** (sets `mem`, `memablate`, `heaptrack`): each process's
  `/proc/PID/smaps` 3 s after the first frame. Every mapping gets one
  category, and thread stacks are found by each thread's stack pointer.
  heaptrack recorded every allocation still live when the program was
  stopped (GLib, Qt and Rust free nothing on SIGTERM, so that is the live
  heap). heaptrack doesn't follow exec, so Nix's wrappers were unwrapped:
  the real binary ran with the environment its wrapper sets.

**Measured** means a clean or ablation timing, a counter, or an smaps or
heaptrack figure. **Inferred** means a profile share turned into
milliseconds, or a reason given for a cost. The text says which is which.

## 1. Starting a GPU API

**Measured.** Switching GTK's renderer from Vulkan (its default) to cairo
takes 65.0 ms off GTK 4's first frame, 69.5 ms off libadwaita's and
74.4 ms off GNOME Text Editor's. The GL renderer saves 27.7–32.1 ms.
Telling the Vulkan loader about RADV's driver alone
(`VK_DRIVER_FILES=…/radeon_icd.x86_64.json`) saves 48.3, 52.0 and
56.2 ms. Qt Quick's software scene graph saves 31.9 ms.

**What the loader does (stamps, [timeline](data/why-apps/stamps.md)).**
GTK calls `vkEnumerateInstanceExtensionProperties` (46 ms) and then
`vkCreateInstance` (15 ms). On each call the Vulkan loader dlopens every
ICD listed in `/run/opengl-driver/share/vulkan/icd.d` to ask what it
supports. This machine lists 14:

- RADV, lavapipe, dozen, NVIDIA, nouveau;
- Intel and the older Intel driver (hasvk);
- asahi, broadcom, freedreno, panfrost, PowerVR;
- the gfxstream and virtio drivers.

Mesa's libraries stay loaded after the first call. NVIDIA's
`libGLX_nvidia.so.0`, with libnvidia-glcore and libnvidia-gpucomp, is
unloaded each time and loaded again four times, for 32 ms of `dlopen` in
all. Lavapipe's first load maps and relocates LLVM (9 ms). The other
twelve cost 0.3–1.3 ms each. `vkEnumeratePhysicalDevices` (5.5 ms) and
`vkCreateDevice` (1.1 ms) follow. NVIDIA's library isn't mapped once the
window is up, so it contributed no device that GTK kept. GTK also opens
EGL (`libEGL_mesa`, 1.6 ms).

**Where the CPU goes (profile, [gtk4](data/why-apps/attribution.md#gtk4)).**
GTK 4 spends 29.5 ms of user CPU under the Vulkan instance and devices.
Of that:

- 14.9 ms is the dynamic linker relocating the drivers (`do_lookup_x`,
  `_dl_relocate_object`);
- 10.2 ms is the drivers' own code, mostly NVIDIA's initialisers;
- 4.3 ms is libc.

A further 8.5 ms is "GPU driver code (other)". Of that, 3.7 ms is expat
parsing XML: Mesa's driconf files, read by each Mesa driver that
initialises (inferred from the call chains).

**Where the kernel time goes (`perf stat`, [pstat](data/why-apps/pstat.md)).**
To its first frame, GTK 4 spends 61.4 ms in user mode and 38.3 ms in the
kernel, with 18,131 page faults. With RADV alone the kernel's share falls
to 13.4 ms (4,679 faults), and with cairo to 7.3 ms (3,011). So 31 ms of
the Vulkan cost is the kernel mapping, faulting in and unmapping drivers'
pages: about 2 µs for each extra fault, the extra syscalls included.
`strace` (set `strace`) counts 28,894 syscalls before GTK 4's first frame
against the kit's 2,732. Among them are 4,466 `readlink`s and 838 opens
under `/sys` and `/dev`. That is the drivers enumerating DRM devices,
again on each probe (inferred from the paths).

**Qt Quick** opens EGL in `eglGetPlatformDisplay` (10.4 ms, almost all of
it `dlopen` of `libEGL_mesa`, which brings in libgallium and LLVM), then
`eglInitialize` (8.6 ms: radeonsi's screen, driconf, the shader cache)
and `eglCreateContext` (2.8 ms). Its render thread then builds the
scene graph's pipelines. User time to the first frame falls from 68.9 to
51.7 ms with the software backend, and system time from 26.0 to 12.1 ms.

**Inferred: how much of this is particular to runnymede.** NVIDIA's four
loads are here only because this machine has NVIDIA's driver beside
Mesa's. They are about 32 ms of the stamps runs' dlopen time, out of the
48 ms that RADV alone saves. Lavapipe (about 9 ms) and the twelve other
Mesa drivers (about 5 ms) would remain on an AMD-only NixOS system, since
every NixOS Mesa build installs them. With RADV alone, GTK 4's Vulkan
start costs 16.7 ms over cairo (48.6 against 31.9 ms). The memory follows
(§8): the RADV-only runs hold 4.7 MiB less.

## 2. Plug-ins the session switches on

The harness, like design.md's, passes on the session's environment
(less the display). On Sekka that includes:

- `QT_QPA_PLATFORMTHEME=gtk3` and `QT_IM_MODULE=ibus`;
- `GTK_IM_MODULE=wayland`;
- `GIO_EXTRA_MODULES` naming gvfs, glib-networking and dconf;
- `XDG_CURRENT_DESKTOP=Aizome`;
- for the kit, `AIZOME_FONT_DIR`.

**Measured.**

| Ablation | Qt Widgets | Qt Quick | KWrite |
|---|---:|---:|---:|
| `QT_QPA_PLATFORMTHEME` unset (no GTK 3 theme) | −23.1 | −23.2 | −29.2 |
| `QT_IM_MODULE` unset (no IBus plug-in) | −0.6 | −1.8 | −3.3 |
| `QT_ACCESSIBILITY=0` | −0.1 | 0.0 | −4.0 |
| all three ("lean"; Qt Quick also software) | −23.0 | −54.2 | −31.1 |

KWrite varies by ±5 ms run to run, so its small deltas are noise. The
theme costs, in order (stamps):

1. `dlopen` of `libqgtk3.so` (6.4–10.3 ms), which maps GTK 3 and about
   40 more libraries;
2. `gtk_init`, which parses GTK 3's CSS (8.5 ms of user CPU), opens a
   second Wayland connection and makes a synchronous call to the portal's
   Settings interface (0.8 ms, warm);
3. a pango fontconfig thread (5 ms of CPU) and gvfs's D-Bus module.

The profile puts 15.5 ms of Qt Widgets' user CPU under `QGtk3Theme` and
`gtk_init`, beside the 6.4 ms of plug-in loading. With the theme unset,
Qt Widgets holds 4.7 MiB less private memory and maps 57 shared objects
instead of 98.

**GTK.** Turning off accessibility (`GTK_A11Y=none`), the portals
(`GDK_DEBUG=no-portals`) or gvfs (`GIO_EXTRA_MODULES` unset) moved GTK 4,
libadwaita and GNOME Text Editor by −1.1 to +3.8 ms. Each is within the
runs' spread. On a warm private bus these are fast: GTK 4's blocking
D-Bus calls total 1.3 ms before its first frame. They are a portal
property read, `org.a11y.Bus.GetAddress`, `ListActivatableNames` and the
proxies for the Settings, Inhibit and FileTransfer portals. Their CPU
(about 1 ms in the profile) is real but small.

## 3. Loading and linking libraries

**Measured (stamps).** `main()` is reached at:

| | ms | Shared objects at the first frame |
|---|---:|---:|
| the kit | 0.8 | 13 |
| Sumi | 0.9 | 13 |
| Qt Widgets | 6.3 | 98 |
| Qt Quick | 10.7 | 156 |
| GTK 4 | 12.6 | 131 |
| libadwaita | 16.4 | 152 |
| GNOME Text Editor | 17.9 | 171 |
| KWrite | 26.8 | 194 |

About 0.5 ms of each is the harness's fork and exec. For the GTK and Qt
programs, Nix's binary wrapper runs first: a small C program that sets
`XDG_DATA_DIRS`, `QT_PLUGIN_PATH` and the like, and execs the real binary
0.05 ms after it starts.

**What the time is (profile and counts).**

- The dynamic linker's user time before `main()` is 8.0 ms for GTK 4,
  10.9 ms for libadwaita, 3.4 ms for Qt Widgets, 6.2 ms for Qt Quick,
  11.9 ms for GNOME Text Editor and 19.6 ms for KWrite.
- It is mostly symbol lookup (`do_lookup_x`, `check_match`, `strcmp`)
  through a global scope of 100–190 libraries.
- `LD_DEBUG=statistics` ([lddebug](data/why-apps/lddebug.md)) counts
  31,011 symbol relocations and 81,917 relative relocations for GTK 4,
  and 59,785 and 181,419 for KWrite. The kit has 328, and its Rust code is
  one statically linked binary. (ld.so's own timings, read as TSC cycles
  at 4.7 GHz, came out 6–7× lower than the profile and the stamps agree
  on, so they aren't used.)
- The rest is kernel time: mapping each library (856 `mmap`s before GTK
  4's first frame, all told) and the page faults of relocating.

**Ablation.** `LD_BIND_NOW=1` adds 5.6 ms to GTK 4 and 3.7 ms to
libadwaita by resolving lazy PLT entries up front. That is a cost the
lazy default puts off and partly avoids. For Qt and the kit the change
was within noise.

**Missed lookups.** Long search paths add `openat` calls that fail with
ENOENT before the first frame: 415 for the kit (fontconfig's directories),
1,739 for GTK 4 and 2,373 for KWrite. Nix's per-package paths lengthen
them, but each costs about a microsecond, so together they are under
2 ms (inferred).

## 4. Toolkit set-up: CSS, icons, QML

This is the "everything else" row: GTK 4's 15.8 ms, libadwaita's 33.1,
Qt Widgets' 7.5 and Qt Quick's 18.4, against the kit's 5.8. The profiles
of the lean runs split the user CPU like this
([attribution](data/why-apps/attribution.md)):

| User CPU under perf, ms | GTK 4 lean | libadwaita lean | Qt Widgets lean | Qt Quick lean |
|---|---:|---:|---:|---:|
| CSS, theme and style | 6.3 | 21.3 | – | – |
| fontconfig (GTK: on Pango's thread) | 4.8 | 5.8 | 7.1 | 6.8 |
| icon theme and images | 1.9 | 4.2 | – | – |
| shaping and glyphs (HarfBuzz, FreeType, Qt's font engine) | 0.9 | 1.7 | 2.2 | 2.4 |
| Wayland and keymaps | 1.2 | 1.2 | 0.7 | 1.5 |
| GObject type and class set-up | 0.7 | 0.9 | – | – |
| QML engine and Quick Controls | – | – | – | 8.1 |
| Qt plug-ins (platform, shell integration) | – | – | 0.8 | 2.9 |
| layout and drawing | 0.3 | 1.1 | 0.2 | 0.1 |

- **libadwaita's stylesheet** is the largest single item. Parsing and
  applying it takes 21 ms of CPU against GTK's default theme's 6.3. It
  explains most of libadwaita's 26.5 ms over GTK 4, with 3.8 ms more
  linking and 2.3 ms more icon work.
- **Qt Quick's QML engine** loads QtQuick.Controls with the Fusion style
  (`QT_QUICK_CONTROLS_STYLE=Basic` saved only 1.4 ms), and its type
  loader runs on a QML thread (10.7 ms of CPU) that the main thread waits
  for. The main thread is off the CPU for 24 ms of Qt Quick's 93: waiting
  on that thread and on the render thread.
- **Layout and the first draw are small everywhere**: 0.1–1.1 ms of CPU
  for a label and a button, GTK's cairo path included.

## 5. Fonts: the kit's largest cost

**Measured.** The fonts set ran every program with `FONTCONFIG_FILE`
naming a configuration with the system's own rules (`conf.d`) and one
font package, DejaVu, instead of the 22 packages and 1,623 faces
installed here. Time to the first frame:

| Program | All fonts | DejaVu only | Δ |
|---|---:|---:|---:|
| the kit | 18.9 | 6.8 | −12.1 |
| Sumi | 19.1 | 6.9 | −12.3 |
| Qt Widgets | 43.1 | 36.5 | −6.6 |
| Qt Quick | 87.8 | 81.4 | −6.4 |
| KWrite | 104.1 | 94.2 | −9.9 |
| GTK 4 | 96.6 | 93.6 | −3.0 |
| libadwaita | 120.6 | 116.7 | −3.9 |
| GNOME Text Editor | 160.8 | 160.5 | −0.3 |

**Why the kit pays most (stamps and profile).** The kit's text engine
(Sukumo's, on parley and fontique) opens fontique's font collection on
the main thread before it connects to Wayland. fontique's
`SystemFonts::new`:

1. loads fontconfig's configuration (`FcInitLoadConfig`, 28 files of XML:
   1.9 ms of CPU) and caches;
2. lists every font (`FcConfigGetFonts`) and builds a family-name map
   over all 1,623 patterns;
3. for each of eight generic families (serif, sans-serif, monospace,
   cursive, fantasy, system-ui, emoji, math), calls `FcConfigSubstitute`
   and a trimmed `FcFontSort` (0.75–0.9 ms each, returning 143–188
   fonts). It then calls `FcFontRenderPrepare` on every font returned, to
   build the fallback lists.

A ninth, untrimmed `FcFontSort` (0.45 ms) returns all 1,623 faces.

In the profile, 14.2 ms of the kit's 15.7 ms of user CPU is fontconfig:

- `FcFontSort`: 6.8 ms;
- `FcFontRenderPrepare`: 3.1 ms;
- configuration: 1.9 ms;
- the name map, substitutions and freeing the patterns: the rest.

The stamps show the main thread in fontconfig from 1.3 ms to about
17.6 ms of a 19.5 ms start, and in two `FcFontMatch` calls (0.5 ms) just
before the window.

GTK runs fontconfig on Pango's own thread (`[pango] fontconfig`, 4.8 ms
of CPU) while its main thread does other work, and asks for one family
when it lays out text. Qt lists the fonts once (`FcFontList`, 2.2 ms)
and fills its font database from them on the main thread (6.6 ms in all,
by the ablation).

**Inferred.** The kit's eager, synchronous fallback discovery is the one
place it does more than GTK. The work doesn't depend on the app, which is
why every Aizome app maps in about 19 ms whatever it opens: they all pay
it first. It scales with the fonts installed, and here that is 1,623
faces, CJK and Noto included. Doing it lazily per generic family, or on a
thread as Sukumo's AppKit path already does (`fonts::prewarm`), would
take most of 12 ms off every Aizome app's start.

## 6. Wayland and the first frame

**Measured (WAYLAND_DEBUG, [wldebug](data/why-apps/wldebug.md)).**

| ms from launch | the kit | GTK 4 | Qt Widgets | Qt Quick |
|---|---:|---:|---:|---:|
| `wl_display.get_registry` (connected) | 17.5–18.0 | 15.1–16.2 | 7.3–7.5 | 12.3 |
| xdg toplevel created | 18.5–18.9 | 90.9–99.1 | 43.1–43.7 | 77.6–80.1 |
| first buffer attached | 19.1–19.6 | 95.5–105.5 | 43.6–44.3 | 88.6–91.9 |
| sway's window event (the measurement) | 20.0–20.4 | 95.9–106.1 | 44.2–45.2 | 89.0–92.3 |

- From connecting to its first buffer, the kit takes 1.6 ms (with debug
  printing). That covers the registry, an shm pool, the toplevel, one
  `configure` round trip of about 0.5 ms, layout, rasterising
  480 × 200 with tiny-skia, and attaching to a subsurface and the
  toplevel.
- GTK connects early and then spends 75–83 ms before it creates the
  toplevel, mostly in the Vulkan device (§1).
- Every toolkit's first commit is followed by sway's event 0.4–1.2 ms
  later, which is the harness's share.

## 7. Where Aizome's own 19 ms and 4.7 MiB go

**Time (clean 19.2 ms; stamps, profile, `perf stat`).**

| Part | ms | How it was found |
|---|---:|---|
| Font discovery that grows with the installed faces | 12.1 | ablation (one font package: 6.8 ms) |
| The rest of the start-up | ≈3.7 | the remainder of the one-font run |
| The window: Wayland, layout, first draw, commit | ≈1.6 | WAYLAND_DEBUG, stamps |
| Harness: fork and exec, then sway's event after the commit | ≈1.2 | stamps, WAYLAND_DEBUG |
| exec and linking 13 shared objects | 0.3 | stamps (first constructor at 0.8 ms from the fork) |
| Kernel time inside the above | (3.2) | `perf stat`: 16.7 ms user, 3.2 ms system, 1,221 page faults |

The rest of the start-up is fontconfig's configuration (1.9 ms of CPU),
reading the session's eight font files, the Rust runtime and the
settings file. The parts add up to 18.9 ms, the fonts set's median.

- **CPU-bound, on one thread.** 18.2 of the 19 ms of CPU is the main
  thread. The render thread uses 0.6 ms and the settings watcher 0.2. The
  main thread is off the CPU for about 1 ms, which is the `configure`
  round trip and file reads (inferred).
- **Not the compositor.** No frame callback or vblank is awaited before
  the first commit, and sway maps the window on that commit.
- **The harness inflates the figure by about 1 ms**, as it does every
  toolkit's.
- **Not drawing.** Raster, layout and the commit are under 2 ms. The
  kit's 19 ms is a font start-up cost, not a drawing cost.

**Memory (4.67 MiB private; smaps and heaptrack, [memory](data/why-apps/memory.md)).**

| Mapping | Private, MiB |
|---|---:|
| malloc's main arena (`[heap]`) | 2.43 |
| other anonymous mappings (malloc's large chunks) | 1.41 |
| the binary's relocated data (.data.rel.ro, GOT) | 0.32 |
| thread stacks (5 threads) | 0.20 |
| shared libraries' relocated and written pages | 0.18 |
| per-thread malloc arenas | 0.12 |

heaptrack finds 3.57 MiB live in malloc 3 s after the first frame:

- **2.6 MiB is the session's fonts, read whole into the heap.** With
  `AIZOME_FONT_DIR` set, as Aizome's session sets it, `aizome_kit::fonts`
  installs eight font files with `std::fs::read`: Archivo roman and
  italic (variable), five IBM Plex Mono faces and Noto Sans Symbols 2.
  Files over malloc's 128 KiB threshold come from `mmap`, which is why
  the "other anonymous" mappings hold 1.4 MiB.
- fontconfig's configuration and patterns hold 0.63 MiB.
- Everything else (the window, layout, the toolkit's state) holds about
  0.3 MiB.

The window's 480 × 200 buffer is shared memory with sway (0.37 MiB RSS,
counted as shmem, not private).

**Measured: the two font costs apart** ([kitfonts](data/why-apps/kitfonts.md),
8 runs each):

| The kit's hello | First frame, ms | Private, MiB |
|---|---:|---:|
| as measured (the session's `AIZOME_FONT_DIR`, 1,623 system faces) | 19.2 | 4.66 |
| `AIZOME_FONT_DIR` unset (the system's copies of the families) | 20.6 | 2.79 |
| one font package | 6.9 | 4.14 |
| one font package, `AIZOME_FONT_DIR` unset | 6.1 | 1.57 |

Sumi moves the same way: 18.9, 20.8, 6.7 and 5.9 ms; 5.59, 3.06, 5.03 and
2.46 MiB.

Reading the session's fonts into memory costs 1.9 MiB of private memory
and saves 1.4 ms, because the kit then needn't ask fontconfig whether the
system has each family. Mapping the files instead of reading them would
keep the time and drop most of the memory: the pages would be file-backed
and shared with every other Aizome app (inferred). Without both font
costs, the kit's window is up in 6 ms with 1.6 MiB of private memory.

## 8. Memory, mapping by mapping

Private (anonymous) MiB 3 s after the first frame, one run each from the
`mem` set (smaps, every mapping categorised). GNOME Text Editor varies
most between runs, 26–31 MiB; the medians are in §9.

| Category | kit | GTK 4 | libadwaita | Qt Widgets | Qt Quick | Sumi | GNOME Text Editor | KWrite |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| malloc main arena (`[heap]`) | 2.43 | 6.72 | 7.54 | 6.75 | 8.40 | 2.86 | 17.45 | 13.54 |
| libraries' relocated read-only data (RELRO: .data.rel.ro, GOT) | 0.13 | 6.22 | 7.32 | 2.25 | 6.70 | 0.13 | 7.54 | 3.89 |
| libraries' writable data (.data, .bss) | 0.05 | 0.98 | 1.12 | 0.62 | 1.24 | 0.05 | 1.19 | 1.19 |
| per-thread malloc arenas | 0.12 | 1.08 | 1.09 | 1.11 | 17.17 | 0.22 | 1.97 | 1.19 |
| other anonymous (large malloc chunks, drivers' mmaps) | 1.41 | 1.49 | 1.89 | 0.64 | 2.30 | 1.39 | 2.11 | 1.03 |
| thread stacks | 0.20 | 0.29 | 0.32 | 0.20 | 0.39 | 0.36 | 0.45 | 0.48 |
| the program's own binary | 0.32 | 0.01 | 0.01 | 0.01 | 0.01 | 0.57 | 0.02 | 0.01 |
| **total** | **4.67** | **16.79** | **19.29** | **11.58** | **36.83** | **5.58** | **30.73** | **21.32** |

**Relocated pages of GPU drivers are GTK's largest gap.** Relocating a
library writes its pointers (.data.rel.ro and the GOT), and each page
written becomes private to the process. In GTK 4 that is 6.2 MiB, of
which the GPU stack is 5.1 MiB:

| Library | MiB |
|---|---:|
| libLLVM | 2.07 |
| libgallium | 1.57 |
| libvulkan_radeon | 0.41 |
| libvulkan_lvp | 0.37 |
| libvulkan_dzn | 0.31 |
| libgtk-4 | 0.26 |

LLVM comes in with both lavapipe and libgallium (which GTK loads for EGL),
and is relocated once. Qt Quick's GL path carries 4.0 MiB of such pages.
Qt's own libraries carry 0.7–2.1 MiB, and the kit's whole binary
0.32 MiB.

**Qt Quick's thread arenas, 17.2 MiB**, are malloc arenas of its QML and
render threads. They are the bulk of the 20.6 MiB the software backend
saves.

**The heaps** hold, by heaptrack:

| Live in malloc 3 s after the first frame, MiB | kit | GTK 4 | libadwaita | Qt Widgets | Qt Quick | GNOME Text Editor | KWrite |
|---|---:|---:|---:|---:|---:|---:|---:|
| **live** | **3.57** | **4.22** | **4.96** | **4.81** | **21.50** | **12.02** | **3.84** |
| peak during the start | 3.78 | 7.59 | 8.77 | 6.46 | 23.37 | 17.96 | 15.57 |
| fontconfig's configuration and font sets | 0.63 | 1.58 | 1.59 | 1.59 | 1.59 | 1.66 | 1.59 |
| other fonts: the kit's session fonts read whole; glyph caches; Qt's font database | 2.63 | 0.15 | 0.24 | 1.04 | 1.12 | 0.76 | 0.22 |
| CSS and style (Qt: GTK 3's, from the theme) | – | 0.59 | 1.16 | 0.90 | 0.86 | 1.80 | 0.85 |
| GPU drivers (instance, context, pipelines, frame) | – | 0.67 | 0.66 | – | 13.16 | 0.80 | – |
| QML engine | – | – | – | – | 2.60 | – | – |
| layout, drawing, the app's state, UI from XML, the editor | 0.20 | 0.40 | 0.49 | 0.23 | 0.22 | 4.61 | 0.39 |

- **fontconfig.** Every GTK and Qt program keeps about 1.6 MiB of it, a
  configuration with the font set behind Pango's or Qt's font list. The
  kit keeps 0.63 MiB, and Qt adds about 1 MiB for its font database.
- **Qt Quick's GL path holds 13.2 MiB of the driver's heap.** radeonsi's
  context and the scene graph's first frame (its shader compiles among
  them) allocate it on the render thread, and it is most of the 17.2 MiB
  in thread arenas. GTK's Vulkan path, by contrast, holds 0.7 MiB of heap.
  Its cost is relocated pages.
- **Memory freed after the start isn't given back.** KWrite's `[heap]`
  holds 13.5 MiB with 3.8 MiB live: its start peaks at 15.6 MiB, and
  glibc's malloc keeps the freed space mapped. The same holds for GNOME
  Text Editor (17.5 MiB, 12.0 live) and GTK 4 (6.7 MiB, 4.2 live). The
  kit's malloc mappings hold 4.0 MiB with 3.6 MiB live, and its start
  peaks 0.2 MiB above what it keeps.

**PSS, the shared side.** The kit maps 7.5 MiB of its own code. GTK 4
maps 11.1 MiB of library code and 14.7 MiB of read-only library data,
counted in full while one process maps them. In a session with many GTK
or Qt apps those shares shrink. The kit's binary is shared only between
copies of the same app (design.md, "Static binaries don't share code").

## 9. The same for a real app: Sumi, GNOME Text Editor, KWrite

Each opens the same 2,514-line Rust file (design.md's `editor-fixture.rs`).

| Contributor, ms | Sumi | GNOME Text Editor | KWrite |
|---|---:|---:|---:|
| First frame (clean) | 19.2 | 168.9 | 111.1 |
| Starting a GPU API | 0 | 74.4 (56.2 of it the Vulkan loader's probing) | 0 |
| Session plug-ins | 0 | ≈0 | 31.1 (GTK 3 theme 29.2) |
| Loading and linking before `main()` | 0.4 | 17.4 | 26.3 |
| Font discovery that grows with the fonts | 12.3 | 0.3 | 9.9 |
| Everything else, harness included | 6.5 | 76.8 | 43.8 |
| Private memory, MiB (memablate medians) | 5.59 | 26.46 | 21.32 |
| … with the GPU path or the GTK 3 theme taken away | – | 19.18 | 16.76 |

- **Sumi maps as soon as the kit's hello, but its text comes one frame
  later.** WAYLAND_DEBUG shows Sumi's toplevel buffer attached at 19.7–20.0
  ms, which maps the window. The editor's content subsurface follows
  4.9 ms later (24.8–24.9 ms), after sway's first sized `configure`. So
  Sumi's text is on screen at about 24 ms in a clean run (inferred: the
  debug runs ran about 1 ms slow). The hello attaches both buffers
  0.2 ms apart. Sumi holds 0.9 MiB more than the hello: the document and
  its lines. GNOME Text Editor and KWrite also send further frames
  (KWrite within 2 ms, GNOME Text Editor 4–30 ms later). Which frame
  first shows the text wasn't checked for them.
- **GNOME Text Editor's "everything else" is libadwaita plus its own UI.**
  Its user CPU (lean profile) is:

  | Part | ms |
  |---|---:|
  | CSS (libadwaita, GtkSourceView's style schemes) | 28.0 |
  | fontconfig on Pango's thread | 8.5 |
  | shaping | 5.3 |
  | its window and pages built from GtkBuilder templates | 4.8 |
  | icons | 4.6 |
  | the app's own set-up | 3.3 |
  | GtkSourceView and spell checking (enchant) | 3.2 |
  | GObject class set-up | 2.6 |

  Its heap is the largest, at 17.5 MiB.
- **KWrite's** is KDE Frameworks'. Its user CPU (lean profile) is:

  | Part | ms |
  |---|---:|
  | linking 194 libraries before `main()` | 19.8 |
  | Qt's font database (on the main thread) | 8.4 |
  | KTextEditor (syntax definitions and themes, the buffer's load, its scripts, pcre2's JIT) | 7.2 |
  | plug-ins (the editor part, Sonnet's spell-checker back-ends, icon engines) | 5.5 |
  | shaping | 4.9 |
  | KConfig | 2.8 |
  | KXMLGUI's XML | 2.5 |

## A correction to design.md

design.md §2 says KDE's apps ran "without Plasma platform integration
(`QT_QPA_PLATFORMTHEME` unset)". The harness passed on the session's
`QT_QPA_PLATFORMTHEME=gtk3`, so every Qt program, the hellos included,
loaded Qt's GTK 3 platform theme. The shared objects agree: 98 for Qt
Widgets in design.md's record and here, and 57 with the variable unset.
That adds 23 ms to the Qt hellos and 29 ms to KWrite. Without it, Qt
Widgets' window maps in 21.6 ms and Qt Quick's in 69.7 ms. That is how
Qt apps start on Sekka's session, so design.md's figures stand as a
measurement of this desktop. The sentence describing its environment is
wrong.

## Caveats

- **One machine, one session's environment.** The NVIDIA ICD's four loads
  are specific to a machine with NVIDIA's driver beside Mesa's. The GTK 3
  theme, IBus and gvfs are Sekka's session choices. The 1,623 fonts are
  this system's. Each is measured separately so it can be set aside.
- **Ablations are net, and not exactly additive.** The cairo renderer
  does its own drawing, and removing the GTK 3 theme also removes a
  fontconfig thread. The "everything else" rows are remainders.
- **Profiles inflate.** perf made starts 15–55% slower and the stamps
  2–7% (GTK). Milliseconds from profiles are shares of a profiled run.
  The tables say where a figure is a share.
- **Lazy work after the first frame isn't counted.** A toolkit that defers
  work past its first frame looks cheaper here than over the next second.
  GTK's fontconfig thread does, and so does Sumi, whose text arrives
  about 5 ms after its window maps.
- **Warm caches only**; no keyboard; headless sway on the Radeon.

## Reproducing

From the repository root, with design.md's artefacts built
([Reproducing](design.md#reproducing)): `/tmp/aizome`, `/tmp/hello`,
`/tmp/kit/release/hello-kit` and the fixtures in `$F`. The harness needs
nothing else: it builds `stamps.so` and finds sway, perf, strace and
heaptrack in Sekka's Nixpkgs.

```sh
# Every set (about 45 minutes of runs; LOCK, optional, wraps each set).
tests/performance/why-apps/run.sh /tmp/why /tmp/hello /tmp/kit/release/hello-kit /tmp/aizome $F
# Or some: ablate stamps perf mem memablate heaptrack strace wldebug fonts lddebug pstat perffonts kitfonts
tests/performance/why-apps/run.sh /tmp/why /tmp/hello /tmp/kit/release/hello-kit /tmp/aizome $F ablate fonts

# Summaries (python3 from nixpkgs; perf and heaptrack_print on PATH for the last two).
W=tests/performance/why-apps
python3 $W/summarize.py /tmp/why/ablate           # first frames, CPU, Δ against each base
python3 $W/timeline.py /tmp/why/stamps            # milestones and spans; --run REP for one run
python3 $W/pstat.py /tmp/why/pstat                # user, system, page faults to the first frame
python3 $W/syscalls.py /tmp/why/strace            # syscalls to the first frame
python3 $W/memory.py /tmp/why/mem kit gtk4 ...    # smaps by category
python3 $W/attribute.py /tmp/why/perf gtk4 --folded gtk4.folded   # CPU by purpose and mechanism
python3 $W/heapattr.py /tmp/why/heaptrack kit gtk4 ...             # live heap by purpose
python3 $W/collect.py /tmp/why docs/performance/data/why-apps     # compact records for the repository
```

To run one program in the same private sway by hand:
`tests/performance/why-apps/with-sway.sh CMD` (sway, swaymsg and
dbus-daemon on PATH).

## Data

In [data/why-apps](data/why-apps) (see its README): the summaries quoted
here, every run's compact record (`SET.runs.jsonl.xz`), the stamps of the
`stamps` set, and the profiles as folded stacks (`folded/*.folded.xz`,
for flame graphs).
