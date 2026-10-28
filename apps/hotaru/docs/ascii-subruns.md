# ASCII subruns inside UTF-8 text

The first isolated ASCII-subrun candidate is rejected for product integration.
It reuses Hotaru's packed ASCII path for printable runs of at least 16 bytes
inside otherwise UTF-8 text. It passes 141 debug and 141 release tests, strict
Clippy, the existing reference suites and 2,640 new pinned-library handoff
comparisons. It adds no unsafe code.

A direct three-engine run on the workstation compares the qualified baseline,
the candidate and pinned Ghostty across 30 cases: four common corpora, ten
malformed patterns and dense wide text, each in both width modes. It uses
120 columns × 12 rows, zero history and 60 feed samples per engine per case.
Truncated-pattern medians improve roughly 37–41%, restricted-tail patterns
about 30%, and interior-error patterns about 7%. The improved truncated cases
still take approximately 1.43–1.58 times Ghostty's feed time.

Mixed text regresses 9.2% in legacy mode and 4.3% in Unicode mode; dense wide
text regresses 17.6% and 10.5%. These four regressions appear in all six round
medians. They disqualify this implementation. Its replacement of cached
`Peekable` lookahead with cloned character decoding is a plausible cause to
investigate in a separate candidate, not an established timing attribution.

The [complete evidence](../upstream/ascii-subruns/README.md) preserves all 30
median/p95/maximum and per-round results, raw samples and host observations,
exact source/vendor/binary identities, the patch, test logs and input generators.
No generated corpus or executable is included in Git. This small-grid
measurement does not replace the earlier 80-row common-corpus qualification or
establish whole-application performance parity. Shared product source is
unchanged by this experiment.
