# why-toolkits: the data behind ../why-toolkits.md

Made by `tests/performance/why-toolkits/` (see the write-up's Reproducing
section). Times are CLOCK_MONOTONIC nanoseconds in the raw files and
milliseconds in the summaries.

The run was stopped after its first chunk (`typing-lat`), so only part of
what the harness writes exists here:

- `environment.json`: store paths and versions of every subject, compositor
  and tool (the `pins`), the kernel, CPU governor, GPU render node and Mesa.
- `summary-sway.json`: the controlled set (every toolkit under the same
  headless sway), from `analyze.py`: per scenario and subject, the pooled
  latency percentiles (first changed frame, `latency_ms`, and the frame
  that shows the settled response, `latency_full_ms`); and every trial's
  reduced row (`trials`). Only `lat-small`, `lat-large` and `lat-fullcopy`.
- `tables-sway.md`: the summary as tables (`report.py`).
- `runs-sway.tar.xz`: every trial of those three scenarios: `result.json`
  (each key's injection and frame times, the program's and sway's CPU and
  GPU over the trial), the observer's, program's and compositor's logs and
  the socket power samples, as the harness wrote them.
- `runs-smoke.tar.xz`: the smoke runs the write-up cites, single trials made
  while building the harness and not all under the exclusive lock:
  `smoke-m` and `smoke-m.json` (the kit and Qt Quick: 4K ceiling, fast
  scroll, resize, blink), `smoke-n` (the native set), `smoke-a` (real
  apps), `smoke-t` (Hotaru and foot, latency and proxy traces).
- `fixtures/`: the text files typed into (`large.txt.xz` compressed).

When the rest runs, `pack.sh` also writes `summary-apps.json`,
`summary-native.json`, their tables, `runs-*.tar.xz` for them, and the
`perf` sets' archives.

Scenario names:

| Name | What |
|---|---|
| `lat-small`, `lat-large` | typing latency, screencopy-timed, 60 Hz with the output on its grid (60-line and 10,000-line text) |
| `lat240-small` | the same at 240 Hz |
| `lat-idle-small` | the same with sway's output idle between commits (no grid) |
| `lat-fullcopy` | the same copying the whole output every frame, as terminals.md's observer did |
| `trace-small`, `trace-large` | typing through the tracing proxy, output on its grid: per-key stages |
| `cost-small`, `cost-large` | 150 keys at 10 a second, sway idle between commits: CPU, GPU, energy per key |
| `scroll-paced` | the list scrolled one row (24 px) every 20 ms by the program |
| `scroll-flat` | four rows every 8 ms (about eight rows a frame at 60 Hz) |
| `scroll-page` | a page every 8 ms: every frame wholly new |
| `resize` | sway resizing the window 60 times a second (900–1500 px wide) |
| `blink` | 8 s with the caret blinking (each toolkit's own blink) |
| `4k-*` | 3840 × 2160 at scale 2, the window 1800 × 1000 points; `4k-ceiling` at 1000 Hz, a page a frame |
| `app-*` | real apps (see above) |
| `keys-*`, `scroll-*` under `native/` | the native set |
