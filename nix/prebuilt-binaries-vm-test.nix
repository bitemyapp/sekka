# Programs built for other Linux distributions run on an installed system
# (installer/genkan/rust/system/prebuilt-binaries.nix): the ones Zed
# downloads into ~/.local/share/zed, put where Zed puts them and started as
# Zed starts them, without a login shell's environment.
{ pkgs }:
let
  fetch = url: hash: pkgs.fetchurl { inherit url hash; };
  # Zed's Rust language server: GitHub's generic build of a recent release
  # (Zed takes the newest; crates/languages/src/rust.rs).
  rustAnalyzer = fetch "https://github.com/rust-lang/rust-analyzer/releases/download/2026-10-19/rust-analyzer-x86_64-unknown-linux-gnu.gz" "sha256-j94cna6BRZ+Y6Z7trjZUhGqRZqvqaGMGwm5XdzvlWNo=";
  # Zed's Node.js, when there is no suitable one on PATH, and what Node
  # version managers install: it needs libstdc++.
  node = fetch "https://nodejs.org/dist/v24.22.1/node-v24.22.1-linux-x64.tar.xz" "sha256-bBcqmrZxj3uAH7WnvUhq8u5eMnWAhqBLhPzOKqdwleY=";
  # Version hints in package.json; it needs OpenSSL 3.
  packageVersionServer = fetch "https://github.com/zed-industries/package-version-server/releases/download/v0.0.10/package-version-server-x86_64-unknown-linux-gnu.tar.gz" "sha256-e7GeDYC0EzFwO2Jv8mW0KoLnoCG2EiGaNgG30Kel2bY=";
  # Zed's debugger for Rust, C and C++, with its own LLDB and Python.
  codelldb = fetch "https://github.com/vadimcn/codelldb/releases/download/v1.12.3/codelldb-linux-x64.vsix" "sha256-HNfzhlmAIrUaW5O5/6I+gSsj9RnP4YMzhOxL70v9G+E=";
  # ~/.local/share/zed as Zed leaves it, with the files unchanged: no
  # fixup phase patches them.
  zed = pkgs.runCommand "zed-downloads" { nativeBuildInputs = [ pkgs.unzip ]; } ''
    mkdir -p $out/languages/rust-analyzer $out/languages/package-version-server \
      $out/node $out/debug_adapters/CodeLLDB
    gzip -dc ${rustAnalyzer} > $out/languages/rust-analyzer/rust-analyzer-2026-10-19
    chmod +x $out/languages/rust-analyzer/rust-analyzer-2026-10-19
    tar -xzf ${packageVersionServer} -C $out/languages/package-version-server
    tar -xJf ${node} -C $out/node
    unzip -q ${codelldb} -d $out/debug_adapters/CodeLLDB
    chmod +x $out/debug_adapters/CodeLLDB/extension/adapter/codelldb \
      $out/debug_adapters/CodeLLDB/extension/lldb/bin/*
  '';
  project = pkgs.runCommand "hello" { } ''
    mkdir -p $out/src
    printf '[package]\nname = "hello"\nversion = "0.1.0"\nedition = "2021"\n' > $out/Cargo.toml
    printf 'fn main() {\n    println!("{}", std::env::args().count());\n}\n' > $out/src/main.rs
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-prebuilt-binaries";
  nodes.machine = {
    imports = [ ../installer/genkan/rust/system/prebuilt-binaries.nix ];
    virtualisation.memorySize = 3072;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
    };
    # The toolchain rust-analyzer runs: Nix programs.
    environment.systemPackages = [
      pkgs.cargo
      pkgs.rustc
    ];
  };
  testScript = ''
    import shlex

    machine.wait_for_unit("multi-user.target")

    # nix-ld, not NixOS's stub, is the generic loader.
    assert "nix-ld" in machine.succeed("readlink -f /lib64/ld-linux-x86-64.so.2")

    home = "/home/alice"
    data = f"{home}/.local/share/zed"
    machine.succeed(f"runuser -u alice -- mkdir -p {data}")
    machine.succeed(f"runuser -u alice -- cp -r ${zed}/. {data}/")
    machine.succeed(f"runuser -u alice -- cp -r --no-preserve=mode ${project} {home}/hello")

    # As Zed starts what it downloads: with its own environment, which has
    # no NIX_LD variables when Zed was started from a launcher.
    def bare(command, *env):
        variables = [f"HOME={home}", "PATH=/run/current-system/sw/bin", *env]
        return machine.succeed(
            "runuser -u alice -- env -i " + " ".join(map(shlex.quote, variables)) + " " + command
        )

    # From a login shell, which has them.
    def login(command):
        return machine.succeed("su - alice -c " + shlex.quote(command))

    analyzer = f"{data}/languages/rust-analyzer/rust-analyzer-2026-10-19"
    node = f"{data}/node/node-v24.22.1-linux-x64/bin/node"
    lldb = f"{data}/debug_adapters/CodeLLDB/extension"

    # Each finds every library it asks for through the generic loader:
    # glibc's in the Nix store, these in nix-ld's set, CodeLLDB's own beside it.
    libraries = "/run/current-system/sw/share/nix-ld/lib"
    needs = {
        analyzer: ["libgcc_s.so.1"],
        node: ["libstdc++.so.6", "libgcc_s.so.1"],
        f"{data}/languages/package-version-server/package-version-server": ["libssl.so.3", "libcrypto.so.3"],
        f"{lldb}/adapter/codelldb": ["libgcc_s.so.1"],
        f"{lldb}/lldb/bin/lldb": ["libz.so.1"],
        f"{lldb}/lldb/bin/lldb-server": ["libz.so.1"],
    }
    for binary, names in needs.items():
        trace = bare(binary, "LD_TRACE_LOADED_OBJECTS=1")
        assert "not found" not in trace, f"{binary}:\n{trace}"
        for name in names:
            assert f"{name} => {libraries}/{name}" in trace, f"{binary}:\n{trace}"

    # Zed runs a downloaded rust-analyzer with --version before using it,
    # and downloads it again when that fails.
    assert "rust-analyzer" in bare(f"{analyzer} --version")
    assert "rust-analyzer" in login(f"{analyzer} --version")

    # It loads a project and the standard library, running cargo and rustc.
    stats = bare(
        f"env -C {home}/hello {analyzer} analysis-stats . 2>&1",
        "RUST_SRC_PATH=${pkgs.rustPlatform.rustLibSrc}",
        "CARGO_NET_OFFLINE=true",
    )
    assert "Database loaded" in stats and "crates: 1," in stats, stats

    assert bare(f"{node} --version").strip() == "v24.22.1"
    assert "lldb version" in bare(f"{lldb}/lldb/bin/lldb --version")

    # What these programs start sees the environment they were given, not
    # the libraries nix-ld gave them.
    child = shlex.quote('process.stdout.write(require("child_process").execFileSync("printenv").toString())')
    for environment in [bare(f"{node} -e {child}"), login(f"{node} -e {child}")]:
        assert f"HOME={home}" in environment, environment
        paths = [line for line in environment.splitlines() if line.startswith("LD_LIBRARY_PATH=")]
        assert all(libraries not in line for line in paths), environment
  '';
}
