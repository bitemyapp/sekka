# Hotaru native URL/path matcher

This private Linux crate matches Ghostty's exact pinned default URL/path regex
against an immutable, bounded UTF8 logical slice. It is intended for an owning
worker: construct `Matcher` there, supply a `LogicalSlice` with byte-to-cell
`MappingRun`s, and call `find_at` with a cancellation flag and `Limits`.
`Match` retains the source and exposes immutable text/range/cell accessors.

This does not capture terminal state, choose semantic prompt boundaries, run a
hover worker, resolve filesystem paths or launch a desktop application. The
caller must preserve exact snapshot identity and reject stale results. OSC8
links take precedence in that caller. Do not derive cells by counting Unicode
scalars or match a truncated logical line as if its context were complete.

The fixed regex and90 upstream examples come from Ghostty commit
`6ba144158577d033ccb6ed4b574e4013957c9161`, `src/config/url.zig`.
Its notice is `LICENSE.ghostty`. The external C engine is unchanged
Oniguruma6.9.9; see `vendor/oniguruma/README.md` and its BSD license.
Rust owns the application logic and checked boundary; this is not a claim that
the external regex engine was rewritten in Rust.

Limits are explicit errors, never silently truncated targets. They bound input,
mapping, match/search counts and conservative granted retry allowances. The
engine stack limit counts entries, not bytes. Cancellation/deadlines are
cooperative between foreign calls, including their error paths. A single
adversarial64KiB input has taken about0.67 seconds inside one100k-retry search;
there is no25ms hard wall-time guarantee. Strict deadlines require a killable
helper process. No regex work belongs in UI or PTY-owner callbacks.
