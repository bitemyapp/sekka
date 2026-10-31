# Remote GPU scene phase investigation

The workstation NVIDIA GPU renders the same 18 recorded native scenes exactly
in offscreen mode. It still cannot export a DMA-BUF under the SSH account:
the explicit export attempts return `EGL_BAD_MATCH`. This investigation does
not qualify native GPU presentation or change the frozen Intel result in
`d8b63a7`.

The [compact evidence](../upstream/gpu-buffer/remote-profile/README.md) contains
reproducible source deltas, exact binary/source/loader identities, full samples
in compressed JSON, test logs and independent review scope. All builds, tests
and profiling runs are remote on wx-workstation under the shared task lock.
No laptop workload was launched.

## Findings

The initial NVIDIA attempt could not find a DRM render-node device string. The
diagnostic now explicitly selects EGL device index 0 and records the renderer:
NVIDIA RTX PRO 6000 Blackwell Max-Q, GLES 3.2, driver 580.159.04. This selection
does not grant device-file access or prove export support.

Each phase gate runs 21 repetitions of every scene, both with an export attempt
and with export explicitly skipped: 756 renders, all RGBA bytes equal. All
fence, resource, cache and context cleanup records must exist and report
success. Offscreen mode returns no descriptor to an event owner, even if a
future machine permits export. The actual presentation path still requires a
validated successful export.

Driver instrumentation separates asset staging/upload, metadata staging/upload,
texture allocation/binding, dispatch submission, EGL image creation, descriptor
export, fence creation, fence waiting/destruction, readback and CPU verification.
These are CPU wall-clock intervals around API calls, not independent GPU timer
queries. Submission time alone is not GPU execution time. The completed fence
delimits producer completion before the diagnostic readback.

Initial warm 1920×1328 Find observations illustrate why these distinctions
matter: about 0.17 ms reaches the producer fence on NVIDIA, then approximately
0.57 ms reads back the image and 2.62 ms compares the bytes on CPU. A production
renderer would omit that readback and reference comparison. The prior Intel
4–10 ms dispatch/export/fence observations remain unexplained by this
different device; they must not be replaced with the NVIDIA figures.

CPU preparation was the larger actionable cost in this prototype. Its original
tile index allocated a separate Vec for each intersected tile. A contiguous
two-pass index first counts intersections, builds disjoint prefix ranges and
then fills them in command order. It checks the same four-million-index limit
before allocation. Every tile retains exactly the same ordered commands.

| Find scene | Original tile build | Contiguous tile build | Current CPU full raster |
| --- | ---: | ---: | ---: |
| 960×664 | 75 µs | 11 µs | 234 µs |
| 1200×830 | 133 µs | 16 µs | 280 µs |
| 1920×1328 | 322 µs | 42 µs | 519 µs |

These are warm medians from sequential diagnostic runs, not an accepted paired
benchmark. The raster uses the existing banded CPU implementation, cached glyphs
and the exact same full-frame pixels. Clearing the transparent destination is
included; conversion to byte output and verification are excluded. The first
profiling fixture incorrectly reused popup destination alpha without clearing;
its exact-pixel assertion rejected the result and the fixture was corrected.
No tolerance was relaxed.

The preparation fixture checks all 18 recorded RGBA files from the hardware
experiment byte for byte, then checks every repeated CPU/prepare result.
Three hundred seeded rectangle sets additionally compare the contiguous index
against the original per-tile algorithm. Eight preparation tests pass, including
cancel/retry and cancellation within one large tile expansion. The standalone
worker/reader/atlas suite passes 19 executions and strict selected-target Clippy.

## Cancellation and admission

The isolated candidate checks preparation cancellation before work, per operation
and glyph, per tile row, in prefix batches and every 256 filtered pixels. Upload
staging checks per asset, every 16 KiB and every 256 commands. A cancelled
preparation returns no scene; a subsequent valid job still succeeds. A single
font load, glyph raster, small-path raster or driver call remains indivisible
and worker-only. No timeout can forcibly preempt a blocked graphics driver;
the event endpoint must remain responsive without joining it.

The GPU atlas now has an explicit 8,192-entry limit in addition to its byte cap.
Complete padded-byte admission and retained identity checks happen before GPU
uploads; the reader rejects duplicate asset IDs. The phase prototype still has
one known admission mismatch: preparation and file validation charge raw bytes,
whereas the atlas charges four-byte padded bytes. Near the 32 MiB boundary,
a scene can pass preparation and then be rejected by the driver. Independent
review supplied a concrete 8,190-asset case. This is safe rejection, but the
current worker classifies it as fatal. The next admission/fallback implementation
must unify padded totals/counts and make scene-limit rejection recoverable.

## Next native boundary

Sidestep's backend currently receives incremental paints and pixel moves. They
cannot be replayed into an empty texture and called a full scene. Native GPU
admission needs a retained complete operation snapshot or must decline the job
and keep the SHM path. The original frame and geometry identity must survive
preparation, import and fallback validation.

A bounded recoverable fallback handoff must retain the original full job and
its reservation until SHM rendering or stale-job discard. Fatal EGL failure
disables new GPU work, but must not free committed buffers before release.
Cache, SSBO and staging allowances must be reserved separately from old/current/
retired frame leases. The present lease setting is not a total GPU/process
memory bound. These requirements are specified in
[NEXT-ADAPTER.md](../upstream/gpu-buffer/remote-profile/NEXT-ADAPTER.md);
the native implementation and private remote compositor checks are the next
isolated slice.

No conclusion here establishes a hardware application speedup, damaged-frame
parity, physical scanout, or equivalence to Ghostty's GPU renderer.
