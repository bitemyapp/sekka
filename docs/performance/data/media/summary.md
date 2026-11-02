
### aizome · play · -

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 14 | 18.37 | 0.00 (0.00–0.00) | 0 | – | – | 0.01 | 7 | 0.09/–/– | – | 0.0 | 600 | – | – | – |  |

### aizome · play · tos-1080p24-h264

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 6 | 18.45 | 0.20 (0.19–0.53) | 12 | 0.83 | 148 | 0.94 | 130 | 0.08/–/– | 3.2 | 3.8 | 635 | 76 | 1440 | 0 | h264 Dmabuf |
| mpv-vaapi-dmabuf | 5 | 18.64 | 0.42 (0.39–0.62) | 25 | 1.44 | 456 | 1.03 | 130 | 0.07/–/– | 3.4 | 3.8 | 600 | 112 | – | 0 | vaapi dmabuf-wayland nv12 |
| mpv-vaapi-gpunext | 6 | 18.74 | 0.51 (0.27–0.97) | 30 | 1.96 | 417 | 0.89 | 160 | 0.07/–/– | 3.5 | 7.4 | 660 | 159 | – | 0 | vaapi gpu-next nv12 |
| mpv-default | 3 | 19.66 | 1.36 (1.35–1.53) | 82 | 12.72 | 383 | 1.10 | 180 | 0.07/–/– | 0.0 | 10.2 | 792 | 180 | – | 0 | no gpu-next yuv420p |
| mpv-sekka | 3 | 19.60 | 1.39 (1.38–1.60) | 83 | 12.67 | 808 | 0.68 | 153 | 0.07/–/– | 0.0 | 8.2 | 920 | 201 | – | 0 | no gpu-next yuv420p |
| vlc-default | 6 | 18.68 | 0.46 (0.35–0.62) | 27 | 1.53 | 268 | 0.76 | 129 | 0.08/0.34/– | 2.9 | 4.7 | 673 | 141 | 1464 | 0 |  |

### aizome · play · tos-1080p24-hevc10

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 20.66 | 0.18 (0.03–0.20) | 11 | 0.70 | 149 | 0.73 | 130 | 0.07/–/– | 4.2 | 3.8 | 651 | 70 | 1440 | 0 | hevc Dmabuf |
| mpv-vaapi-dmabuf | 3 | 21.04 | 0.40 (0.37–2.16) | 24 | 1.17 | 458 | 0.80 | 129 | 0.07/–/– | 4.5 | 3.8 | 643 | 112 | – | 0 | vaapi dmabuf-wayland p010 |
| mpv-default | 3 | 22.01 | 1.43 (1.16–1.73) | 86 | 14.06 | 384 | 0.84 | 180 | 0.08/–/– | 0.0 | 10.2 | 917 | 186 | – | 0 | no gpu-next yuv420p10 |
| vlc-default | 3 | 22.00 | 1.43 (1.21–1.55) | 86 | 14.00 | 411 | 0.61 | 128 | 0.07/0.27/– | 0.0 | 7.8 | 853 | 312 | 1458 | 0 |  |

### aizome · play · tos-1080p24-vp9

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 20.61 | 0.19 (-0.09–0.32) | 12 | 0.86 | 150 | 0.84 | 128 | 0.07/–/– | 4.4 | 3.8 | 603 | 61 | 1440 | 0 | vp9 Dmabuf |
| mpv-vaapi-dmabuf | 3 | 19.06 | 0.34 (0.34–0.34) | 20 | 1.32 | 450 | 0.93 | 129 | 0.07/–/– | 4.6 | 3.8 | 603 | 82 | – | 0 | vaapi dmabuf-wayland nv12 |
| mpv-default | 3 | 21.96 | 1.38 (1.22–1.41) | 83 | 9.61 | 382 | 0.97 | 178 | 0.07/–/– | 0.0 | 10.2 | 645 | 126 | – | 0 | no gpu-next yuv420p |
| vlc-default | 3 | 19.14 | 0.21 (0.21–0.21) | 13 | 1.72 | 319 | 0.61 | 128 | 0.07/0.28/– | 4.4 | 4.7 | 840 | 119 | 1472 | 0 |  |

### aizome · play · tos-1080p24-av1

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 5 | 20.69 | 0.27 (0.09–0.34) | 16 | 0.71 | 148 | 0.85 | 129 | 0.08/–/– | 3.5 | 3.8 | 603 | 58 | 1440 | 0 | av1 Dmabuf |
| mpv-vaapi-dmabuf | 3 | 20.73 | 0.17 (-0.00–0.27) | 10 | 1.11 | 441 | 0.83 | 129 | 0.07/–/– | 3.7 | 3.8 | 600 | 78 | – | 0 | vaapi dmabuf-wayland nv12 |
| mpv-default | 3 | 22.26 | 1.76 (1.67–1.79) | 105 | 8.81 | 2257 | 0.73 | 178 | 0.07/–/– | 0.0 | 11.6 | 866 | 184 | – | 0 | no gpu-next yuv420p |
| vlc-default | 5 | 21.95 | 1.39 (1.32–1.77) | 83 | 8.50 | 2113 | 0.49 | 128 | 0.07/0.21/– | 0.0 | 7.4 | 714 | 217 | 1471 | 0 |  |

