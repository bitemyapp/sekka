Milestones, ms from the fork (median over runs):

| Milestone | kit | gtk4 | adw | qtwidgets | qtquick | sumi | gnome-text-editor | kwrite |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| first exec: libraries mapped and relocated (first ctor) | 0.8 | 0.5 | 0.7 | 0.5 | 0.5 | 0.9 | 0.5 | 0.5 |
| program's ctor (after a Nix wrapper's exec) | 0.8 | 12.6 | 16.3 | 6.3 | 10.7 | 0.9 | 17.9 | 26.8 |
| main() | 0.8 | 12.6 | 16.4 | 6.3 | 10.7 | 0.9 | 17.9 | 26.8 |
| sway's window new event (first frame) | 19.5 | 104.0 | 129.5 | 46.5 | 95.2 | 19.9 | 171.7 | 110.9 |
| xdg toplevel created | nan | 98.4 | 122.5 | 45.3 | 83.2 | nan | 155.0 | 95.5 |
| first buffer committed to the toplevel | nan | 103.6 | 129.1 | 45.9 | 94.7 | nan | 171.2 | 109.8 |

Time inside each kind of call before the first frame, ms (median; main thread unless noted):

| Span | kit | gtk4 | adw | qtwidgets | qtquick | sumi | gnome-text-editor | kwrite |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| D-Bus: blocking calls | 0.0 | 1.3 | 1.5 | 1.3 | 1.1 | 0.0 | 2.5 | 1.1 |
| EGL: display, initialize, context | 0.0 | 0.0 | 0.0 | 0.0 | 20.8 | 0.0 | 0.0 | 0.0 |
| Vulkan: instance, ICDs, devices | 0.0 | 69.1 | 72.6 | 0.0 | 0.0 | 0.0 | 74.7 | 0.0 |
| Vulkan: pipelines | 0.0 | 0.1 | 0.1 | 0.0 | 0.0 | 0.0 | 0.2 | 0.0 |
| Wayland: connect and roundtrips | 0.0 | 0.9 | 0.9 | 0.5 | 1.6 | 0.0 | 0.9 | 0.3 |
| dlopen: GIO modules | 0.0 | 0.4 | 0.5 | 0.4 | 0.4 | 0.0 | 0.4 | 0.6 |
| dlopen: GPU drivers and Vulkan ICDs | 0.0 | 48.4 | 52.7 | 0.0 | 10.4 | 0.0 | 54.8 | 0.0 |
| dlopen: GTK modules | 0.0 | 0.4 | 0.4 | 0.0 | 0.0 | 0.0 | 0.4 | 0.0 |
| dlopen: Qt plugins and QML modules | 0.0 | 0.0 | 0.0 | 7.5 | 10.9 | 0.0 | 0.0 | 13.6 |
| dlopen: all (outermost) | 0.3 | 49.2 | 53.6 | 7.8 | 21.6 | 0.0 | 57.4 | 14.3 |
| dlopen: fontconfig | 0.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 |
| dlopen: other | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 1.8 | 0.0 |
| fontconfig (any thread) | 7.4 | 4.2 | 4.2 | 6.4 | 6.7 | 7.1 | 4.1 | 7.2 |
| main thread blocked in poll (>= 0.2 ms each) | 0.0 | 1.1 | 1.3 | 0.8 | 1.0 | 0.0 | 2.0 | 1.0 |
| xkbcommon | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 |

### kit, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.79     0.00  tid+0    ctor                             /tmp/claude-1000/perf/design-kit-target/release/hello-kit
    0.85     0.00  tid+0    main                             /tmp/claude-1000/perf/design-kit-target/release/hello-kit
    1.25     0.32  tid+0    dlopen                           fontconfig-2.18.3-lib/lib/libfontconfig.so.1
    6.28     0.88  tid+0    FcFontSort                       161
    7.77     0.81  tid+0    FcFontSort                       188
    9.22     0.75  tid+0    FcFontSort                       149
   10.53     0.76  tid+0    FcFontSort                       144
   11.82     0.76  tid+0    FcFontSort                       143
   13.12     0.77  tid+0    FcFontSort                       144
   14.42     0.83  tid+0    FcFontSort                       143
   15.79     0.79  tid+0    FcFontSort                       143
   17.11     0.45  tid+0    FcFontSort                       1623
   18.23     0.00  tid+0    pthread_create                   hello-kit ?
   18.29     0.00  tid+0    pthread_create                   hello-kit ?
   18.31     0.00  tid+0    pthread_create                   hello-kit ?
   18.34     0.00  tid+2    pthread_create                   hello-kit ?
   18.35     0.00  tid+0    pthread_create                   hello-kit ?
   18.57     0.30  tid+0    FcFontMatch                      
   18.93     0.23  tid+0    FcFontMatch                      
   19.51           sway: window new
