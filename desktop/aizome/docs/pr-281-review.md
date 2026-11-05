# Review of PR 281: waiting for the last cross-GPU copy

These are review notes on PR 281 at `0e0069a2`, for whoever addresses
them. Remove this file before the PR is merged.

The review was a reading of the code. Nothing was built or run. The change
was checked against the smithay 0.7.0 source that `Cargo.lock` pins.
Findings 3 and 4 were checked against the code directly. The others come
from reading and have not been reproduced.

The review started on `a01f862d`, before the PR was rebased. The rebased
commit has the same patch, and `udev.rs` is identical in both, so the line
numbers hold. Unless noted, they refer to
`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs` at `0e0069a2`.

## The core ordering is right

`MultiRenderer::wait` runs on the render (primary) GPU. The `sync` in a
`PrimaryPlaneElement::Swapchain` is the fence of the second GPU's copy.
smithay's intermediate buffer is cached per pair of GPUs
(`GpuManager::dmabuf_cache`, keyed by `(render, target)`). So the wait
orders the next draw after the previous copy, as the PR says. The findings
below are about what the wait costs, where it can fail, and what it leaves
open.

## Does the reporter have two GPUs?

The diagnosis only applies if the affected monitors are on a GPU other
than the one that draws. That has not been confirmed.

### What #277 says

A second external monitor brings artifacts and flicker to every external
monitor. One external monitor, in the built-in HDMI port, is fine on its
own. The monitors connect through a DisplayPort-to-HDMI adapter, a
USB-C-to-HDMI adapter, and the built-in HDMI port. The issue gives no
hardware details and no log.

### Where the two-GPU diagnosis comes from

The photo in #277 shows another output's wallpaper mixed into one display
in 32 px tiles. That looks like one GPU writing a buffer while another
reads it. In Aizome's code that can only happen when several same-sized
outputs are on a GPU other than the one doing the drawing. smithay draws
them all on the drawing GPU into one shared buffer, then copies each one
across.

On a single-GPU machine, `render_node == primary` (line 461), so
`single_renderer` draws each output straight into its own swapchain. The
shared buffer is never used, `copied` is never set, and this PR changes
nothing.

The PR's own test on wx-workstation (one NVIDIA GPU registered twice)
produced 0 bad frames out of 20 without the wait. That GPU runs both
contexts' work in order, so the race can't happen there. The PR says it
hasn't been tested on a machine with two GPUs.

### What #141 shows

The same reporter (socialmediarefugee) posted system details in #141:

```
Model      Apple MacBookPro11,5
Processor  Intel Core i7-4980HQ · 4 cores, 8 threads
Graphics   AMD Radeon HD 8870M / R9 M270X/M370X (Venus XT)
Graphics   Intel Crystal Well Integrated Graphics Controller
Kernel     Linux 6.18.57 · x86_64
```

