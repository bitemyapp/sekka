# Disk encryption settings: changes considered and declined

On 2026-11-07 we considered two changes to how Genkan sets up LUKS. The aim was
to cut the CPU that disk encryption costs. We declined both. This page records
what was measured, what was proposed, and why we passed.

## What Genkan does

`installer/genkan/rust/src/encryption.rs` formats root and swap with:

```sh
cryptsetup luksFormat --type luks2 --pbkdf argon2id --offset 32768
```

It passes no `--sector-size`, so cryptsetup chooses 4 KiB encryption sectors
only when the drive reports 4 KiB sectors. Most consumer NVMe drives report
512 bytes, so installs get 512-byte sectors. The generated NixOS configuration
opens both volumes with `allowDiscards = true` and dm-crypt's default
workqueues. `/boot` lives outside LUKS.

## What was measured

All measurements are on wx-workstation, a Genkan install:
- CPU: Threadripper PRO 9985WX (Zen 5, with VAES);
- drive: KIOXIA XG10d, 4 TB NVMe;
- encryption: LUKS2 `aes-xts-plain64` with 512-byte sectors and default
  workqueues.

CPU is system-wide, counted on an otherwise idle host, and these are single
runs.

Encryption, not the filesystem, takes most of the read CPU:

| 1 MiB sequential reads, depth 32 | Throughput | Busy cores | CPU per GiB |
| --- | --- | --- | --- |
| Raw NVMe partition (ciphertext, no decryption) | 6.6 GiB/s | 1.0 | 0.16 s |
| dm-crypt device | 6.5 GiB/s | 12.1 | 1.94 s |
| Btrfs on it | 6.4 GiB/s | 12.5 | 2.05 s |

ext4 cost the same CPU as Btrfs within 0.97–1.09× for sequential reads and
writes, compressed data included. Btrfs's extra cost is per I/O: about 3 µs per
4 KiB random read in the reading thread, against about 1.2 µs for ext4.

The effect of the two settings, measured on dm-crypt over a RAM-backed loop
device so that no disk was involved:

| dm-crypt setup | Sequential read | Sequential write | Write throughput | Random 4 KiB read |
| --- | --- | --- | --- | --- |
| 512-byte sectors, workqueues (today) | 0.53 s/GiB | 0.72 s/GiB | 9.5 GB/s | 5.6 µs |
| 4 KiB sectors, workqueues | 0.46 | 0.62 | 10.3 GB/s | 6.1 |
| 512-byte sectors, no workqueues | 0.52 | 0.62 | 3.9 GB/s | 5.1 |
| 4 KiB sectors, no workqueues | 0.43 | 0.41 | 7.3 GB/s | 3.7 |

## What was proposed

1. Format new volumes with `--sector-size 4096`: one AES-XTS operation per
   4 KiB page instead of eight.
2. Open volumes with `boot.initrd.luks.devices.<name>.bypassWorkqueues`, which
   becomes `no-read-workqueue,no-write-workqueue` with the systemd initrd. Or
   bypass only the read workqueue, through `crypttabExtraOpts`.

## Why we passed

**4 KiB sectors: crash safety.** With 4 KiB encryption sectors, a write the
drive tears inside a sector, for example on power loss, makes that whole 4 KiB
decrypt to garbage. With 512-byte sectors, the damage stays within the
512-byte sectors actually torn. Whether a drive writes 4 KiB atomically varies
by model, and Genkan cannot confirm it for each install. Btrfs and ext4 write
whole 4 KiB blocks, which narrows the difference but does not remove it. Any
added risk of losing data on power failure is out of bounds for Genkan's
defaults, and a 13–14% CPU saving does not justify it.

**Workqueue bypass: not worth it without 4 KiB sectors.** With 512-byte sectors
it barely helped reads (0.52 against 0.53 s/GiB). It made single-stream writes
59% slower, because encryption then runs on the writing thread instead of
spreading across cores. A large copy or game install on a laptop or low-power
CPU would be capped at one core's AES speed. Its effect on real-time audio
jitter was not measured.

## What would reopen this

- **4 KiB sectors:** evidence that the drives Sekka supports write 4 KiB
  atomically (for example, NVMe atomic write units of at least 4 KiB). That
  would also need passing crash-consistency tests in disposable VMs and
  physical power-cut tests. Opting in per install, as an advanced choice, is a
  separate question.
- **Workqueue bypass:** results on low-power hardware showing that
  single-stream writes stay at drive speed with no added audio jitter.
  Read-only bypass is the variant to test first.

The raw fio output and scripts are in
`~/sekka-work/fsbench-results-20261107` on wx-workstation. They are not
committed.
