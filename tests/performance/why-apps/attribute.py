#!/usr/bin/env python3
"""Attribute a program's CPU, from launch to its first frame, to what it
was for (purpose) and what did it (mechanism), from probe.py's perf runs.

    attribute.py RUNDIR NAME [NAME...] > summary.txt
    attribute.py RUNDIR NAME --folded OUT.folded

RUNDIR is probe.py's output for the `perf` set: runs.jsonl, NAME-REP.perf
(cpu-clock:u, a sample every 100 us of CPU, DWARF call chains, CLOCK_MONOTONIC
stamps) and NAME-REP.stamps. Only samples from exec of the program to
sway's window `new` event count. Each sample is given:

- a purpose: walking its call chain from the leaf towards the root, the
  first frame that one of PURPOSE's rules recognises (allocation, string
  and container helpers, and the dynamic linker under dlopen are passed
  over, so a sample is charged to what asked for the work); samples before
  main() are "exec and dynamic linking" or "library constructors";
- a mechanism: the kind of code the leaf frame is in (MECHANISM, by
  library).

The sampled CPU is user time only (perf_event_paranoid=2 hides the kernel);
the kernel's share is each thread's schedstat run time at the first frame
less its user samples, reported beside.
"""
import collections
import json
import os
import re
import subprocess
import sys

PERIOD_MS = 0.1  # -c 100000 on cpu-clock: one sample per 100 us of CPU