If #277 comes from that machine, it fits the PR's diagnosis closely. On
dual-GPU MacBook Pros of that era, the HDMI and Thunderbolt ports are
wired to the discrete GPU, which is why macOS switches to the discrete GPU
when a monitor is plugged in. (That wiring is general knowledge about
these machines; this review didn't check it against a source.)

- **Built-in panel:** driven by the Intel GPU, which matches it being
  unaffected.
- **External monitors:** all on the AMD GPU. One works on its own; two of
  the same size share smithay's buffer and collide.

The log in #141 also shows the AMD card running on the old `radeon` kernel
driver rather than `amdgpu`:

```
Warning: ../src/amd/vulkan/radv_physical_device.c:2564: Device '/dev/dri/renderD128' is not using the AMDGPU kernel driver: Invalid argument (VK_ERROR_INCOMPATIBLE_DRIVER)
```

Sharing buffers between that driver and a Haswell Intel GPU is an old,
little-tested path, the kind where the needed implicit sync could be
missing.

### What would break that link

- **The adapter doesn't fit.** The MacBookPro11,5 has no USB-C ports, only
  two Thunderbolt 2 ports, which take mini DisplayPort plugs. "USB-C to
  HDMI adapter" doesn't fit that machine, so either the description is
  loose or #277 comes from another computer.
- **Which GPU draws matters.** The shared-buffer path is only used if
  Aizome picked the Intel GPU as the primary, which depends on the GPU the
  machine booted on. If the AMD GPU is the primary, the external monitors
  are drawn directly, and this PR changes nothing for this reporter.

### How to settle it

Ask the reporter on #277:

1. Which machine #277 happened on.
2. Whether Aizome's log shows `Found dma-copy set` with both external
   monitors plugged in. That line means the shared-buffer path is in use.
3. For each `/sys/class/drm/card*-*` connector, its `status`. This shows
   which GPU each monitor is on.

The PR's test from the other side also applies. Without the fix, putting
`AIZOME_DRM_DEVICE=/dev/dri/renderD…` (the AMD GPU's render node) in
`~/.config/aizome/environment` makes that GPU the primary. If the artifacts
go away, the diagnosis is confirmed independently.

## Bugs and risks

### 1. A hang on the second GPU now freezes everything

Line 474. The primary GPU now waits on the second GPU's copy fence. If the
second GPU hangs or resets with that fence unsignalled (an amdgpu ring
timeout, an NVIDIA Xid), the next frame on that device puts the stuck
fence into the primary's command stream. The built-in panel and all later
primary-GPU work then stall too. Before this PR, only the external outputs
stalled, because their DRM commits wait for the fence in the kernel.

On the CPU path (no native fence import, or `needs_sync` on NVIDIA older
than 560.35.3), the event loop blocks on `SyncPoint::wait()` with no
timeout: no input, no vblank handling, no IPC. A bounded wait, or skipping
a frame when the fence hasn't signalled in time, would limit the damage to
the second GPU's outputs.

### 2. Blur can still show the other monitor

`desktop/aizome/crates/aizome-comp-core/src/blur.rs`, line 715. This is on
main already, but it is the same shared-buffer problem this PR diagnoses.
When blur rebuilds a cached blur, `BlitFramebuffer` copies the blur's
whole region from the bound framebuffer. On the cross-GPU path, that
framebuffer is smithay's shared intermediate. Wherever the current frame
has no damage, it still holds another output's pixels.

For example: two same-sized outputs on the second GPU, with a translucent
panel that blurs. Its cached blur is dropped after 600 `draw_gles` calls
without being drawn (line 692; the count is per draw call across all blurs
and outputs). Later a small change, such as the clock, damages part of the
region. `recompute` is false, but `chain.region` is `None`, so the whole
region is copied from the intermediate. The parts outside the damage are
the other monitor's picture. A blurred patch of the other display shows
behind the panel and stays in the cached blur for later frames.

This PR's wait doesn't help. One intermediate per output would.

### 3. A failed wait is ignored

Line 485. `let _ = e.sync.wait();` discards an `Interrupted` error, for
example when the EGL client wait fails, and `queue()` still calls
`drm.queue_frame`. On a plane that takes no fence (legacy KMS, or NVIDIA
older than 560.35.3, where `needs_sync()` is true), the half-drawn buffer
can be shown. That is the flicker this hunk was added to prevent, and
nothing is logged. Logging the error, or skipping the queue and redrawing,
would make the failure visible.

### 4. A failed wait also drops the fence

Line 475. The fence is `take()`n before `r.wait()` runs. When the wait
fails (`make_current` fails, or the CPU fallback is interrupted), the fence
is dropped after a warning. `render_frame` then draws output B into the
shared buffer while A's copy may still be reading it, and A shows tiles of
B again. That is the race this PR fixes, coming back with only a warning.
Keeping the fence on error, or falling back to `copied.wait()` on the CPU
before giving up, would keep the ordering.

## Performance

### 5. It waits even when there's no conflict

Line 474. The wait goes into the primary's stream before anyone knows
whether this frame will draw into the shared buffer. Suppose `redraw_due`
draws HDMI (leaving copy H1 pending), then DP. DP still takes H1 and puts a
GPU wait in the primary's stream in any of these cases:

- **No primary-plane damage:** a cursor-only update, or a redraw only for
  frame callbacks.
- **Direct scanout:** a client buffer goes straight to the plane.
- **Different buffer:** DP's size or format differs, so smithay allocates
  a fresh intermediate anyway.

eDP and any screencopy drawn later in the same pass then wait for H1's
copy across PCIe. That can take milliseconds for a 4K frame, which can make
a 120 or 144 Hz panel miss vblank.

### 6. On smithay's CPU-copy fallback, the wait protects nothing

Line 487. When the two GPUs share no dmabuf modifier, smithay logs "Falling
back to cpu-copy". In that case `finish()` has already read the
intermediate back synchronously (`copy_framebuffer`, then `map_texture`)
before `render_frame` returns. The stored fence then covers only the
second GPU's upload and draw. Waiting on it guards nothing, but it
serializes both GPUs' work and adds latency to every output drawn after
it, sometimes as a CPU wait.

### 7. No `is_reached()` check

Line 475. Every frame on the second GPU calls `GlesRenderer::wait`, even
when the copy finished long ago. That costs an EGL `make_current`, a dup
of the fence fd, creating and destroying an EGL sync, and `eglWaitSync`,
or a needless CPU wait on drivers without fence import. With one external
display, or frames spaced apart by vblank, the previous copy has almost
always signalled already. Skip the wait when `copied.is_reached()` is true.

## Design and cleanups

### 8. The fix is in the wrong layer

Line 133. Aizome mirrors smithay's private buffer-cache rules (one buffer
per `(render, target)` pair, reused while size and format match) in a
per-`Device` field. The protection only works while `Udev::redraw` is the
sole caller of `gpus.renderer()` and smithay keeps caching this way:

- **Other callers:** a future cross-GPU render path, such as capture or
  screencopy straight to the second GPU, or mirroring, gets no wait.
- **smithay changes:** a change to the cache key or lifetime makes the
  per-`Device` fence wrong without any warning.

The root fix belongs in smithay's `MultiRenderer`: store the copy's
`SyncPoint` with its `dmabuf_cache` entry and wait on it in
`MultiRenderer::render` before binding that buffer, or give each output
its own intermediate. That covers every caller. One intermediate per
output would also fix the per-frame reallocation for mixed sizes and
finding 2.

Aizome can carry such a change before it lands upstream:
`desktop/aizome/Cargo.toml` (line 100) already has a `[patch.crates-io]`
section, which patches moxcms, zune-core and simdutf8.

### 9. The doc comment overstates the sharing

Line 134. The comment on `copied` says smithay draws all of this GPU's
outputs into one shared buffer. That holds only while their size and
format match. Otherwise smithay drops and reallocates the intermediate on
every frame, and the wait only adds latency. For example, with a 4K output
and a 1440p output on the second GPU, every frame allocates and imports a
new intermediate. The comment should say when the buffer is shared, and
mention the reallocation cost.

### 10. Performance stats count GPU waits as CPU time

Line 525. `Frame.cpu` is `started.elapsed()`, which now includes the
blocking waits: the CPU fallback inside `r.wait` (line 475) and
`e.sync.wait()` on the `needs_sync` path (line 485). So `cpu_us` in
`stats.rs` (line 45) counts GPU completion time as compositor CPU time. On
NVIDIA older than 560.35.3 or legacy KMS, each frame's CPU time would jump
by however long the GPU takes to finish. Performance data from those
machines, including comparisons in `desktop/aizome/docs/performance.md`, would then show a
CPU regression that is really time spent waiting for the GPU. Time the
waits separately, or stop the CPU clock before them.

### 11. The cross-GPU check is written twice

Line 487. `render_node != primary` decides whether to store the fence, and
its inverse at line 461 chooses the renderer. If the renderer choice ever
changes (for example, `single_renderer` for a display-only device with a
render node), the fence condition has to be updated separately, or it
silently stops matching the path that uses the shared buffer. Compute
`let cross_gpu = render_node != primary;` once, or tie the fence to the
renderer choice.

## Conventions

Only the root `AGENTS.md` applies to these files, and the diff doesn't
break it.
