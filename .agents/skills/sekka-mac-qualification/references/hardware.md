# M4 and M5: the J514S, owner-operated

Before anything: a verified backup and written restore route, the Mac's
facts recorded, the ledger rows assigned. The owner runs every step on the
Mac; agents observe, record and reconcile.

M4, in order, on the release build:

1. M4-01: Sekka Installer inspects the Mac as supported. On another Mac (or
   with `--simulate unsupported` for the presentation), unsupported is
   explained and nothing is asked.
2. M4-02: the plan screen shows the APFS limits; record them against
   `diskutil apfs resizeContainer disk0s2 limits`. If the resize fails,
   Apple's message is shown verbatim and the disk is reported unchanged:
   confirm with `diskutil list`.
3. M4-03, M4-04: the installation completes; record the helper's journal
   (`/var/db/computer.sekka.installer/installations/`), the new partitions,
   the ESP's `sekka/install.json` and the Recovery configuration.
4. M4-05, M4-06: shut down, hold the power button, choose Sekka. Finish
   Installing Sekka opens by itself. Enter a wrong password once (asked
   again in place), then the right one; record `bputil -d` before and after
   and the startup disk.
5. M4-07: on a second candidate run, restart during the Recovery step and
   show the app resumes without repeating a completed step.

M5: boot the installed system and go through every device in the rows,
recording success, error or limitation. Compare with the hardware profile's
evidence; update the profile only from these observations.
