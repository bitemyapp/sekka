# Error-prefix-only ASCII partition (rejected)

This isolated candidate is **not integrated**. It restores the qualified
baseline's entire screen.rs and changes only lib.rs's nonempty validated prefix
before a UTF-8 error. An outlined, borrowed, allocation-free helper partitions
that prefix into existing UTF-8 and ASCII writers. It adds no unsafe code.
An incomplete scalar at a chunk boundary can select this path even when the
eventual input is valid UTF-8; see DESIGN.md for the exact scope and invariants.

The new long dense-prefix gate exposes a 16.7% legacy / 10.7% Unicode median
regression. Its candidate/Ghostty ratios are 2.797 and 2.259. Short truncated
patterns improve 32–41%, but that does not offset the dense-prefix penalty.
Invalid-lead inputs also regress 8.9% / 7.6%, even though their zero-length
valid prefixes do not call the helper. Thus the extra scan cannot explain every
regression; the changed feed branch/code generation also needs diagnosis.
No exclusive causal attribution follows from this one compiled comparison.

## Source and validation

Baseline source `7e36bc3139a9d6ee0617c30b602aa95f21bd732fba389c73ae176cfe49c74d6d`,
ELF `5755bcf3479b184ff8b05bcf68a22196d386a63258f99754caf380d90c576728`.
Candidate source `ba30dfe49e8f67f199788e25dab1776081fbe19f7d63b27f4c93496daae641bd`,
ELF `6cae060e0b046b627029f657c82634757ba35d34e82392ea514cb1fa9a1152f5`.
Complete source/vendor/compiler identities are retained. The manifest delta is
exactly lib.rs and tests/utf8_ascii_runs.rs; screen.rs is byte-identical to the
baseline. Both executables use Rust 1.95, native target, optimization 3, thin
LTO, one codegen unit and debug 0.

All 142 workspace tests pass in debug and release, with the same two deliberate
shell-test ignores. The three focused tests are already included in 142.
Strict all-target Clippy passes. Actual unchanged-library comparisons pass:
444 previous, 3,462 VT and 2,652 error-prefix cases. All 756 fresh cursor
records equal the committed fixtures (three chunk sizes in workspace tests).
There is no new candidate-specific Miri claim.

## Direct native comparison

All 32 complete baseline/candidate state gates and pinned-reference common-state
gates pass. Sixteen exact input patterns run in both width modes: four common,
ten earlier malformed, dense wide, and a new long dense prefix before an error.
The new pattern is 16,384 copies of 雪 followed by F0 9F CR LF, repeated to
2,064,552 bytes, SHA-256
`db0728faad48e651babb8caf209db12ca80ce10ac24af22b97207d220138ec28`.
It contains no qualifying ASCII tail, exposing the added dense-prefix scan.

The protocol is CPU 8 / observed SMT sibling 72, 120 columns × 12 rows,
zero history, 64 KiB chunks and feed-only timing. Each engine receives three
warmups and 60 samples across all six engine order permutations. Raw
setup/teardown samples remain separate. Ghostty library/adapter identities,
input hashes, commands and exact common-state omissions are in summary.json.

Preflight activity was 3.85% on CPU 8 and 0% on sibling 72. The 642 observations
(576 between batches) found no compiler; CPU 8 frequencies were 4.677–5.043 GHz,
with no missing readings. Periodic observation cannot exclude every brief
unrelated disturbance. Lower ratios are better. summary.json retains all p95,
maximum and six-round ratios; native-raw.json.gz retains every original sample
and host observation. This 12-row protocol is distinct from the historical
80-row common workload and does not establish whole-application parity.

