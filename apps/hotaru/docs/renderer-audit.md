# Linux renderer audit and whole-application measurement plan

This is a source audit and hardware-capability probe, not a rendering performance
result. It extends [the earlier software GUI probes](gui-probe.md), whose DSR
timings must not be relabeled as frame-completion timings. The reference is
Ghostty `6ba144158577d033ccb6ed4b574e4013957c9161`; the Hotaru, Aizome and patched
Sidestep files inspected here are individually hashed in
[`upstream/renderer-audit/source-manifest.json`](../upstream/renderer-audit/source-manifest.json).
These hashes identify the source audit, not a subsequently built package.

The later [cursor/cache checkpoint](cursor-cache-checkpoint.md) integrates
immutable shaped-row reuse and native cursor shapes/blinking. The table and
source-level gaps below describe the original audited snapshot; their
full-row-shaping and fixed-cursor observations are superseded by that checkpoint.
GPU drawing/presentation and pixel-move scrolling remain open.

## What the two applications do

| Stage | Current Hotaru / native Aizome / Sidestep | Pinned Linux Ghostty |
| --- | --- | --- |
| State delivery | The PTY actor publishes an immutable viewport at most once per 8 ms while output changes, with one pending UI frame. | Terminal changes wake a renderer thread. Normal updates render immediately; the old optional 10 ms coalescing timer is commented out. |
| Shaping | One worker per pane, with one active and one replaceable pending job. Every job visits every visible row, constructs per-cell strings and span lists, then checks the grid layout cache. | `rebuildCells` skips clean rows, retains per-row render cells, and consults a shaping-run cache only for rows being rebuilt. |
| Glyphs | Harfrust terminal clusters and configured font fallback now exist; grayscale and color glyphs are rasterized with swash and cached in CPU memory. | HarfBuzz / FreeType plus built-in terminal sprite glyphs populate separate grayscale and color atlases. |
| Drawing | CPU fill/blend operations write damaged regions of Wayland shared-memory buffers. Large compatible paints use at most four threads by default. | Instanced OpenGL drawing uses atlas textures and cell buffers. Three frame states retain GPU resources. Atlas generation counters avoid uploads when unchanged, although a changed atlas currently uploads its full area. |
| Presentation | Reuse a released `wl_shm` buffer, otherwise repair a stale buffer or allocate another. Submit damage, request a frame callback, then commit the surface and parent frame. | Render to an sRGB framebuffer, blit to an exportable RGBA texture, export DMA-BUF and hand it to GTK. Failed export/import uses an allocated CPU readback and `GdkMemoryTexture`. `Frame.complete` calls `glFinish` before publishing the finished frame. |
| Pacing | Compositor frame callbacks pace native drawing; PTY snapshot throttling and the shaping worker are additional stages before that callback. | GTK controls final presentation. The generic renderer's display-link / `window-vsync` implementation is macOS-only and compiles away on Linux; it must not be cited as Linux frame pacing. |

Hotaru has dirty-row **painting**, but not yet dirty-row **shaping preparation**.
For a full 120×40 single-width ASCII viewport, `font::shape` creates 4,800 owned
cell strings; `FontSet::shape` creates another 4,800 strings for its cache keys
before discovering a hit. It also allocates row match masks when Find is closed,
rescans link metadata, and creates the row/span containers. These counts follow
the code paths, not an allocation profiler. A plain single-cell update therefore
still performs work proportional to the whole viewport before paint-time damage
filtering. The expensive Harfrust work itself can already hit its cache.

The grid layout cache is limited to 512 entries / approximately 8 MiB and clears
as a unit on overflow. Font resolution has separate bounds. CPU glyph caches
use two generations with a 24 MiB image budget, with both thread-local references
and a shared image cache. Those limits are not a process-wide bound on all font
data, face registrations, layouts, or images retained by in-flight frames.

