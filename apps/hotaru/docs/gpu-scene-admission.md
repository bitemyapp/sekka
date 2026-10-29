# Complete-scene admission and CPU fallback prototype

This isolated prototype gives the GPU and CPU paths a shared source identity,
bounded ownership, and an independent CPU fallback worker. The packaged terminal
still uses its qualified CPU/SHM renderer. The prototype starts from the historical
153-sprite `3d10197` scene fixture tree plus the separately recorded remote
preparation/index changes; it needs rebasing before product integration.

The implementation and compact evidence are archived under
`upstream/gpu-buffer/admission/`. Raw builds, captures and intermediate failures
remain at `wx-workstation:/home/callen/work/hotaru-gpu-admission-isolated`.
All builds and tests ran on the workstation.

## Ownership and progress

`CompleteScene` accepts an explicit full-damage recording. It retains original
native drawing operations, so GPU refusal can rasterize the entire scene from
its original source. Validation and compact path copying run on a preparation
worker. Incremental Paint messages and pixel moves cannot establish this contract.
Unresolved composite trees, nested groups, path masks/shadows, encoded images and
recursive image parts return an explicit admission refusal for the existing
backend. Ordinary chrome and all 18 recorded terminal scenes are admitted.

Each renderer has one worker and one coalesced pending job per window, with global
window/lease limits. GPU calls, fence waits, CPU rasterization, allocation and memfd
writes stay on workers. The event endpoint does not join a worker. GPU timeout
stops further submissions to that worker and uses the independent CPU worker;
there is no stream of replacement GPU threads.

Both routes retain the original surface, geometry revision and scene sequence.
The commit gate rejects stale geometry, closed surfaces and any result older than
or equal to the last committed original scene. Renderer-local frame IDs are never
compared across workers. The event adapter has a single `next_deadline()` for
bounded timeout/capacity retry; worker completion and reclaimed capacity signal
a coalesced wake outside metadata locks.

Cancelled event-owned imports retain their resource, byte charge and lease slot
until the token drops. Import destruction closes its export fd before allowing
worker reclamation. Queue ownership and the transition to importing are acquired
under the same metadata lock; stale queued descriptors remain worker-owned and
close before their resource charge retires. Committed resources survive window
close until authoritative buffer release or connection loss.

## Storage accounting

One shared RAII budget counts retained source captures, CPU staging and memfd
storage, fixed worker cache/staging allowances, and GPU output reservations.
Cloning an owner shares its charge. Old committed buffers, cancelled imports and
new resize buffers remain charged together. Storage drops before its reservation.
The `Reserved` wrapper admits fixed GPU storage before driver construction and
output storage before entering the driver.

Admission bounds include 65,536 operations, 64 MiB of retained source charge,
32 MiB of RGBA framebuffer storage, and dimensions no greater than 8,192 per axis.
Path objects are compact-cloned: tiny-skia hides spare Vec capacity, so retaining
an arbitrary original builder allocation could defeat a length-only estimate.
Other source slices expose their length, and gradient/dash Vec capacities are
included. The GPU lowerer charges four-byte-padded asset storage, at most 8,192
assets and 32 MiB total, before publication; raw-byte admission was insufficient
for many odd-sized Alpha8 assets.

The CPU worker reserves a conservative fixed allowance for its scratch/image/glyph
state, uses an explicitly limited thread-local image cache, and clears owned
thread-local caches before releasing that allowance. The existing process-global
font/glyph registry is separate baseline state. These are logical ownership
charges, not an RSS promise or a measurement of graphics-driver internal storage.
Broader font/cache workload qualification remains necessary before integration.

## SHM compatibility finding

CPU rasterization fills a transparent staging buffer, converts premultiplied RGBA
to Wayland ARGB8888, and writes a memfd before publication. The file is sealed
against growing and shrinking, and against changing its seals. The trusted
protocol adapter never writes or reuses it while committed.

The first real compositor attempt used `F_SEAL_WRITE`. Sway/wlroots maps SHM pools
writable, so the server rejected that file with a memory-mapping protocol error.
That failed attempt is retained. Size seals are compatible; immutability is an
ownership/protocol invariant. The exported O_RDWR fd does not provide a
kernel-enforced write ban, and adapters must not retain unaccounted extra fd copies.

## Qualification scope

The worker suite covers blocked GPU calls, continuing CPU work and event-side
close, 1,000 coalesced replacements, fixed/output/staging reservations, failed
GPU startup, scene/protocol refusal, stale GPU and SHM completion, resize, closed
surfaces, held imports across invalidation/disconnection, capacity wakeups,
identity exhaustion and destructor-panic containment. A bounded unsupported blend
uses the original complete scene for CPU fallback.

All 18 historical native scenes compare every RGBA byte through the CPU fallback
worker, including day/night chrome, Find, selection, sprites, CJK and color emoji
at scales 1, 1.25 and 2. The private Wayland fixture presents the 1,200 × 830 Find
scene in a 1,280 × 960 headless Sway pixman output and compares all 2,988,000 RGB
bytes at origin (40,65). Alpha is checked in the worker reference; the captured
scene is opaque. Detach, buffer release, connection shutdown and process cleanup
are recorded separately.

GPU unavailability is simulated after actual scene preparation in this bridge
fixture. Actual Intel GPU composition/presentation and NVIDIA offscreen profiling
remain their separately frozen experiments. This test is evidence for full-frame
CPU fallback and SHM transport, with no new GPU performance or presentation claim.

The final native build uses a dedicated Cargo target. A shared-target attempt
reused an incompatible Sidestep artifact from another isolated mirror and failed
compilation; that result was not accepted as qualification.

## Remaining integration work

The live UI needs an explicit complete-recording mode and bounded capture handoff;
validation must stay off its lock. The backend must connect the two worker wakes
and one retry timer to calloop, import GPU/SHM descriptors with bounded pending
protocol state, and preserve normal chrome/subsurface ordering. The actual EGL
scene driver still needs this admission wrapper, an output path without diagnostic
readback/reference comparison, and recoverable import/export refusal. Production
continues through the existing renderer until those adapters and combined runtime
regressions pass.