# (purpose, symbol regex, dso regex or None). First match from the leaf up.
PURPOSE = [
    # GPU: instance/device/context creation, and the drivers' own work.
    ("GPU: Vulkan instance and devices", r"^(vkCreateInstance|vkEnumerateInstance|vkEnumeratePhysicalDevices|vkCreateDevice|loader_|gdk_display_create_vulkan_instance|gdk_vulkan_|gsk_vulkan_device_get_for_display|gsk_vulkan_device_setup|terminator_|vkGetPhysicalDevice)", None),
    ("GPU: EGL and GL context", r"^(eglInitialize|eglGetPlatformDisplay|eglGetDisplay|eglCreateContext|eglMakeCurrent|gdk_display_init_gl|gdk_display_create_gl_context|gdk_gl_context_realize|gdk_display_prepare_gl|__egl|_eglInitialize|dri2_initialize|QOpenGLContext::create|QEGLPlatformContext|QtWaylandClient::QWaylandGLContext|QWaylandEglClientBufferIntegration|epoxy_)", None),
    ("GPU: pipelines and shaders", r"^(vkCreateGraphicsPipelines|vkCreateShaderModule|vkCreatePipelineCache|gsk_gpu_.*pipeline|gsk_vulkan_.*pipeline|glLinkProgram|glCompileShader|QRhi.*Pipeline|QShader)", None),
    ("GPU: rendering the frame", r"^(vkQueueSubmit|vkQueuePresentKHR|vkAcquireNextImage|vkCreateSwapchain|eglSwapBuffers|glDraw|gsk_gpu_frame|gsk_gpu_renderer_render|gsk_renderer_render|gsk_vulkan|gsk_gpu|QSGRenderer|QSGBatchRenderer|QRhi)", None),
    ("GPU: driver code (other)", None, r"(libvulkan_|libgallium|libLLVM|libnvidia|libGLX_nvidia|libEGL_nvidia|libVkLayer|libdrm_amdgpu|libamdgpu)"),
    # Loading at run time.
    ("Qt plugins (dlopen, metadata)", r"^(QFactoryLoader|QLibraryPrivate|QPluginLoader|QLibrary::|qt_find_plugin|QPlatformIntegrationFactory|QPlatformThemeFactory|QPlatformInputContextFactory|QtWaylandClient::Q.*IntegrationFactory|QJsonDocument|QCborValue)", None),
    ("GIO and GTK modules", r"^(g_io_modules_|g_io_module|_g_io_modules_ensure|g_io_extension_point|gtk_im_modules_init|g_type_module_use|g_module_open)", None),
    # Text.
    ("fontconfig", r"^(Fc|_Fc|FcConfig|IA__Fc)", r"libfontconfig"),
    ("fonts: shaping and glyphs", r"^(hb_|_hb_|FT_|ft_|tt_|cff_|af_|pango_|_pango|pango2|harfrust::|swash::|skrifa::|read_fonts::|parley::|fontique::|QFontEngine|QTextEngine|QFontDatabase|QFontconfigDatabase|QTextLayout|QHarfbuzz|QFont)", r"(libharfbuzz|libfreetype|libpango)"),
    # Toolkit work.
    ("CSS, theme and style", r"^(gtk_css_|_gtk_css|gtk_style_|gtk_settings_|adw_style_manager|adw_style|gtk_theming|QStyle|QFusionStyle|QCommonStyle|QPalette|QStyleSheet|QGtk3Theme|QGnomeTheme|QGenericUnixTheme|gtk_settings)", None),
    ("icon theme and images", r"^(gtk_icon_theme|gtk_icon_|_gtk_icon|gdk_pixbuf|gdk_texture|gdk_memory|png_|rsvg_|QIcon|QImageReader|QPixmap|QSvg)", None),
    ("UI built from XML (GtkBuilder templates, KXMLGUI)", r"^(gtk_builder|_gtk_builder|gtk_widget_init_template|gtk_buildable|KXMLGUI|KXmlGui|QDomDocument|QDom|QXmlStreamReader)", None),
    ("editor component (syntax, buffer, spell checking)", r"^(gtk_source|_gtk_source|spelling_|enchant|editorconfig|KSyntaxHighlighting|Kate|KTextEditor|pcre2|Sonnet)", r"(libgtksourceview|libspelling|libenchant|libKF6SyntaxHighlighting|libKF6TextEditor|libpcre2|libKF6Sonnet)"),
    ("settings (GSettings, dconf, KConfig)", r"^(g_settings|dconf_|g_settings_schema|KConfig|KSharedConfig|KDesktopFile)", r"(libdconf|libKF6Config)"),
    ("D-Bus, portals and accessibility", r"^(g_dbus_|_g_dbus|g_bus_|gdk_.*portal|gtk_at_spi|gtk_atspi|atspi_|QDBus|QSpi|QAccessible|dbus_|_dbus_)", r"(libdbus|libatspi|libQt6DBus)"),
    ("GObject type and class setup", r"^(g_type_class_ref|g_type_class_init|type_class_init|g_type_register|g_type_add_interface|g_type_plugin|g_param_spec_|g_object_class_install|g_signal_new|_gtk_marshal|g_type_ensure|.*_class_init$|.*_get_type_once$|.*_get_type$|g_type_init|gobject_init)", None),
    ("QML engine and Quick Controls", r"^(QQml|QV4|QJSEngine|QQmlEngine|QQmlComponent|QQmlTypeLoader|QQmlMetaType|QQmlPropertyCache|QQuickControls|QQuickStyle|QQuickTemplates|qmlRegister|QQmlApplicationEngine)", r"(libQt6Qml|libQt6QuickControls|libQt6QuickTemplates|qtquickcontrols|libqtquick)"),
    ("Qt Quick items and scene graph", r"^(QQuick|QSG)", r"libQt6Quick\.so"),
    ("Wayland and keymaps", r"^(wl_|gdk_wayland_|_gdk_wayland|QtWaylandClient::|QWayland|xkb_|kbvm::|smithay_client_toolkit::|wayland_client::|wayland_backend::|wayland_protocols)", r"(libwayland-client|libxkbcommon|libQt6WaylandClient|libqwayland)"),
    ("layout and drawing", r"^(gtk_widget_measure|gtk_widget_allocate|gtk_widget_size_allocate|gtk_widget_snapshot|gtk_snapshot|gsk_|cairo_|_cairo|pixman_|gtk_label|gtk_button|gtk_box|QWidget::|QWidgetRepaintManager|QRasterPaintEngine|QPainter|QBackingStore|QLayout|QBoxLayout|QLabel|QPushButton|qt_blend|qt_memfill|tiny_skia::|sidestep_engine::raster|sidestep_engine::text|sidestep_ui::canvas)", r"(libcairo|libpixman)"),
    ("app and toolkit start", r"^(gtk_init|gtk_application|g_application|adw_init|adw_application|gtk_window|QApplication|QGuiApplication|QCoreApplication|QApplicationPrivate|QGuiApplicationPrivate|QCoreApplicationPrivate|aizome_kit::|sidestep_ui::|sidestep_engine::|editor_|KAboutData|KCrash|KDBusService|KLocalized|KActionCollection|KateApp|KWrite)", None),
]

