# Isolated native-scene GPU composition

The isolated Rust prototype renders recorded Hotaru windows on Intel hardware
with **zero differing bytes** against the existing software renderer. It covers
terminal cells and ordinary Aizome chrome together. It is not connected to the
production application's event loop, and it does not establish performance
parity with Ghostty or the current CPU renderer.

The [frozen evidence](../upstream/gpu-buffer/native-scene/README.md) includes
exact source, shader, Cargo lock, native-scene recorder, loader identities,
failure evidence, successful runs and an original-resolution capture. The
prototype starts at Sekka `3d10197261512c92ccc5816dfdd60c88566c4150`, before the
later expanded sprite and partial-region scroll work. Builds and software tests
ran on wx-workstation. The Intel hardware runs completed before the user's
instruction to return all builds, tests and benchmarks to wx-workstation; no
further laptop run is planned.

## What passed

The recorder captures actual Sidestep operations from native Hotaru and its View
menu popup. Day and Night themes, scales 1, 1.25 and 2, selection, Find, cursor,
ANSI styles, CJK, combining text, color emoji, links and Alpha8 sprites produce
18 complete scenes. Each lowered scene matches the original CPU output exactly.
The menu is opened through the actual menu engine in the harness; this is a
renderer fixture, not a keyboard/pointer menu-activation test.

All 18 scenes were rendered twice by the hardware compute worker. Every RGBA
byte matches, and each warm repeat uploads zero asset bytes. The final remote
software rerun reproduces all 36 input-file hashes used by the hardware run.
The original glyph rasterizer and native operation ordering remain the oracle;
this compares composition, not independent font rasterization.

Three independent private headless Sway GLES runs present the 1200×830 Find
scene. Each capture matches 2,988,000 RGB bytes inside the client region,
`[40, 65, 1200, 830]`, of a 1280×960 output. The RGB PPM hash is
`c10d6575ebb046d3698bc89a882cd1d72c9b4a1aa1836b29ccad0bcafd6dc6b2`.
XRGB capture alpha is not compared. The producer separately verifies every RGBA
byte before handing off its descriptor. All three runs exit successfully after
detach, authoritative `wl_buffer.release`, resource/cache/context destruction;
all owned processes exit without forced descendant cleanup. Headless
presentation feedback is not physical scanout evidence.

Remote checks pass 15 lifecycle tests, three unique scene-reader tests and two
feedback tests (23 executions because the reader module is in two binaries),
five safe preparation tests and the 18-scene native fixture. Strict Clippy for
the standalone library and both scene binaries passes after the separately
archived lint/comment-only delta. That later source is not relabeled as the
Intel-tested executable.

## Rendering and numerical contract

Preparation walks the immutable operation list once, preserving order and
clipping. It resolves glyph masks/color glyphs through the existing font cache,
retains Alpha8 sprite ownership and produces solid rectangles, mask operations
and premultiplied image operations. Small vector/icon assets may be rasterized
or filtered during preparation; terminal rows and complete windows are never
CPU-rasterized for upload. The diagnostic CPU interpreter exists only for the
exact comparison gate.

A bounded 16×16 tile index assigns each command to intersecting tiles. GPU
invocations traverse only their tile's ordered list. A GLES 3.1 compute shader
writes immutable RGBA8 texture storage. Integer formulas match the CPU's
byte-space blending and distinct coverage rounding. Ordinary RGBA glyphs use
premultiplied source-over. The CPU raster path is not linear-light sRGB blending,
so the GPU does not add a gamma conversion. Dithering is disabled.

Fractionally placed small icons require the existing tiny-skia high-precision
filter samples to remain f32 until destination blending. The prototype stores
those bounded samples and uses `GL_EXT_gpu_shader5` precise operations and
round-to-even quantization. Other masks and colors use integer arithmetic.
The test tolerance remains **zero**; it was not relaxed to hide a one-unit
blend difference. tiny-skia's BSD license and source attribution are retained.

