"""The real catalog wrapper must preserve a profile when desktops change."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

wrapped, original = map(Path, sys.argv[1:])
executable = wrapped / "bin/signal-desktop"

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    config_home = root / "configuration with spaces"
    profile = config_home / "Signal"
    profile.mkdir(parents=True)
    config = profile / "config.json"
    # A launcher must never modify either this database or the key metadata.
    database = profile / "sql/db.sqlite"
    database.parent.mkdir()
    database.write_bytes(b"encrypted database fixture")
    env = dict(os.environ, HOME=str(root), XDG_CONFIG_HOME=str(config_home))
    env.pop("NODE_CONFIG", None)

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

    config.write_text(json.dumps({"safeStorageBackend": "kwallet6", "encryptedKey": "fixture"}))
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
