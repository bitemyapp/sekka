# Import with pkgs from Sekka's root flake. This exercises the pinned Nix
# split-debug hook on a tiny Rust fixture, not the complete Aizome package.
{ pkgs }:
pkgs.stdenv.mkDerivation {
  pname = "sekka-profiling-symbol-probe";
  version = "1";
  src = pkgs.lib.fileset.toSource {
    root = ./.;
    fileset = ./probe.rs;
  };
  nativeBuildInputs = [ pkgs.rustc pkgs.binutils ];
  separateDebugInfo = true;
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    # Preserve the release profile's line tables, as the earlier Aizome draft
    # did; split-debug's default full -g is not necessary for sampling.
    export NIX_RUSTFLAGS="''${NIX_RUSTFLAGS/ -g -C strip=none/}"
    rustc probe.rs --edition=2024 -C opt-level=3 -C debuginfo=line-tables-only \
      -C codegen-units=4 -C link-arg=-Wl,--build-id=sha1 -o probe
    objcopy --dump-section .text=original.text probe
    objcopy --dump-section .eh_frame=original.eh_frame probe
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 probe $out/bin/probe
    runHook postInstall
  '';
  postFixup = ''
    objcopy --dump-section .text=runtime.text $out/bin/probe
    objcopy --dump-section .eh_frame=runtime.eh_frame $out/bin/probe
    cmp original.text runtime.text
    cmp original.eh_frame runtime.eh_frame
    build_id=$(readelf -n $out/bin/probe | sed -n 's/.*Build ID: //p')
    debug_file=$debug/lib/debug/.build-id/''${build_id:0:2}/''${build_id:2}.debug
    test -f "$debug_file"
    address=$(nm "$debug_file" | awk '/interaction_stage/ && !seen { print $1; seen = 1 }')
    addr2line -f -C -e "$debug_file" "$address" | tee resolved.txt
    grep -q 'probe.rs:' resolved.txt
    mkdir -p $out/share/probe
    cp resolved.txt $out/share/probe/
    printf '%s\n' "$build_id" > $out/share/probe/build-id
    $out/bin/probe > $out/share/probe/output
  '';
}
