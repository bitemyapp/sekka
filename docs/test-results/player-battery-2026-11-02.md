# Fullscreen movie playback battery measurements — 2 November 2026

A later [short mirrored comparison](player-battery-mirrored-2026-11-02.md)
uses the identical 41:00–42:00 interval in all six runs, in U–V–M–M–V–U order.
The original experiment below is retained in full.

This experiment compares the installed Utsushi, VLC and mpv on a ThinkPad X1
Carbon Gen 14 running Sekka/Aizome. It measures **whole-laptop battery power**
while playing the same local 1080p movie, with the user's other applications
left running. Runtime numbers are short-sample projections, not completed
battery rundown tests or guarantees of whole-film endurance.

## Results

**VLC had the lowest mean power across the nine planned runs. Utsushi had the
lowest power in two of the three scenes, but an unexplained high-power episode
in its middle run reversed its aggregate ranking.** mpv with its installed
software-decoding default was consistently around 8.8 W. This is a comparison
of these installed defaults, not of each player's best achievable settings.
In particular, it does not measure mpv with hardware decoding enabled.

| Player | Mean battery W | Full-charge minutes | Energy-slope check (min) | Range across scenes (min) | Minutes at common starting charge |
| --- | ---: | ---: | ---: | ---: | ---: |
| utsushi | 8.067 | 432 | 432 | 330–513 | 331 |
| vlc | 7.200 | 484 | 512 | 476–489 | 371 |
| mpv | 8.802 | 395 | 416 | 391–398 | 304 |

Common starting energy: 44.55 Wh. Full-charge capacity: 58.02 Wh.
Ranges describe the observed scenes; they are not confidence or prediction intervals.

| Round | Player | Seconds | Battery W | Energy endpoint W | Energy slope W | Energy used Wh | Full-charge minutes |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | utsushi | 119.99 | 6.868 | 6.600 | 6.403 | 0.220 | 507 |
| 1 | vlc | 119.99 | 7.166 | 6.901 | 6.723 | 0.230 | 486 |
| 1 | mpv | 119.99 | 8.746 | 8.100 | 8.400 | 0.270 | 398 |
| 2 | utsushi | 119.99 | 10.546 | 11.101 | 11.367 | 0.370 | 330 |
| 2 | vlc | 119.99 | 7.309 | 6.601 | 6.937 | 0.220 | 476 |
| 2 | mpv | 119.99 | 8.750 | 8.100 | 8.259 | 0.270 | 398 |
| 3 | utsushi | 119.99 | 6.787 | 6.600 | 6.377 | 0.220 | 513 |
| 3 | vlc | 119.99 | 7.125 | 7.201 | 6.738 | 0.240 | 489 |
| 3 | mpv | 119.99 | 8.911 | 8.401 | 8.430 | 0.280 | 391 |

The full-charge projection assumes all 58.02 Wh is usable; reserve and shutdown
policy reduce actual viewing time. VLC's 484-minute power-based estimate and
512-minute energy-slope cross-check differ by about 6%; mpv's differ by about
5%. Those differences are another reason to read the numbers as approximate,
not as minute-accurate endurance predictions.

### The Utsushi spike and follow-up

Utsushi's middle run began near 6.9 W, then rose after roughly 64 seconds and
reached 16.115 W. Its CPU package temperature reached 74°C. Its logs continued
to report GPU decoding, DMA-BUF presentation and zero dropped/discarded
frames. The process snapshots include changing background activity, but do
not establish the cause of the spike; attributing it to a specific app or to
Utsushi would go beyond this evidence.

After all nine planned runs, the exact same 40:00 seek point, 60-second warm-up
and 120-second measurement were repeated once in Utsushi. It averaged
**6.924 W**, with a **503-minute full-charge projection** (529 minutes from
the energy-slope cross-check). The spike did not recur. The follow-up is
[recorded separately](player-battery-2026-11-02/followup/results.md) and is
**not substituted into the planned results** above.

The original Utsushi scene projections were 507, 330 and 513 minutes, with
503 minutes in the additional middle-scene run. This supports roughly
500-minute viewing projections during the observed low-power operation, but
it does not establish how often high-power episodes occur. The retained-app
session was not stable enough to claim a precise Utsushi-versus-VLC endurance
ranking from a few minutes alone. VLC and default mpv were much more
consistent across these samples; mpv drew about 22% more mean power than VLC.

