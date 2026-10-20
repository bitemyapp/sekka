# Exercise native ARM execution in an untrusted Nix build on an x86 guest.
# binfmt is registered only in the disposable VM, never on the builder host.
{ pkgs }:
let
  armPkgs = import pkgs.path { system = "aarch64-linux"; };
  probe = pkgs.writeText "arm-smoke.nix" ''
    derivation {
      name = "native-arm-sandbox-probe";
      system = "aarch64-linux";
      builder = "${armPkgs.bash}/bin/bash";
      args = [ "--noprofile" "--norc" "-c"
        "test \"$(${armPkgs.coreutils}/bin/uname -m)\" = aarch64 && ${armPkgs.coreutils}/bin/printf native-arm > $out"
      ];
    }
  '';
in
pkgs.testers.runNixOSTest {
  name = "native-arm-emulation";
  nodes.machine = {
    virtualisation.memorySize = 2048;
    boot.binfmt.emulatedSystems = [ "aarch64-linux" ];
    boot.binfmt.preferStaticEmulators = true;
    nix.settings.sandbox = true;
    users.users.builder = {
      isNormalUser = true;
    };
    environment.etc."arm-smoke.nix".source = probe;
  };
  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("grep '^flags: .*F' /proc/sys/fs/binfmt_misc/aarch64-linux")
    machine.succeed("su - builder -c 'nix-build /etc/arm-smoke.nix --no-out-link > /tmp/arm-output'")
    machine.succeed("test \"$(cat \"$(cat /tmp/arm-output)\")\" = native-arm")
  '';
}