Scrolling currently changes the row identity at almost every screen coordinate,
so it damages most of the viewport even when the same immutable rows merely
moved. Kit already exposes `LayoutCx::scrolled`, and Sidestep supports moving
pixels and repairing stale buffers. Hotaru has not connected terminal scrolling
to that path. Pixel movement needs explicit overlay and geometry checks; it is
not safe to infer unchanged pixels from text equality alone.

## Remaining visual and scheduling gaps

The earlier GUI probe's missing CJK/emoji result describes its old package.
Current native code has a packaged fallback bundle, explicit terminal clusters,
color-emoji fitting, OpenType feature controls and cursor-aware ligature breaks.
Do not repeat that old limitation as the current implementation status.

The following gaps remain visible in the inspected source:

- There is no native GPU atlas or GPU draw/present path. CPU glyph caching saves
  rerasterization but still blends glyph pixels into the framebuffer on every
  damaged draw. A hardware compositor must then import/upload those SHM pixels.
- Ghostty's built-in box, block, braille, powerline, legacy-computing and special
  cursor/decoration sprite generation has not been ported. Current ordinary font
  fallback is not evidence of seamless cell-filling sprite equivalence.
- The terminal widget paints a fixed focused underline / unfocused outline
  cursor, rather than honoring all parsed cursor shapes and blink behavior.
  Cursor movement also changes shaping boundaries, so a future cursor cache
  cannot invalidate only the tiny cursor rectangle.
- The CPU rasterizer blends premultiplied byte channels. Ghostty's Linux default
  is `alpha-blending=linear-corrected`, with shader correction for text coverage.
  Pixel comparisons must distinguish this known color-space difference from
  glyph-position or missing-glyph errors. Hinting also differs: swash currently
  uses `hint(false)`.
- Kitty images/animations, background images/custom shaders and other missing
  rendering features cannot participate in an acceptance benchmark as if both
  applications performed the same work.
- The actor's 8 ms frame cadence can add latency after recent output, and caps
  delivered snapshots around 125 Hz before shaping and compositor pacing.
  This is a source-level bound, not a measured keyboard-latency distribution.
  Idle already blocks on events; removing that delay must retain bounded queues
  and avoid rendering every PTY chunk during a flood.
- Sidestep's frame statistics measure recording, rasterizing, buffer repair and
  commit CPU time. Its frame callback is permission to draw again, not proof of
  physical scanout. Neither statistic is an end-to-end display timestamp.

## Workstation capability, without desktop changes

[`hardware.json`](../upstream/renderer-audit/hardware.json) and its stderr log are
from `tools/renderer-discovery.py --egl` on `ssh wx-workstation`. The tool only
inventories devices/libraries and creates a 16×16 offscreen pbuffer; it changes
no ACL, group, display, power, clock or desktop configuration.

| Observation | Result |
| --- | --- |
| GPU | NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition, 97,887 MiB reported memory |
| Driver / kernel | NVIDIA 580.159.04 / Linux 6.17.0-1032-oem |
| Existing graphical session | GDM's Xorg / GNOME session; it was neither joined nor changed |
| `/dev/dri/card1`, `/dev/dri/renderD128` | Opening read-write as SSH user `callen` returns `EACCES`; current ACL grants GDM access |
| `/dev/nvidia0`, `/dev/nvidiactl` | Accessible with existing permissions |
| NVIDIA device-platform EGL | EGL 1.5, OpenGL 4.6.0 NVIDIA 580.159.04; clear/readback succeeded, GL error zero |
| Separate Mesa software device | llvmpipe, Mesa 25.2.8; explicitly identifiable as software |
| Wayland hardware presentation | Not established by this probe |

This makes **offscreen hardware-GPU experiments possible now**, without using
the real desktop. It does not automatically make the current private compositor
hardware accelerated. Aizome's current headless GPU initializer opens a DRM
render node and creates a GBM device; that path fails under these observed
permissions and falls back to pixman. Its EGL dependency, Smithay 0.7, also has
an `EGLDevice` native-display implementation. A disposable test-only headless
adapter can use the proven device-platform path, then independently qualify
EGL context creation, imports, DMA-BUF formats/modifiers and synchronization.
Do not advertise DMA-BUF feedback based solely on a guessed device identity.

