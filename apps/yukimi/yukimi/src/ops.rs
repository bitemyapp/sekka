// SPDX-License-Identifier: MIT OR Apache-2.0
//! What Yukimi can do to a system, as the commands that do it.
//!
//! Changes for the current user run Nix directly (`nix profile`). Changes
//! for everyone go through `yukimi-helper`, started with `pkexec` so polkit
//! asks for an administrator password; it edits the configuration, builds
//! the new system and switches to it, and puts the configuration back if
//! the build fails. Updating a flake first gets the update ready as the
//! current user (the new lock file, and every new source in the store), so
//! that the helper's build downloads nothing again. Every step reports
//! Nix's progress on its error output.
use std::path::{Path, PathBuf};
use std::process::Command;

use yukimi_system::nix;
use yukimi_system::updates;

use crate::model::Model;

/// When a new system takes over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Now.
    Switch,
    /// At the next restart.
    Boot,
}

impl Mode {
    fn arg(self) -> &'static str {
        match self {
            Mode::Switch => "switch",
            Mode::Boot => "boot",
        }
    }
}

/// A change to what the system installs: each part is left as it is when
/// `None` (or empty).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemChange {
    /// What `yukimi.nix` installs, in full.
    pub packages: Option<Vec<String>>,
    pub programs: Option<Vec<String>>,
    /// A system catalog's setting, and the application ids it lists, in full.
    pub applications: Option<(String, Vec<String>)>,
    /// Entries to take out of `environment.systemPackages`.
    pub unlist_system: Vec<String>,
    /// Entries to take out of a user's `packages`.
    pub unlist_user: Option<(String, Vec<String>)>,
}

#[derive(Clone, Debug)]
pub enum Operation {
    /// Install a Nixpkgs package into the current user's profile.
    InstallForMe { attr: String, unfree: bool },
    /// Remove a package from the current user's profile.
    RemoveForMe { name: String },
    /// Install or remove for everyone; `what` names it for the dialog.
    ChangeSystem { change: SystemChange, what: String, adding: bool },
    /// Update some of a flake's inputs.
    UpdateFlake { inputs: Vec<String>, mode: Mode },
    /// Update root's channels.
    UpdateChannels { mode: Mode },
    /// Make a flake input follow another branch of its repository, and
    /// update it to that branch's newest commit.
    FollowBranch { input: String, branch: String },
    /// Return the system to an earlier generation.
    Rollback { generation: u32 },
    /// Delete old system generations and collect garbage.
    Clean { older_than_days: Option<u32> },
}

/// A command a job runs, and whether it runs as root (through pkexec).
pub struct Run {
    pub command: Command,
    pub as_root: bool,
}

/// One step of a job, done in the job's own thread, in order: it can
/// prepare files, and gives the command to run next, if any.
pub type Step = Box<dyn FnOnce() -> Result<Option<Run>, String> + Send>;

fn step(run: Run) -> Step {
    Box::new(move || Ok(Some(run)))
}

impl Operation {
    /// What is happening, for the progress dialog's title.
    pub fn title(&self) -> String {
        match self {
            Operation::InstallForMe { attr, .. } => format!("Installing {attr} for you"),
            Operation::RemoveForMe { name } => format!("Removing {name}"),
            Operation::ChangeSystem { what, adding: true, .. } => format!("Installing {what} for everyone"),
            Operation::ChangeSystem { what, adding: false, .. } => format!("Removing {what}"),
            Operation::UpdateFlake { .. } | Operation::UpdateChannels { .. } => "Updating your system".to_owned(),
            Operation::FollowBranch { input, branch } => format!("Following {branch} for {input}"),
            Operation::Rollback { generation } => format!("Returning to generation {generation}"),
            Operation::Clean { .. } => "Cleaning up the store".to_owned(),
        }
    }

    /// What success looks like, for a toast.
    pub fn done(&self) -> String {
        match self {
            Operation::InstallForMe { attr, .. } => format!("{attr} is installed for you"),
            Operation::RemoveForMe { name } => format!("{name} is removed"),
            Operation::ChangeSystem { what, adding: true, .. } => format!("{what} is installed for everyone"),
            Operation::ChangeSystem { what, adding: false, .. } => format!("{what} is removed"),
            Operation::UpdateFlake { mode: Mode::Boot, .. } | Operation::UpdateChannels { mode: Mode::Boot } => {
                "Ready: it takes over at the next restart".to_owned()
            }
            Operation::UpdateFlake { .. } | Operation::UpdateChannels { .. } => "Your system is up to date".to_owned(),
            Operation::FollowBranch { input, branch } => format!("{input} follows {branch} now"),
            Operation::Rollback { generation } => format!("Generation {generation} is running"),
            Operation::Clean { .. } => "The store is tidy".to_owned(),
        }
    }

