# Compile the actual patched spawn/which functions in a small driver. This
# exercises the launcher with a real user manager without booting a GPU.
{ pkgs, source }:
pkgs.runCommand "aizome-launch-probe"
  {
    nativeBuildInputs = [
      pkgs.rustc
      pkgs.stdenv.cc
      pkgs.python3
      pkgs.patch
    ];
  }
  ''
    cp ${source}/crates/aizome-comp-core/src/session.rs session.rs
    chmod u+w session.rs
    patch session.rs < ${./aizome-launch-scope.patch}
    python3 - <<'PY'
    from pathlib import Path
    source = Path('session.rs').read_text()
    start = source.index('pub fn spawn(')
    end = source.index('/// A free X display number.', start)
    prefix = r"""
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    struct Aizome;
    mod libc { unsafe extern "C" { pub fn setsid() -> i32; } }
    mod aizome_config {
        pub fn home_dir() -> std::path::PathBuf { std::env::var("HOME").unwrap().into() }
    }
    macro_rules! info { ($($arg:tt)*) => {}; }
    fn session_env(_: &Aizome) -> Vec<(String, String)> {
        vec![("AIZOME_LAUNCH_TEST".into(), "preserved".into())]
    }
    """
    main = r"""
    fn main() {
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let pid = spawn(&Aizome, &argv, Some("/tmp".into())).unwrap();
        std::fs::write("/tmp/aizome-launched-pid", pid.to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(300));
    }
    """
    Path('probe.rs').write_text(prefix + source[start:end] + main)
    PY
    mkdir -p $out/bin
    rustc --edition=2024 probe.rs -o $out/bin/aizome-launch-probe
  ''
