# M4 and M5: the J514S, owner-operated

Before anything: a verified backup and written restore route, the Mac's
facts recorded, the ledger rows assigned. The owner runs every step on the
Mac; agents observe, record and reconcile.

Several M4 rows need more than one installation (FileVault on and off,
free space as well as a resize, the second run of M4-07). This release
can't remove Sekka, so between runs the owner returns the Mac to a starting
layout by their own written route. Record each run's starting layout,
FileVault state and which rows it covers.

M4, in order, on the release build:

1. M4-08: before the first disk change, record the controls: the backup and
   its restore route, macOS starting normally and its Recovery reached from
   startup options (hold the power button), and disposable credentials for
   every account the run will create. Nothing below starts without them.
2. M4-01: Sekka Installer inspects the Mac as supported. On another Mac (or
   with `--simulate unsupported` for the presentation), unsupported is
   explained and nothing is asked.
3. M4-12: on battery, the app says the Mac isn't ready and offers nothing;
   with power connected, Check Again goes on. Turning FileVault on between
   runs gives the converting case: record it then.
4. M4-11: with too little space (the owner fills macOS's volume with a
   disposable file and deletes it afterwards), the app says why, offers no
   plan and asks for no password; `diskutil list` is unchanged.
5. M4-02: the plan screen shows the APFS limits; record them against
   `diskutil apfs resizeContainer disk0s2 limits`. If the resize fails,
   Apple's message is shown verbatim and the disk is reported unchanged:
   confirm with `diskutil list`.
6. M4-13: at Install, enter a wrong administrator password first. The helper
   refuses it, the app asks again in place and `diskutil list` is
   unchanged; then the right one starts the installation.
7. M4-03, M4-04: the installation completes; record the helper's journal
   (`/var/db/computer.sekka.installer/installations/`), the new partitions,
   the ESP's `sekka/install.json` and the Recovery configuration.
8. M4-15: while the helper installs, quit the app and open it again: it
   shows the same installation's progress. After macOS's part, restart the
   Mac before the Recovery step: the app opens on that installation, waiting
   for Recovery, never a new one. Don't restart in the middle of a disk
   stage; that is a separately reviewed interruption test.
9. M4-05, M4-06: shut down, hold the power button, choose Sekka. Finish
   Installing Sekka opens by itself. Enter a wrong password once (asked
   again in place), then the right one; record `bputil -d` before and after
   and the startup disk.
10. M4-14: after the installation, Sekka Installer recognizes Sekka and
    offers no new installation. If a run leaves a partial installation, it
    is recognized and stops with an explanation; record that too.
11. M4-07: on a second candidate run, restart during the Recovery step and
    show the app resumes without repeating a completed step.
12. M4-09: across the runs, one installation with FileVault on and one with
    it off. Record `fdesetup status` before and after each; after the run
    with FileVault on, macOS is still encrypted.
13. M4-10: one run installs into existing free space: the plan offers no
    resize, `diskutil apfs list` shows macOS's container unchanged, and the
    new partitions lie in the space that was free.
14. M4-16, M4-17: failures after the disk changed. A disk stage that fails
    stops with the stage named and what changed, and offers only what the
    journal allows. After the startup-disk stage fails, Set Startup Disk
    Again checks the target, runs only that stage, and the stub then starts
    into Recovery and opens Finish Installing Sekka. Induce neither on the
    owner's disk; if one happens on its own in any run, that run is its
    evidence. Otherwise record the row as blocked (or not-applicable, with
    the scoping decision) and the reason. `--simulate install-fails` and
    `--simulate startup-disk-fails` show only the screens.

M5: boot the installed system and go through every device in the rows,
recording success, error or limitation. Compare with the hardware profile's
evidence; update the profile only from these observations.
