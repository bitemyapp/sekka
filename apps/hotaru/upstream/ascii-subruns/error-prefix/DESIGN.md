# Isolated error-prefix ASCII handoff

This experiment does not change the product. The qualified baseline's entire
`screen.rs` is restored unchanged. Only `feed_inner`'s UTF-8 validation error
branch selects a separately outlined helper for a nonempty, revalidated prefix.
Validation success still calls the existing `State::print_utf8` directly.

The helper reads bytes without allocation and partitions only at printable
ASCII runs of at least 16 bytes. ASCII bytes cannot be UTF-8 continuation bytes,
so the starts and ends of those runs are boundaries in the validated borrowed
string. All slicing is checked. Non-ASCII spans go to the existing UTF-8 writer;
ASCII spans go to the existing packed ASCII writer. Both retain their original
charset, insert, pending-wrap, grapheme, REP and scalar metadata-order paths.
ASCII is a forced grapheme boundary in the pinned terminal semantics, while a
following non-ASCII combining scalar sees the actual preceding cells and can
join normally. No state is reconstructed from text or discarded between calls.

The malformed byte itself is not consumed by this helper. Only the validated
prefix length is removed from the input and cached control-free extent, then
the existing parser recovers from the same malformed bytes as before. Each
prefix is traversed once and consumed once, preserving the linear malformed
input behavior. There is no new unsafe code, allocation or foreign interface.

An incomplete scalar at a PTY chunk boundary is also a validation error, even
when later bytes complete a valid stream. That case selects this helper too;
it is inaccurate to claim that every valid stream is unaffected. The long
dense-prefix timing case and tests at chunk sizes 1, 7, 65,535 and 65,536 expose
the extra scan and exercise this boundary. Acceptance requires the full
32-case paired/reference run, including every earlier malformed input and the
common and dense-wide cases. No performance conclusion follows from source
inspection alone.
