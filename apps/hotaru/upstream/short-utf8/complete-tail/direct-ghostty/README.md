# Absolute expanded UTF-8 diagnostic

The complete-tail candidate remains rejected for integration. This separate diagnostic compares the qualified Hotaru baseline and that same isolated candidate directly with pinned Ghostty; it neither hides the self-regression nor authorizes integration.

All runs use **120 columns × 12 rows**, zero history, 64 KiB chunks, three warmups and 60 measured iterations per engine/case on CPU 8. Six three-engine permutations balance every engine position twice. Setup and teardown are recorded separately from feed. The common four inputs are approximately 2 MiB; the ten malformed patterns are approximately 256 KiB. These are not the older 80-row/~8 MiB workload, and cross-window differences are not code-regression evidence.

Both Rust versions use 1.95.0 with the same release/native flags. Source digests are baseline `7e36bc3139a9d6ee0617c30b602aa95f21bd732fba389c73ae176cfe49c74d6d`, candidate `0b44994701959869dac11f184ce8bc5332a3272a7f9436fb95fb02da5c12600f`; binary hashes are `5755bcf3479b184ff8b05bcf68a22196d386a63258f99754caf380d90c576728` and `c3932c6b5175d69926096581a13e5cf2d514bccd4c4cba98b81eaa50dc4db22c`. The reference remains Ghostty 6ba1441, library 70e1bdcf… and current width-aware adapter dc687741…. Full build/vendor/compiler/source identities are embedded in raw evidence and adjacent build manifests.

All 28 cases passed full baseline/candidate state equality and the unchanged library common-state comparison before timing. Malformed patterns contain only exact specified UTF-8 bytes, printable ASCII and CRLF; they do not introduce graphics, hyperlinks or unmeasured protocol features. The general state comparator still omits those features, pending-wrap metadata, scrollback and rendering. Each timed iteration verifies response counts.

Numbers are Hotaru/Ghostty median feed-time ratios; greater than 1 means slower.

| Workload | Legacy baseline | Legacy candidate | Unicode baseline | Unicode candidate |
| --- | ---: | ---: | ---: | ---: |
| ascii | 0.9546 | 0.9289 | 0.9530 | 0.9272 |
| unicode | 1.2661 | 1.1608 | 1.1967 | 1.1217 |
| styled | 0.8365 | 0.7807 | 0.8330 | 0.7737 |
| cursor | 0.3597 | 0.3374 | 0.3652 | 0.3410 |
| short-invalid-leads | 0.2456 | 0.2509 | 0.2828 | 0.3055 |
| short-invalid-suffix | 2.3487 | 2.5266 | 2.6522 | 2.6361 |
| short-invalid-second | 0.8415 | 0.9407 | 0.8573 | 0.9705 |
| short-overlong-three | 0.9464 | 1.0565 | 0.9798 | 1.1141 |
| short-surrogate | 0.9623 | 1.0529 | 0.9837 | 1.1236 |
| short-too-large | 1.0287 | 1.1252 | 1.0855 | 1.2257 |
| short-interior-error | 1.5121 | 1.6593 | 1.5609 | 1.6812 |
| short-restricted-tail | 1.9387 | 2.1351 | 2.2169 | 2.2708 |
| short-truncated-two | 2.3391 | 2.6352 | 2.5936 | 2.6751 |
| short-truncated-three | 2.4045 | 2.6146 | 2.6584 | 2.7673 |

The current baseline is already 2.3–2.7× slower on these truncated-suffix cases and 1.5–1.6× on the invalid-interior pattern. The candidate does not resolve those gaps, while pushing overlong/surrogate patterns from near parity to slower than Ghostty. Invalid-leading-byte-only and invalid-second-byte inputs remain faster than Ghostty despite their self-regression. Mixed valid text also remains slower with the candidate. This is a bounded corpus result, not a general parity claim.

All individual samples, per-round median ratios, p95 and maximum ratios are retained in summary.json and raw-summary.json.gz. CPU 8/sibling 72 preflight was 3.85%/0%;562 observations included no compiler process and frequencies 4.689–5.047GHz. Periodic observations cannot exclude every brief outside disturbance. No samples are trimmed.

Generated input bytes stay outside Git. `tools/generate-inputs.py DESTINATION` reconstructs and SHA-256 verifies every input using input-manifest.json; raw local/workstation paths are recorded there. The original raw report SHA-256 is `737a5bfc9b8995951379aa750b6f255c5e81f8b9fa4f223e7bb934eee65772ec`.
