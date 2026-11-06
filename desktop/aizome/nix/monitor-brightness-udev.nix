# What lets Settings, the menu bar and the brightness keys set external
# monitors' brightness without root (aizome-sys's `monitors`): udev tags
# two kinds of device `uaccess`, so systemd gives whoever is at the
# computer, the active session's user, an ACL on them, and takes it away
# when they log out or another session takes the seat. Remote and
# inactive sessions get nothing.
#
# The file sorts before systemd's 73-seat-late.rules, which applies the
# tag: a rule in services.udev.extraRules (99-local.rules) would come too
# late. The module loads i2c-dev, which makes the /dev/i2c-N nodes.
{ writeTextFile }:
writeTextFile {
  name = "aizome-monitor-brightness-udev-rules";
  destination = "/lib/udev/rules.d/70-aizome-monitor-brightness.rules";
  text = ''
    # DDC/CI: the I2C buses of display adapters (PCI class 0x03xxxx), which
    # carry each connector's DDC pins or DisplayPort AUX channel; ddcutil's
    # own rule matches them so. Not the SMBus, a touchpad's or sensors'.
    SUBSYSTEM=="i2c-dev", KERNEL=="i2c-[0-9]*", ATTRS{class}=="0x03*", TAG+="uaccess"

    # Apple Studio Displays (2022 0x1114, XDR 0x1116, 2026 0x1118): the
    # HID interface that takes brightness, USB interface 7 of the first
    # configuration, and none of the display's others.
    SUBSYSTEM=="hidraw", KERNEL=="hidraw*", DEVPATH=="*:1.7/0003:05AC:111[468].*", TAG+="uaccess"
  '';
}
