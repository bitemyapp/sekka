# m1n1-aurora 1.6.1.aurora17 from Omarchy's release aurora-2026.10.17.20:
# AsahiLinux/m1n1 main (3e354a24) and Omarchy's patches, among them the
# T6030 display and GPU handoff. The handoff stays off unless boot.bin ends
# with its chosen.asahi,t6030-* switches. Built as Nixpkgs builds m1n1.
{ m1n1, fetchFromGitHub }:
let
  release = "aurora-2026.10.17.20";
  rev = "d05ba17dfb771871d608225be42968bbb1e75a18";
in
m1n1.overrideAttrs (
  finalAttrs: old: {
    pname = "m1n1-aurora";
    version = "1.6.1-aurora17";
    src = fetchFromGitHub {
      owner = "omacom";
      repo = "m1n1";
      inherit rev;
      hash = "sha256-Ed1B/SyqCeq+1YPECS0J80tJickA/mgBJ1S0yGxx4aA=";
    };
    cargoDeps = old.passthru.rustPlatform.fetchCargoVendor {
      inherit (finalAttrs) pname version;
      src = "${finalAttrs.src}/rust";
      sourceRoot = "rust";
      # The same crates as Nixpkgs' m1n1 2.0.0.
      hash = "sha256-7G9aTQEiQ8ocsY0gSoBg9A2cusfqR4a1nhFikpWMXAA=";
    };
    # What the release's boot loader reports in /chosen/asahi,m1n1-stage2-version.
    env = (old.env or { }) // {
      M1N1_VERSION_TAG = "v1.6.1-omarchy.aurora17";
    };
    passthru = old.passthru // {
      inherit release rev;
      # The switches that turn the handoff on for an M3 Pro, as the release's
      # installer writes them (M3_SWITCHES).
      t6030Switches = [
        "chosen.asahi,t6030-gpu=1"
        "chosen.asahi,t6030-dcp=1"
        "chosen.asahi,t6030-dcpext=1"
      ];
    };
    meta = old.meta // {
      description = "Omarchy's m1n1 with the M3 display and GPU handoff (${release})";
      homepage = "https://github.com/omacom/m1n1";
    };
  }
)
