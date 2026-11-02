
#### aizome · play · tos-1080p24-h264

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.20 (0.19–0.53) | 12 | 0.83 | 146 | 0.94 | 130 | – | – | 3.2 / 3.8 | 76 | 0 | 6 |
| mpv-vaapi-dmabuf | 0.42 (0.39–0.62) | 25 | 1.44 | 452 | 1.03 | 130 | – | – | 3.4 / 3.8 | 112 | 0 | 5 |
| mpv-vaapi-gpunext | 0.51 (0.27–0.97) | 30 | 1.96 | 410 | 0.89 | 160 | – | – | 3.5 / 7.4 | 159 | 0 | 6 |
| mpv-default | 1.36 (1.35–1.53) | 82 | 12.72 | 366 | 1.10 | 180 | – | – | 0.0 / 10.2 | 180 | 0 | 3 |
| mpv-sekka | 1.39 (1.38–1.60) | 83 | 12.67 | 793 | 0.68 | 153 | – | – | 0.0 / 8.2 | 201 | 0 | 3 |
| vlc-default | 0.46 (0.35–0.62) | 27 | 1.53 | 264 | 0.76 | 129 | 0.34 | – | 2.9 / 4.7 | 141 | 0 | 6 |

#### aizome · play · tos-1080p24-hevc10

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.18 (0.03–0.20) | 11 | 0.70 | 147 | 0.73 | 130 | – | – | 4.2 / 3.8 | 70 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.40 (0.37–2.16) | 24 | 1.17 | 455 | 0.80 | 129 | – | – | 4.5 / 3.8 | 112 | 0 | 3 |
| mpv-default | 1.43 (1.16–1.73) | 86 | 14.06 | 369 | 0.84 | 180 | – | – | 0.0 / 10.2 | 186 | 0 | 3 |
| vlc-default | 1.43 (1.21–1.55) | 86 | 14.00 | 397 | 0.61 | 128 | 0.27 | – | 0.0 / 7.8 | 312 | 0 | 3 |

#### aizome · play · tos-1080p24-vp9

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.19 (-0.09–0.32) | 12 | 0.86 | 147 | 0.84 | 128 | – | – | 4.4 / 3.8 | 61 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.34 (0.34–0.34) | 20 | 1.32 | 447 | 0.93 | 129 | – | – | 4.6 / 3.8 | 82 | 0 | 3 (power 1) |
| mpv-default | 1.38 (1.22–1.41) | 83 | 9.61 | 372 | 0.97 | 178 | – | – | 0.0 / 10.2 | 126 | 0 | 3 |
| vlc-default | 0.21 (0.21–0.21) | 13 | 1.72 | 316 | 0.61 | 128 | 0.28 | – | 4.4 / 4.7 | 119 | 0 | 3 (power 1) |

#### aizome · play · tos-1080p24-av1

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.27 (0.09–0.34) | 16 | 0.71 | 147 | 0.85 | 129 | – | – | 3.5 / 3.8 | 58 | 0 | 5 |
| mpv-vaapi-dmabuf | 0.17 (-0.00–0.27) | 10 | 1.11 | 438 | 0.83 | 129 | – | – | 3.7 / 3.8 | 78 | 0 | 3 |
| mpv-default | 1.76 (1.67–1.79) | 105 | 8.81 | 2230 | 0.73 | 178 | – | – | 0.0 / 11.6 | 184 | 0 | 3 |
| vlc-default | 1.39 (1.32–1.77) | 83 | 8.50 | 2092 | 0.49 | 128 | 0.21 | – | 0.0 / 7.4 | 217 | 0 | 5 |

#### aizome · play · bbb-2160p30-hevc10

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.56 (0.53–0.57) | 34 | 1.15 | 178 | 1.11 | 162 | – | – | 19.5 / 5.3 | 122 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.62 (0.54–0.68) | 37 | 1.82 | 528 | 1.23 | 160 | – | – | 19.8 / 5.7 | 215 | 0 | 3 |
| mpv-vaapi-gpunext | 4.34 (4.31–4.43) | 260 | 1.87 | 485 | 0.66 | 215 | – | – | 19.9 / 42.4 | 250 | 0 | 3 |
| mpv-default | 8.27 (8.24–8.31) | 496 | 45.57 | 440 | 0.97 | 223 | – | – | 0.0 / 45.2 | 460 | 0 | 3 |
| mpv-sekka | 9.96 (9.82–10.13) | 598 | 47.46 | 939 | 0.61 | 161 | – | – | 0.0 / 53.8 | 989 | 0 | 3 |
| vlc-default | 0.85 (0.69–0.93) | 51 | 2.01 | 334 | 0.85 | 160 | 0.38 | – | 18.7 / 6.4 | 406 | 0 | 3 |

