#!/usr/bin/env python3
"""Where a toolkit's CPU goes, stage by stage, from `perf record` call chains
(docs/performance/why-toolkits.md). Attribution only: these runs are slowed
by perf, so shares are reported, and turned into milliseconds against the
clean runs' CPU totals.

    perfsum.py PERF.data FAMILY [--perf PERF] [--json OUT]

FAMILY is kit, gtk, qt (Widgets), qtquick or sway. Every sample gets:
  a stage: the stage rule matching the frame nearest the root of its call
    chain (rules are tried in priority order; see STAGES), else "other";
  a mechanism: the library (or, for Rust, the crate) of its leaf frame.
User space only (perf_event_paranoid is 2): the kernel's share (syscalls,
page faults) is in the clean runs' totals, not here."""
import collections
import json
import re
import subprocess
import sys

# (stage, pattern) per family, in priority order: the first rule that
# matches any frame of a chain names its stage.
STAGES = {
    "gtk": [
        ("style (CSS)", r"^gtk_css_|^_gtk_css_|^gtk_style_"),
        ("input and model update", r"^gtk_main_do_event|^gtk_event_controller|^gtk_text_buffer_(insert|delete)|"
                                   r"^_gtk_text_btree|^gtk_text_view_key|^gtk_im_context|keyboard_handle_key|"
                                   r"^gdk_wayland_seat"),
        ("layout and measure", r"^gtk_widget_allocate|^gtk_widget_size_allocate|^gtk_widget_measure|"
                               r"^gtk_root_start_layout|^gtk_window_.*layout|^gtk_list_.*(allocate|measure)"),
        ("snapshot (render nodes)", r"^gtk_widget_snapshot|^gtk_snapshot_|^gtk_widget_render|_snapshot$"),
        ("render (GSK, GPU commands)", r"^gsk_renderer_render|^gsk_gpu_|^gsk_vulkan|^gsk_gl_|^gsk_render_node_diff|"
                                       r"^gsk_.*_node_draw"),
        ("present (swapchain, Wayland)", r"^gdk_draw_context_end_frame|^gdk_vulkan_context|^gdk_gl_context|"
                                         r"^vkQueuePresent|^wsi_|^gdk_wayland_surface"),
        ("frame clock and main loop", r"^gdk_frame_clock|^g_main_|^gtk_application"),
    ],
    "qt": [
        ("input and model update", r"QPlainTextEdit::keyPressEvent|QWidgetTextControl|QTextCursor::insert|"
                                   r"QTextCursor::delete|QTextDocumentPrivate::(insert|remove)|"
                                   r"QAbstractItemView::|QScrollBar::|QAbstractSlider::"),
        ("layout and measure", r"QPlainTextDocumentLayout|QTextLayout::(beginLayout|endLayout|createLine)|"
                               r"QTextLine::layout|QLayout::|QWidgetItem|QListView.*(Layout|layout)|doItemsLayout"),
        ("present (shm buffer, Wayland)", r"QWaylandShmBackingStore::flush|QWaylandWindow::(commit|attach|handleUpdate)|"
                                          r"QPlatformBackingStore::flush|QWaylandShmBackingStore::beginPaint|"
                                          r"QWaylandShmBackingStore::endPaint|QWaylandShmBuffer"),
        ("paint (QPainter, raster)", r"QWidgetRepaintManager|QWidgetPrivate::(paint|drawWidget)|::paintEvent|"
                                     r"QPainter::|QRasterPaintEngine|QTextLayout::draw|QStyle"),
        ("event loop", r"QCoreApplication|QEventDispatcher|QEventLoop|QApplication::notify|g_main_"),
    ],
    "qtquick": [
        ("input and model update", r"QQuickTextControl|QQuickTextEdit::(keyPressEvent|inputMethod)|"
                                   r"QTextCursor::|QTextDocumentPrivate::|QQuickWindow.*deliver|QQuickDeliveryAgent|"
                                   r"QQuickFlickable::setContent|QQuickListView|QQuickItemView"),
        ("JavaScript and bindings", r"^QV4::|QQmlBinding|QQmlJavaScript|QQmlBoundSignal|QQmlPropertyBinding"),
        ("layout and polish", r"QTextDocumentLayout|QTextLayout::(beginLayout|endLayout|createLine)|QTextLine::layout|"
                              r"updatePolish|polishItems|QQuickTextEdit::(updateSize|updateWholeDocument)"),
        ("sync (paint nodes)", r"syncSceneGraph|::updatePaintNode|QQuickTextNode|QSGTextNode|QQuickTextNodeEngine|"
                               r"QSGInternalTextNode|QQuickWindowPrivate::sync"),
        ("render (scene graph, GPU commands)", r"QSGBatchRenderer|QSGRenderer::|QSGRhi|QRhi|QSGDefaultRenderContext|"
                                               r"QQuickWindowPrivate::renderSceneGraph|QSGRenderLoop|QSGThreadedRenderLoop|"
                                               r"QSGRenderThread::(sync|syncAndRender)"),
        ("present (EGL, Wayland)", r"eglSwapBuffers|QWaylandEglWindow|QWaylandGLContext::swapBuffers|"
                                   r"dri2_wl_swap|wl_egl"),
        ("event loop", r"QCoreApplication|QEventDispatcher|QEventLoop|QGuiApplication::notify|g_main_"),
    ],
    "kit": [
        ("input and event dispatch", r"aizome_kit::engine::Core>::(key|press|pointer_\w+|ime|bubble|perform|scroll|"
                                     r"window_focus|update_hot)|Handler>::window_event|EditorContent>::perform"),
        ("update and view (the program)", r"tk_kit::Tk as aizome_kit::program::Program>::(update|view)|"
                                          r"Runtime<[^>]*>>::flush|aizome_kit::element::|::into_el"),
        ("layout", r"aizome_kit::tree::Tree>::(layout|layout_window)|aizome_kit::engine::Core>::relayout|"
                   r"as aizome_kit::widget::Widget>::(layout|measure)"),
        ("mount and diff", r"aizome_kit::tree::Tree>::(mount_under|finish|snapshot|kill|uncache)|::hash\b"),
        ("paint (record operations)", r"Handler>::draw|aizome_kit::engine::Core>::paint|aizome_kit::tree::Tree>::paint_node|"
                                      r"aizome_kit::draw::|as aizome_kit::widget::Widget>::paint|sidestep_ui::canvas|"
                                      r"sidestep_ui::app::Cx>::display"),
        ("raster (tiny-skia, glyphs)", r"sidestep_engine::raster|tiny_skia|paint_banded|Glyphs|sidestep_engine::text"),
        ("buffer (copy, move, swizzle)", r"draw_frame|copy_between|move_pixels|scroll_in|swizzle|copy_rows|fault_in"),
        ("Wayland and present", r"wayland_backend|wayland_client|smithay_client_toolkit|backend::State>::(present|handle)|"
                                r"calloop"),
        ("event loop and messages", r"sidestep_ui::app|aizome_kit::app|mpsc|crossbeam"),
    ],
    "sway": [
        ("import shm (upload to texture)", r"wlr_texture_update_from_buffer|gles2_texture_update|texture_write_pixels|"
                                           r"wlr_client_buffer_apply_damage|surface_apply_damage|glTexSubImage|"
                                           r"texsubimage|_mesa_texstore|util_copy_rect|memcpy"),
        ("import dmabuf", r"wlr_texture_from_dmabuf|gles2_texture_from_dmabuf|eglCreateImage|dri2_create_image|"
                          r"wlr_linux_dmabuf"),
        ("render (scene, GLES)", r"wlr_scene_output_build_state|wlr_scene_output_commit|wlr_render_pass|gles2_render|"
                                 r"render_pass_add|scene_node|wlr_renderer|_mesa_|glDraw|si_|radeonsi"),
        ("output commit", r"wlr_output_commit|output_commit|drm_|headless"),
        ("Wayland protocol", r"wl_closure|wl_client_connection|wl_resource|wl_event_loop_dispatch|surface_commit|"
                             r"wl_connection"),
        ("screencopy", r"screencopy"),
    ],
}

