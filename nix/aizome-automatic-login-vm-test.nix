# Logging in automatically on Aizome's login screen, in a disposable VM: a
# Sekka system with Aizome its only desktop and `calamares.autoLogin` naming
# someone, started three times.
#
# 1. The account doesn't exist (removed while still chosen). greetd can't
#    start its initial session and exits; the login screen must come up
#    anyway, and a password login then works.
# 2. The account exists but has to set a password before it can log in, as
#    Settei leaves a new person added without one (AccountsService's
#    SetPasswordMode). The login screen must come up again.
# 3. The account can log in. The computer arrives in its session without
#    the login screen; logging out shows the login screen and logs nobody
#    in again, and neither does the greeter dying.
#
# Checked through what runs, systemd's state and the greeter's inspection
# files; nothing is read off the screen.
#
#     nix build .#checks.x86_64-linux.aizome-automatic-login -L
{
  pkgs,
  desktopsModule,
}:
pkgs.testers.runNixOSTest {
  name = "sekka-aizome-automatic-login";
  globalTimeout = 1500;
  node.pkgsReadOnly = false;
  nodes.machine =
    { config, lib, ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [ "aizome" ];
      calamares.defaultDesktop = "aizome";
      # Kara is added (and made unable, then able, to log in) by the test.
      calamares.autoLogin = "kara";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
      };
      # What the greeter shows, as files the test reads.
      services.greetd.settings.default_session.command =
        lib.mkForce "env AIZOME_KIT_INSPECT=/run/greeter-inspect ${config.programs.aizome.package}/bin/aizome-greeter";
      systemd.tmpfiles.rules = [ "d /run/greeter-inspect 0755 greeter greeter -" ];
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1280,yres=800"
        ];
      };
      hardware.graphics.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    };
  testScript = ''
    import json, time

    INSPECT = "/run/greeter-inspect"
    ACCOUNTS = "busctl call org.freedesktop.Accounts"

    def texts():
        out = machine.succeed(f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true')
        docs = [json.loads(l) for l in out.splitlines() if l.strip()]
        return [e["text"] for d in docs for e in d.get("elements", []) if e.get("text") and e.get("visible", True)]

    def until(what, check, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            r = check()
            if r:
                return r
            time.sleep(0.5)
        raise Exception(f"timed out waiting for {what}; the greeter shows {texts()}")

    def greetd(name):
        return machine.succeed(f"systemctl show greetd.service --property={name} --value").strip()

    def journal():
        return machine.succeed("journalctl -b -u greetd.service --no-pager -o cat")

    def session_of(who):
        return machine.execute(f"pgrep -u {who} -x aizome-shell")[0] == 0

    def greeter_up():
        # Not the display manager's unit alone: that is active while the
        # initial session is being tried, and failed when nothing follows.
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=120)
        until("the greeter's elements", lambda: texts())
        assert greetd("ActiveState") == "active", greetd("ActiveState")

    def nobody_arrives(who, seconds=20):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            assert not session_of(who), f"{who} was logged in again"
            time.sleep(1)

    def start_again():
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")

    machine.start()
    machine.wait_for_unit("multi-user.target")
    initial = machine.succeed("systemctl show greetd.service --property=ExecStart --value")
    config = [w for w in initial.replace(";", " ").split() if w.endswith(".toml")][0]
    assert 'user = "kara"' in machine.succeed(f"cat {config}"), "kara isn't greetd's initial session"

    with subtest("the chosen account is gone: the login screen comes up, and a password logs in"):
        machine.fail("id kara")
        greeter_up()
        assert "USER_UNKNOWN" in journal(), journal()
        assert int(greetd("NRestarts")) == 1, greetd("NRestarts")
        machine.succeed("test -e /run/greetd.run")
        # One person on the list: straight to the password.
        until("the password field", lambda: any(t == "Session: Aizome" for t in texts()))
        machine.send_chars("demo\n")
        machine.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=180)
        machine.wait_until_fails("pgrep -u greeter -x aizome-greeter", timeout=60)
        machine.screenshot("01-password-login-after-fallback")
        machine.succeed("loginctl terminate-user demo")
        machine.wait_until_fails("pgrep -u demo -x aizome-shell", timeout=90)
        machine.succeed(f"rm -f {INSPECT}/*.json")
        greeter_up()

    with subtest("the chosen account has to set a password first: the login screen comes up"):
        # As Settei adds a person with the password left empty.
        created = machine.succeed(f"{ACCOUNTS} /org/freedesktop/Accounts org.freedesktop.Accounts CreateUser ssi kara 'Kara Thrace' 0")
        account = created.split('"')[1]
        machine.succeed(f"{ACCOUNTS} {account} org.freedesktop.Accounts.User SetPasswordMode i 1")
        machine.wait_until_succeeds("chage -l kara | grep -c 'password must be changed'", timeout=30)
        start_again()
        greeter_up()
        assert "NEW_AUTHTOK_REQD" in journal(), journal()
        assert int(greetd("NRestarts")) == 1, greetd("NRestarts")
        assert not session_of("kara")
        machine.screenshot("02-login-screen-for-an-account-without-a-password")

    with subtest("the chosen account can log in: its session, and no login screen"):
        machine.succeed("echo kara:kara | chpasswd")
        start_again()
        machine.wait_until_succeeds("pgrep -u kara -x aizome-shell", timeout=180)
        machine.fail("pgrep -u greeter -x aizome-greeter")
        assert int(greetd("NRestarts")) == 0, greetd("NRestarts")
        assert "session opened for user kara" in journal(), journal()
        machine.screenshot("03-automatic-login")

    with subtest("logging out shows the login screen and logs nobody in again"):
        machine.succeed("loginctl terminate-user kara")
        machine.wait_until_fails("pgrep -u kara -x aizome-shell", timeout=90)
        greeter_up()
        nobody_arrives("kara")
        assert int(greetd("NRestarts")) == 0, greetd("NRestarts")

    with subtest("the greeter dying brings the login screen back and logs nobody in again"):
        # greetd exits when its greeter does; systemd starts it again.
        machine.succeed(f"rm -f {INSPECT}/*.json; pkill -KILL -u greeter -x aizome-greeter")
        machine.wait_until_succeeds("test $(systemctl show greetd.service --property=NRestarts --value) -ge 1", timeout=60)
        greeter_up()
        nobody_arrives("kara")
        machine.screenshot("04-login-screen-after-the-greeter-died")
  '';
}
