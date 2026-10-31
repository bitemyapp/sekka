# SPDX-License-Identifier: GPL-3.0-or-later
# Sekka's boot splash: Plymouth with the Sekka theme, and a quiet console
# under it, from the start of the initrd to the login screen, and again
# when shutting down or restarting. Esc shows the messages, as it does in
# every Plymouth theme.
{
  config,
  options,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.bootSplash;
  font = "${pkgs.ibm-plex}/share/fonts/truetype/IBMPlexMono-Regular.ttf";
  # Where the initrd's fontconfig finds the font (NixOS's plymouth module
  # puts it there). Plymouth keeps that configuration after the switch to
  # the installed system, so the same file has to be there too, or text
  # drawn after it comes out as empty boxes.
  fonts = pkgs.runCommand "sekka-plymouth-fonts" { } ''
    mkdir $out
    ln -s ${font} $out/${baseNameOf font}
  '';
  fontsConf = pkgs.writeText "plymouth-fonts.conf" ''
    <?xml version="1.0"?>
    <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
    <fontconfig>
      <dir>/etc/plymouth/fonts</dir>
    </fontconfig>
  '';
  # NixOS's Plymouth, with fixes for what happens when the GPU's driver
  # takes over from the firmware's framebuffer and when a passphrase is
  # mistyped (DEVELOPERS.md, "Boot splash"):
  # - keyboard-removal: Plymouth freed a display's keyboard without telling
  #   the splash, which went on reading it (Caps Lock) and could crash.
  # - console-owner: the keyboard could end up read by no renderer, so
  #   nothing typed after the takeover arrived, Esc included.
  # - abandoned-prompts: a question stayed up after whoever asked had gone,
  #   such as the second disk's after it was unlocked another way.
  plymouth =
    (pkgs.plymouth.override {
      systemd = config.boot.initrd.systemd.package;
    }).overrideAttrs
      (old: {
        patches = (old.patches or [ ]) ++ [
          ./plymouth/keyboard-removal.patch
          ./plymouth/console-owner.patch
          ./plymouth/abandoned-prompts.patch
        ];
      });
in
{
  options.calamares.bootSplash = {
    enable = lib.mkEnableOption "Sekka's boot splash (Plymouth)";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "Sekka's theme, from rust/system/boot-splash";
      description = "The Plymouth theme named `sekka`.";
    };
  };
  config = lib.mkMerge [
    # Yukimi may turn the splash on or off after installing, as a system
    # installed before Genkan turned it on needs to (Settei's General →
    # Startup asks Yukimi). After Sekka's desktops and login screen.
    (lib.optionalAttrs (options.programs ? yukimi) {
      programs.yukimi.choices = lib.mkAfter [
        {
          setting = "calamares.bootSplash.enable";
          title = "Boot splash";
          boolean = true;
          value = lib.boolToString cfg.enable;
          options = [
            {
              id = "true";
              title = "On";
              detail = "Sekka's crest while the computer starts and stops. Esc shows the messages.";
            }
            {
              id = "false";
              title = "Off";
              detail = "The computer's messages as it starts and stops.";
            }
          ];
        }
      ];
    })
    (lib.mkIf cfg.enable {
      boot.plymouth = {
        enable = true;
        package = plymouth;
        theme = "sekka";
        themePackages = [ cfg.package ];
        # Everything the splash always shows is drawn ahead of time; this is
        # for what it can't know, such as a disk's passphrase prompt.
        inherit font;
        # The theme chooses its own whole-number scale and has its pieces
        # drawn for it; Plymouth's own scaling would stretch them.
        #
        # An encrypted disk asks for its passphrase in the initrd, which has
        # no GPU driver on NixOS. Plymouth waits 8 seconds (DeviceTimeout) for
        # one before it draws on the firmware's framebuffer, and the screen
        # stays blank until then. UseSimpledrm=2 draws there at once, even at
        # 640x480, 800x600 or 1024x768 (GRUB leaves 1024x768 on BIOS), which
        # 1 would still pass over as modes a native driver will replace. The
        # splash moves to the GPU when its driver loads after the disk is
        # unlocked.
        extraConfig = ''
          DeviceScale=1
        ''
        + lib.optionalString (config.boot.initrd.luks.devices != { }) ''
          UseSimpledrm=2
        '';
      };
      environment.etc."plymouth/fonts".source = fonts;
      # Plymouth started by the installed system, to shut down or restart,
      # reads the same font rather than the system's.
      systemd.services =
        lib.genAttrs
          [
            "plymouth-start"
            "plymouth-halt"
            "plymouth-kexec"
            "plymouth-poweroff"
            "plymouth-reboot"
          ]
          (_: {
            environment.FONTCONFIG_FILE = fontsConf;
          });
      # Kernel and udev messages only for errors, so the splash isn't drawn
      # over text, and the initrd says nothing on the way.
      boot.consoleLogLevel = lib.mkDefault 3;
      boot.kernelParams = [
        "quiet"
        "udev.log_level=3"
        "rd.udev.log_level=3"
      ];
    })
  ];
}
