# Exact retained Oniguruma engine

Version6.9.9, selected by Ghostty commit
`6ba144158577d033ccb6ed4b574e4013957c9161`.
Ghostty package URL:
https://deps.files.ghostty.org/oniguruma-1220c15e72eadd0d9085a8af134904d9a0f5dfcbed5f606ad60edc60ebeccd9706bb.tar.gz
Zig dependency hash: `N-V-__8AAHjwMQDBXnLq3Q2QhaivE0kE2aD138vtX2Bq1g7c`.

`sources.txt` preserves the48 C translation units from Ghostty's
`pkg/oniguruma/build.zig`; `src/` contains their unchanged recursive local
includes. `LICENSE` is the original COPYING file. Linux config macros are
reproduced in the crate's build script. No Apple SDK or platform build scripts
are retained. Source hashes and independent external-reference checks are in the
accompanying evidence manifest. This crate deliberately does not use cached
onig_sys69.9.3, which bundles6.9.10 rather than the required6.9.9.

The private Rust boundary initializes this one engine once and never calls
`onig_end`. No other process component may mutate its global configuration or
terminate it while workers exist. Native integration must keep this as the only
Oniguruma consumer or separately establish compatible global ownership.
