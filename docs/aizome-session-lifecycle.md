# Aizome session lifecycle: guarantees and open issues

Aizome already runs its compositor as a systemd user service. Its session
launcher additionally connects that service to the login that started it,
imports the desktop environment, and owns cleanup. The launcher's lock prevents
two login wrappers from claiming that ownership; systemd's guarantee of one
instance of a named service does not replace it.

The current implementation fixes demonstrated logout and overlapping-login
failures, and ends the session even when the launcher dies without running its
cleanup. It still concentrates substantial session-management logic in a shell
script; application lifetime, the custom restart policy, and per-user ownership
remain subjects for improvement. This document records the evidence and
requirements for that work, without choosing a replacement implementation.

Reviewed on 2026-11-02 against Sekka commit
`5b742116c79897e777fe75ba818d53c6088b3a1c`, with root Nixpkgs input
`0bb2eedc20e4739f29e8045034f2828ad7054f8f` (systemd 262). The launcher watch,
the compositor unit's environment cleanup, and the narrower failed-unit reset
described below were added in response to that review. Findings come from
source inspection, Nix configuration evaluation, and the component checks named
at the end; no ISO or physical-hardware run was performed.

## Who owns what

| Component | Lifetime and responsibility |
| --- | --- |
| Login scope managed by logind | Contains the display manager's login worker and `aizome-session`. Ending this scope can signal the launcher without stopping the user manager. |
| Per-user systemd manager | Runs `aizome.service`, graphical-session targets, application scopes, and other user services. It can outlive a particular login. |
| `aizome-session` launcher | Holds the per-user launch lock, imports its environment variables, waits for the compositor, and ends the session when it exits. |
| `aizome-session-watch.service` | Started by the launcher in the user manager. Waits on the launcher's lock and ends the session if the launcher dies without doing so. |
| `aizome.service` | Directly executes the compositor, reports readiness, and uses systemd's watchdog. Removes the compositor's exported variables when it stops. |
| Application scopes | Launch desktop applications separately from the compositor service. Their creation does not bind them to the graphical-session target. |

The implementation is in the [launcher](../desktop/aizome/session/aizome-session),
[watch](../desktop/aizome/session/aizome-session-watch.service),
[compositor unit](../desktop/aizome/session/aizome.service),
[session target](../desktop/aizome/session/aizome-session.target), and
[shutdown target](../desktop/aizome/session/aizome-shutdown.target).

The unit dependencies run in one direction. `aizome.service` has
`BindsTo=graphical-session.target`, and the session's services are `PartOf=`
that target or `aizome-session.target`, so stopping the targets stops the
compositor and the services. Nothing binds the targets to the compositor or to
the login. A compositor that exits leaves the targets and session services
running until something starts `aizome-shutdown.target`: the launcher, or the
watch in its place. The same property keeps session services running while the
launcher restarts a compositor that the watchdog stopped.

Aizome's own Log Out action sends its compositor an IPC `Quit` request in
[the shell](../desktop/aizome/crates/aizome-shell/src/confirm.rs). Yukimi's Sign
Out instead uses `loginctl terminate-session`, with the display-session fallback
`auto`, in [its command builder](../desktop/aizome/apps/yukimi/src/ops.rs).
Both paths need to end the desktop while allowing unrelated user services to
follow their own lifetime policy.

## Logind policy and lingering

Evaluation through the root flake produced these results:

| Configuration | `KillUserProcesses` | Installed user's `linger` |
| --- | --- | --- |
| Live image | `false` | Not assessed here |
| Installed Aizome, Firefox selected | `false` | `null` (unmanaged by NixOS) |
| Installed Aizome, Firefox and Docker selected | `false` | `true` |

