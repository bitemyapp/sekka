# Sekka's agent skills in a real Aizome login: the module puts them in the
# home at login and after a switch, and an agent's process, with no terminal
# and outside the login session, gets root through Aizome's password dialog
# with the skills' own commands. Yukimi's helper is a fixture that records
# what it is asked, and /etc/nixos a flake of just the attributes the skills
# read, so nothing is built; pkexec, polkit, PAM and the dialog are real.
{
  pkgs,
  aizome,
  agentSkills,
}:
let
  helper = pkgs.writeShellScriptBin "yukimi-helper" ''
    set -eu
    test "$(${pkgs.coreutils}/bin/id -u)" = 0
    echo "$*" >> /var/lib/agent-helper-calls
    echo '{"ok":true,"mode":"switch"}'
  '';
  # Yukimi's own action, naming the fixture.
  policy = pkgs.writeTextDir "share/polkit-1/actions/io.github.bitemyapp.Yukimi.policy" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN" "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
    <policyconfig>
      <action id="io.github.bitemyapp.Yukimi.change-system">
        <description>Change the system's packages, version or storage</description>
        <message>Yukimi needs an administrator's password to change the system</message>
        <defaults><allow_any>auth_admin</allow_any><allow_inactive>auth_admin</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
        <annotate key="org.freedesktop.policykit.exec.path">${helper}/bin/yukimi-helper</annotate>
      </action>
    </policyconfig>
  '';
in
pkgs.testers.runNixOSTest {
  name = "agent-skills";
  globalTimeout = 600;
  nodes.machine =
    { lib, ... }:
    {
      imports = [
        aizome.nixosModules.default
        ../agent-skills/module.nix
      ];
      programs.aizome.enable = true;
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
        password = "test-only";
        extraGroups = [ "wheel" ];
      };
      services.greetd = {
        enable = true;
        settings.initial_session = {
          user = "alice";
          command = "/run/current-system/sw/bin/aizome-session";
        };
        settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
      };
      security.polkit.enablePkexecWrapper = true;
      environment.systemPackages = [
        helper
        policy
      ];
      nix.settings.experimental-features = [
        "nix-command"
        "flakes"
      ];
      # Regular files, as Genkan writes them: a path flake can't follow links
      # out of the store.
      system.activationScripts.etcNixos = ''
        install -D -m 0644 ${pkgs.writeText "flake.nix" ''
          {
            outputs = _: {
              nixosConfigurations.machine = {
                pkgs.hello.drvPath = "/nix/store/00000000000000000000000000000000-hello.drv";
                config.calamares.applications = [ ];
                config.system.build.toplevel = derivation {
                  name = "nixos-system-machine";
                  system = "x86_64-linux";
                  builder = "/bin/sh";
                  args = [ "-c" "echo > $out" ];
                };
              };
            };
          }
        ''} /etc/nixos/flake.nix
        install -D -m 0644 ${pkgs.writeText "yukimi.nix" ''
          { lib, pkgs, ... }:
          let
            packages = [ "ripgrep" ];
            programs = [ ];
          in
          {
            environment.systemPackages = map (name: lib.getAttrFromPath (lib.splitString "." name) pkgs) packages;
            programs = lib.genAttrs programs (_: { enable = true; });
          }
        ''} /etc/nixos/yukimi.nix
      '';
      # In the store, as the Yukimi module writes it: reading it gives a
      # string with store context, which builtins.fromJSON refuses as is.
      environment.etc."yukimi/system.json".text = builtins.toJSON {
        catalogs = [
          {
            file = pkgs.writeText "applications.json" (
              builtins.toJSON [
                {
                  id = "docker";
                  name = "Docker";
                  description = "Containers";
                  category = "Development tools";
                  packages = [ "docker" ];
                }
              ]
            );
            setting = "calamares.applications";
            title = "Apps chosen when installing";
          }
        ];
      };
      # A switch to this must sync everyone's skills again.
      specialisation.switched.configuration.environment.etc."agent-skills-switched".text = "";
      systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/auth-inspect";
      systemd.user.tmpfiles.rules = [ "d %t/auth-inspect 0700 - - -" ];
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        resolution = {
          x = 1280;
          y = 800;
        };
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1280,yres=800"
        ];
      };
    };
  testScript = ''
    shipped = "${agentSkills.skills}/share/sekka/agent-skills"
  ''
  + builtins.readFile ./agent-skills-vm-test.py;
}
