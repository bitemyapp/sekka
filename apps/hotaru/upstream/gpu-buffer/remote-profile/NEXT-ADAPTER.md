# Bounded native adapter boundary, after phase profiling

This is an isolated source plan, not a claim that native GPU fallback is wired.
The phase probe has no Wayland event loop. Its dedicated worker is joined only
by the diagnostic main for process completion. Production EventOwner::drop still
never joins or waits for the driver.

## Implemented in this remote candidate

- Preparation has a recoverable Cancelled outcome, checked before work, per op,
  glyph, tile row/prefix batch and 256-pixel filtered-asset chunk. Font loading or
  a single glyph/small-path raster call remains an indivisible worker-only call.
- Atlas admission validates every retained identity and complete padded-byte
  total before any upload. Per-scene and persistent cache counts are capped at
  8192. Scene reader rejects duplicate asset IDs. Metadata cannot accumulate for
  millions of tiny assets across frames.
- Driver staging checks cancellation every 16 KiB of asset bytes, each asset,
  and 256 commands. Driver calls themselves cannot be forcibly timed out; they
  stay on a dedicated context thread. Existing lease tests prove that a blocked
  driver cannot make the event endpoint wait during close/disconnect.
- Whole-scene structural/budget rejection precedes GPU work. There is no partial
  GPU surface. Offscreen profiling returns no Descriptor and cannot enter the
  event-side import path, even when an export happens to succeed.
- Contiguous two-pass tile indexing preserves per-tile command order, checks
  total expansion before allocation, and has bounded cancellation points.

## Native connection that remains to implement

Sidestep backend/mod.rs currently owns both protocol/clipboard dispatch and
SHM rasterization. Its State::draw_frame consumes incremental Pending paints
and scrolls; those do not constitute a complete latest scene. Full-frame GPU
admission must capture a complete immutable operation snapshot before those
increments are discarded, or explicitly decline the job and keep SHM. Replaying
only recent incremental paints into a new texture is incorrect.

A submitted job needs window/frame/geometry identity, immutable original ops,
source retention charge, output extent/scale and full damage intent. Keep one
active plus one latest pending job per window and global lease limits. Render
and preparation run off the event owner. A coalesced calloop wake only signals
completion; it performs no driver work. Existing import controls reject stale
geometry and late results before actual attach.

Recoverable preparation rejection needs a bounded handoff token retaining the
original job and its reservation until the event adapter either schedules full
SHM rasterization or discards it as stale. Cancellation is neither a fatal GPU
error nor a request to draw obsolete content. The SHM handoff must revalidate
window, frame and geometry immediately before attachment; newer submitted
frames can replace pending work without starving already completed progress.

Fatal driver initialization/render/import failure disables further GPU admission
for that backend instance and wakes the event owner to schedule full SHM work.
Already committed GPU leases remain in the wl_buffer table until authoritative
release/disconnect; fallback does not free or overwrite them. Resource Drop and
context teardown stay on the worker. If it is permanently blocked, the event
side remains responsive and must not launch unbounded replacement workers.

Reserve a fixed driver/cache/staging allowance before admitting leases. The
current 128MiB lease-only setting is not a global memory bound: CPU atlas assets,
32MiB GPU atlas, command/tile/index SSBOs, staging vectors, source metadata and
optional diagnostic readback all consume additional storage. The configured
allowance must count old/retired and current allocations during resize and pool
replacement. Pixel counts are logical charges, not driver RSS guarantees.
Remove readback/reference images from production; retain exactness in tests.

Successful native qualification requires blocked-driver clipboard/read-event
responsiveness; unsupported op and allocation rejection -> complete SHM output;
latest-state geometry validation; hidden/closed-window cancellation; multiple
surfaces and popups; import failure; delayed release across resize; and exact
full/damaged/scroll pixels. No CPU raster-upload of a complete frame should be
substituted and called GPU rendering.
