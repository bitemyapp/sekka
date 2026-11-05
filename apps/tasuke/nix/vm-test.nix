# SPDX-License-Identifier: MIT OR Apache-2.0
# Tasuke on a real NixOS system, with a scripted model:
#
# - The module installs the command, the root helper under its polkit
#   action, and the user unit tasuke-llm.service, which starts at login with
#   its API key (0600), its low priority and the router's arguments. A
#   stand-in llama-server answers instead of a model.
# - tasuke-helper runs a JSON request as root, refuses what Tasuke never
#   runs, refuses to run as anyone but root, works through pkexec under its
#   action, and logs to the journal.
# - `tasuke ask --approve all` against a scripted OpenAI-compatible server
#   (fake-openai.py): a read-only command, one needing approval, one as root
#   and one Tasuke refuses, then an answer. As alice, run_as_root goes
#   through pkexec (a polkit rule stands in for her password); as root it
#   runs the helper directly, as in the evaluation VM.
# - The opt-in weekly checkup is a user timer at the lowest priority.
{
  pkgs,
  tasuke,
  tasukeModule,
}:
let
  fakeOpenai = ./fake-openai.py;
  # Answers /health on the router's port, and says how it was started.
  fakeLlamaServer = pkgs.writeShellScriptBin "llama-server" ''
    printf '%s\n' "$@" > "$XDG_RUNTIME_DIR/tasuke/llama-args"
    exec ${pkgs.python3}/bin/python3 -I ${fakeOpenai} 8737 "$XDG_RUNTIME_DIR/tasuke/llama-requests.jsonl"
  '';
