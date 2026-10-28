# Isolated allocator experiment

This experiment changes only a separate replay workspace on `wx-workstation`.
The application and core library do not select a new global allocator.

The measured Rust wrapper is `mimalloc 0.1.52`, with `libmimalloc-sys 0.1.49`
and its bundled **mimalloc 3.3.2**, verified from the C header. Building it
requires a C compiler. This is distinct from the newer mimalloc release
advertised by the current upstream repository.

The [upstream documentation](https://github.com/microsoft/mimalloc) describes
purging as decommitting unused pages and documents the latency tradeoff of
`MIMALLOC_PURGE_DELAY=0`. The bundled `options.c` defaults to a 1000 ms delay.
[The Rust wrapper](https://docs.rs/mimalloc/0.1.52/) supplies the global allocator.
The experiment uses process-local environment variables only; it does not
modify host settings, call `malloc_trim` or force allocator collection.

## Results

The final comparison uses `target-cpu=native`, thin LTO, one codegen unit, CPU 8,
and seven fresh processes per history corpus. Input, retained row count and
checksums agree with Ghostty. `upstream/allocator-experiment.json` records all
samples, exact build and runtime policies, source/binary/corpus/library hashes,
and the intermediate default-policy experiments.
This measured checkpoint precedes the retained-width recycling fix; its hashes
and samples are preserved. Current compressed storage additionally accounts for
two bytes of recycling metadata per row. These are historical measurements,
not a claim that the final application build has been benchmarked.

With **`MIMALLOC_ALLOW_THP=0` and `MIMALLOC_PURGE_DELAY=0`**:

| Corpus | Rust cold RSS | Ghostty cold RSS | Rust full-history read | Ghostty full-history read |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 4,400 KiB | 4,664 KiB | 2.85 ms | 1.10 ms |
| Styled | 10,252 KiB | 15,116 KiB | 6.37 ms | 9.25 ms |
| Unicode | 4,396 KiB | 5,692 KiB | 4.00 ms | 1.93 ms |
| Mixed | 4,444 KiB | 4,836 KiB | 2.74 ms | 1.41 ms |

Both options matter. Default delayed purging does not reduce immediate cold
RSS. Immediate purge with transparent huge pages enabled retains a larger
process baseline. Disabling transparent huge pages here affects this process,
not system configuration.

A separate 31-iteration no-history throughput comparison, using the same
native build, measured this allocator policy against Rust's system allocator:
ASCII 0.560 versus 0.563 ms, styled 21.489 versus 22.215 ms, Unicode 4.488 versus
4.779 ms, and mixed 0.557 versus 0.562 ms. These short measurements do not prove
an application-wide throughput improvement. The same corpus still exposes
Rust/ Ghostty throughput differences independent of allocator choice; all raw
reference samples are retained in the report.

## Recommendation

This allocator policy is worth testing in the native application before
building a custom mmap-backed row allocator. It demonstrates actual RSS
reclamation and lower cold RSS for all four measured corpora with no manual
collection. It does **not** establish overall Ghostty performance parity:
three full-history restore measurements remain slower, and desktop rendering,
cross-thread frees, idle latency and sustained PTY throughput still need
application-level measurements.

Immutable shared compressed-block snapshots should precede moving large
copy/search work off the PTY worker. That addresses the current eager
`snapshot_all` boundary without changing row semantics. Keep the allocator
experiment isolated until native validation supports a product policy; do not
replace an explicit memory-ownership design with a claimed allocator guarantee.
