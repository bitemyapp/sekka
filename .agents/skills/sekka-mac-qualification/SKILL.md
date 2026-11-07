---
name: sekka-mac-qualification
description: Qualify a Sekka release for Apple Silicon Macs (the macOS app, helper, engine, Recovery app and Linux payload) as one frozen candidate, through component, VM, native macOS and owner-operated J514S evidence; gate any support claim and promotion on that evidence.
---

# Sekka Mac qualification

A Mac release is a candidate set, not a single file: the signed Sekka
Installer app (with its helper and engine), the Recovery app, the catalog,
the installer data, the manifest and the Linux payload. Test the exact bytes
an owner would receive, together. A Mac ZIP or DMG is not an ISO
qualification result, and this procedure doesn't replace
[ISO qualification](../sekka-iso-qualification/SKILL.md): its PC branches
stay mandatory for any change to Genkan's PC path or a published ISO.

Read `AGENTS.md`, `DEVELOPERS.md` (Apple Silicon) and
`docs/apple-silicon-installer-plan.md` at the candidate revision. Earlier
candidates, component tests, VM boots and other people's reports can't
certify this candidate.

## Coordinator and evidence

Follow the ISO skill's coordinator and evidence contract: a durable run
directory outside tracked source, unique names, disposable disks only, an
independent final audit. For a Mac candidate also record:

- the candidate's artifacts by name, size and SHA-256 (`build.json` from
  `packaging/build-app.sh`, the release manifest, the signed catalog);
- the source revision and installed-lock digest the catalog binds;
- the native toolchain: macOS version and build, Xcode and SDK, rustc;
- the Mac: model, board (`sysctl hw.model hw.targettype`), system firmware,
  macOS version, FileVault state, disk layout before (`diskutil list`,
  `diskutil apfs list`) and the backup taken.

Start the ledger from the tool, never by hand:

```sh
cargo run -q -p sekka-mac-tool --manifest-path installer/genkan/macos/Cargo.toml -- ledger init candidate.json > ledger.json
cargo run -q -p sekka-mac-tool --manifest-path installer/genkan/macos/Cargo.toml -- ledger check ledger.json
```

It holds one row per required case of gates M0 to M9. `pass` needs what was
observed and where the evidence is; `fail`, `blocked` and `not-applicable`
need a concrete reason; a failure needs an issue or pull request. Untested is
never a pass. `ledger check` exits 0 only when every case passed or doesn't
apply.

## Required tree

| Gate | Read | Required result |
| --- | --- | --- |
| M0 contracts and trust | [Components](references/components.md) | Refusals of every bad input, by the component that must refuse it |
| M1 image | [Image](references/image.md) | The exact payload boots twice in a VM, with its own identity, and updates |
| M2 Linux experience | [Linux](references/linux.md) | First-boot setup, login, sessions and every advertised app, through the real greeter |
| M3 native macOS | [Native](references/native.md) | The app, helper and Recovery app built, signed and exercised on a Mac |
| M4 Asahi and Recovery | [Hardware](references/hardware.md) | A real installation and Recovery step on the exact J514S, owner-operated |
| M5 hardware | [Hardware](references/hardware.md) | Every device of the J514S, success or honest limitation |
| M6 updates and recovery | [Linux](references/linux.md) | Yukimi updates, rollback, macOS and back |
| M7 encryption | [Encryption decision](../../../docs/apple-silicon/encryption.md) | Not offered: `not-applicable`, citing the decision |
| M8 PC regression | [ISO skill](../sekka-iso-qualification/SKILL.md) | Q0 to Q8 for any changed Genkan PC path or published ISO |
| M9 promotion | [Promotion](references/promotion.md) | Exact bytes bound, audited, signed and notarized; dry run only |

## Hardware and the owner

Only the Mac's owner runs a real installation, with a verified backup and a
written restore route (Time Machine or a full clone, and the Mac's DFU
revival route through another Mac). Prefer a spare J514S kept for testing.
Never run installer fixtures, the engine, `install-sekka.sh` or disk tools
against this workstation's disks; the VM harness uses disposable disk files
only. Each owner-operated step is its own explicit authorization.

## Failure and repetition

A code or payload change starts a new candidate and resets every gate it can
affect; evidence isn't inherited from a patched installation. Keep failed
candidates' hashes, logs and screenshots with their issue links.
