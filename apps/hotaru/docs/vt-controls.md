# Repetition, saved modes and alternate screens

This slice adds REP (`CSI Ps b`) and one saved slot for each of the 24 DEC
modes already implemented by Hotaru. It also corrects alternate-screen
transitions and decoded C1 handling. Horizontal margins, reverse wrap, graphics
and other protocol families remain separate work.

`tools/vt-controls-oracle.py` combines 415 targeted cases with 739 independent
adversarial cases from `tools/vt-controls-review.py`. All 1,154 streams match the
actual pinned Ghostty library at chunks of one, seven and 65,536 bytes:
3,462 comparisons. Expected cells, styles, cursor, selected mode flags and PTY
replies in `upstream/vt-controls-fixtures.json` come only from that library.
The ordinary Cargo test repeats these comparisons without linking Ghostty.
The fixture records both generator hashes and the library identity. The
comparison normalizes blank/spacer cells and does not measure rendering,
clipboard transport or every terminal mode.

## Behavior and ownership

REP retains the last independently printed input scalar, before charset
mapping. Combining additions do not replace it. Repetition uses current style,
hyperlink, protection, charset and insertion/wrap state through the existing
print paths. A bounded 1 KiB stack buffer amortizes those paths; scratch storage
does not grow with the requested repeat count. The parser's bounded parameter
representation limits the count. RIS clears the remembered scalar; DECSTR and
screen changes preserve it.

UTF-8-encoded U+0080–U+009F are ignored before consuming a single shift or
changing REP/grapheme state, matching Ghostty's stream dispatcher. Bulk text
paths hand these scalars to that same ignored dispatch. Raw invalid bytes
continue through the incremental decoder. Remaining Latin-1 characters,
including soft hyphen, occupy one cell in both width modes. The exhaustive
width-fastpath tests now account for that measured Latin-1 rule and the C1
sentinel used to leave the bulk path; Unicode property tables are unchanged.

XTSAVE/XTRESTORE use a fixed 32-bit saved-value field. Repeated saves replace
the slot; restores can be repeated. Unsaved slots use protocol defaults,
including a reset 2027 slot even when the application configures Unicode width
by default. Restores invoke actual mode side effects. Unknown modes stay
unrecognized. DECECM 117 and the reference's fixed-reset ANSI modes report
permanently reset without pretending to implement mutable behavior.

Mode bits for 47, 1047 and 1049 are independent of which screen is active.
Mode 47 preserves both screens; 1047 clears the alternate screen on exit.
Every 1049 enable saves the active cursor and clears the alternate screen,
even when already there. Every disable restores the primary cursor, even when
already on primary. Clearing on entry uses the destination screen's old pen
before copying the source cursor and pen. Transitions copy pending wrap and
preserve shared margins. Saved cursors do not restore visibility.

Alternate clearing gives its content a new identity without replacing its
Kitty keyboard stack or saved cursor. Primary history identities and owned
captures remain valid across alternate use. A separate regression holds linked
repeated history across clearing and terminal destruction.

## Review and performance

Independent adversarial tests found decoded-C1 and old-destination-pen bugs;
the original failing report is retained under `upstream/vt-controls-review/`.
The initial corpus did not expose them. Both fixes and the Latin-1 correction
are included in the combined candidate.

Initial REP integration increased several feed timings because its buffer and
alternate-transition code affected the common parser path. Keeping those
helpers out of line removed the styled-text regression. Pre-outline and
outlined experiments retain separate source identities under
`upstream/vt-controls/`; they precede final C1/color fixes and repaired SIMD
integration. Some Unicode overhead remained. The SIMD dependency has its own
[unsafe audit and measurements](utf8-simd.md). Final combined measurements and
native qualification must identify their exact source and executable.

The [combined checkpoint](links-vt-checkpoint.md) now records those exact
identities, final comparisons, regressions and five packaged Wayland suites.

```sh
python3 tools/vt-controls-oracle.py \
  --library /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so \
  --replay /path/to/hotaru-replay --report /tmp/vt-controls-report.json \
  --compact-fixtures /tmp/vt-controls-fixtures.json
cargo test --locked -p hotaru-core --test vt_controls
```
