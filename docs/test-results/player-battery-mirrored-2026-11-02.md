# Short mirrored playback comparison — 2 November 2026

This rerun uses the **same nominal 41:00–42:00 movie interval for every sample** and
the order **Utsushi → VLC → mpv → mpv → VLC → Utsushi**. This is the three-player
extension of ABBA: each player appears twice, at positions averaging 3.5, to
balance a linear change over time. It does not remove nonlinear background
activity, carry-over or battery-gauge lag.

## Results

Utsushi had the lowest observed mean battery power in this fixed-interval
comparison: 0.49 W below VLC and 1.97 W below default mpv. Both observations
for each player agreed on that ordering. These are whole-laptop readings with
the other applications still running, not isolated measurements of the player.

| Player | Mean battery W | Full-charge minutes | Energy-slope check (min) | Range across runs (min) | Minutes at common starting charge |
| --- | ---: | ---: | ---: | ---: | ---: |
| utsushi | 6.972 | 499 | 512 | 495–503 | 328 |
| vlc | 7.464 | 466 | 462 | 458–475 | 307 |
| mpv | 8.944 | 389 | 396 | 384–395 | 256 |

Common starting energy: 38.15 Wh. Full-charge capacity: 58.02 Wh.
Ranges describe the observed runs; they are not confidence or prediction intervals.

| Run | Player | Seconds | Battery W | Energy endpoint W | Energy slope W | Energy used Wh | Full-charge minutes |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | utsushi | 59.99 | 6.918 | 6.601 | 7.011 | 0.110 | 503 |
| 2 | vlc | 59.99 | 7.322 | 7.201 | 7.186 | 0.120 | 475 |
| 3 | mpv | 59.99 | 8.822 | 8.402 | 8.616 | 0.140 | 395 |
| 4 | mpv | 59.99 | 9.066 | 8.402 | 8.957 | 0.140 | 384 |
| 5 | vlc | 59.99 | 7.606 | 7.801 | 7.882 | 0.130 | 458 |
| 6 | utsushi | 59.99 | 7.026 | 6.001 | 6.576 | 0.100 | 495 |

The two run averages were 6.918/7.026 W for Utsushi, 7.322/7.606 W for VLC,
and 8.822/9.066 W for mpv. Every return run was higher than its outward run;
mirroring balances a linear trend, but two runs do not explain the variation.
No episode resembling the earlier 16 W spike appeared in these six samples.
The short-sample full-charge estimates are approximately **499 minutes for
Utsushi, 466 for VLC and 389 for mpv**. These use the battery's reported
58.02 Wh capacity without subtracting a shutdown reserve. mpv still used its
installed software-decoding default; hardware-enabled mpv was not measured.

### Measured movie intervals

These are player-reported positions at the start and immediately after each
60-second interval. VLC's readings crossed the requested start at roughly
41:00.499, so this is matching the same nominal minute, not claiming identical
first/last decoded frames. Differences are under 0.75 seconds at the start.

| Run | Player | Movie start | Movie end |
| ---: | --- | ---: | ---: |
| 1 | utsushi | 41:00.005 | 42:00.099 |
| 2 | vlc | 41:00.499 | 42:00.498 |
| 3 | mpv | 41:00.041 | 42:00.059 |
| 4 | mpv | 41:00.041 | 42:00.059 |
| 5 | vlc | 41:00.499 | 42:00.499 |
| 6 | utsushi | 41:00.007 | 42:00.072 |


## Shorter protocol

Each run launches a fresh player, seeks to 40:35, warms up for 20 seconds,
takes a fullscreen capture, and waits until movie position 41:00 before
sampling for 60 seconds. The harness refuses an alignment error over 750 ms.
All players play the same local file at normal speed with sound and fullscreen
controls hidden. There are two measured minutes per player and six in total.
Warm-up, alignment, launch and cleanup bring elapsed playback-test time to
about nine minutes, versus about 28 minutes for the original planned sequence.

This fixed interval was chosen before collecting this rerun. It ends before
the movie position at which the original Utsushi power spike appeared
(approximately 42:07); this is a shorter controlled comparison, not a
reproduction or explanation of that earlier episode. The
[original results and separate spike follow-up](player-battery-2026-11-02.md)
remain unchanged.

The same downloaded *Night of the Living Dead* file is used: 95:52.811,
1920×1080 progressive H.264 at 24000/1001 fps, AAC stereo, SHA-256
`8d5f6dd2bc41ab9b16db69b182beccddef1547da2dba6e3a3301da092da3672d`.
Its source, download command and public-domain provenance are in the original
report. The completed download is reused; no network video or transcoding is
involved.

Settings are preserved from the first comparison: ThinkPad X1 Carbon Gen 14,
Core Ultra 7 365, Aizome, 2880×1800 at 120 Hz, brightness **149/496 = 30.04%**,
performance profile/EPP, built-in speakers at 50%, player volume 100%, and
Keep Awake enabled. Other applications remain running. The players and
decoding defaults are unchanged: Utsushi/VLC use hardware decoding, while
installed-default mpv uses software decoding. No power settings or player
source code were changed.

