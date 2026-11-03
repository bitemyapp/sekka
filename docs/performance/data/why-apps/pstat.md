perf stat from launch to the first frame (8 runs each; programs using libwayland exit after their first commit, the kit's are stopped at sway's window event):

| Variant | n | task-clock, ms | user, ms | system, ms | page faults | context switches | user instructions, M | IPC (user) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| kit | 8 | 19.3 | 16.6 | 3.2 | 1220 | 0 | 316 | 3.84 |
| gtk4 | 8 | 98.1 | 61.6 | 37.6 | 18130 | 0 | 790 | 2.39 |
| gtk4.lean | 8 | 31.2 | 24.3 | 7.0 | 2974 | 0 | 337 | 2.58 |
| gtk4.cairo | 8 | 33.1 | 25.8 | 7.3 | 3010 | 0 | 338 | 2.47 |
| gtk4.radv-only | 8 | 49.1 | 37.6 | 13.5 | 4680 | 0 | 451 | 2.35 |
| adw | 8 | 120.4 | 82.9 | 39.9 | 17316 | 0 | 1100 | 2.48 |
| adw.lean | 8 | 56.1 | 48.7 | 8.4 | 3870 | 0 | 676 | 2.69 |
| adw.cairo | 8 | 53.9 | 45.5 | 9.3 | 3818 | 0 | 627 | 2.62 |
| adw.radv-only | 8 | 71.4 | 58.7 | 15.7 | 5440 | 0 | 745 | 2.48 |
| qtwidgets | 8 | 45.4 | 36.8 | 9.8 | 4488 | 0 | 508 | 2.64 |
| qtwidgets.lean | 8 | 19.8 | 15.5 | 4.9 | 2774 | 0 | 237 | 3.05 |
| qtwidgets.no-gtk3-theme | 8 | 20.5 | 15.1 | 6.2 | 2800 | 0 | 240 | 2.99 |
| qtquick | 8 | 92.0 | 68.5 | 26.7 | 12908 | 0 | 917 | 2.49 |
| qtquick.lean | 8 | 36.8 | 28.1 | 9.2 | 4592 | 0 | 423 | 2.79 |
| qtquick.software | 8 | 61.6 | 51.5 | 12.1 | 6110 | 0 | 690 | 2.62 |
| qtquick.no-gtk3-theme | 8 | 68.6 | 49.8 | 19.2 | 11472 | 0 | 661 | 2.52 |
| sumi | 8 | 18.7 | 15.9 | 3.0 | 1173 | 0 | 308 | 3.90 |
| gnome-text-editor | 8 | 162.2 | 123.0 | 42.2 | 19322 | 0 | 1571 | 2.47 |
| gnome-text-editor.lean | 8 | 90.5 | 79.1 | 12.8 | 5579 | 0 | 1082 | 2.62 |
| gnome-text-editor.cairo | 8 | 93.6 | 81.5 | 13.1 | 5978 | 0 | 1084 | 2.56 |
| gnome-text-editor.radv-only | 8 | 111.1 | 94.9 | 19.2 | 7326 | 0 | 1199 | 2.47 |
| kwrite | 8 | 102.4 | 83.6 | 19.6 | 7882 | 0 | 1106 | 2.51 |
| kwrite.lean | 8 | 71.1 | 58.7 | 13.0 | 6155 | 0 | 786 | 2.63 |
| kwrite.no-gtk3-theme | 8 | 73.0 | 56.5 | 16.4 | 6176 | 0 | 791 | 2.57 |
