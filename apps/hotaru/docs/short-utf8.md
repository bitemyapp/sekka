# Short UTF-8 validation candidate

This isolated variant is **not accepted for integration**. Its mixed-text improvement does not justify the measured truncated-input regressions, especially the 17.7% Unicode-mode median regression. The code remains outside the product. The [patch](../upstream/short-utf8/short-utf8.patch), [machine-readable summary](../upstream/short-utf8/summary.json), source/vendor manifests, qualification logs and compressed diagnostic profiles preserve the experiment for any separately qualified follow-up.

The baseline is frozen product checkpoint `e2b1ff7`, source digest `b9fcaa0ced20a2212e21685002a6da7b1fda89d3443394fe9afe82507c66ec3b`. The candidate source digest is `b3aa53e1d3e724130ebec6b977c13aea1de727021d20a11cb3654a49c553b467`; its native replay binary is `ae8278cb47d00b1ac87ffbf9e4788f8b6cb925d6de098e77e7b6e0788b641c88`. No vendor source changes are involved.

The existing SIMD validator falls back to scalar validation below 64 bytes. The candidate copies plausible non-ASCII-leading runs of 32–63 bytes into a zeroed 64-byte stack buffer, then invokes that same validator. Successful validation permits one narrowly scoped unsafe conversion of the original borrowed input to `str`: ASCII NUL padding cannot complete a truncated scalar, so the original input must end at a valid scalar boundary. The returned reference never points to the stack buffer. Failed validation uses the original input and validator to preserve exact error offsets and incomplete-suffix behavior. Impossible leading bytes bypass padding.

Debug and release workspace suites each passed **140 tests with two existing intentional ignores**; strict workspace all-target Clippy passed. Focused wrapper tests cover thresholds, embedded NULs, truncated suffixes, 786,432 exhaustive two-byte placements, 65,536 seeded unaligned inputs, original-pointer identity and guard-page boundaries. Miri strict provenance passed the wrapper boundary/suffix/lifetime test using its scalar dependency fallback. Independent source review found no concrete safety or error-path defect.

The unchanged pinned Ghostty library matched **444 previous and 3,462 VT-control comparisons** against the native candidate. Its 756 fresh cursor case objects equal the bundled fixtures exercised with three chunk sizes in both debug and release tests. The intentional ignores are the external-shell startup fixture and its standalone child helper.

Callgrind on a separate `x86-64-v3` diagnostic binary measured **5.68% fewer instructions for mixed Unicode**. ASCII, styled text and cursor workloads were effectively unchanged. The malformed-leading-byte corpus required **1.12% more instructions**, and truncated suffixes required **8.15% more** because the precise-error fallback validates again. These regressions are an explicit tradeoff. The first unguarded prototype is retained in the isolated evidence directory and is not the proposed patch.

Instruction counts include setup and teardown; they are not native time, hardware-counter measurements, a Ghostty parity result or a non-x86 performance claim. Unrelated workstation builds were active during initial qualification. A later quiet preflight allowed the separate native run below. The helpers preserve complete source/vendor/executable identities and check source stability. Gzip files in the compact archive decompress to the original raw evidence; the full isolated tree remains at `/home/callen/work/hotaru-short-text-candidate-20261028` locally and on `wx-workstation`.

## Paired native window

The confirmed baseline and candidate ran serially on CPU 8, with sibling CPU 72 observed. Preflight activity was 2.91% and 0%, respectively. No compiler process was observed in preflight or 144 between-batch snapshots. The archive contains one-second per-core utilization, frequency and process-name observations, complete raw feed/setup/teardown samples, command order, and before/after source and executable hashes. Each engine received three warmups and 60 measured iterations per case in six alternating rounds. All twelve workload/mode cases passed full baseline/candidate JSON equality and the pinned reference's common-state comparison before timing.

Candidate/baseline median feed-time ratios were:

