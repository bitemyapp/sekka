### Startup (warm caches)

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| Launch → PTY ready, ms | – | – | – |
| Launch → first frame, ms | – | – | – |
| trials | – | – | – |
| first frame, all matched launches | 49.6 (n=6) | 186.0 (n=6) | 193.9 (n=6) |

### Key to frame

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| presses | – | – | – |
| p50, ms | – | – | – |
| p95, ms | – | – | – |
| p99, ms | – | – | – |
| max, ms | – | – | – |
| p50, frames (60 Hz) | – | – | – |
| p99, frames | – | – | – |
| key → PTY read, p50 ms | – | – | – |
| PTY write → frame, p50 ms | – | – | – |

### Paced updates (120 per stream)

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| marker 30 Hz: updates seen | – | – | – |
| marker 30 Hz: terminal CPU, ms | – | – | – |
| marker 30 Hz: sway CPU, ms | – | – | – |
| marker 60 Hz: updates seen | – | – | – |
| marker 60 Hz: terminal CPU, ms | – | – | – |
| marker 60 Hz: sway CPU, ms | – | – | – |
| sparse 30 Hz: updates seen | – | – | – |
| sparse 30 Hz: terminal CPU, ms | – | – | – |
| sparse 30 Hz: sway CPU, ms | – | – | – |
| sparse 60 Hz: updates seen | – | – | – |
| sparse 60 Hz: terminal CPU, ms | – | – | – |
| sparse 60 Hz: sway CPU, ms | – | – | – |
| scroll 30 Hz: updates seen | – | – | – |
| scroll 30 Hz: terminal CPU, ms | – | – | – |
| scroll 30 Hz: sway CPU, ms | – | – | – |
| scroll 60 Hz: updates seen | – | – | – |
| scroll 60 Hz: terminal CPU, ms | – | – | – |
| scroll 60 Hz: sway CPU, ms | – | – | – |

### Throughput (64 MiB per corpus)

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| ascii: ms | 91 (84–100) | 89 (78–90) | 81 (74–85) |
| ascii: MiB/s | 705 | 720 | 794 |
| ascii: to final frame, ms | 123 | 117 | 104 |
| ascii: terminal CPU, ms | 151 | 162 | 151 |
| ascii: sway CPU, ms | 10 | 2 | 2 |
| ascii: GPU (terminal + sway), ms | 7 | 69 | 63 |
| ascii: energy, J | 5.3 (4.7–6.4) | 6.2 (5.3–6.8) | 5.2 (5.0–5.5) |
| ascii: over idle baseline, J | 2.8 (2.3–3.1) | 3.4 (2.5–4.2) | 2.9 (2.4–3.3) |
| styled: ms | 213 (207–231) | 236 (227–249) | 185 (170–209) |
| styled: MiB/s | 301 | 271 | 346 |
| styled: to final frame, ms | 249 | 263 | 207 |
| styled: terminal CPU, ms | 286 | 338 | 278 |
| styled: sway CPU, ms | 6 | 2 | 2 |
| styled: GPU (terminal + sway), ms | 5 | 179 | 138 |
| styled: energy, J | 10.0 (9.4–12.1) | 13.0 (11.4–15.2) | 10.1 (8.6–10.9) |
| styled: over idle baseline, J | 4.8 (3.8–5.7) | 5.6 (4.1–7.3) | 5.6 (4.8–6.0) |
| unicode: ms | 389 (380–398) | 893 (854–939) | 255 (252–265) |
| unicode: MiB/s | 165 | 72 | 251 |
| unicode: to final frame, ms | 414 | 920 | 280 |
| unicode: terminal CPU, ms | 479 | 1,118 | 371 |
| unicode: sway CPU, ms | 8 | 3 | 2 |
| unicode: GPU (terminal + sway), ms | 6 | 501 | 184 |
| unicode: energy, J | 16.2 (15.3–22.2) | 41.7 (35.1–49.4) | 12.8 (12.0–14.0) |
| unicode: over idle baseline, J | 7.4 (6.6–8.6) | 16.8 (13.8–26.4) | 7.0 (5.7–8.6) |
| cursor: ms | 507 (505–520) | 2,886 (2,873–2,980) | 353 (344–362) |
| cursor: MiB/s | 126 | 22 | 181 |
| cursor: to final frame, ms | 541 | 2,923 | 374 |
| cursor: terminal CPU, ms | 597 | 3,118 | 468 |
| cursor: sway CPU, ms | 10 | 8 | 2 |
| cursor: GPU (terminal + sway), ms | 6 | 1,145 | 243 |
| cursor: energy, J | 21.9 (20.1–25.1) | 103.0 (90.3–121.2) | 16.8 (16.5–17.2) |
| cursor: over idle baseline, J | 9.6 (6.5–10.1) | 36.7 (3.3–45.9) | 8.9 (7.6–9.5) |

### Idle, matched configuration (60 s)

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| socket power, W | – | – | – |
| over baseline, W | – | – | – |
| terminal CPU, ms/s | – | – | – |
| terminal context switches/s | – | – | – |
| terminal threads | – | – | – |
| sway CPU, ms/s | – | – | – |
| GPU busy (terminal + sway), ms/s | – | – | – |
| trials | – | – | – |

### Idle, default configuration (60 s)

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| socket power, W | – | – | – |
| over baseline, W | – | – | – |
| terminal CPU, ms/s | – | – | – |
| terminal context switches/s | – | – | – |
| terminal threads | – | – | – |
| sway CPU, ms/s | – | – | – |
| GPU busy (terminal + sway), ms/s | – | – | – |
| trials | – | – | – |

### Typing, 10 keys/s for 60 s

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| socket power, W | – | – | – |
| over baseline, W | – | – | – |
| terminal CPU, ms/s | – | – | – |
| terminal context switches/s | – | – | – |
| terminal threads | – | – | – |
| sway CPU, ms/s | – | – | – |
| GPU busy (terminal + sway), ms/s | – | – | – |
| trials | – | – | – |

### Memory

| | Hotaru | Ghostty | ghostty-main |
|---|---:|---:|---:|
| PSS after startup, MiB | – | – | – |
| RSS after startup, MiB | – | – | – |
| USS after startup, MiB | – | – | – |
| GPU memory (DRM, terminal), MiB | – | – | – |
| sway's GPU memory meanwhile, MiB | – | – | – |
| PSS after throughput, MiB | 94.5 (94.4–94.6) | 109.2 (108.9–109.5) | 120.4 (119.7–120.7) |
| RSS after throughput, MiB | 101.1 (101.0–101.3) | 174.2 (174.0–174.4) | 174.9 (174.2–175.2) |
| PSS idle, default config, MiB | – | – | – |

