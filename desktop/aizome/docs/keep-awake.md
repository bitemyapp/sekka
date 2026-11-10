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

The cup has a second meaning, in another colour. With Keep Awake off, a
cup in the menu bar's own colour (as the widgets beside it) says an app
is keeping the screen on: a video playing, say. That holds less than Keep
Awake does. See "Apps that keep the screen on" below.

## What it blocks

While it's on, logind holds a **block** inhibitor, *Aizome Keep Awake*, on
`sleep`, `idle` and `handle-lid-switch` (`systemd-inhibit --list` shows
it):

- **Closing the lid** doesn't suspend the computer. The laptop's own
  screen still goes dark behind it (Aizome turns the panel off while the
  lid is closed, and on again when it opens), and monitors attached stay
  on. With an external display in the layout, the closed panel leaves
  the layout and its focused Desk moves to that display. Opening the lid
  restores the panel and its Desk.
- **logind's idle action** (`IdleAction=` in logind.conf) doesn't run.
- **Sleep asked for by anything else** is refused: other users' sessions,
  and the login screen's own suspend when it's left alone.

Aizome itself also doesn't **lock the screen or turn the displays off**
when the computer is left alone. Keep Awake holds idleness off at the
compositor, as a video player does, and unlike a player's its hold still
counts once the screen is locked: lock the screen yourself with Keep Awake
on, and the displays stay lit behind the lock, as they always have.

Some things still put the computer to sleep, on purpose:

- **Sleep chosen from the menu** (or `systemctl suspend`) by the person
  logged in. logind lets a user past their own inhibitors, so turning Keep
  Awake on never traps anyone.
- **An administrator suspending past it** (`systemctl suspend -i`).

Nothing here prevents a flat battery, or a shutdown by the firmware when
the machine overheats.

## Apps that keep the screen on

An app can ask for the screen to stay on while it has something to show:
Utsushi, mpv or VLC playing a video, a browser playing one, a call. While
one does, the screen doesn't lock or turn off by itself. Nothing was
switched on, so it's easy not to know why the screen stays lit. The cup
says so:

| | The cup | Left alone | Sleep and the lid | Locked by hand |
|---|---|---|---|---|
| **Keep Awake**, your own switch | amber (red: not working) | no lock, displays stay on | blocked | displays stay on |
| **An app** holding the screen on | the bar's own colour | no lock, displays stay on | not blocked: Sleep from the menu, the power key and closing the lid do what they usually do | displays turn off on their timer |
| Both | amber | as Keep Awake | as Keep Awake | as Keep Awake |

Click the cup and its panel lists, under Keep Awake's switch, each app
keeping the screen on: its name, and why in the app's own words ("Playing
video", "Video Wake Lock") or, where it gave none, which of its windows
holds. An app that asks in two ways at once, as Chrome does, is one row.

- **A click on a row** brings that app's window forward.
- **The button at its right** opens Myaku at the app's process. Quitting
  it, with Myaku's confirmation, is how to end a hold the app won't let
  go of.
- **There's no switch to turn an app's hold off.** The app would take it
  again the next time it asks, the shell can't let go of a hold an app
  took from the compositor, and apps aren't told when one is ignored.
  Pause the video, or quit the app.

The cup comes once apps have held for three seconds, and stays two
seconds after the last lets go, so a sound that plays for a moment shows
nothing and a player that lets go and holds again between two files
doesn't blink. The panel's rows don't wait: open (for Keep Awake), it
lists a hold as soon as there is one.

**Once the screen is locked, an app's hold stops counting.** Nobody is
watching what it plays, so the displays turn off after the usual minutes
(`idle.monitors_off_after_minutes`), counted from the lock or the last
key or pointer movement since, while the video plays on. Unlock, and it
holds again. Keep Awake is your own word and still counts, locked or not.

## How it works

`aizome-keep-awake.service`, a user service in the session, runs
`aizome-awake hold`. It:

