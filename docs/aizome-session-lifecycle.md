# Aizome session lifecycle: guarantees and open issues

Aizome already runs its compositor as a systemd user service. Its session
launcher additionally connects that service to the login that started it,
imports the desktop environment, and owns cleanup. The launcher's lock prevents
two login wrappers from claiming that ownership; systemd's guarantee of one
instance of a named service does not replace it.

The current implementation fixes demonstrated logout and overlapping-login
failures, but concentrates substantial session-management logic in a shell
script. Cleanup after an uncatchable launcher death, application lifetime, and
the custom restart policy remain subjects for improvement. This document
records the evidence and requirements for that work, without changing policy or
choosing a replacement implementation.

Reviewed on 2026-11-02 against Sekka commit
`5b742116c79897e777fe75ba818d53c6088b3a1c`, with root Nixpkgs input
`0bb2eedc20e4739f29e8045034f2828ad7054f8f`. Findings below come from source
inspection and Nix configuration evaluation. Historical runtime evidence is
identified separately; no new VM, ISO, or physical-hardware run was performed
for this review.

## Who owns what

| Component | Lifetime and responsibility |
| --- | --- |
| Login scope managed by logind | Contains the display manager's login worker and `aizome-session`. Ending this scope can signal the launcher without stopping the user manager. |
| Per-user systemd manager | Runs `aizome.service`, graphical-session targets, application scopes, and other user services. It can outlive a particular login. |
| `aizome-session` launcher | Holds the per-user launch lock, imports environment variables, waits for the compositor, and initiates graphical cleanup when it exits. |
| `aizome.service` | Directly executes the compositor, reports readiness, and uses systemd's watchdog. Its dependencies connect it to the graphical targets. |
| Application scopes | Launch desktop applications separately from the compositor service. Their creation does not bind them to the graphical-session target. |

The implementation is in the [launcher](../desktop/aizome/session/aizome-session),
[compositor unit](../desktop/aizome/session/aizome.service),
[session target](../desktop/aizome/session/aizome-session.target), and
[shutdown target](../desktop/aizome/session/aizome-shutdown.target).

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

These are separate mechanisms. `KillUserProcesses` controls cleanup of
login-session scopes. Lingering keeps the user manager available after logout;
without it, `UserStopDelaySec` controls the normal delay after the final logout
(upstream default: ten seconds). Upstream still documents
`KillUserProcesses=yes`; describing the historical default change as universally
reverted is inaccurate. See [logind.conf](https://www.freedesktop.org/software/systemd/man/latest/logind.conf.html)
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
started outside that lock, for example by an older launcher.

The [October 28 incident report](incidents/2026-10-28-aizome-orphaned-session.md)
records both the orphaned-compositor bug and the later overlapping-launch
reproduction with real systemd. It also records a delayed start client starting
the desktop after cleanup had finished. The current launcher cancels and reaps
that client before stopping the targets. These safeguards address reproduced
failures; removing them requires an alternative that preserves their guarantees.

## Remaining issues

### Cleanup depends on the launcher surviving long enough

Normal exit, HUP, INT, and TERM run the cleanup trap. SIGKILL cannot. If the
launcher is killed that way while the user manager survives, its lock is
released but the compositor and graphical targets can remain. A subsequent
login then encounters the existing-service guard. This is a documented source
limitation, not a newly reproduced workstation incident in this review.

A replacement should supervise the originating login's lifetime independently
of catchable signals in that login's shell. Adding a user service alone does
not supply that connection to logind's system-manager session scope. UWSM is a
useful implementation to evaluate: it explicitly combines compositor units,
environment setup/cleanup, and a PID-tracking service. Tatami already invokes it
in its [session entry](../desktop/tatami/module.nix). This is a candidate for
investigation, not a claim that Aizome can adopt it without integration work.
See [UWSM's operation overview](https://man.archlinux.org/man/uwsm.1.en#OPERATION_OVERVIEW).

### The shell implements part of the service supervisor

The launcher manages asynchronous `systemctl` execution, signal arrival around
PID capture, cancellation before and after job submission, reaping, and retries
after watchdog failures. Its restart policy permits up to three watchdog
restarts and resets the count after a run of at least five minutes, while the
unit sets `Restart=no`.

Systemd provides [`Restart=on-watchdog`](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#Restart=)
and [start-rate limits](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html#StartLimitIntervalSec=).
Moving restart responsibility into the unit could reduce custom supervision.
Those settings are not automatically equivalent to the current counter/reset
policy. The change must also account for target shutdown, the launcher's wait,
and explicit logout so a restart cannot resurrect an ended session.

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
as rootless Docker. There is no new exhaustive application-survival test in
this review.

### Ownership is currently per user

The lock, service name, graphical targets, and imported manager environment
are shared per user. They do not implement independent concurrent Aizome
desktops for the same UID. A redesign must either retain that restriction
explicitly or provide session-specific ownership and environment handling.
It must not let a rejected login clean up another session.

## Sway references are test infrastructure

The evaluated live and installed Aizome configurations have
`programs.sway.enable=false` and no Sway-named entries in
`environment.systemPackages`. This checks the configured package list, not an
exhaustive runtime-closure audit.

Sukumo's [Wayland tests](../nix/sukumo-test.nix) and Hotaru's
[graphical tests](../desktop/aizome/apps/hotaru/tests/wayland/smoke.py) use
private headless Sway instances. Their `swaymsg` calls inspect windows and
control test outputs, including scale changes and simulated hotplug. The
[greeter VM](../desktop/aizome/nix/vm-greeter-test.nix) also deliberately enables
Sway to test launching another Wayland desktop. Aizome's runtime logout paths
above do not call `swaymsg`.

Using another compositor exercises application/client behavior independently
of Aizome. Such results do not certify Aizome's compositor, its login lifecycle,
or an installable ISO.

## Requirements for a replacement

Keep the existing ownership and cancellation protections until their
replacement is demonstrated. In disposable VMs, check:

- Normal Aizome logout and Yukimi's `loginctl terminate-session auto` both end
  the compositor and desktop services, clear owned environment variables, and
  permit another password login while the user manager remains alive.
- Overlapping launches are rejected both before start submission and before
  compositor readiness; rejection leaves the owner's service and environment
  intact.
- Ending the login before or after job submission cannot leave a client that
  later starts the desktop. If launcher-death robustness is claimed, include
  SIGKILL while the user manager survives.
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
stand-ins; the second uses real systemd with a stand-in compositor; the third
exercises graphical login and real logind termination. The incident report
records their historical results. They were inspected, not rerun, for this
documentation change. They are explicitly invoked `aizome-checks` outputs;
ordinary `nix flake check` does not run them.

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
