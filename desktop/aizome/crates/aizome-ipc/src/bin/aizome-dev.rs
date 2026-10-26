//! `aizome-dev`: hacking on Aizome while it runs. It builds a checkout of
//! Aizome the way the running session was built and hands what it built
//! to the session: the compositor's logic swaps in between two frames, the
//! shell restarts beside the old one, an app closes and starts again. The
//! other windows don't notice. `aizome-dev watch` does it on every save.
//!
//! The logic is a library loaded into the running compositor, so it must
//! be built against the very core that's running: the same sources, the
//! same toolchain, target, profile and flags, and the same packages built
//! together (Cargo resolves dependencies' features per build, and they
//! change every symbol). The session says how it was built (its core's
//! build info, and the package's share/aizome/build-env.json); this builds
//! exactly so. When the core itself changed, the logic is refused and the
//! change runs at the next login. docs/reload.md.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant, SystemTime};

use aizome_ipc::{Client, ReloadReport, Request, Response};
use serde_json::Value;

const USAGE: &str = "\
usage: aizome-dev [COMMAND] [WHAT...] [--checkout PATH] [--optimised]

  watch            rebuild on every save and reload what it changes: the
                   compositor's logic, the shell, the apps you run from the
                   checkout or edit (the default command)
  reload [WHAT...] build, then hand it to the running session: the logic
                   swaps in between frames, the shell restarts beside the old
                   one, an app closes and starts again
  build [WHAT...]  only build
  env              how the session was built, and how aizome-dev builds to match

  WHAT is logic, shell, or an app (hako, sumi, …): default logic and shell.

  --checkout PATH  the Aizome checkout (default: $AIZOME_CHECKOUT, else
                   ~/work/aizome)
  --optimised      build the logic as optimised as the session's own (by
                   default it builds quicker, at opt-level 1)

