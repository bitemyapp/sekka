# Unicode grapheme mode

Hotaru's existing fixtures and performance baselines use the C library's legacy
width default. The native Ghostty app instead configures Unicode grapheme mode
before consuming terminal output. These are different configurations, and their
performance evidence must remain separately identified.

The implementation target is Ghostty
`6ba144158577d033ccb6ed4b574e4013957c9161`, with uucode
`ea62149739404a73c202b48a33bf6dd2af4bd9b0` and its Unicode 18.0.0 data. Generic
Unicode segmentation is insufficient: this version tailors emoji modifiers and
uses updated Indic-conjunct rules. The Rust kernel uses the exact generated
property data, a safe table lookup, and the corresponding state transitions.
No Zig library or compiler is a product runtime dependency.

`tools/unicode-tables.py` translates the reference build's generated `props.zig`
and verifies the pinned enum layout. `upstream/unicode-mode/unicode-tables.json`
records the source, generator and output hashes. `LICENSE.ghostty`,
`LICENSE.uucode` and `LICENSE.unicode` preserve the applicable notices. The
Unicode data notice was retrieved from <https://www.unicode.org/license.txt>.

The standalone Rust kernel matches the actual C API for every valid
Unicode scalar's width (1,112,064 comparisons) and 56,601 grapheme measurements.
Those measurements include property-representative triples, 630 applicable UCD
test vectors and 20,000 seeded longer sequences. This establishes the isolated
kernel; terminal integration has separate evidence below.
See `upstream/unicode-mode/kernel-report.json` for hashes and counts.

## Configured defaults and DEC mode 2027

The API is `Terminal::set_default_grapheme_width(GraphemeWidthMethod)`
with `Legacy` and `Unicode` variants. It sets both the current mode and the
remembered reset default, as the reference C API's `MODE_DEFAULT` does.
`Terminal::new` retains its legacy default; native construction selects Unicode
before PTY input. `Modes::grapheme_cluster` exposes the current mode.

Actual reference behavior is:

| Operation | Result |
| --- | --- |
| DECSET 2027 | Enable Unicode clustering |
| DECRST 2027 | Disable clustering, including with a Unicode configured default |
| DECSTR | Preserve current mode 2027 |
| RIS | Restore the configured default |
| XTSAVE / XTRESTORE | Save and restore the current mode |
| DECRQM 2027 | Report 1 when enabled, 2 when disabled |

The `Config.zig` comment says resetting 2027 restores the configured method;
the pinned mode implementation and direct C API observations disagree. The
implementation follows actual observed behavior. The native app applies its
choice to newly constructed terminals rather than mutating existing sessions.

## Bounds and terminal evidence

The upstream per-cell grapheme limit is 64 **suffix** scalars, plus the base
scalar. Actual terminal probes confirm 65 total scalars in both width modes.
Rust now accepts 65 total scalars and validates at most 260 UTF-8 bytes in the
history codec, with a 266-byte encoded-entry bound. Allocation-capacity checks
and the existing archive/block byte limits remain enforced. Width mutations
still occur before a full cluster rejects an additional suffix: actual C API
fixtures show a full 65-scalar cluster narrowing on VS15 and widening again on
VS16 without storing either additional selector.

`upstream/unicode-mode-fixtures.json` contains 484 actual-reference terminal
cases and 968 independent reference chunk comparisons. Rust checks all 11,866
two-part byte splits. Cases include one-column grids, both wrap modes, wide
cell boundary repairs, cursor and pending-wrap behavior, emoji/RI/Hangul/Indic
clusters, mode transitions, links/styles between cluster components, 54 resize
sequences and the cluster cap. Tests compare full cell styles, widths, text,
links, cursor, replies, mode state and plain/VT formatting. The public VT copy
API's documented default-color prelude and palette-to-RGB conversion are
handled explicitly; original C formatter bytes remain in the fixture.

Additional tests exercise 260-byte clusters through compressed history,
reflow, search, copying and owner teardown, plus byte-budget invariance across
input chunks, metadata changes and held snapshots. A separate read-only review
checked row ownership and codec allocation bounds. The combined clean run in
`upstream/unicode-mode/validation-combined.log` passes 97 core tests, 20 I/O
tests, the replay fixture test and strict workspace Clippy. An earlier isolated
I/O EOF-readiness test failure is retained in the evidence; its test correction
was independently verified before that combined run.

This remains partial module coverage. Horizontal-margin and graphics-specific
grapheme interactions are outside the current terminal implementation. The
Unicode-mode table matches every reference scalar width; legacy printing
retains its earlier width policy outside the named compatibility fixtures.

## Performance evidence

`upstream/unicode-mode/candidate-build.json` identifies the isolated measured
source, including byte-budget accounting and its metadata-preserving scalar
fallback. This is before integration of later search anchors; old checkpoints
are unchanged. Both engines use the same configured width mode, zero history,
64-KiB chunks, CPU affinity, three warmups and 20 alternating-order samples.
Full compared state must match before timing.

| Reviewed pattern | Rust/Ghostty, legacy | Rust/Ghostty, Unicode |
| --- | ---: | ---: |
| ASCII | 1.712 | 1.663 |
| Mixed Unicode | 1.428 | 1.232 |
| Styled | 1.686 | 1.710 |
| Cursor/erase | 0.152 | 0.153 |

The separately labeled dense-wide diagnostic is 2.401 in legacy mode and 2.263
in Unicode mode. Ratios above one are slower. **Performance parity is not met.**
On this same Rust binary, enabling Unicode adds about 16% to the mixed Unicode
workload while ASCII, styled and cursor timings are nearly unchanged. The
larger regressions from earlier snapshots therefore require a separate audit
of budget accounting and metadata fallback. These measurements do not qualify
rendering, clipboard transport, the full application or every Unicode workload.

The subsequent `metadata-guard-profile.json` is a causal counterfactual, **not a
candidate for use**. Disabling the metadata scalar guard improves mixed Unicode
by about 28% on its 2-MiB diagnostic, but loses allocation-order equivalence and
can change retained history. Its source metadata explicitly identifies that
limitation. The next experiment keeps every per-cell metadata operation in
order while reducing repeated row ownership checks.
