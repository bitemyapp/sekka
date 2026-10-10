# USB HID wake must reach the host through every hub in the input's path.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  wake = pkgs.writeScript "sekka-usb-wake" ''
    #!${pkgs.python3}/bin/python3
    ${builtins.readFile ./usb-wake.py}
  '';
in
{
  options.calamares.usbWake.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Allow USB keyboards and mice to wake the system through their parent hubs and controllers.";
  };
  config = lib.mkIf config.calamares.usbWake.enable {
    services.udev.extraRules = ''
      ACTION=="add|change", SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_KEYBOARD}=="1", RUN+="${wake} /sys%p"
      ACTION=="add|change", SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_MOUSE}=="1", RUN+="${wake} /sys%p"
    '';
    # Apply to already-connected devices on activation/boot and just before
    # suspend. Hotplug and driver re-enumeration are handled by the rules.
    systemd.services.sekka-usb-wake = {
      description = "Enable wake paths for USB keyboards and mice";
      wantedBy = [
        "multi-user.target"
        "sleep.target"
      ];
      before = [ "sleep.target" ];
      after = [ "systemd-udevd.service" ];
      serviceConfig.Type = "oneshot";
      script = ''
        ${pkgs.systemd}/bin/udevadm trigger --subsystem-match=input --action=change
        ${pkgs.systemd}/bin/udevadm settle --timeout=10
      '';
    };
  };
}
