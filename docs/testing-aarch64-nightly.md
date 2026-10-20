# ARM64 nightly integration validation — 2026-10-20

Implementation checked at `6ecb8e91375995f7f3b54d5f77f96ab536c1cde4` on
`codex/aarch64-utm`, including `main` through `f4c832e`. Later documentation
changes do not alter the tested implementation.

## Checks run

| Check | Result |
| --- | --- |
| `nix flake check --no-build --no-update-lock-file` on wx-workstation | Passed for x86; Nix explicitly omitted incompatible ARM checks |
| Native ARM application catalog, Genkan, Yukimi, Tatami and UTM profile checks | Built successfully on the local aarch64 Linux builder |
| `checks.x86_64-linux.nightly-assets` on wx-workstation | Passed five tests running the real publisher against a filesystem S3 fixture |
| `checks.x86_64-linux.nightly-workflow` on wx-workstation | Actionlint and ShellCheck passed |
| `checks.x86_64-linux.arm-emulation` on wx-workstation | Passed in a disposable x86 VM: static binfmt with `F`, native ARM shell/coreutils executed through an untrusted user's sandboxed Nix build |
| `packages.x86_64-linux.site` on wx-workstation | Built the website, production Wasm worker and native rendering tests successfully |
| Local Rust site/renderer tests | Five generator tests and seven renderer tests passed |
| Release asset regression tests | Two tests passed; versioned release job remains inhibited |
| Download page layout | Reviewed the built site with both architecture rows, using the actual Rust renderer and explicitly marked fixture data |

The publisher tests check both ISOs and their separate checksum files, shared
source metadata, legacy x86 fields, first publication, repeated revisions and
seven-revision retention, including nested ARM objects. Failed ARM/metadata/index
uploads preserve the prior index and old builds. Missing, empty, swapped,
duplicate and dirty-checkout inputs are rejected; access errors and malformed
indexes are not treated as an empty history. No real bucket was written.

The catalog check now discards string context on its metadata-only package
paths. Complete system derivations are still forced for validation, while
writing the JSON no longer builds every optional application. This matters for
the focused ARM image, which omits the optional-application cache. The actual
ARM package/cache builds remain separate nightly steps.

Wrangler 4.148's local Pages preview failed on a middleware-module import.
The visual check therefore used the production Rust HTML renderer with the
publisher's fixture JSON. The production Wasm bundle built successfully, but
no Cloudflare deployment or live Workers request was tested in this run.

## Required workstation setup and coverage limits

wx-workstation is Ubuntu 24.04 x86_64. Read-only inspection found no ARM binfmt
registration or ARM entry in its Nix `extra-platforms`. Before merging/running
the dual-architecture nightly, a maintainer must run the updated
`sudo bash scripts/runner-host-setup.sh` there while the runner is idle. This
installs static QEMU, enables ARM binfmt, adds the platform and signed bootstrap
cache, and restarts the Nix daemon. It was not applied to the workstation during
this PR's validation. The workflow's real ARM sandbox preflight fails early if
setup is missing. Its job container and admission restrictions are unchanged.

The VM test establishes native ARM execution in a sandbox on an x86 CPU. It is
not a complete ARM ISO build on the production workstation. No nightly workflow
was dispatched and no main/nightly branch was moved by hand. The prior native
ARM image's actual UTM installation, disk boots, Plasma/Tatami sessions and
graphics checks are recorded separately in
[the 2026-10-19 report](testing-aarch64-utm.md), with its own exact source and
ISO hash; they are not evidence that every future nightly has been tried in UTM.

ARM64 remains experimental and has only been tested in UTM on Apple silicon.
Its cache coverage check targets the media's prebuilt Plasma/Tatami systems and
free catalog applications. Other desktop selections may compile locally.
The website changes are source changes for the PR; deployment still follows
the repository's reviewed-main procedure.
