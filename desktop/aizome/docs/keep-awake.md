# Keep Awake

Keep Awake stops the computer from going to sleep while it's on: for a long
build, a download, an agent working, or a laptop closed on the desk with a
monitor attached. It's Tatami's Keep Awake, in Aizome.

## Using it

- **The menu bar:** the Battery panel has a Keep Awake switch. While it's
  on, a cup shows at the front of the systray, kohaku amber. Click the cup
  for its own panel, which turns it off.
- **The palette:** ❖Space, then "Keep Awake" (or "Turn Off Keep Awake").
- **A terminal:** `aizome-awake on`, `off`, `toggle`, and `status`.
  `status` prints `{"enabled": …, "protected": …}`.

It stays on until it's turned off, across logging out and restarting. At
the next login it starts again.

If the cup turns **red**, Keep Awake is on but logind doesn't list its
locks, so the computer may sleep. That happens while the service restarts
(for a second) or when it can't take them. `systemctl --user status
aizome-keep-awake` says why; turning it off and on again retries.

## What it blocks

While it's on, logind holds a **block** inhibitor, *Aizome Keep Awake*, on
`sleep`, `idle` and `handle-lid-switch` (`systemd-inhibit --list` shows
it):

- **Closing the lid** doesn't suspend the computer.
- **logind's idle action** (`IdleAction=` in logind.conf) doesn't run.
- **Sleep asked for by anything else** is refused: other users' sessions,
  and the login screen's own suspend when it's left alone.

Aizome itself also doesn't **lock the screen or turn the displays off**
when the computer is left alone. Keep Awake holds the compositor's idle
inhibitor, as a video player does.

Some things still put the computer to sleep, on purpose:

- **Sleep chosen from the menu** (or `systemctl suspend`) by the person
  logged in. logind lets a user past their own inhibitors, so turning Keep
  Awake on never traps anyone.
- **An administrator suspending past it** (`systemctl suspend -i`).

Nothing here prevents a flat battery, or a shutdown by the firmware when
the machine overheats.

## How it works

`aizome-keep-awake.service`, a user service in the session, runs
`aizome-awake hold`. It:

1. takes logind's inhibitor (the descriptor logind's `Inhibit` returns);
2. checks that logind lists it, held by its own process;
3. connects to the compositor and creates an idle inhibitor on a surface
   it never shows. Aizome holds off idleness while any surface has one,
   and lets go when the surface goes, with its client if it dies, so a
   killed holder doesn't keep the screen awake after Keep Awake is off;
   `crates/aizome-comp/tests/idle_inhibit.rs` checks both;
4. tells systemd it's ready (`Type=notify`). `aizome-awake on` then
   confirms with logind before it says Keep Awake is on.

After that it checks every second that logind still lists the lock. A
restarted logind forgets its inhibitors, so the service ends with an error
and systemd starts it again a second later, taking the locks afresh
(`Restart=always`, no start limit). It does the same if the compositor
goes away.

The setting is the file `~/.local/state/aizome/keep-awake`. The unit's
`ExecCondition` (`aizome-awake requested`) starts it at login only when
the file is there. The unit is `PartOf=aizome-session.target`, so logging
out lets go of the locks. A system update doesn't restart it
(`restartIfChanged = false`), so the locks are never dropped in between.

The shell only asks `aizome-awake` to turn it on or off, and watches
whether logind lists the locks: every two seconds while it's on, and only
a check that a file exists while it's off. Restarting or reloading the
shell doesn't touch the locks. Only the real session's shell changes Keep
Awake. A headless or nested Aizome, or a test, leaves it alone.

## Checks

- `cargo test -p aizome-awake`: an inhibitor counts as Keep Awake only as
  a block on all three, held by the right user (and process); "on" and
  "protected" are never confused.
- `cargo test -p aizome-comp --test idle_inhibit`: an inhibitor on a
  surface never shown holds off idleness, and idleness comes once it's
  gone, or once its client goes without destroying it.
- `cargo test -p aizome-shell --test panels`: the cup, its panel, the
  Battery panel's switch and the palette's command.
- `nix build .#checks.x86_64-linux.settings` (*Keep Awake* part), against
  real logind: on puts the block in `BlockInhibited` and the cup in the
  bar; a killed holder takes the locks again; off lets them go and takes
  the cup away; and with it off, the unit doesn't start.
