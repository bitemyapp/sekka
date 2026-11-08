# Aizome's reclaim budgets (programs.aizome.memoryReserve), evaluated
# without building the systems: each service's drop-in, the sums its slices
# get, since a service is protected only as far as every ancestor is, and the
# login session rule that grants the sum to the person at the machine. The
# aizome-login VM test reads them back from systemd and the kernel.
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
  withoutAizome = system [ "tatami" ] { };
  disabled = system [ "aizome" ] {
    programs.aizome.memoryReserve = lib.mapAttrs (_: lib.mapAttrs (_: _: 0)) defaults;
  };
  empty = system [ "aizome" ] {
    programs.aizome.memoryReserve.session = lib.mkForce { };
    programs.aizome.memoryReserve.system = lib.mkForce { };
  };
  explicitLow =
    value: extra:
    system [ "aizome" ] {
      imports = [ extra ];
      i18n.inputMethod = {
        enable = true;
        type = "ibus";
      };
      # Aizome and logind are packaged units; IBus is defined by a module.
      systemd.user.services.aizome.serviceConfig.MemoryLow = value;
      systemd.user.services.aizome-ibus.serviceConfig.MemoryLow = value;
      systemd.services.systemd-logind.serviceConfig.MemoryLow = value;
    };
  explicit = explicitLow "128M" {
    # A share of RAM, which the sums can't follow: pipewire's budget stays.
    systemd.user.services.pipewire.serviceConfig.MemoryLow = "5%";
  };
  explicitZero = explicitLow 0 { };
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

  defaults = alone.programs.aizome.memoryReserve;
  applied = c: c.programs.aizome.effectiveMemoryReserve;
  sum = units: "${toString (lib.foldl' builtins.add 0 (lib.attrValues units))}M";
  sums = c: {
    session = c.systemd.user.slices.session.sliceConfig.MemoryLow;
    system = c.systemd.slices.system.sliceConfig.MemoryLow;
  };
  dropIns = c: lib.findFirst (p: (p.name or "") == "aizome-memory-reserve") null c.systemd.packages;
  # The slices follow each service's MemoryLow as it applies. Each person's
  # user manager and user-UID.slice get theirs at login, never statically.
  slicesMatch =
    c:
    sums c == {
      session = sum (applied c).session;
      system = sum (applied c).system;
    }
    && !(c.systemd.slices ? "user-")
    && !(c.systemd.services."user@".serviceConfig ? MemoryLow);
  rule = c: c.security.pam.services.login.rules.session.aizome-memory-reserve or null;
  script = c: lib.last (rule c).args;
  # Runs after pam_systemd, which registers the session it reads.
  loginRule =
    c:
    rule c != null
    && (rule c).enable
    && (rule c).control == "optional"
    && (rule c).order > c.security.pam.services.login.rules.session.systemd.order
    && lib.hasInfix (builtins.unsafeDiscardStringContext "pam_exec.so quiet type=open_session ${script c}") (
      lib.last (builtins.split "pam_systemd\\.so" c.security.pam.services.login.text)
    );
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
  explicitMatches =
    c: value:
    lib.hasInfix "MemoryLow=${toString value}\n" c.systemd.user.units."aizome.service".text
    && lib.hasInfix "MemoryLow=${toString value}\n" c.systemd.user.units."aizome-ibus.service".text
    && lib.hasInfix "MemoryLow=${toString value}\n" c.systemd.units."systemd-logind.service".text
    && slicesMatch c;
  expectExplicit = c: ''
    test ! -e ${dropIns c}/lib/systemd/user/aizome.service.d
    test ! -e ${dropIns c}/lib/systemd/user/aizome-ibus.service.d
    test ! -e ${dropIns c}/lib/systemd/system/systemd-logind.service.d
    expect ${dropIns c} user pipewire-pulse.service 48
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
assert plain.programs.aizome.memoryReserve.session."dbus.service" == 16;
assert plain.programs.aizome.memoryReserve.system."dbus.service" == 32;
assert !(plain.programs.aizome.memoryReserve.session ? "dbus-broker.service");
assert builtins.all slicesMatch [
  alone
  coexist
  plain
  custom
  disabled
  empty
  explicit
  explicitZero
  systemAudio
  systemAudioMinimal
  noAudio
];
# Independent of the module's arithmetic: the sums each case should reach.
assert
  sums alone == {
    session = "1200M";
    system = "200M";
  };
assert
  sums coexist == {
    session = "1296M";
    system = "200M";
  };
