# Independent review

The Linux I/O agent reviewed the safe reservation, SHM, worker and cross-worker
selection files without running duplicate tests. Review found and fixed:

1. Capacity retirement did not wake an event endpoint that had retried before a
   slow resource destructor completed. Reclamation and cancelled non-published
   renders now signal capacity outside metadata locks.
2. GPU startup failure reported Closed after setting stopping, preventing CPU-only
   startup. DriverFailed now takes precedence when the endpoint has failed.
3. Invalidation retired worker resource charges while event-owned imports still
   held export fds. Cancelled import/fallback phases retain leases until token drop.
4. Import destruction notified reclamation before automatic field destruction
   closed its fd. The descriptor now closes explicitly before reclamation.
5. Popping a queued Ready before acquiring its importing phase allowed the same
   stale-fd lifetime gap. Queue removal and phase acquisition are now atomic under
   the predicate mutex; stale queued descriptors remain worker-owned.

The final source rereview of `lib.rs` at
`dc0d0b8b4e1915989c56530918d242f27ecf6a0eb6ef937743161e9c028039c1`
found no remaining concrete race or ownership flaw in these mechanisms. The
review also confirmed that cancelled event tokens remain bounded through window
close and disconnection, and `next_deadline` excludes disabled GPU deadlines.

The renderer agent independently identified and corrected the memfd interop
failure with a real private compositor: WRITE sealing blocks wlroots' writable
SHM mapping. Size sealing plus trusted immutable ownership is the final contract.
An additional source audit compacted tiny-skia paths before retention, because
private builder Vec capacity cannot be measured through public point lengths.

These findings apply to this isolated boundary. They are not an endorsement of
production wiring that is not yet implemented, arbitrary extra fd clones, an RSS
bound, or hardware performance.
