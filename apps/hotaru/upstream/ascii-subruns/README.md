# ASCII subrun experiment (rejected)

This isolated candidate is **not integrated**. It sends long printable ASCII
subruns within validated UTF-8 text to the existing packed-cell ASCII path.
It preserves scalar metadata allocation order, row ownership, wrap, cursor,
REP and byte-budget rules. It adds no unsafe code.

The malformed-input gains are substantial, but mixed Unicode and dense-wide
regressions reject this version. All six round medians are worse for both mixed
and dense-wide cases in both width modes. Replacing cached `Peekable` lookahead
with `Chars::clone().next()` may repeat non-ASCII decoding; that is a source-level
hypothesis, not a measured causal attribution. A separate cached-lookahead
experiment must preserve this evidence and pass its own gates.

## Identities and correctness

Baseline source `7e36bc3139a9d6ee0617c30b602aa95f21bd732fba389c73ae176cfe49c74d6d`,
ELF `5755bcf3479b184ff8b05bcf68a22196d386a63258f99754caf380d90c576728`.
Candidate source `9077006c6fbbd5c40e26ea113f6f8ce16ba9cc38dd84c284f95f1fb3d451017f`,
ELF `0e41fc82400b360c2898aedd0072e30cdac27d0aa33fdea95a9a4d384d95b1c2`.
The source/build manifests include all vendor inputs and exact compiler/flags.
Only `screen.rs` and new `tests/utf8_ascii_runs.rs` differ from the baseline's
compiled files. Rust 1.95, native target, release optimization 3, thin LTO,
one codegen unit and debug 0 are common to both builds.

All 141 workspace tests pass in debug and release; the same two shell tests
remain deliberately ignored. The initial focused two tests are already included
in 141. Strict all-target Clippy passes. The unchanged pinned library passes
444 prior cases, 3,462 VT comparisons, and 2,640 new ASCII-handoff comparisons.
All 756 fresh cursor records equal the committed fixtures, checked at three
chunk sizes by each workspace test run. There is no new candidate-specific Miri
claim because this change adds no unsafe code.

The first release qualification reached the 444 comparisons then stopped on a
missing copied Python helper. `qualification.log` preserves that setup error;
`reference-finish.log` records the remaining comparisons after copying the
helper, without changing compiled source or the release binary.

## Native protocol

The benchmark directly compares all three engines on CPU 8, observing its
SMT sibling 72: 120 columns × 12 rows, zero history, 64 KiB input chunks,
feed-only timing with setup/teardown samples preserved separately. Each case
has three warmups followed by all six engine order permutations and ten
samples per engine per order (60 feed samples). Common corpora and dense-wide
are approximately 2 MiB; malformed corpora are approximately 256 KiB.

Ghostty is the unchanged `6ba144158577d033ccb6ed4b574e4013957c9161` library
(`70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639`) with the
current width-aware adapter
`dc687741377b3058f7b06bcdf287df9990c183eadd571a1fbe835fb3869625cd`.
All 30 complete baseline/candidate JSON gates and pinned-reference common-state
gates pass. Common-state omissions and exact input scopes are in `summary.json`;
this is not complete terminal or whole-application parity.

The preflight observed CPU 8 at 3.88% activity and sibling 72 at 0%. There are
602 process/CPU/frequency observations, including 540 between batches, with no
observed compiler and no missing CPU 8 frequency readings (4.683–5.046 GHz).
Periodic observations cannot exclude every brief disturbance. The lock excludes
this effort's heavy jobs, not unrelated machine activity.

Ratios are median feed times; lower is better. `summary.json` also retains
nearest-rank p95, maxima and every round ratio for all engines. Raw samples,
commands, host observations and setup/teardown data are preserved in
`native-raw.json.gz`. This 12-row workload must not be compared as a code
regression with the earlier 80-row/~8 MiB qualification.

