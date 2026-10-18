# The catalog's fish in a disposable VM: a person's TTY login hands over from
# bash to fish, set up as Omarchy sets it up (its settings, functions, fzf.fish
# bindings, zoxide, mise and Starship prompt), while bash -c, root and shells
# started from fish stay bash. The packages are the catalog's own.
{
  pkgs,
  applicationsModule,
  applicationPkgs,
}:
let
  # Typed into the autologin's fish, so it reports on that interactive shell.
  check = pkgs.writeText "fish-check.fish" ''
    begin
        status is-login; and echo login
        echo keys=$fish_key_bindings
        set -q fish_greeting; and test -z "$fish_greeting"; and echo greeting=none
        echo bat=$BAT_THEME
        functions ls | string match -q '*eza -lh --group-directories-first --icons=auto $argv*'; and echo ls=eza
        functions cd | string match -q '*zd $argv*'; and echo cd=zd
        alias | string match -q '*ls --color*'; or echo nixos-aliases=none
        functions -q __zoxide_z; and echo zoxide
        functions -q __mise_env_eval; and echo mise
        functions fish_prompt | string match -q '*starship*'; and echo prompt=starship
        # Omarchy's error symbol; Starship's own is a red ❯.
        starship prompt --status=1 | string match -q '*✗*'; and echo prompt=omarchy
        bind -M insert | string match -q '*_fzf_search_history*'; and echo history=fzf
        bind -M insert | string match -q '*_fzf_search_directory*'; and echo directory=fzf
        bind -M insert | string match -q '*_fzf_search_processes*'; or echo processes=off
        ls / | string match -q '*Permissions*'; and echo listing=eza
    end >/tmp/fish-state 2>&1
  '';
  rootInput = pkgs.writeText "root-input" ''
    echo "shell=$BASH_VERSION" > /tmp/root-shell
    exit
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-fish";
  nodes.machine = {
    imports = [ applicationsModule ];
    # What the installed flake passes; only fish is chosen here.
    _module.args = {
      inherit applicationPkgs;
      aiPackages = { };
      ompPackage = null;
    };
    calamares.applications = [ "fish" ];
    calamares.installUser = "alice";
    users.users.alice.isNormalUser = true;
    services.getty.autologinUser = "alice";
  };
  testScript = ''
    machine.wait_for_unit("multi-user.target")

    # The TTY login's bash handed over to fish.
    machine.wait_until_succeeds("pgrep -u alice -x fish", timeout=60)
    machine.fail("pgrep -u alice -x bash")
    parent = machine.succeed("ps -o comm= -p $(ps -o ppid= -p $(pgrep -u alice -x fish))").strip()
    assert parent == "login", parent

    machine.wait_until_tty_matches("1", "~")
    machine.send_chars("source ${check}\n")
    machine.wait_for_file("/tmp/fish-state", timeout=30)
    machine.sleep(1)
    state = machine.succeed("cat /tmp/fish-state").splitlines()
    expected = [
        "login",
        "keys=fish_vi_key_bindings",
        "greeting=none",
        "bat=ansi",
        "ls=eza",
        "cd=zd",
        "nixos-aliases=none",
        "zoxide",
        "mise",
        "prompt=starship",
        "prompt=omarchy",
        "history=fzf",
        "directory=fzf",
        "processes=off",
        "listing=eza",
    ]
    assert state == expected, state
    machine.screenshot("fish")

    # Omarchy's cd falls back to zoxide for a directory it has been to.
    machine.send_chars("cd /etc/fish; cd /; cd fis; pwd > /tmp/zoxide\n")
    machine.wait_for_file("/tmp/zoxide", timeout=30)
    machine.sleep(1)
    assert machine.succeed("cat /tmp/zoxide").strip() == "/etc/fish"

    # bash started from fish stays bash.
    machine.send_chars("bash\n")
    machine.wait_until_succeeds("pgrep -u alice -x bash", timeout=30)
    machine.send_chars("echo shell=$BASH_VERSION > /tmp/nested; exit\n")
    machine.wait_for_file("/tmp/nested", timeout=30)
    machine.sleep(1)
    assert machine.succeed("cat /tmp/nested").strip() != "shell=", "nested bash handed over"
    machine.wait_until_fails("pgrep -u alice -x bash", timeout=30)

    # Scripts run in bash.
    assert machine.succeed("su -l alice -c 'echo $BASH_VERSION'").strip() != ""

    # So does root's interactive bash.
    machine.succeed("script -qec 'env -u SHLVL bash -i' /dev/null < ${rootInput}")
    assert machine.succeed("cat /tmp/root-shell").strip() != "shell=", "root handed over"
  '';
}
