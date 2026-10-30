"""Skills in the home, and an agent's root through Aizome's password dialog."""
import json
import shlex
import time

INSPECT = "/run/user/1000/auth-inspect"
SKILLS = "/home/alice/.agents/skills"
CLAUDE = "/home/alice/.claude/skills"
# What a terminal in the session has, and so an agent started from it.
PATH = ("/run/wrappers/bin:/home/alice/.nix-profile/bin:/etc/profiles/per-user/alice/bin:"
        "/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin")


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command))


def until(description, condition, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = condition()
        if result:
            return result
        time.sleep(0.2)
    machine.screenshot("failed")
    raise AssertionError(description)


def documents():
    return [json.loads(line) for line in machine.succeed(
        f'for f in {INSPECT}/*.json; do if test -f "$f"; then cat "$f"; printf "\\n"; fi; done'
    ).splitlines() if line.strip()]


def password_visible():
    pids = {int(pid) for pid in machine.succeed("pgrep -u alice -x aizome-shell").split()}
    return any(d.get("pid") in pids and any(e.get("name") == "auth-password" and e["visible"]
                                            for e in d["elements"]) for d in documents())


def agent(name, command):
    """Start COMMAND as an agent's tool would: no terminal, its input a pipe
    nobody writes to, in a service of the user's manager rather than in the
    login session, from the session's directory of programs."""
    script = (f"exec 0< <(sleep infinity); {command} >/home/alice/{name}.out 2>&1; "
              f"echo $? >/home/alice/{name}.status")
    user(f"systemd-run --user --collect --unit={name} --setenv=PATH={PATH} "
         f"/run/current-system/sw/bin/bash -c {shlex.quote(script)}")


def finished(name):
    machine.wait_until_succeeds(f"test -e /home/alice/{name}.status", timeout=60)
    return (int(machine.succeed(f"cat /home/alice/{name}.status")),
            machine.succeed(f"cat /home/alice/{name}.out"))


def answer(name, keys):
    until("the password dialog", password_visible)
    machine.screenshot(f"dialog-{name}")
    for key in keys:
        user("aizomectl input " + key)
        time.sleep(0.2)
    until("the password dialog closes", lambda: not password_visible())


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
user("pkill -x aizome-annai || true")

# At login: Sekka's copies, as regular files the user owns, and Claude's links.
machine.wait_until_succeeds(f"test -f {SKILLS}/sekka/SKILL.md", timeout=60)
for skill in ["sekka", "yukimi", "aizome", "tatami"]:
    machine.succeed(f"test -f {SKILLS}/{skill}/SKILL.md -a ! -L {SKILLS}/{skill}/SKILL.md")
    machine.succeed(f"test $(stat -c %U {SKILLS}/{skill}/SKILL.md) = alice")
    machine.succeed(f"diff -r {shipped}/{skill} {SKILLS}/{skill}")
    assert machine.succeed(f"readlink {CLAUDE}/{skill}").strip() == f"{SKILLS}/{skill}"
machine.succeed(f"test -x {SKILLS}/yukimi/scripts/software -a -x {SKILLS}/sekka/scripts/apply-configuration")
print(user("sekka-agent-skills status"))

# Changed, removed and forgotten: a switch syncs again, keeps the change and
# the removal, and installs what it no longer knows it installed.
user(f"echo 'My note.' >> {SKILLS}/aizome/SKILL.md")
user(f"rm -r {SKILLS}/tatami")
user(f"rm -r {SKILLS}/sekka ~/.local/state/sekka/agent-skills/installed/sekka")
machine.succeed("/run/current-system/specialisation/switched/bin/switch-to-configuration test")
machine.wait_until_succeeds(f"test -f {SKILLS}/sekka/SKILL.md", timeout=60)
machine.succeed(f"grep -q 'My note.' {SKILLS}/aizome/SKILL.md")
machine.succeed(f"test ! -e {SKILLS}/tatami -a ! -L {CLAUDE}/tatami")
status = user("sekka-agent-skills status")
assert "aizome     changed" in status and "tatami     removed" in status, status
user("sekka-agent-skills restore")
for skill in ["aizome", "tatami"]:
    machine.succeed(f"diff -r {shipped}/{skill} {SKILLS}/{skill}")
    machine.succeed(f"test -L {CLAUDE}/{skill}")
machine.succeed("grep -q 'My note.' /home/alice/.local/state/sekka/agent-skills/backups/*/aizome/SKILL.md")

# The skill's command reads the real flake and yukimi.nix with Nix, then
# asks once through Aizome's dialog and gives the helper the whole list.
listed = user(f"cd /home/alice && {SKILLS}/yukimi/scripts/software list")
assert "Packages for everyone (yukimi.nix): ripgrep" in listed, listed
catalog = user(f"{SKILLS}/yukimi/scripts/software catalog")
assert catalog.splitlines() == ["docker\tDocker\tContainers"], catalog
agent("install", f"{SKILLS}/yukimi/scripts/software add hello")
answer("install", ["type test-only", "key Return"])
status, out = finished("install")
assert status == 0, out
assert machine.succeed("cat /var/lib/agent-helper-calls").splitlines() == [
    "change --packages ripgrep,hello --mode auto"], out
assert '"ok":true' in out, out

# A NixOS option: built as alice first, then installed and applied as root
# after one password, through the helper.
user("printf '{ ... }: { services.openssh.enable = true; }\\n' > /home/alice/configuration.nix")
agent("configure", f"{SKILLS}/sekka/scripts/apply-configuration configuration.nix=/home/alice/configuration.nix")
answer("configure", ["type test-only", "key Return"])
status, out = finished("configure")
assert status == 0, out
assert "+{ ... }: { services.openssh.enable = true; }" in out, out
assert machine.succeed("cat /var/lib/agent-helper-calls").splitlines()[-1] == "change --mode auto", out
machine.succeed("cmp /home/alice/configuration.nix /etc/nixos/configuration.nix")
assert machine.succeed("stat -c %U:%a /etc/nixos/configuration.nix").strip() == "root:644"

# The sekka skill's form: root in root's environment, with the system's
# programs (NixOS's PAM gives pkexec its PATH), and none of the caller's
# variables or its directory.
agent("generic", "cd /tmp && SECRET=agent pkexec --disable-internal-agent "
      "sh -c 'id -u; command -v systemctl; echo \"$HOME ${SECRET:-unset} $PWD\"' </dev/null")
answer("generic", ["type test-only", "key Return"])
status, out = finished("generic")
assert status == 0 and out.splitlines() == [
    "0", "/run/current-system/sw/bin/systemctl", "/root unset /root"], out

# Dismissing the dialog is pkexec's 126, and nothing runs.
agent("dismissed", "pkexec --disable-internal-agent touch /root/ran </dev/null")
answer("dismissed", ["key Escape"])
status, out = finished("dismissed")
assert status == 126, (status, out)
machine.succeed("test ! -e /root/ran")
machine.screenshot("done")
