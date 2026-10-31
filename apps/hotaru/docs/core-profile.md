# Core instruction-count diagnostic

The [Callgrind evidence](../upstream/core-profile/) identifies work to investigate
in the combined VT/SIMD checkpoint. It measures simulated instructions, not
native elapsed time, CPU cycles or performance parity.

The profiled source manifest is
`a3e74ef33281a79744da2fa15e63d019699950490bbcc94eef783997391ca128`, identical to
the [combined checkpoint](links-vt-checkpoint.md). The diagnostic executable
uses `target-cpu=x86-64-v3`, optimized release code and debug information;
its SHA-256 is
`749ca4640420ab2e0f869d87458c4cc38beca91f98209d2e71999a71c64fca00`.
The build metadata records all compiler settings. The regular native benchmark
and Nix GUI executables have different identities.

Valgrind 3.27.1 rejected an AVX-512 instruction in the ordinary native build
before terminal execution. That partial profile is invalid and its error log
is retained. The separately compiled v3 executable ran successfully without
changing host permissions, CPU settings or the application source. Different
instruction selection limits extrapolation to the native build.

One 2 MiB mixed Unicode input, 120 columns × 12 rows, Unicode grapheme mode,
65,536-byte chunks and zero history recorded 307,252,265 instructions including
process initialization and teardown. The annotated report groups inlined code
by source location. `print_utf8` and its scalar/grapheme paths dominate. Full-row
memory-charge recomputation accounts for about 8% across its inlined locations;
standard UTF-8 validation accounts for about 6.5%. The latter is consistent with
short control-delimited spans taking the SIMD dependency's scalar fallback.

These observations motivate testing whether grapheme operations can report
unchanged allocations without recomputing the whole row charge. Any such
optimization must preserve capacity accounting, allocation order and budget
peaks, and pass equal-retention/history checks. Counts alone do not justify
integration: native paired timing remains necessary, including workloads that
do not exercise the changed path.

The updated `tools/unicode-profile.py` accepts an explicit CPU and records
host load, a process-name/CPU snapshot and available CPU-frequency readings
before and after each workload. This supplements the shared workstation lock,
which cannot exclude unrelated host activity. Earlier reports retain their
original schema and measurements; missing host observations are not invented.
