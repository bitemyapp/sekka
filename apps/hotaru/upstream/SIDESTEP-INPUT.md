# Native input metadata patch

Apply `sidestep-input.patch` after `sidestep-clipboard.patch` to the pinned
Sidestep revision documented in `SIDESTEP.md`. The clipboard patch is unchanged.

```sh
git -C vendor/sidestep apply --check ../../apps/hotaru/upstream/sidestep-input.patch
git -C vendor/sidestep apply ../../apps/hotaru/upstream/sidestep-input.patch
```

The patch adds `KeyEvent.native` metadata to Sidestep UI: actual XKB keysym,
layout/compose text without Control transformation, unshifted layout scalar and
consumed modifiers. Num Lock has its own modifier bit, distinct from the existing
AppKit numeric-pad flag. The backend uses its existing pure Rust kbvm keymap and
current layout group; it does not infer characters from US key positions.

Native clients also receive modifier key presses and releases. The AppKit
compatibility layer continues receiving these solely as flags-changed events,
and text-widget insertion still suppresses Control/Super shortcut text. Holding
a modifier does not stop an existing repeating ordinary key.

Hotaru maps physical identity separately from layout identity, preserves keypad
remapping through actual keysyms, and sends normalized events to its PTY worker.
Only that owner encodes terminal keyboard/mouse protocols. Aizome KeyEvent test
constructors initialize the new metadata field explicitly.

Validation on `wx-workstation`, with system XKB data present:

```sh
cargo +1.95.0 test --manifest-path desktop/aizome/Cargo.toml -p sidestep-engine --lib backend::keyboard::tests
cargo +1.95.0 test --manifest-path desktop/aizome/Cargo.toml -p sidestep-ui --lib event::tests
```

Nine keyboard tests and five UI event tests pass, including US Shift/Control,
German layout identity, consumed Shift, Num Lock and compose behavior. Keyboard
tests report a skip when system XKB data is unavailable; the workstation run did
not skip these tests. Hotaru additionally tests negotiated Kitty press/release
and SGR mouse input through a real PTY. Private Wayland smoke evidence is stored
separately with source and binary hashes.
