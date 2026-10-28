# Isolated Rust worker and Wayland transport

This real Rust path passed on a private Intel hardware compositor. It does not
render the native terminal scene or replace the production Sidestep backend.

The archived standalone crate has 15 safe-lifecycle tests and two feedback tests;
strict all-target Clippy passes. Driver and resource types need not be Send: they
are created, used and destroyed on a dedicated worker. Event operations never
join it. Logical reservations include pending, rendering, importing, committed
and retired leases. Release is separate from frame pacing. Geometry invalidation
uses monotonic identities with exhaustion errors. Tests cover blocked drivers,
source/FD lifetime, cancellation, stale imports and continued display progress.

Review corrected resource/source-destructor panics, panic payloads with panicking
Drop, and the distinction between format tables and advertised tranche entries.
Exceptional payloads are deliberately forgotten after rendering fails, protecting
other committed leases. This is a bounded exceptional count, not a byte promise
about arbitrary caller-supplied payloads.

The diagnostic loads typed EGL entry points, creates exactly one context on the
worker, draws a fixed 32×32 pattern, fences, then verifies 4,096 RGBA bytes. GL/EGL
runs on ThreadId(2); the offscreen owner records ThreadId(1). Rc proves confinement
and lifetime, not currentness: a reusable adapter must preserve the one-context
invariant or rebind before rendering/destruction.

SCTK 0.19.2 admits only validated surface-feedback tranche references. It receives
an OwnedFd clone while the worker retains the original FD, texture and image.
Async params.created precedes attachment. Sway's hardware GLES capture matches all
3,072 RGB bytes in the 32×32 region. Detach leads to wl_buffer.release, then worker
resource/context destruction succeeds. This fixture requires the retained path;
earlier release is valid protocol behavior. Headless feedback is not scanout.

source/ contains the exact prototype and Cargo lock, not a production patch.
result.json records source/binary/loaded-library hashes, events and environment;
build-identity.txt records Rust1.95.0 and binary hashes. Exact copied binaries run
under the explicit Nix loader/Mesa in the task-owned unprivileged namespace. No
host permissions or desktop settings changed. capture.png directly decodes the
saved RGB PPM without resizing. Raw artifacts and earlier passing attempts remain
at /home/callen/work/hotaru-local-gui-20261028/rust-transport.

run.py preserves task paths and depends on the frozen frame-matrix889c1151
subreaper/pidfd helper and existing run-private-gpu.sh/closure. It is a diagnostic,
not an installer. Final acceptance requires every fence/resource/context cleanup
record to exist and report success. No forced descendant cleanup was needed and
all owned processes exited. Application rendering/performance remain unqualified.
