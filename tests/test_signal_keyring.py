"""The real catalog wrapper must preserve a profile when desktops change."""

from contextlib import ExitStack
import itertools
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time

wrapped, original = map(Path, sys.argv[1:])
executable = wrapped / "bin/signal-desktop"
keyrings = ["org.freedesktop.secrets", "org.kde.kwalletd", "org.kde.kwalletd5", "org.kde.kwalletd6"]
buses = itertools.count()

# The buses stop before their directory is removed, also when a check fails.
with tempfile.TemporaryDirectory() as directory, ExitStack() as cleanup:
    root = Path(directory)

    def start(*command, **options):
        process = subprocess.Popen(command, **options)
        cleanup.callback(process.wait)
        cleanup.callback(process.terminate)
        return process

    def bus(*activatable):
        """A private session bus on which the named services are activatable."""
        services = root / f"bus-{next(buses)}"
        services.mkdir()
        for name in activatable:
            (services / f"{name}.service").write_text(f"[D-BUS Service]\nName={name}\nExec=/bin/false\n")
        (services / "bus.conf").write_text(f"""<busconfig>
  <type>session</type>
  <listen>unix:path={services}/socket</listen>
  <servicedir>{services}</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
""")
        daemon = start(
            "dbus-daemon", "--nofork", "--print-address", f"--config-file={services}/bus.conf",
            stdout=subprocess.PIPE, text=True,
        )
        return daemon.stdout.readline().strip()

    config_home = root / "configuration with spaces"
    profile = config_home / "Signal"
    profile.mkdir(parents=True)
    config = profile / "config.json"
    # A launcher must never modify either this database or the key metadata.
    database = profile / "sql/db.sqlite"
    database.parent.mkdir()
    database.write_bytes(b"encrypted database fixture")
    env = dict(
        os.environ, HOME=str(root), XDG_CONFIG_HOME=str(config_home),
        DBUS_SESSION_BUS_ADDRESS=bus(*keyrings),
    )
    env.pop("NODE_CONFIG", None)
    env.pop("XDG_RUNTIME_DIR", None)

    def run(*args, desktop="Aizome", path=executable, extra_env=None):
        before = config.read_bytes() if config.exists() else None
        result = subprocess.run(
            [str(path), *args],
            env=dict(env, XDG_CURRENT_DESKTOP=desktop, **(extra_env or {})),
            check=True, text=True, capture_output=True,
        )
        assert (config.read_bytes() if config.exists() else None) == before
        assert database.read_bytes() == b"encrypted database fixture"
        return json.loads(result.stdout)

    # Negative control: the old unwrapped package supplies no keyring in Aizome.
    assert run(path=original / "bin/signal-desktop") == []
    assert run() == ["--password-store=gnome-libsecret"]
    assert run(desktop="KDE") == ["--password-store=gnome-libsecret"]

    # Reproduce the reported Plasma -> Aizome failure, as well as the reverse
    # direction and existing basic_text profiles. No new key is generated.
    for backend, flag in [
        ("kwallet6", "kwallet6"), ("kwallet5", "kwallet5"),
        ("kwallet", "kwallet"), ("gnome_libsecret", "gnome-libsecret"),
        ("basic_text", "basic"),
    ]:
        config.write_text(json.dumps({"safeStorageBackend": backend, "encryptedKey": "fixture"}))
        for desktop in ["Aizome", "KDE", "GNOME", "Hyprland", "Tatami:Hyprland"]:
            assert run("--use-tray-icon", "sgnl://test?one=two&three=four", desktop=desktop) == [
                f"--password-store={flag}", "--use-tray-icon", "sgnl://test?one=two&three=four",
            ]

    # A recorded keyring the session can't reach, as when Plasma (and with it
    # KWallet) was removed: Electron would report it selected but unusable,
    # and Signal would show a generic error offering to delete its data.
    # Without the flag, Signal reports the changed keyring instead.
    bare = {"DBUS_SESSION_BUS_ADDRESS": bus()}
    absent = {"DBUS_SESSION_BUS_ADDRESS": f"unix:path={root}/no-bus"}
    for backend in ["kwallet6", "kwallet5", "kwallet", "gnome_libsecret"]:
        config.write_text(json.dumps({"safeStorageBackend": backend, "encryptedKey": "fixture"}))
        assert run("sgnl://test", extra_env=bare) == ["sgnl://test"]
        assert run(extra_env=absent) == []
    config.write_text(json.dumps({"safeStorageBackend": "basic_text", "encryptedKey": "fixture"}))
    assert run(extra_env=bare) == ["--password-store=basic"]
    config.unlink()
    assert run(extra_env=bare) == []

    # A keyring that is running but not activatable counts as reachable.
    config.write_text(json.dumps({"safeStorageBackend": "kwallet6", "encryptedKey": "fixture"}))
    start("dbus-test-tool", "black-hole", "--session", "--name=org.kde.kwalletd6", env=dict(env, **bare))
    for _ in range(100):
        if run(extra_env=bare) == ["--password-store=kwallet6"]:
            break
        time.sleep(0.1)
    else:
        raise AssertionError("a running kwalletd6 was not found")

    for args in [
        ["--password-store=gnome-libsecret", "sgnl://test"],
        ["--password-store", "kwallet6"],
    ]:
        assert run(*args) == args

    # Electron's explicit userData path must not borrow the default's backend.
    alternate = root / "other profile"
    alternate.mkdir()
    alternate_config = alternate / "config.json"
    alternate_config.write_text('{"safeStorageBackend":"gnome_libsecret"}')
    for args in [[f"--user-data-dir={alternate}"], ["--user-data-dir", str(alternate)]]:
        assert run(*args) == ["--password-store=gnome-libsecret", *args]
    assert run(extra_env={"NODE_CONFIG": '{"storageProfile":"test"}'}) == []
    assert run(extra_env={"NODE_CONFIG": '{"storagePath":"/custom/path"}'}) == []

    # The desktop entry must execute our wrapper, including store-qualified Exec.
    entry = (wrapped / "share/applications/signal.desktop").read_text()
    entry_executable = shlex.split(next(line[5:] for line in entry.splitlines() if line.startswith("Exec=")))[0]
    assert entry_executable == str(executable)
    assert run(path=entry_executable) == ["--password-store=kwallet6"]

    for content in [
        '{"encryptedKey":"fixture"}', '{"safeStorageBackend":"future_backend"}',
        '{"safeStorageBackend":["kwallet6"]}', '{"safeStorageBackend":null}',
        '{"safeStorageBackend":"$(touch SHOULD_NOT_EXIST)"}', "{broken", "[]", "null",
    ]:
        config.write_text(content)
        assert run() == []
    config.write_text('{"key":"legacy plaintext fixture"}')
    assert run() == ["--password-store=gnome-libsecret"]
    config.unlink()
    assert run(extra_env={"XDG_CONFIG_HOME": ""}) == ["--password-store=gnome-libsecret"]

print("Signal keeps its recorded backend across desktops; new profiles use the keyring.")
