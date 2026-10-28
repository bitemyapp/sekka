# Sukumo

Sukumo (蒅) is Aizome's toolkit: the Wayland engine and the native Rust
toolkit every Aizome program draws with, from the compositor's own
surfaces to the calculator. Sukumo is the composted indigo leaf an indigo
vat is built from. You never see it in the finished cloth.

It began as part of [Sidestep](https://github.com/bitemyapp/sidestep),
which runs objc2 programs on Linux with a Rust Objective-C runtime and
Foundation and AppKit written in Rust. Those programs and Aizome's shared
an engine on Sidestep's `aizome` branch. Sukumo keeps the engine and the
native toolkit and leaves the Objective-C runtime and frameworks in
Sidestep. Nothing here uses objc2.

| Crate | What it is |
| --- | --- |
| `crates/sidestep-engine` | The render thread that owns the Wayland connection, the CPU rasterizer, Core Animation-style layer trees, the text engine (fontconfig, shaping, bidi, color emoji), keyboards through XKB keymaps and compose tables, the clipboard, drag and drop, outputs and the desktop's settings |
| `crates/sidestep-ui` | The toolkit programs use: an `App` that calls a `Handler`, windows and layer surfaces, a `Canvas` to draw on, `TextLayout`, and lock screens, activation tokens and foreign parents for a desktop's own programs |

The crates keep their Sidestep names, so code and comments still say
`sidestep_ui` and "Sidestep's engine", and the environment variables are
still `SIDESTEP_*` (`SIDESTEP_BACKEND=null`, `SIDESTEP_APPEARANCE`,
`SIDESTEP_FRAME_STATS`, `SIDESTEP_TRACE_FRAMES` and the rest). Changes
written against Sidestep's `crates/` apply here with
`git apply --directory=desktop/aizome/sukumo`.

## Where it came from

The source is Sidestep's `aizome-combined` branch at
[`853fbb3`](https://github.com/bitemyapp/sidestep/tree/853fbb3cda7fa2a7843674432fd8962e6263524e):
the `aizome` branch, the video subsurface Utsushi uses
([bitemyapp/sidestep#6](https://github.com/bitemyapp/sidestep/pull/6)), and
the clipboard and text attachments Omperor uses
([bitemyapp/sidestep#7](https://github.com/bitemyapp/sidestep/pull/7)).
[docs/imports.json](../../../docs/imports.json) records it, and the
[migration record](../../../docs/migration.md#sukumo) says what changed
on the way in. The two crates are as they were there, apart from where
three test fixtures live. The fixtures are now in `fixtures/` rather than
Sidestep's `conformance/tests/fixtures/`.

Sidestep's history and its design notes stay in its repository. In
[docs/architecture.md](https://github.com/bitemyapp/sidestep/blob/853fbb3cda7fa2a7843674432fd8962e6263524e/docs/architecture.md),
the sections "The engine" and "The native toolkit" describe the code here.
The rest covers the Objective-C side, though the AppKit sections around
those two say a good deal about the engine underneath.

## Building and testing

Sukumo is a Cargo workspace of its own. Aizome's workspace excludes this
directory and takes the two crates by path, so building Aizome builds
them. Its lock is the subset of Aizome's that these crates use. When
Aizome's lock moves one of their dependencies, prune a copy of Aizome's
lock to make Sukumo's again:

```sh
cp desktop/aizome/Cargo.lock desktop/aizome/sukumo/Cargo.lock
cargo metadata --offline --format-version 1 --manifest-path desktop/aizome/sukumo/Cargo.toml >/dev/null
```

Its tests run without a display, through the null render thread and
offscreen, and the Wayland ones run under a headless sway (1.11 or later,
which closes a layer surface when its output goes):

```sh
nix develop .#aizome -c cargo test --locked --manifest-path desktop/aizome/sukumo/Cargo.toml
nix build .#checks.x86_64-linux.sukumo -L
```

The check runs the unit tests, the toolkit's headless, offscreen, desktop,
scrolling and drag-out tests, and its Wayland system and desktop tests under
`scripts/headless-wayland`. It fails if the Wayland tests skip.
`frame_budget` and `image_budget` only measure, so they run by hand:

```sh
cd desktop/aizome/sukumo
SIZE=2880x1800 SCALE=2 scripts/headless-wayland cargo test --release -p sidestep-ui --test frame_budget
```

Run them with `WAYLAND_DISPLAY` and `DISPLAY` unset, so nothing reaches
the desktop you're using. Keyboard tests read `XKB_CONFIG_ROOT`.

## License

MIT OR Apache-2.0, as Sidestep is: [LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE). `fixtures/DejaVuSans.ttf` is under
its own license ([fixtures/DejaVuSans-LICENSE.txt](fixtures/DejaVuSans-LICENSE.txt)).
