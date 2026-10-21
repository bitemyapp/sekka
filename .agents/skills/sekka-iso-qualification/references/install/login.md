# Q3 — installed disk, Aizome greeter, session lifecycle

- Boot the installed disk with no ISO, no live root and no installation share.
  Prove the root/store mounts and boot source. Repeat a cold boot and restart.
- In the primary and default-only cases, confirm `display-manager.service`
  runs greetd/Aizome's greeter, the running
  greeter executable is from the installed system, and no GDM, SDDM or Plasma
  greeter substitutes for it. Inspect the screen, process and unit together.
- In the separate all-six/GNOME case, verify the documented GDM selection,
  then GNOME's actual lock, wrong-password rejection and unlock. Genkan does
  not expose `loginScreen`; an all-six install cannot select Aizome's greeter
  through the normal GUI. Keep this coexistence coverage separate from the
  mandatory Aizome-greeter cases; never force a different greeter in the guest.
- Greeter lists exactly the selected sessions once each. Exercise user choice,
  password visibility, session choice, keyboard layout, focus and keyboard-only
  access, and the power menu. Aizome is the chosen default; last-session
  persistence follows the implemented contract.
- Wrong password shows a recoverable error without launching a session.
  Correct password starts Aizome through actual typing, no autologin or shell
  start. Confirm user, environment, compositor and shell, then open an app.
- Lock through the desktop, reject wrong password, unlock with the correct
  password; no exposed unlocked desktop while locked. Test idle lock and wake.
- Log out via the UI, then log into Tatami via the greeter. Rotate through
  every installed desktop and back to Aizome. Repeat several cycles, including
  an X11 desktop, and reboot between representative cycles.
- Inspect user services/processes and environment between sessions: no stale
  compositor, wrong portal, competing notification/polkit agents or settings
  leaking across desktops. Verify login keyring unlocks without a second prompt
  using a disposable secret; never use personal browser/session data.
- Keyboard and mouse remain functional after logout, lock, VT switch and
  suspend/resume where the VM supports it. Test shutdown/reboot via UI, not
  only `poweroff`. Check fresh journals and coredumps after each transition.

Component checks `aizome-login` and `aizome-greeter-desktops` supplement these
cases. Their synthetic system configuration cannot prove the installer emitted
or booted the same configuration on the candidate ISO.
