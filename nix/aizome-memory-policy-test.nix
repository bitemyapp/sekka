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
  disabled = system [ "aizome" ] {
    programs.aizome.memoryReserve = lib.mapAttrs (_: lib.mapAttrs (_: _: 0)) defaults;
  };
  empty = system [ "aizome" ] {
    programs.aizome.memoryReserve.session = lib.mkForce { };
    programs.aizome.memoryReserve.system = lib.mkForce { };
  };
  explicitLow =
    value:
    system [ "aizome" ] {
      i18n.inputMethod = {
        enable = true;
        type = "ibus";
      };
      # Aizome and logind are packaged units; IBus is defined by a module.
      systemd.user.services.aizome.serviceConfig.MemoryLow = value;
      systemd.user.services.aizome-ibus.serviceConfig.MemoryLow = value;
      systemd.services.systemd-logind.serviceConfig.MemoryLow = value;
    };
  explicit = explicitLow "128M";
  explicitZero = explicitLow 0;
  systemAudio = system [ "aizome" ] {
    services.pipewire.systemWide = true;
    # A null setting is omitted by NixOS, so it must not suppress a budget.
    systemd.services.pipewire.serviceConfig.MemoryLow = null;
    systemd.user.services.aizome.serviceConfig.MemoryLow = null;
  };
  systemAudioMinimal = system [ "aizome" ] {
    services.pipewire = {
      systemWide = true;
      pulse.enable = lib.mkForce false;
      wireplumber.enable = lib.mkForce false;
    };
  };
  noAudio = system [ "aizome" ] {
    services.pipewire = {
      enable = lib.mkForce false;
      systemWide = true;
    };
  };
  audioUnits = [
    "pipewire.service"
    "pipewire-pulse.service"
    "wireplumber.service"
  ];

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
  explicitMatches =
    c: value:
    lib.hasInfix "MemoryLow=${toString value}\n" c.systemd.user.units."aizome.service".text
    && lib.hasInfix "MemoryLow=${toString value}\n" c.systemd.user.units."aizome-ibus.service".text
    && lib.hasInfix "MemoryLow=${toString value}\n" c.systemd.units."systemd-logind.service".text
    # Direct service overrides leave all ancestor sums at the reserve values.
    && c.systemd.slices.user.sliceConfig.MemoryLow == coexist.systemd.slices.user.sliceConfig.MemoryLow
    && ancestorsMatch c;
  expectExplicit = c: ''
    test ! -e ${dropIns c}/lib/systemd/user/aizome.service.d
    test ! -e ${dropIns c}/lib/systemd/user/aizome-ibus.service.d
    test ! -e ${dropIns c}/lib/systemd/system/systemd-logind.service.d
    expect ${dropIns c} user pipewire.service 32
    expect ${dropIns c} system systemd-udevd.service 64
  '';
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
  disabled
  empty
  systemAudio
  systemAudioMinimal
  noAudio
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
assert disabled.systemd.slices.user.sliceConfig.MemoryLow == "0M";
assert disabled.systemd.slices.system.sliceConfig.MemoryLow == "0M";
assert empty.systemd.slices.user.sliceConfig.MemoryLow == "0M";
assert empty.systemd.slices.system.sliceConfig.MemoryLow == "0M";
assert explicitMatches explicit "128M";
assert explicitMatches explicitZero 0;
# The same 112 MiB of audio budgets moves to the active system manager.
assert builtins.all (
  unit:
  !((reserve systemAudio).session ? ${unit})
  && (reserve systemAudio).system.${unit} == defaults.session.${unit}
  && !(defaults.system ? ${unit})
  && systemAudio.systemd.units.${unit}.enable
  && !systemAudio.systemd.user.units.${unit}.enable
) audioUnits;
assert systemAudio.systemd.slices.user.sliceConfig.MemoryLow == "1088M";
assert systemAudio.systemd.slices.system.sliceConfig.MemoryLow == "312M";
assert (reserve systemAudioMinimal).system."pipewire.service" == 32;
assert !((reserve systemAudioMinimal).system ? "pipewire-pulse.service");
assert !((reserve systemAudioMinimal).system ? "wireplumber.service");
assert systemAudioMinimal.systemd.slices.system.sliceConfig.MemoryLow == "232M";
assert builtins.all (
  unit: !((reserve noAudio).session ? ${unit}) && !((reserve noAudio).system ? ${unit})
) audioUnits;
assert builtins.all (c: c.system.build.toplevel.drvPath != "") [
  alone
  coexist
  custom
  disabled
  explicit
  explicitZero
  systemAudio
  systemAudioMinimal
];
pkgs.runCommand "aizome-memory-policy" { } ''
  expect() {
    file="$1/lib/systemd/$2/$3.d/aizome-memory.conf"
    printf '[Service]\nMemoryLow=%sM\n' "$4" | cmp - "$file"
  }
  ${expectDropIns alone}
  ${expectDropIns coexist}
  ${expectDropIns custom}
  ${expectDropIns systemAudio}
  ${expectDropIns systemAudioMinimal}
  ${expectDropIns noAudio}
  ${expectExplicit explicit}
  ${expectExplicit explicitZero}
  # Building these packages must succeed even when no drop-ins are emitted.
  test -d ${dropIns disabled}
  test -z "$(find ${dropIns disabled} -mindepth 1 -print -quit)"
  test -d ${dropIns empty}
  test -z "$(find ${dropIns empty} -mindepth 1 -print -quit)"
  for unit in pipewire.service pipewire-pulse.service wireplumber.service; do
    test ! -e ${dropIns alone}/lib/systemd/system/$unit.d
    test ! -e ${dropIns systemAudio}/lib/systemd/user/$unit.d
    test ! -e ${dropIns noAudio}/lib/systemd/user/$unit.d
    test ! -e ${dropIns noAudio}/lib/systemd/system/$unit.d
  done
  test ! -e ${dropIns systemAudioMinimal}/lib/systemd/system/pipewire-pulse.service.d
  test ! -e ${dropIns systemAudioMinimal}/lib/systemd/system/wireplumber.service.d
  test ! -e ${dropIns custom}/lib/systemd/user/pipewire-pulse.service.d
  test ! -e ${dropIns custom}/lib/systemd/system/systemd-udevd.service.d
  echo 'Budgets, ancestor sums, empty packages, explicit overrides and user/system audio drop-ins' > "$out"
''
