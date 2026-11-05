## kit: 1 process(es) (hello-kit); PSS 12.58 MiB, Anonymous 4.67 MiB, Private_Dirty 4.67 MiB, RSS 19.06 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| program binary (code and data) | 7.53 | 0.32 | 0.32 | 7.54 | 4 |
| malloc: main arena ([heap]) | 2.43 | 2.43 | 2.43 | 2.43 | 1 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 1.41 | 1.41 | 1.41 | 1.41 | 13 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| thread stacks | 0.20 | 0.20 | 0.20 | 0.20 | 3 |
| shared memory (memfd: Wayland buffers, caches) | 0.18 | 0.00 | 0.00 | 0.37 | 1 |
| library read-only data and RELRO (r--) | 0.18 | 0.13 | 0.13 | 1.23 | 39 |
| malloc: thread arenas | 0.12 | 0.12 | 0.12 | 0.12 | 10 |
| library writable data (rw-) | 0.05 | 0.05 | 0.05 | 0.05 | 13 |
| library code (r-x) | 0.04 | 0.00 | 0.00 | 2.59 | 13 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 0.13 | 0.07 | 0.07 | 0.61 |
| C and C++ runtime | 0.07 | 0.06 | 0.06 | 2.60 |
| other libraries | 0.06 | 0.05 | 0.05 | 0.52 |
| cairo, pixman, image codecs | 0.02 | 0.01 | 0.01 | 0.14 |

Shared objects with the most dirty (relocated or written) pages:

    0.04 MiB anonymous,   0.04 MiB PSS  libfreetype.so
    0.04 MiB anonymous,   0.09 MiB PSS  libfontconfig.so
    0.02 MiB anonymous,   0.03 MiB PSS  libc.so
    0.02 MiB anonymous,   0.02 MiB PSS  libexpat.so
    0.01 MiB anonymous,   0.01 MiB PSS  ld-linux-x86-64.so
    0.01 MiB anonymous,   0.01 MiB PSS  libbrotlicommon.so
    0.01 MiB anonymous,   0.01 MiB PSS  libz.so
    0.01 MiB anonymous,   0.01 MiB PSS  libbrotlidec.so
    0.01 MiB anonymous,   0.02 MiB PSS  libpng16.so
    0.01 MiB anonymous,   0.01 MiB PSS  libbz2.so
    0.01 MiB anonymous,   0.01 MiB PSS  libm.so
    0.01 MiB anonymous,   0.01 MiB PSS  libpthread.so
    0.01 MiB anonymous,   0.01 MiB PSS  libgcc_s.so

