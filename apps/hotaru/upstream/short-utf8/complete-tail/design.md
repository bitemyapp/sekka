# Isolated complete-tail UTF-8 guard

This candidate remains outside the product. It starts from the current qualified ASCII-store baseline and preserves the rejected padded-fallback and error-translation variants independently.

The narrower ASCII-ending eligibility test covered only 1 of 38,175 helper calls in the approximately 2 MiB mixed corpus, and 3 of 152,694 in the approximately 8 MiB corpus. It was not given a native timing window. The reviewed pattern ends in an emoji immediately before CRLF, so most text spans end non-ASCII.

The replacement guard examines at most four trailing byte positions using safe slice patterns. It accepts exactly a complete final scalar: ASCII; C2–DF plus one continuation; E0 with A0–BF, ED with 80–9F, or other legal three-byte leads; F0 with 90–BF, F4 with 80–8F, or F1–F3 plus legal continuations. All other endings use the unchanged baseline validator. Only runs of 32–63 bytes beginning with a plausible multibyte lead are eligible for padding.

A complete final scalar rules out an incomplete original suffix. Its leading byte is not a continuation, so any earlier unfinished sequence is diagnosed before or at that leading byte, within the original slice. If an earlier error exists, appended ASCII NULs neither repair it nor change its first invalid start or error length. If no error exists, the original is complete valid UTF-8 and appended ASCII remains valid. Therefore padded errors can be returned unchanged: no private error record or error translation is required.

Success retains the prior narrowly reviewed unsafe conversion of the validated original input to str. The initialized stack block contains exactly that input followed by NULs; a NUL cannot complete a truncated multibyte scalar. The returned string borrows the original input, never the stack block. Copying and guard matching are bounds checked, and protected-page tests cover original-slice ends.

Independent standard-library suffix decoding checks the guard against every scalar and its prefixes, plus boundary-class combinations through four bytes. Existing exhaustive three-byte suffixes, all scalar/prefix validation, malformed restrictions, NULs, pair mutations, original-pointer identity and guard-page tests remain. The actual Terminal.feed coverage test reports 99.85% eligibility for both mixed corpus sizes, with counters compiled only into tests. Debug and release each pass 149 tests with two existing intentional ignores; strict Clippy, three Miri tests and unchanged-library fixtures pass. Exact source and binary identities are in summary.json.

Expanded native inputs deliberately include invalid second bytes, overlong and surrogate encodings, values above the Unicode maximum, invalid interior sequences, malformed complete-looking tails and truncated 2/3/4-byte suffixes. The guard may still have overhead on these inputs. No performance improvement or integration recommendation follows before that paired gate completes.

The completed 28-case native gate rejected this candidate for integration: mixed improves, but several malformed-prefix workloads regress consistently by roughly 10–14.5%. All raw samples, tails and round order are preserved in the adjacent summary/archive. A separate direct Ghostty comparison is diagnostic evidence gathering only; it does not change this self-regression or authorize integration. No further guard variant is planned.

Generated input corpora remain outside Git at the recorded local/workstation paths. `tools/generate-malformed.py` reconstructs all ten exact malformed inputs from `expanded-manifest.json` and verifies every byte length and SHA-256. The common four corpora use the retained corpus generator and recorded hashes.
