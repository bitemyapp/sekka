### Boot and login (4096 MiB; median, min–max over trials)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Trials | 3 | 3 | 3 | 3 |
| Boot: QEMU start → login screen settled (s) | 6.36 (6.27–6.36) | 7.56 (7.46–7.67) | 8.56 (8.47–8.56) | 5.07 (4.96–5.86) |
| &nbsp;&nbsp;kernel + initrd + userspace (systemd-analyze, s) | 0.20 + 2.73 + 2.07 | 0.20 + 2.74 + 2.23 | 0.20 + 2.75 + 1.97 | 0.20 + 1.84 + 1.88 |
| &nbsp;&nbsp;display manager started (host timeline, s) | 5.24 (5.24–5.31) | 5.48 (5.42–5.57) | 5.21 (5.21–5.24) | – |
| Login: Enter → desktop settled (s) | 1.23 (1.13–1.24) | 2.04 (2.04–2.04) | 5.24 (5.14–5.34) | – |
| &nbsp;&nbsp;Enter → first change on screen (s) | 0.63 (0.63–0.64) | 1.74 (1.04–1.74) | 0.13 (0.13–0.14) | – |
| &nbsp;&nbsp;Enter → PAM session opened (guest, s) | 0.10 (0.09–0.10) | 0.03 (0.03–0.03) | 0.10 (0.09–0.10) | – |

### Memory (4096 MiB guest; MiB, median and range)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| At the login screen: used (MemTotal − MemAvailable) | 531 (516–543) | 691 (688–705) | 1022 (1018–1052) | 375 (366–377) |
| &nbsp;&nbsp;over the no-desktop VM | +156 | +315 | +646 | +0 |
| &nbsp;&nbsp;swap in use | 1 (0–2) | 4 (4–4) | 2 (2–2) | 2 (1–2) |
| Desktop, 60 s after login: used (MemTotal − MemAvailable) | 602 (588–602) | 961 (960–968) | 1249 (1237–1251) | 374 (358–383) |
| &nbsp;&nbsp;over the no-desktop VM | +227 | +587 | +875 | +0 |
| &nbsp;&nbsp;swap in use | 2 (0–2) | 5 (5–6) | 4 (4–5) | 2 (1–2) |
| Desktop, 190 s after login: used (MemTotal − MemAvailable) | 591 (580–594) | 937 (937–948) | 1245 (1241–1245) | 370 (357–370) |
| &nbsp;&nbsp;over the no-desktop VM | +221 | +567 | +875 | +0 |
| &nbsp;&nbsp;swap in use | 2 (0–2) | 5 (5–6) | 4 (4–5) | 2 (1–2) |
| PSS at 190 s: system services | 85 (85–87) | 90 (90–90) | 80 (80–95) | 67 (67–68) |
| PSS at 190 s: login screen (still running) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) |
| PSS at 190 s: the user's session | 276 (276–276) | 698 (696–698) | 1115 (1115–1122) | 0 (0–0) |

### Idle cost over 120 s at the desktop (4096 MiB)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Guest CPU busy (% of one vCPU) | 0.17 (0.17–0.25) | 0.26 (0.19–0.27) | 0.19 (0.17–0.78) | 0.17 (0.08–0.19) |
| Context switches / s | 182 (181–188) | 82 (81–83) | 118 (115–184) | 106 (106–106) |
| Thread wakeups / s (voluntary switches) | 100 (100–101) | 43 (42–43) | 60 (59–80) | 56 (56–56) |
| Interrupts / s | 186 (185–194) | 102 (100–104) | 141 (137–178) | 189 (187–189) |
| Host: QEMU CPU (% of one core) | 0.89 (0.85–0.89) | 0.80 (0.79–0.88) | 0.92 (0.83–1.42) | 0.83 (0.83–0.88) |
| Host: PPT over host idle (W) | 2.37 (1.27–2.53) | 2.49 (0.66–5.97) | 1.71 (-0.58–1.79) | 3.42 (2.47–3.50) |

### Input to screen and frame pacing (4096 MiB)