#### aizome · play · bbb-2160p60-av1

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.89 (0.83–1.26) | 54 | 1.05 | 328 | 1.19 | 332 | – | – | 36.6 / 9.4 | 73 | 0 | 3 |
| mpv-vaapi-dmabuf | 1.06 (1.02–1.08) | 64 | 1.63 | 796 | 1.32 | 326 | – | – | 36.7 / 9.2 | 100 | 0 | 3 |
| mpv-vaapi-gpunext | 7.62 (7.44–7.64) | 457 | 1.99 | 667 | 0.68 | 346 | – | – | 36.7 / 71.2 | 140 | 557 | 3 |
| mpv-default | 13.32 (13.20–13.87) | 799 | 44.55 | 11383 | 0.85 | 415 | – | – | 0.0 / 81.6 | 469 | 145 | 3 |
| vlc-default | 7.60 (7.48–7.70) | 456 | 51.57 | 11119 | 0.69 | 285 | 0.30 | – | 0.0 / 24.1 | 674 | 315 | 3 |

#### aizome · pause · tos-1080p24-h264

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | -0.10 (-0.17–-0.00) | -6 | 0.01 | 4 | 0.02 | 7 | 0.00 | 0 | 0.0 / 0.0 | 79 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.03 (-0.10–0.20) | 2 | 0.00 | 2 | 0.02 | 7 | 0.09 | 47 | 0.0 / 0.0 | 112 | 0 | 3 |
| mpv-default | -0.02 (-0.14–0.09) | -1 | 0.00 | 2 | 0.02 | 7 | 0.09 | 47 | 0.0 / 0.0 | 179 | 0 | 2 |
| vlc-default | 0.04 (0.03–0.14) | 2 | 0.35 | 41 | 0.48 | 74 | 0.22 | 1 | 0.0 / 2.6 | 139 | 0 | 3 |

#### aizome · audio · bbb-music.mp3

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.11 (0.06–0.27) | 6 | 0.17 | 15 | 0.07 | 12 | 0.02 | 12 | – / 0.1 | 43 | 0 | 3 |
| mpv-default | 0.12 (0.12–0.18) | 7 | 0.84 | 192 | 0.01 | 7 | 0.13 | 94 | – / 0.0 | 52 | 0 | 3 |
| vlc-default | 0.16 (0.15–0.34) | 10 | 0.96 | 122 | 0.16 | 26 | 0.55 | 228 | – / 0.4 | 70 | 0 | 3 |

#### aizome · audio · bbb-music.opus

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.10 (0.04–0.15) | 6 | 0.20 | 15 | 0.06 | 12 | 0.20 | 36 | – / 0.1 | 35 | 0 | 2 |
| mpv-default | 0.37 (0.18–1.60) | 22 | 0.74 | 213 | 0.01 | 7 | 0.26 | 118 | – / 0.0 | 42 | 0 | 3 |
| vlc-default | 0.27 (0.26–1.09) | 16 | 0.58 | 84 | 0.06 | 12 | 0.47 | 266 | – / 0.1 | 58 | 0 | 3 |

#### aizome · playsound · tos-1080p24-h264

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.27 (0.13–0.41) | 16 | 0.72 | 151 | 0.68 | 130 | 0.23 | 71 | 3.2 / 3.7 | 64 | 0 | 3 (power 2) |
| mpv-vaapi-dmabuf | 0.27 (-0.54–0.40) | 16 | 1.24 | 521 | 0.73 | 129 | 0.25 | 118 | 3.4 / 3.7 | 100 | 0 | 3 |
| mpv-default | 2.35 (2.02–2.69) | 141 | 11.94 | 437 | 0.70 | 178 | 0.25 | 118 | 0.0 / 10.2 | 168 | 0 | 3 (power 2) |
| vlc-default | 0.28 (-0.35–0.52) | 17 | 1.32 | 332 | 0.52 | 129 | 0.63 | 260 | 2.9 / 4.7 | 132 | 0 | 3 |
| showtime | 0.40 (0.11–0.69) | 24 | 1.61 | 543 | 0.50 | 154 | 0.28 | 114 | 3.2 / 5.0 | 159 | – | 3 (power 2) |
| haruna | 0.82 (0.34–0.96) | 49 | 2.29 | 814 | 0.49 | 129 | 0.24 | 118 | 3.7 / 9.6 | 301 | – | 3 |

#### sway · play · tos-1080p24-h264

| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 0.38 (0.36–0.38) | 23 | 0.73 | 145 | 0.48 | 98 | – | – | 4.2 / 3.7 | 67 | 0 | 3 |
| mpv-vaapi-dmabuf | 0.39 (0.36–0.53) | 23 | 1.45 | 459 | 0.29 | 73 | – | – | 3.2 / 0.0 | 102 | 0 | 3 |
| vlc-default | 0.25 (0.20–0.31) | 15 | 1.44 | 264 | 0.22 | 48 | 0.00 | – | 2.9 / 3.8 | 132 | 0 | 3 (power 2) |
