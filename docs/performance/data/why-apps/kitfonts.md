/tmp/claude-1000/perf/why-apps/runs/kitfonts: 2026-11-02T21:19:40-0600, load before ['0.54', '0.59', '0.62'], after ['0.11', '0.36', '0.53']

| Variant | n | First frame, ms (range) | Δ vs base, ms | CPU, all threads, ms | Main thread CPU, ms | Main thread not running, ms | Shared objects |
|---|---:|---|---:|---:|---:|---:|---:|
| kit | 8 | 19.2 (18.9–20.0) |  | 19.1 | 18.2 | 0.9 | 13.0 |
| kit.no-font-dir | 8 | 20.6 (20.5–21.1) | +1.4 | 20.6 | 19.7 | 1.1 | 13.0 |
| kit.dejavu-only | 8 | 6.9 (6.6–7.2) | -12.3 | 6.5 | 5.6 | 1.3 | 13.0 |
| kit.dejavu-only.no-font-dir | 8 | 6.1 (5.9–6.4) | -13.1 | 5.6 | 4.8 | 1.3 | 13.0 |
| sumi | 8 | 18.9 (18.9–20.5) |  | 18.6 | 17.9 | 1.1 | 13.0 |
| sumi.no-font-dir | 8 | 20.8 (20.2–21.5) | +1.8 | 20.3 | 19.6 | 1.2 | 13.0 |
| sumi.dejavu-only | 8 | 6.7 (6.3–6.7) | -12.3 | 6.3 | 5.6 | 1.1 | 13.0 |
| sumi.dejavu-only.no-font-dir | 8 | 5.9 (5.6–6.1) | -13.0 | 5.6 | 4.9 | 1.0 | 13.0 |

Memory 3 s after the first frame (smaps_rollup, medians):

| Variant | n | PSS, MiB | Private (Pss_Anon), MiB | Pss_File, MiB | RSS, MiB |
|---|---:|---:|---:|---:|---:|
| kit | 8 | 12.47 | 4.66 | 7.62 | 18.98 |
| kit.no-font-dir | 8 | 10.72 | 2.79 | 7.75 | 17.81 |
| kit.dejavu-only | 8 | 11.56 | 4.14 | 7.23 | 15.36 |
| kit.dejavu-only.no-font-dir | 8 | 9.02 | 1.57 | 7.26 | 12.99 |
| sumi | 8 | 16.22 | 5.59 | 9.95 | 23.42 |
| sumi.no-font-dir | 8 | 14.32 | 3.06 | 10.58 | 22.17 |
| sumi.dejavu-only | 8 | 15.59 | 5.03 | 9.88 | 20.05 |
| sumi.dejavu-only.no-font-dir | 8 | 13.21 | 2.46 | 10.06 | 18.29 |
