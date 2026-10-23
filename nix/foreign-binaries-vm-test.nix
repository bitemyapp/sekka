# Programs built for generic Linux, in two disposable VMs: plain NixOS, where
# they cannot start, and one with Sekka's foreign-binaries module, where they
# run as on Ubuntu. The programs are the ones Zed downloads for itself: its
# generic rust-analyzer, Node.js from nodejs.org, and Zed's own release (what
# its updater installs), unpacked into the account the way Zed unpacks them.
# The VMs have no network, so the downloads are fetched here first. The
# second VM also has the catalog's Zed, which installs that release itself,
# and a third opens a Rust project in it on a desktop.
{
  pkgs,
  foreignBinariesModule,
  zedPackage,
}:
let
  rustAnalyzer = pkgs.fetchurl {
    url = "https://github.com/rust-lang/rust-analyzer/releases/download/2026-10-19/rust-analyzer-x86_64-unknown-linux-gnu.gz";
    sha256 = "1njqwlxpfmvfq8366s7amdk92sl4ahvaxvcyx6c9yic1msfirplg";
  };
  node = pkgs.fetchurl {
    url = "https://nodejs.org/dist/v24.22.1/node-v24.22.1-linux-x64.tar.xz";
    sha256 = "6c172a9ab6718f7b801fb5a7bd486af2ee5e32758086a04b84fcce2aa77095e6";
  };
  # As Zed lays them out under ~/.local.
  download = pkgs.writeShellScript "download" ''
    set -eu
    languages=~/.local/share/zed/languages
    mkdir -p $languages/rust-analyzer $languages/node ~/.local
    gzip -dc ${rustAnalyzer} > $languages/rust-analyzer/rust-analyzer
    chmod +x $languages/rust-analyzer/rust-analyzer
    tar -xJf ${node} -C $languages/node
    if ! command -v zed >/dev/null; then
      tar -xzf ${zedPackage.release} -C ~/.local
    fi
    printf '#!/bin/bash\necho "bash $BASH_VERSION"\n' > ~/script
    chmod +x ~/script
  '';
  common = {
    users.users.alice.isNormalUser = true;
    environment.systemPackages = with pkgs; [
      gnutar
      gzip
      xz
    ];
    virtualisation.memorySize = 2048;
  };
  # A Rust project, Zed trusting it (it asks otherwise), and the analyzer Zed
  # downloaded for it last time it was online, under the name Zed gives it.
  project = pkgs.writeShellScript "project" ''
    set -eu
    mkdir -p ~/hello/src ~/.config/zed ~/.local/share/zed/languages/rust-analyzer
    printf '[package]\nname = "hello"\nversion = "0.1.0"\nedition = "2024"\n' > ~/hello/Cargo.toml
    printf 'fn main() {\n    println!("hello");\n}\n' > ~/hello/src/main.rs
    echo '{ "session": { "trust_all_worktrees": true } }' > ~/.config/zed/settings.json
    gzip -dc ${rustAnalyzer} > ~/.local/share/zed/languages/rust-analyzer/rust-analyzer-2026-10-19
    chmod +x ~/.local/share/zed/languages/rust-analyzer/rust-analyzer-2026-10-19
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-foreign-binaries";
  nodes = {
    plain = common;
    sekka = {
      imports = [
        common
        foreignBinariesModule
      ];
      environment.systemPackages = [ zedPackage ];
    };
    desktop = {
      imports = [
        common
        foreignBinariesModule
        "${pkgs.path}/nixos/tests/common/x11.nix"
      ];
      test-support.displayManager.auto.user = "alice";
      # Zed draws with Vulkan: Mesa's software renderer, in a VM.
      hardware.graphics.enable = true;
      environment.systemPackages = [
        zedPackage
        pkgs.cargo
        pkgs.rustc
      ];
      environment.variables.ZED_ALLOW_EMULATED_GPU = "1";
      virtualisation.memorySize = pkgs.lib.mkForce 4096;
      virtualisation.cores = 4;
    };
  };
  testScript = ''
    # As alice, with nothing from a login shell: what a desktop launcher or
    # a systemd user service passes on.
    def run(machine, command):
        return machine.execute(
            f"su alice -c 'cd && env -i HOME=/home/alice PATH=/run/current-system/sw/bin {command}' 2>&1"
        )

    analyzer = "~/.local/share/zed/languages/rust-analyzer/rust-analyzer"
    node = "~/.local/share/zed/languages/node/node-v24.22.1-linux-x64/bin/node"
    zed = "~/.local/zed.app/bin/zed"
    version = "${zedPackage.version}"
    editor = "~/.local/zed.app/libexec/zed-editor"

    start_all()
    desktop.succeed("su alice -c ${project}")
    for machine in [plain, sekka]:
        machine.wait_for_unit("multi-user.target")
        machine.succeed("su alice -c ${download}")

    with subtest("Plain NixOS cannot start them"):
        for command in [f"{analyzer} --version", f"{node} -p 1+1", f"{zed} --version", "./script"]:
            status, output = run(plain, command)
            print(f"plain: {command}: {status}: {output}")
            assert status != 0, f"{command} started on plain NixOS"

    with subtest("The catalog's Zed installs Zed's release in the account"):
        sekka.fail("su alice -c 'test -e ~/.local/zed.app'")
        status, output = run(sekka, "zed --version")
        assert status == 0 and version in output, output
        sekka.succeed("su alice -c 'test -O ~/.local/zed.app/libexec/zed-editor'")
        sekka.fail("su alice -c 'ls -A ~/.local | grep -F .zed.app.'")

    with subtest("Sekka runs them"):
        status, output = run(sekka, f"{analyzer} --version")
        assert status == 0 and output.startswith("rust-analyzer "), output
        status, output = run(sekka, f"{node} -p 1+1")
        assert status == 0 and output.strip() == "2", output
        status, output = run(sekka, f"{zed} --version")
        assert status == 0 and version in output, output
        status, output = run(sekka, "./script")
        assert status == 0 and output.startswith("bash "), output
        print(f"sekka: {output}")

    with subtest("Zed finds every library it links and loads"):
        status, output = run(sekka, f"LD_TRACE_LOADED_OBJECTS=1 {editor}")
        print(output)
        assert status == 0 and "not found" not in output, output
        # Opened with dlopen when the window appears, so not in the list above.
        for library in ["libvulkan.so.1", "libwayland-client.so.0", "libxkbcommon.so.0",
                        "libxkbcommon-x11.so.0", "libX11.so.6", "libX11-xcb.so.1", "libxcb.so.1",
                        "libasound.so.2", "libEGL.so.1", "libfontconfig.so.1"]:
            sekka.succeed(f"test -e /run/current-system/sw/share/nix-ld/lib/{library}")

    with subtest("Zed's updates are left alone"):
        # What Zed's updater does: rsync a newer release over the copy.
        sekka.succeed("su alice -c 'echo newer > ~/.local/zed.app/updated'")
        status, output = run(sekka, "zed --version")
        assert status == 0, output
        sekka.succeed("su alice -c 'grep -q newer ~/.local/zed.app/updated'")
        # Zed's updater finds tar and rsync, which plain NixOS lacks.
        sekka.succeed("grep -q rsync $(readlink -f $(command -v zed))")

    with subtest("A removed copy comes back"):
        sekka.succeed("su alice -c 'rm -rf ~/.local/zed.app'")
        status, output = run(sekka, "zed --version")
        assert status == 0 and version in output, output

    with subtest("Zed starts the rust-analyzer it downloaded"):
        desktop.wait_for_x()
        # So that the test, as root, can see alice's windows.
        desktop.wait_for_file("/home/alice/.Xauthority")
        desktop.succeed("cp /home/alice/.Xauthority /root/.Xauthority")
        desktop.succeed("su - alice -c 'DISPLAY=:0 zed ~/hello ~/hello/src/main.rs >&2 &'")
        try:
            desktop.wait_for_window("main.rs", timeout=120)
            desktop.wait_until_succeeds("pgrep -u alice -f languages/rust-analyzer/rust-analyzer-2026-10-19", timeout=120)
            # Loaded the project: cargo metadata ran, and it is still up.
            desktop.wait_until_succeeds("grep -q 'rust-analyzer' /home/alice/.local/share/zed/logs/Zed.log", timeout=60)
            desktop.sleep(15)
            desktop.succeed("pgrep -u alice -f languages/rust-analyzer/rust-analyzer-2026-10-19")
        finally:
            zed_log = desktop.execute("cat /home/alice/.local/share/zed/logs/Zed.log")[1]
            print(zed_log)
            desktop.screenshot("zed")
        assert "Could not start dynamically linked executable" not in zed_log
  '';
}