### aizome · play · bbb-2160p30-hevc10

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 18.66 | 0.56 (0.53–0.57) | 34 | 1.15 | 179 | 1.11 | 162 | 0.08/–/– | 19.5 | 5.3 | 1417 | 122 | 1800 | 0 | hevc Dmabuf |
| mpv-vaapi-dmabuf | 3 | 18.79 | 0.62 (0.54–0.68) | 37 | 1.82 | 534 | 1.23 | 160 | 0.07/–/– | 19.8 | 5.7 | 600 | 215 | – | 0 | vaapi dmabuf-wayland p010 |
| mpv-vaapi-gpunext | 3 | 22.48 | 4.34 (4.31–4.43) | 260 | 1.87 | 491 | 0.66 | 215 | 0.06/–/– | 19.9 | 42.4 | 605 | 250 | – | 0 | vaapi gpu-next p010 |
| mpv-default | 3 | 26.40 | 8.27 (8.24–8.31) | 496 | 45.57 | 463 | 0.97 | 223 | 0.08/–/– | 0.0 | 45.2 | 600 | 460 | – | 0 | no gpu-next yuv420p10 |
| mpv-sekka | 3 | 28.16 | 9.96 (9.82–10.13) | 598 | 47.46 | 971 | 0.61 | 161 | 0.08/–/– | 0.0 | 53.8 | 1537 | 989 | – | 0 | no gpu-next yuv420p10 |
| vlc-default | 3 | 19.05 | 0.85 (0.69–0.93) | 51 | 2.01 | 341 | 0.85 | 160 | 0.07/0.38/– | 18.7 | 6.4 | 600 | 406 | 1838 | 0 |  |

### aizome · play · bbb-2160p60-av1

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 19.13 | 0.89 (0.83–1.26) | 54 | 1.05 | 330 | 1.19 | 332 | 0.07/–/– | 36.6 | 9.4 | 802 | 73 | 3600 | 0 | av1 Dmabuf |
| mpv-vaapi-dmabuf | 3 | 19.28 | 1.06 (1.02–1.08) | 64 | 1.63 | 801 | 1.32 | 326 | 0.07/–/– | 36.7 | 9.2 | 789 | 100 | – | 0 | vaapi dmabuf-wayland nv12 |
| mpv-vaapi-gpunext | 3 | 25.86 | 7.62 (7.44–7.64) | 457 | 1.99 | 673 | 0.68 | 346 | 0.05/–/– | 36.7 | 71.2 | 1870 | 140 | – | 557 | vaapi gpu-next nv12 |
| mpv-default | 3 | 31.57 | 13.32 (13.20–13.87) | 799 | 44.55 | 11459 | 0.85 | 415 | 0.07/–/– | 0.0 | 81.6 | 2080 | 469 | – | 145 | no gpu-next yuv420p |
| vlc-default | 3 | 25.84 | 7.60 (7.48–7.70) | 456 | 51.57 | 11184 | 0.69 | 285 | 0.08/0.30/– | 0.0 | 24.1 | 1050 | 674 | 3359 | 315 |  |

### aizome · pause · -

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 3 | 18.16 | 0.00 (0.00–0.00) | 0 | – | – | 0.01 | 7 | 0.08/–/0.00 | – | 0.0 | 600 | – | – | – |  |

### aizome · pause · tos-1080p24-h264

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 18.15 | -0.10 (-0.17–-0.00) | -6 | 0.01 | 4 | 0.02 | 7 | 0.09/–/0.00 | 0.0 | 0.0 | 600 | 79 | 25 | 0 | h264 Dmabuf |
| mpv-vaapi-dmabuf | 3 | 18.32 | 0.03 (-0.10–0.20) | 2 | 0.00 | 2 | 0.02 | 7 | 0.08/–/0.09 | 0.0 | 0.0 | 600 | 112 | – | 0 | vaapi dmabuf-wayland nv12 |
| mpv-default | 2 | 18.18 | -0.02 (-0.14–0.09) | -1 | 0.00 | 2 | 0.02 | 7 | 0.09/–/0.09 | 0.0 | 0.0 | 600 | 179 | – | 0 | no gpu-next yuv420p |
| vlc-default | 3 | 18.30 | 0.04 (0.03–0.14) | 2 | 0.35 | 42 | 0.48 | 74 | 0.08/0.21/0.01 | 0.0 | 2.6 | 642 | 139 | 0 | 0 |  |