```

### gtk4, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.73     0.00  tid+0    ctor                             hello-gtk4-1/bin/hello-gtk4
    0.79     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-gtk4
   12.82     0.00  tid+0    ctor                             hello-gtk4-1/bin/.hello-gtk4-wrapped
   12.84     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-gtk4
   13.08     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   13.13     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   13.43     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   13.87     0.00  tid+0    g_bus_get_sync                   2
   13.87     0.11  tid+0    g_dbus_connection_call_sync      org.freedesktop.DBus org.freedesktop.DBus.ListActivatableNames ok
   13.99     0.00  tid+0    g_bus_get_sync                   2
   13.99     0.14  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   14.13     0.00  tid+0    g_bus_get_sync                   2
   14.14     0.06  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Documents org.freedesktop.portal.FileTransfer
   14.20     0.74  tid+3    poll                             2 ppoll
   14.24     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   14.29     4.41  tid+4    FcInit                           
   14.40     0.39  tid+0    dlopen                           /run/current-system/sw/lib/gtk-4.0/4.0.0/immodules/libim-ibus.so
   14.87     0.01  tid+0    wl_display_connect               
   14.91     0.00  tid+0    xkb_context_new                  
   14.92     0.00  tid+0    g_bus_get_sync                   2
   14.93     0.08  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   15.01     0.18  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   15.19     0.92  tid+0    poll                             1 ppoll
   15.20     0.85  tid+3    poll                             2 ppoll
   16.12     1.56  tid+3    poll                             2 ppoll
   16.20     0.10  tid+0    wl_display_roundtrip             
   16.20     0.10  tid+0    wl_display_roundtrip             queue
   16.32     0.00  tid+0    xkb_context_new                  
   17.65     0.00  tid+0    g_bus_get_sync                   2
   17.66     0.13  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   17.79     0.19  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Inhibit
   18.32     0.47  tid+0    dlopen                           gvfs-1.60.3/lib/gio/modules/libgvfsdbus.so
   18.83     0.00  tid+0    g_bus_get_sync                   2
   19.22     8.84  tid+3    poll                             2 ppoll
   25.60     0.00  tid+1    pthread_create                   libglib-2.0.so.0 ?
   28.04     0.00  tid+0    g_bus_get_sync                   2
   28.05     0.20  tid+0    g_dbus_connection_call_sync      org.a11y.Bus org.a11y.Bus.GetAddress ok
   28.25    68.95  tid+3    poll                             2 ppoll
   28.63    37.56  tid+0    vkEnumerateInstanceExtensionProperties 
   28.77     0.60  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   29.40     9.10  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   38.55     0.37  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   38.94     9.10  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
   48.08     1.49  tid+0    dlopen                           mesa-26.2.6/lib/libEGL_mesa.so.0
   50.76     0.37  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
   51.15     0.40  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
   51.57     0.54  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
   52.14     0.47  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
   52.63     0.33  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
   52.98     0.33  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
   53.33     0.46  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
   53.81     0.44  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
   54.28     1.32  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
   55.63     0.22  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
   55.91     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   55.92     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   55.93     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   55.94     8.55  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
   65.81     0.01  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
   65.83     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
   65.83     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
   65.84     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
   65.85     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
   65.86     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
   65.87     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
   65.88     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
   65.90     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
   65.91     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
   66.19     8.66  tid+0    vkEnumerateInstanceExtensionProperties 
   66.26     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   66.27     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   66.28     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   66.29     7.31  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
   74.55     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
   74.56     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
   74.57     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
   74.58     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
   74.59     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
   74.60     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
   74.60     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
   74.62     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
   74.64     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
   74.65     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
   74.85    15.05  tid+0    vkCreateInstance                 0
   74.96     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   74.97     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   74.98     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   74.98     6.81  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
   82.74     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
   82.75     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
   82.76     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
   82.77     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
   82.78     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
   82.78     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
   82.79     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
   82.80     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
   82.82     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
   82.83     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
   82.98     0.08  tid+0    dlopen                           mesa-26.2.6/lib/libVkLayer_MESA_device_select.so
   89.93     4.34  tid+0    vkEnumeratePhysicalDevices       2
   93.07     0.00  tid+0    pthread_create                   libvulkan_radeon.so ?
   93.15     0.00  tid+0    pthread_create                   libvulkan_radeon.so ?
   94.01     0.02  tid+0    wl_display_connect               
   94.12     0.15  tid+0    wl_display_roundtrip             
   94.12     0.15  tid+0    wl_display_roundtrip             queue
   94.27     0.91  tid+0    vkEnumeratePhysicalDevices       2
   94.29     0.01  tid+0    wl_display_connect               
   94.32     0.11  tid+0    wl_display_roundtrip             
   94.32     0.11  tid+0    wl_display_roundtrip             queue
   95.21     1.11  tid+0    vkCreateDevice                   0
   96.47     0.05  tid+0    wl_display_roundtrip             queue
   96.52     0.05  tid+0    wl_display_roundtrip             queue
   96.57     0.02  tid+0    wl_display_roundtrip             queue
   96.60     0.03  tid+0    wl_display_roundtrip             queue
   96.63     0.03  tid+0    wl_display_roundtrip             queue
   96.66     0.01  tid+0    wl_display_roundtrip             queue
   96.68     0.03  tid+0    wl_display_roundtrip             queue
   96.70     0.02  tid+0    wl_display_roundtrip             queue
   96.72     0.01  tid+0    wl_display_roundtrip             queue
   96.73     0.02  tid+0    wl_display_roundtrip             queue
   96.76     0.02  tid+0    wl_display_roundtrip             queue
   96.77     0.01  tid+0    wl_display_roundtrip             queue
   96.78     0.02  tid+0    wl_display_roundtrip             queue
   96.81     0.01  tid+0    wl_display_roundtrip             queue
   96.82     0.00  tid+0    wl_display_roundtrip             queue
   96.83     0.03  tid+0    wl_display_roundtrip             queue
   96.86     0.01  tid+0    wl_display_roundtrip             queue
   96.88     0.00  tid+0    wl_display_roundtrip             queue
   96.88     0.02  tid+0    wl_display_roundtrip             queue
   97.10     0.18  tid+0    g_dbus_connection_new_for_address_sync ok
   97.30     0.00  tid+0    xdg_toplevel                     64
   97.31     0.00  tid+0    commit                           32 toplevel
   97.36     3.38  tid+3    poll                             3 ppoll
  100.39     0.04  tid+0    wl_display_roundtrip             queue
  100.44     0.03  tid+0    wl_display_roundtrip             queue
  100.47     0.01  tid+0    wl_display_roundtrip             queue
  100.48     0.03  tid+0    wl_display_roundtrip             queue
  100.51     0.02  tid+0    wl_display_roundtrip             queue
  100.53     0.01  tid+0    wl_display_roundtrip             queue
  100.54     0.01  tid+0    wl_display_roundtrip             queue
  100.75     1.17  tid+3    poll                             3 ppoll
  101.55     0.03  tid+0    vkCreateGraphicsPipelines        1
  101.62     0.04  tid+0    vkCreateGraphicsPipelines        1
  101.67     0.03  tid+0    vkCreateGraphicsPipelines        1
  101.72     0.03  tid+0    vkCreateGraphicsPipelines        1
  101.87     0.00  tid+0    attach                           32 buffer
  101.87     0.00  tid+0    commit-buffer                    32 toplevel
  101.92     1.39  tid+3    poll                             3 ppoll
  102.22     0.00  tid+0    commit                           32 toplevel
  102.45     0.88  tid+0    poll                             2 ppoll
  102.27           sway: window new
```