It talks to the session at $AIZOME_SOCKET (or the one for $WAYLAND_DISPLAY).
Changes to the compositor's core (aizome-comp-core, aizome-ipc,
aizome-config, aizome-theme, Sidestep) can't reload: they run at the next
login.
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help" || a == "help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let optimised = take_flag(&mut args, "--optimised");
    let checkout = match take_opt(&mut args, "--checkout") {
        Ok(c) => c.map(PathBuf::from),
        Err(e) => return fail(&e),
    };
    let command = if args.first().is_some_and(|a| !a.starts_with('-')) { args.remove(0) } else { "watch".into() };
    if let Some(a) = args.iter().find(|a| a.starts_with('-')) {
        return fail(&format!("unknown option {a}"));
    }
    let checkout = checkout
        .or_else(|| std::env::var_os("AIZOME_CHECKOUT").map(PathBuf::from))
        .unwrap_or_else(|| home().join("work/aizome"));
    let checkout = match std::fs::canonicalize(&checkout) {
        Ok(c) if c.join("Cargo.toml").is_file() => c,
        _ => return fail(&format!("{} isn't an Aizome checkout (--checkout PATH)", checkout.display())),
    };
    let mut dev = match Dev::new(checkout, optimised) {
        Ok(d) => d,
        Err(e) => return fail(&e),
    };
    let what: Vec<What> = match args.iter().map(|a| dev.what(a)).collect::<Result<Vec<_>, _>>() {
        Ok(w) if w.is_empty() => vec![What::Logic, What::Shell],
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let r = match command.as_str() {
        "env" => {
            dev.print_env();
            Ok(())
        }
        "build" => what.iter().try_for_each(|w| dev.build(w).map(|_| ())),
        "reload" => what.iter().try_for_each(|w| dev.build_and_reload(w, true)),
        "watch" => dev.watch(),
        c => Err(format!("unknown command {c} (aizome-dev --help)")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

fn fail(e: &str) -> ExitCode {
    eprintln!("aizome-dev: {e}");
    ExitCode::FAILURE
}

fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != name);
    args.len() != before
}

fn take_opt(args: &mut Vec<String>, name: &str) -> Result<Option<String>, String> {
    let Some(i) = args.iter().position(|a| a == name) else { return Ok(None) };
    if i + 1 >= args.len() {
        return Err(format!("{name} needs a value"));
    }
    let v = args.remove(i + 1);
    args.remove(i);
    Ok(Some(v))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn say(msg: impl AsRef<str>) {
    let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (h, m, s) = ((t / 3600) % 24, (t / 60) % 60, t % 60);
    eprintln!("[{h:02}:{m:02}:{s:02} UTC] {}", msg.as_ref());
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum What {
    Logic,
    Shell,
    /// An app by its directory under apps/ (its binary is aizome-<name>).
    App(String),
}

impl What {
    fn package(&self) -> String {
        match self {
            What::Logic => "aizome-comp-logic".into(),
            What::Shell => "aizome-shell".into(),
            What::App(a) => format!("aizome-{a}"),
        }
    }
}

/// How the running session was built, and how to build to match it.
struct Dev {
    checkout: PathBuf,
    optimised: bool,
    /// The running core's build info (target, profile, flags, ABI).
    core: Value,
    /// The package's build-env.json, when the session runs from one.
    env: Option<Value>,
    /// The environment cargo runs in (the checkout's dev shell's, with the
    /// session's toolchain first).
    cargo_env: Option<HashMap<String, String>>,
    target_dir: Option<PathBuf>,
}

impl Dev {
    fn new(checkout: PathBuf, optimised: bool) -> Result<Dev, String> {
        let mut c = Client::connect().map_err(|e| format!("no Aizome session to talk to ({e})"))?;
        let (exe, build) = match c.request(&Request::Version).map_err(|e| e.to_string())? {
            Response::Version { exe, build, .. } => (exe, build),
            r => return Err(format!("the compositor answered {r:?}")),
        };
        let core = build.ok_or("this session's compositor is older than live reload: log out and in once")?;
        // A package's compositor is <out>/bin/aizome; its build-env.json is
        // in <out>/share/aizome.
        let env = exe
            .map(|e| PathBuf::from(e.trim_end_matches(" (deleted)")))
            .and_then(|e| Some(e.parent()?.parent()?.join("share/aizome/build-env.json")))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok());
        Ok(Dev { checkout, optimised, core, env, cargo_env: None, target_dir: None })
    }

    fn what(&self, name: &str) -> Result<What, String> {
        Ok(match name {
            "logic" | "compositor" => What::Logic,
            "shell" => What::Shell,
            app => {
                let app = app.strip_prefix("aizome-").unwrap_or(app);
                if !self.checkout.join("apps").join(app).join("Cargo.toml").is_file() {
                    return Err(format!("{name}: not logic, shell, or an app under {}/apps", self.checkout.display()));
                }
                What::App(app.into())
            }
        })
    }

    fn core_str(&self, key: &str) -> String {
        self.core[key].as_str().unwrap_or_default().to_string()
    }

    /// The profile the session's compositor was built with (the package
    /// says; a build from a checkout is `release` or `dev`).
    fn profile(&self) -> String {
        if let Some(p) = self.env.as_ref().and_then(|e| e["profile"].as_str()) {
            return p.into();
        }
        match self.core_str("profile").as_str() {
            "debug" => "dev".into(),
            p => p.into(),
        }
    }

    fn profile_dir(&self) -> String {
        match self.profile().as_str() {
            "dev" | "test" => "debug".into(),
            "bench" => "release".into(),
            p => p.into(),
        }
    }

    /// The packages built together with the logic in the session's build.
    fn logic_packages(&self) -> Vec<String> {
        self.env
            .as_ref()
            .and_then(|e| e["packages"].as_array())
            .map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["aizome-comp".into(), "aizome-comp-logic".into()])
    }

    /// The session's compiler and Cargo: the store paths the package
    /// recorded (fetched if they aren't here), as bin directories.
    fn toolchain(&self) -> Vec<PathBuf> {
        let Some(env) = &self.env else { return Vec::new() };
        let mut dirs = Vec::new();
        for key in ["rustc_store_path", "cargo_store_path"] {
            let Some(parts) = env[key].as_array() else { continue };
            let path: String = parts.iter().filter_map(|p| p.as_str()).collect();
            let path = PathBuf::from(path);
            if !path.exists() {
                say(format!("fetching the session's toolchain: {}", path.display()));
                let ok = Command::new("nix-store")
                    .arg("--realise")
                    .arg(&path)
                    .stdout(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if !ok {
                    say(format!("couldn't fetch {}; using the checkout's toolchain", path.display()));
                    continue;
                }
            }
            dirs.push(path.join("bin"));
        }
        dirs
    }

    /// The environment to run cargo in: the checkout's dev shell (once, as
    /// `nix develop` is slow), the session's toolchain first on PATH, the
    /// session's flags, and nothing that would change what Cargo builds.
    fn cargo_env(&mut self) -> Result<HashMap<String, String>, String> {
        if let Some(e) = &self.cargo_env {
            return Ok(e.clone());
        }
        let mut env: HashMap<String, String> = std::env::vars().collect();
        let in_shell = std::env::var_os("IN_NIX_SHELL").is_some();
        if !in_shell && self.checkout.join("flake.nix").is_file() {
            say(format!("entering the dev shell of {}", self.checkout.display()));
            let out = Command::new("nix")
                .arg("develop")
                .arg(&self.checkout)
                .args(["--command", "env", "-0"])
                .stderr(Stdio::inherit())
                .output()
                .map_err(|e| format!("nix develop: {e}"))?;
            if !out.status.success() {
                return Err(format!("nix develop {} failed", self.checkout.display()));
            }
            env = out
                .stdout
                .split(|b| *b == 0)
                .filter_map(|kv| {
                    let kv = String::from_utf8_lossy(kv);
                    let (k, v) = kv.split_once('=')?;
                    Some((k.to_string(), v.to_string()))
                })
                .collect();
        }
        let tools = self.toolchain();
        if !tools.is_empty() {
            let path = env.get("PATH").cloned().unwrap_or_default();
            let mut dirs = tools;
            dirs.extend(std::env::split_paths(&path));
            env.insert("PATH".into(), std::env::join_paths(dirs).unwrap().to_string_lossy().into_owned());
        }
        // Nothing that changes how Cargo builds, but the session's flags.
        env.retain(|k, _| {
            !(k.starts_with("CARGO_PROFILE_")
                || k.starts_with("CARGO_BUILD_")
                || k == "CARGO_ENCODED_RUSTFLAGS"
                || k == "RUSTC_WORKSPACE_WRAPPER"
                || k == "RUSTC"
                || k == "CARGO_TARGET_DIR")
        });
        let mut rustflags = self.core_str("rustflags");
        // The linker doesn't change the code: mold, if the shell has it.
        if find_in(env.get("PATH").map(String::as_str).unwrap_or(""), "mold").is_some() {
            rustflags = format!("{rustflags} -C link-arg=-fuse-ld=mold").trim().to_string();
        }
        env.insert("RUSTFLAGS".into(), rustflags);
        let wrapper = self.checkout.join("scripts/rustc-wrapper");
        if !wrapper.is_file() {
            return Err(format!("{} has no scripts/rustc-wrapper: it predates live reload", self.checkout.display()));
        }
        env.insert("RUSTC_WRAPPER".into(), wrapper.to_string_lossy().into_owned());

        // The same compiler as the session's?
        let rustc = find_in(env.get("PATH").map(String::as_str).unwrap_or(""), "rustc")
            .ok_or("no rustc: run aizome-dev in the checkout's dev shell (nix develop)")?;
        let version = Command::new(&rustc)
            .arg("-vV")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string())
            .unwrap_or_default();
        let want = self.core_str("rustc");
        if version != want {
            return Err(format!(
                "the session was built with {want}, but this toolchain is {version} ({}): the logic \
                 wouldn't load. Use the checkout's dev shell at the same nixpkgs as the session",
                rustc.display()
            ));
        }
        self.cargo_env = Some(env.clone());
        Ok(env)
    }

    /// Where aizome-dev builds: beside the checkout's own builds (its
    /// flags differ, and would rebuild everything there each time).
    fn target_dir(&mut self) -> PathBuf {
        if let Some(t) = &self.target_dir {
            return t.clone();
        }
        // The checkout's own target directory (its .cargo/config.toml may
        // name one), as the dev shell's cargo sees it.
        let env = self.cargo_env().ok();
        let t = std::env::var_os("AIZOME_DEV_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| {
            let mut cmd = Command::new("cargo");
            if let Some(env) = &env {
                cmd.env_clear().envs(env);
            }
            let out = cmd
                .args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"])
                .arg(self.checkout.join("Cargo.toml"))
                .current_dir(&self.checkout)
                .stderr(Stdio::null())
                .output()
                .ok()
                .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok());
            out.and_then(|m| m["target_directory"].as_str().map(PathBuf::from))
                .unwrap_or_else(|| self.checkout.join("target"))
                .join("aizome-dev")
        });
        self.target_dir = Some(t.clone());
        t
    }

    fn out_dir(&mut self) -> PathBuf {
        let mut d = self.target_dir();
        if self.core["explicit_target"].as_bool().unwrap_or(false) {
            d = d.join(self.core_str("target"));
        }
        d.join(self.profile_dir())
    }

    /// The cargo command for one part.
    fn cargo_args(&self, what: &What) -> Vec<String> {
        let mut a: Vec<String> = vec!["build".into(), "--manifest-path".into()];
        a.push(self.checkout.join("Cargo.toml").to_string_lossy().into_owned());
        if self.core["explicit_target"].as_bool().unwrap_or(false) {
            a.extend(["--target".into(), self.core_str("target")]);
        }
        match self.profile().as_str() {
            "dev" => {}
            "release" => a.push("--release".into()),
            p => a.extend(["--profile".into(), p.into()]),
        }
        match what {
            What::Logic => {
                for p in self.logic_packages() {
                    a.extend(["-p".into(), p]);
                }
                if !self.optimised && self.profile() != "dev" {
                    // The logic alone compiles quicker; the core and every
                    // dependency stay exactly as the session's.
                    let p = self.profile();
                    for c in ["opt-level=1", "codegen-units=64", "incremental=true"] {
                        a.extend(["--config".into(), format!("profile.{p}.package.aizome-comp-logic.{c}")]);
                    }
                }
            }
            w => a.extend(["-p".into(), w.package()]),
        }
        a
    }

    fn print_env(&mut self) {
        println!("session core: {}", serde_json::to_string_pretty(&self.core).unwrap_or_default());
        match &self.env {
            Some(e) => println!("package build: {}", serde_json::to_string_pretty(e).unwrap_or_default()),
            None => println!("package build: none recorded (the session doesn't run from a package)"),
        }
        println!("checkout: {}", self.checkout.display());
        println!("target directory: {}", self.target_dir().display());
        for w in [What::Logic, What::Shell] {
            println!("{}: cargo {}", w.package(), self.cargo_args(&w).join(" "));
        }
        match self.cargo_env() {
            Ok(env) => {
                println!("RUSTFLAGS={}", env.get("RUSTFLAGS").cloned().unwrap_or_default());
                println!("RUSTC_WRAPPER={}", env.get("RUSTC_WRAPPER").cloned().unwrap_or_default());
            }
            Err(e) => println!("toolchain: {e}"),
        }
    }

    /// Builds one part; the file it built, and whether it changed.
    fn build(&mut self, what: &What) -> Result<(PathBuf, bool), String> {
        let env = self.cargo_env()?;
        let target_dir = self.target_dir();
        let out = self.out_dir().join(match what {
            What::Logic => "libaizome_comp_logic.so".to_string(),
            w => w.package(),
        });
        let before = mtime(&out);
        if *what == What::Logic {
            self.check_sidestep();
        }
        let args = self.cargo_args(what);
        say(format!("building {}", what.package()));
        let started = Instant::now();
        let status = Command::new("cargo")
            .args(&args)
            .env_clear()
            .envs(&env)
            .env("CARGO_TARGET_DIR", &target_dir)
            .current_dir(&self.checkout)
            .status()
            .map_err(|e| format!("cargo: {e}"))?;
        if !status.success() {
            return Err(format!("building {} failed", what.package()));
        }
        let changed = mtime(&out) != before;
        say(format!(
            "built {} in {:.1} s{}",
            what.package(),
            started.elapsed().as_secs_f64(),
            if changed { "" } else { " (unchanged)" }
        ));
        Ok((out, changed))
    }

    fn build_and_reload(&mut self, what: &What, always: bool) -> Result<(), String> {
        let (path, changed) = self.build(what)?;
        if !changed && !always {
            return Ok(());
        }
        match what {
            What::Logic => self.reload_logic(&path),
            What::Shell => reload(Request::Reload {
                logic: false,
                shell: false,
                from: None,
                shell_from: Some(path.to_string_lossy().into_owned()),
                check: false,
            }),
            What::App(app) => restart_app(app, &path, always),
        }
    }

    fn reload_logic(&mut self, path: &Path) -> Result<(), String> {
        let running = self.core_str("abi");
        if let Some(built) = read_abi(path)
            && built != running
        {
            // Which part of the core differs: the new build's compositor says.
            let theirs = self.built_core_info();
            let parts = theirs.as_ref().and_then(|t| t["abi_parts"].as_str()).unwrap_or("?").to_string();
            let mut what = String::new();
            if let Some(t) = &theirs {
                let differ = differing(&self.core_str("sources"), t["sources"].as_str().unwrap_or(""));
                if !differ.is_empty() {
                    what = format!(" The packages whose sources differ: {}.", differ.join(", "));
                }
            }
            return Err(format!(
                "the logic was built against another compositor core than the session's \
                 (session: {}; this build: {parts}).{what} The core changed (its sources, Sidestep, \
                 the lock file, the toolchain or the flags): that runs at the next login",
                self.core_str("abi_parts"),
            ));
        }
        reload(Request::Reload {
            logic: true,
            shell: false,
            from: Some(path.to_string_lossy().into_owned()),
            shell_from: None,
            check: false,
        })
    }

    /// The build info of the compositor core aizome-dev just built.
    fn built_core_info(&mut self) -> Option<Value> {
        let dir = self.out_dir();
        let env = self.cargo_env().ok()?;
        let rustc = find_in(env.get("PATH")?, "rustc")?;
        let sysroot = Command::new(rustc).args(["--print", "sysroot"]).output().ok()?;
        let sysroot = String::from_utf8_lossy(&sysroot.stdout).trim().to_string();
        let std_dir = Path::new(&sysroot).join("lib/rustlib").join(self.core_str("target")).join("lib");
        let out = Command::new(dir.join("aizome"))
            .arg("--build-info")
            .env("LD_LIBRARY_PATH", std::env::join_paths([dir.clone(), dir.join("deps"), std_dir]).ok()?)
            .output()
            .ok()?;
        serde_json::from_slice(&out.stdout).ok()
    }

    /// Whether the checkout's Sidestep is the session's: a word when it
    /// isn't, as a logic built against it wouldn't load.
    fn check_sidestep(&self) {
        let Some(want) = self.env.as_ref().and_then(|e| e["sidestep_rev"].as_str()) else { return };
        let dir = self.checkout.join("sidestep");
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let Some(have) = git(&["rev-parse", "HEAD"]) else { return };
        let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
        if have != want || dirty {
            say(format!(
                "the session was built against Sidestep {want}, but {} is at {have}{}: the logic \
                 won't load until they match (`git -C {} switch --detach {want}`, or a worktree there)",
                dir.display(),
                if dirty { " with changes" } else { "" },
                dir.display()
            ));
        }
    }

    /// Rebuilds and reloads on every save.
    fn watch(&mut self) -> Result<(), String> {
        say(format!("watching {} (Ctrl+C to stop)", self.checkout.display()));
        // Built once first, so the first save is quick.
        for w in [What::Logic, What::Shell] {
            if let Err(e) = self.build(&w) {
                say(e);
            }
        }
        let mut seen = snapshot(&self.checkout);
        say("ready: save a file to reload what it changes");
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let now = snapshot(&self.checkout);
            if now == seen {
                continue;
            }
            // Let a burst of saves settle.
            let mut settled = now;
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let again = snapshot(&self.checkout);
                if again == settled {
                    break;
                }
                settled = again;
            }
            let changed = changed_files(&seen, &settled);
            seen = settled;
            let plan = plan(&changed, &self.checkout);
            if plan.is_empty() {
                continue;
            }
            for p in &changed {
                say(format!("changed: {}", p.display()));
            }
            for w in plan {
                let r = match &w {
                    What::App(app) => {
                        // Restarted only if it runs (or was edited and runs
                        // from elsewhere: then into this build).
                        self.build(&w).and_then(|(path, built)| {
                            let edited = changed.iter().any(|c| c.starts_with(Path::new("apps").join(app)));
                            if built || edited { restart_app(app, &path, false) } else { Ok(()) }
                        })
                    }
                    w => self.build_and_reload(w, false),
                };
                if let Err(e) = r {
                    say(e);
                }
            }
        }
    }
}