The host driver test used the host loader. The packaged applications use Nix
loaders; their NVIDIA userspace libraries must match the loaded kernel module
and be resolved under the actual package interpreter. Merely setting
`LIBGL_ALWAYS_SOFTWARE=0`, adding host `/usr/lib` wholesale to a Nix process, or
seeing a GPU in `nvidia-smi` does not prove hardware rendering. Save actual
`GL_VENDOR` / `GL_RENDERER`, loaded-library maps and import/export failures for
the compositor and both applications. Ghostty normally obtains its EGL display
from GDK's Wayland display; its fallback Mesa-surfaceless path is not proof that
the GTK application can use the NVIDIA offscreen device tested here.

Until the disposable hardware compositor and both client paths work, keep
software whole-app, hardware offscreen-component, and hardware whole-app
results in different result sets. No permission/configuration workaround is
required for the present audit, and none was made.

## Reproducible whole-application benchmark

Use a dedicated temporary runtime/home/config/cache and a private compositor and
D-Bus without service activation, as the existing GUI harness already does.
Use exact packaged release binaries, the same font files and feature settings,
opaque backgrounds, equal grid and measured physical cell geometry, equal
history limits, and identical PTY programs/data. Record both terminal content
dimensions and outer window/chrome dimensions. If font metrics cannot yet match,
report that qualification failure rather than silently normalizing pixel costs.

Run one application at a time under the shared workstation lock, alternate
application order, pin application/compositor/observer to disjoint documented
CPU sets, and record GPU/CPU clock, temperature and unrelated activity. Do not
change host governors, GPU clocks, permissions or caches. Repeat fresh-process
tests separately from warm steady-state tests; a fresh font cache is not a cold
kernel page cache. Measure the application and compositor separately and also
report their combined CPU/memory use. A GPU comparison must include VRAM and
upload/readback traffic where the API exposes it.

| Test | Input and correctness gate | Required reported measurements |
| --- | --- | --- |
| Startup | Fresh process executes a raw PTY fixture and paints a unique marker; verify geometry and the first complete marker frame. Use at least 30 alternating runs. | Launch→PTY ready and launch→first correct compositor frame separately; RSS/PSS, faults, source/ELF/font hashes. |
| Idle | Quiescent shell with blink disabled in both apps, then a separate matched blink test; focused, unfocused and hidden states. Allow history compression to settle first. | At least 30 s CPU interval, per-process/thread activity, frame count, wakeups where available, RSS/PSS/VRAM; never treat 0 ticks over 3 s as zero cost. |
| Keyboard latency | Persistent private virtual-keyboard client injects real protocol key events. Raw fixture records read time and emits text plus a sequence/CRC color marker. At least 500 paced trials with randomized intervals. | Injection→fixture read, fixture read→write, injection→first complete matching output frame; p50/p95/p99/max, missed/coalesced samples. Label virtual input and headless completion explicitly. |
| Changed-row drawing | Static large viewport, then one cell, one row, cursor crossing a ligature, selection, Find overlay, and OSC8 hover changes. | Shaped/reused rows, allocations, shape/cache hits, damaged pixels, record/raster/submit CPU, correct output frame latency. |
| Scroll/frame throughput | Deterministic full-screen and one-row-scroll streams at controlled rates, including a saturated producer and keyboard input during saturation. ASCII, mixed CJK/combining, styled, color emoji and ligature corpora use pinned fonts. | Unique correct frame serials/s, dropped/coalesced serials, final write→final visible marker, bytes/s, app+compositor CPU, GPU time, memory; DSR is a separate parser checkpoint. |
| Resize | Repeated 80×24→120×40→240×80 transitions and back, with history; separately test fractional scale 1, 1.25 and 2. Verify PTY size, reflow and final marker. | Configure dispatch→PTY resize and configure→first/settled correct frame, transient stale/blank frames, allocations, peak memory and recovery. |
| Multiple panes | One, four and sixteen panes with active/inactive output, close/reopen and font changes. | Total resources, queue bounds, active-pane latency and cleanup; do not compare different visible workloads. |