### adw, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.78     0.00  tid+0    ctor                             hello-adw-1/bin/hello-adw
    0.84     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-adw
   16.49     0.00  tid+0    ctor                             hello-adw-1/bin/.hello-adw-wrapped
   16.51     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-adw
   16.77     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   16.81     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   17.10     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   17.56     0.00  tid+0    g_bus_get_sync                   2
   17.56     0.12  tid+0    g_dbus_connection_call_sync      org.freedesktop.DBus org.freedesktop.DBus.ListActivatableNames ok
   17.69     0.00  tid+0    g_bus_get_sync                   2
   17.70     0.13  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   17.83     0.00  tid+0    g_bus_get_sync                   2
   17.84     0.07  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Documents org.freedesktop.portal.FileTransfer
   17.91     0.87  tid+3    poll                             2 ppoll
   17.96     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   18.00     4.24  tid+4    FcInit                           
   18.15     0.45  tid+0    dlopen                           /run/current-system/sw/lib/gtk-4.0/4.0.0/immodules/libim-ibus.so
   18.69     0.02  tid+0    wl_display_connect               
   18.74     0.01  tid+0    xkb_context_new                  
   18.75     0.00  tid+0    g_bus_get_sync                   2
   18.76     0.08  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   18.87     0.22  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   19.10     1.11  tid+0    poll                             1 ppoll
   19.12     1.01  tid+3    poll                             2 ppoll
   20.22     1.48  tid+3    poll                             2 ppoll
   20.30     0.11  tid+0    wl_display_roundtrip             
   20.30     0.10  tid+0    wl_display_roundtrip             queue
   20.42     0.00  tid+0    xkb_context_new                  
   21.67     0.00  tid+0    g_bus_get_sync                   2
   21.68     0.13  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   21.81     0.20  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Inhibit
   22.33     0.40  tid+0    dlopen                           gvfs-1.60.3/lib/gio/modules/libgvfsdbus.so
   22.75     0.00  tid+0    g_bus_get_sync                   2
   23.12     9.26  tid+3    poll                             2 ppoll
   29.80     0.00  tid+1    pthread_create                   libglib-2.0.so.0 ?
   32.34     0.25  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   33.16     0.10  tid+0    dlopen                           dconf-0.49.0-lib/lib/gio/modules/libdconfsettings.so
   33.33     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   33.38     0.00  tid+6    g_bus_get_sync                   2
   33.43    15.51  tid+3    poll                             2 ppoll
   48.91     0.00  tid+0    g_bus_get_sync                   2
   48.92     0.17  tid+0    g_dbus_connection_call_sync      org.a11y.Bus org.a11y.Bus.GetAddress ok
   49.08    75.29  tid+3    poll                             2 ppoll
   49.87    40.51  tid+0    vkEnumerateInstanceExtensionProperties 
   50.01     0.71  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   50.76    10.94  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   61.74     0.42  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   62.18    10.04  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
   72.27     1.61  tid+0    dlopen                           mesa-26.2.6/lib/libEGL_mesa.so.0
   75.19     0.40  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
   75.62     0.43  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
   76.07     0.67  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
   76.76     0.38  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
   77.16     0.37  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
   77.56     0.36  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
   77.96     0.63  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
   78.62     0.50  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
   79.16     1.53  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
   80.71     0.23  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
   81.02     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   81.03     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   81.04     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   81.04     7.88  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
   90.00     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
   90.01     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
   90.02     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
   90.03     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
   90.04     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
   90.05     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
   90.06     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
   90.07     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
   90.09     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
   90.09     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
   90.37     9.40  tid+0    vkEnumerateInstanceExtensionProperties 
   90.45     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   90.45     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   90.46     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   90.47     7.99  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
   99.45     0.01  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
   99.47     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
   99.48     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
   99.49     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
   99.51     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
   99.52     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
   99.52     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
   99.53     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
   99.56     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
   99.56     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
   99.78    16.32  tid+0    vkCreateInstance                 0
   99.89     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   99.90     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   99.91     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   99.92     7.77  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
  108.68     0.01  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
  108.70     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
  108.71     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
  108.72     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
  108.73     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
  108.74     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
  108.75     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
  108.76     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
  108.78     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
  108.79     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
  108.97     0.11  tid+0    dlopen                           mesa-26.2.6/lib/libVkLayer_MESA_device_select.so
  116.17     5.07  tid+0    vkEnumeratePhysicalDevices       2
  120.02     0.00  tid+0    pthread_create                   libvulkan_radeon.so ?
  120.09     0.00  tid+0    pthread_create                   libvulkan_radeon.so ?
  121.01     0.01  tid+0    wl_display_connect               
  121.09     0.14  tid+0    wl_display_roundtrip             
  121.09     0.14  tid+0    wl_display_roundtrip             queue
  121.24     0.98  tid+0    vkEnumeratePhysicalDevices       2
  121.26     0.01  tid+0    wl_display_connect               
  121.29     0.11  tid+0    wl_display_roundtrip             
  121.29     0.11  tid+0    wl_display_roundtrip             queue
  122.25     1.02  tid+0    vkCreateDevice                   0
  123.40     0.05  tid+0    wl_display_roundtrip             queue
  123.45     0.03  tid+0    wl_display_roundtrip             queue
  123.48     0.01  tid+0    wl_display_roundtrip             queue
  123.49     0.03  tid+0    wl_display_roundtrip             queue
  123.52     0.02  tid+0    wl_display_roundtrip             queue
  123.54     0.01  tid+0    wl_display_roundtrip             queue
  123.56     0.02  tid+0    wl_display_roundtrip             queue
  123.58     0.01  tid+0    wl_display_roundtrip             queue
  123.60     0.01  tid+0    wl_display_roundtrip             queue
  123.61     0.04  tid+0    wl_display_roundtrip             queue
  123.65     0.02  tid+0    wl_display_roundtrip             queue
  123.67     0.01  tid+0    wl_display_roundtrip             queue
  123.69     0.05  tid+0    wl_display_roundtrip             queue
  123.74     0.03  tid+0    wl_display_roundtrip             queue
  123.78     0.01  tid+0    wl_display_roundtrip             queue
  123.80     0.17  tid+0    wl_display_roundtrip             queue
  123.97     0.04  tid+0    wl_display_roundtrip             queue
  124.02     0.01  tid+0    wl_display_roundtrip             queue
  124.03     0.02  tid+0    wl_display_roundtrip             queue
  124.26     0.19  tid+0    g_dbus_connection_new_for_address_sync ok
  124.48     0.00  tid+0    xdg_toplevel                     63
  124.48     0.00  tid+0    commit                           32 toplevel
  124.58     5.49  tid+3    poll                             3 ppoll
  129.69     0.04  tid+0    wl_display_roundtrip             queue
  129.74     0.03  tid+0    wl_display_roundtrip             queue
  129.77     0.01  tid+0    wl_display_roundtrip             queue
  129.78     0.03  tid+0    wl_display_roundtrip             queue
  129.81     0.02  tid+0    wl_display_roundtrip             queue
  129.83     0.01  tid+0    wl_display_roundtrip             queue
  129.84     0.01  tid+0    wl_display_roundtrip             queue
  130.09     1.38  tid+3    poll                             3 ppoll
  131.11     0.04  tid+0    vkCreateGraphicsPipelines        1
  131.19     0.03  tid+0    vkCreateGraphicsPipelines        1
  131.24     0.02  tid+0    vkCreateGraphicsPipelines        1
  131.30     0.02  tid+0    vkCreateGraphicsPipelines        1
  131.42     0.00  tid+0    attach                           32 buffer
  131.42     0.00  tid+0    commit-buffer                    32 toplevel
  131.48     1.36  tid+3    poll                             3 ppoll
  131.82     0.00  tid+0    commit                           32 toplevel
  132.04     0.86  tid+0    poll                             2 ppoll
  131.87           sway: window new
