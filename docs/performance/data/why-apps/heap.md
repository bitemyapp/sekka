## kit: 3.57 MiB live in malloc 3 s after the first frame
    total runtime: 1.878000s.
    calls to allocation functions: 146178 (77837/s)
    peak heap memory consumption: 3.78M
    peak RSS (including heaptrack overhead): 24.91M
    total memory leaked: 3.74M
    suppressed leaks: 20.44K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| app and toolkit start | 2.63 | 73.7% |
| fontconfig | 0.63 | 17.6% |
| layout and drawing | 0.15 | 4.2% |
| library constructors (before main) | 0.07 | 2.0% |
| other | 0.05 | 1.3% |
| fonts: shaping and glyphs | 0.02 | 0.7% |
| Wayland and keymaps | 0.02 | 0.6% |

Largest direct callers of the allocator (MiB):
    3.57  

## sumi: 4.01 MiB live in malloc 3 s after the first frame
    total runtime: 3.077000s.
    calls to allocation functions: 154995 (50372/s)
    peak heap memory consumption: 4.27M
    peak RSS (including heaptrack overhead): 31.12M
    total memory leaked: 4.21M
    suppressed leaks: 20.44K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| app and toolkit start | 2.66 | 66.3% |
| fontconfig | 0.63 | 15.7% |
| layout and drawing | 0.24 | 6.0% |
| other | 0.21 | 5.3% |
| fonts: shaping and glyphs | 0.18 | 4.4% |
| library constructors (before main) | 0.07 | 1.8% |
| Wayland and keymaps | 0.02 | 0.5% |

Largest direct callers of the allocator (MiB):
    4.01  

## gtk4: 4.22 MiB live in malloc 3 s after the first frame
    total runtime: 1.490000s.
    calls to allocation functions: 235115 (157795/s)
    peak heap memory consumption: 7.59M
    peak RSS (including heaptrack overhead): 170.21M
    total memory leaked: 4.42M
    suppressed leaks: 762.78K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| fontconfig | 1.58 | 37.5% |
| CSS, theme and style | 0.59 | 14.0% |
| layout and drawing | 0.34 | 8.1% |
| GPU: Vulkan instance and devices | 0.33 | 7.8% |
| GPU: pipelines and shaders | 0.25 | 6.0% |
| library constructors (before main) | 0.22 | 5.2% |
| GObject type and class setup | 0.19 | 4.5% |
| Wayland and keymaps | 0.18 | 4.4% |
| icon theme and images | 0.15 | 3.7% |
| fonts: shaping and glyphs | 0.15 | 3.5% |
| GPU: rendering the frame | 0.09 | 2.1% |
| app and toolkit start | 0.06 | 1.5% |
| D-Bus, portals and accessibility | 0.06 | 1.5% |
| GIO and GTK modules | 0.01 | 0.2% |
| other | 0.00 | 0.1% |
| GPU: EGL and GL context | 0.00 | 0.0% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 0.00 | 0.0% |
| settings (GSettings, dconf, KConfig) | 0.00 | 0.0% |

Largest direct callers of the allocator (MiB):
    4.22  

## adw: 4.96 MiB live in malloc 3 s after the first frame
    total runtime: 1.574000s.
    calls to allocation functions: 363408 (230881/s)
    peak heap memory consumption: 8.77M
    peak RSS (including heaptrack overhead): 190.59M
    total memory leaked: 5.20M
    suppressed leaks: 764.85K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| fontconfig | 1.59 | 32.0% |
| CSS, theme and style | 1.16 | 23.3% |
| layout and drawing | 0.39 | 7.8% |
| GPU: Vulkan instance and devices | 0.33 | 6.6% |
| GObject type and class setup | 0.30 | 6.0% |
| fonts: shaping and glyphs | 0.24 | 4.9% |
| GPU: pipelines and shaders | 0.24 | 4.8% |
| library constructors (before main) | 0.22 | 4.4% |
| Wayland and keymaps | 0.18 | 3.7% |
| app and toolkit start | 0.10 | 1.9% |
| D-Bus, portals and accessibility | 0.09 | 1.8% |
| GPU: rendering the frame | 0.09 | 1.8% |
| icon theme and images | 0.02 | 0.3% |
| other | 0.01 | 0.2% |
| GIO and GTK modules | 0.01 | 0.1% |
| settings (GSettings, dconf, KConfig) | 0.01 | 0.1% |
| GPU: EGL and GL context | 0.00 | 0.0% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 0.00 | 0.0% |

Largest direct callers of the allocator (MiB):
    4.96  

## qtwidgets: 4.81 MiB live in malloc 3 s after the first frame
    total runtime: 0.712000s.
    calls to allocation functions: 309241 (434327/s)
    peak heap memory consumption: 6.46M
    peak RSS (including heaptrack overhead): 85.44M
    total memory leaked: 5.04M
    suppressed leaks: 140.75K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| fontconfig | 1.59 | 33.0% |
