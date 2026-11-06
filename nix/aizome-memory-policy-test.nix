# Aizome's reclaim budgets (programs.aizome.memoryReserve), evaluated
# without building the systems: each service's drop-in, and the sums its
# slices and the user manager get, since a service is protected only as far
# as every ancestor is. The aizome-login VM test reads them back from systemd
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
  custom = system [ "aizome" ] {
    programs.aizome.memoryReserve.session = {
      "aizome.service" = 1536;
      "pipewire-pulse.service" = 0;
    };
    programs.aizome.memoryReserve.system."systemd-udevd.service" = 0;
  };
  shared = system [ "aizome" ] { systemd.slices.user.sliceConfig.MemoryLow = "4G"; };
  withoutAizome = system [ "tatami" ] { };

  reserve = c: lib.mapAttrs (_: lib.filterAttrs (_: mib: mib > 0)) c.programs.aizome.memoryReserve;
  sum = units: "${toString (lib.foldl' builtins.add 0 (lib.attrValues units))}M";
  dropIns = c: lib.findFirst (p: (p.name or "") == "aizome-memory-reserve") null c.systemd.packages;
  ancestorsMatch =
    c:
    let
      session = sum (reserve c).session;
    in
    c.systemd.slices."user-".sliceConfig.MemoryLow == session
    && c.systemd.services."user@".serviceConfig.MemoryLow == session
    && c.systemd.user.slices.session.sliceConfig.MemoryLow == session
    && c.systemd.slices.system.sliceConfig.MemoryLow == sum (reserve c).system;
  # Each budget, as the drop-in that carries it.
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
          inherit (reserve c) system;
          user = (reserve c).session;
        }
    );
  defaults = alone.programs.aizome.memoryReserve;
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
assert builtins.all ancestorsMatch [
  alone
  coexist
  custom
];
assert alone.systemd.slices.user.sliceConfig.MemoryLow == sum (reserve alone).session;
assert sum (reserve coexist).session == sum ((reserve alone).session // { ibus = 96; });
assert (reserve custom).session."aizome.service" == 1536;
assert !((reserve custom).session ? "pipewire-pulse.service");
assert !((reserve custom).system ? "systemd-udevd.service");
# A shared machine raises user.slice alone; each person's reserve stays.
assert shared.systemd.slices.user.sliceConfig.MemoryLow == "4G";
assert ancestorsMatch shared;
# Drop-ins, which keep the user manager's delegation, on every user's slice.
assert alone.systemd.units."user-.slice".overrideStrategy == "asDropin";
assert alone.systemd.units."user@.service".overrideStrategy == "asDropin";
assert lib.hasInfix "Delegate=" alone.systemd.units."user@.service".text;
assert !(withoutAizome.systemd.slices.user.sliceConfig ? MemoryLow);
assert !(withoutAizome.systemd.slices.system.sliceConfig ? MemoryLow);
assert dropIns withoutAizome == null;
assert builtins.all (c: c.system.build.toplevel.drvPath != "") [
  alone
  coexist
  custom
];
pkgs.runCommand "aizome-memory-policy" { } ''
  expect() {
    file="$1/lib/systemd/$2/$3.d/aizome-memory.conf"
    printf '[Service]\nMemoryLow=%sM\n' "$4" | cmp - "$file"
  }
  ${expectDropIns alone}
  ${expectDropIns coexist}
  ${expectDropIns custom}
  test ! -e ${dropIns custom}/lib/systemd/user/pipewire-pulse.service.d
  test ! -e ${dropIns custom}/lib/systemd/system/systemd-udevd.service.d
  echo 'Budgets, ancestor sums and drop-ins for Aizome alone, with IBus, customized, on a shared machine and without Aizome' > "$out"
''