| Workload | Legacy width | Unicode width |
| --- | ---: | ---: |
| ASCII | 1.0059 | 1.0102 |
| Mixed Unicode | 0.8951 | 0.9505 |
| Styled text | 0.9845 | 0.9637 |
| Cursor controls | 1.0083 | 1.0180 |
| Short malformed leading bytes | 0.9946 | 1.0190 |
| Short truncated suffixes | 1.0657 | 1.1767 |

The truncated-suffix regression also appears in tail measurements: p95 ratios are 1.1017/1.1108 and maximum ratios are 1.1094/1.1014 for legacy/Unicode width. Mixed Unicode's Unicode-mode median improves 4.95%, but its p95 improves only 0.36%. The smaller ASCII and cursor regressions are retained in the report too. CPU 8 frequency observations ranged from 4.695 to 5.046 GHz; this single paired window supports a bounded workload result, not universal performance acceptance. The proposed code remains separate.

## Follow-up: translate padded errors

A [separate isolated follow-up](../upstream/short-utf8/error-translation/summary.json) removes the explicit second validation pass after padded SIMD failure. It preserves the first invalid offset and translates the padded error to an incomplete original suffix only when the accepted multibyte prefix ends exactly at the original slice boundary. Invalid final leads retain a one-byte error. A private error record keeps the dependency's fields encapsulated; no dependency or product API changes are involved. The [proof and patch](../upstream/short-utf8/error-translation/design.md) retain the same bounded validated-prefix unsafe conversion.

The source is `7f41c53e192576d238ddefdf682b69eec0a891749147a0709737a7a035b90e83`, native binary `4800fbc922b836f21bfe2df170903cbb88f8f97518ab6b15626ae107381d253d`. Only the helper and its tests differ from the qualified ASCII baseline source `7e36bc31…`, binary `5755bcf3…`. Debug and release each pass 146 tests with the same two intentional shell-related ignores, strict Clippy and two strict-provenance Miri tests. Exhaustive checks cover 33,554,432 three-byte suffixes and 17,530,368 valid-scalar/prefix combinations, plus byte pairs, thresholds, alignments, NULs and protected-page ends. The unchanged Ghostty library matches 444 earlier and 3,462 VT comparisons; 756 fresh cursor cases equal the fixtures checked at three chunk sizes.

This variant is **also rejected for integration**. On CPU 8, with the same native Rust 1.95 flags, zero history, a 120×12 grid and six alternating rounds of ten samples per engine, mixed Unicode improves but truncated input remains slower:

| Workload | Legacy median ratio | Legacy p95 ratio | Unicode median ratio | Unicode p95 ratio |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 0.9934 | 0.9857 | 1.0016 | 1.0348 |
| Mixed Unicode | 0.9101 | 0.8747 | 0.9342 | 0.9185 |
| Styled text | 1.0163 | 1.0298 | 1.0069 | 1.0520 |
| Cursor controls | 0.9890 | 0.9543 | 1.0012 | 1.0198 |
| Malformed leading bytes | 0.9476 | 0.9606 | 1.0046 | 1.0122 |
| Truncated suffixes | 1.0812 | 1.0665 | 1.0398 | 1.0630 |

Ratios compare this candidate directly with the current ASCII baseline; they are not Ghostty ratios. All 12 complete baseline/candidate state and pinned-reference common-state gates pass. Truncated input is slower in all six legacy rounds and five of six Unicode rounds. Its maximum ratios are 1.0264 and 1.0576. Preflight CPU 8/sibling 72 activity was 2.91%/0%; no compiler appeared in 144 observations, and observed CPU 8 frequencies ranged from 4.674 to 5.046 GHz. Raw samples, all rounds, setup/teardown samples, process observations and per-core activity remain archived without trimming. Periodic observations cannot exclude every brief external disturbance.

The experiment establishes exact error translation and a mixed-text improvement in this bounded run, but removing the duplicate validation alone did not eliminate the malformed-input regression. It remains outside the product, separately preserved from the earlier rejected implementation. A future input guard would need broader malformed-prefix and suffix workloads before any acceptance decision.
