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

/// User services may have no XDG_SESSION_ID. logind's `auto` then selects
/// the user's display session, as Aizome's own power controls do.
pub(crate) fn logout_command(session: Option<&str>) -> Command {
    let mut command = Command::new("loginctl");
    command.args(["terminate-session", session.filter(|id| !id.is_empty()).unwrap_or("auto")]);
    command
}

#[test]
fn sign_out_selects_the_display_session_when_no_session_id_is_exported() {
    for (session, expected) in [(None, "auto"), (Some(""), "auto"), (Some("7"), "7")] {
        let command = logout_command(session);
        assert_eq!(command.get_program(), "loginctl");
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["terminate-session", expected]);
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Install a Nixpkgs package into the current user's profile.
    InstallForMe { attr: String, unfree: bool },
    /// Remove a package from the current user's profile.
    RemoveForMe { name: String },
    /// Install or remove for everyone; `what` names it for the dialog.
    ChangeSystem { change: SystemChange, what: String, adding: bool },
    /// Update some of a flake's inputs.
    UpdateFlake { inputs: Vec<String> },
    /// Update the distro, including migration from its predecessor repositories.
    UpdateSekka { inputs: Vec<String> },
    /// Update root's channels.
    UpdateChannels,
    /// Make a flake input follow another branch of its repository, and
    /// update it to that branch's newest commit.
    FollowBranch { input: String, branch: String },
    /// Update Sekka early (from its main branch, as soon as changes are
    /// merged) or, `on: false`, from the nightly again.
    Early { on: bool },
    /// Change some of the system's own choices (its desktops, its login
    /// screen) to these values, each in full; `what` names them.
    Choose { choices: Vec<(String, Vec<String>)>, what: String },
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
            Operation::UpdateFlake { .. } | Operation::UpdateChannels => "Updating your system".to_owned(),
            Operation::UpdateSekka { .. } => "Updating Sekka".to_owned(),
            Operation::FollowBranch { input, branch } => format!("Following {branch} for {input}"),
            Operation::Early { on: true } => "Turning on early updates".to_owned(),
            Operation::Early { on: false } => "Going back to nightly updates".to_owned(),
            Operation::Choose { what, .. } => format!("Changing {what}"),
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
            Operation::UpdateSekka { .. } => "Update prepared; checking completion".to_owned(),
            Operation::UpdateFlake { .. } | Operation::UpdateChannels => {
                "Update prepared; checking completion".to_owned()
            }
            Operation::FollowBranch { input, branch } => format!("{input} follows {branch} now"),
            Operation::Early { on: true } => "Sekka updates early now".to_owned(),
            Operation::Early { on: false } => "Sekka updates from the nightly now".to_owned(),
            Operation::Choose { what, .. } => format!("{} changed", capitalized(what)),
            Operation::Rollback { generation } => format!("Generation {generation} is running"),
            Operation::Clean { .. } => "The store is tidy".to_owned(),
        }
    }

    /// What to say when it worked, given what the helper reported.
    pub fn outcome(&self, out: &str) -> String {
        let report: serde_json::Value = serde_json::from_str(out.trim()).unwrap_or_default();
        if self.is_system()
            && let Ok(completion) =
                serde_json::from_value::<yukimi_system::completion::Completion>(report["completion"].clone())
        {
            let title = completion.title();
            return match self {
                Operation::Rollback { .. } if report["configuration"] == false => format!(
                    "{title}. This generation's configuration wasn't kept, so your next change builds on the newer one"
                ),
                Operation::Rollback { .. } if report["left"].as_array().is_some_and(|l| !l.is_empty()) => {
                    format!("{title}. Files you changed by hand since were left as they are")
                }
                _ => title.into(),
            };
        }
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

    pub fn is_update(&self) -> bool {
        matches!(
            self,
            Operation::UpdateSekka { .. }
                | Operation::UpdateFlake { .. }
                | Operation::UpdateChannels
                | Operation::Early { .. }
        )
    }

    /// What the helper is asked to do, after `pkexec yukimi-helper`: the
    /// same arguments the GTK Yukimi gives it. `lock` is the lock file an
    /// update got ready as this user, if it did.
    pub fn helper_args(&self, lock: Option<&Path>) -> Vec<String> {
        let mut a: Vec<String> = Vec::new();
        let mut push = |items: &[&str]| a.extend(items.iter().map(|s| (*s).to_owned()));
        match self {
            Operation::InstallForMe { .. } | Operation::RemoveForMe { .. } => {}
            Operation::ChangeSystem { change, .. } => {
                push(&["change"]);
                if let Some(packages) = &change.packages {
                    push(&["--packages", &packages.join(",")]);
                }
                if let Some(programs) = &change.programs {
                    push(&["--programs", &programs.join(",")]);
                }
                if let Some((setting, ids)) = &change.applications {
                    push(&["--setting", setting, "--applications", &ids.join(",")]);
                }
                if !change.unlist_system.is_empty() {
                    push(&["--unlist-system", &change.unlist_system.join(",")]);
                }
                if let Some((user, attrs)) = &change.unlist_user {
                    push(&["--unlist-user", &format!("{user}:{}", attrs.join(","))]);
                }
                push(&["--mode", "auto"]);
            }
            Operation::UpdateFlake { inputs } => {
                push(&["change", "--update", &inputs.join(","), "--mode", "auto"]);
                if let Some(lock) = lock {
                    push(&["--lock", &lock.to_string_lossy()]);
                }
            }
            Operation::UpdateSekka { inputs } => {
                push(&["change", "--sekka", "--mode", "auto"]);
                if !inputs.is_empty() {
                    push(&["--update", &inputs.join(",")]);
                }
            }
            Operation::UpdateChannels => push(&["change", "--channels", "--mode", "auto"]),
            Operation::FollowBranch { input, branch } => {
                push(&["change", "--follow", &format!("{input}={branch}"), "--mode", "auto"]);
            }
            Operation::Early { on } => {
                let branch = if *on { "sekka=main" } else { "sekka=nightly" };
                push(&["change", "--sekka", "--follow", branch, "--mode", "auto"]);
            }
            Operation::Choose { choices, .. } => {
                push(&["change"]);
                for (setting, values) in choices {
                    push(&["--choose", &format!("{setting}={}", values.join(","))]);
                }
                push(&["--mode", "auto"]);
            }
            Operation::Rollback { generation } => push(&["rollback", &generation.to_string()]),
            Operation::Clean { older_than_days } => {
                push(&["clean"]);
                if let Some(days) = older_than_days {
                    push(&["--older-than-days", &days.to_string()]);
                }
            }
        }
        a
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
            Operation::UpdateFlake { inputs } => update_steps(self, model, inputs),
            _ => vec![root_step(self.helper_args(None))],
        }
    }
}

