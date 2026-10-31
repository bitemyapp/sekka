# Native clipboard patch

`sidestep-clipboard.patch` applies to Sekka's Sidestep submodule revision
`7bfacbbbc9cd70cf3442def70368e254f3c3712d`. It is a source patch, not a changed
submodule reference. Nix packaging and the development setup must apply it to
that exact source before building Hotaru's Aizome interface.

From the Sekka checkout, with an initialized and otherwise clean submodule:

```sh
git -C vendor/sidestep apply --check ../../apps/hotaru/upstream/sidestep-clipboard.patch
git -C vendor/sidestep apply ../../apps/hotaru/upstream/sidestep-clipboard.patch
```

Before removing an already applied patch, check that the reverse applies:

```sh
git -C vendor/sidestep apply --reverse --check ../../apps/hotaru/upstream/sidestep-clipboard.patch
git -C vendor/sidestep apply --reverse ../../apps/hotaru/upstream/sidestep-clipboard.patch
```

The patch adds `PreparedClipboardText` and `ClipboardRead` to `sidestep-ui`.
Text conversion, MIME preparation, and foreign clipboard reads belong on a
worker. `Cx::set_prepared_clipboard_text` commits shared allocations;
`Cx::clipboard_read` returns a small owned snapshot. Readers have cancellation,
an absolute timeout, a byte limit, and an identity that changes immediately
when another offer arrives or the application copies new text. Check
`is_current()` before applying a worker result. Existing synchronous APIs remain
available for existing clients; Hotaru must not use them for paste.

The native Wayland pipe callbacks yield after 256 KiB or 16 syscalls. Their
Smithay event sources use level-triggered readiness, so a still-ready pipe is
serviced on a later event-loop turn. Transfers have a 30-second absolute cap;
worker requests may set a shorter deadline. Canceled reads are checked by a
50-millisecond timer even when the pipe never becomes ready. Completion, peer
failure, and timer-installation failure remove the associated resources.

Validation on `wx-workstation`, Rust 1.95.0:

```sh
CARGO_TARGET_DIR=/home/callen/.cache/hotaru-aizome-target \
  cargo +1.95.0 test --manifest-path desktop/aizome/Cargo.toml \
  -p sidestep-engine -p sidestep-ui --lib clipboard
CARGO_TARGET_DIR=/home/callen/.cache/hotaru-aizome-target \
  cargo +1.95.0 test --manifest-path desktop/aizome/Cargo.toml \
  -p sidestep-engine --lib backend::selection::pipe_tests
```

These pass 12 engine clipboard tests, two UI clipboard tests, and four native
pipe-pump tests. They cover continuous readiness, multi-MiB exact data, zero
writes, broken pipes, interrupted syscalls, size limits, stale offers,
cancellation, and absolute deadlines under a trickling producer. Hotaru's
separate I/O crate adds 19 Linux PTY and real-pipe regressions, including
4-MiB-plus selections through a 4-KiB pipe and default-disposition SIGPIPE.
These component tests do not establish compositor-to-client end-to-end
clipboard behavior or performance parity with Ghostty.