| | Aizome | GNOME | Plasma |
|---|---|---|---|
| Refresh the compositor reports (ms) | 13.33 | 13.33 | 13.33 |
| Key → presented, idle: p50 / p95 / p99 / max (ms) | 6.5 / 13.0 / 13.6 / 13.7 | 10.5 / 16.4 / 17.0 / 24.0 | 9.6 / 23.2 / 26.0 / 29.4 |
| &nbsp;&nbsp;in frames: p50 / p99 | 0.49 / 1.02 | 0.78 / 1.28 | 0.72 / 1.95 |
| &nbsp;&nbsp;presses (boots) | 660 (3) | 660 (3) | 660 (3) |
| Continuous frames, idle: fps | 75.0 (75.0–75.0) | 74.9 (74.9–75.0) | 75.0 (75.0–75.0) |
| &nbsp;&nbsp;interval p99 / max (ms) | 13.3 / 13.3 | 13.3 / 26.7 | 13.3 / 13.3 |
| &nbsp;&nbsp;refreshes missed (%) | 0.00 (0.00–0.00) | 0.09 (0.04–0.13) | 0.00 (0.00–0.00) |
| Key → presented, load: p50 / p95 / p99 / max (ms) | 7.0 / 13.0 / 13.5 / 15.1 | 13.0 / 20.0 / 23.6 / 55.4 | 13.5 / 24.2 / 26.2 / 36.2 |
| &nbsp;&nbsp;in frames: p50 / p99 | 0.53 / 1.02 | 0.97 / 1.77 | 1.01 / 1.97 |
| &nbsp;&nbsp;presses (boots) | 660 (3) | 660 (3) | 660 (3) |
| Continuous frames, load: fps | 75.0 (75.0–75.0) | 74.4 (74.2–74.5) | 75.0 (74.9–75.0) |
| &nbsp;&nbsp;interval p99 / max (ms) | 13.3 / 13.3 | 13.3 / 40.0 | 13.3 / 13.3 |
| &nbsp;&nbsp;refreshes missed (%) | 0.00 (0.00–0.00) | 0.84 (0.62–1.02) | 0.00 (0.00–0.09) |

### App start-up (4096 MiB): launch → window settled (s); first change on screen (s); PSS (MiB)

| Desktop | App | Settled, cold | Settled, warm | First change, cold | First change, warm | PSS cold | PSS warm | Other new processes, cold (MiB) |
|---|---|---|---|---|---|---|---|---|
| Aizome | Hotaru (terminal) | 0.12 (0.11–0.17) | 0.17 (0.11–0.22) | 0.11 (0.11–0.12) | 0.12 (0.11–0.13) | 57 (57–57) | 57 (57–57) | 0 (0–0) |
| Aizome | Hako (files) | 0.12 (0.12–0.16) | 0.12 (0.11–0.12) | 0.12 (0.11–0.12) | 0.12 (0.11–0.12) | 21 (20–21) | 23 (22–23) | 0 (0–0) |
| Aizome | Settei (settings) | 0.11 (0.11–0.12) | 0.12 (0.11–0.17) | 0.11 (0.11–0.12) | 0.12 (0.11–0.12) | 25 (25–26) | 26 (23–26) | 0 (0–0) |
| Aizome | Sumi (editor) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 16 (16–16) | 16 (16–17) | 0 (0–0) |
| Aizome | Soroban (calculator) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 0.11 (0.11–0.12) | 15 (15–15) | 15 (14–15) | 0 (0–0) |
| Aizome | Shashin (image) | 0.32 (0.32–0.33) | 0.32 (0.28–0.34) | 0.12 (0.11–0.12) | 0.12 (0.11–0.13) | 323 (323–326) | 326 (324–327) | 0 (0–0) |
| Aizome | Shiori (pdf) | 0.17 (0.12–0.17) | 0.12 (0.11–0.17) | 0.12 (0.11–0.12) | 0.12 (0.11–0.12) | 38 (38–39) | 38 (37–39) | 0 (0–0) |
| GNOME | Console (terminal) | 0.57 (0.56–0.57) | 0.47 (0.46–0.49) | 0.57 (0.56–0.57) | 0.47 (0.46–0.49) | 143 (143–144) | 143 (142–143) | 0 (0–0) |
| GNOME | Files (Nautilus) (files) | 0.67 (0.67–0.68) | 0.57 (0.52–0.58) | 0.52 (0.52–0.52) | 0.47 (0.47–0.52) | 256 (256–257) | 253 (228–255) | 8 (8–8) |
| GNOME | Settings (settings) | 0.57 (0.57–0.62) | 0.52 (0.50–0.53) | 0.57 (0.57–0.57) | 0.47 (0.45–0.47) | 144 (141–144) | 143 (140–143) | 0 (0–0) |
| GNOME | Text Editor (editor) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 144 (144–144) | 144 (141–144) | 0 (0–0) |
| GNOME | Calculator (calculator) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 0.47 (0.47–0.47) | 0.47 (0.46–0.48) | 146 (145–147) | 146 (143–147) | 0 (0–0) |
| GNOME | Image Viewer (Loupe) (image) | 3.19 (1.14–3.20) | 3.19 (1.14–3.20) | 0.52 (0.52–0.77) | 0.68 (0.57–0.77) | 440 (439–467) | 436 (433–441) | 0 (0–0) |
| GNOME | Document Viewer (Papers) (pdf) | 0.82 (0.73–0.83) | 0.79 (0.78–0.84) | 0.42 (0.42–0.42) | 0.42 (0.37–0.43) | 255 (233–256) | 252 (248–256) | 0 (0–0) |
| Plasma | Konsole (terminal) | 1.27 (1.17–1.27) | 1.22 (1.17–1.28) | 0.27 (0.27–0.27) | 0.22 (0.21–0.27) | 39 (37–39) | 37 (37–40) | 0 (0–0) |
| Plasma | Dolphin (files) | 0.63 (0.63–0.63) | 0.63 (0.58–0.65) | 0.32 (0.27–0.37) | 0.27 (0.26–0.32) | 96 (94–96) | 93 (91–94) | 0 (0–0) |
| Plasma | System Settings (settings) | 0.87 (0.87–0.87) | 0.63 (0.57–0.68) | 0.77 (0.77–0.77) | 0.53 (0.52–0.58) | 178 (178–178) | 177 (176–178) | 0 (0–0) |
| Plasma | KWrite (editor) | 0.27 (0.26–0.27) | 0.27 (0.26–0.32) | 0.22 (0.22–0.26) | 0.26 (0.21–0.28) | 35 (33–35) | 34 (32–35) | 0 (0–0) |
| Plasma | Gwenview (image) | 0.58 (0.57–0.58) | 0.58 (0.57–0.63) | 0.32 (0.27–0.32) | 0.27 (0.26–0.32) | 166 (159–168) | 166 (157–168) | 13 (13–13) |
| Plasma | Okular (pdf) | 0.53 (0.53–0.54) | 0.48 (0.47–0.53) | 0.37 (0.37–0.37) | 0.27 (0.26–0.32) | 82 (82–83) | 72 (69–72) | 0 (0–0) |