```

### qtwidgets, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.52     0.00  tid+0    ctor                             hello-qtwidgets-1/bin/hello-qtwidgets
    0.58     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-qtwidgets
    6.32     0.00  tid+0    ctor                             hello-qtwidgets-1/bin/.hello-qtwidgets-wrapped
    6.34     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-qtwidgets
    6.78     0.00  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforms/libqwayland.so.avx2 FAILED
    6.79     0.61  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforms/libqwayland.so
    7.42     0.24  tid+0    wl_display_connect               
    7.68     0.00  tid+0    xkb_context_new                  
    7.79     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
    7.92     0.25  tid+1    dbus_bus_get_private             0
    7.92     0.03  tid+1    dbus_connection_open_private     
    7.96     0.20  tid+1    dbus_call_block                  
    8.29     0.15  tid+0    wl_display_roundtrip             
    8.32     0.12  tid+0    wl_display_roundtrip             queue
    8.35     7.02  tid+1    poll                             2 ppoll
    8.44     0.07  tid+0    wl_display_roundtrip             
    8.44     0.07  tid+0    wl_display_roundtrip             queue
    8.64     0.00  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-shell-integration/libxdg-shell.so.avx2 FAILED
    8.65     0.18  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-shell-integration/libxdg-shell.so
    8.92     0.00  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platformthemes/libqgtk3.so.avx2 FAILED
    8.93     6.41  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platformthemes/libqgtk3.so
   15.41     0.96  tid+1    poll                             2 ppoll
   15.55     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   15.60     4.18  tid+2    FcInit                           
   15.62     0.01  tid+0    wl_display_connect               
   15.65     0.00  tid+0    xkb_context_new                  
   15.70     0.90  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   15.86     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   15.89     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   16.19     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   16.38    16.42  tid+1    poll                             2 ppoll
   16.61     0.83  tid+0    poll                             1 ppoll
   16.63     0.72  tid+5    poll                             2 ppoll
   17.44     4.91  tid+5    poll                             2 ppoll
   17.54     0.08  tid+0    wl_display_roundtrip             
   17.54     0.08  tid+0    wl_display_roundtrip             queue
   20.17     0.00  tid+0    xkb_context_new                  
   21.84     0.15  tid+0    dbus_call_block                  
   22.00     0.14  tid+0    dbus_call_block                  
   22.14     0.02  tid+0    dbus_connection_open_private     
   22.16     0.06  tid+0    dbus_call_block                  
   22.24     0.08  tid+0    g_bus_get_sync                   2
   22.41     0.93  tid+5    poll                             2 ppoll
   22.84     0.42  tid+0    dlopen                           gvfs-1.60.3/lib/gio/modules/libgvfsdbus.so
   23.29     0.00  tid+0    g_bus_get_sync                   2
   32.85     0.51  tid+1    poll                             2 ppoll
   32.92     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   32.99     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   33.04    13.44  tid+7    poll                             2 -1
   33.11     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libibusplatforminputcontextplugin.so.avx2 FAILED
   33.12     0.17  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libibusplatforminputcontextplugin.so
   33.40     5.81  tid+1    poll                             2 ppoll
   33.42     0.00  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libcomposeplatforminputcontextplugin.so.avx2 FAILED
   33.43     0.09  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libcomposeplatforminputcontextplugin.so
   39.62     6.86  tid+1    poll                             2 ppoll
   39.77     0.00  tid+0    FcInit                           
   39.78     2.22  tid+0    FcFontList                       1506
   46.36     0.00  tid+0    xdg_toplevel                     29
   46.37     0.00  tid+0    commit                           25 toplevel
   46.49     0.96  tid+7    poll                             2 -1
   46.93     0.00  tid+0    attach                           25 buffer
   46.94     0.00  tid+0    commit-buffer                    25 toplevel
   47.15     0.00  tid+0    attach                           25 buffer
   47.15     0.00  tid+0    commit-buffer                    25 toplevel
   47.59     0.00  tid+0    attach                           25 buffer
   47.59     0.00  tid+0    commit-buffer                    25 toplevel
   47.62     0.00  tid+0    attach                           25 buffer
   47.62     0.00  tid+0    commit-buffer                    25 toplevel
   47.68     0.00  tid+0    attach                           25 buffer
   47.69     0.00  tid+0    commit-buffer                    25 toplevel
   47.86     0.00  tid+0    attach                           25 buffer
   47.86     0.00  tid+0    commit-buffer                    25 toplevel
   48.36     0.00  tid+0    attach                           25 buffer
   48.36     0.00  tid+0    commit-buffer                    25 toplevel
   47.62           sway: window new
```

