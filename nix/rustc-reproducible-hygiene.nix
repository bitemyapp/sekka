# Keep the root-locked compiler and bootstrap dependencies. Fix hygiene
# serialization and build the bundled BPF libraries without unsafe backend
# concurrency; parallel frontend interning must not change crate metadata.
# Preserve the normal compiler's std, targets and rustc-dev outputs so tools
# linking rustc_private use the same compiler rather than the unpatched driver.
{ pkgs }:
let
  unwrapped = pkgs.rustc.unwrapped.overrideAttrs (old: {
    # LLVM 21's BPFCoreSharedInfo::SeqNum is a process-global, unsynchronized
    # counter. Parallel CGU optimization changes passthrough IDs and ThinLTO
    # symbol hashes in BPF bitcode. Serialize only these two std builds: retain
    # every target, CGU and optimization, with unchanged host/user Cargo flags.
    # This avoids the build race; it does not fix general LLVM BPF concurrency.
    # Route the scheduling flag through bootstrap's compiler-only std flags;
    # target.rustflags also reaches rustdoc, which does not accept it.
    patches = (old.patches or [ ]) ++ [ ./patches/rustc-reproducible-hygiene.patch ];
  });
  rustc = pkgs.rustc.override { rustc-unwrapped = unwrapped; };
  cargo = pkgs.cargo;
  rustPlatform = pkgs.makeRustPlatform { inherit cargo rustc; };
in
{
  inherit rustc cargo rustPlatform;
  clippy = pkgs.clippy.override { inherit rustc rustPlatform; };
  rustfmt = pkgs.rustfmt.override { inherit rustc rustPlatform; };
}
