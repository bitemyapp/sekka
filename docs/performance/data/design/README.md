# Data: how Aizome is built for speed

Raw records for [design.md](../../design.md), all from runnymede on
2 November 2026. Harness: [tests/performance/design](../../../../tests/performance/design).

| File | What it holds | Made by |
|---|---|---|
| `hello-windows.json` | The same window (a label and a button) in Aizome's kit, GTK 4, libadwaita, Qt 6 Widgets and Qt 6 Quick: 10 measured runs each after a warm-up, every measure per run | `window-cost.py` via `run-window-cost.sh` |
| `small-apps.json` | Soroban, GNOME Calculator, KCalc; Tokei, GNOME Clocks; Sumi, GNOME Text Editor, KWrite (6 runs each) | the same |
| `document-apps.json` | Shashin, Loupe, Gwenview (a 6000 × 4000 JPEG); Shiori, Papers, Okular (a 756-page PDF); Hako, Files (Nautilus), Dolphin (a folder of 2,000 files) | the same |
| `monitors.json` | Myaku, GNOME System Monitor, Plasma System Monitor; Kura, Disk Usage Analyzer (Baobab), Filelight; 30 s at rest each | the same |
| `summary.md`, `summary.json` | Medians and ranges of the four records above | `summarize.py` |
| `session-idle.json` | A headless Aizome session (compositor, shell, xwayland-satellite) left alone for 60 s at 1920 × 1080 and 3840 × 2160 (scale 2), three times each: frames rendered, CPU, wakeups and memory per process | `session-idle.py` |
| `frame-bench/` | The compositor's processor time per frame in its own scenarios, GLES on the Radeon and on the RTX 5090 and pixman, at 3840 × 2160, scale 2, three rounds | `run-frame-bench.sh` (`frame_bench`) |
| `scroll-ab/` | The kit's gallery scrolling a 10,000-row list and table, and its editor, with pixels moved (the default) and drawn again (`AIZOME_NO_PIXEL_SCROLL=1`): Sidestep's frame statistics, CPU and socket energy | `scroll-ab.sh` |
| `offscreen/` | A GL client paced by frame callbacks (es2gears_wayland) visible, on another Desk and minimised: its frames, its and the compositor's CPU, and socket energy | `offscreen.sh` |
| `live-session.json` | A snapshot of the machine's own Aizome session while in use: the compositor's frame statistics for its 3840 × 2160 240 Hz display and the CPU time and memory of Aizome's processes (no window titles or identities) | by hand, see design.md |
| `live-session-later.json` | The same session about four hours later, with the compositor's and shell's memory broken down by mapping (anonymous mappings over 8 MiB listed) | by hand, see design.md |
| `live-shell-memory.tsv` | The live shell's and compositor's CPU ticks, threads and PSS parts every five minutes from then on | a sampler loop, see design.md |
| `thread-wakeups.txt` | Per-thread wakeups at rest of Hako, Shiori, Sumi and Tokei in a private sway, and of the live shell | `thread-wakeups.sh` |
| `budget/` | Aizome's own headless frame-budget tests (the kit, the shell, Sumi, Hako, Myaku), release builds, on this machine: their output as printed | `budget.sh` |

In the window records, each run has: `first_frame_ms` (exec to sway's
`window` `new` event), `cpu_to_first_frame_ms` (utime+stime, 10 ms ticks)
and `cpu_to_first_frame_sched_ms` (schedstat of live threads, ns),
`pss_kib`, `rss_kib` and the anonymous, file and shmem parts of PSS,
`threads`, `shared_objects`, `gpu_driver_libs`, and over the idle window
`idle_switches_per_s` and `idle_cpu_sched_ms_per_s`. Idle counts are
summed over live threads, so a thread that exits during the window can
make them slightly negative; the summaries count those as zero.
