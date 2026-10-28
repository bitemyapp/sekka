# Q1 — actual live media

For every ISO being considered for retention or publication:

- Q1.1 Verify downloaded size and SHA-256 before boot. Save the manifest and
  public URL. Reject truncated or mismatched media without presenting it as a
  filesystem regression.
- Q1.2 Boot UEFI USB mass storage and BIOS USB mass storage. Separately boot
  UEFI optical media. Select the actual default menu entry using keys. Check
  the kernel version and the complete boot path, not merely the boot menu.
  Sekka's boot splash shows from the initrd, Esc shows the boot messages, and
  the splash hands over to the automatically logged-in Aizome session.
- Q1.3 Exercise the advertised LTS and compatibility boot entries (the
  compatibility entry draws Aizome with pixman). Capture each selected entry,
  live desktop, renderer, kernel, graphics driver and boot errors.
- Q1.4 Confirm `/iso`, `/nix/.ro-store`, writable overlay and `/nix/store` are
  mounted as intended. In the initrd, inspect `/sysroot/nix/.ro-store`, its
  mount unit, source loop device and backing `/iso/nix-store.squashfs`.
  An emergency prompt, dependency failure or fallback shell is a failed boot.
- Q1.5 Inspect serial and `journalctl -b`; check `systemctl --failed`, storage
  errors and missing firmware. Distinguish ignorable virtual hardware messages
  from failures affecting boot, input, graphics, network or installation.
- Q1.6 Confirm live GUI and Genkan launch, USB and PS/2 input where supported,
  display resize, terminal, networking, DNS and access to configured caches.
  Open/close/relaunch installer normally. Test keyboard-only navigation.
- Q1.7 Leave the live desktop idle through its configured idle thresholds;
  confirm it remains usable and does not lock/suspend during installation.
  Test network loss and reconnection; document offline installation support
  using a separate disconnected VM, not a temporarily warm host cache.

On a mount failure preserve boot screenshot, serial log, generated mount
units, `/proc/cmdline`, device labels/UUIDs, loop status and kernel logs. Do not
change the ISO or boot parameters and count that as the original case passing.
Use the modified boot only to isolate the defect; see the failure loop.