# The leaf's library, by kind.
MECHANISM = [
    ("dynamic linker", r"ld-linux"),
    ("C library (malloc, strings, I/O)", r"/libc\.so|libm\.so|libstdc\+\+|libgcc_s"),
    ("GPU drivers (Mesa, LLVM, NVIDIA)", r"libvulkan_|libgallium|libLLVM|libnvidia|libGLX_nvidia|libEGL|libVkLayer|libdrm|libvulkan\.so|libGLdispatch|libglapi|libgbm|libelf|libz\.so|libzstd"),
    ("XML parsing (expat: fontconfig's and Mesa's configuration)", r"libexpat"),
    ("fontconfig, FreeType, HarfBuzz", r"libfontconfig|libfreetype|libharfbuzz|libpango|libfribidi|libpng|libbrotli|libbz2"),
    ("GLib and GObject", r"libglib|libgobject|libgio|libgmodule"),
    ("GTK and libadwaita", r"libgtk|libadwaita|libgdk|libgraphene|libcairo|libpixman|libepoxy"),
    ("Qt", r"libQt6|/plugins/|libqgtk3|libqwayland"),
    ("Wayland and xkbcommon", r"libwayland|libxkbcommon|libffi"),
    ("D-Bus and a11y libraries", r"libdbus|libatspi|libatk"),
    ("GTK 3 (Qt's platform theme)", r"libgtk-3|libgdk-3|libatk"),
    ("the program itself", r"hello-kit|aizome-|\.hello-"),
    ("stamps.so (measurement)", r"stamps\.so"),
]

SKIP = re.compile(r"^(malloc|calloc|realloc|free|_int_|__libc_|__memcpy|__memset|__memmove|__mem|__str|str|mem|g_malloc|g_free|g_slice|g_realloc|g_hash_table|g_ptr_array|g_array|g_str|g_quark|g_intern|g_once|g_mutex|g_rec_mutex|g_rw_lock|g_atomic|g_bit_|g_data|g_datalist|g_slist|g_list|g_strdup|g_utf8|operator new|operator delete|QArrayData|QString|QByteArray|QHash|QMap|QList|QListData|alloc::|core::|std::|hashbrown::|__rust|_dl_|dl_open_worker|dlopen|___dlopen|_dlerror_run|dlerror_run|__GI|_dl_catch|__dlopen|dl_main|do_lookup_x|check_match|elf_|_dl_relocate)")


def classify(frames):
    """frames: [(sym, dso)] leaf first. Returns (purpose, mechanism)."""
    syms = [s for s, _ in frames]
    mech = "other"
    leaf_dso = frames[0][1] if frames else ""
    for m, rx in MECHANISM:
        if re.search(rx, leaf_dso):
            mech = m
            break
    # Before main: the dynamic linker's start-up (mapping and relocating
    # the DT_NEEDED libraries), then the libraries' constructors, which it
    # calls from _dl_init outside any dlopen.
    in_dlopen = any(s.startswith(("dlopen", "_dl_open", "dl_open_worker", "___dlopen", "__dlopen")) for s in syms)
    if any(s in ("dl_main", "_dl_start_final", "_dl_sysdep_start", "_dl_start") for s in syms):
        return "exec and dynamic linking (before main)", mech
    if not in_dlopen and any(s in ("_dl_init", "call_init", "_dl_start_user") for s in syms):
        return "library constructors (before main)", mech
    for sym, dso in frames:
        if not sym or sym == "[unknown]":
            continue
        for purpose, srx, drx in PURPOSE:
            if srx and re.search(srx, sym):
                return purpose, mech
            if drx and re.search(drx, dso) and not SKIP.search(sym):
                return purpose, mech
    if any(s.startswith(("dlopen", "_dl_open", "dl_open_worker")) for s in syms):
        return "dlopen (other)", mech
    return "other", mech


def parse_script(path):
    """Yield (comm, tid, time_s, frames) from perf script."""
    p = subprocess.Popen(["perf", "script", "-i", path, "-F", "comm,tid,time,ip,sym,dso", "--ns", "--no-inline"],
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, errors="replace")
    head, frames = None, []
    hre = re.compile(r"^(\S.*?)\s+(\d+)\s+(\d+\.\d+):\s*$")
    fre = re.compile(r"^\s+[0-9a-f]+\s+(.*?)\s+\((.*)\)\s*$")
    for line in p.stdout:
        if not line.strip():
            if head:
                yield head + (frames,)
            head, frames = None, []
            continue
        m = hre.match(line)
        if m and not line.startswith(("\t", " ")):
            if head:
                yield head + (frames,)
            head, frames = (m.group(1), int(m.group(2)), float(m.group(3))), []
            continue
        m = fre.match(line)
        if m and head:
            sym = re.sub(r"\+0x[0-9a-f]+$", "", m.group(1))
            frames.append((sym, m.group(2)))
    if head:
        yield head + (frames,)
    p.wait()