### qtquick, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.49     0.00  tid+0    ctor                             hello-qtquick-1/bin/hello-qtquick
    0.58     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-qtquick
   10.46     0.00  tid+0    ctor                             hello-qtquick-1/bin/.hello-qtquick-wrapped
   10.48     0.00  tid+0    main                             /tmp/claude-1000/perf/design-hello/bin/hello-qtquick
   10.96     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforms/libqwayland.so.avx2 FAILED
   10.97     0.82  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforms/libqwayland.so
   11.81     0.95  tid+0    wl_display_connect               
   12.81     0.00  tid+0    xkb_context_new                  
   12.92     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   13.02     0.26  tid+1    dbus_bus_get_private             0
   13.03     0.04  tid+1    dbus_connection_open_private     
   13.07     0.21  tid+1    dbus_call_block                  
   13.40     0.21  tid+0    wl_display_roundtrip             
   13.44     7.37  tid+1    poll                             2 ppoll
   13.46     0.15  tid+0    wl_display_roundtrip             queue
   13.61     0.06  tid+0    wl_display_roundtrip             
   13.61     0.06  tid+0    wl_display_roundtrip             queue
   13.80     0.00  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-shell-integration/libxdg-shell.so.avx2 FAILED
   13.81     0.23  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-shell-integration/libxdg-shell.so
   14.11     0.00  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platformthemes/libqgtk3.so.avx2 FAILED
   14.12     6.64  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platformthemes/libqgtk3.so
   20.85     0.87  tid+1    poll                             2 ppoll
   20.99     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   21.05     4.07  tid+2    FcInit                           
   21.06     0.01  tid+0    wl_display_connect               
   21.09     0.00  tid+0    xkb_context_new                  
   21.14     0.75  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   21.18     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   21.20     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   21.47     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   21.75    15.86  tid+1    poll                             2 ppoll
   21.91     0.77  tid+0    poll                             1 ppoll
   21.91     0.68  tid+5    poll                             2 ppoll
   22.68     4.69  tid+5    poll                             2 ppoll
   22.79     0.08  tid+0    wl_display_roundtrip             
   22.79     0.08  tid+0    wl_display_roundtrip             queue
   25.27     0.00  tid+0    xkb_context_new                  
   26.93     0.15  tid+0    dbus_call_block                  
   27.09     0.14  tid+0    dbus_call_block                  
   27.23     0.01  tid+0    dbus_connection_open_private     
   27.24     0.08  tid+0    dbus_call_block                  
   27.34     0.00  tid+0    g_bus_get_sync                   2
   27.42     0.83  tid+5    poll                             2 ppoll
   27.74     0.43  tid+0    dlopen                           gvfs-1.60.3/lib/gio/modules/libgvfsdbus.so
   28.20     0.00  tid+0    g_bus_get_sync                   2
   37.65     0.54  tid+1    poll                             2 ppoll
   37.73     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   37.81     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   37.86    27.86  tid+7    poll                             2 -1
   37.91     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libibusplatforminputcontextplugin.so.avx2 FAILED
   37.93     0.21  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libibusplatforminputcontextplugin.so
   38.22    35.94  tid+1    poll                             2 ppoll
   38.28     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libcomposeplatforminputcontextplugin.so.avx2 FAILED
   38.29     0.10  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libcomposeplatforminputcontextplugin.so
   43.52     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   46.36     0.01  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Controls/libqtquickcontrols2plugin.so.avx2 FAILED
   46.38     1.29  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Controls/libqtquickcontrols2plugin.so
   47.88     0.01  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Controls/Fusion/libqtquickcontrols2fusionstyleplugin.so.avx2 FAILED
   47.89     0.54  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Controls/Fusion/libqtquickcontrols2fusionstyleplugin.so
   48.75     0.01  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Controls/Basic/libqtquickcontrols2basicstyleplugin.so.avx2 FAILED
   48.76     0.18  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Controls/Basic/libqtquickcontrols2basicstyleplugin.so
   49.06     0.00  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Layouts/libqquicklayoutsplugin.so.avx2 FAILED
   49.07     0.26  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Layouts/libqquicklayoutsplugin.so
   49.70     0.01  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Templates/libqtquicktemplates2plugin.so.avx2 FAILED
   49.71     0.14  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Templates/libqtquicktemplates2plugin.so
   53.10     0.01  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Window/libquickwindowplugin.so.avx2 FAILED
   53.12     0.12  tid+8    dlopen                           qtdeclarative-6.12.0/lib/qt-6/qml/QtQuick/Window/libquickwindowplugin.so
   54.53    20.38  tid+8    poll                             1 ppoll
   54.80     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-graphics-integration-client/libqt-plugin-wayland-egl.so.avx2 FAILED
   54.82     0.26  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-graphics-integration-client/libqt-plugin-wayland-egl.so
   55.08    10.40  tid+0    eglGetPlatformDisplay            0x31d8
   55.10    10.35  tid+0    dlopen                           mesa-26.2.6/lib/libEGL_mesa.so.0
   65.48     8.56  tid+0    eglInitialize                    1
   65.64     0.11  tid+0    wl_display_roundtrip             queue
   65.75     0.19  tid+0    wl_display_roundtrip             queue
   65.78    15.98  tid+7    poll                             2 -1
   72.24     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   72.69     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   73.03     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   73.06     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   74.63     7.11  tid+1    poll                             2 ppoll
   75.29     0.00  tid+0    FcInit                           
   75.30     2.37  tid+0    FcFontList                       1506
   81.62     0.00  tid+0    xdg_toplevel                     34
   81.64     0.00  tid+0    commit                           30 toplevel
   81.77     4.08  tid+7    poll                             2 -1
   81.86     1.07  tid+0    eglCreateContext                 ok
   82.11     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   82.28     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   82.87     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   82.95     0.27  tid+0    eglMakeCurrent                   
   83.23     0.00  tid+0    eglMakeCurrent                   
   83.24     0.46  tid+0    eglCreateContext                 ok
   83.32     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   83.35     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   83.66     0.00  tid+0    pthread_create                   libgallium-26.2.6.so ?
   84.40     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   84.48     0.78  tid+21   eglCreateContext                 ok
   84.58     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   84.61     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   85.21     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   85.26     0.00  tid+21   eglMakeCurrent                   
   85.28     0.00  tid+21   eglMakeCurrent                   
   85.28     0.50  tid+21   eglCreateContext                 ok
   85.35     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   85.37     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   85.73     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   85.82     0.04  tid+21   wl_display_roundtrip             queue
   85.85     7.95  tid+7    poll                             2 -1
   85.86     0.74  tid+21   eglMakeCurrent                   
   85.95     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   93.42     0.00  tid+21   pthread_create                   libgallium-26.2.6.so ?
   93.47     0.00  tid+21   attach                           30 buffer
   93.47     0.00  tid+21   commit-buffer                    30 toplevel
   93.85     0.00  tid+21   eglMakeCurrent                   
   93.99     0.00  tid+21   attach                           30 buffer
   93.99     0.00  tid+21   commit-buffer                    30 toplevel
   94.02     0.00  tid+21   eglMakeCurrent                   
   94.09     0.00  tid+21   attach                           30 buffer
   94.10     0.00  tid+21   commit-buffer                    30 toplevel
   94.10     0.00  tid+21   eglMakeCurrent                   
   94.24     0.00  tid+21   attach                           30 buffer
   94.24     0.00  tid+21   commit-buffer                    30 toplevel
   94.29     0.00  tid+21   attach                           30 buffer
   94.29     0.00  tid+21   commit-buffer                    30 toplevel
   94.43     0.00  tid+21   attach                           30 buffer
   94.43     0.00  tid+21   commit-buffer                    30 toplevel
   93.89           sway: window new