Sekka inherits `KillUserProcesses=false` from the
[pinned NixOS module](https://github.com/NixOS/nixpkgs/blob/0bb2eedc20e4739f29e8045034f2828ad7054f8f/nixos/modules/system/boot/systemd/logind.nix).
That default preserves programs such as `nohup`, `tmux`, and `screen` across
logout. Sekka does not override `UserStopDelaySec`, `KillOnlyUsers`, or
`KillExcludeUsers` in these configurations. Selecting Docker enables lingering
in the [application module](../installer/genkan/rust/src/applications.nix) for
the installed user's rootless service. An unmanaged lingering setting is not
evidence that a particular workstation has lingering disabled.

These are separate mechanisms. `KillUserProcesses` decides whether logind kills
a login scope when its session ends normally; systemd's build-time default is
`yes`, and NixOS sets `no`. `loginctl terminate-session`, which Yukimi's Sign Out
runs, stops the scope whatever that setting says: logind's `Terminate` method
stops the session with force. Either way the user manager, and `aizome.service`
in it, can outlive the login.

Lingering keeps the user manager running after logout. Without it, and with no
other session open, the manager stops `UserStopDelaySec` (upstream default: ten
seconds) after the final logout, and everything left in it stops too. With
lingering, as when Docker is selected, whatever a login leaves in the manager
stays there. See [logind.conf](https://www.freedesktop.org/software/systemd/man/latest/logind.conf.html)
and [loginctl](https://www.freedesktop.org/software/systemd/man/latest/loginctl.html).

An inactive session also need not be logged out: switching VTs can leave a
session running in the background, and sessions need not be attached to a seat.
Cleanup should follow the intended session lifetime, not merely loss of
foreground status. See [systemd's session states](https://www.freedesktop.org/software/systemd/man/latest/sd_session_is_active.html).

Enabling `KillUserProcesses` globally would change SSH/background-job behavior
without, by itself, establishing ownership of Aizome's user-manager services.
It is not a substitute for the lifecycle work described here.

## Why the lock exists

Systemd will not create two concurrent instances of the same named service in
one manager. However, multiple callers can request and wait for that service.
Without exclusive launcher ownership, the following sequence is possible:

1. Login A starts its launcher and requests `aizome.service`.
2. Login B passes a startup check while the service is still activating and
   attaches another launcher to the same service.
3. Login B ends. Its exit trap stops the graphical targets and clears the
   environment, ending Login A's desktop too.

There is still only one compositor service in this failure. The conflict is
between the launchers' cleanup responsibilities. The per-user `flock` covers
startup through teardown and is acquired before installing cleanup traps. The
service-state check additionally rejects an active or transitioning service
started outside that lock, for example by an older launcher. The kernel
releases the lock with the launcher, however the launcher dies, which is what
the watch waits for.

The [October 28 incident report](incidents/2026-10-28-aizome-orphaned-session.md)
records both the orphaned-compositor bug and the later overlapping-launch
reproduction with real systemd. It also records a delayed start client starting
the desktop after cleanup had finished. The current launcher cancels and reaps
that client before stopping the targets. These safeguards address reproduced
failures; removing them requires an alternative that preserves their guarantees.

## Ending the session

### When the launcher exits

Normal exit, HUP, INT, and TERM run the launcher's cleanup trap. It kills and
reaps its start client, starts `aizome-shutdown.target` with
`--job-mode=replace-irreversibly`, removes the variables it imported, and then
stops the watch while it still holds the lock. Stopping the watch earlier would
leave a window in which nothing could finish an interrupted cleanup; leaving it
running would let it take the lock as the launcher exits and repeat the
teardown, refusing a login meanwhile.

### When the launcher dies

A launcher killed with SIGKILL runs no trap. Neither does one killed partway
through its cleanup: when a login scope's stop times out, systemd sends SIGKILL
to whatever is left in it, which can include a launcher still waiting for a
compositor that is slow to stop. Before the watch, either left the compositor
and graphical targets running, and the next login failed at the
existing-service guard. Whether that orphan persisted depended on lingering, as
described above.

[`aizome-session-watch.service`](../desktop/aizome/session/aizome-session-watch.service)
covers both cases. The launcher starts it once it holds the lock and has
imported its variables. The watch runs `flock` on the same lock file and waits.
When the launcher dies, the watch takes the lock, so a new login is refused
rather than started into a session that is ending. It then runs the launcher's
own teardown, `aizome-session --end-session`: it starts `aizome-shutdown.target`
and removes the variables named in `AIZOME_ENVIRONMENT_KEYS`, the launcher's
record, which the watch inherited from the manager. Waiting on the lock rather
than a PID means PID reuse cannot mislead it. UWSM, which
[Tatami's session entry](../desktop/tatami/module.nix) invokes, follows the same
pattern with a unit that waits on the launcher's PID.

The remaining gaps are narrow:

- A launcher killed between importing its variables and starting the watch
  leaves those variables, but no compositor, in the manager. The next login
  replaces them.
- A process that inherited the lock descriptor holds the lock until it exits.
  If only the launcher is killed during cleanup, its `systemctl` call keeps the
  lock until that call's job finishes, and the watch runs after it. The
  long-running start client never inherits the descriptor.
- If the user manager stops, everything in it stops with it, the watch included.

### The environment

Each exported variable has one owner, which removes it:

- The launcher imports the desktop names (`XDG_CURRENT_DESKTOP`,
  `XDG_SESSION_DESKTOP`, `XDG_SESSION_TYPE`), the keys of the system and user
  environment files, and its own record (`AIZOME_ENVIRONMENT_KEYS`,
  `AIZOME_ENVIRONMENT_FILE`). The launcher, or the watch in its place, removes
  them.
- The compositor exports its sockets and session flags: `WAYLAND_DISPLAY`,
  `DISPLAY`, `AIZOME_SOCKET`, `AIZOME_SHELL_SOCKET`, `AIZOME_SESSION`, and the
  desktop names again. `aizome.service` removes them in `ExecStopPost=`, which
  runs however the compositor stops, including a crash, a watchdog stop, or a
  kill. A unit test in [`aizome-comp-core`](../desktop/aizome/crates/aizome-comp-core/src/session.rs)
  keeps the unit's list equal to the variables the compositor exports.

The launcher previously removed a fixed list on the compositor's behalf. That
list omitted `AIZOME_SESSION`, which then outlived every session.

Unsetting a variable removes only the value set through the manager's API. A
value the manager had from PAM or its environment generators, such as NixOS's
`XDG_CONFIG_DIRS`, which Aizome's system environment file overrides, becomes
visible again. The checks therefore compare the manager's whole environment
before login and after logout rather than a list of names.

The launcher and the compositor also call
`dbus-update-activation-environment --systemd`. dbus-broker, the default D-Bus
implementation in the pinned NixOS and the one Sekka uses, forwards
activation-environment updates to the user manager's `SetEnvironment` and keeps
no copy, so removing a variable from the manager removes it for D-Bus
activation too. The reference dbus-daemon instead keeps its own activation
environment for services it starts directly. The D-Bus API can set variables
there but not remove them, so with that implementation they persist until
overwritten.

### Failed units

Before importing its variables, the launcher resets the failed state of the
units bound to the session: those listed in the `ConsistsOf=` and `BoundBy=`
properties of `graphical-session.target` and `aizome-session.target`. A session
service that hit its start limit as the previous session ended can therefore
start again. The launcher previously ran `systemctl --user reset-failed` with no
unit names, which also cleared the failures of unrelated user services.

## Remaining issues

### The shell implements part of the service supervisor

The launcher manages asynchronous `systemctl` execution, signal arrival around
PID capture, cancellation before and after job submission, reaping, the watch,
and retries after watchdog failures. Its restart policy permits up to three
watchdog restarts and resets the count after a run of at least five minutes,
while the unit sets `Restart=no`.

Systemd provides [`Restart=on-watchdog`](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#Restart=)
and [start-rate limits](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html#StartLimitIntervalSec=).
Moving restart responsibility into the unit could reduce custom supervision.
Those settings are not automatically equivalent to the current counter/reset
policy. The change must also account for target shutdown, the launcher's wait,
and explicit logout so a restart cannot resurrect an ended session. Because the
dependencies run one way, a restart by systemd would also leave session services
running, as the launcher's does. Binding the session targets to the compositor
would instead let its exit end the session within the unit graph, but would
restart those services with every compositor restart.

The maintainability concern is the amount of lifecycle machinery maintained
here, rather than the existence of a shell entry point. The non-systemd direct
launch fallback is a separate branch of the script; it does not explain the
lock inside the systemd branch.

### Graphical cleanup does not define every application's lifetime

The shutdown target stops the graphical targets and services tied to them.
Desktop-entry launches in [apps/launch.rs](../desktop/aizome/crates/aizome-sys/src/apps/launch.rs)
and keybinding/IPC launches in [session.rs](../desktop/aizome/crates/aizome-comp-core/src/session.rs)
create separate user-manager scopes without a dependency on those targets.
An application may exit when its display disconnects, but this is not a
guarantee that all its background processes end with the desktop. Processes
that survive display loss can remain while the user manager runs.

Before changing this, define which application scopes belong to the desktop
and which user services are deliberately independent. Do not use terminating
the whole user as a shortcut: it also affects other logins and services such
as rootless Docker. There is no exhaustive application-survival test.

### Ownership is currently per user

The lock, the watch, the service name, the graphical targets, and the imported
manager environment are shared per user. They do not implement independent
concurrent Aizome desktops for the same UID. A redesign must either retain that
restriction explicitly or provide session-specific ownership and environment
handling. It must not let a rejected login clean up another session. Tying the
desktop to its logind session rather than to the launcher process is one route:
the compositor already holds that session through libseat, though it does not
yet react when logind removes it.

## Requirements for a replacement

Keep the existing ownership, cancellation, and launcher-death protections until
their replacement is demonstrated. In disposable VMs, check:

- Normal Aizome logout and Yukimi's `loginctl terminate-session auto` both end
  the compositor and desktop services, leave the user manager's environment as
  it was before login, and permit another password login while the user
  manager remains alive.
- Overlapping launches are rejected both before start submission and before
  compositor readiness; rejection leaves the owner's service and environment
  intact.
- Ending the login before or after job submission cannot leave a client that
  later starts the desktop. SIGKILL to the launcher while the desktop runs, and
  to its whole login scope during cleanup, still ends the session while the
  user manager survives, and logins are refused until it has ended.
- Units bound to the session can start again after failing at the previous
  session's end; other units' failures remain.
- Watchdog failure, repeated failure, ordinary compositor crash, and explicit
  logout follow the chosen restart policy without restarting after logout.
- Desktop-bound applications follow the documented logout policy while a
  separate login and intentionally persistent user service remain unaffected.
- Switching to another supported desktop does not retain Aizome's services or
  environment. Concurrent-session behavior matches the stated per-user policy.

Existing component checks are
[`session-launcher`](../desktop/aizome/nix/session-launcher-test.py),
[`session-launcher-systemd`](../desktop/aizome/nix/session-launcher-systemd-test.py),
and [`greeter`](../desktop/aizome/nix/vm-greeter-test.nix). The first uses isolated
stand-ins, including the watch's command against a launcher killed with
SIGKILL. The second uses real systemd with a stand-in compositor that exports
variables and can be held while stopping; it covers the launcher-death,
environment, and failed-unit cases above. The third exercises graphical login
and real logind termination, comparing the user manager's environment with its
state before the first login. They are explicitly invoked `aizome-checks`
outputs; ordinary `nix flake check` does not run them.

```sh
nix build .#aizome-checks.session-launcher \
  .#aizome-checks.session-launcher-systemd .#aizome-checks.greeter -L
```

Component checks do not qualify an image. Any subsequent ISO qualification or
publication work must follow the repository's
[qualification procedure](../.agents/skills/sekka-iso-qualification/SKILL.md).

## Reproducing the configuration findings

From the repository root at the reviewed revision, this evaluates the live
configuration and the installer's Aizome reference systems without building or
activating a system:

```sh
nix eval --impure --json --no-write-lock-file --expr '
let
  root = builtins.getFlake (toString ./.);
  external = root.inputs;
  components = import ./nix/components.nix {
    inherit (external) nixpkgs applications superluminal tracy uta;
  };
  inputs = external // components;
  applicationCatalog = import ./nix/applications.nix { inherit inputs; };
  reference = applications: (import ./nix/reference-systems.nix {
    inherit inputs applicationCatalog applications;
    desktops = [ "aizome" ];
  }).systems.aizome.config;
  inspect = c: {
    logind = c.services.logind.settings;
    swayEnabled = c.programs.sway.enable;
    swayPackages = builtins.filter (name: builtins.match ".*sway.*" name != null)
      (map (p: p.name or "unnamed") c.environment.systemPackages);
  };
  installed = reference [ "firefox" ];
  docker = reference [ "firefox" "docker" ];
in {
  nixpkgsRevision = external.nixpkgs.rev;
  live = inspect root.nixosConfigurations.installer.config;
  installed = inspect installed // {
    linger = installed.users.users.${installed.calamares.installUser}.linger;
  };
  installedWithDocker = inspect docker // {
    linger = docker.users.users.${docker.calamares.installUser}.linger;
  };
}
'
```