The harness samples battery power/energy and brightness every two seconds,
with playback/fullscreen/output/audio checks and process CPU deltas every
ten seconds. It retains the original power/brightness/reserve abort checks.
The only protocol changes are explicit player ordering, shorter runs and
alignment to a fixed movie timestamp. A fresh process and warm-up are used
even for the two adjacent mpv runs.

```sh
nix shell --inputs-from . nixpkgs#python3 nixpkgs#ffmpeg-headless -c \
  python3 scripts/player_battery.py \
  ~/Videos/player-battery-benchmark/NightOfTheLivingDead_PS3.mp4 \
  --output "$PWD/artifacts/player-battery-mirrored-new" \
  --starts 2435 --warmup 20 --measure-at 2460 --seconds 60 \
  --sequence utsushi vlc mpv mpv vlc utsushi

python3 scripts/player_battery_report.py artifacts/player-battery-mirrored-new
```

`--sequence` requires one seek point and creates a separate numbered directory
for every entry, including repeated players. Without it, the original rotated
scene schedule remains available. `--measure-at` must allow 4–25 seconds after
warm-up for capture and alignment. The manifest records the exact schedule,
target timestamp and harness source hash.

## Interpretation and evidence

The runtime formula is unchanged: duration-weighted mean battery power across
both runs, then `60 × energy_full / watts`. Everyone is also projected from the
same experiment-start energy. Battery energy endpoints and slopes provide
cross-checks. They come from the same gauge, not an independent meter.

Shorter samples trade precision for time: the 0.01 Wh battery-energy steps are
large relative to one minute's consumption, and power readings update less
often than the two-second sampling interval. Two repetitions cannot establish
a reliable confidence interval, nor prove that a power spike will never recur.
The range shown is only the observed range across two runs. These estimates
describe this one movie interval, these settings and this retained-app session;
they are not battery rundown measurements or whole-film endurance guarantees.

The completed cycle started at **2026-11-02T21:20:35Z** and reached its
last sample at **2026-11-02T21:29:09Z**: **8 minutes 34 seconds**,
plus final capture/cleanup. There are **186 raw samples** (31 per run).
The battery went from **38.15 Wh (66%)** to
**37.09 Wh (64%)** during this cycle. It remained unplugged
and discharging; brightness stayed 149/496 and display/audio checks passed.

An earlier alignment attempt completed one Utsushi sample (6.960 W), then
rejected VLC **before measurement** when its reported position jumped to
41:00.499, past the original 250 ms tolerance. The tolerance was widened to
750 ms, and the complete six-run cycle was restarted. The earlier attempt's
31 samples and its `invalid.json` are preserved separately; they are not mixed
into the results above. That attempt added about two minutes of setup playback.

The [evidence directory](player-battery-mirrored-2026-11-02/) includes:

- [Manifest and schedule](player-battery-mirrored-2026-11-02/main/manifest.json),
  the file probe, all six runs' raw samples, playback state, logs, summaries and
  [regenerated tables](player-battery-mirrored-2026-11-02/main/results.md).
- Original 2880×1800 screenshots from the first
  [Utsushi](player-battery-mirrored-2026-11-02/main/01-utsushi/fullscreen-before.png),
  [VLC](player-battery-mirrored-2026-11-02/main/02-vlc/fullscreen-before.png) and
  [mpv](player-battery-mirrored-2026-11-02/main/03-mpv/fullscreen-before.png) runs.
  These real compositor captures are taken near 40:55, before alignment and
  outside measurement, with controls hidden. They have not been resized.
- [Capture hashes and dimensions](player-battery-mirrored-2026-11-02/captures.json)
  for all screenshots, including those retained locally rather than committed.
  All originals remain under `~/Videos/player-battery-benchmark/mirrored-final/`
  and `~/Videos/player-battery-benchmark/mirrored/`.
- The [rejected alignment attempt](player-battery-mirrored-2026-11-02/alignment-attempt/),
  kept outside the completed-cycle statistics.

Utsushi reported GPU decoding/DMA-BUF presentation and zero dropped/discarded
frames. mpv reported software decoding and zero decoder/presentation frame
drops. VLC's logs confirmed Intel iHD hardware decoding; VLC frame-drop
counters were not collected. Playback advanced normally and fullscreen state
was confirmed for all six completed runs.

Validation: five harness tests passed, including explicit-order balancing and
non-overwriting run identifiers. The current reporter still reproduces both
original experiment tables byte-for-byte, and the new table regenerates from
the raw samples. The measured harness hash matches the submitted source.

```sh
python3 scripts/player_battery_report.py docs/test-results/player-battery-mirrored-2026-11-02/main
python3 -m unittest discover -s tests -p test_player_battery.py
```