### Boot and login (2048 MiB; median, min–max over trials)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Trials | 2 | 2 | 2 | 2 |
| Boot: QEMU start → login screen settled (s) | 6.26 (6.26–6.26) | 7.41 (7.36–7.46) | 8.11 (7.66–8.56) | 5.81 (5.66–5.96) |
| &nbsp;&nbsp;kernel + initrd + userspace (systemd-analyze, s) | 0.19 + 2.75 + 2.14 | 0.20 + 2.74 + 2.27 | 0.19 + 2.31 + 1.97 | 0.19 + 2.74 + 1.97 |
| &nbsp;&nbsp;display manager started (host timeline, s) | 5.35 (5.33–5.37) | 5.51 (5.45–5.57) | 4.76 (4.33–5.19) | – |
| Login: Enter → desktop settled (s) | 1.23 (1.13–1.33) | 2.04 (2.04–2.04) | 5.79 (5.64–5.94) | – |
| &nbsp;&nbsp;Enter → first change on screen (s) | 0.63 (0.63–0.64) | 1.74 (1.64–1.84) | 0.13 (0.13–0.14) | – |
| &nbsp;&nbsp;Enter → PAM session opened (guest, s) | 0.10 (0.10–0.10) | 0.03 (0.03–0.03) | 0.08 (0.08–0.09) | – |

### Memory (2048 MiB guest; MiB, median and range)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| At the login screen: used (MemTotal − MemAvailable) | 426 (422–430) | 617 (616–618) | 946 (941–951) | 292 (292–292) |
| &nbsp;&nbsp;over the no-desktop VM | +134 | +325 | +654 | +0 |
| &nbsp;&nbsp;swap in use | 2 (1–2) | 4 (4–5) | 2 (2–2) | 1 (1–1) |
| Desktop, 60 s after login: used (MemTotal − MemAvailable) | 501 (496–506) | 862 (860–863) | 1135 (1129–1142) | 289 (288–290) |
| &nbsp;&nbsp;over the no-desktop VM | +212 | +572 | +846 | +0 |
| &nbsp;&nbsp;swap in use | 3 (2–4) | 5 (4–6) | 25 (25–26) | 1 (1–1) |
| Desktop, 190 s after login: used (MemTotal − MemAvailable) | 499 (495–502) | 853 (849–857) | 1135 (1134–1137) | 293 (288–297) |
| &nbsp;&nbsp;over the no-desktop VM | +206 | +560 | +842 | +0 |
| &nbsp;&nbsp;swap in use | 3 (2–4) | 5 (4–6) | 25 (25–26) | 1 (1–1) |
| PSS at 190 s: system services | 85 (85–86) | 90 (89–91) | 67 (66–68) | 68 (68–68) |
| PSS at 190 s: login screen (still running) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) |
| PSS at 190 s: the user's session | 278 (277–280) | 695 (691–699) | 1093 (1093–1093) | 0 (0–0) |

### Idle cost over 120 s at the desktop (2048 MiB)

| | Aizome | GNOME | Plasma | No desktop |
|---|---|---|---|---|
| Guest CPU busy (% of one vCPU) | 0.27 (0.22–0.31) | 0.24 (0.21–0.28) | 0.24 (0.20–0.29) | 0.09 (0.09–0.09) |
| Context switches / s | 182 (178–187) | 82 (81–82) | 116 (115–117) | 106 (106–106) |
| Thread wakeups / s (voluntary switches) | 100 (98–101) | 42 (42–42) | 59 (59–60) | 56 (56–56) |
| Interrupts / s | 187 (183–192) | 102 (102–102) | 138 (137–140) | 188 (187–189) |
| Host: QEMU CPU (% of one core) | 0.96 (0.95–0.97) | 0.81 (0.78–0.85) | 0.92 (0.92–0.92) | 0.91 (0.90–0.92) |
| Host: PPT over host idle (W) | -0.55 (-2.05–0.94) | -0.66 (-0.81–-0.51) | 3.30 (3.07–3.54) | 2.75 (1.81–3.70) |

