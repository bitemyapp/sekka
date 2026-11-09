# Aizome's reclaim budgets (programs.aizome.memoryReserve), evaluated
# without building the systems: each service's drop-in, and the sums its
# slices and user managers get, since a service is protected only as far as
# every ancestor is. The aizome-login VM test reads them back from systemd
# and the kernel.
{ pkgs, inputs }:
let
  inherit (pkgs) lib;
  system =
    desktops: extra:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        inputs.calamares.nixosModules.desktops
        {
          calamares.desktops = desktops;
          calamares.defaultDesktop = builtins.head desktops;
          calamares.tuning.enable = true;
          system.stateVersion = "26.11";
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "none";
            fsType = "tmpfs";
          };
        }
        extra
      ];
    }).config;
  alone = system [ "aizome" ] { };
  # GNOME alongside brings IBus, and with it Aizome's IBus frontend.
  coexist = system [ "aizome" "gnome" ] { };
  # Without tuning, and with dbus-daemon rather than nixpkgs' default broker.
  plain = system [ "aizome" ] {
    calamares.tuning.enable = lib.mkForce false;
    services.dbus.implementation = lib.mkForce "dbus";
  };
  custom = system [ "aizome" ] {
    programs.aizome.memoryReserve.session = {
      "aizome.service" = 1536;
      "pipewire-pulse.service" = 0;
    };
    # 0 also overrides the 64 MiB that systemd-oomd's own unit asks for.
    programs.aizome.memoryReserve.system = {
      "systemd-udevd.service" = 0;
      "systemd-oomd.service" = 0;
    };
    # A shared machine raises user.slice alone; the rest follow the budgets.
    systemd.slices.user.sliceConfig.MemoryLow = "4G";
  };
  # An explicit MemoryLow keeps its unit, whether a module defines it (IBus)
  # or a package does (logind), and warns, since the sums count the budgets.
  explicit = system [ "aizome" ] {
    i18n.inputMethod = {
      enable = true;
      type = "ibus";
    };
    systemd.user.services.aizome-ibus.serviceConfig.MemoryLow = "128M";
    systemd.services.systemd-logind.serviceConfig.MemoryLow = "128M";
  };
  # PipeWire's budgets follow its services to the system manager.
  systemAudio = system [ "aizome" ] { services.pipewire.systemWide = true; };
  withoutAizome = system [ "tatami" ] { };

  defaults = alone.programs.aizome.memoryReserve;
  sums = c: {
    session = c.systemd.user.slices.session.sliceConfig.MemoryLow;
    user = c.systemd.slices.user.sliceConfig.MemoryLow;
    userSlice = c.systemd.slices."user-".sliceConfig.MemoryLow;
    manager = c.systemd.services."user@".serviceConfig.MemoryLow;
    system = c.systemd.slices.system.sliceConfig.MemoryLow;
  };
  # Every slice and user manager above the session's services gets its sum.
  slices = session: system: {
    inherit session system;
    user = session;
    userSlice = session;
    manager = session;
  };
  dropIns = c: lib.findFirst (p: (p.name or "") == "aizome-memory-reserve") null c.systemd.packages;
  # Each budget, 0 included, as the drop-in that carries it.
  expectDropIns =
    c:
    lib.concatStrings (
      lib.mapAttrsToList
        (
          manager: units:
          lib.concatStrings (
            lib.mapAttrsToList (unit: mib: ''
              expect ${dropIns c} ${manager} ${lib.escapeShellArg unit} ${toString mib}
            '') units
          )
        )
        {
          inherit (c.programs.aizome.memoryReserve) system;
          user = c.programs.aizome.memoryReserve.session;
        }
    );
in
assert defaults.session."aizome.service" == 1024;
assert defaults.session."pipewire.service" == 32;
assert defaults.session."pipewire-pulse.service" == 48;
assert defaults.session."wireplumber.service" == 32;
assert defaults.session."dbus-broker.service" == 16;
assert !(defaults.session ? "aizome-ibus.service");
assert coexist.programs.aizome.memoryReserve.session."aizome-ibus.service" == 96;
assert defaults.system."systemd-logind.service" == 16;
assert defaults.system."systemd-udevd.service" == 64;
assert defaults.system."dbus-broker.service" == 32;
assert defaults.system."systemd-oomd.service" == 64;
assert defaults.system."rtkit-daemon.service" == 8;
assert plain.programs.aizome.memoryReserve.session."dbus.service" == 16;
assert plain.programs.aizome.memoryReserve.system."dbus.service" == 32;
assert !(plain.programs.aizome.memoryReserve.session ? "dbus-broker.service");
assert sums alone == slices "1200M" "200M";
assert sums coexist == slices "1296M" "200M";
assert sums plain == slices "1200M" "200M";
assert sums custom == slices "1664M" "72M" // { user = "4G"; };
assert sums explicit == slices "1296M" "200M";
assert sums systemAudio == slices "1088M" "312M";
assert lib.hasInfix "MemoryLow=128M\n" explicit.systemd.user.units."aizome-ibus.service".text;
assert lib.hasInfix "MemoryLow=128M\n" explicit.systemd.units."systemd-logind.service".text;
assert lib.any (lib.hasInfix "aizome-ibus.service, systemd-logind.service set") explicit.warnings;
assert !(lib.any (lib.hasInfix "programs.aizome.memoryReserve") alone.warnings);
assert !(withoutAizome.systemd.slices.user.sliceConfig ? MemoryLow);
assert !(withoutAizome.systemd.slices.system.sliceConfig ? MemoryLow);
assert dropIns withoutAizome == null;
pkgs.runCommand "aizome-memory-policy" { } ''
  expect() {
    file="$1/lib/systemd/$2/$3.d/40-aizome-memory.conf"
    printf '[Service]\nMemoryLow=%sM\n' "$4" | cmp - "$file"
  }
  ${expectDropIns alone}
  ${expectDropIns coexist}
  ${expectDropIns plain}
  ${expectDropIns custom}
  ${expectDropIns systemAudio}
  test ! -e ${dropIns explicit}/lib/systemd/user/aizome-ibus.service.d
  test ! -e ${dropIns explicit}/lib/systemd/system/systemd-logind.service.d
  expect ${dropIns explicit} user aizome.service 1024
  for unit in pipewire.service pipewire-pulse.service wireplumber.service; do
    test ! -e ${dropIns alone}/lib/systemd/system/$unit.d
    test ! -e ${dropIns systemAudio}/lib/systemd/user/$unit.d
  done
  echo 'Budgets, sums, drop-ins, explicit settings and user/system audio' > "$out"
''