in
pkgs.testers.runNixOSTest {
  name = "tasuke";
  globalTimeout = 600;
  nodes.machine =
    { ... }:
    {
      imports = [ tasukeModule ];
      programs.tasuke = {
        enable = true;
        package = tasuke;
        llamaCpp = fakeLlamaServer;
        checkup.enable = true;
      };
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
        # Her user manager, and so tasuke-llm.service, runs from boot.
        linger = true;
      };
      # Stands in for alice's password in the polkit dialog.
      security.polkit.extraConfig = ''
        polkit.addRule(function(action, subject) {
          if (action.id == "io.github.bitemyapp.Tasuke.run-as-root" && subject.user == "alice") {
            return polkit.Result.YES;
          }
        });
      '';
      environment.etc."tasuke-test/marker".text = "tasuke-marker-6271\n";
      systemd.tmpfiles.rules = [
        "d /var/lib/tasuke-test 0755 root root -"
        "d /tmp/tasuke-test 1777 root root -"
      ];
      systemd.services.fake-openai = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          ExecStart = "${pkgs.python3}/bin/python3 -I ${fakeOpenai} 8090 /var/lib/fake-openai/requests.jsonl";
          StateDirectory = "fake-openai";
        };
      };
      virtualisation = {
        memorySize = 2048;
        cores = 2;
      };
    };
  testScript = ''
    import json, shlex

    POLICY = "/run/current-system/sw/share/polkit-1/actions/io.github.bitemyapp.Tasuke.policy"
    REQUESTS = "/var/lib/fake-openai/requests.jsonl"

    def as_alice(command, check=True):
        wrapped = "su - alice -c " + shlex.quote(command)
        return machine.succeed(wrapped) if check else machine.execute(wrapped)

    def user_unit(prop):
        return machine.succeed(
            f"systemctl --machine=alice@.host --user show -p {prop} --value tasuke-llm.service"
        ).strip()

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("fake-openai.service")
    machine.wait_for_open_port(8090)

    with subtest("the module installs the helper under its polkit action"):
        policy = machine.succeed(f"cat {POLICY}")
        assert "io.github.bitemyapp.Tasuke.run-as-root" in policy, policy
        assert "<allow_active>auth_admin_keep</allow_active>" in policy, policy
        helper = policy.split('policykit.exec.path">')[1].split("<")[0].strip()
        assert helper.startswith("/nix/store/") and helper.endswith("/libexec/tasuke-helper"), helper
        machine.succeed(f"test -x {helper}")
        machine.succeed("test -u /run/wrappers/bin/pkexec")

    with subtest("the model server starts at login, at low priority, with its key"):
        machine.wait_for_unit("user@1000.service")
        machine.wait_until_succeeds(
            "test \"$(systemctl --machine=alice@.host --user show -p ActiveState --value tasuke-llm.service)\" = active",
            timeout=60,
        )
        machine.wait_for_open_port(8737)
        assert user_unit("Nice") == "10", user_unit("Nice")
        assert user_unit("CPUWeight") == "20", user_unit("CPUWeight")
        assert user_unit("IOWeight") == "20", user_unit("IOWeight")
        assert user_unit("OOMScoreAdjust") == "500", user_unit("OOMScoreAdjust")
        assert machine.succeed("stat -c '%a %U' /run/user/1000/tasuke/api-key").strip() == "600 alice"
        args = machine.succeed("cat /run/user/1000/tasuke/llama-args").split("\n")
        for expected in ["--models-preset", "--models-max", "127.0.0.1", "8737", "--api-key-file", "--sleep-idle-seconds"]:
            assert expected in args, args
        assert args[args.index("--sleep-idle-seconds") + 1] == "600", args
        machine.succeed("su - alice -c 'test -s ~/.local/share/tasuke/models.ini'")
        status = as_alice("tasuke server status")
        assert "answering" in status, status

    with subtest("the opt-in weekly checkup is a low-priority user timer"):
        machine.wait_until_succeeds(
            "test \"$(systemctl --machine=alice@.host --user show -p ActiveState --value tasuke-checkup.timer)\" = active",
            timeout=30,
        )
        calendar = machine.succeed(
            "systemctl --machine=alice@.host --user show -p TimersCalendar --value tasuke-checkup.timer"
        )
        assert "Mon *-*-* 10:00:00" in calendar, calendar
        props = machine.succeed(
            "systemctl --machine=alice@.host --user show -p Nice -p CPUWeight -p IOWeight tasuke-checkup.service"
        )
        assert sorted(props.split()) == ["CPUWeight=10", "IOWeight=10", "Nice=19"], props

    with subtest("the helper runs one request as root, and refuses what Tasuke never runs"):
        out = machine.succeed(f"echo '{{\"command\": \"id -un; echo $HOME\", \"purpose\": \"test\"}}' | {helper} run")
        r = json.loads(out)
        assert r == {"exit_code": 0, "output": "root\n/root\n", "truncated": False, "timed_out": False}, r
        status, out = machine.execute(f"echo '{{\"command\": \"reboot\", \"purpose\": \"test\"}}' | {helper} run")
        assert status == 3, (status, out)
        assert "refused" in json.loads(out), out
        status, out = machine.execute(f"echo '{{\"command\": \"rm -rf /home/alice\"}}' | {helper} run")
        assert status == 3, (status, out)
        machine.succeed("test -d /home/alice")
        r = json.loads(machine.succeed(f"echo '{{\"command\": \"sleep 30\", \"timeout_seconds\": 1}}' | {helper} run"))
        assert r["timed_out"] and r["exit_code"] is None, r

    with subtest("the helper runs only as root, and through pkexec under its action"):
        status, out = as_alice(f"echo '{{\"command\": \"id\"}}' | {helper} run", check=False)
        assert status == 1, (status, out)
        out = as_alice(
            f"echo '{{\"command\": \"id -un > /var/lib/tasuke-test/pkexec-ran\", \"purpose\": \"pkexec test\"}}'"
            f" | /run/wrappers/bin/pkexec --disable-internal-agent {helper} run"
        )
        assert json.loads(out)["exit_code"] == 0, out
        assert machine.succeed("stat -c %U /var/lib/tasuke-test/pkexec-ran").strip() == "root"
        journal = machine.succeed("journalctl -t tasuke-helper --no-pager")
        assert "alice (uid 1000) runs as root: id -un > /var/lib/tasuke-test/pkexec-ran (purpose: pkexec test)" in journal, journal
        assert "refused for root (uid 0): reboot" in journal, journal

    ASK = (
        "tasuke ask --act --approve all --base-url http://127.0.0.1:8090/v1 --model fake "
        + shlex.quote("Read the marker, make a file, write one as root, then tidy up.")
    )

    def check_ask(user, home, output):
        assert "All four steps are done." in output, output
        requests = [json.loads(l) for l in machine.succeed(f"cat {REQUESTS}").splitlines() if l.strip()]
        tool_results = [
            m.get("content") or "" for r in requests for m in r["body"].get("messages", []) if m.get("role") == "tool"
        ]
        # The read-only command ran without asking, and its output went back.
        assert any("tasuke-marker-6271" in str(t) for t in tool_results), tool_results
        # The one needing approval ran as the person, once approved.
        assert machine.succeed("stat -c %U /tmp/tasuke-test/approved").strip() == user
        # run_as_root ran as root.
        assert machine.succeed("stat -c %U /var/lib/tasuke-test/root-ran").strip() == "root"
        # The refused one didn't run.
        machine.succeed(f"test -f {home}/keep")

    def reset():
        machine.succeed(f"rm -f /tmp/tasuke-test/approved /var/lib/tasuke-test/root-ran; : > {REQUESTS}")

    with subtest("tasuke ask as alice: approvals, run_as_root through pkexec, a refusal"):
        reset()
        machine.succeed("su - alice -c 'touch ~/keep'")
        out = as_alice(ASK)
        print(out)
        check_ask("alice", "/home/alice", out)
        journal = machine.succeed("journalctl -t tasuke-helper --no-pager")
        assert "alice (uid 1000) runs as root: date > /var/lib/tasuke-test/root-ran" in journal, journal

    with subtest("tasuke ask as root, as the evaluation VM runs it: the helper directly"):
        reset()
        machine.succeed("touch /root/keep")
        out = machine.succeed(ASK)
        print(out)
        check_ask("root", "/root", out)
  '';
}