def exec_time(stamps_path, prog_hint):
    """CLOCK_MONOTONIC (s) of the program's first stamps ctor: its exec (the
    profiled command's own ctor, perf's, is skipped)."""
    first = None
    for line in open(stamps_path):
        parts = line.split()
        if len(parts) >= 6 and parts[3] == "ctor" and os.path.basename(parts[5]) != "perf":
            t = int(parts[0]) / 1e9
            first = t if first is None else min(first, t)
    return first


def short(dso):
    return os.path.basename(dso)


def main():
    args = sys.argv[1:]
    rundir = args.pop(0)
    folded_out = None
    if "--folded" in args:
        i = args.index("--folded")
        folded_out = args[i + 1]
        del args[i:i + 2]
    names = args
    runs = [json.loads(l) for l in open(f"{rundir}/runs.jsonl")]
    runs = [r for r in runs if not r.get("meta") and not r.get("warmup") and r.get("name") in names]
    folded = collections.Counter()
    for name in names:
        rs = [r for r in runs if r["name"] == name and r.get("t_map_mono_ns")]
        purpose = collections.Counter()
        mech = collections.Counter()
        cell = collections.Counter()
        leaf = collections.Counter()
        dsos = collections.Counter()
        threads = collections.Counter()
        kernel_ms = []
        user_ms = []
        sched_ms = []
        for r in rs:
            tag = f"{name}-{r['rep']}"
            t_map = r["t_map_mono_ns"] / 1e9
            t_exec = exec_time(f"{rundir}/{tag}.stamps", name) or r["t0_mono_ns"] / 1e9
            n = 0
            per_tid = collections.Counter()
            for comm, tid, t, frames in parse_script(f"{rundir}/{tag}.perf"):
                if t < t_exec or t > t_map:
                    continue
                n += 1
                per_tid[tid] += 1
                p, m = classify(frames)
                purpose[p] += 1
                mech[m] += 1
                cell[(p, m)] += 1
                if frames:
                    leaf[(frames[0][0], short(frames[0][1]))] += 1
                    dsos[short(frames[0][1])] += 1
                threads[comm] += 1
                if folded_out:
                    stack = ";".join(f"{s or '?'}" for s, _ in reversed(frames))
                    folded[f"{name};{comm};{p};{stack}"] += 1
            # The threads' CPU at the map (schedstat), less their user
            # samples, is time in the kernel (and the threads' share of
            # sampling's own overhead).
            run_ns = sum(th["run_ns"] for th in r["threads_at_map"] if th.get("exe") and os.path.basename(th["exe"]) != "perf")
            sched_ms.append(run_ns / 1e6)
            user_ms.append(n * PERIOD_MS)
            kernel_ms.append(run_ns / 1e6 - n * PERIOD_MS)
        k = len(rs)
        if not k:
            print(f"## {name}: no runs\n")
            continue
        med = lambda xs: sorted(xs)[len(xs) // 2]
        print(f"## {name}: {k} runs, first frame (under perf) median {med([r['first_frame_ms'] for r in rs]):.1f} ms")
        print(f"CPU to first frame (schedstat, all threads) median {med(sched_ms):.1f} ms: user (sampled) "
              f"{med(user_ms):.1f} ms, kernel and unsampled {med(kernel_ms):.1f} ms\n")
        print("| Purpose | user ms per run | share |")
        print("|---|---:|---:|")
        tot = sum(purpose.values())
        for p, c in purpose.most_common():
            print(f"| {p} | {c * PERIOD_MS / k:.2f} | {100 * c / tot:.1f}% |")
        print("\n| Mechanism (leaf library) | user ms per run | share |")
        print("|---|---:|---:|")
        for m, c in mech.most_common():
            print(f"| {m} | {c * PERIOD_MS / k:.2f} | {100 * c / tot:.1f}% |")
        print("\nLargest purpose x mechanism cells (ms per run):")
        for (p, m), c in cell.most_common(15):
            print(f"  {c * PERIOD_MS / k:7.2f}  {p}  /  {m}")
        print("\nThreads (ms per run):", ", ".join(f"{t} {c * PERIOD_MS / k:.1f}" for t, c in threads.most_common(10)))
        print("\nLeaf libraries (self ms per run):")
        for d, c in dsos.most_common(15):
            print(f"  {c * PERIOD_MS / k:7.2f}  {d}")
        print("\nLeaf functions (self ms per run):")
        for (s, d), c in leaf.most_common(30):
            print(f"  {c * PERIOD_MS / k:7.2f}  {s}  ({d})")
        print()
    if folded_out:
        with open(folded_out, "w") as f:
            for s, c in sorted(folded.items()):
                f.write(f"{s} {c}\n")


if __name__ == "__main__":
    main()
