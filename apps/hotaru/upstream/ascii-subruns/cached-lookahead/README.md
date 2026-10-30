# Cached-lookahead ASCII subruns (rejected)

This separate safe cursor version is **not integrated**. It retains the first
experiment's ASCII handoff but replaces repeated cloned character decoding
with `TextChars`: an original string, a `Chars` iterator and one cached scalar.
Each scalar is decoded once; checked slicing reconstructs the original suffix
including lookahead. ASCII metadata writes use the original standard Peekable.
The private cursor's exhaustive scalar and seeded mixed/handoff tests pass.
Independent source review found no bounds, lifetime or state-order defect.

The change still regresses mixed Unicode and dense-wide medians in all six
rounds in both width modes. Correcting duplicate decoding was insufficient;
these results do not establish which remaining loop/code-generation change is
responsible. Truncated/restricted patterns still improve substantially, but the
broad regressions reject this implementation.

## Identities and validation

Baseline source `7e36bc3139a9d6ee0617c30b602aa95f21bd732fba389c73ae176cfe49c74d6d`,
ELF `5755bcf3479b184ff8b05bcf68a22196d386a63258f99754caf380d90c576728`.
Candidate source `3df8575f6c19f619c4c32ea06ec81a7f3d6805bc1e287d3b7a41fce11c79a562`,
ELF `882389d16fd11da1e46d1f748299f2d794f309cac014f41735fc1c71ab1c1282`.
Full vendor/source/compiler manifests are retained. Rust 1.95/native/release 3/
thin LTO/one codegen unit/debug 0 are shared. Source differs only in screen.rs,
new screen/text_chars.rs and tests/utf8_ascii_runs.rs.

All 143 workspace tests pass in debug and release; the same two shell tests
remain deliberately ignored. The focused two tests are already counted in 143.
Strict all-target Clippy passes, as do 444 previous, 3,462 VT and 2,640 new ASCII
handoff comparisons against the unchanged library. All 756 fresh cursor records
match the committed fixtures (three chunk sizes in the workspace tests).
No new unsafe code or candidate-specific Miri claim is introduced.

## Timing scope

All 30 full baseline/candidate state gates and pinned-reference common-state
gates pass. The exact same 15 input patterns run in both width modes on CPU 8,
120 columns × 12 rows, zero history and 64 KiB chunks. Timing is feed-only; raw
setup/teardown samples remain separate. Each engine receives 3 warmups then
60 samples across all six order permutations. Common and dense-wide inputs are
approximately 2 MiB; malformed inputs approximately 256 KiB. Reference binary,
library hashes, input hashes and commands are retained in summary.json.

Preflight activity was 5.77% on CPU 8 and 1.92% on sibling 72. The 602 observations
(540 between batches) found no compiler; CPU 8 frequency 4.625–5.036GHz, no
missing readings. Periodic samples cannot exclude brief unrelated activity.
One legacy dense-wide round has a 1.8875 candidate/baseline ratio and a large
tail; all other five rounds also regress (1.0647–1.1913). This outlier is
preserved, not used as the sole acceptance reason. Mixed and Unicode dense
regressions are also consistent across every round.

Ratios below are median feed times; lower is better. Full p95/max and all round
ratios are in summary.json; native-raw.json.gz retains every original sample,
host observation and command. This 12-row result is distinct from the historical
80-row common corpus and says nothing about whole-application parity.

| Mode | Input | Baseline/Ghostty | Candidate/Ghostty | Candidate/baseline | Six-round ratio range |
| --- | --- | ---: | ---: | ---: | ---: |
| legacy | ascii | 0.9381 | 0.9499 | 1.0125 | 0.9959–1.0388 |
| legacy | unicode | 1.2683 | 1.3243 | 1.0441 | 1.0357–1.0656 |
| legacy | styled | 0.8513 | 0.8439 | 0.9914 | 0.7304–1.0298 |
| legacy | cursor | 0.3624 | 0.3635 | 1.0029 | 0.9843–1.0418 |
| legacy | short-invalid-leads | 0.2535 | 0.2575 | 1.0160 | 0.9734–1.0679 |
| legacy | short-invalid-suffix | 2.4345 | 1.5253 | 0.6265 | 0.4645–1.2317 |
| legacy | short-invalid-second | 0.8376 | 0.8461 | 1.0102 | 0.9995–1.0275 |
| legacy | short-overlong-three | 0.9606 | 0.9553 | 0.9944 | 0.9901–1.0096 |
| legacy | short-surrogate | 0.9493 | 0.9466 | 0.9972 | 0.9268–1.0227 |
| legacy | short-too-large | 1.0144 | 1.0260 | 1.0114 | 0.7116–1.1031 |
| legacy | short-interior-error | 1.4985 | 1.3811 | 0.9217 | 0.9043–0.9353 |
| legacy | short-restricted-tail | 2.0352 | 1.4307 | 0.7030 | 0.3678–0.7933 |
| legacy | short-truncated-two | 2.3437 | 1.4675 | 0.6261 | 0.5889–0.6798 |
| legacy | short-truncated-three | 2.4437 | 1.5353 | 0.6283 | 0.5557–0.6803 |
| legacy | dense-wide | 1.8423 | 2.0901 | 1.1345 | 1.0647–1.8875 |
| unicode | ascii | 0.9323 | 0.9604 | 1.0301 | 1.0122–1.0973 |
| unicode | unicode | 1.1946 | 1.2534 | 1.0492 | 1.0397–1.0742 |
| unicode | styled | 0.8353 | 0.8613 | 1.0311 | 0.9998–1.0708 |
| unicode | cursor | 0.3632 | 0.3629 | 0.9992 | 0.9943–1.0193 |
| unicode | short-invalid-leads | 0.2853 | 0.2995 | 1.0501 | 0.9866–1.1259 |
| unicode | short-invalid-suffix | 2.5815 | 1.6459 | 0.6376 | 0.6155–0.6563 |
| unicode | short-invalid-second | 0.8496 | 0.8730 | 1.0275 | 1.0062–1.0503 |
| unicode | short-overlong-three | 0.9994 | 0.9871 | 0.9877 | 0.9711–1.0062 |
| unicode | short-surrogate | 0.9924 | 0.9789 | 0.9864 | 0.9716–0.9958 |
| unicode | short-too-large | 1.0734 | 1.1245 | 1.0476 | 1.0130–1.1069 |
| unicode | short-interior-error | 1.5725 | 1.4683 | 0.9337 | 0.9047–0.9465 |
| unicode | short-restricted-tail | 2.1405 | 1.5477 | 0.7231 | 0.7110–0.7549 |
| unicode | short-truncated-two | 2.5486 | 1.5593 | 0.6118 | 0.5851–0.6345 |
| unicode | short-truncated-three | 2.5791 | 1.6506 | 0.6400 | 0.5616–0.6602 |
| unicode | dense-wide | 1.7171 | 1.9427 | 1.1314 | 1.0880–1.2057 |

Raw local and remote evidence remains at
`/home/callen/work/hotaru-ascii-cached-20261028/native-thirty/`.
Input patterns, lengths, hashes and locations are in input-manifest.json;
`tools/generate-inputs.py DEST` reconstructs them outside Git. Generated inputs
and executable outputs are excluded from this archive. Runners retain the exact
measurement paths and require reviewed path adjustments in another environment.
