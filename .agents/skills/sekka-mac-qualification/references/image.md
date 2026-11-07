# M1: the exact image in a VM

Build the candidate's payload from the candidate revision, then check its
digest against the catalog before booting it.

```sh
nix build .#packages.aarch64-linux.mac-release-j514s --no-update-lock-file --out-link result-mac-release
```

Use the VM harness the image documentation describes
(`docs/apple-silicon/image.md`): on an aarch64 Linux host with KVM, or on a Mac
with Apple's hypervisor. It lays the disk out as the engine would (random GPT
partition UUIDs, the handoff written, the boot entry filled), boots twice and
checks the store, growth, the firmware handoff and the mounts by partition
UUID. Run its two-identity case (two byte-identical images on two disks) for
M1-06, and its rebuild, update and rollback cases for M1-08.

The VM doesn't run m1n1, U-Boot, Apple's firmware or the Mac's drivers;
record those rows' limits and leave the hardware to M5.