| Mode | Input | Baseline/Ghostty | Candidate/Ghostty | Candidate/baseline | Six-round ratio range |
| --- | --- | ---: | ---: | ---: | ---: |
| legacy | ascii | 0.9516 | 0.9815 | 1.0315 | 1.0132–1.0388 |
| legacy | unicode | 1.2613 | 1.2781 | 1.0134 | 1.0015–1.0823 |
| legacy | styled | 0.8488 | 0.8590 | 1.0120 | 0.9750–1.0593 |
| legacy | cursor | 0.3575 | 0.3607 | 1.0088 | 0.9804–1.0324 |
| legacy | short-invalid-leads | 0.2443 | 0.2661 | 1.0892 | 1.0533–1.1237 |
| legacy | short-invalid-suffix | 2.3101 | 1.5608 | 0.6756 | 0.6578–0.6856 |
| legacy | short-invalid-second | 0.8296 | 0.8688 | 1.0473 | 1.0381–1.0535 |
| legacy | short-overlong-three | 0.9636 | 0.9878 | 1.0251 | 0.9665–1.0528 |
| legacy | short-surrogate | 0.9566 | 0.9898 | 1.0347 | 1.0152–1.0528 |
| legacy | short-too-large | 1.0312 | 1.0563 | 1.0243 | 1.0122–1.0457 |
| legacy | short-interior-error | 1.5199 | 1.4376 | 0.9459 | 0.8994–0.9553 |
| legacy | short-restricted-tail | 2.0309 | 1.4398 | 0.7089 | 0.5657–0.7395 |
| legacy | short-truncated-two | 2.3997 | 1.4955 | 0.6232 | 0.5918–0.6610 |
| legacy | short-truncated-three | 2.5333 | 1.5563 | 0.6143 | 0.5971–0.6533 |
| legacy | dense-wide | 1.8850 | 1.9592 | 1.0393 | 0.9993–1.1052 |
| legacy | long-dense-error-prefix | 2.3967 | 2.7968 | 1.1670 | 1.0893–1.2622 |
| unicode | ascii | 0.9481 | 0.9817 | 1.0354 | 0.9481–1.0651 |
| unicode | unicode | 1.1928 | 1.2485 | 1.0467 | 0.9989–1.0564 |
| unicode | styled | 0.8434 | 0.8657 | 1.0265 | 1.0153–1.0537 |
| unicode | cursor | 0.3602 | 0.3636 | 1.0095 | 0.9812–1.0182 |
| unicode | short-invalid-leads | 0.2811 | 0.3024 | 1.0757 | 1.0098–1.1534 |
| unicode | short-invalid-suffix | 2.6631 | 1.6042 | 0.6024 | 0.5734–0.6454 |
| unicode | short-invalid-second | 0.8541 | 0.9089 | 1.0642 | 1.0313–1.0874 |
| unicode | short-overlong-three | 0.9947 | 1.0356 | 1.0410 | 0.9794–1.0849 |
| unicode | short-surrogate | 0.9858 | 1.0444 | 1.0595 | 0.9888–1.1153 |
| unicode | short-too-large | 1.0779 | 1.1482 | 1.0652 | 1.0389–1.1730 |
| unicode | short-interior-error | 1.5682 | 1.4770 | 0.9419 | 0.9355–0.9569 |
| unicode | short-restricted-tail | 2.1790 | 1.5499 | 0.7113 | 0.6872–0.7260 |
| unicode | short-truncated-two | 2.6213 | 1.5375 | 0.5865 | 0.5777–0.6017 |
| unicode | short-truncated-three | 2.6543 | 1.6109 | 0.6069 | 0.5969–0.6385 |
| unicode | dense-wide | 1.7609 | 1.8152 | 1.0308 | 0.9861–1.0869 |
| unicode | long-dense-error-prefix | 2.0414 | 2.2590 | 1.1066 | 1.1024–1.1368 |

Raw local and remote results remain at
`/home/callen/work/hotaru-error-ascii-prefix-20261028/native-thirty-two/`.
input-manifest.json retains every pattern, length, hash and raw location;
`tools/generate-inputs.py DEST` reconstructs all 16 inputs outside Git.
Generated corpora and executable outputs are excluded from this archive.
The retained runner paths identify the actual environment and need reviewed
adjustments before reproducing elsewhere.
