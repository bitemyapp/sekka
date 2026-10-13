# Kakehashi 架け橋

*A bridge from an image to a USB drive.*

Kakehashi writes Sekka, or any other disk image, to a USB drive on Linux,
macOS or Windows, then reads every byte back to make sure the drive holds
exactly the image. It is the way to make a Sekka installer drive without
reaching for `dd`.

## Using it

1. **Image.** Kakehashi lists Sekka's recent nightly builds, newest first,
   with when each was built and from which revision. Download one and
   Kakehashi checks it against its published SHA-256 as it arrives; an
   interrupted download picks up where it stopped. Or choose an image you
   already have, or drop it on the window. If a `.sha256` file or
   `SHA256SUMS` sits beside it, Kakehashi checks the image against it and
   won't write one that differs.
2. **Drive.** Only drives that are safe to offer are offered: USB or
   removable, 2 TB or smaller, writable, with media in them, and not
   holding the running system or the image itself. Every other drive is
   listed underneath with the reason it isn't offered. **Show all external
   drives** also offers external drives that are large or don't say they
   are removable. Internal drives are never offered.
3. **Write.** Kakehashi names the drive, its size and serial number before
   erasing it, and asks once more. Your system then asks for an
   administrator's password. Writing shows its speed and the time left, and
   can be stopped. A drive stopped partway is left partly written: write it
   again, or format it, before using it.

Before it writes, Kakehashi checks that the drive is still the one you
chose, so a stick swapped for another in the meantime is not erased. It
unmounts the drive's volumes, writes the image, clears the end of the drive
(where an old partition table's backup would otherwise linger), reads it all
back, and ejects it.

## Current limits

- Kakehashi is new. Writing has been tested in a disposable Linux virtual
  machine with a virtual USB stick. It has not yet written a real drive on
  macOS or Windows.
- There are no signed builds or installers for macOS or Windows yet. Their
  builds, and Linux's, are kept for a while by GitHub after each change, as
  artifacts of the
  [Kakehashi workflow](https://github.com/bitemyapp/sekka/actions/workflows/kakehashi.yml),
  and need a GitHub account to download. The first time it is opened, macOS
  refuses until you allow it under **System Settings → Privacy & Security**,
  and Windows SmartScreen may warn about it.
- On Linux, Kakehashi asks for the password through polkit (`pkexec`), so a
  polkit agent must be running, as it is in Plasma, GNOME, Xfce and Tatami.
  NixOS 26.11 and later install `pkexec` only on request: Sekka does, but
  elsewhere set `security.polkit.enablePkexecWrapper = true`.
- Images are written as they are. Kakehashi doesn't make Windows installer
  drives or add persistent storage.

## Installing

On Sekka, open **Yukimi → Discover** and install Kakehashi from
**Utilities**, or choose it among the applications when installing Sekka.

On another Linux system with Nix:

```sh
nix profile add github:bitemyapp/sekka#kakehashi
```

## Building

From the root of this repository, on Linux:

```sh
nix build .#kakehashi
nix run .#kakehashi
```

Elsewhere, with Rust 1.92 or newer, run `cargo build --release` here. Keep
`kakehashi` and `kakehashi-helper` side by side: the window starts the
helper from its own directory.

## License

Kakehashi is available under either the [MIT](LICENSE-MIT) or the
[Apache 2.0](LICENSE-APACHE) license, at your option.