assert
  sums plain == {
    session = "1200M";
    system = "200M";
  };
assert
  sums custom == {
    session = "1664M";
    system = "72M";
  };
# Explicit settings move the sums: 128 MiB for the compositor and IBus, and
# for logind; pipewire's 5% can't be followed, so its 32 MiB stays.
assert
  sums explicit == {
    session = "432M";
    system = "312M";
  };
assert
  sums explicitZero == {
    session = "176M";
    system = "184M";
  };
assert
  sums disabled == {
    session = "0M";
    system = "0M";
  };
assert
  sums empty == {
    session = "0M";
    system = "0M";
  };
assert lib.any (lib.hasInfix "pipewire.service (MemoryLow=5%)") explicit.warnings;
assert !(lib.any (lib.hasInfix "programs.aizome.memoryReserve") alone.warnings);
assert alone.systemd.slices.user.sliceConfig.MemoryLow == "1200M";
assert explicitZero.systemd.slices.user.sliceConfig.MemoryLow == "176M";
assert custom.systemd.slices.user.sliceConfig.MemoryLow == "4G";
assert !((applied custom).session ? "pipewire-pulse.service");
assert !((applied custom).system ? "systemd-oomd.service");
assert explicitMatches explicit "128M";
assert explicitMatches explicitZero 0;
# The login screens' session rules include login's, where the rule lives.
assert lib.hasInfix "session include login" alone.security.pam.services.greetd.text;
assert lib.hasInfix "session include login" coexist.security.pam.services.gdm-password.text;
assert builtins.all loginRule [
  alone
  coexist
  plain
  custom
  explicit
];
assert builtins.all (c: rule c == null) [
  disabled
  empty
  withoutAizome
];
assert !(withoutAizome.systemd.slices.user.sliceConfig ? MemoryLow);
assert !(withoutAizome.systemd.slices.system.sliceConfig ? MemoryLow);
assert dropIns withoutAizome == null;
# The same 112 MiB of audio budgets moves to the active system manager.
assert builtins.all (
  unit:
  !((applied systemAudio).session ? ${unit})
  && (applied systemAudio).system.${unit} == defaults.session.${unit}
  && !(defaults.system ? ${unit})
  && systemAudio.systemd.units.${unit}.enable
  && !systemAudio.systemd.user.units.${unit}.enable
) audioUnits;
assert
  sums systemAudio == {
    session = "1088M";
    system = "312M";
  };
assert (applied systemAudioMinimal).system."pipewire.service" == 32;
assert !((applied systemAudioMinimal).system ? "pipewire-pulse.service");
assert !((applied systemAudioMinimal).system ? "wireplumber.service");
assert (sums systemAudioMinimal).system == "232M";
assert builtins.all (
  unit: !((applied noAudio).session ? ${unit}) && !((applied noAudio).system ? ${unit})
) audioUnits;
# The cases whose units differ: drop-ins, explicit settings, system-wide
# audio and dbus-daemon. The rest differ only in the values checked above
# (and building `explicit` would print its intended warning).
assert builtins.all (c: c.system.build.toplevel.drvPath != "") [
  alone
  plain
  explicitZero
  systemAudio
];
pkgs.runCommand "aizome-memory-policy" { } ''
  expect() {
    file="$1/lib/systemd/$2/$3.d/40-aizome-memory.conf"
    printf '[Service]\nMemoryLow=%sM\n' "$4" | cmp - "$file"
  }
  ${expectDropIns alone}
  ${expectDropIns coexist}
  ${expectDropIns plain}
  ${expectDropIns custom}
  ${expectDropIns disabled}
  ${expectDropIns systemAudio}
  ${expectDropIns systemAudioMinimal}
  ${expectDropIns noAudio}
  ${expectExplicit explicit}
  ${expectExplicit explicitZero}
  test ! -e ${dropIns explicit}/lib/systemd/user/pipewire.service.d
  expect ${dropIns explicitZero} user pipewire.service 32
  # Building the package must succeed even when no drop-ins are emitted.
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
  # The login rule grants the session sum it was built with, and the unit
  # bound to the session takes it back.
  grep -qF 'reserve "$uid" 1200M' ${script alone}
  grep -qF 'reserve "$uid" 1296M' ${script coexist}
  grep -qF 'reserve "$2" 0' ${script alone}
  grep -qF -- '--property=BindsTo="$scope"' ${script alone}
  echo 'Budgets, sums, explicit overrides, zeroed and empty packages, the login rule and user/system audio drop-ins' > "$out"
''
