### Startup (warm caches)

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| Launch → PTY ready, ms | – | – | – | – | – | – | – |
| Launch → first frame, ms | – | – | – | – | – | – | – |
| trials | – | – | – | – | – | – | – |
| first frame, all matched launches | 49.6 (n=6) | 186.0 (n=6) | 193.9 (n=6) | 129.9 (n=6) | 130.2 (n=6) | 130.2 (n=6) | 65.8 (n=6) |

### Key to frame

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| presses | – | – | – | – | – | – | – |
| p50, ms | – | – | – | – | – | – | – |
| p95, ms | – | – | – | – | – | – | – |
| p99, ms | – | – | – | – | – | – | – |
| max, ms | – | – | – | – | – | – | – |
| p50, frames (60 Hz) | – | – | – | – | – | – | – |
| p99, frames | – | – | – | – | – | – | – |
| key → PTY read, p50 ms | – | – | – | – | – | – | – |
| PTY write → frame, p50 ms | – | – | – | – | – | – | – |

### Paced updates (120 per stream)

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| marker 30 Hz: updates seen | – | – | – | – | – | – | – |
| marker 30 Hz: terminal CPU, ms | – | – | – | – | – | – | – |
| marker 30 Hz: sway CPU, ms | – | – | – | – | – | – | – |
| marker 60 Hz: updates seen | – | – | – | – | – | – | – |
| marker 60 Hz: terminal CPU, ms | – | – | – | – | – | – | – |
| marker 60 Hz: sway CPU, ms | – | – | – | – | – | – | – |
| sparse 30 Hz: updates seen | – | – | – | – | – | – | – |
| sparse 30 Hz: terminal CPU, ms | – | – | – | – | – | – | – |
| sparse 30 Hz: sway CPU, ms | – | – | – | – | – | – | – |
| sparse 60 Hz: updates seen | – | – | – | – | – | – | – |
| sparse 60 Hz: terminal CPU, ms | – | – | – | – | – | – | – |
| sparse 60 Hz: sway CPU, ms | – | – | – | – | – | – | – |
| scroll 30 Hz: updates seen | – | – | – | – | – | – | – |
| scroll 30 Hz: terminal CPU, ms | – | – | – | – | – | – | – |
| scroll 30 Hz: sway CPU, ms | – | – | – | – | – | – | – |
| scroll 60 Hz: updates seen | – | – | – | – | – | – | – |
| scroll 60 Hz: terminal CPU, ms | – | – | – | – | – | – | – |
| scroll 60 Hz: sway CPU, ms | – | – | – | – | – | – | – |

### Throughput (64 MiB per corpus)

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| ascii: ms | 91 (84–100) | 89 (78–90) | 81 (74–85) | 279 (249–292) | 19,496 (17,181–20,749) | 276 (257–298) | 439 (435–461) |
| ascii: MiB/s | 705 | 720 | 794 | 230 | 3 | 232 | 146 |
| ascii: to final frame, ms | 123 | 117 | 104 | 323 | 19,546 | 325 | 461 |
| ascii: terminal CPU, ms | 151 | 162 | 151 | 281 | 19,495 | 291 | 455 |
| ascii: sway CPU, ms | 10 | 2 | 2 | 10 | 20 | 3 | 13 |
| ascii: GPU (terminal + sway), ms | 7 | 69 | 63 | 10 | 7 | 24 | 7 |
| ascii: energy, J | 5.3 (4.7–6.4) | 6.2 (5.3–6.8) | 5.2 (5.0–5.5) | 11.0 (10.4–12.1) | 778.6 (741.3–852.8) | 11.2 (10.6–14.3) | 15.6 (14.7–18.0) |
| ascii: over idle baseline, J | 2.8 (2.3–3.1) | 3.4 (2.5–4.2) | 2.9 (2.4–3.3) | 4.3 (4.0–4.9) | 375.9 (237.8–437.4) | 4.2 (3.2–5.9) | 5.4 (2.9–8.2) |
| styled: ms | 213 (207–231) | 236 (227–249) | 185 (170–209) | 411 (391–460) | 94,955 (86,258–102,992) | 393 (369–424) | 191 (184–195) |
| styled: MiB/s | 301 | 271 | 346 | 156 | 1 | 163 | 334 |
| styled: to final frame, ms | 249 | 263 | 207 | 468 | 95,003 | 467 | 212 |
| styled: terminal CPU, ms | 286 | 338 | 278 | 431 | 94,829 | 432 | 196 |
| styled: sway CPU, ms | 6 | 2 | 2 | 6 | 82 | 3 | 6 |
| styled: GPU (terminal + sway), ms | 5 | 179 | 138 | 7 | 9 | 17 | 4 |
| styled: energy, J | 10.0 (9.4–12.1) | 13.0 (11.4–15.2) | 10.1 (8.6–10.9) | 16.5 (15.3–18.0) | 3,958.3 (3,632.8–4,388.7) | 16.3 (15.4–19.2) | 8.3 (7.6–9.5) |
| styled: over idle baseline, J | 4.8 (3.8–5.7) | 5.6 (4.1–7.3) | 5.6 (4.8–6.0) | 6.2 (5.0–7.4) | 1,999.9 (1,674.9–2,487.8) | 6.2 (4.7–6.7) | 3.7 (2.0–4.6) |
| unicode: ms | 389 (380–398) | 893 (854–939) | 255 (252–265) | 728 (720–800) | 115,315 (101,460–121,776) | 725 (711–796) | 423 (406–429) |
| unicode: MiB/s | 165 | 72 | 251 | 88 | 1 | 88 | 151 |
| unicode: to final frame, ms | 414 | 920 | 280 | 816 | 115,374 | 831 | 447 |
| unicode: terminal CPU, ms | 479 | 1,118 | 371 | 762 | 115,155 | 772 | 436 |
| unicode: sway CPU, ms | 8 | 3 | 2 | 7 | 123 | 5 | 9 |
| unicode: GPU (terminal + sway), ms | 6 | 501 | 184 | 15 | 10 | 32 | 6 |
| unicode: energy, J | 16.2 (15.3–22.2) | 41.7 (35.1–49.4) | 12.8 (12.0–14.0) | 27.4 (25.3–31.2) | 4,952.9 (4,458.0–6,247.8) | 27.7 (25.6–34.0) | 16.4 (15.1–20.4) |
| unicode: over idle baseline, J | 7.4 (6.6–8.6) | 16.8 (13.8–26.4) | 7.0 (5.7–8.6) | 11.1 (7.2–13.3) | 2,111.6 (1,616.2–2,352.8) | 10.0 (5.8–15.5) | 6.2 (4.8–8.3) |
| cursor: ms | 507 (505–520) | 2,886 (2,873–2,980) | 353 (344–362) | 2,538 (2,458–2,614) | 708 (702–728) | 2,620 (2,587–2,647) | 618 (603–634) |
| cursor: MiB/s | 126 | 22 | 181 | 25 | 90 | 24 | 104 |
| cursor: to final frame, ms | 541 | 2,923 | 374 | 2,627 | 728 | 2,683 | 638 |
| cursor: terminal CPU, ms | 597 | 3,118 | 468 | 2,457 | 704 | 2,450 | 631 |
| cursor: sway CPU, ms | 10 | 8 | 2 | 17 | 3 | 8 | 11 |
| cursor: GPU (terminal + sway), ms | 6 | 1,145 | 243 | 14 | 6 | 59 | 6 |
| cursor: energy, J | 21.9 (20.1–25.1) | 103.0 (90.3–121.2) | 16.8 (16.5–17.2) | 87.7 (77.9–112.6) | 27.1 (24.6–30.0) | 87.9 (83.7–102.1) | 23.1 (22.4–30.8) |
| cursor: over idle baseline, J | 9.6 (6.5–10.1) | 36.7 (3.3–45.9) | 8.9 (7.6–9.5) | 30.7 (26.3–44.4) | 10.3 (5.9–12.9) | 27.0 (20.9–35.7) | 8.9 (7.3–12.5) |