MECH = [
    ("GPU driver (Mesa)", r"libgallium|radeonsi|libvulkan_radeon|libLLVM|libdrm|libEGL_mesa|libGLX|libglapi|libgbm"),
    ("Vulkan / GL loader", r"libvulkan\.so|libEGL\.so|libGLESv2|libGL\.so|libepoxy"),
    ("text: HarfBuzz / FreeType / Pango / fontconfig", r"libharfbuzz|libfreetype|libpango|libfontconfig"),
    ("cairo / pixman", r"libcairo|libpixman"),
    ("GTK / GDK / GSK", r"libgtk-4"),
    ("libadwaita", r"libadwaita"),
    ("GLib / GObject", r"libglib|libgobject|libgio"),
    ("Qt Core", r"libQt6Core"),
    ("Qt Gui (incl. raster painting)", r"libQt6Gui"),
    ("Qt Widgets", r"libQt6Widgets"),
    ("Qt Quick / QML", r"libQt6Quick|libQt6Qml"),
    ("Qt Wayland", r"libQt6Wayland"),
    ("libwayland", r"libwayland"),
    ("C library (memcpy, malloc)", r"libc\.so|libc-|ld-linux|libm\.so|libstdc\+\+|libgcc"),
    ("wlroots", r"libwlroots"),
    ("sway", r"/sway$|sway-unwrapped"),
    ("pixman", r"pixman"),
]
RUST_CRATES = r"^<*(tiny_skia|sidestep_engine|sidestep_ui|aizome_kit|wayland_backend|wayland_client|" \
              r"smithay_client_toolkit|calloop|harfrust|rustybuzz|swash|skrifa|read_fonts|fontique|parley|zeno|" \
              r"core|alloc|std|hashbrown|ropey|tk_kit|crossbeam\w*|rayon\w*)"