| fonts: shaping and glyphs | 1.04 | 21.7% |
| CSS, theme and style | 0.90 | 18.7% |
| GObject type and class setup | 0.36 | 7.5% |
| Wayland and keymaps | 0.27 | 5.7% |
| library constructors (before main) | 0.15 | 3.2% |
| app and toolkit start | 0.13 | 2.8% |
| D-Bus, portals and accessibility | 0.10 | 2.1% |
| icon theme and images | 0.10 | 2.0% |
| layout and drawing | 0.08 | 1.6% |
| Qt plugins (dlopen, metadata) | 0.05 | 1.1% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 0.02 | 0.4% |
| GIO and GTK modules | 0.01 | 0.1% |
| other | 0.00 | 0.1% |
| settings (GSettings, dconf, KConfig) | 0.00 | 0.0% |

Largest direct callers of the allocator (MiB):
    4.81  

## qtquick: 21.50 MiB live in malloc 3 s after the first frame
    total runtime: 2.423000s.
    calls to allocation functions: 428323 (176773/s)
    peak heap memory consumption: 23.37M
    peak RSS (including heaptrack overhead): 203.40M
    total memory leaked: 22.55M
    suppressed leaks: 765.69K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| GPU: rendering the frame | 8.35 | 38.8% |
| GPU: EGL and GL context | 4.64 | 21.6% |
| QML engine and Quick Controls | 2.60 | 12.1% |
| fontconfig | 1.59 | 7.4% |
| fonts: shaping and glyphs | 1.12 | 5.2% |
| other | 1.10 | 5.1% |
| CSS, theme and style | 0.86 | 4.0% |
| Wayland and keymaps | 0.28 | 1.3% |
| GObject type and class setup | 0.21 | 1.0% |
| GPU: pipelines and shaders | 0.17 | 0.8% |
| library constructors (before main) | 0.17 | 0.8% |
| Qt Quick items and scene graph | 0.13 | 0.6% |
| D-Bus, portals and accessibility | 0.10 | 0.5% |
| Qt plugins (dlopen, metadata) | 0.07 | 0.3% |
| app and toolkit start | 0.05 | 0.3% |
| layout and drawing | 0.04 | 0.2% |
| icon theme and images | 0.01 | 0.0% |
| GIO and GTK modules | 0.01 | 0.0% |
| GPU: Vulkan instance and devices | 0.00 | 0.0% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 0.00 | 0.0% |
| settings (GSettings, dconf, KConfig) | 0.00 | 0.0% |

Largest direct callers of the allocator (MiB):
   21.50  

## gnome-text-editor: 12.02 MiB live in malloc 3 s after the first frame
    total runtime: 3.296000s.
    calls to allocation functions: 1625302 (493113/s)
    peak heap memory consumption: 17.96M
    peak RSS (including heaptrack overhead): 217.29M
    total memory leaked: 12.60M
    suppressed leaks: 796.50K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| CSS, theme and style | 1.80 | 15.0% |
| fontconfig | 1.66 | 13.8% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 1.43 | 11.9% |
| app and toolkit start | 1.20 | 10.0% |
| layout and drawing | 1.18 | 9.8% |
| D-Bus, portals and accessibility | 1.12 | 9.4% |
| editor component (syntax, buffer, spell checking) | 0.80 | 6.7% |
| fonts: shaping and glyphs | 0.76 | 6.3% |
| GObject type and class setup | 0.57 | 4.8% |
| GPU: Vulkan instance and devices | 0.33 | 2.7% |
| GPU: pipelines and shaders | 0.32 | 2.7% |
| icon theme and images | 0.23 | 1.9% |
| library constructors (before main) | 0.22 | 1.8% |
| Wayland and keymaps | 0.18 | 1.5% |
| GPU: rendering the frame | 0.15 | 1.3% |
| other | 0.03 | 0.2% |
| settings (GSettings, dconf, KConfig) | 0.02 | 0.1% |
| GIO and GTK modules | 0.01 | 0.1% |
| GPU: EGL and GL context | 0.00 | 0.0% |

Largest direct callers of the allocator (MiB):
   12.02  

## kwrite: 3.84 MiB live in malloc 3 s after the first frame
    total runtime: 3.362000s.
    calls to allocation functions: 624709 (185814/s)
    peak heap memory consumption: 15.57M
    peak RSS (including heaptrack overhead): 148.84M
    total memory leaked: 4.03M
    suppressed leaks: 121.30K

| Purpose | live heap, MiB | share |
|---|---:|---:|
| fontconfig | 1.59 | 41.4% |
| CSS, theme and style | 0.85 | 22.1% |
| GObject type and class setup | 0.37 | 9.6% |
| Wayland and keymaps | 0.23 | 6.0% |
| app and toolkit start | 0.23 | 5.9% |
| fonts: shaping and glyphs | 0.22 | 5.6% |
| library constructors (before main) | 0.11 | 2.8% |
| editor component (syntax, buffer, spell checking) | 0.08 | 2.2% |
| layout and drawing | 0.07 | 1.7% |
| D-Bus, portals and accessibility | 0.06 | 1.6% |
| icon theme and images | 0.02 | 0.6% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 0.01 | 0.4% |
| GIO and GTK modules | 0.01 | 0.1% |
| Qt plugins (dlopen, metadata) | 0.00 | 0.1% |
| other | 0.00 | 0.0% |
| settings (GSettings, dconf, KConfig) | 0.00 | 0.0% |

Largest direct callers of the allocator (MiB):
    3.84  

