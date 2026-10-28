# Independent review of REP and saved terminal modes

The reviewed slice adds REP, save/restore slots for the 24 currently supported DEC modes, fixed-reset mode reports, and corrected 47/1047/1049 alternate-screen transitions. The implementation was compared with pinned Ghostty revision `6ba144158577d033ccb6ed4b574e4013957c9161`, including its stream, terminal and mode-state code.

Three concrete defects were found and corrected before qualification:

* Encoded U+0080–U+009F controls were forwarded to printing. Legacy width handling attached them to the preceding grapheme; Unicode mode printed them and changed REP's previous scalar. Ghostty ignores these decoded controls before invoking its print handler. The correction ignores them before consuming a pending character-set shift or modifying previous-character state, and makes bulk runs yield to that scalar dispatch. Raw invalid 8-bit bytes remain a separate parser case.
* 1049 clears the destination alternate screen before copying the source cursor. Consequently the clear uses the alternate screen's prior pen background. The previous implementation used the current source-screen style. Each screen now remembers its inactive style, and 1049 chooses the destination pen before switching. Repeated 1049 on an already active alternate screen and 1047 on exit use the current pen.
* Legacy soft-hyphen width differed from the pinned terminal's treatment of non-C1 Latin-1 scalars. The byte-range fast path now gives the remaining Latin-1 scalars width one in both policies. This does not claim parity for all legacy Unicode width classifications.

The first independent run had 813 matches and 216 divergences across 343 cases and three input chunk sizes. The C1 and alternate-pen corrections left only three soft-hyphen comparisons failing in an expanded run. The final combined suite has **1,154 cases and 3,462 matches**, using chunk sizes 1, 7 and 65,536. Expected snapshots come from the real pinned C library; no hand-authored expected outputs substitute for them. The compact fixture generator imports the independent cases and records both generator hashes.

Coverage includes all encoded C1 scalars in both width modes, raw 8-bit counterparts, pending single shifts, styled and hyperlinked-row mutation paths, Latin-1, repeated grapheme bases, one-column screens, wide boundaries, disabled wrapping, parameter overflow, saved-mode defaults and request order, repeated alternate entry/exit, saved cursors, visibility, margins, and seeded combinations. The common differential projection compares cell text/graphemes, widths, symbolic colors and styles, row wraps, cursor, observed mode flags and replies. It does not independently prove hyperlink URI values, graphics, every mode, history retention, or rendering.

Source review found no additional concrete bounds or ownership defect in this slice. REP keeps a bounded stack buffer and preserves the previous raw printed scalar rather than substituting a rendered character or attached combining suffix. Saved-mode state is a fixed-size bitset with protocol defaults and no allocation. Alternate clears mutate rows through copy-on-write, retain old immutable snapshots, update existing memory charges, and replace the history identity so old Find anchors cannot refer to cleared content. These are source-review conclusions; the review did not introduce a new independent memory-pressure stress run solely for alternate clearing.

Final reviewed candidate source SHA-256:
`0a11196551c74ef433ab789ebaa3dbbeff5d6dde1fb1f9df257a2c8c9195a7ab`.
Replay SHA-256:
`f7d18cd71666661c59775d6fbb2a75a7c58ced84971a7f9afcee078a75290684`.
The replay hash was independently checked against the report and build manifest. Raw initial, intermediate and final reports plus the original generator are retained under `upstream/vt-controls-review/`. The intermediate report identifies its own binary; its source build manifest was superseded during the fixes and is not relabeled as the final source.

The audited SIMD dependency inventory contains 29 files, 137,200 logical bytes and 15 Rust files (232 KiB allocated on the local filesystem). It contains no macOS-specific code. AArch64 NEON supports Linux. Its two WASM implementation files are excluded by Rust configuration on Linux and retained unchanged with the audited upstream source; they are not macOS components.
