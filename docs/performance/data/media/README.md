# Media players: the data

What `docs/performance/media.md` is made of. Everything was produced by
`tests/performance/media/` (`run.sh` and `batch.sh` run `bench.py` and
`ttff.py`; `collect.sh` copies the results here and runs `summarize.py`).
The clips themselves are not here; `encode.sh` makes them from the
Blender sources, and their SHA-256 sums are in `clips.sha256`.

| File | What it is |
|---|---|
| `summary-compact.md` | The write-up's tables: per compositor, mode and clip, medians over trials with the range of the power delta |
| `summary.md` | Wider tables of the same groups |
| `summary.csv` | The same groups, every metric as median, min and max (`*_min`, `*_max`), with the number of trials (`n`), failures and trials excluded for interference |
| `trials.csv` | One row per trial: the numbers the groups are made of, the trial's interpolated baseline (`baseline_w`), its delta (`delta_w`, and `delta_median_w` against the batch's median baseline), and the interference check (`other_cpu_pct`, `interfered`) |
| `ttff.jsonl` | Time from launch to the first video frame, one line per launch |
| `clips.sha256`, `sources.sha256` | SHA-256 of the clips measured and of the downloaded sources (the zips and the files in them) |
| `environment.txt` | Versions: store paths of every program, the commit, Mesa, kernel, governor, PipeWire's clock settings |
| `batches/<batch>/programs.txt` | The store paths the batch ran |
| `batches/<batch>/batches.jsonl` | The batch's plan, random seed, start and end, and whether the default audio sink was unchanged after it |
| `batches/<batch>/results.jsonl` | One JSON object per trial: power summary, CPU and switches per role, iGPU, DRM engine time, memory, the player's own counters before and after the window |
| `batches/<batch>/trials/<n>-<compositor>-<mode>-<clip>-<player>/trial.json` | Everything recorded for one trial: the quiet check, processes and their executables, windows, audio routing, per-role CPU and switches, per-thread switches and CPU time of the player, DRM devices and engine time per role, compositor frame statistics, the player's counters, memory |
| `…/power.tsv.xz` | The PPT samples of the window (seconds since its start, watts), every 10 ms |
| `…/igpu.tsv.xz` | The iGPU's `gpu_busy_percent` and `sclk` every 100 ms (unreliable on this part, see the write-up) |
| `…/player.log.xz` | The player's own output (Utsushi's `UTSUSHI_STATS` lines, mpv's and VLC's messages) |

Batches, in the order they ran (2 November 2026): `a` 1080p H.264 and
paused (03:53–04:30), `d` 4K HEVC Main 10 and MP3 (05:01–05:41), `b` 4K60
AV1 and more 1080p H.264 (08:01–08:38), `c` 1080p HEVC Main 10 and AV1,
then `ttff` (10:10–10:45), `e` sway, VP9 and more 1080p AV1
(12:19–12:58), `f` with sound, Showtime, Haruna and Opus (13:37–14:17),
`g` mpv's `dmabuf-wayland` put full screen by the compositor beside
Utsushi (15:02–15:13, stopped after a round; marked
`exclude-from-summary`: its power is unusable, its other records are in
the write-up).
From about 10:00 the user was using the desktop (on the NVIDIA card),
which raised and moved the idle floor: 18.6–20.8 W in batches `c`, `e`
and `f` against 18.1–18.4 W at night.

Modes: `play` (video, null audio), `pause` (paused after 5 s, with sound
to the null sink, 30 s window), `audio` (music to the null sink),
`playsound` (video with sound to the null sink).

Roles in `cpu` and `gpu`: `player` (the player's process tree),
`compositor`, `shell` (Aizome's shell, under Aizome only), `xwayland`
(xwayland-satellite and Xwayland, or sway's Xwayland, for VLC),
`dbus` (the session's private bus), and `pw:pipewire`,
`pw:pipewire-pulse`, `pw:wireplumber` (the user's PipeWire daemons, in
trials with sound). `cpu_s` and `cpu_pct` (percent of one core) are the
threads' run time from `/proc/PID/task/TID/schedstat`; `cpu_ticks_s` is
the coarser `utime + stime` of `/proc/PID/stat` as a check;
`vol_switches` and `invol_switches` are voluntary and involuntary context
switches summed over threads (a thread that ended during the window keeps
its last 1 Hz sample). `gpu.<role>.engine_ns` is DRM fdinfo engine time
per engine (`gfx`, `dec`, …) over the window, summed over the process's
DRM clients; `devices` and `pdevs` are the DRM nodes and PCI devices it
held (0000:78:00.0 is the Radeon; `nvidia_fds` counts `/dev/nvidia*`).