| Mode | Input | Baseline/Ghostty | Candidate/Ghostty | Candidate/baseline | Six-round ratio range |
| --- | --- | ---: | ---: | ---: | ---: |
| legacy | ascii | 0.9449 | 0.9765 | 1.0334 | 1.0002–1.1139 |
| legacy | unicode | 1.2797 | 1.3980 | 1.0924 | 1.0192–1.1022 |
| legacy | styled | 0.8281 | 0.8546 | 1.0321 | 1.0123–1.0785 |
| legacy | cursor | 0.3602 | 0.3657 | 1.0152 | 0.9548–1.0587 |
| legacy | short-invalid-leads | 0.2477 | 0.2607 | 1.0523 | 1.0168–1.0965 |
| legacy | short-invalid-suffix | 2.3559 | 1.4858 | 0.6307 | 0.6216–0.6420 |
| legacy | short-invalid-second | 0.8443 | 0.8443 | 1.0001 | 0.9931–1.0237 |
| legacy | short-overlong-three | 0.9534 | 0.9690 | 1.0163 | 1.0055–1.0249 |
| legacy | short-surrogate | 0.9563 | 0.9663 | 1.0104 | 0.9967–1.0180 |
| legacy | short-too-large | 1.0287 | 1.0420 | 1.0129 | 0.9827–1.0243 |
| legacy | short-interior-error | 1.4773 | 1.3694 | 0.9270 | 0.8629–0.9398 |
| legacy | short-restricted-tail | 2.0300 | 1.4306 | 0.7047 | 0.6886–0.7505 |
| legacy | short-truncated-two | 2.4077 | 1.4339 | 0.5955 | 0.5759–0.6293 |
| legacy | short-truncated-three | 2.4973 | 1.5181 | 0.6079 | 0.5997–0.6588 |
| legacy | dense-wide | 1.8545 | 2.1807 | 1.1759 | 1.0771–1.2223 |
| unicode | ascii | 0.9454 | 0.9725 | 1.0287 | 1.0201–1.0419 |
| unicode | unicode | 1.1908 | 1.2417 | 1.0428 | 1.0376–1.0561 |
| unicode | styled | 0.8584 | 0.8662 | 1.0091 | 0.9458–1.0528 |
| unicode | cursor | 0.3593 | 0.3624 | 1.0085 | 1.0034–1.0688 |
| unicode | short-invalid-leads | 0.2802 | 0.2925 | 1.0439 | 1.0280–1.0592 |
| unicode | short-invalid-suffix | 2.6041 | 1.5789 | 0.6063 | 0.5857–0.6224 |
| unicode | short-invalid-second | 0.8561 | 0.8504 | 0.9932 | 0.9679–1.0139 |
| unicode | short-overlong-three | 0.9782 | 0.9815 | 1.0034 | 0.9858–1.0274 |
| unicode | short-surrogate | 1.0004 | 0.9923 | 0.9918 | 0.9598–1.0186 |
| unicode | short-too-large | 1.0896 | 1.1203 | 1.0282 | 1.0077–1.0503 |
| unicode | short-interior-error | 1.5775 | 1.4665 | 0.9296 | 0.8742–0.9473 |
| unicode | short-restricted-tail | 2.1787 | 1.5162 | 0.6959 | 0.6635–0.7129 |
| unicode | short-truncated-two | 2.5326 | 1.5149 | 0.5982 | 0.5314–0.6196 |
| unicode | short-truncated-three | 2.6486 | 1.5608 | 0.5893 | 0.5817–0.6091 |
| unicode | dense-wide | 1.7547 | 1.9382 | 1.1046 | 1.0550–1.1362 |

## Reproduction

`input-manifest.json` retains every input's pattern, byte count, SHA-256 and raw
locations. `tools/generate-inputs.py /outside/git/destination` reconstructs and
verifies all 15 inputs; generated corpora and executable outputs are excluded
from Git. The runner scripts retain the actual isolated and reference paths;
recreate those paths or review explicit path changes before rerunning them.
Full raw local and workstation reports remain under
`/home/callen/work/hotaru-ascii-subruns-20261028/native-thirty/`.
