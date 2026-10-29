# Aizome login refused after the previous login session ended

Reported 2026-10-28. Component: Aizome's session launcher.

## Report and evidence

After leaving a machine running Steam tests, the user returned to the login
greeter. Entering the correct password briefly reported that Aizome was
already running, then returned to the greeter. The user initially suspected
idle sleep with Keep Awake off.

The workstation journal instead records a system activation at 18:46:48 CDT,
followed at 18:46:57 by termination of the existing graphical login scope.
systemd sent SIGTERM to its greetd worker, `aizome-session`, the launcher's
waiting `systemctl`, and the login's keyring process. logind then removed
the login session. There was no `systemd-suspend` entry in that boot.

The compositor remained active in the user's `aizome.service`, in a separate
cgroup under the user manager. Its IPC still listed windows, but attempts
to scan out frames failed with DRM permission errors after the login scope
ended. Later password attempts opened new logind sessions and immediately
closed them. The launcher's existing-service guard explains the observed
error and the repeated return to the greeter.

Yukimi's Sign Out invokes `loginctl terminate-session`, a path capable of
producing this sequence. The journal does not identify who requested this
particular termination, so it does not establish whether the testing agent
used that control or another caller ended the session. The preceding system
activation alone is not evidence that switching the configuration killed
the login.

## Cause and fix

The launcher previously cleaned up only after `systemctl --user --wait start
aizome.service` returned. A termination signal killed the launcher before
those commands ran, leaving the compositor and session target alive outside
the login scope. Subsequent launches refused to start a second compositor.

An exit trap now stops the graphical session through `aizome-shutdown.target`
and unsets its imported environment. HUP, INT and TERM exit through that
trap. The blocking `systemctl` runs in the background, with the shell's
`wait` builtin, so traps can run while the compositor is alive. Waiting for
the same command in the foreground would defer signal handling. Duplicate
logins are rejected before installing the trap, so they cannot stop a valid
existing desktop. The watchdog restart loop and normal compositor exit
status are retained.

## Regression checks and limits

`aizome-checks.session-launcher` uses isolated stand-ins for systemctl and
D-Bus activation. It covers normal exit, HUP, INT, TERM to just the launcher,
TERM to its process group, logging back in, duplicate-login isolation, and
the watchdog restart limit. Against the old launcher, signal tests reproduce
the missing cleanup; the interrupt case also demonstrates a deferred wait.

`aizome-checks.greeter` terminates only the graphical login with real logind,
keeps the user manager alive, checks compositor and graphical-target
shutdown and environment removal, then logs the same user in again through
the greeter. It also signs out from the user manager with Yukimi's exact
`loginctl terminate-session auto` command. Terminating the whole user, as the
previous test did, also killed the user manager and hid the missing cleanup.

On 2026-10-28, all seven launcher checks, ShellCheck, the packaged Aizome build
and the complete greeter VM check passed. The old launcher failed the
isolated termination tests with the stand-in service still active. The
reported workstation's orphaned desktop was left running at the user's
request; the fix was not deployed there during this investigation.

These checks do not establish physical suspend/resume or exact-ISO behavior.
SIGKILL cannot run a shell trap. A session already orphaned by the previous
launcher must be stopped before a new login; that closes its remaining
windows, so workstation recovery needs the user's agreement if unsaved work
may remain.
