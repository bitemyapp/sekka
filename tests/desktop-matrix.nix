# Evaluate actual configuration.nix strings emitted by the pinned Rust backend.
# Usage inside the rootless builder: nix eval --impure --json --file
# tests/desktop-matrix.nix --apply
# 'f: f { configurations = "/workspace/.work/desktop-configurations.json"; caseName = "plasma-xfce"; }'
{ configurations, caseName }:
let
  flake = builtins.getFlake "git+file:///workspace";
  # The driver writes and fsyncs these generated modules. Avoid adding ephemeral
  # test source files to the persistent Nix store just to evaluate them.
  module = builtins.toPath ("/workspace/.work/desktop-modules/" + caseName + ".nix");
  system = flake.inputs.nixpkgs.lib.nixosSystem {
    system = "x86_64-linux";
    specialArgs = {
      applicationPkgs = import flake.inputs.applications {
        system = "x86_64-linux";
        config.allowUnfree = true;
      };
      aiPackages = flake.inputs.ai-apps.packages.x86_64-linux;
      ompPackage = flake.inputs.omp.packages.x86_64-linux.omp;
    };
    modules = [
      flake.inputs.determinate.nixosModules.default
      # The modules installed systems import from their `sekka` input.
      flake.nixosModules.default
      module
      {
        fileSystems."/" = {
          # Simulate hardware detection choosing aliases from before a reformat.
          device = "/dev/disk/by-uuid/00000000-0000-0000-0000-000000000000";
          fsType = "ext4";
        };
        fileSystems."/boot" = {
          device = "/dev/disk/by-uuid/0000-0000";
          fsType = "vfat";
        };
      }
    ];
  };
in
assert system.config.networking.networkmanager.enable;
assert system.config.networking.wireless.enable;
assert system.config.networking.wireless.dbusControlled;
assert system.config.hardware.enableRedistributableFirmware;
assert system.config.nixpkgs.config.allowUnfree;
assert
  system.config.fileSystems."/".device == "/dev/disk/by-uuid/11111111-2222-4333-8444-555555555555";
assert system.config.fileSystems."/boot".device == "/dev/disk/by-uuid/A1B2-C3D4";
# RAM-sized swap partition with zswap and hibernation, and CachyOS-inspired tuning.
assert
  map (swap: swap.device) system.config.swapDevices == [
    "/dev/disk/by-uuid/66666666-7777-4888-9999-aaaaaaaaaaaa"
  ];
assert system.config.boot.resumeDevice == "/dev/disk/by-uuid/66666666-7777-4888-9999-aaaaaaaaaaaa";
assert builtins.elem "zswap.enabled=1" system.config.boot.kernelParams;
assert system.config.boot.kernel.sysctl."vm.swappiness" == 100;
assert system.config.boot.kernel.sysctl."vm.dirty_bytes" == 268435456;
assert system.config.services.ananicy.enable;
# Processes must stay in their systemd units (VT switching, logout cleanup).
assert !system.config.services.ananicy.settings.apply_cgroup;
assert !system.config.services.ananicy.settings.cgroup_load;
assert !system.config.services.ananicy.settings.cgroup_realtime_workaround;
# Xfce's polkit-gnome agent must not start in the other desktops.
assert
  !builtins.elem "xfce" system.config.calamares.desktops
  ||
    flake.inputs.nixpkgs.lib.hasInfix "OnlyShowIn=XFCE;"
      system.config.environment.etc."xdg/autostart/polkit-gnome-authentication-agent-1.desktop".text;
# A Wayland login screen: GDM whenever GNOME is installed (GNOME locks the
# screen only under GDM), Plasma Login Manager otherwise; never SDDM (X11
# greeter: fixed GPU layout; Wayland greeter: sddm#1443).
assert !system.config.services.displayManager.sddm.enable;
assert
  system.config.services.displayManager.gdm.enable
  == builtins.elem "gnome" system.config.calamares.desktops;
assert
  system.config.services.displayManager.plasma-login-manager.enable
  == !system.config.services.displayManager.gdm.enable;
# Plasma Login Manager starts X11 sessions with NixOS's X server arguments,
# without which they have no input driver.
assert
  system.config.services.displayManager.plasma-login-manager.enable
  -> flake.inputs.nixpkgs.lib.hasInfix "plasmalogin-xserver" system.config.services.displayManager.plasma-login-manager.settings.X11.ServerPath;
# GNOME's IBus autostart must skip the Hyprland sessions.
assert
  !(
    builtins.elem "gnome" system.config.calamares.desktops
    && (
      builtins.elem "hyprland" system.config.calamares.desktops
      || builtins.elem "tatami" system.config.calamares.desktops
    )
  )
  ||
    flake.inputs.nixpkgs.lib.hasInfix "NotShowIn=GNOME;KDE;Hyprland;"
      system.config.environment.etc."xdg/autostart/ibus-daemon.desktop".text;