### aizome · audio · -

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 6 | 18.40 | 0.00 (0.00–0.00) | 0 | – | – | 0.01 | 7 | 0.09/–/0.10 | – | 0.0 | 600 | – | – | – |  |

### aizome · audio · bbb-music.mp3

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 18.23 | 0.11 (0.06–0.27) | 6 | 0.17 | 15 | 0.07 | 12 | 0.09/–/0.02 | – | 0.1 | 600 | 43 | 0 | 0 | None |
| mpv-default | 3 | 18.21 | 0.12 (0.12–0.18) | 7 | 0.84 | 208 | 0.01 | 7 | 0.08/–/0.13 | – | 0.0 | 600 | 52 | – | 0 |  |
| vlc-default | 3 | 18.33 | 0.16 (0.15–0.34) | 10 | 0.96 | 125 | 0.16 | 26 | 0.08/0.14/0.41 | – | 0.4 | 600 | 70 | 0 | 0 |  |

### aizome · audio · bbb-music.opus

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 2 | 18.84 | 0.10 (0.04–0.15) | 6 | 0.20 | 15 | 0.06 | 12 | 0.07/–/0.20 | – | 0.1 | 600 | 35 | 0 | 0 | None |
| mpv-default | 3 | 19.09 | 0.37 (0.18–1.60) | 22 | 0.74 | 214 | 0.01 | 7 | 0.07/–/0.26 | – | 0.0 | 600 | 42 | – | 0 |  |
| vlc-default | 3 | 18.98 | 0.27 (0.26–1.09) | 16 | 0.58 | 85 | 0.06 | 12 | 0.08/0.04/0.43 | – | 0.1 | 603 | 58 | 0 | 0 |  |

### aizome · playsound · -

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 3 | 18.91 | 0.00 (0.00–0.00) | 0 | – | – | 0.01 | 7 | 0.09/–/0.20 | – | 0.0 | 600 | – | – | – |  |

### aizome · playsound · tos-1080p24-h264

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 19.14 | 0.27 (0.13–0.41) | 16 | 0.72 | 153 | 0.68 | 130 | 0.07/–/0.23 | 3.2 | 3.7 | 762 | 64 | 1440 | 0 | h264 Dmabuf |
| mpv-vaapi-dmabuf | 3 | 19.17 | 0.27 (-0.54–0.40) | 16 | 1.24 | 542 | 0.73 | 129 | 0.07/–/0.25 | 3.4 | 3.7 | 674 | 100 | – | 0 | vaapi dmabuf-wayland nv12 |
| mpv-default | 3 | 20.92 | 2.35 (2.02–2.69) | 141 | 11.94 | 459 | 0.70 | 178 | 0.07/–/0.25 | 0.0 | 10.2 | 600 | 168 | – | 0 | no gpu-next yuv420p |
| vlc-default | 3 | 19.19 | 0.28 (-0.35–0.52) | 17 | 1.32 | 337 | 0.52 | 129 | 0.07/0.23/0.40 | 2.9 | 4.7 | 600 | 132 | 1458 | 0 |  |
| showtime | 3 | 19.59 | 0.40 (0.11–0.69) | 24 | 1.61 | 548 | 0.50 | 154 | 0.06/–/0.28 | 3.2 | 5.0 | 717 | 159 | – | – |  |
| haruna | 3 | 19.83 | 0.82 (0.34–0.96) | 49 | 2.29 | 831 | 0.49 | 129 | 0.07/–/0.24 | 3.7 | 9.6 | 603 | 301 | – | – |  |

### sway · play · -

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline | 3 | 20.64 | 0.00 (0.00–0.00) | 0 | – | – | 0.00 | 0 | –/–/– | – | 0.0 | 600 | – | – | – |  |

### sway · play · tos-1080p24-h264

| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| utsushi | 3 | 21.09 | 0.38 (0.36–0.38) | 23 | 0.73 | 147 | 0.48 | 98 | –/–/– | 4.2 | 3.7 | 770 | 67 | 1440 | 0 | h264 Dmabuf |
| mpv-vaapi-dmabuf | 3 | 21.08 | 0.39 (0.36–0.53) | 23 | 1.45 | 463 | 0.29 | 73 | –/–/– | 3.2 | 0.0 | 600 | 102 | – | 0 | vaapi dmabuf-wayland nv12 |
| vlc-default | 3 | 19.82 | 0.25 (0.20–0.31) | 15 | 1.44 | 270 | 0.22 | 48 | –/0.00/– | 2.9 | 3.8 | 600 | 132 | 1464 | 0 |  |
