# Installation failed after erasing the target disk

Investigated 2026-10-24; the report's own date wasn't recorded. Component:
Genkan's installer and the ISO's live store. Fix:
[#102](https://github.com/bitemyapp/sekka/pull/102). Evidence:
[installer-integrity-2026-10-24.json](../test-results/installer-integrity-2026-10-24.json)
and
[installer-memory-2026-10-24.json](../test-results/installer-memory-2026-10-24.json).

## Report

A user installing on an Intel-based Mac (model, RAM and USB writer not
supplied) saw the installation fail after the target had been partitioned and
formatted: copying Firefox failed with `hash mismatch importing path`. A
screenshot showed the expected and actual hashes. The reporter's exact ISO
filename and checksum weren't confirmed; the investigation used the October 22
nightly on the download page, whose Firefox store path matches the report.

## Investigation

- A fresh download of that image matched its published size and SHA-256. All
  3641 packed store paths verified after extraction, and all 3644 registered
  live-store paths verified in a booted UEFI VM. Firefox's expected hash
  agreed across the screenshot, cache.nixos.org's metadata, the extracted ISO
  and the booted ISO.
- Changing one version string in Firefox's `application.ini` (keeping its
  size) inside the booted image's live store reproduced the report: the
  shipped helper reported Prepared and its preflight passed, then the install
  failed after partitioning and formatting with the same path, expected hash,
  stage and message. This is deliberate fault injection, not proof of what
  happened on the reporter's machine.

## Cause

Genkan didn't verify existing store contents before erasing the target. Nix's
validity records, successful builds and page-cache warming don't hash bytes
already in the store, so a readable but corrupt package passed preparation,
and `nix copy` found its hash mismatch only after the target had been erased.

What damaged the reporter's bytes is unresolved: no original media, live-store
dump or kernel logs were available. USB or media read faults, RAM faults and
live-store changes remain possibilities, not diagnoses.

## Fix

- Genkan verifies every registered live-store path before evaluating the
  installation, including the inputs the deferred-build route uses, and
  verifies the prepared system's runtime closure again after building.
- Every nonzero verification result stops preparation. Progress stays
  cancellable, and failures keep Nix's expected and actual hashes and kernel
  warnings in a root-only `/run/sekka-integrity-*.log`.
- The public `iso` derivation unpacks and verifies its actual squashfs
  against the build's closure registration before assembling the ISO.
- Copy-time integrity checks, signature policy and privilege boundaries are
  unchanged.

## Checks

- `genkan-store-integrity`: the packaged helper and Determinate Nix in a
  disposable VM. Its negative control shows an existing-path build and an
  ordinary read accepting corrupt bytes, then a real Nix copy reporting the
  mismatch. Corrupt and missing paths are refused, the whole 40 GiB target is
  unchanged, and diagnostics stay private and secret-free.
- `iso-store-integrity`: a small ISO built through the same gate, rejecting
  changed contents, executable bits, symlinks, missing directories or paths
  and altered registration.
- A diagnostic ISO (the published revision plus only the integrity changes)
  refused the identical corruption before writing to the target, whose
  SHA-256 was the same afterwards. Uncorrupted, it completed an Aizome and
  Firefox installation, booted from disk, accepted a keyboard login and opened
  Firefox with its expected hash.

### Memory pressure

With the same diagnostic image, a 4 GiB VM completed the normal GUI
installation while a background process held 1.5 GiB of random data resident
and could not swap. The installer chose the deferred route, built the 8.31 GiB
system on disk, then booted, accepted a normal login and opened Firefox with
its expected hash. Available RAM fell to 56 MiB and zram swap use reached
3.23 GiB without OOM kills. So the complete system need not fit in physical
RAM; this doesn't establish a minimum memory requirement.

The 2 GiB clean-install case failed during Nix evaluation on both the
unmodified public ISO and the diagnostic image: live-store space ran out and
the kernel OOM-killed desktop processes before any target writes. The
evaluations were then interrupted; these are failed tests, not graceful
refusals. The separate corrupt-file case at 2 GiB did refuse installation
before erasure and preserved the whole target.

Physical hardware, full release qualification and publication were not part
of this work.
