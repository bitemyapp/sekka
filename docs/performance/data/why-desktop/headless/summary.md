### Compositor and shell memory by mapping, headless on the host (MiB PSS; median of clean runs)

| Mapping | Aizome compositor | Aizome shell | GNOME compositor+shell | Plasma compositor | Plasma shell |
|---|---:|---:|---:|---:|---:|
| other anonymous mmap (large malloc blocks, engines' own heaps, driver buffers) | 8.1 | 1.2 | 128.4 | 23.0 | 72.4 |
| malloc: main heap (brk) | 2.5 | 3.9 | 82.4 | 55.0 | 97.2 |
| libraries: code | 13.6 | 0.2 | 54.7 | 39.1 | 52.6 |
| malloc: thread arenas | 0.0 | 27.8 | 10.8 | 7.1 | 30.9 |
| libraries: read-only data | 8.3 | 0.3 | 23.6 | 17.1 | 29.6 |
| executable | 0.0 | 18.2 | 0.0 | 1.8 | 1.1 |
| libraries: relocated data (private) | 1.7 | 0.5 | 8.9 | 11.2 | 14.0 |
| JS GC heap (Qt's QML engine, named memfd) | 0.0 | 0.0 | 0.0 | 0.6 | 13.5 |
| llvmpipe display buffers (memfd shared with the compositor) | 0.0 | 0.0 | 0.0 | 10.6 | 10.6 |
| JS GC heap (1 MiB-aligned chunks) | 0.0 | 0.0 | 8.7 | 0.0 | 0.0 |
| caches, schemas, locale, resources | 0.0 | 0.0 | 1.3 | 4.7 | 5.0 |
| other files | 0.3 | 0.3 | 4.2 | 0.3 | 0.4 |
| shared memory (window buffers, wl_shm pools) | 0.5 | 1.5 | 0.0 | 0.0 | 1.3 |
| fonts (files mapped) | 0.3 | 0.6 | 0.8 | 0.0 | 0.2 |
| **PSS total** | **35.6** | **54.7** | **324.7** | **171.2** | **329.6** |
| of which anonymous | 12.5 | 34.3 | 239.7 | 97.4 | 229.8 |

### The wallpaper's share (MiB PSS of the compositor and shell; medians)

| | Sekka's wallpaper | With it | Against | Difference | of it anonymous |
|---|---|---:|---:|---:|---:|
| Aizome | Tokyo at dusk, 6000 × 3750 | 90.3 | 65.3 (no picture) | +24.9 | +24.5 |
| GNOME | Foggy forest, 4928 × 3264 | 324.7 | 245.8 (the same picture at 1920 × 1080) | +78.9 | +78.4 |
| Plasma | Tatami ocean wave (look-and-feel) | 500.8 | 507.6 (Plasma's default look and feel and its wallpaper) | -6.8 | -2.8 |

### The whole headless session (MiB PSS; median of clean runs)

| | Compositor + shell | Everything else the session started | Total | Processes |
|---|---:|---:|---:|---:|
| Aizome | 90.3 | 45.5 | 135.8 | 12 |
| GNOME | 324.7 | 170.1 | 494.8 | 22 |
| Plasma | 500.8 | 434.1 | 934.9 | 35 |

### Largest other processes of the headless sessions (MiB PSS, anonymous; one clean run)

- Aizome: Xwayland 17 (10); xdg-desktop-portal-gtk 13 (6); xdg-desktop-portal 6 (3); aizome-portal 4 (1); xwayland-satellite 4 (1); xdg-permission-store 1 (1); dconf-service 1 (1); dbus-daemon 1 (1)
- GNOME: evolution-source-registry 36 (13); ibus-extension-gtk3 18 (14); xdg-desktop-portal-gnome 16 (9); gnome-control-center-global-shortcuts-provider 15 (9); gjs 15 (6); evolution-addressbook-factory 14 (6); evolution-calendar-factory 12 (5); gnome-shell-calendar-server 10 (4)
- Plasma: mariadbd 77 (57); akonadi_mailfilter_agent 28 (18); akonadi_archivemail_agent 28 (18); akonadi_followupreminder_agent 28 (18); akonadi_unifiedmailbox_agent 27 (18); akonadi_sendlater_agent 27 (18); akonadi_mailmerge_agent 27 (18); kded6 25 (13)

### Work, headless on the host (two CPUs; median of clean runs)

| | Idle: wakeups/s | Idle: CPU ms/s | One update: CPU ms | One update: wakeups | One move step: CPU ms |
|---|---:|---:|---:|---:|---:|
| Aizome compositor | 6.8 | 0.122 | 0.50 | 2.5 | 0.33 |
| Aizome shell | 47.7 | 0.518 | 0.10 | 10.2 | 0.11 |
| GNOME compositor+shell | 2.0 | 0.595 | 2.10 | 6.1 | 2.21 |
| Plasma compositor | 0.2 | 0.032 | 2.31 | 13.1 | 1.82 |
| Plasma shell | 3.6 | 1.171 | 0.34 | 1.5 | 0.07 |

### Where an update's and a move step's CPU goes, by thread (ms; mean of clean runs)

| | One update | One move step |
|---|---|---|
| Aizome | aizome: main thread 0.49 | aizome: main thread 0.33; aizome-shell: main thread 0.09 |
| GNOME | gnome-shell: main thread 1.06; gnome-shell: llvmpipe-1 0.70; gnome-shell: llvmpipe-0 0.32 | gnome-shell: llvmpipe-1 1.08; gnome-shell: llvmpipe-0 0.86; gnome-shell: main thread 0.27 |
| Plasma | kwin_wayland: llvmpipe-1 1.00; kwin_wayland: llvmpipe-0 0.80; kwin_wayland: main thread 0.51; plasmashell: main thread 0.21; plasmashell: llvmpipe-1 0.06 | kwin_wayland: llvmpipe-1 0.98; kwin_wayland: llvmpipe-0 0.68; kwin_wayland: main thread 0.16; plasmashell: main thread 0.05 |

### Who wakes at idle, headless (wakeups/s by thread; one clean run)

- Aizome compositor: aizome-watchdog 4.0; main thread 1.8; aizome-config-w 1.0
- Aizome shell: aizome-sys org. (7 threads) 28.0; aizome-sys audi 5.0; aizome-sys mpri 4.0; aizome-sys stat 4.0; blocking-1 2.0; aizome-config-w (2 threads) 2.0; aizome-sys back 1.0; meters 0.5
- GNOME compositor+shell: JS Helper (4 threads) 1.2; main thread 0.7; gmain 0.2
- Plasma shell: main thread 3.3; WaylandEventThr (2 threads) 0.3; QThread 0.2; plasmas:disk$0 0.1; QSGRenderThread 0.1
- Plasma compositor: main thread 0.2; llvmpipe-1 0.2; llvmpipe-0 0.1
