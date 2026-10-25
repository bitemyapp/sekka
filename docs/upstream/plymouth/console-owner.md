# Plymouth: the console terminal after the GPU takes over

| | |
| --- | --- |
| Sekka's copy | [`console-owner.patch`](../../../installer/genkan/rust/system/boot-splash/plymouth/console-owner.patch), for 26.134.222 |
| Upstream version | [console-owner.patch](console-owner.patch): 647bb13f, for `main` at 074cc389 |
| Status | In review since 2026-10-25: [plymouth!520](https://gitlab.freedesktop.org/plymouth/plymouth/-/merge_requests/520) ([Submitting](README.md#submitting)) |
| Sent with | [keyboard-removal](keyboard-removal.md) |

## What goes wrong

Without an XKB layout for evdev input, plymouthd reads keys from the console
terminal, and only one renderer at a time is given that terminal. NixOS's
`vconsole.conf` has no `XKBLAYOUT`, so Sekka's systems read keys this way. The
device manager's `local_console_managed` records that a renderer has the
terminal. It was cleared only once no keyboards at all were left.

When a GPU's DRM driver takes over from simpledrm, the GPU's framebuffer device
can be found before its DRM device. It gets a renderer of its own, without the
terminal, which simpledrm's renderer still has. When simpledrm's renderer is
freed, the framebuffer renderer's keyboard is still there, so
`local_console_managed` stays set and the DRM renderer that follows doesn't get
the terminal either. No renderer reads the terminal after that: nothing typed
reaches plymouthd, and neither does Esc.

## What it did in Sekka

`encrypted-boot-splash` asks for a passphrase with `plymouth ask-for-password`
after the GPU's driver has taken over. In 4 of 8 VM boots it got no keys, Esc
included: the boots in which `/dev/fb0` got a renderer before `card0`.

## The fix

One commit, **device-manager: Free the local console when its renderer goes**
(647bb13f). `free_devices_from_device_path()` clears `local_console_managed` as
soon as the renderer it frees is the one with the console terminal, so the next
renderer made gets the terminal. A new `ply_renderer_get_terminal()` tells it
which renderer that is.

## Sekka's copy

[`installer/genkan/rust/system/boot-splash/plymouth/console-owner.patch`](../../../installer/genkan/rust/system/boot-splash/plymouth/console-owner.patch)
adds and removes the same lines in 26.134.222. Only the line numbers in its
hunks differ.

## Testing

The branch passes Plymouth's tests, its sanitizer build and its format check as
`main` does, and `encrypted-boot-splash` passes with Plymouth's `main` and both
branches ([Testing](README.md#testing)). Across 14 boots of the check, 12 with
Sekka's copies on 26.134.222 and 2 with `main` and both branches, keys typed
after the takeover reached plymouthd. The branch adds no test of its own.

## When Sekka's copy goes

Remove `console-owner.patch` from `boot-splash/default.nix` once Nixpkgs'
Plymouth is a release with this commit or another fix for the lost terminal,
and run `encrypted-boot-splash`. Run it more than once: the order in which the
devices appear differs from boot to boot, and the old code lost the terminal in
about half of them.
