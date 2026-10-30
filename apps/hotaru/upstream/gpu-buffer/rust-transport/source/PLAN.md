# Isolated Rust GPU transport boundary

This prototype starts with safe bounded ownership and a blocked-driver test seam,
then adds runtime-loaded EGL and an actual SCTK client. It is not a renderer or
an integrated application backend. Shared qualified product source stays intact.

## Integration map

Sidestep `backend/mod.rs::run` owns Wayland dispatch, clipboard selection pipes,
and CPU `draw_frame`. A new worker must own all EGL/GL calls, context setup,
fences and driver resource destruction. Calloop only submits immutable jobs and
receives coalesced readiness. It must not wait for the worker, including close,
resize, import failure and application teardown. No mutex spans driver calls.

`State::present` currently drains pending paints, assigns a frame sequence, calls
`draw_frame`, requests a surface frame callback and commits. A GPU path must delay
attach/commit until its exact job is ready. Superseded frames cannot discard
partial paints blindly: initial integration requires complete immutable scene
jobs or an acknowledged full-damage software fallback. Existing `Win::commit`
commits content before the synchronized toplevel parent; preserve that order.
`FrameTag` callbacks control pacing only. `wl_buffer.release` controls reuse.
`close_window` must retire committed leases independently of the removed Win.

SCTK 0.19.2 already provides DmabufState, async DmabufParams::create and
DmabufHandler::{created,failed,released,dmabuf_feedback}. Version 3 uses modifier
events; version 4+ uses surface feedback format tables and preferred tranches.
Copy at most 4096 validated format/modifier entries. Match the exported pair
before create; do not guess equivalence between primary/render device numbers.
Initial hardware qualification supports one-plane AB24/linear, flags zero and
producer row zero at the surface top. Missing capabilities fall back to SHM.

## Safe ownership module

A single event owner accepts opaque window identities and immutable Arc frame
sources. IDs increase monotonically and explicitly fail on exhaustion. Each
window has one replaceable pending job; global window count, lease count and
retained bytes are bounded. The worker reserves both source and future buffer bytes at admission, before allocation. Reservations
include rendering, ready, importing, committed and retired buffers; resize or
close does not exempt old leases. Descriptor dimensions/stride/offset are checked
against the reservation before publication.

The worker is constructed inside its thread and may contain !Send context and
resources. Only an OwnedFd descriptor crosses threads. It keeps each original
resource and source Arc until authoritative release, failed import or cancellation
before commit. Per-lease atomic flags are a lossless control path independent of
pending job capacity. A coalesced condvar wake and readiness notification carry
no unbounded per-frame messages. The actual diagnostic uses calloop Ping; its
notification does not transfer ownership. Ordinary newer output keeps a useful
completed frame eligible until a newer frame commits, so continuous input cannot
starve presentation. Explicit geometry/scale invalidation advances a monotonic
revision and rejects the prior uncommitted work.

Driver failures or panics disable further rendering for this worker. Already
committed resources are retained until release or explicit connection loss.
Dropping an event owner never joins a potentially blocked worker. Connection
loss is explicit: the adapter must disconnect/destroy its Wayland connection
before retiring outstanding leases without reuse: compositor/kernel references
may outlive our disconnect, but no new client writes or allocations are allowed.
If an owner disappears while
still committed and without that declaration, retain the finite resources rather
than reclaim early. Production adapter RAII must enforce disconnect-before-stop.

Source admission charges include a caller-provided conservative source allocation
estimate plus validated buffer allocation bytes. This is a logical bound, not a
GPU-driver RSS guarantee. Replaced pending sources are dropped outside the shared
mutex. Driver allocations, waits and resource destructors run only on the worker.

## Qualification sequence

1. Deterministic blocked-context tests: submit/resize/close/drop stay responsive;
   finite budgets persist across closed windows; frame callback cannot release;
   stale callbacks cannot match a reused ID; release/cancel is never lost; source
   identity and resource destruction thread are verified. Strict Clippy and Miri
   for the safe module where available.
2. Runtime EGL adapter uses libloading, audited typed entry points and thread-
   confined context handles. Fence polls have a deadline/cancel checks on worker.
   Arbitrary driver calls may still block: keep at most one worker, never launch
   unbounded replacements, and software-fallback without joining.
3. Rust SCTK diagnostic on the private Intel compositor repeats exact advertised
   pair/import/capture/release evidence with bounded lifecycle and cleanup. This
   remains separate from scene/glyph rendering and physical scanout qualification.


## Initial implementation and review status

The safe module has fifteen passing deterministic tests and strict all-target
Clippy. Independent review found and corrected destructor-panic and panicking
panic-payload paths that could unwind another committed lease. Each exceptional
payload is deliberately forgotten; rendering stops, so exception count is bounded
by outstanding resources/jobs. This exception does not promise a byte bound on
arbitrary caller-supplied panic payloads. Ordinary resource/source destruction
remains on the worker and outside metadata locks. Explicit requested cancellation
is separate from driver failure and permits the next valid frame.

The EGL diagnostic creates exactly one context on its worker and no external code
can make another one current there. Rc enforces thread confinement and keeps the
library/context alive, but alone cannot prove current-context identity. A reusable
production adapter must preserve this single-context rule or rebind at render and
destruction boundaries. The fixed 32×32 quadrant scene/readback is diagnostic.
The fence precedes readback, unlike the earlier C probe, and is polled in 5 ms
slices with cancellation and a five-second deadline. Arbitrary driver calls may
still block; the event owner never joins or waits for them.

Reviewed ABI/ownership references:
- https://registry.khronos.org/EGL/extensions/MESA/EGL_MESA_image_dma_buf_export.txt
- https://registry.khronos.org/EGL/extensions/KHR/EGL_KHR_fence_sync.txt

The export query accepts four modifier slots (the specification maximum), then
requires one plane before passing single-element FD/stride/offset outputs.
OwnedFd closes both failed exports and successful transfer descriptors. The
worker retains the original exported FD and GPU objects until release.
