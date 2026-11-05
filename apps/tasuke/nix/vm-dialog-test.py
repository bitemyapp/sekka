"""The Assistant window through Aizome's real password dialog."""

QUESTION = "Read the marker, make a file, write one as root, then tidy up."
ANSWER = "All four steps are done."
# Gemma 4 12B's two files at their catalog sizes, sparse: the window counts
# the model as downloaded, and the scripted server answers for it.
MODEL = "gemma-4-12b"
FILES = {"gemma-4-12b-it-IQ4_XS.gguf": 6375734080, "mmproj-F16.gguf": 175115840}

start_session()

with subtest("the window opens on a downloaded model, and the question goes"):
    user(f"mkdir -p ~/.config/tasuke ~/.local/share/tasuke/models/{MODEL}")
    user(f"printf '[model]\\nid = \"{MODEL}\"\\n' > ~/.config/tasuke/config.toml")
    for name, size in FILES.items():
        user(f"truncate -s {size} ~/.local/share/tasuke/models/{MODEL}/{name}")
    user("touch ~/keep")
    user("aizomectl spawn aizome-tasuke")
    click("the composer", lambda e: e.get("role") == "multilinetextinput")
    input_("type " + shlex.quote(QUESTION))
    input_("key Return")

with subtest("a change waits for Run, then runs as alice"):
    click("Run", button("Run"))
    machine.wait_until_succeeds("test -f /tmp/tasuke-test/approved", timeout=30)
    assert machine.succeed("stat -c %U /tmp/tasuke-test/approved").strip() == "alice"

with subtest("Run as administrator brings up the password dialog"):
    click("Run as administrator", button("Run as administrator"))
    until("the administrator password dialog", password_visible)
    machine.screenshot("password-dialog")
    # Nothing has run as root before the password.
    machine.fail("test -e /var/lib/tasuke-test/root-ran")
    input_("type test-only")
    input_("key Return")
    until("the password dialog closes", lambda: not password_visible())

with subtest("the helper ran the command as root, and logged who asked"):
    machine.wait_until_succeeds("test -f /var/lib/tasuke-test/root-ran", timeout=30)
    assert machine.succeed("stat -c %U /var/lib/tasuke-test/root-ran").strip() == "root"
    journal = machine.succeed("journalctl -t tasuke-helper --no-pager")
    assert "alice (uid 1000) runs as root: date > /var/lib/tasuke-test/root-ran" in journal, journal

with subtest("the refused command didn't run, and the answer arrives"):
    until("the answer", shows(ANSWER))
    machine.succeed("test -f /home/alice/keep")
    # Its line says it was refused, not that it ran.
    assert find(lambda e: e.get("text") == "Refused"), "the refused command's line"
    machine.screenshot("answered")
