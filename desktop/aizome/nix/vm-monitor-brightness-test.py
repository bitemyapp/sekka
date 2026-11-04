machine.wait_for_unit("multi-user.target")

with subtest("i2c-dev is loaded, and the rules come before uaccess is applied"):
    machine.succeed("grep -qw i2c_dev /proc/modules")
    rules = "/etc/udev/rules.d/70-aizome-monitor-brightness.rules"
    machine.succeed(f"test -e {rules}")
    machine.succeed("test -e /etc/udev/rules.d/73-seat-late.rules")
    machine.succeed(f"udevadm verify --resolve-names=never {rules}")

with subtest("the display shows a Studio Display's EDID"):
    machine.succeed("grep -qx connected /sys/class/drm/card*-Virtual-1/status")
    machine.succeed("grep -aq StudioDisplay /sys/class/drm/card*-Virtual-1/edid")

# alice is at the computer: her tty1 session is seat0's active one.
machine.wait_until_succeeds(
    "loginctl show-session $(loginctl show-seat seat0 -p ActiveSession --value) -p Name --value | grep -qx alice"
)

with subtest("an I2C bus that isn't a display adapter's stays root's"):
    machine.succeed("modprobe i2c-stub chip_addr=0x50")
    name = machine.wait_until_succeeds("grep -l 'SMBus stub' /sys/class/i2c-dev/*/name")
    node = name.strip().split("/")[4]
    machine.wait_until_succeeds(f"test -e /dev/{node}")
    machine.fail(f"getfacl -p /dev/{node} | grep -q user:alice")

with subtest("alice gets the Studio Display's interface 7, and only that"):
    machine.succeed("systemctl start fake-studio-display")
    machine.wait_until_succeeds("journalctl -u fake-studio-display | grep -q 'plugged in'")

    def hidraw(interface: int) -> str:
        return machine.wait_until_succeeds(
            # An if, not &&: the shell runs with pipefail, and the loop's
            # status is its last turn's.
            "for h in /sys/class/hidraw/*; do "
            f"if readlink -f $h/device | grep -q ':1.{interface}/0003:05AC:1114'; then basename $h; fi; "
            "done | grep ."
        ).strip()

    seven = hidraw(7)
    five = hidraw(5)
    machine.wait_until_succeeds(f"getfacl -p /dev/{seven} | grep -q '^user:alice:rw'")
    machine.fail(f"getfacl -p /dev/{five} | grep -q user:alice")

with subtest("aizome-brightness reads and sets it as alice"):
    listed = machine.succeed("su - alice -c 'aizome-brightness list'")
    print(listed)
    assert "Virtual-1\tAPP StudioDisplay\t50%\t" in listed, listed
    assert f"/dev/{seven}" in listed, listed
    machine.succeed("su - alice -c 'aizome-brightness set Virtual-1 80'")
    machine.wait_until_succeeds("grep -qx 48080 /run/fake-studio-display/level")
    listed = machine.succeed("su - alice -c 'aizome-brightness list'")
    assert "\t80%\t" in listed, listed
    remembered = machine.succeed("cat /home/alice/.local/state/aizome/monitor-brightness.json")
    print(remembered)
    assert '"model": "StudioDisplay"' in remembered, remembered

with subtest("someone not at the computer can't, and is told why"):
    listed = machine.succeed("su - bob -c 'aizome-brightness list'")
    print(listed)
    assert f"can't open /dev/{seven}" in listed, listed
    machine.fail("su - bob -c 'aizome-brightness set Virtual-1 20'")
    machine.succeed("grep -qx 48080 /run/fake-studio-display/level")
