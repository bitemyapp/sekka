# Direct Ghostty measurements

Both directories preserve raw case JSON, full runner metadata and host observations (gzip), compact summaries, all samples and per-round ratios. No raw results were edited or trimmed. `direct-ghostty-80rows/` matches the prior grapheme-charge table's dimensions and exact data; `direct-ghostty-12rows/` is a separate smaller-grid workload.

The source workspace is `/home/callen/work/hotaru-ascii-cell-store-20261028` on wx-workstation. The archived scripts in `tools/` run there against the identified native binary and metadata; they verify source, binary and reference hashes, take the shared workstation lock, run a preflight, observe host activity, and invoke the established benchmark with alternating engine order. The nested benchmark uses a separate lock because the outer script owns the shared lock throughout.

The 80-row corpus can be reproduced with `tools/corpus.py DEST --bytes 8388608`; the expected hashes are asserted in `tools/direct-ghostty-80rows.py`. The smaller corpus uses each archived pattern repeated exactly `data_bytes / len(pattern)` times, as recorded in each raw report. Rebuilding requires the archived complete source/vendor/compiler manifests and original build helper; do not run the workstation's native binary on a different CPU.

`prior-comparison.json` checks protocol identities against the older raw reports. Unchanged Rust replay timing source and reference adapter exclude an accidental move of setup/teardown into feed timing. The original table had a different candidate source and unrelated host compilation, so the comparison does not identify the cause of every cross-window ratio change. Periodic process/frequency observations are evidence of the observed window, not proof of perfect isolation.
