import json
import re

machine.wait_for_unit("multi-user.target")

with subtest("the policy names the installed reader, and pkexec can run it"):
    policy = machine.succeed("cat /run/current-system/sw/share/polkit-1/actions/org.aizome.Nisshi.policy")
    named = re.search(r'exec\.path">([^<]+)<', policy)
    assert named, policy
    helper = named.group(1)
    assert helper.endswith("/libexec/aizome-nisshi-helper"), helper
    machine.succeed(f"test -x {helper}")
    machine.succeed("test -u /run/wrappers/bin/pkexec")

# Something only the system's journal has.
machine.succeed("echo 'written by root for nisshi' | systemd-cat -t nisshi-marker -p warning")
machine.wait_until_succeeds("journalctl -t nisshi-marker | grep -q 'written by root'")

with subtest("someone who isn't an administrator reads only their own"):
    out = machine.succeed("su - alice -c 'aizome-nisshi --print --lines 500' 2>&1")
    print(out)
    assert "Reading as uid 1000" in out and "only this user's journal" in out, out
    assert "written by root" not in out, out

with subtest("unlocked, the reader runs as root and reads the system's"):
    out = machine.succeed("su - alice -c 'aizome-nisshi --print --system --lines 500' 2>&1")
    print(out)
    assert "Reading as uid 0" in out and "the system's journal" in out, out
    assert re.search(r"warning +nisshi-marker(\[\d+\])?: written by root for nisshi", out), out

with subtest("refused, with no agent to ask, before the reader runs"):
    out = machine.fail("su - bob -c 'aizome-nisshi --print --system' 2>&1")
    print(out)
    assert "The password dialog isn't available right now." in out, out

def ask(*requests):
    lines = "\n".join(json.dumps(r) for r in requests)
    out = machine.succeed(f"printf '%s\\n' '{lines}' | {helper}")
    print(out)
    return out

with subtest("as root, the reader takes only reads"):
    machine.succeed("ln -sf /etc/nisshi-not-a-log /var/log/escape.log")
    machine.succeed("echo 'a log line' > /var/log/real.log && chmod 600 /var/log/real.log")
    out = ask(
        {"id": 1, "op": {"query": {"matches": [["--rotate", "x"]]}}},
        {"id": 2, "op": {"read": {"path": "/etc/nisshi-not-a-log", "max": 100}}},
        {"id": 3, "op": {"read": {"path": "/var/log/escape.log", "max": 100}}},
        {"id": 4, "op": {"read": {"path": "/var/log/real.log", "max": 100}}},
        {"id": 5, "op": {"query": {"before": "--vacuum-time=1s", "limit": 1}}},
    )
    assert re.search(r"^1 x .*isn't a journal field", out, re.M), out
    assert re.search(r"^2 x .*isn't a log under /var/log", out, re.M), out
    assert re.search(r"^3 x .*isn't a log under /var/log", out, re.M), out
    assert "4 t a log line" in out, out
    assert re.search(r"^5 x .*isn't a journal cursor", out, re.M), out
    assert "not for the reader" not in out, out
    # The journal is as it was: nothing rotated or vacuumed.
    machine.succeed("journalctl -t nisshi-marker | grep -q 'written by root'")
