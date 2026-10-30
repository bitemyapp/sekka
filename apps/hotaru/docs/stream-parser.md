# Stream parser and width agreement

The [2026-10-30 comparison](ghostty-comparison-2026-10-30.md) ran Hotaru
through Ghostty's own benchmark generators. Two of the generated inputs, random
UTF-8 and random OSC, left screens different from the pinned libghostty-vt.

A prefix search over those inputs found where they first diverged. A seeded
fuzzer of control-heavy byte strings and a sweep of CSI parameter counts then
found the related rules. Hotaru now follows the reference on each of them.

## Widths

**Legacy width mode now takes each scalar's width from the same pinned uucode
table** that mode 2027 already used, and that the reference's print path uses
in both modes:

- U+0000–U+00FF are one cell wide.
- Everything above that comes from the table.

Before, legacy mode used the `unicode-width` crate, which disagreed with the
table on 100 scalars in 71 ranges:

- **Ghostty gives width 1:** spacing vowel signs, length marks and viramas
  (Bengali, Tamil, Kannada, Balinese, Grantha and others), and prepended
  concatenation marks such as U+0605.
- **Ghostty gives width 2:** the Hangul and Vietnamese tone marks
  (U+302E/F, U+16FF0/1).
- **Ghostty gives width 0:** U+115F, U+2028/9 and U+2D7F.
- **Others:** a few Khmer and Unicode 17 scalars.

The first random-UTF-8 difference was U+302F. The crate is no longer a
dependency.

## Parser rules

- **DEL in ground state** prints, one cell wide, where xterm drops it.
- **Raw C1 bytes (0x80–0x9F)** act after ESC, in CSI and DCS headers, in an
  ignored CSI, and inside SOS, PM and APC strings:
  - CSI, DCS, OSC, SOS, PM and APC start their sequences;
  - ST ends a string;
  - the rest execute as `ESC (byte − 0x40)`.

  OSC and DCS payloads keep these bytes as data, and the header states ignore
  0xA0–0xFF.
- **DCS headers** ignore C0 controls and are abandoned at a colon. DCS
  payloads keep C0 controls; only DEL is dropped.
- **DECRQSS** gets no reply when its request is longer than two bytes,
  matching the reference's two-byte buffer.
- **Private markers** (`<`, `=`, `>`, `?`) count only as the first byte of a
  header; anywhere later they make the sequence invalid.
- **Parameter limit:** a 24th separator drops the CSI or DCS.
- **Colon subparameters** are accepted only by SGR (`m`); any other CSI with a
  colon is dropped.
- **An escape with three or more intermediates** ends at any final byte from
  0x30 to 0x7E, without dispatching.

## CSI commands

- **Extra parameters** make a command a no-op:
  - beyond one for `@ A–G I–M P S T W X Z \` a d e g j k n` and for
    DECSED/DECSEL;
  - beyond two for CUP, HVP and DECSTBM.
- **An explicit zero** is a no-op for CHT, CBT, IL, DL, DCH, SU and SD.
  - HPR and VPR move by zero, which still clears a pending wrap.
  - The cursor movements, ICH and ECH treat zero as one.
- **HPB and VPB** (`CSI j`, `CSI k`) move like CUB and CUU.
- **Tab-stop controls:** CTC (`CSI W`: 0 sets a stop, 2 clears it, 5 clears
  all) and DECST8C (`CSI ? 5 W`) manage tab stops.
- **RI at the top margin** scrolls without clearing a pending wrap, as the
  reference's scroll-down does.

## Evidence

Evidence is in [`upstream/stream-parser/`](../upstream/stream-parser/). The
final build is `6c4f06ad…`
([`upstream/candidate-stream-parser-build.json`](../upstream/candidate-stream-parser-build.json)).

- **Ghostty's suite inputs:** all seven 64 MiB generated inputs from the
  comparison now leave the same screen as the library at 120×80. The seeded
  ones reproduce the recorded files byte for byte, random UTF-8 included; ASCII
  and OSC are fresh time-seeded files.
- **Fresh generator seeds:** twelve 8 MiB inputs (random UTF-8 with four seeds;
  OSC at the default and at 10 % and 90 % valid; ASCII; mixed styled) match at
  every power-of-two prefix and at the end.
- **Widths:** every scalar from U+00A0 to U+10FFFF prints with the library's
  width, 1,111,904 comparisons.
- **Oracle cases:** `tools/stream-parser-oracle.py` records 855 targeted and
  seeded cases from the actual library. All match at chunk sizes of 1, 7 and
  65,536 bytes: 2,565 comparisons. The Cargo test
  `stream_parser_and_widths_match_actual_pinned_library` replays them without
  linking Ghostty.
- **Fuzzing:** on the final build, 60,000 fresh control-heavy cases (six seeds)
  show no difference.
- **CSI sweep:** `sweep.json` covers 44,100 sequences: every final byte with
  each private marker and intermediate, and 0–4 parameters from six value sets.
  Four commands still differ, all named below.

## Remaining differences

These are missing features, not parser disagreements:

- **Unimplemented modes.** DECRQM reports them unrecognized where the
  reference knows them: ANSI 2 (KAM) and 12 (SRM); DEC 3, 4, 5, 8, 40, 45,
  1039, 1045, 2026, 2031, 2033 and 2048.
- **DECSASD** (`CSI 1 $ }`): the reference stops printing to the main display.
- **XTVERSION** names Hotaru, by design.

## Performance

`tools/benchmark.py` on the reviewed corpus with the final build, at a load
average of about 1.3; reports are in
`upstream/benchmarks/stream-parser-*.json`:

| Pattern | Ghostty | Hotaru | Hotaru ÷ Ghostty | Hotaru at the clipboard checkpoint |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 6.71 ms | 5.63 ms | 0.840 | 5.66 ms |
| Styled text | 31.98 ms | 29.05 ms | 0.908 | 27.70 ms |
| Unicode | 31.05 ms | 31.77 ms | 1.023 | 33.99 ms |
| Cursor/erase | 750.3 ms | 94.07 ms | 0.125 | 92.0 ms |

**Unicode is 7 % faster:** legacy widths now use the table lookup that mode
2027 already used, in place of the crate.

**Styled text is about 4 % slower and cursor/erase about 2 % slower.** As first
written, the fix cost styled text 10 % (27.9 → 30.4 ms, back to back with its
parent). That corpus is about 700,000 SGR sequences, so the cost was per
sequence. Two changes recovered most of it:

- `csi()` looks up each command's parameter limit with a `match` instead of
  searching a byte string.
- The parser's DCS-only checks moved inside the C0 and separator arms, so the
  header's arms no longer overlap.

Neither alone helped much (29.2–29.8 ms and 30.4 ms); together they gave
28.7–28.9 ms. Skipping the parameter-count gate entirely was slower still
(32.0 ms), so the rest is probably code layout rather than the new checks.
`perf` isn't available to users on wx, and the cause wasn't isolated further.
The variants and their runs are in
[`dispatch-ab.json`](../upstream/stream-parser/dispatch-ab.json).

```sh
python3 tools/stream-parser-oracle.py \
  --library /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so \
  --replay /path/to/hotaru-replay --report /tmp/stream-parser-report.json \
  --compact-fixtures upstream/stream-parser-fixtures.json
cargo test --locked -p hotaru-core --test vt_controls
```
