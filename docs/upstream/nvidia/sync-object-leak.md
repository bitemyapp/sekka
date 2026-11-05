# NVIDIA: every sync object leaks a few bytes

| | |
| --- | --- |
| Driver | 615.78.08 (`libnvidia-eglcore.so.615.78.08`), RTX 5090 (10de:2b85), Linux 7.3.1 |
| Reproducer | [`fenceleak.c`](fenceleak.c): EGL device platform, a surfaceless GLES 3 context, nothing else |
| Sekka's workaround | Aizome's compositor draws textures without GL fences: `gles_renderer` in [`backend/mod.rs`](../../../desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs) |
| Status | Not sent. The driver is closed: the report below is for NVIDIA's Linux forum (with `nvidia-bug-report.sh`'s log) or linux-bugs@nvidia.com |

## What goes wrong

Creating and deleting a sync object grows a heap array inside
`libnvidia-eglcore` that is never shrunk or reused. A GL sync
(`glFenceSync` + `glDeleteSync`) costs about 16 bytes, 28 after GPU work.
An EGL sync (`EGL_SYNC_FENCE` or `EGL_SYNC_NATIVE_FENCE_ANDROID`) costs
nothing on an idle context and about 16 bytes when it follows GPU work.
Waiting for the fence to signal before deleting it changes nothing. The
array grows 1.5 times at a time; past glibc's mmap threshold it is an
anonymous mapping of its own that only ever grows.

A Wayland compositor fences every frame, and smithay's GLES renderer also
fences every draw that reads a texture. On runnymede, Aizome at 3840×2160 and
240 Hz grew this array to 410 MiB in 50 hours (7.4 million frames, about
25 bytes a frame), 300 MiB of it swapped out.

## How it was found

- The compositor's 410 MiB mapping had a write head advancing 22–28 bytes per
  rendered frame and nothing written after it (pagemap, 2026-11-05).
- `perf record -e page-faults:u -d --call-graph dwarf --user-callchains`
  on the live compositor: all 92 faults in that mapping in two minutes came
  from `libnvidia-eglcore.so.615.78.08+0x9ff1dd`, called from `+0xa00cd5`,
  under smithay's `TextureSync::update_read` (67), `EGLFence::create` (21),
  Aizome's blur (4) and a texture upload (1).
- A standalone smithay renderer loop faults at the same two offsets, and grows
  with the number of fences per frame, not with targets (dmabuf or
  renderbuffer), fence export, context priority or EGL platform (GBM or
  device).
- `fenceleak.c` reproduces it without smithay or a window system. Mesa 26.2.6
  (radeonsi on the Radeon 610M, llvmpipe) runs the same program flat.

`fenceleak.c` on 615.78.08, bytes of RssAnon per iteration:

| Iteration | Bytes |
| --- | --- |
| `glFenceSync` + `glDeleteSync` (5 million) | 15.5–16.2 |
| the same after clearing a 64×64 renderbuffer | 28.5 |
| `eglCreateSync(EGL_SYNC_FENCE)` + `eglDestroySync` | 0.0 |
| the same after a clear | 14.4 |
| `EGL_SYNC_NATIVE_FENCE_ANDROID`, `glFlush`, dup the fd, close it, destroy | 0.0 |
| the same after a clear (3 million) | 13.1–15.8 |
| a clear and `glFlush`, no sync object (control) | 0.2 |

## The report

> **EGL/GLES: each sync object leaks 16–28 bytes in libnvidia-eglcore (615.78.08)**
>
> Creating and deleting sync objects grows process memory without bound:
> about 16 bytes per `glFenceSync`/`glDeleteSync` pair (28 if the context did
> GPU work since the last sync), and about 16 bytes per `eglCreateSync`/
> `eglDestroySync` that follows GPU work (`EGL_SYNC_FENCE` and
> `EGL_SYNC_NATIVE_FENCE_ANDROID` alike). The memory is a heap array in
> libnvidia-eglcore (writes at `+0x9ff1dd`, called from `+0xa00cd5`) that
> grows by half each time and is never shrunk or reused. Waiting for the
> sync to signal before deleting it doesn't help. Mesa doesn't show it.
>
> Wayland compositors create at least one sync per frame. A smithay-based
> compositor at 3840×2160 240 Hz reached 410 MiB in this array after 50
> hours.
>
> Reproducer attached (`fenceleak.c`: EGL_EXT_platform_device, a surfaceless
> GLES 3 context; `cc -O2 -o fenceleak fenceleak.c -ldl`):
>
> ```
> $ ./fenceleak 5000000 0 gl
> EGL NVIDIA 1.5; GL NVIDIA Corporation OpenGL ES 3.2 NVIDIA 615.78.08
>   500000 pairs: RssAnon 18308 kB (+16.2 bytes per pair)
>  2500000 pairs: RssAnon 48720 kB (+15.7 bytes per pair)
>  5000000 pairs: RssAnon 86008 kB (+15.5 bytes per pair)
> $ ./fenceleak 3000000 0 egl-native work
>   900000 pairs: RssAnon 24260 kB (+15.8 bytes per pair)
>  3000000 pairs: RssAnon 55120 kB (+15.3 bytes per pair)
> $ ./fenceleak 1000000 0 flush work
>  1000000 pairs: RssAnon 10580 kB (+0.2 bytes per pair)
> ```
>
> Expected: deleting a sync object frees what creating it took.
>
> Driver 615.78.08, RTX 5090, Linux 7.3.1, libglvnd 1.7.0. The same happens
> on EGL_PLATFORM_GBM_KHR and EGL_PLATFORM_DEVICE_EXT, at any context
> priority.

## Sekka's workaround

smithay's GLES renderer puts a GL fence after every texture upload and every
draw that reads a texture, so that a context sharing the texture knows when it
is free. Aizome's compositor shares no contexts and draws on one thread, so
it builds its renderers without `Capability::Fencing`. That leaves one EGL
fence per frame, which DRM waits on (`IN_FENCE_FD`). Headless on the RTX 5090
with two clients redrawing every frame, the array grew 39 bytes a frame
before and 14 after (faults at `+0x9ff1dd` over nine minutes, about 31,000
frames each). Once a driver without the leak reaches Nixpkgs, the workaround
can stay: the fences it drops did no work.