`glTexStorage2D` provides the immutable RGBA8 image required by image load/store.
The compute dispatch is followed by image-access, texture-fetch and framebuffer
memory barriers. Exported EGL images preserve their source. A producer fence
wait runs only on the render worker, in cancellable 5 ms slices with an absolute
5 s deadline, before readback or event-side descriptor admission. The contracts
are the Khronos [image binding](https://registry.khronos.org/OpenGL-Refpages/es3/html/glBindImageTexture.xhtml),
[memory barrier](https://registry.khronos.org/OpenGL-Refpages/es3/html/glMemoryBarrier.xhtml)
and [DMA-BUF export](https://registry.khronos.org/EGL/extensions/MESA/EGL_MESA_image_dma_buf_export.txt)
specifications.

## Ownership, bounds and failure

The existing isolated worker/lease boundary owns GL/EGL objects on one dedicated
thread. The event side never calls EGL, waits for a fence, or joins the worker.
An immutable source identity stays with each lease through import, presentation,
close and release. Only referenced surface-feedback tranche entries admit a
format/modifier, and asynchronous DMA-BUF creation can fail before attachment.
The diagnostic accepts single-plane AB24/linear; it does not claim general
modifier or multi-plane support.

Preparation preflights the whole list. Unsupported transforms, groups, blends,
shadows, masks, gradients, image mipmaps or bounds return a whole-scene fallback
outcome. No partial scene is submitted. Production SHM failover is not wired by
this diagnostic. The representative ordinary chrome scenes all take the GPU
path, but that is not coverage of every Aizome appearance or operation.

Current caps include 65,536 input operations, 131,072 emitted commands/glyph
visits, 4,194,304 tile-index entries, 1 MiB per asset, 32 MiB of scene asset data,
and an 8192-pixel dimension limit. The preparation cache has 4,096 entries and a
16 MiB asset-byte cap; its retained source Arcs are additional ownership.
The GPU atlas allocates 32 MiB and retains corresponding source assets. Its
entry count is bounded indirectly by nonempty padded atlas entries, but needs
a practical explicit metadata cap before production use. Tile/command SSBOs,
cache data and temporary staging allocations are outside the lifecycle's
per-lease budget. **The diagnostic's 128 MiB lease setting is not a total
process/GPU-memory bound.**

The binary reader uses bounded reads on the actual open file, validates complete
lengths and every command/asset/tile relation, then exposes immutable accessors.
This avoids accepting a truncated expected image via a prefix-only comparison.
Driver FFI and GLSL indexing were independently reviewed after these fixes.

The first Wayland attempt compared all pixels and logged object cleanup, then
crashed while exiting. Its core shows libc exit on the main thread concurrent
with worker `dlclose`/`_dl_unmap`. The repair pins one selected EGL library in a
process-lifetime `OnceLock`; contexts, buffers, images and fences still have
explicit destruction. The first load result, including failure, is cached and
a different library path is rejected. No event-side blocking join was added.
The repaired offscreen run and three fresh Wayland processes pass. Failed and
successful evidence are retained separately.

## Cost and remaining work

The recorded costs are diagnostics, not controlled benchmarks. Warm
1920×1328 dispatch/export/fence observations range from roughly 4.2 to 10.5 ms;
another warm 1200×830 scene reaches 54.3 ms. Preparation/upload, readback and
verification are recorded separately. There is no accepted paired CPU/Ghostty
comparison, and the prototype creates and exports a new texture per frame.
These results do not justify a speed claim.

Before integration, reserve cache/staging bytes in a coherent global budget,
cap cache metadata, preflight/cancel preparation work, separate dispatch/export/
fence cost, and qualify buffer pooling without reusing a committed buffer.
Connect a bounded latest-frame queue and calloop wake to the real Sidestep
backend, preserving clipboard responsiveness, frame pacing, stale geometry,
window teardown and complete SHM fallback. Then compare the original 60 Hz
partial-region scroll workload and damaged-frame CPU path on the same eligible
hardware. Hardware-dependent changes after this freeze remain unqualified until
they can be tested on an authorized remote machine.