1. takes logind's inhibitor (the descriptor logind's `Inhibit` returns);
2. checks that logind lists it, held by its own process;
3. asks the compositor, over its IPC, to hold idleness off (`keep-awake`,
   `docs/ipc.md`), on a connection it keeps open. The hold is the
   connection: it goes when the service stops, and with the service if
   it's killed, so a dead holder doesn't keep the screen awake after Keep
   Awake is off. Asking this way is also how the compositor tells the
   user's own switch from an app's hold, with nothing guessed from a
   process's name: only programs of the session reach that socket, and an
   app's Wayland idle inhibitor is never taken for Keep Awake.
   `crates/aizome-comp/tests/idle_holders.rs` and `idle_inhibit.rs` check
   it. Where the request can't be asked, the compositor gets a Wayland
   idle inhibitor on a surface never shown instead, as before, and
   idleness is held off there all the same, as an app's hold is: an
   Aizome from before the request, still running after an update, which
   answers that it doesn't know it, and another desktop, which has no
   such socket. Only there. An Aizome that takes the request and doesn't
   answer within five seconds, drops the connection or refuses is an
   error: the service ends, and systemd starts it again a second later
   to ask afresh. Held as an app's instead, the hold would stop counting
   behind a lock while the cup stayed amber;
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

The apps' side is `docs/shell.md`, "Staying awake for a video": the two
ways an app asks, how the shell names who holds, and the cup's waits.

## Checks

- `cargo test -p aizome-awake`: an inhibitor counts as Keep Awake only as
  a block on all three, held by the right user (and process); "on" and
  "protected" are never confused. The compositor's hold is asked for by
  name and ends with its compositor. A compositor that says it doesn't
  know the request, or a socket nobody listens at, gets the inhibitor
  instead; one that answers late, hangs up or refuses is an error, and
  never an app's hold.
- `cargo test -p aizome-comp --test idle_inhibit`: an inhibitor on a
  surface never shown holds off idleness, and idleness comes once it's
  gone, or once its client goes without destroying it. Keep Awake's hold
  over IPC does the same for as long as its connection lasts.
- `cargo test -p aizome-comp --test idle_holders`: the compositor says who
  holds idleness off, and tells subscribers when that changes. Locked,
  an app's inhibitor lets the displays turn off on their timer and Keep
  Awake's hold doesn't; unlocked, both hold as before.
- `cargo test -p aizome-shell --lib holders` and `--test holders`: both
  ways of asking are one list with one row an app, named by its desktop
  entry; the shell's own inhibitor and Keep Awake's hold are nobody's row;
  the cup's two waits; the panel's rows and wording.
- `cargo test -p aizome-comp --test displays lid`: closing the lid turns
  the laptop's panel off and leaves other monitors on, turning the
  displays on doesn't light the panel behind it, and opening the lid
  lights it again. Docked, the panel leaves the layout and its focused
  Desk moves to the external display. Tests also cover docking while
  closed, restoring live display settings, and keeping a manually
  disabled panel disabled.
- `cargo test -p aizome-shell --test panels`: the cup, its panel, the
  Battery panel's switch and the palette's command.
- `nix build .#aizome-checks.settings` (*Keep Awake* part), against
  real logind: on puts the block in `BlockInhibited` and the cup in the
  bar; a killed holder takes the locks again; off lets them go and takes
  the cup away; and with it off, the unit doesn't start.
- `nix build .#aizome-checks.idle-inhibit`, in a real session: with
  Utsushi, mpv, VLC and Chromium playing in turn, the cup's panel lists
  each by name, and the cup goes when it pauses; locked by hand with mpv
  or VLC playing, the displays turn off on their minute and the video
  plays on; with Keep Awake on beside VLC, its service's hold is the
  compositor's `keep-awake`, the cup is amber over the same list, and
  locked by hand the displays stay on until it's turned off.
