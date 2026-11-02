# Terminal comparison data

Raw records behind [`../../terminals.md`](../../terminals.md), from
`tests/performance/terminals/` on runnymede (Ryzen 7 9800X3D, private
headless sway on the integrated Radeon).

- `summary.json` — `terminal-summary.py`'s output: per terminal and phase,
  medians with min/max (and quartiles), the accepted trial counts and every
  rejected trial with its reasons. The write-up's tables come from it.
- `summary.md` — the same as Markdown tables, as printed.
- `environment.json` — store paths, versions and closure sizes of every
  terminal, the Sekka commit, the tools (sway, grim, dbus, Python, cc) and
  the host's CPU, governor and GPU driver.
- `calibration.json` — each terminal's calibrated outer window for a 120×40
  grid, its cell size, its processes and executables, its GPU evidence
  (libraries mapped, DRM devices held, GTK's renderer line), the exact
  configuration files and dconf keyfile, the font files' hashes and the
  list of files in its warm cache template.
- `PHASE.tar.xz` — one per matrix run (`startup`, `steady`, `throughput`,
  `energy-a`, `energy-b`, `idle-default-a`, `idle-default-b`): the matrix's
  `result.json` (preflights, activity, acceptance), the tools as they ran,
  and for each trial its `result.json`, `power.tsv` (PPT samples: monotonic
  ns, mW), `fixture.jsonl`, `observer.jsonl` (captures and keys),
  `activity.jsonl`, application, bus and compositor logs, and for the
  first round a final 1920×1440 capture of the private output (fixture
  content only).
- `calibration.tar.xz` — the two calibration launches of each terminal
  (not measurements: they ran beside other builds), without captures or
  caches.

- `throughput-history-a.tar.xz`, `throughput-history-b.tar.xz`,
  `summary-history.json`, `summary-history.md` — the throughput floods
  measured again with history in every terminal (`--scrollback
  ghostty-default`): Hotaru, Ghostty 1.4.0 and Ghostty main (`0b1729e`)
  with 50 MB, then gnome-terminal, Konsole, Console and foot with
  1,000,000 lines; 42 trials, all accepted, summarised together and apart
  from the zero-scrollback run. `calibration-history.json` is the
  calibration they used: the six terminals' from `calibration.json`,
  Ghostty main's from `calibration-ghostty-main.tar.xz`.

132 measured trials, all accepted, in the first run. Unpack with `tar xJf PHASE.tar.xz`;
`tests/performance/terminals/terminal-summary.py DIR OUT` rebuilds
`summary.json` and `summary.md` from the unpacked folders.

Each trial `result.json` holds, by phase:

- every phase: `startup` (launch → PTY ready, launch → first frame showing
  the fixture's marker), `geometry`, `grid`, `window_rect`,
  `terminal_processes`, `gpu`, `configuration`;
- `startup`: `memory_after_startup` (smaps_rollup per process, DRM fdinfo);
- `steady`: `keyboard` (per press: inject, PTY read, write, capture),
  `streams` (per paced stream: updates seen, CPU per role);
- `throughput`: `throughput` (per corpus: bytes, consume time, final frame,
  CPU and GPU time per role), `memory_after_throughput`;
- `energy`/`idle-default`: `windows` (`baseline_pre`, `idle`, `typing`,
  `baseline_post`: CPU, context switches and GPU time per role, a per-second
  series), `memory_before_idle`;
- `power`: energy and mean power of each window, integrated from `power.tsv`.

Times are `CLOCK_MONOTONIC` nanoseconds. CPU is from per-thread schedstat
(`cpu_ms_sched`) and from `/proc/PID/stat` ticks (`cpu_ms_ticks`, 10 ms
resolution, keeps exited threads). Memory sizes are KiB.
