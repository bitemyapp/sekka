{
  writeShellApplication,
  rustup,
  coreutils,
}:
writeShellApplication {
  name = "sekka-prepare-rust-tools";
  runtimeInputs = [
    rustup
    coreutils
  ];
  text = builtins.readFile ./prepare.sh;
}