## gtk4: 1 process(es) (.hello-gtk4-wra); PSS 37.01 MiB, Anonymous 16.79 MiB, Private_Dirty 16.79 MiB, RSS 90.37 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| library read-only data and RELRO (r--) | 14.69 | 6.22 | 6.22 | 38.88 | 393 |
| library code (r-x) | 11.13 | 0.00 | 0.00 | 36.41 | 131 |
| malloc: main arena ([heap]) | 6.72 | 6.72 | 6.72 | 6.72 | 1 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 1.49 | 1.49 | 1.49 | 1.49 | 49 |
| malloc: thread arenas | 1.08 | 1.08 | 1.08 | 1.08 | 14 |
| library writable data (rw-) | 1.00 | 0.98 | 0.98 | 1.07 | 133 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| thread stacks | 0.29 | 0.29 | 0.29 | 0.29 | 5 |
| font files | 0.06 | 0.00 | 0.00 | 0.66 | 2 |
| locale data | 0.05 | 0.00 | 0.00 | 0.55 | 1 |
| icon caches | 0.03 | 0.00 | 0.00 | 0.06 | 2 |
| program binary (code and data) | 0.02 | 0.01 | 0.01 | 0.02 | 5 |
| shared memory (memfd: Wayland buffers, caches) | 0.01 | 0.00 | 0.00 | 0.02 | 3 |
| Mesa shader cache | 0.00 | 0.00 | 0.00 | 0.01 | 2 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| GPU drivers (Mesa, LLVM, NVIDIA, Vulkan loader) | 13.79 | 5.11 | 5.11 | 42.86 |
| GTK, libadwaita, GDK, GSK | 7.82 | 0.32 | 0.32 | 8.91 |
| other libraries | 2.41 | 0.99 | 0.99 | 7.61 |
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 1.01 | 0.16 | 0.16 | 3.57 |
| cairo, pixman, image codecs | 0.67 | 0.15 | 0.15 | 2.43 |
| D-Bus, a11y, portals, input methods | 0.55 | 0.19 | 0.19 | 2.46 |
| C and C++ runtime | 0.23 | 0.14 | 0.14 | 4.20 |
| GLib, GObject, GIO | 0.20 | 0.07 | 0.07 | 3.56 |
| Wayland, xkbcommon | 0.09 | 0.05 | 0.05 | 0.61 |
| GTK 3 (Qt's gtk3 theme) | 0.04 | 0.01 | 0.01 | 0.16 |

Shared objects with the most dirty (relocated or written) pages:

    2.07 MiB anonymous,   3.93 MiB PSS  libLLVM.so
    1.57 MiB anonymous,   2.06 MiB PSS  libgallium-26.2.6.so
    0.41 MiB anonymous,   3.86 MiB PSS  libvulkan_radeon.so
    0.37 MiB anonymous,   1.93 MiB PSS  libvulkan_lvp.so
    0.31 MiB anonymous,   1.14 MiB PSS  libvulkan_dzn.so
    0.26 MiB anonymous,   7.52 MiB PSS  libgtk-4.so
    0.12 MiB anonymous,   0.17 MiB PSS  libp11-kit.so
    0.11 MiB anonymous,   0.15 MiB PSS  libGLdispatch.so
    0.08 MiB anonymous,   0.19 MiB PSS  libgnutls.so
    0.08 MiB anonymous,   0.23 MiB PSS  libgstvideo-1.0.so
    0.07 MiB anonymous,   0.15 MiB PSS  libstdc++.so
    0.06 MiB anonymous,   0.12 MiB PSS  libtinysparql-3.0.so
    0.06 MiB anonymous,   0.23 MiB PSS  libepoxy.so
    0.05 MiB anonymous,   0.09 MiB PSS  libEGL_mesa.so
    0.05 MiB anonymous,   0.15 MiB PSS  liborc-0.4.so

## adw: 1 process(es) (.hello-adw-wrap); PSS 41.93 MiB, Anonymous 19.29 MiB, Private_Dirty 19.29 MiB, RSS 99.39 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| library read-only data and RELRO (r--) | 17.19 | 7.32 | 7.32 | 43.81 | 456 |
| library code (r-x) | 12.12 | 0.00 | 0.00 | 38.37 | 152 |
| malloc: main arena ([heap]) | 7.54 | 7.54 | 7.54 | 7.54 | 1 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 1.89 | 1.89 | 1.89 | 1.89 | 51 |
| library writable data (rw-) | 1.14 | 1.12 | 1.12 | 1.24 | 155 |
| malloc: thread arenas | 1.09 | 1.09 | 1.09 | 1.09 | 16 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| thread stacks | 0.32 | 0.32 | 0.32 | 0.32 | 5 |
| font files | 0.10 | 0.00 | 0.00 | 1.30 | 4 |
| locale data | 0.05 | 0.00 | 0.00 | 0.55 | 1 |
| program binary (code and data) | 0.02 | 0.01 | 0.01 | 0.02 | 5 |
| other files | 0.02 | 0.00 | 0.00 | 0.12 | 7 |
| shared memory (memfd: Wayland buffers, caches) | 0.01 | 0.00 | 0.00 | 0.02 | 3 |
| Mesa shader cache | 0.00 | 0.00 | 0.00 | 0.01 | 2 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| GPU drivers (Mesa, LLVM, NVIDIA, Vulkan loader) | 13.73 | 5.11 | 5.11 | 42.74 |
| GTK, libadwaita, GDK, GSK | 10.33 | 0.77 | 0.77 | 11.86 |
| other libraries | 3.57 | 1.77 | 1.77 | 11.49 |
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 0.99 | 0.16 | 0.16 | 3.54 |
| cairo, pixman, image codecs | 0.67 | 0.15 | 0.15 | 2.47 |
| D-Bus, a11y, portals, input methods | 0.58 | 0.20 | 0.20 | 2.59 |
| C and C++ runtime | 0.24 | 0.14 | 0.14 | 4.33 |
| GLib, GObject, GIO | 0.21 | 0.07 | 0.07 | 3.61 |
| Wayland, xkbcommon | 0.09 | 0.05 | 0.05 | 0.61 |
| GTK 3 (Qt's gtk3 theme) | 0.05 | 0.01 | 0.01 | 0.18 |

Shared objects with the most dirty (relocated or written) pages:

    2.07 MiB anonymous,   3.93 MiB PSS  libLLVM.so
    1.57 MiB anonymous,   2.06 MiB PSS  libgallium-26.2.6.so
    0.50 MiB anonymous,   0.55 MiB PSS  libcrypto.so
    0.41 MiB anonymous,   0.76 MiB PSS  libappstream.so
    0.41 MiB anonymous,   3.80 MiB PSS  libvulkan_radeon.so
    0.37 MiB anonymous,   1.93 MiB PSS  libvulkan_lvp.so
    0.31 MiB anonymous,   1.14 MiB PSS  libvulkan_dzn.so
    0.26 MiB anonymous,   7.71 MiB PSS  libgtk-4.so
    0.12 MiB anonymous,   0.17 MiB PSS  libp11-kit.so
    0.11 MiB anonymous,   0.15 MiB PSS  libGLdispatch.so
    0.08 MiB anonymous,   0.19 MiB PSS  libgnutls.so
    0.08 MiB anonymous,   0.23 MiB PSS  libgstvideo-1.0.so
    0.07 MiB anonymous,   0.15 MiB PSS  libstdc++.so
    0.06 MiB anonymous,   0.10 MiB PSS  libkrb5.so
    0.06 MiB anonymous,   0.12 MiB PSS  libtinysparql-3.0.so

## qtwidgets: 1 process(es) (.hello-qtwidget); PSS 34.94 MiB, Anonymous 11.58 MiB, Private_Dirty 11.83 MiB, RSS 65.76 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| library code (r-x) | 11.73 | 0.00 | 0.00 | 27.41 | 97 |
| library read-only data and RELRO (r--) | 11.72 | 2.25 | 2.25 | 21.71 | 293 |
| malloc: main arena ([heap]) | 6.75 | 6.75 | 6.75 | 6.75 | 1 |
| library writable data (rw-) | 1.15 | 0.62 | 0.62 | 1.19 | 100 |
| malloc: thread arenas | 1.11 | 1.11 | 1.11 | 1.11 | 12 |
| shared memory (memfd: Wayland buffers, caches) | 1.03 | 0.00 | 0.25 | 1.81 | 5 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 0.64 | 0.64 | 0.64 | 0.64 | 33 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| thread stacks | 0.20 | 0.20 | 0.20 | 0.20 | 3 |
| font files | 0.15 | 0.00 | 0.00 | 1.36 | 4 |
| program binary (code and data) | 0.02 | 0.01 | 0.01 | 0.02 | 5 |
| locale data | 0.01 | 0.00 | 0.00 | 0.44 | 3 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| Qt | 13.64 | 0.70 | 0.70 | 17.89 |
| GTK 3 (Qt's gtk3 theme) | 4.19 | 0.17 | 0.17 | 7.41 |
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 2.94 | 0.31 | 0.31 | 5.60 |
| other libraries | 1.83 | 0.96 | 0.96 | 5.40 |
| cairo, pixman, image codecs | 0.47 | 0.09 | 0.09 | 1.84 |
| GPU drivers (Mesa, LLVM, NVIDIA, Vulkan loader) | 0.44 | 0.20 | 0.20 | 1.73 |
| D-Bus, a11y, portals, input methods | 0.28 | 0.11 | 0.11 | 1.45 |
| C and C++ runtime | 0.27 | 0.14 | 0.14 | 4.19 |
| GTK, libadwaita, GDK, GSK | 0.23 | 0.06 | 0.06 | 0.56 |
| GLib, GObject, GIO | 0.23 | 0.08 | 0.08 | 3.64 |
| Wayland, xkbcommon | 0.09 | 0.05 | 0.05 | 0.61 |

Shared objects with the most dirty (relocated or written) pages:

    0.50 MiB anonymous,   0.55 MiB PSS  libcrypto.so
    0.28 MiB anonymous,   3.07 MiB PSS  libQt6Widgets.so
    0.17 MiB anonymous,   5.08 MiB PSS  libQt6Gui.so
    0.11 MiB anonymous,   0.15 MiB PSS  libGLdispatch.so
    0.10 MiB anonymous,   3.32 MiB PSS  libQt6Core.so
    0.09 MiB anonymous,   3.59 MiB PSS  libgtk-3.so
    0.08 MiB anonymous,   0.59 MiB PSS  libicuuc.so
    0.08 MiB anonymous,   1.34 MiB PSS  libicui18n.so
    0.07 MiB anonymous,   0.18 MiB PSS  libstdc++.so
    0.07 MiB anonymous,   0.95 MiB PSS  libQt6WaylandClient.so
    0.06 MiB anonymous,   0.12 MiB PSS  libtinysparql-3.0.so
    0.06 MiB anonymous,   0.23 MiB PSS  libepoxy.so
    0.05 MiB anonymous,   0.07 MiB PSS  libssl.so
    0.05 MiB anonymous,   0.42 MiB PSS  libgdk-3.so
    0.04 MiB anonymous,   0.11 MiB PSS  libgio-2.0.so

## qtquick: 1 process(es) (.hello-qtquick-); PSS 86.70 MiB, Anonymous 36.83 MiB, Private_Dirty 37.10 MiB, RSS 162.25 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| library code (r-x) | 30.92 | 0.00 | 0.00 | 74.54 | 155 |
| library read-only data and RELRO (r--) | 23.96 | 6.70 | 6.70 | 50.97 | 467 |
| malloc: thread arenas | 17.17 | 17.17 | 17.17 | 17.17 | 30 |
| malloc: main arena ([heap]) | 8.40 | 8.40 | 8.40 | 8.40 | 1 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 2.30 | 2.30 | 2.30 | 2.30 | 47 |
| library writable data (rw-) | 1.81 | 1.24 | 1.24 | 1.93 | 159 |
| shared memory (memfd: Wayland buffers, caches) | 0.87 | 0.61 | 0.87 | 0.87 | 9 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| thread stacks | 0.39 | 0.39 | 0.39 | 0.39 | 10 |
| Mesa shader cache | 0.21 | 0.00 | 0.01 | 0.25 | 1 |
| font files | 0.15 | 0.00 | 0.00 | 1.73 | 5 |
| locale data | 0.05 | 0.00 | 0.00 | 0.56 | 3 |
| program binary (code and data) | 0.02 | 0.01 | 0.01 | 0.02 | 5 |
| other files | 0.00 | 0.00 | 0.00 | 0.00 | 1 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| Qt | 29.19 | 1.68 | 1.68 | 34.31 |
| GPU drivers (Mesa, LLVM, NVIDIA, Vulkan loader) | 17.67 | 4.00 | 4.00 | 58.71 |
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 2.93 | 0.31 | 0.31 | 5.52 |
| GTK 3 (Qt's gtk3 theme) | 2.79 | 0.17 | 0.17 | 5.94 |
| other libraries | 2.59 | 1.23 | 1.23 | 10.46 |
| cairo, pixman, image codecs | 0.41 | 0.09 | 0.09 | 1.76 |
| D-Bus, a11y, portals, input methods | 0.29 | 0.12 | 0.12 | 1.46 |
| C and C++ runtime | 0.27 | 0.14 | 0.14 | 4.50 |
| GTK, libadwaita, GDK, GSK | 0.23 | 0.06 | 0.06 | 0.56 |
| GLib, GObject, GIO | 0.23 | 0.08 | 0.08 | 3.62 |
| Wayland, xkbcommon | 0.09 | 0.05 | 0.05 | 0.61 |

Shared objects with the most dirty (relocated or written) pages:

    2.07 MiB anonymous,   8.78 MiB PSS  libLLVM.so
    1.57 MiB anonymous,   8.09 MiB PSS  libgallium-26.2.6.so
    0.50 MiB anonymous,   0.74 MiB PSS  libcrypto.so
    0.36 MiB anonymous,   5.71 MiB PSS  libQt6Quick.so
    0.23 MiB anonymous,   2.30 MiB PSS  libQt6QuickTemplates2.so
    0.19 MiB anonymous,   5.97 MiB PSS  libQt6Qml.so
    0.17 MiB anonymous,   5.27 MiB PSS  libQt6Gui.so
    0.11 MiB anonymous,   0.18 MiB PSS  libGLdispatch.so
    0.10 MiB anonymous,   3.59 MiB PSS  libQt6Core.so
    0.09 MiB anonymous,   0.71 MiB PSS  libQt6QuickControls2Basic.so
    0.09 MiB anonymous,   2.19 MiB PSS  libgtk-3.so
    0.08 MiB anonymous,   0.71 MiB PSS  libQt6QuickControls2Fusion.so
    0.08 MiB anonymous,   0.59 MiB PSS  libicuuc.so
    0.08 MiB anonymous,   1.34 MiB PSS  libicui18n.so
    0.07 MiB anonymous,   0.19 MiB PSS  libstdc++.so

## sumi: 1 process(es) (aizome-sumi); PSS 16.09 MiB, Anonymous 5.58 MiB, Private_Dirty 5.58 MiB, RSS 23.28 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| program binary (code and data) | 9.86 | 0.57 | 0.57 | 9.86 | 6 |
| malloc: main arena ([heap]) | 2.86 | 2.86 | 2.86 | 2.86 | 1 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 1.39 | 1.39 | 1.39 | 1.39 | 13 |
| shared memory (memfd: Wayland buffers, caches) | 0.69 | 0.00 | 0.00 | 1.38 | 1 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| thread stacks | 0.36 | 0.36 | 0.36 | 0.36 | 8 |
| malloc: thread arenas | 0.22 | 0.22 | 0.22 | 0.22 | 18 |
| library read-only data and RELRO (r--) | 0.18 | 0.13 | 0.13 | 1.38 | 39 |
| library writable data (rw-) | 0.05 | 0.05 | 0.05 | 0.05 | 13 |
| library code (r-x) | 0.04 | 0.00 | 0.00 | 2.66 | 13 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 0.14 | 0.07 | 0.07 | 0.66 |
| C and C++ runtime | 0.07 | 0.06 | 0.06 | 2.79 |
| other libraries | 0.06 | 0.05 | 0.05 | 0.51 |
| cairo, pixman, image codecs | 0.01 | 0.01 | 0.01 | 0.13 |

Shared objects with the most dirty (relocated or written) pages:

    0.04 MiB anonymous,   0.04 MiB PSS  libfreetype.so
    0.04 MiB anonymous,   0.10 MiB PSS  libfontconfig.so
    0.02 MiB anonymous,   0.03 MiB PSS  libc.so
    0.02 MiB anonymous,   0.02 MiB PSS  libexpat.so
    0.01 MiB anonymous,   0.01 MiB PSS  ld-linux-x86-64.so
    0.01 MiB anonymous,   0.01 MiB PSS  libbrotlicommon.so
    0.01 MiB anonymous,   0.01 MiB PSS  libz.so
    0.01 MiB anonymous,   0.01 MiB PSS  libbrotlidec.so
    0.01 MiB anonymous,   0.01 MiB PSS  libpng16.so
    0.01 MiB anonymous,   0.01 MiB PSS  libbz2.so
    0.01 MiB anonymous,   0.01 MiB PSS  libm.so
    0.01 MiB anonymous,   0.01 MiB PSS  libpthread.so
    0.01 MiB anonymous,   0.01 MiB PSS  libgcc_s.so

## gnome-text-editor: 1 process(es) (.gnome-text-edi); PSS 68.87 MiB, Anonymous 30.73 MiB, Private_Dirty 30.73 MiB, RSS 129.84 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| library read-only data and RELRO (r--) | 24.46 | 7.54 | 7.54 | 51.90 | 512 |
| library code (r-x) | 19.54 | 0.00 | 0.00 | 47.13 | 170 |
| malloc: main arena ([heap]) | 17.45 | 17.45 | 17.45 | 17.45 | 1 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 2.11 | 2.11 | 2.11 | 2.11 | 61 |
| malloc: thread arenas | 1.97 | 1.97 | 1.97 | 1.97 | 24 |
| library writable data (rw-) | 1.26 | 1.19 | 1.19 | 1.37 | 173 |
| program binary (code and data) | 0.58 | 0.02 | 0.02 | 0.58 | 5 |
| thread stacks | 0.45 | 0.45 | 0.45 | 0.45 | 7 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| font files | 0.34 | 0.00 | 0.00 | 2.77 | 14 |
| locale data | 0.17 | 0.00 | 0.00 | 0.68 | 2 |
| other files | 0.06 | 0.00 | 0.00 | 0.22 | 9 |
| icon caches | 0.03 | 0.00 | 0.00 | 0.06 | 2 |
| shared memory (memfd: Wayland buffers, caches) | 0.01 | 0.00 | 0.00 | 0.02 | 3 |
| Mesa shader cache | 0.00 | 0.00 | 0.00 | 0.01 | 2 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| GPU drivers (Mesa, LLVM, NVIDIA, Vulkan loader) | 19.31 | 5.11 | 5.11 | 48.39 |
| GTK, libadwaita, GDK, GSK | 11.54 | 0.77 | 0.77 | 13.09 |
| other libraries | 9.53 | 1.98 | 1.98 | 18.22 |
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 2.65 | 0.24 | 0.24 | 5.80 |
| cairo, pixman, image codecs | 0.69 | 0.15 | 0.15 | 2.53 |
| D-Bus, a11y, portals, input methods | 0.58 | 0.20 | 0.20 | 2.64 |
| C and C++ runtime | 0.56 | 0.14 | 0.14 | 5.23 |
| GLib, GObject, GIO | 0.27 | 0.07 | 0.07 | 3.75 |
| Wayland, xkbcommon | 0.09 | 0.05 | 0.05 | 0.61 |
| GTK 3 (Qt's gtk3 theme) | 0.04 | 0.01 | 0.01 | 0.15 |

Shared objects with the most dirty (relocated or written) pages:

    2.07 MiB anonymous,   3.93 MiB PSS  libLLVM.so
    1.57 MiB anonymous,   2.06 MiB PSS  libgallium-26.2.6.so
    0.50 MiB anonymous,   0.55 MiB PSS  libcrypto.so
    0.41 MiB anonymous,   0.75 MiB PSS  libappstream.so
    0.41 MiB anonymous,   9.36 MiB PSS  libvulkan_radeon.so
    0.37 MiB anonymous,   1.93 MiB PSS  libvulkan_lvp.so
    0.31 MiB anonymous,   1.14 MiB PSS  libvulkan_dzn.so
    0.26 MiB anonymous,   8.71 MiB PSS  libgtk-4.so
    0.12 MiB anonymous,   0.16 MiB PSS  libp11-kit.so
    0.11 MiB anonymous,   0.15 MiB PSS  libGLdispatch.so
    0.08 MiB anonymous,   0.19 MiB PSS  libgnutls.so
    0.08 MiB anonymous,   0.19 MiB PSS  libgstvideo-1.0.so
    0.08 MiB anonymous,   1.08 MiB PSS  libicuuc.so
    0.07 MiB anonymous,   0.44 MiB PSS  libstdc++.so
    0.06 MiB anonymous,   0.10 MiB PSS  libkrb5.so

## kwrite: 1 process(es) (.kwrite-wrapped); PSS 73.37 MiB, Anonymous 21.32 MiB, Private_Dirty 21.57 MiB, RSS 113.22 MiB

| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |
|---|---:|---:|---:|---:|---:|
| library code (r-x) | 27.01 | 0.00 | 0.00 | 47.66 | 193 |
| library read-only data and RELRO (r--) | 25.61 | 3.89 | 3.89 | 38.34 | 581 |
| malloc: main arena ([heap]) | 13.54 | 13.54 | 13.54 | 13.54 | 1 |
| shared memory (memfd: Wayland buffers, caches) | 2.01 | 0.00 | 0.25 | 3.77 | 4 |
| library writable data (rw-) | 1.72 | 1.19 | 1.19 | 1.76 | 196 |
| malloc: thread arenas | 1.19 | 1.19 | 1.19 | 1.19 | 24 |
| other anonymous (large malloc chunks, driver and runtime mmaps) | 1.03 | 1.03 | 1.03 | 1.03 | 56 |
| thread stacks | 0.48 | 0.48 | 0.48 | 0.48 | 7 |
| fontconfig caches | 0.43 | 0.00 | 0.00 | 3.10 | 19 |
| font files | 0.23 | 0.00 | 0.00 | 1.66 | 6 |
| other files | 0.08 | 0.00 | 0.00 | 0.20 | 3 |
| program binary (code and data) | 0.03 | 0.01 | 0.01 | 0.03 | 5 |
| locale data | 0.02 | 0.00 | 0.00 | 0.45 | 6 |
| kernel ([vdso], [vvar]) | 0.00 | 0.00 | 0.00 | 0.02 | 4 |

Shared objects by group (all segments):

| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |
|---|---:|---:|---:|---:|
| Qt | 38.24 | 2.06 | 2.06 | 43.69 |
| other libraries | 6.86 | 1.78 | 1.78 | 16.63 |
| GTK 3 (Qt's gtk3 theme) | 4.20 | 0.17 | 0.17 | 7.43 |
| fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU) | 2.90 | 0.31 | 0.31 | 5.47 |
| GPU drivers (Mesa, LLVM, NVIDIA, Vulkan loader) | 0.46 | 0.21 | 0.21 | 1.78 |
| cairo, pixman, image codecs | 0.42 | 0.09 | 0.09 | 1.77 |
| D-Bus, a11y, portals, input methods | 0.36 | 0.12 | 0.12 | 1.75 |
| C and C++ runtime | 0.33 | 0.14 | 0.14 | 4.43 |
| GLib, GObject, GIO | 0.24 | 0.08 | 0.08 | 3.63 |
| GTK, libadwaita, GDK, GSK | 0.23 | 0.06 | 0.06 | 0.57 |
| Wayland, xkbcommon | 0.09 | 0.05 | 0.05 | 0.61 |

Shared objects with the most dirty (relocated or written) pages:

    0.50 MiB anonymous,   0.74 MiB PSS  libcrypto.so
    0.28 MiB anonymous,   4.88 MiB PSS  libQt6Widgets.so
    0.19 MiB anonymous,   1.47 MiB PSS  libQt6Qml.so
    0.17 MiB anonymous,   5.96 MiB PSS  libQt6Gui.so
    0.12 MiB anonymous,   4.08 MiB PSS  libKF6TextEditor.so
    0.11 MiB anonymous,   0.13 MiB PSS  libvorbisenc.so
    0.11 MiB anonymous,   0.15 MiB PSS  libGLdispatch.so
    0.10 MiB anonymous,   0.24 MiB PSS  libpipewire-0.3.so
    0.10 MiB anonymous,   4.22 MiB PSS  libQt6Core.so
    0.10 MiB anonymous,   1.31 MiB PSS  libKF6WidgetsAddons.so
    0.09 MiB anonymous,   0.85 MiB PSS  libQt6Multimedia.so
    0.09 MiB anonymous,   3.59 MiB PSS  libgtk-3.so
    0.08 MiB anonymous,   1.65 MiB PSS  libkateprivate.so
    0.08 MiB anonymous,   0.59 MiB PSS  libicuuc.so
    0.08 MiB anonymous,   1.34 MiB PSS  libicui18n.so

