# Kakehashi's helper writing a USB stick in a disposable VM: the stick is
# plugged in while the VM runs, as a person would, and the helper writes an
# image from a plan made the way the window makes one. The stick's bytes are
# then compared from outside the VM. The helper must also refuse the VM's
# own disk, and a drive that is no longer the one in the plan.
#
# The stopped and the real write go as the window's do: an ordinary user
# starts the helper through pkexec, with the plan and progress file in a
# private directory of theirs. A polkit rule stands in for the password.
{
  pkgs,
  kakehashi,
}:
pkgs.testers.runNixOSTest {
  name = "kakehashi";
  nodes.machine = {
    virtualisation.memorySize = 1024;
    virtualisation.qemu.options = [ "-device qemu-xhci,id=xhci" ];
    environment.systemPackages = [ kakehashi ];
    users.users.alice.isNormalUser = true;
    security.polkit.enable = true;
    security.polkit.enablePkexecWrapper = true;
    security.polkit.extraConfig = ''
      polkit.addRule(function (action, subject) {
        if (action.id == "org.freedesktop.policykit.exec" && subject.user == "alice") {
          return polkit.Result.YES;
        }
      });
    '';
  };
  testScript = ''
    import hashlib, json, os, shlex

    MiB = 1024 * 1024
    size = 256 * MiB
    stick = os.path.join(os.environ.get("TMPDIR", "/tmp"), "stick.img")
    with open(stick, "wb") as f:
        f.truncate(size)
        # What a stale backup partition table would leave at the end.
        f.seek(size - 4096)
        f.write(b"\xaa" * 4096)
    with open(stick, "rb") as f:
        blank = hashlib.sha256(f.read()).hexdigest()

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("mkdir /images && head -c ${toString (64 * 1024 * 1024)} /dev/urandom > /images/test.iso")
    image_sha = machine.succeed("sha256sum /images/test.iso | cut -d' ' -f1").strip()

    # Plug the stick in.
    machine.send_monitor_command(f"drive_add 0 if=none,id=stick,format=raw,file={stick}")
    machine.send_monitor_command("device_add usb-storage,bus=xhci.0,drive=stick,removable=on,serial=KAKEHASHI1")
    machine.wait_until_succeeds("lsblk -dno TRAN /dev/sda | grep -qx usb")

    drives = json.loads(machine.succeed("kakehashi-helper --drives"))
    usb = next(d for d in drives if d["name"] == "sda")
    system = next(d for d in drives if d["system"])
    assert usb["usb_or_removable"] and not usb["system"] and usb["serial"] == "KAKEHASHI1", usb

    def plan(drive):
        return json.dumps({"image": "/images/test.iso", "sha256": image_sha, "drive": drive})

    def write(drive, name):
        """The helper as root, as a test of its own refusals."""
        machine.succeed(f"cat > /tmp/{name}.json <<'EOF'\n{plan(drive)}\nEOF")
        machine.succeed(f"rm -f /tmp/{name}.progress && touch /tmp/{name}.progress")
        status, _ = machine.execute(f"kakehashi-helper --plan /tmp/{name}.json --progress /tmp/{name}.progress")
        lines = [json.loads(l) for l in machine.succeed(f"cat /tmp/{name}.progress").splitlines()]
        return status, lines

    def as_alice(command):
        return machine.execute(f"su - alice -c {shlex.quote(command)}")

    def write_as_alice(drive, cancel=False):
        """The helper as the window starts it: pkexec, from alice's own files."""
        status, _ = as_alice(
            "set -e; rm -rf /tmp/kakehashi-job; mkdir -m 700 /tmp/kakehashi-job; cd /tmp/kakehashi-job; "
            f"printf '%s\\n' {shlex.quote(plan(drive))} > plan.json; : > progress"
            + ("; : > progress.cancel" if cancel else "")
        )
        assert status == 0
        helper = machine.succeed("readlink -f /run/current-system/sw/bin/kakehashi-helper").strip()
        # polkit judges pkexec's parent. In the window that is Kakehashi, as
        # alice; here it must be alice's shell, not su (root), so the shell
        # must not exec pkexec in its place.
        status, output = as_alice(
            f"/run/wrappers/bin/pkexec {helper} --plan /tmp/kakehashi-job/plan.json --progress /tmp/kakehashi-job/progress 2>&1; exit $?"
        )
        lines = [json.loads(l) for l in machine.succeed("cat /tmp/kakehashi-job/progress").splitlines()]
        return status, output, lines

    with subtest("refuses the VM's own disk"):
        status, lines = write(system, "system")
        assert status != 0 and lines[-1]["stage"] == "failed" and not lines[-1]["touched"], lines

    with subtest("refuses a drive that is not the one in the plan"):
        status, lines = write(dict(usb, serial="SOMEONE-ELSE"), "other")
        assert status != 0 and "no longer the drive you picked" in lines[-1]["message"], lines
        assert not lines[-1]["touched"], lines

    with subtest("stops through pkexec when the window asks, before writing"):
        status, output, lines = write_as_alice(usb, cancel=True)
        assert status == 1, (status, output, lines)
        assert lines[-1] == {"stage": "failed", "message": "Stopped before the drive was finished.", "touched": False}, lines
        with open(stick, "rb") as f:
            assert hashlib.sha256(f.read()).hexdigest() == blank, "a stopped write changed the stick"

    with subtest("writes the stick through pkexec, reads it back and clears its end"):
        status, output, lines = write_as_alice(usb)
        assert status == 0 and lines[-1]["stage"] == "finished", (status, output, lines)
        stages = [l["stage"] for l in lines]
        assert "writing" in stages and "verifying" in stages, stages
        with open(stick, "rb") as f:
            written = f.read(64 * MiB)
            f.seek(size - MiB)
            tail = f.read()
        assert hashlib.sha256(written).hexdigest() == image_sha, "the stick differs from the image"
        assert tail == bytes(MiB), "the end of the stick was not cleared"
  '';
}