# LXQt must not ask for a window manager when other desktops bring theirs.
assert
  !builtins.elem "lxqt" system.config.calamares.desktops
  ||
    flake.inputs.nixpkgs.lib.hasInfix "window_manager=openbox"
      system.config.environment.etc."xdg/lxqt/session.conf".text;
# Nor put its shortcuts in ~/Desktop, where the desktops with icons show them.
assert
  system.config.environment.etc ? "xdg/pcmanfm-qt/lxqt/settings.conf" == (
    builtins.elem "lxqt" system.config.calamares.desktops
    && builtins.any (d: builtins.elem d system.config.calamares.desktops) [
      "plasma"
      "xfce"
      "mate"
      "cinnamon"
    ]
  );
# Tatami has its own Wi-Fi list; NetworkManager's applet stays out of it.
assert
  builtins.elem "tatami" system.config.calamares.desktops
  ->
    flake.inputs.nixpkgs.lib.hasSuffix "nm-applet.desktop"
      system.config.environment.etc."xdg/autostart/nm-applet.desktop".source.name;
# The login screen lists exactly the chosen desktops, one entry each: Plasma
# Login Manager reads only the chosen sessions, and GDM's greeter, which finds
# every installed session, has each one not chosen hidden from it.
assert
  system.config.services.displayManager.plasma-login-manager.enable
  -> flake.inputs.nixpkgs.lib.hasInfix "login-sessions" system.config.systemd.services.plasmalogin.environment.XDG_DATA_DIRS;
assert
  system.config.services.displayManager.gdm.enable
  -> builtins.all (
    name:
    builtins.elem name (
      map (
        desktop:
        {
          plasma = "plasma";
          gnome = "gnome";
          xfce = "xfce";
          hyprland = "hyprland-uwsm";
          tatami = "tatami";
        }
        .${desktop}
      ) system.config.calamares.desktops
    )
    ||
      flake.inputs.nixpkgs.lib.hasInfix "Hidden=true"
        system.config.environment.etc."X11/sessions/${name}.desktop".text
  ) system.config.services.displayManager.sessionData.sessionNames;
# One keyring: GNOME Keyring everywhere, also behind Plasma's KWallet API.
assert system.config.services.gnome.gnome-keyring.enable;
assert
  builtins.elem "plasma" system.config.calamares.desktops
  -> (
    !system.config.security.pam.services.login.kwallet.enable
    &&
      flake.inputs.nixpkgs.lib.hasInfix "[KSecretD]\nEnabled=false"
        system.config.environment.etc."xdg/kwalletrc".text
  );
# Every desktop offers Tatami's wallpapers, and each starts with its own.
assert
  builtins.elem system.config.programs.tatami.wallpapers system.config.environment.systemPackages
  && builtins.elem "/share/backgrounds" system.config.environment.pathsToLink;
assert
  builtins.elem "plasma" system.config.calamares.desktops
  ->
    flake.inputs.nixpkgs.lib.hasInfix "LookAndFeelPackage=org.nixos.breeze-ocean.desktop"
      system.config.environment.etc."xdg/kdeglobals".text;
assert
  builtins.elem "gnome" system.config.calamares.desktops
  -> flake.inputs.nixpkgs.lib.hasInfix "tatami/foggy-forest.jpg" system.config.services.desktopManager.gnome.extraGSettingsOverrides;
assert
  builtins.elem "xfce" system.config.calamares.desktops
  -> flake.inputs.nixpkgs.lib.any (
    package:
    (package.pname or "") == "xfdesktop"
    && flake.inputs.nixpkgs.lib.hasInfix "tatami/looks-flat.jpg" (
      toString (package.configureFlags or [ ])
    )
  ) system.config.environment.systemPackages;
# Yukimi comes with every desktop; its package carries its polkit action.
assert system.config.programs.yukimi.enable;
assert builtins.elem system.config.programs.yukimi.package system.config.environment.systemPackages;
assert system.config.security.polkit.enable;
# With several desktops, each login starts from the user manager's own
# environment instead of the one the previous desktop exported.
assert
  (builtins.length system.config.calamares.desktops > 1) == (
    flake.inputs.nixpkgs.lib.hasInfix "calamares-session-env reset" (
      system.config.security.pam.services.${
        if system.config.services.displayManager.gdm.enable then "gdm-password" else "plasmalogin"
      }.text or ""
    )
    && system.config.systemd.user.services ? calamares-session-env
  );
assert builtins.elem system.config.services.displayManager.defaultSession
  system.config.services.displayManager.sessionData.sessionNames;
{
  derivation = system.config.system.build.toplevel.drvPath;
  defaultSession = system.config.services.displayManager.defaultSession;
  sessions = system.config.services.displayManager.sessionData.sessionNames;
  zone = system.config.time.timeZone;
  supplicantEnabled = system.config.networking.wireless.enable;
  redistributableFirmware = system.config.hardware.enableRedistributableFirmware;
  allowUnfree = system.config.nixpkgs.config.allowUnfree;
  rootDevice = system.config.fileSystems."/".device;
  bootDevice = system.config.fileSystems."/boot".device;
}