/// `what` with its first letter a capital: "The login screen".
fn capitalized(what: &str) -> String {
    let mut chars = what.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A step that runs the helper as root with `args`, once it is found.
fn root_step(args: Vec<String>) -> Step {
    Box::new(move || {
        let mut command = helper()?;
        command.args(args);
        Ok(Some(Run { command, as_root: true }))
    })
}

/// Updating a flake: the new lock file and its new sources, got ready as
/// this user, then the helper. Without a cache directory to get it ready
/// in, the helper fetches everything itself.
fn update_steps(operation: &Operation, model: &Model, inputs: &[String]) -> Vec<Step> {
    let flake = model.setup.flake();
    let current = model.setup.dir.join("flake.lock");
    let Some(dir) = crate::paths::cache_dir() else {
        return vec![root_step(operation.helper_args(None))];
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
    vec![lock_step, fetch_step, root_step(operation.helper_args(Some(&lock)))]
}

/// The polkit action Yukimi's helper runs under: its prompt says what
/// Yukimi is asking for.
pub const ACTION: &str = "io.github.bitemyapp.Yukimi.change-system";

/// The policy Sekka's Yukimi package installs, which names its helper.
const POLICY: &str = "io.github.bitemyapp.Yukimi.policy";

/// Where polkit finds actions: NixOS links them into the system profile.
const ACTION_DIRS: [&str; 3] =
    ["/run/current-system/sw/share/polkit-1/actions", "/etc/polkit-1/actions", "/usr/share/polkit-1/actions"];

/// What to say when there is no helper to ask.
pub const NO_HELPER: &str = "Yukimi's helper isn't installed, so nothing can be changed for everyone. Sekka installs \
                             it with Yukimi's NixOS module (programs.yukimi.enable = true). Nothing was changed.";

/// Sekka's installed `yukimi-helper`: the program Yukimi's polkit action
/// names (`org.freedesktop.policykit.exec.path`), so that pkexec asks under
/// that action as it does for the GTK Yukimi; or else the one the system
/// profile links to.
pub fn find_helper() -> Result<PathBuf, String> {
    let dirs = ACTION_DIRS.map(Path::new);
    find_helper_in(&dirs, Path::new("/run/current-system/sw/bin/yukimi-helper"))
}

/// [`find_helper`], looking for the policy in `dirs`, then at `fallback`.
pub fn find_helper_in(dirs: &[&Path], fallback: &Path) -> Result<PathBuf, String> {
    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(dir.join(POLICY)) else { continue };
        if let Some(path) = exec_path(&text, ACTION).map(PathBuf::from)
            && path.is_file()
        {
            return Ok(path);
        }
    }
    // pkexec matches the action by the path it is given: the store path
    // the system profile's link leads to is the one the policy names.
    match std::fs::canonicalize(fallback) {
        Ok(path) if path.is_file() => Ok(path),
        _ => Err(NO_HELPER.to_owned()),
    }
}

/// The program a polkit policy's `action` runs, from its
/// `org.freedesktop.policykit.exec.path` annotation.
fn exec_path(policy: &str, action: &str) -> Option<String> {
    let start = policy.find(&format!("<action id=\"{action}\""))?;
    let body = &policy[start..];
    let body = &body[..body.find("</action>")?];
    let key = "<annotate key=\"org.freedesktop.policykit.exec.path\">";
    let value = &body[body.find(key)? + key.len()..];
    let value = value[..value.find("</annotate>")?].trim();
    let value = value.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&");
    value.starts_with('/').then_some(value)
}

/// `pkexec yukimi-helper`. On NixOS only the wrapper in /run/wrappers is
/// setuid; the polkit package's own pkexec, which can come first in the
/// search path, refuses to run.
fn helper() -> Result<Command, String> {
    let helper = find_helper()?;
    let wrapper = Path::new("/run/wrappers/bin/pkexec");
    let mut c = Command::new(if wrapper.exists() { wrapper } else { Path::new("pkexec") });
    // A graphical application has no controlling terminal. Let the job
    // recover a missing desktop agent instead of pkexec trying /dev/tty.
    c.arg("--disable-internal-agent").arg(helper).env("LC_ALL", "C");
    Ok(c)
}

/// Open a terminal running `command`, with whichever terminal is installed:
/// the desktop's choice, or foot, Aizome's.
pub fn open_terminal(command: &[String]) -> Result<(), String> {
    // (program, arguments before the command)
    let terminals: [(&str, &[&str]); 11] = [
        ("xdg-terminal-exec", &[]),
        ("foot", &[]),
        ("kgx", &["--"]),
        ("ptyxis", &["--"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
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
    fn prepared_rollback_does_not_claim_it_is_running_or_hide_configuration_warning() {
        let op = Operation::Rollback { generation: 3 };
        let text = op.outcome(r#"{"configuration":false,"completion":{"action":"reboot","reasons":[]}}"#);
        assert!(text.starts_with("Restart to finish updating"));
        assert!(text.contains("configuration wasn't kept"));
        assert!(!text.contains("is running"));
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
        assert_eq!(
            change.helper_args(None),
            ["change", "--packages", "htop,ripgrep", "--unlist-user", "alice:thunderbird", "--mode", "auto"]
        );
        assert_eq!(change.steps(&model).len(), 1);
        assert_eq!(change.title(), "Installing htop for everyone");
        assert!(Operation::Rollback { generation: 3 }.is_system());
        assert_eq!(
            Operation::UpdateSekka { inputs: vec![] }.helper_args(None),
            ["change", "--sekka", "--mode", "auto"]
        );
        assert_eq!(Operation::Rollback { generation: 7 }.helper_args(None), ["rollback", "7"]);
        assert_eq!(
            Operation::Clean { older_than_days: Some(30) }.helper_args(None),
            ["clean", "--older-than-days", "30"]
        );
        assert_eq!(
            Operation::FollowBranch { input: "sekka".into(), branch: "nightly".into() }.helper_args(None),
            ["change", "--follow", "sekka=nightly", "--mode", "auto"]
        );
        assert_eq!(
            Operation::Early { on: true }.helper_args(None),
            ["change", "--sekka", "--follow", "sekka=main", "--mode", "auto"]
        );
        assert_eq!(Operation::Early { on: false }.helper_args(None)[3], "sekka=nightly");
        assert!(Operation::Early { on: true }.is_update());
        let choose = Operation::Choose {
            choices: vec![
                ("calamares.desktops".into(), vec!["aizome".into(), "tatami".into()]),
                ("calamares.loginScreen".into(), vec!["aizome".into()]),
            ],
            what: "the desktops and the login screen".into(),
        };
        assert_eq!(
            choose.helper_args(None),
            [
                "change",
                "--choose",
                "calamares.desktops=aizome,tatami",
                "--choose",
                "calamares.loginScreen=aizome",
                "--mode",
                "auto"
            ]
        );
        assert!(choose.is_system() && !choose.is_update());
        assert_eq!(choose.done(), "The desktops and the login screen changed");
        let try_it = try_command("nixpkgs", "cowsay", "cowsay", false);
        assert_eq!(try_it, ["env", "nix", "shell", "nixpkgs#cowsay", "--command", "cowsay"]);
    }

    #[test]
    fn updates_get_ready_before_the_helper() {
        let model = Model::default();
        let update = Operation::UpdateFlake { inputs: vec!["nixpkgs".into()] };
        let steps = update.steps(&model);
        if crate::paths::cache_dir().is_some() {
            assert_eq!(steps.len(), 3);
        }
        let args = update.helper_args(Some(Path::new("/tmp/update.lock")));
        assert!(args.windows(2).any(|w| w == ["--update", "nixpkgs"]), "{args:?}");
        assert!(args.windows(2).any(|w| w == ["--mode", "auto"]), "{args:?}");
        assert!(args.windows(2).any(|w| w == ["--lock", "/tmp/update.lock"]), "{args:?}");
    }

    #[test]
    fn the_helper_is_the_one_the_policy_names() {
        let dir = std::env::temp_dir().join(format!("aizome-yukimi-policy-{}", std::process::id()));
        let (actions, bin) = (dir.join("actions"), dir.join("bin"));
        std::fs::create_dir_all(&actions).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let helper = bin.join("yukimi-helper");
        std::fs::write(&helper, "#!/bin/sh\n").unwrap();
        // No policy and no helper: say so.
        let none = find_helper_in(&[actions.as_path()], &dir.join("missing")).unwrap_err();
        assert!(none.contains("programs.yukimi.enable"));
        // The link the system profile has leads to the helper.
        std::os::unix::fs::symlink(&helper, dir.join("link")).unwrap();
        assert_eq!(find_helper_in(&[actions.as_path()], &dir.join("link")).unwrap(), helper.canonicalize().unwrap());
        // The policy's helper comes first.
        let policy = format!(
            r#"<policyconfig>
  <action id="io.github.bitemyapp.Yukimi.other"><annotate key="org.freedesktop.policykit.exec.path">/nowhere</annotate></action>
  <action id="io.github.bitemyapp.Yukimi.change-system">
    <description>Change the system's packages, version or storage</description>
    <annotate key="org.freedesktop.policykit.exec.path">{}</annotate>
  </action>
</policyconfig>"#,
            helper.display()
        );
        std::fs::write(actions.join(POLICY), policy).unwrap();
        assert_eq!(find_helper_in(&[actions.as_path()], &dir.join("missing")).unwrap(), helper);
        let _ = std::fs::remove_dir_all(dir);
    }
}
