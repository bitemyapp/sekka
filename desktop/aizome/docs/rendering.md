# Rendering: GPU and software

Aizome draws in two places, and both have a GPU path and a software path.

| | GPU | Software |
|---|---|---|
| The compositor (window frames, tabs, composition, the overview, the switcher) | smithay's GLES renderer on the GPU | smithay's pixman renderer on the CPU |
| The shell and the apps | — | Sidestep's rasterizer (tiny-skia) on the CPU, into shared memory |

Sidestep clients rasterize on the CPU: Sidestep draws only what changed, into
the shared-memory buffer the compositor shows, in parallel bands for large
changes. So an app looks and performs the same whichever renderer the
compositor uses, and the choice is the compositor's alone.

## Choosing

The compositor picks GLES and falls back to pixman by itself when there is
no usable GPU: no EGL display on the render node, no GBM, or a KMS device
without acceleration (a virtual machine's virtio-gpu without virgl, a
server's BMC framebuffer, a broken driver). It says which in its log ("udev
backend, pixman renderer") and in `aizomectl stats`.

To force one:

- Settings › Performance › Renderer: Automatic, GPU or Software (the
  `[performance] renderer` key: `auto`, `gpu`, `software`). It applies at the
  next login; Settings and `aizomectl stats` say so.
- `AIZOME_RENDERER=gles|pixman`, or `aizome --renderer gles|pixman`, for one
  run.

## What each mode does

Under GLES, shared-memory client buffers are uploaded to textures when they are committed
and released straight away (so Sidestep draws the next frame into the same
buffer), damage tracking repaints only what changed, and window moves are
pure composition. Under pixman, the compositor composites client memory
directly, damage-tracked the same way; a buffer is released when the next one
replaces it.

Both scan out through KMS on real displays (udev backend, with libseat), and
both render offscreen for the headless backend that tests and screenshots
use.

GPU clients instead submit DMA-BUFs. On a DRM device with syncobj eventfd
support, Aizome advertises `linux-drm-syncobj-v1`: the pending surface commit
waits for its acquire timeline point before buffer import, without blocking
the compositor thread. Smithay retains the release point with the renderer
buffer until its last reference retires, including scanout. Destroying a
surface cancels abandoned acquire waiters. Clients without explicit sync
keep the implicit DMA-BUF fence path. Software and winit backends do not
advertise this protocol; the GLES headless backend does so for GPU tests.
The pinned xwayland-satellite forwards the protocol to Xwayland.

KMS outputs send scanout DMA-BUF feedback for formats the primary renderer
can also import. An 8-bit game can use direct scanout even when the
compositor's own buffers are 10-bit; a failed plane test falls back to
composition. This is still SDR, with no HDR colour conversion or metadata.

`[[outputs]] vrr` accepts `off`, `fullscreen` (the default), or `always`.
Adaptive sync requires a capable KMS output and GLES; fullscreen policy
checks the top visible window on each output. Lock, recovery, keyboard
overlays, animation and mirroring disable it. Transitions force a fresh
frame even when pixels are unchanged; rejected VRR commits retry at fixed
refresh. Variable-refresh presentation feedback and frame scheduling stop
predicting a fixed vblank interval. Settings uses its normal Keep/Revert
preview for policy changes. Actual panel timing still needs a physical KMS
test: a render-node test cannot validate VRR, direct scanout, HDR or tearing.

## Measured

Release builds, 2880×1800 at scale 2 on this laptop (Intel), CPU time per
frame the compositor renders (see `performance.md` for method and the full
tables):

| | GLES p50 / p95 | pixman p50 / p95 |
|---|---|---|
| Moving a window | 0.34 / 0.67 ms | 0.54 / 0.84 ms |
| A client drawing every frame | 0.39 / 0.60 ms | 1.59 / 1.97 ms |
| A full-screen client | 0.38 / 0.58 ms | 2.53 / 3.55 ms |
| The Desks overview opening and closing (19 October) | 0.60–1.05 / 1.2–4.1 ms | 1.6–2.3 / 2.9–6.8 ms |
| A frosted menu (Full) over a moving window (19 October) | 0.55–0.95 / 0.86–3.3 ms | 0.73–0.82 / 1.4–3.6 ms (no blur) |

The 19 October rows are from `examples/frame_bench` (`--only overview`,
`--only frost`) on a machine other builds shared (load 3 to 19): their
high p95s are other processes as much as the frames (a frame the bench
measures is wall time).

In the NixOS VM test (virtio-gpu, no 3D, so pixman on KMS at 1440×900), a
whole session — login, terminal, Settings, Calculator, Files, lock and unlock
— rendered at p50 0.4–0.6 ms and p95 1.4–3.8 ms.

## Differences and limits in software mode

- **The Desks overview** used to scale every window's thumbnail on the CPU
  each frame and blend the whole output several times a frame while it
  faded in (p95 about 20 ms at 2880×1800). Now each thumbnail is kept as a
  picture at its size, scaled again only where its window drew
  (`thumbs.rs`), and the overview's see-through pieces lying on a flat
  colour are filled as the colour they make rather than blended
  (`Painter::ground`, pixman only: under GLES the opaque pieces would only
  give the damage tracker more to cut around). A frame fading in costs
  about a millisecond; what's left over 2 ms are its opening and closing
  frames, which repaint the whole output as a full-screen client does.
  The same pictures make pixman draw the filter's dimmed thumbnails as GLES
  does (it drew them far too pale).
- **Night Light** uses the display's gamma table (CRTC GAMMA_LUT), which works
  in both modes on real hardware; a display without a gamma table (most
  virtual GPUs, headless outputs) can't show it, and Settings hides the
  switch there.
- **Mirroring** reads the source output back on the CPU in both modes.
- **Effects** cost little in either mode: Aizome's look is hard shadows,
  opaque surfaces and motion that moves layers. The Full effects tier (not
  the default) makes the shell's menus, the Deskbar, the palette and the
  notification cards translucent with a fine grain (frost), which the
  compositor blends. **Under GLES it also blurs what's behind them**
  (`blur.rs`, chosen by the logic's `frost.rs`): the frame copies what it
  holds under such a surface so far, blurs it with dual Kawase (the copy at
  half size, down to an eighth and back up, sampling 32 pixels beyond the
  surface) and draws the blur only where the surface's own pixels are at
  least 75% opaque, so the face shows a soft backdrop and the hard shadow
  and clear gaps stay as drawn. It's taken again only when something under
  it changed (an element came, went, moved or drew there); otherwise the
  frame draws the blur it kept. Under pixman the frost is as the shell draws
  it, with nothing blurred; outputs that are rotated or flipped don't blur
  either.

The desktop's windows and apps work under pixman, and the VM test runs the
whole session that way. GPU explicit sync, scanout feedback and adaptive
sync require the GLES backend.