```

### sumi, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.87     0.00  tid+0    ctor                             aizome-0.1.0/bin/aizome-sumi
    0.93     0.00  tid+0    main                             /tmp/claude-1000/perf/design-aizome/bin/aizome-sumi
    1.04     0.00  tid+0    pthread_create                   aizome-sumi ?
    6.91     0.86  tid+0    FcFontSort                       161
    8.33     0.81  tid+0    FcFontSort                       188
    9.72     0.79  tid+0    FcFontSort                       149
   11.10     0.83  tid+0    FcFontSort                       144
   12.43     0.81  tid+0    FcFontSort                       143
   13.72     0.99  tid+0    FcFontSort                       144
   15.20     0.86  tid+0    FcFontSort                       143
   16.55     0.80  tid+0    FcFontSort                       143
   17.86     0.46  tid+0    FcFontSort                       1623
   18.97     0.00  tid+0    pthread_create                   aizome-sumi ?
   19.01     0.00  tid+0    pthread_create                   aizome-sumi ?
   19.03     0.00  tid+0    pthread_create                   aizome-sumi ?
   19.06     0.00  tid+0    pthread_create                   aizome-sumi ?
   19.07     0.00  tid+3    pthread_create                   aizome-sumi ?
   19.21     0.00  tid+0    pthread_create                   aizome-sumi ?
   20.45           sway: window new
```