    /// What to say when it worked, given what the helper reported.
    pub fn outcome(&self, out: &str) -> String {
        let report: serde_json::Value = serde_json::from_str(out.trim()).unwrap_or_default();
        match self {
            Operation::Rollback { generation } if report["configuration"] == false => format!(
                "Generation {generation} is running. Its configuration wasn't kept, so your next change builds on \
                 the newer one"
            ),
            Operation::Rollback { generation } if report["left"].as_array().is_some_and(|l| !l.is_empty()) => {
                format!("Generation {generation} is running. Files you changed by hand since were left as they are")
            }
            Operation::Clean { .. } => match report["freed"].as_str() {
                Some(freed) if !freed.is_empty() => format!("The store is tidy: {freed}"),
                _ => self.done(),
            },
            _ => self.done(),
        }
    }

    /// Whether this changes the whole system (and asks for a password).
    pub fn is_system(&self) -> bool {
        !matches!(self, Operation::InstallForMe { .. } | Operation::RemoveForMe { .. })
    }

    /// The steps that do it.
    pub fn steps(&self, model: &Model) -> Vec<Step> {
        match self {
            Operation::InstallForMe { attr, unfree } => {
                let mut c = nix::command();
                c.args(["profile", "install", "--log-format", "internal-json", "-v"]);
                if *unfree {
                    c.env("NIXPKGS_ALLOW_UNFREE", "1").arg("--impure");
                }
                c.arg(format!("{}#{attr}", model.nixpkgs_ref()));
                vec![step(Run { command: c, as_root: false })]
            }
            Operation::RemoveForMe { name } => {
                let mut c = nix::command();
                c.args(["profile", "remove", "--log-format", "internal-json", "-v", name]);
                vec![step(Run { command: c, as_root: false })]
            }
            Operation::ChangeSystem { change, .. } => {
                let mut c = helper();
                c.arg("change");
                if let Some(packages) = &change.packages {
                    c.arg("--packages").arg(packages.join(","));
                }
                if let Some(programs) = &change.programs {
                    c.arg("--programs").arg(programs.join(","));
                }
                if let Some((setting, ids)) = &change.applications {
                    c.arg("--setting").arg(setting).arg("--applications").arg(ids.join(","));
                }
                if !change.unlist_system.is_empty() {
                    c.arg("--unlist-system").arg(change.unlist_system.join(","));
                }
                if let Some((user, attrs)) = &change.unlist_user {
                    c.arg("--unlist-user").arg(format!("{user}:{}", attrs.join(",")));
                }
                c.args(["--mode", "switch"]);
                vec![step(Run { command: c, as_root: true })]
            }
            Operation::UpdateFlake { inputs, mode } => update_steps(model, inputs, *mode),
            Operation::UpdateChannels { mode } => {
                let mut c = helper();
                c.args(["change", "--channels", "--mode", mode.arg()]);
                vec![step(Run { command: c, as_root: true })]
            }
            Operation::FollowBranch { input, branch } => {
                let mut c = helper();
                c.args(["change", "--follow", &format!("{input}={branch}"), "--mode", "switch"]);
                vec![step(Run { command: c, as_root: true })]
            }
            Operation::Rollback { generation } => {
                let mut c = helper();
                c.args(["rollback", &generation.to_string()]);
                vec![step(Run { command: c, as_root: true })]
            }
            Operation::Clean { older_than_days } => {
                let mut c = helper();
                c.arg("clean");
                if let Some(days) = older_than_days {
                    c.arg("--older-than-days").arg(days.to_string());
                }
                vec![step(Run { command: c, as_root: true })]
            }
        }
    }
}

/// Updating a flake: the new lock file and its new sources, got ready as
/// this user, then the helper. Without a cache directory to get it ready
/// in, the helper fetches everything itself.
fn update_steps(model: &Model, inputs: &[String], mode: Mode) -> Vec<Step> {
    let flake = model.setup.flake();
    let current = model.setup.dir.join("flake.lock");
    let mut helper_command = helper();
    helper_command.args(["change", "--update", &inputs.join(","), "--mode", mode.arg()]);
    let Some(dir) = crate::cache_dir() else {
        return vec![step(Run { command: helper_command, as_root: true })];
    };
    let (lock, sources) = (dir.join("update.lock"), dir.join("update-sources.json"));
    let lock_step: Step = {
        let (dir, lock, inputs) = (dir.clone(), lock.clone(), inputs.to_vec());
        Box::new(move || {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let _ = std::fs::remove_file(&lock);
            Ok(Some(Run { command: updates::lock_command(&flake, &inputs, &lock), as_root: false }))
        })
    };
    let fetch_step: Step = {
        let (lock, sources) = (lock.clone(), sources.clone());
        Box::new(move || {
            let current = std::fs::read_to_string(&current).map_err(|e| format!("{}: {e}", current.display()))?;
            let proposed = std::fs::read_to_string(&lock).map_err(|e| format!("{}: {e}", lock.display()))?;
            let fresh = updates::new_sources(&current, &proposed);
            if fresh.is_empty() {
                return Ok(None);
            }
            let json = serde_json::to_string(&fresh).map_err(|e| e.to_string())?;
            std::fs::write(&sources, json).map_err(|e| format!("{}: {e}", sources.display()))?;
            Ok(Some(Run { command: updates::fetch_command(&sources), as_root: false }))
        })
    };
    helper_command.arg("--lock").arg(&lock);
    vec![lock_step, fetch_step, step(Run { command: helper_command, as_root: true })]
}

