# Q2 — installation

At least one complete installation for each firmware must use only normal
Genkan mouse/keyboard UI. The primary UEFI case installs Aizome and Tatami
plus the other advertised desktop choices, with Aizome default and password
login. A second case keeps default choices. Do not replace the GUI flow with
the fixture. Use fixtures to cover the remaining storage combinations, and
record the distinction in every result.

## GUI journey

- Welcome and locale: forward/back navigation, language/keyboard selection,
  layout typing, timezone detection and manual map/list choice, persistence
  when returning to earlier pages.
- Storage: empty and preformatted disposable disks, correct disk identity,
  filesystem choices, partition preview, available space, clear confirmation
  before erasure. Verify refusing/cancelling never mutates the disk.
- Account: username/hostname validation, mismatched and empty passwords,
  password visibility, Unicode display name, normal-user privileges. No
  autologin in the authentication qualification case.
- GitHub/SSH: successful public-key lookup and invalid/offline lookup; optional
  settings disabled and enabled. Verify installed keys, SSH policy and Git
  identity. Use dedicated test identities and no real credentials.
- Desktops: every advertised option, Aizome default, multiple sessions, exact
  review summary. Back navigation preserves selection. Applications: none,
  representative categories, all supported catalog apps; search and selection
  remain responsive and no hidden selection is installed.
- Review: every chosen value matches; cancellation and back-navigation remain
  safe. Install via the actual button; visible progress/errors; window remains
  responsive. Capture timing without claiming physical-disk performance.
- Completion: clear success, installation log saved, reboot action works.
  Eject/remove media before next boot. A false-success screen is a release bug.

## Storage coverage

Track a matrix with firmware (UEFI, BIOS), filesystem (ext4, Btrfs, XFS), disk
bus (virtio, NVMe 512-byte, NVMe 4K), initial layout (blank and an existing
filesystem), and supported encryption/swap options. Cover each supported
combination that changes partitioning, formatting or bootloader behavior; read
`installer/genkan` storage tests to identify constraints, not to omit cases.
Run encrypted and unencrypted installation and reboot; wrong passphrase must
fail safely, correct passphrase succeeds. Unsupported combinations must be
rejected clearly before writes and recorded as such.

Verify actual partition table, filesystem/UUIDs, mount options, swap size vs
RAM, encryption and discard policy, bootloader/EFI entries, generated flake and
lock, sources, selected packages/desktops, hostname, locale, timezone, user and
password policy. Check logs for hidden network compilation failures. Test
low-space/refused-device and recoverable network errors in disposable guests.
Count backend, GUI and actual disk-boot results separately.
