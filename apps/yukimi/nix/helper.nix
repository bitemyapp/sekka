# SPDX-License-Identifier: MIT OR Apache-2.0
{
  lib,
  rustPlatform,
  pkg-config,
  sqlite,
  util-linux,
  clippy,
}:
let
  # Content-addressed source: the same derivation wherever the checkout is.
  source = lib.fileset.toSource {
    root = ../../..;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../yukimi-helper
      # Completion detection uses the UI-independent Aizome IPC protocol.
      ../../../desktop/aizome/Cargo.toml
      ../../../desktop/aizome/crates/aizome-ipc
      # Typestate support: `Unsettled` for restore points and the clean-up,
      # `SameTarget` for saved configurations.
      ../../../desktop/aizome/crates/aizome-typestate
      ../../../desktop/aizome/session/aizome-session
    ];
  };
in
rustPlatform.buildRustPackage {
  pname = "yukimi-helper";
  version = "0.1.0";
  src = source;
  postUnpack = ''
    sourceRoot="$sourceRoot/apps/yukimi"
  '';
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  nativeCheckInputs = [
    util-linux
    clippy
  ];
  # The helper runs programs only through its library, which names the ones
  # it may run; yukimi-helper/clippy.toml denies the other ways to start one.
  # Only that lint fails the build, so a toolchain's new lints don't.
  preCheck = ''
    cargo clippy --offline --locked -p yukimi-helper --all-targets -- \
      -A clippy::all -D clippy::disallowed_methods

    # The programs forbid the program ban's lint, so an allow there doesn't
    # compile under Clippy (E0453). The library denies it, and may allow it
    # only on process.rs's one function that makes commands; the namespace
    # tests, which run scripts, allow it at their root.
    found=$(grep -rnE --include='*.rs' '(allow|expect|warn|deny|forbid)\([^)]*disallowed_methods' yukimi-helper \
      | sed -E 's/^([^:]*):[0-9]+: */\1:/' | sort)
    expected=$(sort <<'EOF'
    yukimi-helper/src/bin/sekka-update.rs:#![forbid(clippy::disallowed_methods)]
    yukimi-helper/src/lib.rs:#![deny(clippy::disallowed_methods)]
    yukimi-helper/src/main.rs:#![forbid(clippy::disallowed_methods)]
    yukimi-helper/src/process.rs:#[allow(clippy::disallowed_methods)]
    yukimi-helper/tests/namespace.rs:#![allow(clippy::disallowed_methods)]
    EOF
    )
    if [ "$found" != "$expected" ]; then
      echo "The program ban's lint levels differ from the allowed ones:"
      printf 'found:\n%s\nallowed:\n%s\n' "$found" "$expected"
      exit 1
    fi
    if [ "$(grep -A1 -F '#[allow(clippy::disallowed_methods)]' yukimi-helper/src/process.rs | tail -n1)" \
      != "fn new_command(program: &Path) -> Command {" ]; then
      echo "process.rs allows the program ban on something other than new_command"
      exit 1
    fi

    # Unsafe code lives only in yukimi-system's sys module. Every attribute
    # that names the unsafe_code lint must be one of these, and the one
    # allow must be on `pub mod sys;`. Nothing outside sys.rs may contain an
    # unsafe block, function, impl, trait or extern block, including tests,
    # examples and the programs.
    lint_levels() {
      grep -rnE --include='*.rs' "$1" crates yukimi-helper | sed -E 's/^([^:]*):[0-9]+:/\1:/' | sort
    }
    found=$(lint_levels '(allow|expect|warn|deny|forbid)\([^)]*unsafe_code')
    expected=$(sort <<'EOF'
    crates/yukimi-config/src/lib.rs:#![forbid(unsafe_code)]
    crates/yukimi-store/src/lib.rs:#![forbid(unsafe_code)]
    crates/yukimi-system/src/lib.rs:#![deny(unsafe_code)]
    crates/yukimi-system/src/lib.rs:#[allow(unsafe_code)]
    yukimi-helper/src/bin/sekka-update.rs:#![forbid(unsafe_code)]
    yukimi-helper/src/lib.rs:#![forbid(unsafe_code)]
    yukimi-helper/src/main.rs:#![forbid(unsafe_code)]
    EOF
    )
    expected=$(printf '%s\n' "$expected" | sed -E 's/^ +//' | sort)
    if [ "$found" != "$expected" ]; then
      echo "The unsafe_code lint levels differ from the allowed ones:"
      printf 'found:\n%s\nallowed:\n%s\n' "$found" "$expected"
      exit 1
    fi
    if [ "$(grep -A1 -F '#[allow(unsafe_code)]' crates/yukimi-system/src/lib.rs | tail -n1)" != "pub mod sys;" ]; then
      echo "yukimi-system allows unsafe code on something other than its sys module"
      exit 1
    fi
    if grep -rnE --include='*.rs' '\bunsafe\s*(\{|fn\b|impl\b|trait\b|extern\b)' crates yukimi-helper \
      | grep -v '^crates/yukimi-system/src/sys.rs:'; then
      echo "Unsafe code outside yukimi-system's sys module"
      exit 1
    fi
  '';
  buildInputs = [ sqlite ];
  postInstall = ''
    # Names this exact helper, so polkit's prompt says what Yukimi is asking for.
    mkdir -p $out/share/polkit-1/actions
    substitute yukimi-helper/data/io.github.bitemyapp.Yukimi.policy.in \
      $out/share/polkit-1/actions/io.github.bitemyapp.Yukimi.policy --subst-var out
  '';
  meta = {
    description = "Snow-viewing for NixOS: see what's installed, find what's available, and change it without editing config";
    homepage = "https://github.com/bitemyapp/sekka/tree/main/apps/yukimi";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "sekka-update";
    platforms = lib.platforms.linux;
  };
}
