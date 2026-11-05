# Utsushi's energy against mpv and VLC

Utsushi, Aizome's media player, is built to do as little work per frame as
the machine allows (`desktop/aizome/docs/apps.md`, Utsushi): FFmpeg decodes
on the GPU through VA-API, each decoded surface goes to the compositor as a
dmabuf without a copy or a GL pass in the player, frames are timed to the
compositor's clock with presentation feedback, nothing is decoded out of
sight, audio runs on long PipeWire cycles, and the window redraws its own
interface rarely. `desktop/aizome/docs/performance.md` measured its CPU time
and context switches on an Intel laptop and left whole-system power
unmeasured. This page measures power on a desktop: the processor's socket
power (CPU cores, the integrated GPU that decodes and composes, and the I/O
die) while each player plays the same clip full screen under the same
compositor, against an idle baseline taken in between.

## In short

On a Ryzen 7 9800X3D whose Radeon graphics decode and compose, under
Aizome's compositor running headless, full screen, sound off, 60 s per
trial, the extra socket power over idle (median of three or more trials;
1080p rows within about ±0.2 W are noise):

| Clip | Utsushi | mpv VA-API → dmabuf-wayland | mpv VA-API → gpu-next | VLC 3 | mpv defaults (software) | mpv with Sekka's `mpv.conf` |
|---|---|---|---|---|---|---|
| Film, 1080p24 H.264 | +0.20 W | +0.42 W | +0.51 W | +0.46 W | +1.36 W | +1.39 W |
| Film, 1080p24 HEVC 10-bit | +0.18 W | +0.40 W | – | +1.43 W (software) | +1.43 W | – |
| Film, 1080p24 VP9 | +0.19 W | +0.34 W (1 trial) | – | +0.21 W (1 trial) | +1.38 W | – |
| Film, 1080p24 AV1 | +0.27 W | +0.17 W | – | +1.39 W (software) | +1.76 W | – |
| 4K30 HEVC 10-bit | +0.56 W | +0.62 W | +4.34 W | +0.85 W | +8.27 W | +9.96 W |
| 4K60 AV1 | +0.89 W, no drops | +1.06 W, no drops | +7.62 W, 557 dropped | +7.60 W (software), 315 dropped | +13.32 W, 145 dropped | – |

- **Power: Utsushi is among the lowest everywhere and lowest at 4K, but
  it isn't alone.** At 1080p every player that decodes on the GPU sits a
  few tenths of a watt over idle in no consistent order (Utsushi lowest
  for H.264 and HEVC, mpv's `dmabuf-wayland` for AV1, VLC under sway);
  mpv set up the way Utsushi works (`--hwdec=vaapi --vo=dmabuf-wayland`)
  is within 0.2 W of it on every clip. The large differences come from
  decoding in software (mpv's defaults, Sekka's shipped `mpv.conf`, VLC 3
  for AV1 and for 1080p HEVC Main 10: +1.4 W at 1080p, +8 to +13 W at 4K)
  and from rendering 4K frames again in the player (mpv's gpu-next: +4.3
  and +7.6 W, and frames dropped at 4K60).
- **Work around the frames: Utsushi does the least.** At 1080p 0.7–0.9%
  of a core and about 147 wakeups a second, against 1.1–1.4% and 450 for
  mpv `dmabuf-wayland`, 2.0% and 410 for mpv gpu-next and 1.5–1.7% and
  260–320 plus Xwayland for VLC; the smallest memory (58–122 MiB PSS,
  against 78–989 for the others).
- **Paused**, every player is at idle power, but mpv keeps its PipeWire
  stream cycling (47 wakeups a second in PipeWire) and VLC keeps
  redrawing (13 frames a second composed); Utsushi wakes 4 times a second.
- **Music** (MP3 and Opus) is within noise of idle for all; Utsushi's
  170 ms PipeWire cycles mean 15 wakeups a second in the player and
  12–36 in PipeWire, against 190–215 and 94–118 for mpv and 84–122 and
  230–270 for VLC (through `pipewire-pulse`).
- **With sound**, as players are used (1080p H.264 to a null sink),
  Utsushi, mpv `dmabuf-wayland`, VLC and GNOME's Showtime are all at
  +0.27–0.40 W; KDE's Haruna costs +0.82 W (libmpv rendering into its Qt
  Quick scene, 800 wakeups a second, 300 MiB) and mpv's software default
  +2.35 W.
- **Under sway** the same three hardware-decoding players tie within
  0.15 W, with the same CPU and wakeup order: Aizome isn't what puts
  Utsushi ahead.
- **First frame** on screen: Utsushi 58 ms, mpv 76–156 ms, VLC 172 ms.

This is a desktop and a headless compositor: no display planes, an
18–20 W idle floor read in whole watts, part of it measured while the
user was using the machine; not a battery-life figure (see Limits).

<!-- RESULTS -->

## Results

