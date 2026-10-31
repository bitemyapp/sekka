# Recycled-row construction cleanup

The decoder and compressed-row recycling previously called `Row::new`, then
immediately replaced its freshly allocated cell and style vectors. The narrow
[patch](../upstream/row-construction/construction.patch) constructs the final
retained capacities directly. All default fields, dirty-prefix bounds, logical
charges and archive capacities remain identical. It adds no storage abstraction,
dependency, unsafe code or archive-format change.

The isolated candidate is based on
`cb91125928563e11700f4c3f2034a00300578ca6`. Source manifests differ only in
`crates/hotaru-core/src/row/history.rs`, including a regression that compares
the direct constructor against the former construction sequence across widths
and retained capacities. All **133 workspace tests**, including **109 core
tests**, and strict all-target workspace Clippy pass. Full-content comparisons
also pass for resident/compressed history, held captures across owner mutation
and destruction, and the 50 MB URI case retaining 8,291 rows in both versions.

[Validation](../upstream/row-construction/validation.json) records exact source
and artifact hashes. The isolated build used an older evidence helper whose
aggregate source digest excludes `vendor`; that digest has not been relabelled.
A separate [vendor manifest](../upstream/row-construction/vendor-provenance.json)
verifies all 29 vendored files in both trees against their exact `cb91125` Git
blobs and confirms they remained unchanged after measurement. The current
production build helper already includes vendored files in its source digest.

The five-sample [timing report](../upstream/row-construction/interference-report.json)
is explicitly **unqualified for final speed claims**. Unrelated compiler jobs
were active despite the cooperative benchmark lock; recorded load rose from
about 29 to 41 and some samples nearly doubled. The observed ASCII cold-read
median was 2.433 ms before and 2.197 ms after, but this is provisional. The full
report preserves every sample and per-case process/load snapshot. RSS remained
essentially unchanged, as expected for removal of temporary allocations.

Correctness supports integrating the small cleanup. A quiet-host comparison
of the final combined source remains required before closing the performance
gate; the noisy intermediate run is not a substitute for that qualification.