### gnome-text-editor, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.51     0.00  tid+0    ctor                             gnome-text-editor-50.1/bin/gnome-text-editor
    0.57     0.00  tid+0    main                             gnome-text-editor-50.1/bin/gnome-text-editor
   18.66     0.00  tid+0    ctor                             gnome-text-editor-50.1/bin/.gnome-text-editor-wrapped
   18.68     0.00  tid+0    main                             gnome-text-editor-50.1/bin/gnome-text-editor
   19.07     0.10  tid+0    dlopen                           dconf-0.49.0-lib/lib/gio/modules/libdconfsettings.so
   19.31     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   19.37     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   19.43    49.98  tid+2    poll                             1 ppoll
   19.44     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   19.49     1.32  tid+3    g_bus_get_sync                   2
   19.51     0.62  tid+0    dlopen                           gvfs-1.60.3/lib/gio/modules/libgvfsdbus.so
   20.17     0.64  tid+0    g_bus_get_sync                   2
   20.49     0.00  tid+3    pthread_create                   libglib-2.0.so.0 ?
   21.30    14.57  tid+3    poll                             1 ppoll
   21.61     0.00  tid+0    g_bus_get_sync                   2
   21.62     0.09  tid+0    g_dbus_connection_call_sync      org.freedesktop.DBus org.freedesktop.DBus.ListActivatableNames ok
   21.71     0.00  tid+0    g_bus_get_sync                   2
   21.72     0.09  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   21.82     0.00  tid+0    g_bus_get_sync                   2
   21.83     0.05  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Documents org.freedesktop.portal.FileTransfer
   21.87     0.86  tid+4    poll                             2 ppoll
   21.92     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   21.97     4.13  tid+5    FcInit                           
   22.13     0.44  tid+0    dlopen                           /run/current-system/sw/lib/gtk-4.0/4.0.0/immodules/libim-ibus.so
   22.65     0.01  tid+0    wl_display_connect               
   22.69     0.01  tid+0    xkb_context_new                  
   22.71     0.00  tid+0    g_bus_get_sync                   2
   22.71     0.10  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   22.81     0.18  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   23.00     0.84  tid+0    poll                             1 ppoll
   23.00     0.77  tid+4    poll                             2 ppoll
   23.84     1.51  tid+4    poll                             2 ppoll
   23.92     0.13  tid+0    wl_display_roundtrip             
   23.92     0.12  tid+0    wl_display_roundtrip             queue
   24.06     0.00  tid+0    xkb_context_new                  
   25.32     0.00  tid+0    g_bus_get_sync                   2
   25.32     0.09  tid+0    g_dbus_connection_call_sync      org.freedesktop.portal.Desktop org.freedesktop.DBus.Properties.Get ok
   25.42     0.15  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Inhibit
   26.21     8.96  tid+4    poll                             2 ppoll
   32.36     0.00  tid+1    pthread_create                   libglib-2.0.so.0 ?
   35.13     0.23  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   35.92    15.90  tid+4    poll                             2 ppoll
   51.45     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   51.52     0.00  tid+7    pthread_create                   libglib-2.0.so.0 ?
   51.69     0.00  tid+7    pthread_create                   libglib-2.0.so.0 ?
   51.78     0.25  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   52.05     0.94  tid+0    poll                             1 ppoll
   52.06     0.87  tid+4    poll                             2 ppoll
   52.99     3.10  tid+4    poll                             2 ppoll
   56.06     0.00  tid+0    g_bus_get_sync                   2
   56.07     0.17  tid+0    g_dbus_connection_call_sync      org.a11y.Bus org.a11y.Bus.GetAddress ok
   56.23    98.65  tid+4    poll                             2 ppoll
   66.81     0.34  tid+0    dlopen                           enchant-2.6.9/lib/enchant-2/enchant_nuspell.so
   67.17     0.06  tid+0    dlopen                           enchant-2.6.9/lib/enchant-2/enchant_aspell.so
   67.23     0.06  tid+0    dlopen                           enchant-2.6.9/lib/enchant-2/enchant_hspell.so
   67.29     0.81  tid+0    dlopen                           enchant-2.6.9/lib/enchant-2/enchant_voikko.so
   68.11     0.46  tid+0    dlopen                           enchant-2.6.9/lib/enchant-2/enchant_hunspell.so
   69.42    88.99  tid+2    poll                             2 ppoll
   73.75     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   77.89     0.00  tid+1    pthread_create                   libglib-2.0.so.0 ?
   77.93     0.00  tid+1    pthread_create                   libglib-2.0.so.0 ?
   80.23    41.94  tid+0    vkEnumerateInstanceExtensionProperties 
   80.39     0.75  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
   81.16    11.07  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
   92.28     0.42  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
   92.72    10.52  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
  103.29     1.82  tid+0    dlopen                           mesa-26.2.6/lib/libEGL_mesa.so.0
  106.55     0.42  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
  106.99     0.42  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
  107.44     0.59  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
  108.05     0.34  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
  108.41     0.33  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
  108.76     0.36  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
  109.14     0.53  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
  109.68     0.48  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
  110.20     1.65  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
  111.87     0.25  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
  112.18     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
  112.19     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
  112.20     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
  112.21     8.52  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
  121.77     0.01  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
  121.79     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
  121.80     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
  121.81     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
  121.82     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
  121.83     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
  121.84     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
  121.85     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
  121.87     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
  121.88     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
  122.17     9.44  tid+0    vkEnumerateInstanceExtensionProperties 
  122.25     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
  122.26     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
  122.27     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
  122.28     7.96  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
  131.24     0.01  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
  131.26     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
  131.27     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
  131.28     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
  131.29     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
  131.30     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
  131.31     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
  131.32     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
  131.35     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
  131.36     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
  131.62    15.25  tid+0    vkCreateInstance                 0
  131.74     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_virtio.so
  131.76     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_lvp.so
  131.76     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_powervr_mesa.so
  131.77     7.39  tid+0    dlopen                           nvidia-x11-615.78.08/lib/libGLX_nvidia.so.0
  140.19     0.01  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_panfrost.so
  140.21     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel.so
  140.22     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_radeon.so
  140.23     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_dzn.so
  140.24     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_broadcom.so
  140.25     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_intel_hasvk.so
  140.26     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_asahi.so
  140.27     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_nouveau.so
  140.30     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_freedreno.so
  140.32     0.00  tid+0    dlopen                           mesa-26.2.6/lib/libvulkan_gfxstream.so
  140.49     0.10  tid+0    dlopen                           mesa-26.2.6/lib/libVkLayer_MESA_device_select.so
  146.90     4.69  tid+0    vkEnumeratePhysicalDevices       2
  150.36     0.00  tid+0    pthread_create                   libvulkan_radeon.so ?
  150.42     0.00  tid+0    pthread_create                   libvulkan_radeon.so ?
  151.33     0.02  tid+0    wl_display_connect               
  151.42     0.16  tid+0    wl_display_roundtrip             
  151.42     0.16  tid+0    wl_display_roundtrip             queue
  151.59     0.95  tid+0    vkEnumeratePhysicalDevices       2
  151.61     0.01  tid+0    wl_display_connect               
  151.65     0.13  tid+0    wl_display_roundtrip             
  151.65     0.12  tid+0    wl_display_roundtrip             queue
  152.56     1.00  tid+0    vkCreateDevice                   0
  153.72     0.05  tid+0    wl_display_roundtrip             queue
  153.77     0.03  tid+0    wl_display_roundtrip             queue
  153.80     0.01  tid+0    wl_display_roundtrip             queue
  153.81     0.03  tid+0    wl_display_roundtrip             queue
  153.84     0.02  tid+0    wl_display_roundtrip             queue
  153.85     0.00  tid+0    wl_display_roundtrip             queue
  153.87     0.02  tid+0    wl_display_roundtrip             queue
  153.90     0.01  tid+0    wl_display_roundtrip             queue
  153.91     0.00  tid+0    wl_display_roundtrip             queue
  153.92     0.02  tid+0    wl_display_roundtrip             queue
  153.94     0.01  tid+0    wl_display_roundtrip             queue
  153.96     0.01  tid+0    wl_display_roundtrip             queue
  153.96     0.03  tid+0    wl_display_roundtrip             queue
  153.99     0.01  tid+0    wl_display_roundtrip             queue
  154.00     0.01  tid+0    wl_display_roundtrip             queue
  154.01     0.02  tid+0    wl_display_roundtrip             queue
  154.04     0.01  tid+0    wl_display_roundtrip             queue
  154.05     0.01  tid+0    wl_display_roundtrip             queue
  154.06     0.02  tid+0    wl_display_roundtrip             queue
  154.77     0.20  tid+0    g_dbus_connection_new_for_address_sync ok
  154.98     2.77  tid+4    poll                             3 ppoll
  154.99     0.00  tid+0    xdg_toplevel                     63
  155.00     0.00  tid+0    commit                           32 toplevel
  158.21    11.01  tid+4    poll                             3 ppoll
  166.21     0.07  tid+0    wl_display_roundtrip             queue
  166.29     0.05  tid+0    wl_display_roundtrip             queue
  166.34     0.01  tid+0    wl_display_roundtrip             queue
  166.36     0.03  tid+0    wl_display_roundtrip             queue
  166.39     0.02  tid+0    wl_display_roundtrip             queue
  166.41     0.01  tid+0    wl_display_roundtrip             queue
  166.43     0.01  tid+0    wl_display_roundtrip             queue
  169.24     3.03  tid+4    poll                             3 ppoll
  170.70     0.05  tid+0    vkCreateGraphicsPipelines        1
  170.78     0.03  tid+0    vkCreateGraphicsPipelines        1
  170.88     0.02  tid+0    vkCreateGraphicsPipelines        1
  170.91     0.02  tid+0    vkCreateGraphicsPipelines        1
  170.93     0.03  tid+0    vkCreateGraphicsPipelines        1
  170.97     0.02  tid+0    vkCreateGraphicsPipelines        1
  171.04     0.02  tid+0    vkCreateGraphicsPipelines        1
  171.06     0.02  tid+0    vkCreateGraphicsPipelines        1
  171.20     0.00  tid+0    attach                           32 buffer
  171.20     0.00  tid+0    commit-buffer                    32 toplevel
  172.19     0.00  tid+0    attach                           32 buffer
  172.19     0.00  tid+0    commit-buffer                    32 toplevel
  172.63     0.53  tid+0    poll                             1 ppoll
  171.68           sway: window new