Medians over trials, with the lowest and highest trial; `n` is the
number of trials (three or more, except where interference left two, and
"(power k)" where only k of them have a usable power figure because the
machine's floor moved under the others). **Δ power** is the socket's mean
power over the 60 s window minus the idle floor at that time
(interpolated between the baselines run around it; about 18.2 W at
night, 18.6–20.8 W once the user was at the desktop); **Δ J/min** is that × 60.
CPU is percent of one core (the process and all its threads), wakeups are
voluntary context switches per second summed over the player's threads;
the compositor's switches include its handling of every client's
commits. GPU is the share of the window the Radeon's video decoder
(`dec`) and 3D engine (`gfx`: the player's own rendering, the
compositor's and Xwayland's together) were busy, from DRM fdinfo. PSS is
the player's proportional memory at the end. Dropped is the player's own
count (mpv's dropped frames, VLC's lost pictures, Utsushi's dropped).

`mpv-vaapi-dmabuf` ran windowed at the picture's size under Aizome (its
`--fs` didn't take; see "mpv's dmabuf-wayland put full screen" below,
which shows that full screen changes nothing in its work).

### Film, 1080p H.264 (the everyday case)

*Tears of Steel*, 1920 × 800 at 24 fps, H.264 High at 8 Mbit/s, full
screen on a 1920 × 1080 output, sound off. Two batches (`a` and `b`), each
against its own baselines.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.20 (0.19–0.53) | 12 | 0.83 | 146 | 0.94 | 130 | – | – | 3.2 / 3.8 | 76 | 0 | 6 |
| mpv-vaapi-dmabuf | 0.42 (0.39–0.62) | 25 | 1.44 | 452 | 1.03 | 130 | – | – | 3.4 / 3.8 | 112 | 0 | 5 |
| mpv-vaapi-gpunext | 0.51 (0.27–0.97) | 30 | 1.96 | 410 | 0.89 | 160 | – | – | 3.5 / 7.4 | 159 | 0 | 6 |
| mpv-default | 1.36 (1.35–1.53) | 82 | 12.72 | 366 | 1.10 | 180 | – | – | 0.0 / 10.2 | 180 | 0 | 3 |
| mpv-sekka | 1.39 (1.38–1.60) | 83 | 12.67 | 793 | 0.68 | 153 | – | – | 0.0 / 8.2 | 201 | 0 | 3 |
| vlc-default | 0.46 (0.35–0.62) | 27 | 1.53 | 264 | 0.76 | 129 | 0.34 | – | 2.9 / 4.7 | 141 | 0 | 6 |


Every player that decodes on the GPU costs the socket between 0.2 and
0.5 W over idle. Utsushi's median is the lowest, but the ranges overlap
and the differences between the hardware-decoding players are about the
size of the sensor's noise. The clear differences are in the work around
the frames: Utsushi's process uses about 0.8% of a core and wakes about
146 times a second (its render thread twice per frame, for the frame and
its presentation feedback; the video thread; the null audio output's
cycles; the radeonsi submission thread), mpv with the same design
(`dmabuf-wayland`) 1.4% and 450, mpv's gpu-next 2.0% and 410 plus twice
the GPU 3D time, VLC 1.5% and 260 plus Xwayland. Utsushi's memory is the
smallest (76 MiB PSS against 110–160 for mpv and 140 for VLC). Software
decoding, mpv's own default and what Sekka's `mpv.conf` leaves in place,
costs 1.4 W more: an eighth of a core.

### 4K HEVC Main 10, 30 fps

*Big Buck Bunny* at 3840 × 2160, 10-bit, 16 Mbit/s, shown on the
1920 × 1080 output (each player or the compositor scales it down).

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.56 (0.53–0.57) | 34 | 1.15 | 178 | 1.11 | 162 | – | – | 19.5 / 5.3 | 122 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.62 (0.54–0.68) | 37 | 1.82 | 528 | 1.23 | 160 | – | – | 19.8 / 5.7 | 215 | 0 | 3 |
| mpv-vaapi-gpunext | 4.34 (4.31–4.43) | 260 | 1.87 | 485 | 0.66 | 215 | – | – | 19.9 / 42.4 | 250 | 0 | 3 |
| mpv-default | 8.27 (8.24–8.31) | 496 | 45.57 | 440 | 0.97 | 223 | – | – | 0.0 / 45.2 | 460 | 0 | 3 |
| mpv-sekka | 9.96 (9.82–10.13) | 598 | 47.46 | 939 | 0.61 | 161 | – | – | 0.0 / 53.8 | 989 | 0 | 3 |
| vlc-default | 0.85 (0.69–0.93) | 51 | 2.01 | 334 | 0.85 | 160 | 0.38 | – | 18.7 / 6.4 | 406 | 0 | 3 |


The decoder's own work is the same for all three VA-API paths (the VCN
busy about 20% of the time). What differs is the picture's way out:
handed to the compositor as a P010 dmabuf (Utsushi, mpv `dmabuf-wayland`),
the compositor scales and converts it in about 1.8 ms of GPU time per
frame, and the whole costs about 0.6 W. Rendered again by mpv's gpu-next
(libplacebo's default scalers on Vulkan), the 3D engine is busy 42% of the
time and the socket draws 4.3 W more. VLC (VA-API here, its own GL
rendering through Xwayland) is in between at 0.85 W. Decoding 4K 10-bit
HEVC in software costs 8.3 W (mpv defaults, gpu-next on Vulkan) to 10.0 W
(Sekka's `mpv.conf`, gpu-next on OpenGL); none of them dropped frames on
this processor, and their memory is 4 to 8 times Utsushi's.

### 4K AV1, 60 fps

*Big Buck Bunny* at 3840 × 2160 and 60 fps, AV1 8-bit at 11 Mbit/s: the
heaviest clip, at the output's own rate.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.89 (0.83–1.26) | 54 | 1.05 | 328 | 1.19 | 332 | – | – | 36.6 / 9.4 | 73 | 0 | 3 |
| mpv-vaapi-dmabuf | 1.06 (1.02–1.08) | 64 | 1.63 | 796 | 1.32 | 326 | – | – | 36.7 / 9.2 | 100 | 0 | 3 |
| mpv-vaapi-gpunext | 7.62 (7.44–7.64) | 457 | 1.99 | 667 | 0.68 | 346 | – | – | 36.7 / 71.2 | 140 | 557 | 3 |
| mpv-default | 13.32 (13.20–13.87) | 799 | 44.55 | 11383 | 0.85 | 415 | – | – | 0.0 / 81.6 | 469 | 145 | 3 |
| vlc-default | 7.60 (7.48–7.70) | 456 | 51.57 | 11119 | 0.69 | 285 | 0.30 | – | 0.0 / 24.1 | 674 | 315 | 3 |


Utsushi and mpv's `dmabuf-wayland` play every frame for about 1 W. mpv's
gpu-next can't render 4K60 through libplacebo on this iGPU in time: it
drops about one frame in six and costs 7.6 W. VLC 3.0.24 doesn't decode
AV1 with VA-API (its process never touches the video decoder): it decodes
in software, drops 315 frames a minute and costs 7.6 W. mpv's defaults
decode with dav1d in under half a core, but its GPU rendering is the
bottleneck again: 13.3 W and 145 frames dropped. The compositor rendered
about 3230 of the 3600 refreshes for both dmabuf players: the headless
output's timer and 60 fps video don't line up exactly, the same for both.

### HEVC Main 10 and AV1 at 1080p

The same film in HEVC Main 10 (5 Mbit/s) and AV1 (2.8 Mbit/s). These ran
after the user had started using the desktop (on the NVIDIA card): the
idle floor was about 20.5 W instead of 18.2 W. Two AV1 trials (Utsushi,
VLC) were dropped for interference, and three more of each ran in batch
`e`.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.18 (0.03–0.20) | 11 | 0.70 | 147 | 0.73 | 130 | – | – | 4.2 / 3.8 | 70 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.40 (0.37–2.16) | 24 | 1.17 | 455 | 0.80 | 129 | – | – | 4.5 / 3.8 | 112 | 0 | 3 |
| mpv-default | 1.43 (1.16–1.73) | 86 | 14.06 | 369 | 0.84 | 180 | – | – | 0.0 / 10.2 | 186 | 0 | 3 |
| vlc-default | 1.43 (1.21–1.55) | 86 | 14.00 | 397 | 0.61 | 128 | 0.27 | – | 0.0 / 7.8 | 312 | 0 | 3 |


| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.27 (0.09–0.34) | 16 | 0.71 | 147 | 0.85 | 129 | – | – | 3.5 / 3.8 | 58 | 0 | 5 |
| mpv-vaapi-dmabuf | 0.17 (-0.00–0.27) | 10 | 1.11 | 438 | 0.83 | 129 | – | – | 3.7 / 3.8 | 78 | 0 | 3 |
| mpv-default | 1.76 (1.67–1.79) | 105 | 8.81 | 2230 | 0.73 | 178 | – | – | 0.0 / 11.6 | 184 | 0 | 3 |
| vlc-default | 1.39 (1.32–1.77) | 83 | 8.50 | 2092 | 0.49 | 128 | 0.21 | – | 0.0 / 7.4 | 217 | 0 | 5 |


Utsushi and mpv's `dmabuf-wayland` decode both on the GPU and stay at
0.1–0.4 W. VLC 3.0.24 decodes neither on the GPU at this size: its
process never touches the video decoder for the 1080p HEVC Main 10 clip
(it did for the 4K one) nor for AV1, and it costs as much as mpv's
software default, about 1.4 W and 8–14% of a core. The AV1 software
decoders (dav1d) wake 2100–2200 times a second.

### VP9 at 1080p

The same film in VP9 (5 Mbit/s), in batch `e`, during which the user's
activity changed and the socket's floor dropped by 2 W between two
baselines: the power of the trials across that step is left out (`n`
shows how many trials have power), their CPU, wakeups and GPU are kept.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.19 (-0.09–0.32) | 12 | 0.86 | 147 | 0.84 | 128 | – | – | 4.4 / 3.8 | 61 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.34 (0.34–0.34) | 20 | 1.32 | 447 | 0.93 | 129 | – | – | 4.6 / 3.8 | 82 | 0 | 3 (power 1) |
| mpv-default | 1.38 (1.22–1.41) | 83 | 9.61 | 372 | 0.97 | 178 | – | – | 0.0 / 10.2 | 126 | 0 | 3 |
| vlc-default | 0.21 (0.21–0.21) | 13 | 1.72 | 316 | 0.61 | 128 | 0.28 | – | 4.4 / 4.7 | 119 | 0 | 3 (power 1) |


VLC uses VA-API for VP9 (its decoder engine is busy as much as the
others'), and the three GPU-decoding players are again within noise of
each other; software VP9 (mpv's defaults) costs 1.4 W.

### Paused

The 1080p H.264 film, with sound to the null sink, paused after 5 s
through each player's own interface (MPRIS for Utsushi, which has it only
with sound; IPC for mpv; the rc interface for VLC), measured over 30 s.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | -0.10 (-0.17–-0.00) | -6 | 0.01 | 4 | 0.02 | 7 | 0.00 | 0 | 0.0 / 0.0 | 79 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.03 (-0.10–0.20) | 2 | 0.00 | 2 | 0.02 | 7 | 0.09 | 47 | 0.0 / 0.0 | 112 | 0 | 3 |
| mpv-default | -0.02 (-0.14–0.09) | -1 | 0.00 | 2 | 0.02 | 7 | 0.09 | 47 | 0.0 / 0.0 | 179 | 0 | 2 |
| vlc-default | 0.04 (0.03–0.14) | 2 | 0.35 | 41 | 0.48 | 74 | 0.22 | 1 | 0.0 / 2.6 | 139 | 0 | 3 |


A paused player costs nothing measurable in power, but not all of them
stop. mpv keeps its PipeWire stream connected and running, so the audio
graph keeps cycling at 1024 frames: 47 wakeups a second in the PipeWire
daemon for as long as it stays paused (with a real sound card, its
stream would keep running too). VLC keeps redrawing its full-screen video window:
the compositor composes 13 frames a second, and Xwayland and VLC wake 40
times a second. Utsushi's stream stops with the picture (it drops what
the graph held) and nothing is drawn: 4 wakeups a second in Utsushi, and
PipeWire idle.

### Music

*Big Buck Bunny*'s soundtrack as MP3 (192 kbit/s, 44.1 kHz) and Opus
(128 kbit/s), 60 s, to the null sink. Utsushi shows its window (cover and
title, a clock ticking once a second); mpv plays without a window, as it
does for audio files; VLC shows its Qt window.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.11 (0.06–0.27) | 6 | 0.17 | 15 | 0.07 | 12 | 0.02 | 12 | – / 0.1 | 43 | 0 | 3 |
| mpv-default | 0.12 (0.12–0.18) | 7 | 0.84 | 192 | 0.01 | 7 | 0.13 | 94 | – / 0.0 | 52 | 0 | 3 |
| vlc-default | 0.16 (0.15–0.34) | 10 | 0.96 | 122 | 0.16 | 26 | 0.55 | 228 | – / 0.4 | 70 | 0 | 3 |


| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.10 (0.04–0.15) | 6 | 0.20 | 15 | 0.06 | 12 | 0.20 | 36 | – / 0.1 | 35 | 0 | 2 |
| mpv-default | 0.37 (0.18–1.60) | 22 | 0.74 | 213 | 0.01 | 7 | 0.26 | 118 | – / 0.0 | 42 | 0 | 3 |
| vlc-default | 0.27 (0.26–1.09) | 16 | 0.58 | 84 | 0.06 | 12 | 0.47 | 266 | – / 0.1 | 58 | 0 | 3 |


(MP3 in batch `d`, at night; Opus in batch `f`, with the user at the
desktop, where one Utsushi trial was dropped for interference.)

All three are within noise of idle in power (a few tenths of a watt, the
ranges overlap). The wakeups are where they differ: Utsushi asks
PipeWire for 170 ms cycles (8192 frames, allowed here by
`clock.max-quantum`), wakes 15 times a second and keeps the daemon to 12;
mpv's stream runs the graph at 1024 frames (21 ms): 190 wakeups in mpv and
94 in PipeWire; VLC goes through `pipewire-pulse`: 120 in VLC and about 230
in the two daemons.

### With sound, and GNOME's and KDE's players

The 1080p H.264 film with its sound to the null sink, each player as
it's used, adding GNOME's Showtime and KDE's Haruna, which have no null
output of their own (batch `f`, with the user at the desktop; the floor
moved under three trials). Showtime decoded with VA-API through
GStreamer once CUDA was hidden (see Setup), Haruna with VA-API through
libmpv by default; neither reports dropped frames here.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.27 (0.13–0.41) | 16 | 0.72 | 151 | 0.68 | 130 | 0.23 | 71 | 3.2 / 3.7 | 64 | 0 | 3 (power 2) |
| mpv-vaapi-dmabuf | 0.27 (-0.54–0.40) | 16 | 1.24 | 521 | 0.73 | 129 | 0.25 | 118 | 3.4 / 3.7 | 100 | 0 | 3 |
| mpv-default | 2.35 (2.02–2.69) | 141 | 11.94 | 437 | 0.70 | 178 | 0.25 | 118 | 0.0 / 10.2 | 168 | 0 | 3 (power 2) |
| vlc-default | 0.28 (-0.35–0.52) | 17 | 1.32 | 332 | 0.52 | 129 | 0.63 | 260 | 2.9 / 4.7 | 132 | 0 | 3 |
| showtime | 0.40 (0.11–0.69) | 24 | 1.61 | 543 | 0.50 | 154 | 0.28 | 114 | 3.2 / 5.0 | 159 | – | 3 (power 2) |
| haruna | 0.82 (0.34–0.96) | 49 | 2.29 | 814 | 0.49 | 129 | 0.24 | 118 | 3.7 / 9.6 | 301 | – | 3 |


With sound the GPU-decoding players, Showtime included, stay within
noise of each other (0.3–0.4 W). Haruna costs about twice that: libmpv
renders the picture into its Qt Quick scene (the 3D engine busy 9.6%,
against 3.7% for the compositor alone with Utsushi), it wakes 800 times a
second, and its PSS is 300 MiB. PipeWire's daemons wake 71 times a second
for Utsushi's stream (40 ms cycles with a picture), about 115–120 for
mpv, Showtime and Haruna, and 260 for VLC through `pipewire-pulse`.
Software decoding with sound (mpv's defaults) costs about 2.4 W.

### The same players under sway

To check that Aizome's compositor isn't what favours Utsushi, the three
hardware-decoding players ran the 1080p H.264 film under sway 1.12
(wlroots 0.20, headless, GLES on the same Radeon, `xwayland` lazy for
VLC), without Aizome's shell; Utsushi there draws its in-window menu
bar, as kit apps do without the shell. Utsushi was put full screen with
`swaymsg`, mpv and VLC with their own flags.

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.38 (0.36–0.38) | 23 | 0.73 | 145 | 0.48 | 98 | – | – | 4.2 / 3.7 | 67 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.39 (0.36–0.53) | 23 | 1.45 | 459 | 0.29 | 73 | – | – | 3.2 / 0.0 | 102 | 0 | 3 |
| vlc-default | 0.25 (0.20–0.31) | 15 | 1.44 | 264 | 0.22 | 48 | 0.00 | – | 2.9 / 3.8 | 132 | 0 | 3 (power 2) |


Under sway the three are within 0.15 W of each other (VLC lowest, by
less than the noise), with the same order in CPU and wakeups as under
Aizome. One difference in how the compositors work shows: sway put mpv's
full-screen NV12 buffer straight on its headless output without
composing (its 3D engine time is zero), while Utsushi's window, with its
menu bar, was composed; Aizome composes both. Sway's own wakeups were
fewer than Aizome's for every player (48–98 a second against 128–130).

### mpv's dmabuf-wayland put full screen

Under Aizome, mpv's `--fs` didn't take with `--vo=dmabuf-wayland` (it did
with gpu-next, and under sway), so in batches `a`–`f` that configuration
played in a window the picture's size. Batch `g` put it full screen
through the compositor, as Utsushi is, beside Utsushi in the same batch.
The user's activity moved the floor by several watts during it (its
baseline was 25.2 W, above the players' trials), so it was stopped after
one round and its power isn't used; it's kept as data
(`batches/g`, left out of the summaries). One trial each (Utsushi's 4K
HEVC twice):

| Clip | Player | Player CPU % | Player wakeups/s | Compositor CPU % / switches/s | GPU dec / gfx % | PSS MiB |
|---|---|---|---|---|---|---|
| 1080p24 H.264 | utsushi | 0.46 | 146 | 0.51 / 128 | 3.2 / 3.7 | 62 |
| 1080p24 H.264 | mpv-vaapi-dmabuf, full screen | 0.87 | 455 | 0.51 / 129 | 3.4 / 3.7 | 101 |
| 4K30 HEVC 10-bit | utsushi | 0.71–0.72 | 177 | 0.63–0.66 / 159 | 19.5 / 5.2 | 109 |
| 4K30 HEVC 10-bit | mpv-vaapi-dmabuf, full screen | 1.22 | 538 | 0.62 / 159 | 19.7 / 5.2 | 193 |
| 4K60 AV1 | utsushi | 0.75 | 329 | 0.86 / 333 | 36.5 / 9.3 | 62 |
| 4K60 AV1 | mpv-vaapi-dmabuf, full screen | 1.30 | 810 | 0.86 / 326 | 36.6 / 9.3 | 79 |

Full screen, mpv's `dmabuf-wayland` gives the compositor exactly the
work Utsushi does (the same 3D engine time to the tenth of a percent) and
its own CPU and wakeups are what they were windowed; the windowed power
figures above stand for it. (Both players' CPU is lower here than in the
other batches, the processor running faster while the rest of the
machine was busy.)

### Time to the first frame

From launch to the first frame of the test pattern on screen, five
launches each, windowed, files cached:

| Player | First frame, median (range) | Window mapped |
|---|---|---|
| utsushi | 58 ms (57–58) | 33 ms |
| mpv-vaapi-dmabuf | 76 ms (76–76) | 76 ms |
| mpv-vaapi-gpunext | 152 ms (151–154) | 152 ms |
| mpv-default | 156 ms (152–163) | 156 ms |
| vlc-default | 172 ms (166–183) | 99 ms |

mpv maps its window with its first frame, so its two times are the same
and as fine as the harness's window polling (about 15 ms); Utsushi maps
its window first and shows the frame 25 ms later; VLC's window (through
Xwayland) comes up 70 ms before its first frame. Data: `ttff.jsonl`.

<!-- /RESULTS -->

## What it shows

- **Decoding on the GPU is most of it.** Every configuration that decodes
  in software costs watts: mpv's own defaults (and Sekka's `mpv.conf`,
  which doesn't turn on hardware decoding either) add 1.4–1.8 W at 1080p,
  8–10 W for 4K HEVC and 13 W for 4K60 AV1, against well under 1 W for the
  players that decode with VA-API. VLC 3.0.24 uses VA-API for the 1080p
  H.264 and VP9 clips and the 4K HEVC one, but not for the 1080p HEVC
  Main 10 clip or for AV1, and there it costs what software mpv does.
- **Then, who draws the picture.** Handing the decoded surface to the
  compositor (Utsushi, mpv `dmabuf-wayland`) leaves the iGPU's 3D engine
  only the compositor's scaling: 4–9% busy. Rendering it again in the
  player (mpv `gpu-next`, libplacebo's default scalers on Vulkan) makes it
  42% busy for 4K HEVC (+4.3 W) and 71% for 4K60 AV1 (+7.6 W, and 557 of
  3600 frames dropped). VLC renders with GL too, cheaply at these sizes,
  and pays a little for Xwayland.
- **Among the GPU-decoding players, power doesn't separate them at
  1080p.** They are all a few tenths of a watt over idle, in an order that
  changes from clip to clip and between compositors, within this
  sensor's noise. At 4K Utsushi is the lowest, 0.06–0.17 W under mpv's
  `dmabuf-wayland`, which does the same thing in the same way; that gap
  is small next to everything else here. Utsushi's design pays off
  against players that don't decode on the GPU or that render the picture
  again, which is what mpv, Sekka's `mpv.conf` and VLC do by default for
  some or all of these clips.
- **Where Utsushi is clearly ahead is the work around the frames:** a
  third to two fifths of mpv's wakeups and about 40% less CPU than the
  lightest mpv configuration on every clip, about half VLC's wakeups
  without Xwayland, and the smallest memory. None of that moves this
  desktop's socket power measurably; on a laptop, fewer wakeups mean more
  time in deep idle states, which this setup can't show.
- **Paused**, every player is at the idle baseline's power, but mpv leaves
  its PipeWire stream running, so the audio graph keeps cycling (47
  wakeups a second in PipeWire; on a real sound card the stream would
  keep running too), and VLC keeps redrawing its full-screen window (13
  frames a second composed). Utsushi stops its stream and draws nothing:
  4 wakeups a second.
- **Music.** Power is within noise for all three players, but Utsushi's
  long PipeWire cycles (170 ms, 8192 frames) show in the wakeups:
  15 a second in the player and 12 in PipeWire, against mpv's 192 and 94
  (1024-frame cycles) and VLC's 122 and 228 (through `pipewire-pulse`).
- **The compositor isn't the reason.** Under sway the three
  hardware-decoding players tie, with the same CPU and wakeup order as
  under Aizome.

## Subjects

| Name | Program | Configuration |
|---|---|---|
| `utsushi` | Utsushi (`aizome-utsushi`) from this checkout | as installed: VA-API when the codec allows, dmabufs to the compositor; `--no-sound` for video (its null output) |
| `mpv-default` | mpv 0.41.0 | `--no-config`: mpv's own defaults, which decode in software (`hwdec=no`) and render with `vo=gpu-next` (here Vulkan, RADV) |
| `mpv-sekka` | mpv 0.41.0 | `--no-config --include=desktop/tatami/mpv.conf`: the `/etc/mpv/mpv.conf` Sekka installs with Tatami (`gpu-api=opengl,vulkan`, `gpu-sw=yes`): software decoding, gpu-next on OpenGL |
| `mpv-vaapi-gpunext` | mpv 0.41.0 | `--no-config --hwdec=vaapi --vo=gpu-next`: VA-API decoding, frames imported into Vulkan and rendered by libplacebo |
| `mpv-vaapi-dmabuf` | mpv 0.41.0 | `--no-config --hwdec=vaapi --vo=dmabuf-wayland`: VA-API decoding, surfaces handed to the compositor as dmabufs (Utsushi's design) |
| `vlc-default` | VLC 3.0.24 | its defaults (Qt interface, `avcodec-hw=any`: VA-API where its decoder takes the stream), `--no-qt-privacy-ask`; under Xwayland, see below |
| `showtime` | GNOME's Showtime 50.0 (GTK 4, GStreamer) | its defaults; only in the trials with sound (it has no null output) |
| `haruna` | KDE's Haruna 1.8.1 (Qt 6, libmpv) | its defaults; only in the trials with sound |
| `baseline` | none | the same session with no player |

Video trials use null audio outputs (`--no-sound`, `--ao=null`,
`--aout=dummy`): nothing reaches PipeWire. mpv and VLC go full screen with
their own flags (`--fs`, `--fullscreen`), Utsushi, Showtime and Haruna
through the compositor (`aizomectl fullscreen ID on`; Utsushi takes it as
its own full screen, and its controls hide 2.5 s later without a
pointer). Each trial records the windows: every player's covered the
1920 × 1080 output full screen except mpv's with `--vo=dmabuf-wayland`
under Aizome, whose `--fs` didn't take (under sway it did): its window
stayed the picture's size, 1920 wide (1920 × 800 for the film, 1920 ×
1080 for 4K, 90 rows of it below the output), over the desktop, so the
compositor drew the same picture at the same scale but no black bars.
Batch `g` ran it again put full screen by the compositor, as Utsushi is:
the same work (see Results). Smoke runs' screenshots of the
others showed only the picture. For their counters the
harness adds mpv's IPC socket (`--input-ipc-server`), VLC's remote-control
interface on a Unix socket (`--extraintf=oldrc --rc-unix … --rc-fake-tty`)
and Utsushi's `UTSUSHI_STATS=1` (a log line every 5 s); each is asked
just before and just after the measured window. mpv keeps its
on-screen controller script as it ships, VLC its D-Bus (MPRIS) interface;
Utsushi with `--no-sound` has no MPRIS.

**VLC 3 needs X.** VLC 3.0's Qt interface doesn't start without an X
server. Without its interface (`-I dummy`) its Wayland window module
(`xdg_shell`) listed Aizome's globals and gave up, and the video fell back
to a text-art output. On a Wayland desktop VLC 3 therefore runs through
Xwayland: here `xwayland-satellite` (as an Aizome session starts it) or
sway's own Xwayland. Its cost is counted in the `xwayland` role and, like
everything else, in the socket power. VLC 4, with native Wayland, isn't
released.

## Setup

**The session.** Every trial starts a fresh private session and ends it:
a D-Bus session bus of its own (no service activation), then Aizome's
compositor headless at 1920 × 1080, scale 1, 60 Hz, rendering with GLES on
the Radeon (`AIZOME_RENDER_NODE=/dev/dri/renderD128`), with its shell, as
an Aizome session has them, but without the session's own services
(`--no-session`) and without Xwayland unless VLC needs it:

```sh
aizome --backend headless --headless-output 1920x1080@1 --socket az --no-xwayland --no-session
```

`XDG_RUNTIME_DIR`, `HOME` and the XDG directories are private and empty at
the start of each trial; `WAYLAND_DISPLAY`, `DISPLAY` and the user's D-Bus
address are unset. The shell is there because Aizome's apps put their menus
in its menu bar: without it the kit draws an in-window menu bar, even full
screen, which isn't how Utsushi is used. The shell's own idle work (about
50 wakeups a second, 0.1% of a core, headless) is the same in every trial
and in the baseline.

**One GPU.** Everything renders and decodes on the Radeon, so its work is
inside the socket power: EGL through Mesa only
(`__EGL_VENDOR_LIBRARY_FILENAMES=…/50_mesa.json`, `__GLX_VENDOR_LIBRARY_NAME=mesa`),
Vulkan through RADV only (`VK_DRIVER_FILES=…/radeon_icd.x86_64.json`),
`LIBVA_DRIVER_NAME=radeonsi`, `VDPAU_DRIVER=radeonsi`, and no CUDA
devices (`CUDA_VISIBLE_DEVICES=`: in a smoke run without it, Showtime's
GStreamer opened the NVIDIA card through its `nvcodec` plugin and didn't
decode on the Radeon; with it, Showtime uses VA-API like the others;
set from batch `b` on, and the batches before it had no player holding an
NVIDIA device either). Each trial records
the DRM devices every process holds open and their PCI addresses (from
`/proc/PID/fdinfo`): every player, the compositor and Xwayland had only
`/dev/dri/renderD128` (0000:78:00.0, the Radeon) and no `/dev/nvidia*`
file open.

**Decoding.** radeonsi on this Raphael iGPU (VCN 3.1) decodes H.264
(Constrained Baseline, Main, High), HEVC Main and Main 10, VP9 profiles 0
and 2 and AV1 profile 0 (`vainfo`). Whether a player decoded on the GPU is
read from its own report (mpv's `hwdec-current`, Utsushi's `gpu: true` and
`path: Dmabuf` in `UTSUSHI_STATS`) and, for all of them, from the DRM
`dec` engine time its process accumulated during the window.

**Sound.** Nothing was audible. Video trials use each player's null output
and never connect to PipeWire. Music, paused and with-sound trials send the player's
stream to a null sink the harness creates in the user's PipeWire
(`support.null-audio-sink`, `node.name=perf-null`, 48 kHz stereo,
`priority.session=0`) and removes at the end; the player is pointed at it
(`PIPEWIRE_NODE`, `PULSE_SINK`, mpv `--audio-device=pipewire/perf-null`),
and the harness checks from `pw-dump`, after the player starts and again
before the window, that its streams link to that sink and nowhere else
(it would stop the trial otherwise). The default sink was checked before
and after each batch and never changed. PipeWire's daemons (`pipewire`,
`pipewire-pulse`, `wireplumber`) are counted in those trials. The user's
PipeWire allows quanta up to 8192 frames (`clock.max-quantum`), as Aizome's
module sets it.

## Clips

Freely licensed sources (Creative Commons Attribution 3.0, © Blender
Foundation, www.blender.org):

| Source | URL | SHA-256 |
|---|---|---|
| *Tears of Steel* (2012), 1080p, 1920 × 800, 24 fps (H.264 + MP3 in MOV) | https://download.blender.org/demo/movies/ToS/tears_of_steel_1080p.mov.zip | zip `d87a41de…7a5`, mov `99486359…a15` |
| *Big Buck Bunny* (2008), "sunflower" 3840 × 2160, 60 fps (H.264 + MP3 + AC-3 in MP4) | https://download.blender.org/demo/movies/BBB/bbb_sunflower_2160p_60fps_normal.mp4.zip | zip `8f1d53cf…19`, mp4 `35db9a00…36` |

`tests/performance/media/encode.sh` cut 90 s from each (*Tears of Steel*
from 3:00, live action; *Big Buck Bunny* from 2:00, animation) with FFmpeg
9.0.2 from Sekka's Nixpkgs (full sums in `data/media/clips.sha256` and
`sources.sha256`):

| Clip | Video | Audio | Mean bit rate |
|---|---|---|---|
| `tos-1080p24-h264.mp4` | H.264 High, 1920 × 800, 24 fps, x264 `medium`, 8 Mbit/s (VBV 12 Mbit/s) | AAC 160 kbit/s, 44.1 kHz | 8.1 Mbit/s |
| `tos-1080p24-hevc10.mp4` | HEVC Main 10, 1920 × 800, 24 fps, x265 `medium`, 5 Mbit/s | AAC 160 kbit/s | 5.3 Mbit/s |
| `tos-1080p24-vp9.webm` | VP9 profile 0, 1920 × 800, 24 fps, libvpx `good`/`cpu-used 2`, 5 Mbit/s | Opus 128 kbit/s | 5.2 Mbit/s |
| `tos-1080p24-av1.webm` | AV1 Main, 8-bit, 1920 × 800, 24 fps, SVT-AV1 preset 6, 4 Mbit/s target | Opus 128 kbit/s | 2.8 Mbit/s |
| `bbb-2160p30-hevc10.mp4` | HEVC Main 10, 3840 × 2160, 30 fps (every other frame of the 60 fps source), x265 `fast`, 16 Mbit/s | AAC 160 kbit/s (5.1 downmixed) | 15.8 Mbit/s |
| `bbb-2160p60-av1.webm` | AV1 Main, 8-bit, 3840 × 2160, 60 fps, SVT-AV1 preset 8, 16 Mbit/s target | Opus 128 kbit/s | 11.0 Mbit/s |
| `bbb-music.mp3` | – | MP3 192 kbit/s, 44.1 kHz stereo | 192 kbit/s |
| `bbb-music.opus` | – | Opus 128 kbit/s, 48 kHz stereo | 128 kbit/s |
| `pattern-1080p24-h264.mp4` | FFmpeg `testsrc2`, 1920 × 1080, 24 fps, H.264 6 Mbit/s, 20 s (first-frame timing only) | 440 Hz tone, AAC | 6.1 Mbit/s |

The film is scope (2.40:1), so full screen at 16:9 it is letterboxed, as
films are; each player draws or leaves its own black bars. The media files
aren't in the repository.

## Method

`tests/performance/media/bench.py` runs a plan (`plans/*.json`) in rounds;
each round runs every subject of the plan once, in an order shuffled with a
recorded seed, the idle baseline among them. A trial:

1. waits until the machine is quiet (CPU use outside our processes under
   one core over 2 s, at most 90 s) and records the load average;
2. starts the private session, waits 3 s for the shell to settle;
3. starts the player on the clip, waits for its window, puts it full screen
   (Utsushi, Showtime and Haruna through the compositor's IPC, or
   `swaymsg` under sway; mpv and VLC by their own flags) and waits 5 s (for pause trials, then pauses it through its
   own interface, MPRIS for Utsushi, IPC for mpv, the rc interface for VLC,
   and waits 3 s more);
4. measures a window of 60 s (30 s paused): the shared `powermeter` samples
   the AMD socket power (PPT) every 10 ms and integrates it; meanwhile the
   harness samples the iGPU's `gpu_busy_percent` and clock every 100 ms,
   the processor's temperature and every thread of the session's processes
   once a second;
5. stops everything.

Per trial and per process role (player, compositor, shell, Xwayland,
D-Bus, PipeWire's daemons) it keeps: CPU time from each thread's
`schedstat` (ns), voluntary and involuntary context switches summed over
threads, DRM engine time per engine from fdinfo (`dec` is the video
decoder, `gfx` the 3D engine), the player's PSS and RSS
(`smaps_rollup`, summed over its process tree) at the end of the window,
the compositor's frame count, and the player's own counters before and
after: mpv's `frame-drop-count`, `decoder-frame-drop-count`,
`vo-delayed-frame-count` and `playback-time` over its IPC socket, VLC's
`stats` (frames displayed and lost) over its rc interface, and Utsushi's
`UTSUSHI_STATS` log line (shown, dropped, decoded, presented; every 5 s,
so its frame counts are over the nearest 5-second span).

**Power.** The figure to compare is the delta over the idle baseline:
a trial's mean power minus the idle floor at its time, interpolated
between the baselines run before and after it in the same batch and
mode (under either compositor: the floor is the machine's; the nearer
one alone at a batch's ends). When those two baselines differ by more
than 0.5 W, the floor moved under the trial (the rest of the machine
changed) and its power is left out (`floor_moved`); this happened to
eight trials, all in batches `e` and `f`, while the user was at the
desktop. The socket's idle floor here was about 18.2 W at night (the
`performance` governor and energy preference keep it high) and
18.6–20.8 W, moving, once the user was using the desktop; it drifts by a
few tenths of a watt over half an hour with temperature, and the sensor
reports whole watts: a delta under about 0.2 W is within noise. J/min is the delta × 60 s. The delta
against the batch's median baseline is in the data as a check
(`delta_median_w`); it changes no conclusion.

**Interference.** Other agents' work and the user's desktop share the
machine; measurements held an exclusive lock that kept the other
benchmarks and builds off, but not everything. A trial whose CPU use
outside the session (the system's busy time minus the session's
processes) exceeded the batch's baselines' by more than a quarter of a
core is dropped from the summaries and counted (`excluded_interference`);
it stays in `trials.csv` flagged `interfered`.

**First frame.** `ttff.py` starts each player windowed on the test pattern
and screenshots a 96 × 54 area in the middle of its window through the
compositor every 5–15 ms until most of it is strongly coloured; the time
from the launch to that screenshot's request is the time to the first
frame (late by at most one interval). Five launches each, interleaved,
files in the page cache.

## Limits

- **Headless, one desktop.** Aizome's headless backend composes every
  frame into an off-screen buffer on the Radeon; there is no display
  engine and no KMS planes (sway's headless output did take mpv's
  full-screen buffer directly, see the sway section). On a real display Aizome's DRM backend can
  show a topmost dmabuf (Utsushi's and mpv's `dmabuf-wayland` picture
  while nothing covers it) on a plane without composing it, which would
  lower those two further; players that render the picture themselves
  (gpu-next, VLC) can be scanned out too when their buffer is the whole
  output. None of that is measured here. The displays of this desktop
  are on the NVIDIA card, which the socket sensor doesn't see.
- **A desktop processor.** The socket's floor is about 18 W and moves by
  a few tenths of a watt with temperature; the sensor reads whole watts.
  Deltas under about 0.2 W, which is where every GPU-decoding player is
  at 1080p, are at the edge of what this can resolve, and the medians of
  the hardware-decoding players overlap at 1080p. A laptop's battery
  would show the same work as a larger share of a much lower floor; this
  isn't a battery-life figure.
- **The iGPU's own counters.** `gpu_busy_percent` and the `sclk` reading
  of this Raphael iGPU, sampled every 100 ms, are in the data but
  disagree with the DRM engine times (a trial with 42% of the 3D engine
  busy can read 1% busy at the lowest clock); the tables use the engine
  times from fdinfo instead.
- **What each player is asked to do differs.** mpv's gpu-next renders the
  picture itself with its high-quality scalers; dmabuf-wayland and
  Utsushi leave scaling and colour conversion to the compositor (whose
  conversion doesn't yet tell BT.709 from BT.601: Utsushi's documented
  limit). VLC 3 goes through Xwayland. These are the players' own
  choices, measured as they make them, not a like-for-like renderer
  comparison.
- **Null audio for video.** Video trials send no sound anywhere, so they
  leave out PipeWire's share, which the music and paused trials (and
  the trials with sound) measure.
- **Shared machine.** Four benchmark agents shared it, and from about
  10:00 the user was using the desktop (its apps on the NVIDIA card),
  which raised the socket's floor by up to 2.6 W in batches `c`, `e` and
  `f` and moved it during `e` and `f`. The lock kept the other benchmarks and builds off during
  measurements, and trials disturbed by anything using CPU were dropped
  (`excluded_interference`, which leaves two trials in a few groups), but
  background noise remains, and one outlier that the CPU check didn't
  catch (mpv `dmabuf-wayland` on 1080p HEVC, +2.2 W) is in its group's
  range, not its median.
- **The harness's own reads.** Utsushi's counters come from a log line
  every 5 s (its frame counts are over the nearest 5 s span), mpv's
  status line goes to a log file once a second, VLC answers its rc socket;
  the harness itself samples sysfs at 10 Hz in every trial, the baseline
  included.

## Reproducing it

On a machine with an AMD processor whose amdgpu driver exposes the `PPT`
power sensor (or adapt `POWERMETER`), from this checkout:

```sh
# 0. The shared power meter and locks (docs/performance/README.md):
#    tests/performance/powermeter.c built to $PERF_DIR/bin/powermeter
#    (PERF_DIR defaults to $TMPDIR/aizome-perf); run.sh takes the measurement
#    lock with tests/performance/lock-measure.

# 1. Sources (CC BY 3.0, Blender Foundation), unzipped:
mkdir -p src/tos src/bbb
curl -L -o src/tos/tears_of_steel_1080p.mov.zip \
  https://download.blender.org/demo/movies/ToS/tears_of_steel_1080p.mov.zip
curl -L -o src/bbb/bbb_sunflower_2160p_60fps_normal.mp4.zip \
  https://download.blender.org/demo/movies/BBB/bbb_sunflower_2160p_60fps_normal.mp4.zip
(cd src/tos && unzip *.zip) && (cd src/bbb && unzip *.zip)
(cd src && sha256sum -c ../docs/performance/data/media/sources.sha256)

# 2. Clips (every core). Their sums are in data/media/clips.sha256; the
#    encoders' threading may make other machines' bytes differ.
nix shell --inputs-from . nixpkgs#ffmpeg -c \
  tests/performance/media/encode.sh src "$PERF_DIR/media/clips"

# 3. Batches (each one lock hold of under 45 minutes); run.sh resolves
#    Aizome from this flake and every other program from its Nixpkgs:
M=tests/performance/media
$M/run.sh $M/plans/a-h264-1080p-paused.json      out/a
$M/run.sh $M/plans/d-hevc10-2160p30-mp3.json     out/d
$M/run.sh $M/plans/b-av1-2160p60-h264-reps.json  out/b
$M/batch.sh $M/plans/c-hevc10-av1-1080p.json     out/c out/ttff   # + first-frame timing
$M/run.sh $M/plans/e-sway-vp9.json               out/e
$M/run.sh $M/plans/f-sound-desktops-opus.json    out/f
$M/run.sh $M/plans/g-dmabuf-fullscreen.json      out/g   # mpv dmabuf-wayland, full screen

# 4. Dataset and tables:
PATH=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#python3)/bin:$PATH \
  $M/collect.sh out docs/performance/data/media
```

Trials with sound create and remove a null sink in the running user's
PipeWire and refuse to run if the player's stream links anywhere else.
Everything else is private to the trial: a headless compositor on the
Radeon render node, its own D-Bus and runtime directory. The batch
letters are names, not the order they ran in (a, d, b, c, e, f, g).
Batch `g` was stopped by hand after a round and marked with a file named
`exclude-from-summary` in its directory: `collect.sh` keeps such a batch
as data and leaves it out of the tables.
