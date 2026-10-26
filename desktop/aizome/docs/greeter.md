# The login screen

`aizome-greeter` is Aizome's login screen. greetd runs it as its greeter.
It shows every session the computer has: Aizome, other desktops and window
managers, and X11 ones. It looks like Aizome's lock screen, because it draws
with the same pieces (`aizome-face`).

## What it shows

- The wallpaper under the kachi scrim, by night or by day (Night from
  18:00 to 06:00), the time in IBM Plex Mono and the date, and the mark at
  the foot, as on the lock screen.
- The computer's name at the top left, and the power button at the top
  right. Its menu has Sleep, Restart and Shut Down; F3 opens it too.
- **The people.** With more than one person, their pictures are in a row.
  The selection has the focus rail under it. ← and → move it, and Return
  or a click picks. Pictures are AccountsService's, set in Settings; without
  one, the initial shows on the selection colour.
- **The password**, once someone is picked. Their picture and name sit
  above the field, which has the focus rail. A wrong password shakes the
  field and says *That password didn't work. Try again.* PAM's other
  questions and messages show too (a code, a key to touch). Caps Lock is
  pointed out.
- **The session**, under the field: the one that will start. Click it, or
  press F2, for the menu of every session. Type to filter, and Return picks.
  Each person's last session is picked for them next time, and the default
  (`defaultSession`) for someone new.
- *Log in as someone else*, or Escape, goes back to the people.
- Without a list (`user_list = false`), or with nobody on it, the user name
  is typed.

Whoever logged in last starts at their password. Other displays show the
wallpaper and the time.

## How it runs

greetd starts `aizome-greeter` as the `greeter` user, in a logind session
of the greeter class. The greeter then:

1. **Starts the compositor.** It runs Aizome's compositor as its child,
   without the shell, Xwayland or the session plumbing
   (`aizome --backend udev --no-shell --no-xwayland --no-session`), on a
   socket of its own, with `PR_SET_PDEATHSIG`. If the greeter dies, the
   kernel stops the compositor. If the compositor dies, the greeter loses its
   display and exits. Either way greetd starts the greeter again. On the way
   out the greeter stops the compositor and waits for it, so the display is
   free before the session starts.
2. **Shows a surface on each display.** Each is a layer-shell surface on
   the overlay layer. The keyboard goes to the display with the login.
3. **Talks to greetd**, on one connection on a thread of its own:
   `create_session` for the person picked, the answers to PAM's questions,
   then `start_session` with the session's command and environment
   (`XDG_SESSION_TYPE`, `XDG_SESSION_DESKTOP`, `XDG_CURRENT_DESKTOP`). Then
   it quits, and greetd starts the session.

The conversation is `login.rs`: pure logic, tested without greetd. Every
request carries its login's generation. An answer that comes for someone no
longer picked is let go, so picking someone else mid-check can never start
the first person's session.

## Security

- **Privileges.** The greeter runs unprivileged as `greeter`. greetd does
  the PAM conversation as root and starts the session.
- **The password** lives only in the field and in the one message to
  greetd. It's zeroed once sent, as is every copy the greeter made. It's
  never logged or shown (the field is secure). PAM's lockout (faillock),
  where configured, applies as it does everywhere.
- **What it runs.** It runs only the commands in session entries from
  root-owned directories. On NixOS these are the store paths in
  `/etc/aizome/greeter.toml`.
- **What it reads:** `/etc/passwd` and `login.defs` (accounts in
  `UID_MIN`–`UID_MAX` with a login shell), AccountsService's pictures, and
  its own state, `/var/lib/aizome-greeter/state.toml` (mode 0700, the
  greeter's). The state holds who logged in last and each person's
  session, nothing secret. Nothing in people's homes is read.
- **What it can do.** Power actions go to logind through `systemctl`, and
  polkit lets the person at the login screen take them.

## Configuration

On NixOS, `programs.aizome.greeter.enable = true` (with
`programs.aizome.enable`) sets up greetd and writes
`/etc/aizome/greeter.toml`.

greetd starts the greeter as `/run/current-system/sw/bin/aizome-greeter`,
not by its store path. Sekka starts Tatami the same way. The greeter's own
configuration names the compositor by store path, and the greeter reads it
each time it starts. So an Aizome update leaves greetd's configuration and
unit unchanged and applies live, and the next login screen is the new one.
By store path, every update would change greetd's configuration, and
updaters stage a display manager change for the next start.

Its keys:

| Key | Default | |
|---|---|---|
| `sessions` | the system's `wayland-sessions` and `xsessions` | session directories, in order |
| `hidden_sessions` | none | session ids not to offer |
| `default_session` | `services.displayManager.defaultSession` | for someone new |
| `x11_prefix` | `startx` with NixOS's session wrapper and logout cleanup | how X11 sessions start; empty: none offered |
| `hidden_users` | `services.displayManager.hiddenUsers` | |
| `user_list` | true | list people, or type the name |
| `wallpaper_night`, `wallpaper_day` | Aizome's wallpapers | |
| `wallpaper` | `auto` | `night`, `day` or `auto` |
| `power` | true | the power menu |
| `compositor`, `compositor_settings` | Aizome's, with the system's keyboard layout | |

On NixOS, the X11 session wrapper imports the display environment and starts
the graphical session target for desktops such as Xfce. Logging out stops
that target and its portals, even if the user's service manager stays alive.
The `greeter-x11` VM check covers Xfce followed by an Aizome login.

`services.displayManager.autoLogin` becomes greetd's initial session,
`aizome-greeter --session ID`. It starts the default session at boot with
the environment the greeter would give it, without showing the greeter.

On Sekka, `calamares.loginScreen` (`auto`) picks Aizome's login screen
whenever Aizome is chosen, listing each chosen desktop once. The exception
is GNOME, which locks its screen only under GDM, so GDM stays its login
screen. Setting `"aizome"` takes Aizome's anyway, and GNOME sessions then
have no lock screen.

To look at it inside a session: `aizome-greeter --nested --demo`. `--demo`
stands in for greetd and takes anyone, with the password `aizome`.

## Checks

- `cargo test -p aizome-greeter` covers:
  - greetd's framing and messages;
  - desktop entries and `Exec` quoting;
  - the people and `login.defs`;
  - the configuration and what's remembered;
  - the conversation: a wrong password, several questions, cancelling, and
    late answers;
  - the screens, headless, with pictures in `target/greeter-shots/`.
- `nix build .#checks.x86_64-linux.greeter` runs the VM test: two people,
  and Aizome, sway and xterm's X11 session, typed at as a person would. It
  checks:
  - the list and the session menu;
  - a wrong password, then the right one, which starts Aizome and unlocks
    the keyring;
  - logging out and back, remembered;
  - Bob in sway, then in X11;
  - the power menu.
