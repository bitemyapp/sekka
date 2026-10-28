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

A separate [cached-lookahead version](../upstream/ascii-subruns/cached-lookahead/README.md)
removes repeated decoding with a safe private cursor. It passes 143 debug and
release tests and the same reference gates, but also remains rejected. Its
mixed-text medians regress 4.4–4.9%, and dense-wide medians regress about 13%
in both modes. Every corresponding round median is slower; one legacy
dense-wide round has an additional large outlier, preserved in the raw data.
The malformed-prefix gains remain, so the next investigation can restrict the
handoff to the error path while leaving ordinary valid-text printing unchanged.
The cached-cursor results do not prove that repeated decoding caused the first
version's entire regression.

The [error-prefix-only version](../upstream/ascii-subruns/error-prefix/README.md)
restores the original screen implementation and invokes an outlined partition
only after a UTF-8 validation error. It also remains rejected. Its new long
dense-prefix case exposes a 16.7% legacy / 10.7% Unicode scan penalty, despite
32–41% improvements on short truncated patterns. Invalid-lead cases regress
even though their empty valid prefixes never call the helper, so the changed
feed branch and generated code also require attribution. All 142 debug and
release tests, lint checks, pinned-reference gates and 32 timing-state gates
pass. An incomplete scalar at a chunk boundary selects this path too; it is
not accurate to say that all ultimately valid streams are unaffected.

The subsequent [bounded instruction attribution](../upstream/ascii-subruns/error-prefix-attribution/README.md)
uses matching diagnostic compiler flags and preserves native disassembly
separately. In a single long dense-prefix case, the added helper accounts for
344,099 of 344,220 extra instructions; the original UTF-8 printer's count is
unchanged. This identifies the scan cost directly. Invalid-lead and mixed-text
whole-process counts change only 0.011% and 0.017%, respectively, which does not
explain their native timing differences. The feed function's native layout and
branch order change, but no hardware-counter experiment has established the
remaining cause. Diagnostic instruction counts are not native wall-clock
measurements. No fourth core candidate is included in this evidence.
