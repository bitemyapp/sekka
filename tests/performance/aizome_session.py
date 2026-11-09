"""Activation environment for performance harnesses that own a private bus."""

from pathlib import Path
import shlex


def private_bus_shell(root, bus_address, shell):
    """Publish the compositor's child environment before starting its shell.

    The compositor supervises this wrapper just like the shell it replaces.
    Copy it into the trial directory so it is visible inside bubblewrap's
    private /tmp. Never import into the host's systemd user manager.
    """
    if not bus_address.startswith("unix:path="):
        raise ValueError("expected the harness's private Unix session bus")
    wrapper = Path(root).resolve() / "aizome-private-shell"
    wrapper.write_text(
        "#!/bin/sh\nset -eu\n"
        f'if [ "${{DBUS_SESSION_BUS_ADDRESS-}}" != {shlex.quote(bus_address)} ]; then\n'
        "  echo 'refusing to export outside the harness session bus' >&2\n"
        "  exit 1\n"
        "fi\n"
        "dbus-update-activation-environment WAYLAND_DISPLAY AIZOME_SOCKET AIZOME_SHELL_SOCKET "
        "XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE AIZOME_SESSION ${DISPLAY:+DISPLAY}\n"
        f'exec {shlex.quote(str(shell))} "$@"\n'
    )
    wrapper.chmod(0o700)
    return str(wrapper)
