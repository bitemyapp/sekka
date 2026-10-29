# Hotaru 蛍

Hotaru ("firefly") is a Linux terminal being developed for Sekka and Aizome.
It uses Aizome's native menus, toolbar, typography and theme, with a Rust
terminal engine and Linux PTY implementation. It contains no GTK frontend,
macOS application code or Zig runtime.

**The Ghostty rewrite is in progress. Hotaru is not yet a feature-complete or
performance-qualified replacement.** The current app provides shell sessions,
tabs, split panes, saved settings, selection, scrollback and native text input.
The port targets the pinned
Ghostty `1.4.1-dev` source, including the large-clipboard behavior identified
in bitemyapp's fork. A limited passing terminal test suite is not full Ghostty
compatibility.

Copy formatting runs on a bounded worker with owned snapshots; clipboard
publication and paste delivery are ordered by request and session identity.
Slow or abandoned clipboard peers must not block the interface thread.

On Aizome built from this branch, open **Hotaru** in the application launcher
or run `aizome-hotaru`. See the [usable checkpoint](docs/usable-checkpoint.md)
for the ready-to-run development launcher, packaged validation and deferred work.

See [development and remaining work](DEVELOPERS.md) and the
[upstream audit and coverage ledger](docs/upstream.md) for build commands,
validation evidence, the memory design and the unresolved port scope.
