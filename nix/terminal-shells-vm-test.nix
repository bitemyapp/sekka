# Each terminal shell Yukimi offers (fish, bash, zsh and nushell) in Hotaru,
# foot, Ghostty and kitty, in Aizome's session and in Tatami's, on disposable
# VMs built from the modules installed systems import. Component evidence,
# not qualification of an ISO.
#
# In Aizome's session, Yukimi's Desktops page offers the four. Each VM boots
# its desktop's default (fish with Aizome, bash with Tatami), then switches
# to the other shells' specialisations as the rebuild after a Yukimi choice
# would, within the same session. For each shell it opens each terminal as
# its session's launcher does and checks
#
# - the processes: the terminal's bash handed itself over (exec, so none is
#   left), or Hotaru started the shell itself;
# - a probe typed into the window: which shell it is (one under that
#   terminal's process, so the keys went to its window), the setup Sekka
#   gives it (Starship prompt, zoxide, fzf's history search, eza, zsh's
#   suggestions, nushell without its banner, zsh without its new-user
#   wizard), and that bash started from it stays bash;
# - the window's title, which names the folder rather than the bash that
#   started, for every shell but bash;
# - that a login bash, as a console or SSH starts, hands over with --login.
#
# Ghostty and kitty aren't Sekka's own; the VMs add them as a person would.
# Hotaru is Aizome's: the Tatami VM adds Aizome's package to have it.
# Screenshots of each terminal and of Yukimi's choice are in the result.
#
#     nix build .#checks.x86_64-linux.terminal-shells -L
{
  pkgs,
  sekkaModule,
  applicationPkgs,
}:
let
  inherit (pkgs) lib;
  shells = [
    "fish"
    "bash"
    "zsh"
    "nushell"
  ];
  node =
    desktop: default:
    { config, pkgs, ... }:
    {
      imports = [ sekkaModule ];
      # What the installed flake passes the application catalog.
      _module.args = {
        inherit applicationPkgs;
        aiPackages = { };
        ompPackage = null;
      };
      calamares.desktops = [ desktop ];
      calamares.defaultDesktop = desktop;
      calamares.applications = [ ];
      calamares.installUser = "demo";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
      };
      services.displayManager.autoLogin = {
        enable = true;
        user = "demo";
      };
      environment.systemPackages = [
        pkgs.ghostty
        pkgs.kitty
      ]
      ++ lib.optional (desktop == "tatami") config.programs.aizome.package;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
      # The others, as Yukimi would write them.
      specialisation = lib.genAttrs (lib.remove default shells) (shell: {
        configuration.calamares.terminalShell = shell;
      });
      virtualisation = {
        memorySize = 6144;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1920,yres=1080"
        ];
      };
      hardware.graphics.enable = true;
    };
  # Typed into each terminal: what the shell reports of itself, written once
  # it's all there.
  probes = {
    fish = pkgs.writeText "probe.fish" ''
      begin
          echo shell=fish
          echo "term=$TERM:$TERM_PROGRAM"
          echo "shlvl=$SHLVL"
          echo pid=$fish_pid
          status is-interactive; and echo interactive
          functions fish_prompt | string match -q '*starship*'; and echo prompt=starship
          functions -q __zoxide_z; and echo zoxide
          bind -M insert | string match -q '*_fzf_search_history*'; and echo fzf
          functions ls | string match -q '*eza*'; and echo ls=eza
          echo 'echo nested=$BASH_VERSION' | bash -i 2>/dev/null | string match -q 'nested=5*'; and echo nested=bash
      end >/tmp/shell-probe.part
      mv /tmp/shell-probe.part /tmp/shell-probe
    '';
    bash = pkgs.writeText "probe.bash" ''
      {
        echo shell=bash
        echo "term=$TERM:''${TERM_PROGRAM-}"
        echo "shlvl=$SHLVL"
        echo pid=$$
        [[ $- == *i* ]] && echo interactive
        [[ $(echo 'echo nested=$BASH_VERSION' | bash -i 2>/dev/null) == nested=5* ]] && echo nested=bash
      } >/tmp/shell-probe.part
      mv /tmp/shell-probe.part /tmp/shell-probe
    '';
    zsh = pkgs.writeText "probe.zsh" ''
      {
        print shell=zsh
        print term=$TERM:$TERM_PROGRAM
        print shlvl=$SHLVL
        print pid=$$
        [[ -o interactive ]] && print interactive
        [[ $PROMPT == *starship* ]] && print prompt=starship
        (( $+functions[__zoxide_z] )) && print zoxide
        [[ $(bindkey '^R') == *fzf-history-widget* ]] && print fzf
        [[ ''${aliases[ls]} == eza* ]] && print ls=eza
        (( $+functions[_zsh_autosuggest_start] )) && print suggestions
        [[ -e ~/.zshrc ]] || print newuser=off
        [[ $(print 'echo nested=$BASH_VERSION' | bash -i 2>/dev/null) == nested=5* ]] && print nested=bash
      } >/tmp/shell-probe.part
      mv /tmp/shell-probe.part /tmp/shell-probe
    '';
    nushell = pkgs.writeText "probe.nu" ''
      [
          "shell=nu"
          $'term=($env.TERM? | default ""):($env.TERM_PROGRAM? | default "")'
          $'shlvl=($env.SHLVL? | default "")'
          $"pid=($nu.pid)"
          (if $nu.is-interactive { "interactive" })
          (if ($env.STARSHIP_SHELL? == "nu") { "prompt=starship" })
          (if (which z | is-not-empty) { "zoxide" })
          (if ($env.config.keybindings | any {|k| ($k.name? | default "") == "fzf_history" }) { "fzf" })
          (if not $env.config.show_banner { "banner=off" })
          (if (("echo nested=$BASH_VERSION" | ^bash -i e> /dev/null | str trim) starts-with "nested=5") { "nested=bash" })
      ] | compact | str join "\n" | save -f /tmp/shell-probe.part
      ^mv /tmp/shell-probe.part /tmp/shell-probe
    '';
  };
in
pkgs.testers.runNixOSTest {
  name = "sekka-terminal-shells";
  globalTimeout = 5400;
  node.pkgsReadOnly = false;
  nodes = {
    aizome = node "aizome" "fish";
    tatami = node "tatami" "bash";
  };
  testScript = ''
    PROBES = ${builtins.toJSON probes}
    ${builtins.readFile ./terminal-shells-vm-test.py}
  '';
}
