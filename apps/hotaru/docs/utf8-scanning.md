# UTF-8 scanner work and rejected dense-write experiments

Hotaru now remembers the remaining control-free byte range inside one `feed_inner` call. After UTF-8 validation rejects a sequence, the scalar parser consumes it without repeatedly scanning the entire printable suffix. The count is shortened after ASCII runs, valid UTF-8 runs, and individual parser bytes. It is discarded at a feed boundary, including an early return for reply backpressure. Classification depends only on the bytes; entering or leaving a parser state cannot invalidate it.

This fixes a quadratic malformed-input path. That checkpoint retained the standard-library UTF-8 validator and added no dependency. The integrated change and regression tests are recorded in `upstream/utf8-scanner-cache.patch`. The later [repaired SIMD integration](utf8-simd.md) changes validation while preserving this extent cache; neither checkpoint establishes terminal-wide performance parity.

## Qualification

The isolated candidate contains the current core and stable I/O source, with only this scanner change and the existing OSC 7 formatting repair. It passed 103 core tests, 23 ordinary I/O tests, the replay fixture test, and strict workspace Clippy. The two real-shell helper tests remain intentionally ignored by the ordinary Cargo suite; their separate native qualification is documented elsewhere.

The three new scanner tests cover 5,120 comparisons of whole/chunked input against the scalar bytewise parser in both width modes, malformed 2/3/4-byte sequences and invalid leads in 64 KiB runs, and interrupted UTF-8/parser states resumed with bounded reply budgets. Existing Unicode, hyperlink, resize, memory-pressure, compressed-history, and snapshot ownership tests also ran.

`tools/malformed-utf8-profile.py` checks the full compared grid and replies against pinned Ghostty before timing each of twelve inputs. It uses one CPU, the shared workstation lock, alternating process order, two warmups and twelve measured iterations per build, fresh zero-history terminals, and input preparation outside the timer. Each malformed run ends in a cursor-position query so reply preservation is also checked during every timed iteration.

| 64 KiB malformed prefix | Before | Standard validator plus range cache |
|---|---:|---:|
| repeated `FF` | 55.849 ms | 1.000 ms |
| repeated `C2 58` | 28.139 ms | 0.885 ms |
| repeated `E0 80 AF` | 37.815 ms | 0.948 ms |
| repeated `F4 90 80 80` | 41.965 ms | 0.953 ms |

Increasing input by four scales the corrected candidate by approximately 3.6–4.1, while the prior implementation grows by roughly 11–14. These measurements support the scanner's linear-work invariant; they are not measurements of rendering or large scrollback.

The separate valid-input comparison preserves the historical control and candidate source identities. ASCII differs by less than 1%; mixed Unicode changes by +0.8% in legacy mode and −4.1% in Unicode mode; dense Han changes by +3.2% and −2.3%, respectively. The cache does not resolve the outstanding dense Unicode throughput gap. Raw timings, source manifests, and test output are under `upstream/utf8-scanning/`.

Candidate source SHA-256: `aa694e32864aa80a3d5e9a47addeabc420c6b8f2b584fb2e59fd4532dfc2dd57`. Replay SHA-256: `cc785c745ae85e2eb255e61d6ee1be50f14de4700a1297983e4ad070dfdc541e`. The earlier control includes an unfinished I/O test snapshot; its measured core/replay targets passed. The corrected candidate includes the finalized I/O tests and passed the full workspace.

## Rejected alternatives

The captured reports under `upstream/utf8-scanning/rejected/` remain unchanged. Their presence does not qualify them for integration:

* A fixed-width inner run increased mixed Unicode time by 10.8–14.5% and legacy dense Han time by 14.2%. It was discarded.
* Caching Han classification inside the run improved one Unicode dense case but regressed legacy dense and mixed input. It was discarded.
* The published `simdutf8` 0.1.5 compatibility validator reduced dense Han feed time and preserved exact error boundaries in exhaustive 0–3-byte inputs and seeded long cases. Independent source review nevertheless found undefined pointer arithmetic in its AVX2 prefetch path: `.add(idx + 128)` can form a pointer beyond the allocation. Passing state tests and nonfaulting hardware prefetch do not establish correctness. Both this candidate and its variant with the scanner cache are rejected for integration in their measured form. Their reports may say that the narrow performance workload is comparable, but that field predates the independent unsafe audit and is not safety acceptance.

The dependency defect is corrected upstream by [commit 1890235e87c59a44d45efa6a44fcf9b7166183d4](https://github.com/rusticstuff/simdutf8/commit/1890235e87c59a44d45efa6a44fcf9b7166183d4), which uses `wrapping_add` at both prefetch sites. A minimally repaired dependency was subsequently audited and qualified separately; [that record](utf8-simd.md) includes provenance, licenses, CPU/load/prefetch review and Miri evidence. Stock registry 0.1.5 is not used by Hotaru.
