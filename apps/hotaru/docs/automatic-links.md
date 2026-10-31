# Native automatic links

Hotaru now detects the pinned Ghostty default URL and path patterns in displayed
text. Aizome's modifier-click opens the exact matched target. Explicit OSC 8
links take precedence, and stale results cannot activate after the displayed
frame, pointer gesture, pane, request or accepted working directory changes.
Relative paths use the last accepted local OSC 7 directory when the resolved
path exists. Arguments reach the desktop opener literally.

Matching and filesystem resolution run outside the UI and PTY workers. Each
worker retains one active request, one replaceable pending request and one
unread result; completion notifications do not carry snapshots. Resolution uses
a supervised child with bounded output and cancellation. The UI never waits
for that child or a foreign regex call to return.

The matcher retains Ghostty's exact external Oniguruma 6.9.9 dependency behind a
checked Rust ownership boundary. This is not a rewrite of that external C
library. Its original BSD notice accompanies both Nix package definitions.
No macOS application or platform code was added.

Qualification used `ssh wx-workstation` exclusively. The integrated source
matches all 1,355 files in the final tested inventory, including 91 changed or
added product files. The native package passed 73 ordinary tests, with seven
existing intentional ignores, and strict all-target Clippy. Both the new
automatic-link and existing OSC 8 private Wayland suites passed. Independent
reference gates passed 11,088 logical-selection/matched-endpoint cases and
3,474 exact-engine/rule cases. The old-suite failure that exposed an overly
eager OSC 8 hover refresh is retained with the corrected result.

Coverage remains limited to complete context within the displayed snapshot.
Clipped logical lines, the first visible row when earlier history exists, more
than 4,096 columns, or more than 64 KiB of logical text produce no automatic
match. Full scrollback context, configurable rules, always-on highlighting and
automatic-link double-click selection remain open.

The 25 ms matching deadline is cooperative. One adversarial 64 KiB input took
about 0.67 seconds inside a bounded foreign call; cancellation is checked after
the call returns. The path helper's 250 ms deadline begins after spawn, and a
child in uninterruptible kernel I/O retains its one slot until reaped. These
limits and software-compositor checks do not establish whole-application
latency, memory or performance parity. A new combined Nix package checkpoint is
also still required.

See the [integration receipt](../upstream/native-auto-links/integration.json),
[source inventory](../upstream/native-auto-links/final-source-after.json), and
[detailed qualification record](../upstream/native-auto-links/README.md).
The [compressed evidence](../upstream/native-auto-links/evidence.tar.gz)
preserves the exact patch, reference adapters, raw results, failed attempts,
logs and private-compositor captures.
