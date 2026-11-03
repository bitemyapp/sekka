memablate: 2026-11-02T21:12:54-0600, load before ['0.37', '0.58', '0.63'], after ['0.37', '0.57', '0.62']; medians over runs, 3 s after the first frame (smaps_rollup summed over the program's processes)

| Variant | n | PSS, MiB | Private (Pss_Anon), MiB | Δ private vs base | Pss_File, MiB | Pss_Shmem, MiB | RSS, MiB |
|---|---:|---:|---:|---:|---:|---:|---:|
| kit | 3 | 12.6 | 4.67 |  | 7.7 | 0.18 | 19.1 |
| kit.dejavu-only | 3 | 11.6 | 4.14 | -0.53 | 7.2 | 0.18 | 15.3 |
| gtk4 | 3 | 37.1 | 16.79 |  | 20.3 | 0.01 | 90.5 |
| gtk4.lean | 3 | 18.0 | 7.35 | -9.44 | 10.5 | 0.19 | 42.9 |
| gtk4.cairo | 3 | 18.4 | 7.43 | -9.36 | 10.8 | 0.19 | 43.9 |
| gtk4.radv-only | 3 | 29.4 | 12.12 | -4.67 | 17.3 | 0.01 | 78.3 |
| gtk4.dejavu-only | 3 | 36.3 | 16.34 | -0.45 | 19.9 | 0.01 | 86.9 |
| adw | 3 | 42.0 | 19.30 |  | 22.7 | 0.01 | 99.6 |
| adw.lean | 3 | 23.4 | 9.89 | -9.41 | 13.3 | 0.26 | 52.1 |
| adw.cairo | 3 | 23.1 | 9.64 | -9.66 | 13.2 | 0.26 | 52.7 |
| adw.radv-only | 3 | 34.0 | 14.29 | -5.01 | 19.7 | 0.01 | 86.9 |
| adw.dejavu-only | 3 | 41.3 | 18.86 | -0.43 | 22.5 | 0.01 | 95.7 |
| qtwidgets | 3 | 34.9 | 11.59 |  | 22.2 | 1.03 | 65.8 |
| qtwidgets.lean | 3 | 23.6 | 6.88 | -4.71 | 16.2 | 0.39 | 42.8 |
| qtwidgets.no-gtk3-theme | 3 | 24.0 | 6.91 | -4.68 | 16.6 | 0.39 | 43.2 |
| qtwidgets.dejavu-only | 3 | 30.2 | 7.43 | -4.16 | 21.8 | 1.03 | 57.7 |
| qtquick | 3 | 86.8 | 36.85 |  | 49.7 | 0.26 | 162.3 |
| qtquick.lean | 3 | 43.4 | 12.04 | -24.81 | 31.0 | 0.39 | 68.6 |
| qtquick.software | 3 | 52.1 | 16.27 | -20.58 | 35.2 | 0.64 | 87.2 |
| qtquick.no-gtk3-theme | 3 | 79.2 | 33.63 | -3.22 | 45.5 | 0.01 | 144.9 |
| qtquick.dejavu-only | 3 | 83.2 | 33.58 | -3.27 | 49.3 | 0.26 | 155.4 |
| sumi | 3 | 16.4 | 5.59 |  | 10.1 | 0.69 | 23.7 |
| sumi.dejavu-only | 3 | 15.3 | 5.04 | -0.55 | 9.5 | 0.69 | 19.7 |
| gnome-text-editor | 3 | 59.2 | 26.46 |  | 32.8 | 0.01 | 119.9 |
| gnome-text-editor.lean | 3 | 41.6 | 17.71 | -8.76 | 23.1 | 0.82 | 73.4 |
| gnome-text-editor.cairo | 3 | 43.0 | 19.18 | -7.29 | 23.0 | 0.82 | 76.3 |
| gnome-text-editor.radv-only | 3 | 53.3 | 23.65 | -2.82 | 29.7 | 0.01 | 109.4 |
| gnome-text-editor.dejavu-only | 3 | 57.6 | 25.25 | -1.21 | 32.4 | 0.01 | 114.4 |
| kwrite | 3 | 73.5 | 21.32 |  | 50.1 | 2.01 | 113.3 |
| kwrite.lean | 3 | 62.9 | 16.72 | -4.61 | 44.3 | 1.81 | 92.7 |
| kwrite.no-gtk3-theme | 3 | 63.2 | 16.76 | -4.57 | 44.5 | 1.81 | 93.1 |
| kwrite.dejavu-only | 3 | 70.7 | 18.82 | -2.50 | 49.9 | 2.01 | 107.3 |