### Idle, matched configuration (60 s)

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| socket power, W | – | – | – | – | – | – | – |
| over baseline, W | – | – | – | – | – | – | – |
| terminal CPU, ms/s | – | – | – | – | – | – | – |
| terminal context switches/s | – | – | – | – | – | – | – |
| terminal threads | – | – | – | – | – | – | – |
| sway CPU, ms/s | – | – | – | – | – | – | – |
| GPU busy (terminal + sway), ms/s | – | – | – | – | – | – | – |
| trials | – | – | – | – | – | – | – |

### Idle, default configuration (60 s)

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| socket power, W | – | – | – | – | – | – | – |
| over baseline, W | – | – | – | – | – | – | – |
| terminal CPU, ms/s | – | – | – | – | – | – | – |
| terminal context switches/s | – | – | – | – | – | – | – |
| terminal threads | – | – | – | – | – | – | – |
| sway CPU, ms/s | – | – | – | – | – | – | – |
| GPU busy (terminal + sway), ms/s | – | – | – | – | – | – | – |
| trials | – | – | – | – | – | – | – |

### Typing, 10 keys/s for 60 s

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| socket power, W | – | – | – | – | – | – | – |
| over baseline, W | – | – | – | – | – | – | – |
| terminal CPU, ms/s | – | – | – | – | – | – | – |
| terminal context switches/s | – | – | – | – | – | – | – |
| terminal threads | – | – | – | – | – | – | – |
| sway CPU, ms/s | – | – | – | – | – | – | – |
| GPU busy (terminal + sway), ms/s | – | – | – | – | – | – | – |
| trials | – | – | – | – | – | – | – |

### Memory

| | Hotaru | Ghostty | ghostty-main | gnome-terminal | Konsole | Console (kgx) | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| PSS after startup, MiB | – | – | – | – | – | – | – |
| RSS after startup, MiB | – | – | – | – | – | – | – |
| USS after startup, MiB | – | – | – | – | – | – | – |
| GPU memory (DRM, terminal), MiB | – | – | – | – | – | – | – |
| sway's GPU memory meanwhile, MiB | – | – | – | – | – | – | – |
| PSS after throughput, MiB | 94.5 (94.4–94.6) | 109.2 (108.9–109.5) | 120.4 (119.7–120.7) | 38.2 (27.2–42.5) | 942.5 (942.3–942.5) | 69.4 (56.7–76.2) | 1,522.9 (1,522.7–1,523.1) |
| RSS after throughput, MiB | 101.1 (101.0–101.3) | 174.2 (174.0–174.4) | 174.9 (174.2–175.2) | 68.4 (57.6–73.0) | 979.9 (979.6–980.1) | 120.3 (107.7–127.1) | 1,533.1 (1,532.8–1,533.3) |
| PSS idle, default config, MiB | – | – | – | – | – | – | – |

