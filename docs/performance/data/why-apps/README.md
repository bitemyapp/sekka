# Data: why Aizome's apps start faster and hold less

Records for [why-apps.md](../../why-apps.md), all from runnymede on
2 November 2026 (20:49–21:25 Central), made by
[tests/performance/why-apps](../../../../tests/performance/why-apps)
(`run.sh`, then the summaries named below). The programs are design.md's:
the five hello windows (kit, gtk4, adw, qtwidgets, qtquick) and three
editors opening `editor-fixture.rs` (sumi, gnome-text-editor, kwrite).

| File | What it holds | Made by |
|---|---|---|
| `ablate.md`, `ablate.json` | First frame, CPU at it (all threads, main thread), the main thread's time off the CPU, shared objects mapped; each program as measured and with one part taken away; 10 runs each | `summarize.py` on the `ablate` set |
| `fonts.md` | The same, as measured and with one font package (DejaVu) in place of the system's 1,623 faces | `summarize.py` on `fonts` |
| `kitfonts.md` | The kit's hello and Sumi with and without the session's `AIZOME_FONT_DIR`, and with one font package: time and memory | `summarize.py` on `kitfonts` |
| `stamps.md` | Milestones (constructor, `main()`, toplevel, first commit) and spans (dlopen by kind, Vulkan, EGL, fontconfig, D-Bus, Wayland) of each start, medians of 5 runs; then one run's every stamp per program | `timeline.py` on `stamps` |
| `stamps/` | stamps.so's raw lines for every run of the `stamps` set (ns CLOCK_MONOTONIC, pid, tid, event, duration ns, detail) | `stamps.so` via `collect.py` |
| `pstat.md`, `pstat.json` | perf stat to the first frame: task-clock, user and system time, page faults, user instructions; as measured, lean, and with GPU ablations | `pstat.py` on `pstat` |
| `attribution.md` | User CPU to the first frame by purpose and by the library it ran in, per program (as measured, lean, and the kit's with one font package) | `attribute.py` on `perf`, `perffonts` |
| `folded/NAME.folded.xz` | The same samples as folded stacks (`program;thread;purpose;frame;…;leaf count`, one sample = 0.1 ms of CPU, 3 runs summed), for flame graphs | `attribute.py --folded` |
| `folded/NAME.heap.folded.xz` | heaptrack's live allocations 3 s after the first frame as folded stacks weighted in bytes | `heapattr.py` (`heaptrack_print -F`) |
| `heap.md` | The live heap by purpose, with heaptrack's peak and totals | `heapattr.py` on `heaptrack` |
| `memory.md`, `memory.json` | Every mapping of every process 3 s after the first frame, by category, and the shared objects by group and by dirty pages | `memory.py` on `mem` |
| `memablate.md` | smaps_rollup (PSS, private, file, shmem, RSS) as measured and with the costly parts taken away, 3 runs each | `probe.py`'s `memablate` set |
| `syscalls.md` | Syscalls to the first frame by kind, files opened by kind, failed opens, sockets connected (under strace) | `syscalls.py` on `strace` |
| `wldebug.md` | Wayland milestones (connected, toplevel, first buffer) from WAYLAND_DEBUG beside sway's event | `wayland.py` on `wldebug` |
| `lddebug.md` | The dynamic linker's start-up relocation counts (LD_DEBUG=statistics) | `lddebug.py` on `lddebug` |
| `SET.runs.jsonl.xz` | Every run of every set, compact: variant, rep, first frame, each thread's name and CPU (ms) at it, shared objects then, smaps_rollup after settling; the session's environment and the Nix wrappers' path values left out | `collect.py` |

A variant is `PROGRAM.ABLATION`. The ablations:

| Ablation | What it changes |
|---|---|
| `cairo`, `gl` | `GSK_RENDERER` |
| `radv-only` | `VK_DRIVER_FILES=/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json` |
| `no-a11y` | `GTK_A11Y=none` or `QT_ACCESSIBILITY=0` |
| `no-portals` | `GDK_DEBUG=no-portals` |
| `no-gvfs` | `GIO_EXTRA_MODULES` unset |
| `no-gtk3-theme` | `QT_QPA_PLATFORMTHEME` unset |
| `no-ibus` | `QT_IM_MODULE` unset |
| `software` | `QT_QUICK_BACKEND=software` |
| `basic-style` | `QT_QUICK_CONTROLS_STYLE=Basic` |
| `bind-now` | `LD_BIND_NOW=1` |
| `dejavu-only` | `FONTCONFIG_FILE`: the system's rules with DejaVu alone |
| `no-font-dir` | `AIZOME_FONT_DIR` unset |
| `lean` | GTK: cairo, no a11y, no portals, no gvfs. Qt: no GTK 3 theme, no IBus, no a11y (Qt Quick also software) |

The first run of each variant in a set is a warm-up, marked `"warmup": true`
and left out of every summary. A kit warm-up in a fresh private home takes
about 80 ms.