/// `pkexec yukimi-helper`, the helper installed next to Yukimi. On NixOS
/// only the wrapper in /run/wrappers is setuid; the polkit package's own
/// pkexec, which can come first in the search path, refuses to run.
fn helper() -> Command {
    let wrapper = Path::new("/run/wrappers/bin/pkexec");
    let mut c = Command::new(if wrapper.exists() { wrapper } else { Path::new("pkexec") });
    c.arg(helper_path());
    c
}

pub fn helper_path() -> PathBuf {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("/run/current-system/sw/bin"));
    dir.join("yukimi-helper")
}

/// Open a terminal running `command`, with whichever terminal is installed.
pub fn open_terminal(command: &[String]) -> Result<(), String> {
    // (program, arguments before the command)
    let terminals: [(&str, &[&str]); 11] = [
        ("xdg-terminal-exec", &[]),
        ("kgx", &["--"]),
        ("ptyxis", &["--"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
        ("foot", &[]),
        ("ghostty", &["-e"]),
        ("kitty", &[]),
        ("alacritty", &["-e"]),
        ("xterm", &["-e"]),
    ];
    for (program, prefix) in terminals {
        if find_program(program) {
            return Command::new(program).args(prefix).args(command).spawn().map(|_| ()).map_err(|e| e.to_string());
        }
    }
    Err("No terminal was found".to_owned())
}

fn find_program(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| Path::new(&dir).join(name).is_file()))
        .unwrap_or(false)
}

/// The command that tries a package without installing it.
pub fn try_command(nixpkgs: &str, attr: &str, program: &str, unfree: bool) -> Vec<String> {
    let mut command = vec!["env".to_owned()];
    if unfree {
        command.push("NIXPKGS_ALLOW_UNFREE=1".to_owned());
    }
    command.extend(["nix", "shell"].map(str::to_owned));
    if unfree {
        command.push("--impure".to_owned());
    }
    command.push(format!("{nixpkgs}#{attr}"));
    command.push("--command".to_owned());
    if program.is_empty() {
        command.push(std::env::var("SHELL").unwrap_or_else(|_| "bash".to_owned()));
    } else {
        command.push(program.to_owned());
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(run: Run) -> Vec<String> {
        run.command.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn commands_for_each_operation() {
        let model =
            Model { nixpkgs: Some("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-source".into()), ..Model::default() };
        let install = Operation::InstallForMe { attr: "htop".into(), unfree: false };
        let run = install.steps(&model).remove(0)().unwrap().unwrap();
        assert!(!run.as_root);
        assert!(args(run).ends_with(&["path:/nix/store/0123456789abcdfghijklmnpqrsvwxyz-source#htop".to_owned()]));
        let change = Operation::ChangeSystem {
            change: SystemChange {
                packages: Some(vec!["htop".into(), "ripgrep".into()]),
                unlist_user: Some(("alice".into(), vec!["thunderbird".into()])),
                ..SystemChange::default()
            },
            what: "htop".into(),
            adding: true,
        };
        let run = change.steps(&model).remove(0)().unwrap().unwrap();
        assert!(run.as_root);
        assert_eq!(
            &args(run)[1..],
            ["change", "--packages", "htop,ripgrep", "--unlist-user", "alice:thunderbird", "--mode", "switch"]
        );
        assert_eq!(change.title(), "Installing htop for everyone");
        assert!(Operation::Rollback { generation: 3 }.is_system());
        let try_it = try_command("nixpkgs", "cowsay", "cowsay", false);
        assert_eq!(try_it, ["env", "nix", "shell", "nixpkgs#cowsay", "--command", "cowsay"]);
    }

    #[test]
    fn updates_get_ready_before_the_helper() {
        let model = Model::default();
        let steps = Operation::UpdateFlake { inputs: vec!["nixpkgs".into()], mode: Mode::Boot }.steps(&model);
        if crate::cache_dir().is_some() {
            assert_eq!(steps.len(), 3);
        }
        let helper = steps.into_iter().last().unwrap()().unwrap().unwrap();
        let args = args(helper);
        assert!(args.windows(2).any(|w| w == ["--update", "nixpkgs"]), "{args:?}");
        assert!(args.windows(2).any(|w| w == ["--mode", "boot"]), "{args:?}");
    }
}