Utsushi's logs reported zero dropped/discarded frames throughout the measured
runs and follow-up. mpv's decoder and presentation drop counters ended at zero
in all three runs. VLC's logs confirmed Intel iHD hardware decoding; this
harness did not collect VLC frame-drop counters. All three had advancing
playback positions, visible movie frames and confirmed fullscreen windows.


## Machine and preserved settings

| Item | Value |
| --- | --- |
| Laptop | Lenovo ThinkPad X1 Carbon Gen 14, model 21V7006FUS |
| Processor | Intel Core Ultra 7 365, 8 cores / 8 threads |
| Graphics | Intel integrated graphics, PCI 8086:b090 |
| System | Sekka, NixOS 26.11.20261030.1c89083; Linux 7.3.0 |
| Display | SDC ATNA40HQ10-0, 2880×1800, 120 Hz, scale 2 |
| Brightness | Requested and actual **149 / 496 = 30.04%**; not a measured luminance in nits |
| Power profile / CPU EPP | `performance` / `performance`, preserved |
| Battery | BAT0, reported full energy 58.02 Wh; design 58.00 Wh; 10 cycles |
| Audio | Built-in speakers at system output 50%, unmuted; player volume 100%, normal speed, same AAC track |
| Sleep inhibition | Keep Awake was already enabled and protected; preserved |
| Utsushi | Installed `aizome-0.1.0`, exact Nix store path in the manifest |
| VLC | 3.0.24 Vetinari |
| mpv | 0.41.0, FFmpeg 9.0.2, libplacebo 7.360.1; installed scripts enabled |

Chrome, Hotaru, Uta, Yukimi, Codex, Claude and the existing session services
were left open. Before testing, Uta reported `Stopped` and Chrome's MPRIS
player reported `Paused`. The harness does not close, pause or kill unrelated
applications. Its own launched player is closed between runs and the original
focused window is restored at the end. Wi-Fi, screen refresh, power mode and
brightness were not tuned for this test.

The root checkout was checked first: tracked files were clean, no commits were
ahead of `origin/main`, and local main was nine commits behind after fetching.
Unrelated untracked `.claude/`, `.env` and `vendor/` entries were left alone.
Work started on `perf/player-battery-life` in a separate worktree from
`b9b0aa458d6f263fefb76908f4f714edf5f39fee`. The installed players were tested;
they were not rebuilt from that checkout. No release workflows were changed
or dispatched.

## Movie and provenance