```

### kwrite, run 2: every stamp (ms from the fork, duration ms, thread, event, detail; polls under 0.5 ms left out)

```
    0.49     0.00  tid+0    ctor                             kate-26.08.2/bin/kwrite
    0.55     0.00  tid+0    main                             kate-26.08.2/bin/kwrite
   26.79     0.00  tid+0    ctor                             kate-26.08.2/bin/.kwrite-wrapped
   26.81     0.00  tid+0    main                             kate-26.08.2/bin/kwrite
   27.37     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforms/libqwayland.so.avx2 FAILED
   27.38     0.24  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforms/libqwayland.so
   27.64     0.02  tid+0    wl_display_connect               
   27.67     0.01  tid+0    xkb_context_new                  
   27.78     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   27.88     0.28  tid+1    dbus_bus_get_private             0
   27.89     0.04  tid+1    dbus_connection_open_private     
   27.93     0.23  tid+1    dbus_call_block                  
   28.29     0.12  tid+0    wl_display_roundtrip             
   28.29     0.12  tid+0    wl_display_roundtrip             queue
   28.33    11.22  tid+1    poll                             2 ppoll
   28.42     0.06  tid+0    wl_display_roundtrip             
   28.42     0.06  tid+0    wl_display_roundtrip             queue
   28.61     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-shell-integration/libxdg-shell.so.avx2 FAILED
   28.62     0.45  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/wayland-shell-integration/libxdg-shell.so
   29.22     0.00  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platformthemes/libqgtk3.so.avx2 FAILED
   29.22    10.28  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platformthemes/libqgtk3.so
   39.59     0.94  tid+1    poll                             2 ppoll
   39.75     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   39.81     4.06  tid+2    FcInit                           
   39.83     0.02  tid+0    wl_display_connect               
   39.86     0.00  tid+0    xkb_context_new                  
   39.92     0.84  tid+0    g_dbus_proxy_new_sync            org.freedesktop.portal.Desktop org.freedesktop.portal.Settings
   39.97     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   40.01     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   40.34     0.00  tid+0    pthread_create                   libglib-2.0.so.0 ?
   40.56    16.38  tid+1    poll                             2 ppoll
   40.78     0.78  tid+0    poll                             1 ppoll
   40.78     0.70  tid+5    poll                             2 ppoll
   41.55     4.79  tid+5    poll                             2 ppoll
   41.65     0.09  tid+0    wl_display_roundtrip             
   41.65     0.08  tid+0    wl_display_roundtrip             queue
   44.11     0.00  tid+0    xkb_context_new                  
   45.95     0.11  tid+0    dbus_call_block                  
   46.07     0.15  tid+0    dbus_call_block                  
   46.22     0.01  tid+0    dbus_connection_open_private     
   46.24     0.06  tid+0    dbus_call_block                  
   46.32     0.01  tid+0    g_bus_get_sync                   2
   46.37     1.21  tid+5    poll                             2 ppoll
   46.84     0.66  tid+0    dlopen                           gvfs-1.60.3/lib/gio/modules/libgvfsdbus.so
   47.53     0.00  tid+0    g_bus_get_sync                   2
   57.07     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   57.12     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
   57.17    49.02  tid+7    poll                             2 -1
   57.25     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libibusplatforminputcontextplugin.so.avx2 FAILED
   57.26     0.30  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libibusplatforminputcontextplugin.so
   57.48     8.12  tid+1    poll                             2 ppoll
   57.68     0.01  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libcomposeplatforminputcontextplugin.so.avx2 FAILED
   57.70     0.18  tid+0    dlopen                           qtbase-6.12.0/lib/qt-6/plugins/platforminputcontexts/libcomposeplatforminputcontextplugin.so
   65.29     0.01  tid+0    dlopen                           kiconthemes-6.31.0/lib/qt-6/plugins/kiconthemes6/iconengines/KIconEnginePlugin.so.avx2 FAILED
   65.31     0.25  tid+0    dlopen                           kiconthemes-6.31.0/lib/qt-6/plugins/kiconthemes6/iconengines/KIconEnginePlugin.so
   65.62    24.62  tid+1    poll                             2 ppoll
   73.92     0.00  tid+0    FcInit                           
   73.93     2.15  tid+0    FcFontList                       1506
   79.11     0.79  tid+0    FcFontSort                       1623
   91.20     0.80  tid+1    poll                             2 ppoll
   92.03     2.08  tid+1    poll                             2 ppoll
   94.28    11.70  tid+1    poll                             2 ppoll
   95.46     0.00  tid+0    xdg_toplevel                     29
   95.48     0.00  tid+0    commit                           25 toplevel
  100.45     0.02  tid+0    dlopen                           sonnet-6.31.0/lib/qt-6/plugins/kf6/sonnet/sonnet_aspell.so.avx2 FAILED
  100.47     1.06  tid+0    dlopen                           sonnet-6.31.0/lib/qt-6/plugins/kf6/sonnet/sonnet_aspell.so
  101.64     0.01  tid+0    dlopen                           sonnet-6.31.0/lib/qt-6/plugins/kf6/sonnet/sonnet_hunspell.so.avx2 FAILED
  101.65     0.62  tid+0    dlopen                           sonnet-6.31.0/lib/qt-6/plugins/kf6/sonnet/sonnet_hunspell.so
  106.20     4.61  tid+7    poll                             2 -1
  106.62    16.19  tid+1    poll                             2 ppoll
  107.34     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
  107.40     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
  107.43     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
  107.46     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
  107.66     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
  107.68     0.00  tid+0    pthread_create                   libQt6Core.so.6 ?
  108.39     0.01  tid+0    dlopen                           qtsvg-6.12.0/lib/qt-6/plugins/imageformats/libqsvg.so.avx2 FAILED
  108.40     0.23  tid+0    dlopen                           qtsvg-6.12.0/lib/qt-6/plugins/imageformats/libqsvg.so
  109.53     0.00  tid+0    attach                           25 buffer
  109.54     0.00  tid+0    commit-buffer                    25 toplevel
  109.55     1.27  tid+6    poll                             2 -1
  110.84     1.83  tid+7    poll                             2 -1
  111.39     0.00  tid+0    attach                           25 buffer
  111.40     0.00  tid+0    commit-buffer                    25 toplevel
  111.81     0.00  tid+0    attach                           25 buffer
  111.81     0.00  tid+0    commit-buffer                    25 toplevel
  111.83     0.84  tid+6    poll                             2 -1
  112.69     0.51  tid+7    poll                             2 -1
  112.78     0.00  tid+0    attach                           25 buffer
  112.78     0.00  tid+0    commit-buffer                    25 toplevel
  112.81     0.00  tid+0    attach                           25 buffer
  112.82     0.00  tid+0    commit-buffer                    25 toplevel
  113.22     0.56  tid+7    poll                             2 -1
  113.58     0.00  tid+0    attach                           25 buffer
  113.58     0.00  tid+0    commit-buffer                    25 toplevel
  113.78    14.28  tid+7    poll                             2 -1
  122.80     5.21  tid+6    poll                             2 -1
  122.86     1.51  tid+1    poll                             2 ppoll
  124.33     0.00  tid+0    attach                           25 null
  124.33     0.00  tid+0    commit                           25 toplevel
  125.31     1.72  tid+1    poll                             2 ppoll
  127.04     0.66  tid+1    poll                             2 ppoll
  127.99     0.90  tid+1    poll                             2 ppoll
  110.91           sway: window new
```