A persistent observer is needed; spawning `grim` for every sample adds process
startup and capture overhead. The first implementation can use a private
compositor's screencopy protocol to detect complete sequence/CRC markers and
record capture timestamps. Its readback cost must be measured with a static
control and reported; it is an observer-completion metric, not photon latency.
An instrumented disposable compositor can recognize the marker during its
existing render/capture stage and record output completion without modifying
either application. Preserve that compositor's patch, source hash and binary.

For definitive frame accounting, correlate client commit and frame serials with
compositor presentation feedback. `wl_surface.frame` alone is insufficient.
Headless presentation has a synthetic refresh clock and no physical scanout;
record actual refresh and feedback flags. Physical keyboard-to-photon latency
would require separate display/instrument qualification and is not claimed by
this plan. Low-rate `nvidia-smi` sampling also cannot replace GPU timer queries
for short draws. Asynchronous GPU timer queries are useful for component
experiments; avoid inserting a synchronous `glFinish` into every candidate frame
solely to time it.

Every run should retain commands/environment allowlist, source/build manifests,
binary/font/corpus hashes, graphics stack identities, scale/grid/pixel geometry,
raw event timestamps, serial/CRC outcomes, presentation flags, process samples,
capture overhead controls, logs and representative original-resolution images.
The harness must fail its hardware label when llvmpipe, CPU readback or an
unqualified import path appears. Unsupported visual workloads remain recorded
functional failures, not faster performance samples.

## Implementation order after the checkpoint

1. Add measurement before changing the renderer: frame identifiers and bounded
   counters at actor publication, shaping, recording, raster/submit and private
   compositor completion. Keep the default path free of allocations/logging
   when diagnostics are off. Extend the old harness in a new file, preserving
   the historical probe and reports.
2. Reuse immutable shaped rows before constructing cell strings. Own the RowRef
   cache key to prevent allocator-address reuse. Include font/scale revision,
   cursor run boundary, selection and projected Find/current-match intervals.
   Store row-relative link spans and layouts so a scrolled row can be reused at
   a new y position. Keep the exact `Frame.snapshot` pairing used by pointer,
   search and copy code; coalescing must not starve a coherent completed frame.
   Start with the preceding visible frame, a bounded ownership set. Verify
   unchanged-row, cursor, palette, resize, overlay, scrolling and stale-result
   cases before measuring allocations and latency.
3. Connect exact integer-row scrolling to Kit's existing pixel-move operation
   where geometry, scale, background and overlays permit it. Repaint uncovered
   rows plus old/new cursor and overlay regions; fall back to normal damage on
   uncertainty. Preserve byte-for-byte software output in this optimization.
4. Qualify a disposable NVIDIA EGLDevice headless renderer and the Nix driver
   closure. This is a capability/instrumentation task before a speed claim.
   The existing CPU renderer remains a correctness oracle and fallback.
5. Add a Linux GPU adapter to the native toolkit's existing render thread and
   draw protocol. Reuse worker-owned `grid::Layout` and face identities; upload
   grayscale/color glyph images on cache misses and draw cell backgrounds,
   glyphs and decorations as batches. Native Aizome chrome remains in the same
   ownership/event model. Normal presentation must not read the full framebuffer
   back to CPU. Keep contexts, textures, atlas generations and in-flight buffer
   ownership explicit; release/reuse buffers only after the appropriate fence
   and compositor release. Bound the queue and handle scale/resize, device loss,
   format/modifier rejection and software fallback without stale pointers.
6. Port sprite generation and remaining cursor/color-space behavior, then expand
   visual fixtures and workload acceptance. Do not recover performance by
   disabling required shaping or silently omitting visuals. Compare the final
   common complete workload against the unchanged reference, including chrome,
   process startup, idle, input, scrolling and resize.

No GPU renderer or incremental shaping change is implemented by this audit.
The original whole-application efficiency requirement remains open.
