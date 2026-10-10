# Developing Sekka

Sekka is the source home for the ISO, Genkan installer, Tatami desktop, and Yukimi system manager. Work on a short-lived topic branch from `main`, open a pull request, squash-merge it, and delete the topic branch. There is no `stable`, `develop`, or release branch. Releases identify a commit with a version tag.

## Repository map

| Path | Responsibility |
| --- | --- |
| `flake.nix`, `flake.lock` | Public outputs and the authoritative external dependency lock |
| `installer/genkan/` | Rust installer, privileged helper, system modules, application catalog, storage tests |
| `desktop/tatami/` | Desktop module, configuration, Rust commands, Displays app, wallpapers |
| `apps/yukimi/` | GTK application, privileged helper, store/configuration/system libraries |
| `modules/`, `templates/` | Live ISO and the installed-system flake template |
| `nix/` | Component integration, packages, application resolution, prebuilt systems |
| `rust/`, `scripts/` | Image, VM, diagnostics, USB and release tooling |
| `tests/` | Generated-system configuration matrices and integration checks |
| `docs/` | Architecture, migration notes, screenshots, historical evidence |

The initial import preserves component layouts and separate Cargo workspaces. There are no first-party submodules or remote first-party flake inputs. `nix/components.nix` adapts the local components to the existing integration interface; its `calamares` name and the `calamares.*` options are compatibility names, not another source repository.

Sidestep, Upleft, Omperor, upstream forks, and Arch/Omarchy packaging stay outside Sekka. Add package definitions and pinned dependencies when the distro needs them.

## Build and check

Use x86_64 Linux with Nix and flakes enabled. The full image includes several desktops and an application cache: allow substantial disk space (a dedicated builder with at least 250 GiB free and 32 GiB RAM is a useful starting point). Component builds are much smaller. Never run installer storage tests against host disks.

```sh
nix develop
nix flake check --no-build --no-update-lock-file
nix build .#genkan .#tatami .#displays .#yukimi .#tools
nix build .#checks.x86_64-linux.storage
nix flake check --no-update-lock-file
nix build .#iso --no-update-lock-file --out-link result-iso
```

The ISO is under `result-iso/iso/`. `nix build .#installed-lock` exposes the lock shipped to the installed machine. `nix fmt` formats Nix files supplied to it.

The [migration record](docs/migration.md) explains retained work, exclusions,
compatibility tags and the limits of the migration validation. To exercise the
generated installed flake directly, build the fixture and run the template test:

```sh
cargo build --locked --manifest-path installer/genkan/tests/vm-fixture/Cargo.toml --target-dir .work/fixture-target
python3 tests/check_installed_template.py
python3 -m unittest discover -s tests -p test_release.py
```

For focused Rust work, run Cargo inside the development shell:

```sh
cargo test --locked --manifest-path installer/genkan/rust/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/tool/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/displays/Cargo.toml
cargo test --locked --manifest-path apps/yukimi/Cargo.toml
cargo test --locked --manifest-path rust/Cargo.toml
```

Do not merge unrelated Cargo workspaces or broaden package source filters as part of routine edits. Preserve Genkan and Yukimi's unprivileged GUI/privileged helper boundaries. Keep slow I/O off the interface thread.

The retained VM tooling is available through `nix build .#tools`. Run `result/bin/respin-tools qemu-test --help` for its firmware, storage, desktop, and application options. The Genkan fixture now builds from `installer/genkan/tests/vm-fixture`, not a remote installer revision. The rootless builder's static guest tools require the Rust musl target. The older reports in `TESTING.md`, `installer/genkan/RUST-TESTING.md`, and `docs/test-results/` describe their named predecessor revisions only.

Before publishing an installable image, exercise the exact image through a disposable UEFI and BIOS install, disk-only reboot, desktop login, and Yukimi configuration/update checks. Record the ISO checksum, source revision, results and outstanding hardware limitations. Passing component tests alone is insufficient evidence of an end-to-end installation.

## Sources and installed-system updates

The root lock retains distinct base-system and application Nixpkgs inputs, plus Determinate Nix, FlakeHub CLI, AI applications, and upstream oh-my-pi. First-party code comes directly from this checkout.

`nix/installed-lock.nix` extends that external lock with Sekka itself. A clean Git build pins its commit and source hash; a dirty development build pins the exact source copied into the Nix store. The source is included on the ISO. The installed flake follows `github:bitemyapp/sekka/main` for subsequent updates, with all external inputs explicitly following the installed system's top-level inputs.

Update the distro through Yukimi or `nix flake update sekka`, then rebuild. External inputs retain their independent update controls. Do not copy the root lock verbatim into an installed machine: it lacks the Sekka input until the installed lock is generated.

Standalone users can import `sekka.nixosModules.tatami` with `programs.tatami.enable = true`, or `sekka.nixosModules.yukimi` with `programs.yukimi.enable = true`, without enabling the whole distro. Set `inputs.sekka.inputs.nixpkgs.follows = "nixpkgs"` in the consuming flake.

## Release workflow: deliberately disabled

`.github/workflows/release.yml` has only a manual trigger and an unconditional `if: ${{ false }}` on its sole job. No dispatch, push, tag, or schedule can currently build or publish an image through it. **Do not enable it as part of ordinary maintenance.**

When release readiness is established, a maintainer must review a change removing that inhibition. Provision a clean dedicated Linux x86_64 runner labeled `sekka-builder`, with enough storage, Python 3, Git, and GitHub CLI. The workflow installs Nix. Run the workflow from `main`, specifying an existing `v`-prefixed tag whose commit is reachable from `main`.

It checks and builds the tagged source without updating the lock, creates checksums and a source manifest, uploads assets into a draft release, then publishes it as a prerelease. The workflow uses its repository-scoped `GITHUB_TOKEN`; no personal token is required. It does not trigger on pull requests or create release tags automatically.

GitHub limits individual release assets to 2 GiB. `scripts/prepare-release.py` keeps a smaller ISO whole or splits a larger one into 1900 MiB parts. Release notes include Linux/macOS and Windows reassembly instructions. `SHA256SUMS` verifies downloadable parts; `ISO-SHA256SUMS` verifies the reconstructed image. Failed uploads leave a draft to inspect, not a partially published release. The script refuses to overwrite an existing output directory.

## Screenshots and attribution

Keep original full-resolution PNGs and record how they were captured in `docs/screenshots.md`. Use real app captures, not generated mockups. Mark previews or historical images accurately. Preserve the original component licenses, SPDX declarations, artwork credits, and `docs/imports.json` provenance.

`nix build .#screenshots --out-link result-screenshots` captures Genkan's debug
preview and Yukimi in a disposable NixOS VM. This is a development output and
is not included in the ISO or the normal component checks.