The movie is George A. Romero's *Night of the Living Dead* (1968), downloaded
from the [Internet Archive item](https://archive.org/details/Night.Of.The.Living.Dead_1080p).
That item's metadata identifies it as public domain. Wikimedia Commons also
documents the film's [US public-domain status](https://commons.wikimedia.org/wiki/File:Night_of_the_Living_Dead_(1968).webm#Licensing).
The downloaded file is the Archive's `NightOfTheLivingDead_PS3.mp4`, **not** its
smaller `NightOfTheLivingDead_1080p.mp4` derivative, which is actually 640×480.
Resolution and duration below come from `ffprobe` on the completed local file.

| Property | Verified value |
| --- | --- |
| Duration | 5752.810667 seconds = 95 minutes 52.811 seconds |
| Video | H.264, 1920×1080 progressive, `yuv420p`, 24000/1001 fps |
| Audio | AAC, 48 kHz, 2 channels |
| File size | 1,862,510,336 bytes |
| Overall bit rate | 2,590,052 bit/s |
| SHA-256 | `8d5f6dd2bc41ab9b16db69b182beccddef1547da2dba6e3a3301da092da3672d` |
| MD5 (matches Archive metadata) | `7b04baa71fb6bbed0a79d1a19d68bf4c` |

The file remains at
`~/Videos/player-battery-benchmark/NightOfTheLivingDead_PS3.mp4` on the test
machine. It is not committed to Git. Downloads, checksumming and probing
finished before the measured intervals; every player read the same local
file without transcoding or network streaming.

To obtain the same input:

```sh
mkdir -p ~/Videos/player-battery-benchmark
curl --fail --location --retry 4 \
  'https://archive.org/download/Night.Of.The.Living.Dead_1080p/NightOfTheLivingDead_PS3.mp4' \
  -o ~/Videos/player-battery-benchmark/NightOfTheLivingDead_PS3.mp4
sha256sum ~/Videos/player-battery-benchmark/NightOfTheLivingDead_PS3.mp4
```

## Protocol

The [harness](../../scripts/player_battery.py) requires Python 3.11 or later,
`ffprobe`, `busctl`, `wpctl`, the installed players, a running Aizome session,
and a Linux battery exporting `energy_now`, `energy_full` and `power_now`.
It is an unprivileged physical-host benchmark, not a VM or installer test.
It intentionally supports this energy-reporting battery interface rather than
silently converting charge-only telemetry using a changing battery voltage.

Each player plays three scenes. Each run seeks to its scene's starting point,
warms up for 60 seconds, captures fullscreen evidence, waits three seconds
after the capture, and measures for 120 seconds. The measured movie intervals
therefore begin approximately 63 seconds after each seek. Exact positions are
recorded in each run's `before.json` and `summary.json`.

| Round | Seek point | Player order |
| --- | --- | --- |
| 1 | 10:00 | Utsushi → VLC → mpv |
| 2 | 40:00 | VLC → mpv → Utsushi |
| 3 | 70:00 | mpv → Utsushi → VLC |

The rotation places each player first, second and third once. It reduces
simple order bias but cannot eliminate changes in background load, battery
state or temperature. Six measured minutes per player sample three different
parts of a 96-minute film; they are not six independent observations of the
entire movie. Startup, seeking and warm-up energy are excluded from the runtime
projection. The operating system's file cache is not flushed.

The harness samples battery energy, power, discharge status, external-power
indicators, backlight and temperature every two seconds using monotonic time.
Every ten seconds it also checks the compositor's focused/fullscreen window,
display mode, system volume and the player's advancing position. It samples
process CPU deltas by process name without collecting command lines or the
titles of unrelated windows. Monitoring overhead is included in measured
whole-system power. Short-lived processes may escape that CPU snapshot.

Runs fail and retain `invalid.json` if power reconnects, brightness changes,
the battery reaches the 15% reserve, the player exits, the visible fullscreen
window changes, playback stalls, or checked audio/display settings change.
Window, playback and output checks occur at ten-second intervals, so they do
not certify that a shorter transient never occurred. Screenshots are taken
outside the measured interval. Cleanup targets only the newly launched player.

Utsushi is put into fullscreen using its own `F` action; VLC and mpv launch
with `--fullscreen`. Compositor state is verified as well. Aizome reports the
Xwayland satellite PID for VLC's window, so the harness identifies its newly
created `vlc` window and separately ties its MPRIS service to the launched
VLC process. The cursor is parked in the picture's middle and allowed to hide;
parking it on mpv's bottom controls kept those controls visible in preflight.

Player configuration is preserved, including mpv's installed scripts and
default software decoding. Utsushi uses a temporary state directory to avoid
changing saved resume positions and volume. Player volume is explicitly set
to 100%. Utsushi's lightweight five-second statistics logging is enabled to
record decode/presentation state. VLC's normal log records its selected
hardware decoder, and mpv's IPC records decoding mode and frame-drop counters.
These are small, different instrumentation costs and are included in results.

Run from this repository (the root flake pins the tool dependencies):

```sh
nix shell --inputs-from . nixpkgs#python3 nixpkgs#ffmpeg-headless -c \
  python3 scripts/player_battery.py \
  ~/Videos/player-battery-benchmark/NightOfTheLivingDead_PS3.mp4 \
  --output "$PWD/artifacts/player-battery-new"

python3 scripts/player_battery_report.py artifacts/player-battery-new
python3 -m unittest discover -s tests -p test_player_battery.py
```

Output directories must be new. `--starts`, `--warmup`, `--seconds`,
`--interval`, `--battery` and `--players` are configurable. The additional
`mpv-auto` choice adds `--hwdec=auto`; it must be identified separately from
installed-default mpv in comparisons. Keep Awake should already prevent idle
blanking; the harness does not change it or any power settings.

## Projection and uncertainty

Battery telemetry uses the Linux power-supply interface's energy and power
units, converted from µWh and µW to Wh and W; percentage charge is **not** used
to estimate consumption. See the [kernel power-supply documentation](https://docs.kernel.org/power/power_supply_class.html).

For each run:

```text
P = trapezoidal integral of power_now over elapsed time / elapsed time
full-charge minutes = 60 × reported energy_full / P
minutes at common starting charge = 60 × experiment-start energy_now / P
endpoint cross-check W = 3600 × (start energy_now − end energy_now) / seconds
slope cross-check W = −3600 × least-squares slope(energy_now versus seconds)
```

The aggregate uses duration-weighted power across each player's three runs,
then converts that power to minutes. It does not average the projected
runtimes. Comparing everyone at the same starting energy avoids making later
players look worse merely because the laptop has less charge left.

The energy endpoint and slope estimates check the instantaneous-power
integration. They use the same battery gauge and are **not independent meter
validation**. Repeated two-second readings can be correlated or unchanged;
treating them as independent samples would manufacture narrow confidence
intervals. The reported runtime range is the range across the three observed
scenes, not a 95% confidence interval or a bound on future playback.

Battery-gauge calibration, quantization and update lag, thermal conditions,
OLED image brightness, background activity, codec/bitrate, renderer settings,
audio output, and the rest of the film can all change runtime. The formula
assumes constant average power and full use of reported capacity; actual
low-battery shutdown reserves shorten useful runtime. It omits startup energy.
A 5% capacity reserve, for example, multiplies the full-charge projection by
0.95. A reliable whole-film endurance claim needs longer repetitions and
eventually a controlled battery rundown.

## Evidence and validation

The planned sequence ran on 2026-11-02 starting at 20:18:24 UTC. The laptop
went from 44.55 Wh (77%) to 41.11 Wh (71%), including warm-ups and transitions.
The follow-up began at 41.07 Wh and ended at 40.73 Wh. Every recorded external-power
indicator was zero and battery status was Discharging. Requested and actual
brightness remained 149/496; the output remained 2880×1800 at 120 Hz.

The [evidence directory](player-battery-2026-11-02/) contains:

- [Planned-run manifest](player-battery-2026-11-02/main/manifest.json),
  [hardware details](player-battery-2026-11-02/main/hardware.json),
  [download provenance](player-battery-2026-11-02/main/source.json),
  [ffprobe output](player-battery-2026-11-02/main/media.json) and ending settings.
- All ten runs' original two-second `samples.jsonl`, player logs,
  before/after playback state and calculated summaries, under `main/` and
  `followup/`. There are 61 samples per run (610 total).
- [Regenerated primary tables](player-battery-2026-11-02/main/results.md) and
  the [separate follow-up table](player-battery-2026-11-02/followup/results.md).
- One original **2880×1800 PNG** from each player's first measured run:
  [Utsushi](player-battery-2026-11-02/main/01-utsushi/fullscreen-before.png),
  [VLC](player-battery-2026-11-02/main/01-vlc/fullscreen-before.png), and
  [mpv](player-battery-2026-11-02/main/01-mpv/fullscreen-before.png).
  These are real Aizome compositor captures on the named laptop, not mockups
  or resized images. They show the same movie at approximately 11:00 with
  player controls hidden; captures differ by fractions of a second.
- [Capture dimensions and SHA-256 hashes](player-battery-2026-11-02/captures.json)
  for all 20 before/after images. The other 17 originals remain locally under
  `~/Videos/player-battery-benchmark/{measurement-final,utsushi-followup}/`;
  only three are committed to avoid duplicating many megabytes of film stills.

Recompute the published tables without playing a movie:

```sh
python3 scripts/player_battery_report.py docs/test-results/player-battery-2026-11-02/main
python3 scripts/player_battery_report.py docs/test-results/player-battery-2026-11-02/followup
```

The reporter refuses incomplete or marked-invalid runs and checks measurement
duration, battery conditions, recorded fullscreen state and playback advance
before recalculating the energy/power estimates from the raw samples.


Numerical tests exercise irregular sample timing, unit conversion, quantized
energy, frozen measurements, and refusal of charging/brightness/reserve
violations. Real fullscreen smoke runs exercised each installed player's
launch, seek, monitoring and cleanup before the measurement sequence. The
first VLC smoke run exposed its Xwayland PID representation; it was rejected
and the harness corrected. An initial measurement attempt was stopped during
warm-up to move the cursor away from mpv's control strip. Those setup runs are
excluded from the results.

This is playback evidence on the named physical laptop. It is not an ISO
build, VM installation, release qualification, or a claim about other laptops.
