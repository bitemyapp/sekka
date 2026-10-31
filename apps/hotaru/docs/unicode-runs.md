# Positive-width printing experiment

This candidate was measured in isolated workstation workspaces based on exact
checkpoint `1d3c9cf4df2744e6a714d2bea18a31d90f5ddc7a`. It has not been applied to
the shared core by this experiment. The reviewable change is
`upstream/positive-width-runs.patch`; all reports are under
`upstream/unicode-runs/`. The work uses safe Rust and adds no cell/row fields.

## Measured cause and chosen change

Controlled native builds separated a last-scalar width cache, bounded temporary
write batches, streaming batches, and reduced inner-loop bookkeeping. All
compared the same final grid/effects to the pinned Ghostty C API before timing.
On approximately 2 MiB of repeated wide text, the width cache alone reduced
feed time by 13.6%; batching overwrite/style work reduced it by 42.8%. The
selected streaming implementation reduced dense-wide time by 54.1% and mixed
Unicode by 40.3% against the exact checkpoint. These are isolated component
experiments rather than inferred profiler samples; hardware perf access is
unavailable on this workstation. Raw samples and build identities are retained
in `profile*.json` and their referenced build manifests.

The selected implementation interns a style and clears overwritten metadata
once per positive-width run. Runs stop at combining marks and row boundaries.
The left wide-glyph boundary is captured before writes; the right neighbor is
untouched until the shared boundary-repair helper runs. Interior cells can be
replaced before clearing their old metadata because the row is exclusively
mutable and no snapshot observes that intermediate state. Exterior wide halves,
graphemes, hyperlinks and dirty-prefix accounting still use the existing
helpers. Insert mode, character sets, combining and wrapping keep their scalar
handling. The candidate also fast-paths broad CJK ranges whose width is constant;
an exhaustive test compares every valid Unicode scalar to the prior policy.

This patch must be merged with the separate memory-accounting change. Cached
charge invalidation must remain in the common metadata-clearing helper, not
only its `prepare_write` wrapper. `style_id`, grapheme removal and hyperlink
range mutation continue to own their respective allocation invalidation.

## Final validation and timings

The final source passes 71 core tests, the replay's upstream fixture test and
strict all-target Clippy. This includes 1,792 scalar-versus-bulk operations over
seven geometries, with long Unicode runs, combining marks, invalid UTF8,
overwrites, styles/protection, hyperlinks, wrapping, scrolling, resize and held
snapshots. Existing reflow, ownership, guard-page, input, format and terminal
oracle suites also pass. The width regression covers all 1,112,064 Unicode
scalars. `validation.log` retains the actual output.

Final native feed ratios are Hotaru time / Ghostty time; lower is faster:

| Workload | Ratio |
| --- | ---: |
| Existing ASCII pattern, 8 MiB | 1.067 |
| Existing mixed Unicode pattern, 8 MiB | 0.723 |
| Existing styled pattern, 8 MiB | 0.576 |
| Existing cursor pattern, 8 MiB | 0.102 |
| Dense wide Unicode, approximately 2 MiB | 2.225 |
| Dense wide Unicode with persistent 2,031-byte URI | 0.126 |
| Longer ASCII-line diagnostic without link | 1.358 |
| Same ASCII payload with persistent URI | 0.060 |

These tests use identical dimensions/chunking per pair, zero history, one pinned
CPU, native optimized builds, three warmups, alternating process order and
twenty feed-only samples per engine. Startup, input reading and teardown are
separate. The link diagnostic additionally checks every visible URI and exact
VT link transitions. Binary, source, tool and compiler identities are captured
in `candidate-build.json` and each report. This candidate's source digest is
`136c7a8fbbc8514172136d4b8f15f082aa7cebc164198404c46585bf544d26bc` and replay
binary digest is `dcfa8fdb9c6a5e38e169a4e16210c18de22f81f6957e49ce67932aa09739a0fd`.

Dense-wide text is still over twice the reference time, and the ASCII gap
remains. Whole-app performance parity has not been achieved. Measurements are
for this isolated patch before memory-budget/search integration and must not
be relabeled as measurements of a later merged binary.

## Reproduction

Archive `apps/hotaru` from the checkpoint into a new workstation directory,
apply `upstream/positive-width-runs.patch` at the Sekka repository path level,
and copy `tools/hyperlink-probe.rs` into the isolated core `examples/` directory
for the diagnostic probe. `tools/build-candidate.py` records the exact native
build; use the same environment for the probe. Run `cargo test -p hotaru-core
-p hotaru-replay` and strict Clippy, then `tools/benchmark.py` for the four exact
`tools/corpus.py` patterns and `tools/hyperlink-benchmark.py` for the dense/link
streams. All heavy work and measurements use the shared workstation lock.
`tools/unicode-experiment.py` and `tools/unicode-profile.py` reproduce the
controlled alternatives; the final reviewed patch includes additional comments
and the scalar-regression test, so its source hash is intentionally distinct.