def samples(path, perf):
    p = subprocess.Popen([perf, "script", "-i", path, "-F", "comm,tid,period,ip,sym,dso", "--no-inline"],
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, errors="replace")
    head, frames = None, []
    for line in p.stdout:
        line = line.rstrip("\n")
        if not line:
            if head:
                yield head, frames
            head, frames = None, []
            continue
        if not line.startswith((" ", "\t")):
            m = re.match(r"\s*(.+?)\s+(\d+)\s+(\d+)\s*$", line) or re.match(r"\s*(.+?)\s+(\d+)\s+(\d+)", line)
            head = (m.group(1), int(m.group(3))) if m else (line.split()[0], 1)
            frames = []
        else:
            m = re.match(r"\s*([0-9a-f]+)\s+(.*?)\s+\((.*)\)\s*$", line)
            if m:
                frames.append((m.group(2), m.group(3)))
    if head:
        yield head, frames
    p.wait()


def mechanism(sym, dso):
    for name, pat in MECH:
        if re.search(pat, dso):
            return name
    m = re.match(RUST_CRATES, sym)
    if m:
        crate = m.group(1)
        return f"Rust: {crate}"
    return "the program (other)" if dso else "unknown"


def main():
    path, family = sys.argv[1], sys.argv[2]
    perf = sys.argv[sys.argv.index("--perf") + 1] if "--perf" in sys.argv else "perf"
    rules = [(n, re.compile(p)) for n, p in STAGES[family]]
    stage = collections.Counter()
    mech = collections.Counter()
    cell = collections.Counter()
    leaf = collections.Counter()
    threads = collections.Counter()
    total = 0
    for (comm, period), frames in samples(path, perf):
        total += period
        threads[re.sub(r"\d+$", "#", comm)] += period
        st = "other"
        syms = [f[0] for f in frames]
        for name, rx in rules:
            if any(rx.search(s) for s in syms):
                st = name
                break
        mc = mechanism(*frames[0]) if frames else "unknown"
        stage[st] += period
        mech[mc] += period
        cell[(st, mc)] += period
        if frames:
            leaf[f"{frames[0][0][:90]}  ({frames[0][1].rsplit('/', 1)[-1]})"] += period
    share = lambda c: [(k, round(v / total, 4)) for k, v in c.most_common()]  # noqa: E731
    out = dict(total_period=total, stages=share(stage), mechanisms=share(mech), threads=share(threads),
               cells=[(f"{a} / {b}", s) for (a, b), s in share(cell)[:25]], leaves=share(leaf)[:30])
    if "--json" in sys.argv:
        with open(sys.argv[sys.argv.index("--json") + 1], "w") as f:
            json.dump(out, f, indent=1)
    else:
        print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