/// What a set of changed files (relative to the checkout) needs: the
/// logic for anything it's built from, the shell likewise, an app for its
/// own sources and the shared crates it runs on.
fn plan(changed: &[PathBuf], checkout: &Path) -> BTreeSet<What> {
    let mut out = BTreeSet::new();
    for p in changed {
        let s = p.to_string_lossy();
        let crate_dir = p.components().nth(1).map(|c| c.as_os_str().to_string_lossy().into_owned());
        if s.starts_with("apps/") {
            if let Some(app) = crate_dir {
                out.insert(What::App(app));
            }
            continue;
        }
        match crate_dir.as_deref() {
            Some("aizome-comp-logic") => {
                out.insert(What::Logic);
            }
            Some("aizome-shell") | Some("aizome-shell-proto") => {
                out.insert(What::Shell);
            }
            Some("aizome-comp") if s.contains("/tests/") || s.contains("/examples/") => {}
            _ => {
                // Shared: the core, the kit, Sidestep, the lock file. The
                // logic (a core change is refused, and says so), the shell,
                // and the apps that run from this checkout.
                out.insert(What::Logic);
                out.insert(What::Shell);
                for app in running_checkout_apps(checkout) {
                    out.insert(What::App(app));
                }
            }
        }
    }
    out
}

