# Direct immutable decode: measured rejection

The [reviewed proposal](history-direct-decode-proposal.md) was implemented only
in `/home/callen/work/hotaru-page-decode-isolated`, locally and on the workstation.
It passes correctness checks but is **rejected for integration**: cold reads
regress in every measured workload, without a useful cold-RSS reduction.

The implementation uses a shared checked header parser, a bounded framing pass,
one private cell mapping and a shared semantic row decoder. Metadata remains in
`DecodedRowParts` until every row validates and all mutable mapping borrows end.
Only then does read-only publication create final rows. No unsafe code was added
to the previously reviewed owner. Original archive capacities and logical charges
are preserved. Map/protection failure drops attempted metadata and retries heap
row decoding with unchanged row bytes and the original post-dictionary budget;
the already-owned dictionary is charged exactly once.

The framing pass can reserve at most 4 MiB of cell scratch before later logical
row-budget rejection. This is not a zero-allocation guarantee for a zero row
budget: raw decompression and small layout vectors are also bounded scratch,
separate from the decoded logical-row charge. Zero-row and zero-width blocks
remain invalid as in the original decoder.

All **115 core tests** and strict Clippy pass. Added tests compare heap/page
acceptance and exact reconstructed archives, capacities and charges, including
10,000 mutated two-row archives, all truncations, late semantic corruption,
exact budget thresholds, map/protect failure, maximum cell layout and captured
row teardown/COW. Three small decoder tests pass with strict-provenance Miri in
both default and Tree Borrows models. The maximum OS-mapping case passes natively
and is ignored under Miri, where the same builder/owner logic uses heap backing.
An independent source review found no concrete acceptance, ownership or budget
defect. [Validation and hashes](../upstream/page-decode/validation.json) and the
[patch](../upstream/page-decode/direct-decode.patch) are preserved.

Five fresh-process samples alternate three engines on CPU 8 under the shared
workstation lock: prior uniform owner, refactored decoder with heap cells, and
the same refactored source with direct mapped cells. The heap control separates
constructor cleanup from mapping effects. All cases pass complete content and
equal-retention checks, including the 50 MB URI case and captures across owner
mutation/destruction. Exact commands, source manifests, binary hashes, load
snapshots and samples are in the [report](../upstream/page-decode/report.json).

| Cold history read | Prior owner | Refactored heap | Direct pages |
| --- | ---: | ---: | ---: |
| ASCII, 7,977 rows | 2.615 ms | 2.271 ms | 3.598 ms |
| RGB styles, 7,977 rows | 4.955 ms | 4.016 ms | 5.885 ms |
| Wide/combining, 7,977 rows | 3.744 ms | 3.437 ms | 5.316 ms |
| Four long URIs/row, 2,977 rows | 1.927 ms | 1.800 ms | 2.607 ms |
| 50 MB URI stress, 8,291 rows | 5.790 ms | 5.470 ms | 7.953 ms |

Direct mapping adds mapping/protection/unmap operations and a framing pass for
each restored block. Only one small restored block is ordinarily live during
iteration, so cell vectors did not create a large retained-RSS problem there.
The data establishes the regression, without attributing an exact fraction to
each added operation.

The useful result is removal of temporary constructor allocations. The old
decoder constructs a default `Row` and immediately replaces its cell and style
vectors with capacity-preserving allocations. A separate current-source
candidate can construct those final vectors directly, without introducing any
mapped storage, new dependencies or archive-format changes. That smaller change
must earn its own current-source validation and measurements.