/// Apps running from this checkout's builds (aizome-dev's or its own).
fn running_checkout_apps(checkout: &Path) -> Vec<String> {
    let mut apps = BTreeSet::new();
    for (_, exe) in processes() {
        if !exe.starts_with(checkout) && !exe.to_string_lossy().contains("/aizome-dev/") {
            continue;
        }
        if let Some(name) = exe.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("aizome-"))
            && checkout.join("apps").join(name).is_dir()
        {
            apps.insert(name.to_string());
        }
    }
    apps.into_iter().collect()
}

/// This user's processes and their programs.
fn processes() -> Vec<(u32, PathBuf)> {
    let me = my_uid();
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return out };
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        use std::os::unix::fs::MetadataExt;
        if e.metadata().map(|m| m.uid()).ok() != Some(me) {
            continue;
        }
        if let Ok(exe) = std::fs::read_link(e.path().join("exe")) {
            let exe = PathBuf::from(exe.to_string_lossy().trim_end_matches(" (deleted)"));
            out.push((pid, exe));
        }
    }
    out
}

fn my_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(u32::MAX)
}

/// Closes an app's windows (asking, as its close button does), waits for
/// it to go, and starts the new build in its place. `always`: start it
/// even if it wasn't running.
fn restart_app(app: &str, bin: &Path, always: bool) -> Result<(), String> {
    let name = format!("aizome-{app}");
    let mut c = Client::connect().map_err(|e| e.to_string())?;
    let windows = match c.request(&Request::Windows).map_err(|e| e.to_string())? {
        Response::Windows { windows } => windows,
        r => return Err(format!("{r:?}")),
    };
    let exe_of = |pid: i32| std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    let mut pids = BTreeSet::new();
    for w in &windows {
        let Some(pid) = w.pid else { continue };
        let is_it = exe_of(pid)
            .is_some_and(|e| e.file_name().is_some_and(|n| n.to_string_lossy().trim_end_matches(" (deleted)") == name));
        if is_it {
            pids.insert(pid);
            let _ = c.request(&Request::Close { id: Some(w.id) });
        }
    }
    if pids.is_empty() && !always {
        return Ok(());
    }
    if !pids.is_empty() {
        say(format!("closing {name} (pid {pids:?})"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while pids.iter().any(|p| Path::new(&format!("/proc/{p}")).exists()) {
            if Instant::now() > deadline {
                return Err(format!(
                    "{name} didn't quit (it may be asking to save): close it, then run {}",
                    bin.display()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    match c
        .request(&Request::Spawn { argv: vec![bin.to_string_lossy().into_owned()], cwd: None, in_desk_folder: true })
        .map_err(|e| e.to_string())?
    {
        Response::Error { message } => Err(message),
        _ => {
            say(format!("started {}", bin.display()));
            Ok(())
        }
    }
}

fn reload(req: Request) -> Result<(), String> {
    let mut c = Client::connect().map_err(|e| e.to_string())?;
    match c.request(&req).map_err(|e| e.to_string())? {
        Response::Reload { report } => print_report(&report),
        Response::Error { message } => Err(message),
        r => Err(format!("{r:?}")),
    }
}

fn print_report(r: &ReloadReport) -> Result<(), String> {
    let mut failed = None;
    for (what, part) in [("logic", &r.logic), ("shell", &r.shell), ("core", &r.core)] {
        if part.action == "skipped" || (what == "core" && part.action == "same") {
            continue;
        }
        let line = if part.detail.is_empty() {
            format!("{what}: {}", part.action)
        } else {
            format!("{what}: {} — {}", part.action, part.detail)
        };
        if matches!(part.action.as_str(), "failed" | "refused") {
            failed = Some(line.clone());
        }
        say(line);
    }
    failed.map_or(Ok(()), Err)
}

/// The packages whose source hashes differ between two `name=hash …` lists.
fn differing(a: &str, b: &str) -> Vec<String> {
    let parse = |s: &str| -> BTreeMap<String, String> {
        s.split_whitespace().filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.into(), v.into())).collect()
    };
    let (a, b) = (parse(a), parse(b));
    let mut names: BTreeSet<&String> = a.keys().collect();
    names.extend(b.keys());
    names.into_iter().filter(|n| a.get(*n) != b.get(*n)).cloned().collect()
}

/// The ABI mark a logic library carries.
fn read_abi(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mark = b"AIZOME-LOGIC-ABI:";
    let at = bytes.windows(mark.len()).position(|w| w == mark)? + mark.len();
    let abi = bytes.get(at..at + 64)?;
    Some(String::from_utf8_lossy(abi).into_owned())
}

fn find_in(path: &str, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path).map(|d| d.join(name)).find(|p| p.is_file())
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Every source file's modification time, relative to the checkout.
fn snapshot(checkout: &Path) -> BTreeMap<PathBuf, SystemTime> {
    let mut out = BTreeMap::new();
    for top in ["crates", "apps", "sidestep/crates", "Cargo.toml", "Cargo.lock"] {
        walk(checkout, &checkout.join(top), &mut out);
    }
    out
}

fn walk(root: &Path, p: &Path, out: &mut BTreeMap<PathBuf, SystemTime>) {
    let Ok(meta) = std::fs::metadata(p) else { return };
    if meta.is_file() {
        if let (Ok(t), Ok(rel)) = (meta.modified(), p.strip_prefix(root)) {
            out.insert(rel.to_path_buf(), t);
        }
        return;
    }
    let Ok(dir) = std::fs::read_dir(p) else { return };
    for e in dir.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" || name.ends_with('~') || name.ends_with(".swp") {
            continue;
        }
        walk(root, &e.path(), out);
    }
}

fn changed_files(a: &BTreeMap<PathBuf, SystemTime>, b: &BTreeMap<PathBuf, SystemTime>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = b.iter().filter(|(k, v)| a.get(*k) != Some(*v)).map(|(k, _)| k.clone()).collect();
    out.extend(a.keys().filter(|k| !b.contains_key(*k)).cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_map_to_what_reloads() {
        let checkout = Path::new("/nonexistent");
        let p = |s: &[&str]| plan(&s.iter().map(PathBuf::from).collect::<Vec<_>>(), checkout);
        assert_eq!(p(&["crates/aizome-comp-logic/src/wm/mod.rs"]), BTreeSet::from([What::Logic]));
        assert_eq!(p(&["crates/aizome-shell/src/shell.rs"]), BTreeSet::from([What::Shell]));
        assert_eq!(p(&["apps/hako/src/main.rs"]), BTreeSet::from([What::App("hako".into())]));
        assert_eq!(p(&["crates/aizome-kit/src/app.rs"]), BTreeSet::from([What::Logic, What::Shell]));
        assert_eq!(p(&["crates/aizome-comp/tests/reload.rs"]), BTreeSet::new());
    }

    #[test]
    fn names_the_packages_that_differ() {
        assert_eq!(
            differing("aizome-ipc=aaa sidestep-ui=bbb", "aizome-ipc=aaa sidestep-ui=ccc sidestep-engine=ddd"),
            ["sidestep-engine", "sidestep-ui"]
        );
    }
}
