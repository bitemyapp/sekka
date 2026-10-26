//! Applications: which processes belong together, and what to call them.
//!
//! A process belongs with its parent, unless the parent is a process
//! manager (init, a systemd instance, the kernel's thread spawner, the
//! compositor or desktop shell), and with every other process in its systemd unit, unless
//! the unit is a manager's own. Aizome starts applications as units of
//! their own (`app-aizome-ID-RANDOM.scope`): entering a different such scope
//! starts a new group, even when another application opened it. Chromium and Electron move
//! their helpers into scopes of their own, so an application's processes
//! come together however it was started.
//!
//! A group is named for its window's application (the compositor's
//! windows), else its unit's application ID, else its program, as each
//! matches a launcher entry; otherwise for the program itself.
//!
//! Ported from Tatami's Activity (`apps.rs`).

use std::collections::{HashMap, HashSet};

use aizome_sys::apps::AppIndex;

use crate::model::Kind;

/// A name without NixOS's wrapper decoration (".firefox-wrapped"), whole
/// or cut short as the kernel's 15-character command names are.
pub fn tidy(name: &str) -> String {
    let name = name.strip_prefix('.').unwrap_or(name);
    ["-wrapped", "-wrappe", "-wrapp"].iter().find_map(|s| name.strip_suffix(s)).unwrap_or(name).to_owned()
}

/// A program's name as people know it, from its path.
pub fn program(path: &str) -> String {
    tidy(path.rsplit('/').next().unwrap_or(path))
}

/// What Chromium and Electron helpers do, from their `--type` switches.
fn chromium_role(args: &[String]) -> Option<String> {
    let value = |flag: &str| args.iter().find_map(|a| a.strip_prefix(flag));
    let role = match value("--type=")? {
        "renderer" if args.iter().any(|a| a == "--extension-process") => "Extension",
        "renderer" => "Renderer",
        "gpu-process" => "GPU process",
        "zygote" => "Zygote",
        "broker" => "Broker",
        "crashpad-handler" => "Crash reporter",
        "utility" => match value("--utility-sub-type=").unwrap_or("").split('.').next() {
            Some("network") => "Network service",
            Some("storage") => "Storage service",
            Some("audio") => "Audio service",
            Some("video_capture") => "Video capture",
            Some("data_decoder") => "Data decoder",
            _ => "Utility",
        },
        other => return Some(other.to_owned()),
    };
    Some(role.to_owned())
}

const INTERPRETERS: [&str; 12] =
    ["python", "python3", "node", "nodejs", "perl", "ruby", "bun", "deno", "lua", "luajit", "php", "java"];
const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "fish", "dash", "nu"];

/// A process's name and role: "chrome" and "Renderer", "python3" and
/// "server.py". Chromium rewrites its title into one argument, which is
/// split back into words.
pub fn name_and_role(comm: &str, args: &[String], exe: &str) -> (String, String) {
    let words: Vec<String>;
    let args = if args.len() == 1 && args[0].contains(" --") {
        words = args[0].split_whitespace().map(str::to_owned).collect();
        &words[..]
    } else {
        args
    };
    let first = args.first().map(String::as_str).unwrap_or("");
    let name = if first.is_empty() || first.contains(": ") {
        // Kernel threads have no arguments; daemons that set a title
        // ("sshd: callen@pts/0") are named by their program.
        if exe.is_empty() { tidy(comm) } else { program(exe) }
    } else {
        program(first.split_whitespace().next().unwrap_or(first))
    };
    let name = if name.is_empty() { comm.to_owned() } else { name };
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let role = if let Some(role) = chromium_role(args) {
        role
    } else if INTERPRETERS.contains(&base) || INTERPRETERS.contains(&name.as_str()) {
        let mut rest = args.iter().skip(1);
        let mut role = String::new();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "-m" | "-jar" => {
                    role = rest.next().map(|n| program(n)).unwrap_or_default();
                    break;
                }
                "-c" | "-e" => break,
                flag if flag.starts_with('-') => {}
                script => {
                    role = program(script);
                    break;
                }
            }
        }
        role
    } else if SHELLS.contains(&name.as_str()) {
        args.get(1)
            .filter(|a| !a.starts_with('-') && (a.contains('/') || a.ends_with(".sh")))
            .map(|a| program(a))
            .unwrap_or_default()
    } else {
        String::new()
    };
    (name, role)
}

/// Programs that only start another: a group is named for what they start.
fn launcher(name: &str) -> bool {
    SHELLS.contains(&name)
        || name.starts_with("start-")
        || matches!(
            name,
            "env"
                | "uwsm"
                | "systemd-run"
                | "dbus-run-session"
                | "setsid"
                | "nohup"
                | "timeout"
                | "xdg-open"
                | "gtk-launch"
                | "bwrap"
                | "steam-run"
                | "aizome-session"
        )
}

/// Process managers: their children are applications of their own.
fn manager(pid: u32, name: &str, kernel: bool) -> bool {
    pid <= 2
        || kernel
        || matches!(
            name,
            "systemd"
                | "init"
                | "aizome"
                | "aizome-shell"
                | "Hyprland"
                | "sway"
                | "niri"
                | "river"
                | "labwc"
                | "wayfire"
                | "kwin_wayland"
                | "gnome-shell"
                | "cosmic-comp"
                | "Xwayland"
                | "xwayland-satellite"
        )
}

/// What grouping needs to know of a process.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub pid: u32,
    pub ppid: u32,
    pub started: u64,
    pub uid: u32,
    pub kernel: bool,
    pub name: String,
    pub comm: String,
    pub exe: String,
    pub cgroup: String,
    /// The unit's application ID (`app-aizome-org.aizome.Sumi-12.scope`).
    pub app_id: Option<String>,
}

impl Node {
    fn unit(&self) -> &str {
        self.cgroup.rsplit('/').next().unwrap_or("")
    }

    /// A deliberate desktop launch, not an application's private helper scope.
    fn launch_scope(&self) -> bool {
        let unit = self.unit();
        unit.starts_with("app-aizome-") && unit.ends_with(".scope")
    }
}

struct Sets(Vec<usize>);

impl Sets {
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// The groups: indices into `nodes`, each group's first-started process
/// first, the groups in the order they started. The kernel's threads are
/// one group.
pub fn group(nodes: &[Node]) -> Vec<Vec<usize>> {
    let index: HashMap<u32, usize> = nodes.iter().enumerate().map(|(i, n)| (n.pid, i)).collect();
    let mut sets = Sets((0..nodes.len()).collect());
    let managers: Vec<bool> = nodes.iter().map(|n| manager(n.pid, &n.name, n.kernel)).collect();
    // A manager's unit holds what it started without systemd's help: those
    // are not one application.
    let managed: HashSet<&str> =
        nodes.iter().zip(&managers).filter(|(n, m)| **m && !n.kernel).map(|(n, _)| n.cgroup.as_str()).collect();
    let mut units: HashMap<&str, usize> = HashMap::new();
    let mut kernel = None;
    for (i, node) in nodes.iter().enumerate() {
        if node.kernel || node.pid == 2 {
            match kernel {
                Some(first) => sets.join(first, i),
                None => kernel = Some(i),
            }
            continue;
        }
        if let Some(&parent) = index.get(&node.ppid)
            && !managers[parent]
            // Opening another app does not make it part of the opener. Do
            // keep ordinary descendants and Chromium/Electron's own helper
            // scopes together: a changed cgroup alone is not an app boundary.
            && !(node.launch_scope() && node.cgroup != nodes[parent].cgroup)
        {
            sets.join(i, parent);
        }
        let cgroup = node.cgroup.as_str();
        if !cgroup.is_empty() && cgroup != "/" && !managed.contains(cgroup) && !node.unit().ends_with("init.scope") {
            match units.get(cgroup) {
                Some(&first) => sets.join(first, i),
                None => {
                    units.insert(cgroup, i);
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..nodes.len() {
        let root = sets.find(i);
        groups.entry(root).or_default().push(i);
    }
    let mut groups: Vec<Vec<usize>> = groups.into_values().collect();
    for g in &mut groups {
        g.sort_by_key(|&i| (nodes[i].started, nodes[i].pid));
    }
    groups.sort_by_key(|g| (nodes[g[0]].started, nodes[g[0]].pid));
    groups
}

/// A window, from the compositor.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub pid: u32,
    pub app_id: String,
    pub title: String,
    pub focused: bool,
}

/// The compositor's windows (`$AIZOME_SOCKET`), or none outside Aizome.
pub fn windows() -> Vec<Window> {
    let Ok(mut client) = aizome_ipc::Client::connect() else { return Vec::new() };
    let _ = client.stream().set_read_timeout(Some(std::time::Duration::from_millis(300)));
    match client.call(&aizome_ipc::Request::Windows) {
        Ok(aizome_ipc::Response::Windows { windows }) => windows
            .into_iter()
            .filter_map(|w| {
                let pid = u32::try_from(w.pid?).ok()?;
                Some(Window { pid, app_id: w.app_id, title: w.title, focused: w.focused })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// How a group shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub kind: Kind,
    pub name: String,
    pub app_id: Option<String>,
    pub detail: String,
    pub windows: usize,
    pub focused: bool,
}

/// What to call a group of `members`, and what it is.
pub fn identify(nodes: &[Node], members: &[usize], windows: &[Window], apps: Option<&AppIndex>, uid: u32) -> Identity {
    let first = &nodes[members[0]];
    if first.kernel || first.pid == 2 {
        return Identity {
            kind: Kind::Kernel,
            name: "Kernel".into(),
            app_id: None,
            detail: "The kernel's own threads".into(),
            windows: 0,
            focused: false,
        };
    }
    let pids: HashSet<u32> = members.iter().map(|&i| nodes[i].pid).collect();
    let own: Vec<&Window> = windows.iter().filter(|w| pids.contains(&w.pid)).collect();
    // The program it runs, past any launcher.
    let representative = members.iter().map(|&i| &nodes[i]).find(|n| !launcher(&n.name)).unwrap_or(first);
    let mine = members.iter().any(|&i| nodes[i].uid == uid);
    let unit = first.unit().to_owned();
    let detail = match own.as_slice() {
        [] if unit.ends_with(".service") && !unit.starts_with("run-") => unit.clone(),
        [] => String::new(),
        [only] => only.title.clone(),
        many => format!("{} windows", many.len()),
    };
    let entry = apps.and_then(|apps| {
        own.iter()
            .find_map(|w| apps.match_window(&w.app_id))
            .or_else(|| members.iter().find_map(|&i| nodes[i].app_id.as_deref()).and_then(|id| apps.match_window(id)))
            .or_else(|| {
                [representative.name.clone(), program(&representative.exe), tidy(&representative.comm)]
                    .iter()
                    .filter(|n| !n.is_empty())
                    .find_map(|n| apps.match_window(n))
            })
    });
    let is_app = !own.is_empty() || entry.is_some_and(|e| !e.no_display && !e.hidden);
    let kind = if is_app && mine {
        Kind::App
    } else if mine {
        Kind::Service
    } else {
        Kind::System
    };
    let name = match entry {
        Some(e) if kind == Kind::App => e.name.clone(),
        _ => representative.name.clone(),
    };
    Identity {
        kind,
        name: if name.is_empty() { representative.comm.clone() } else { name },
        app_id: entry.map(|e| e.id.clone()),
        detail,
        windows: own.len(),
        focused: own.iter().any(|w| w.focused),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(pid: u32, ppid: u32, name: &str, cgroup: &str) -> Node {
        Node {
            pid,
            ppid,
            started: u64::from(pid),
            uid: 1000,
            name: name.into(),
            comm: name.into(),
            cgroup: cgroup.into(),
            ..Default::default()
        }
    }

    #[test]
    fn names_and_roles() {
        assert_eq!(tidy(".firefox-wrapped"), "firefox");
        assert_eq!(tidy(".Hyprland-wrapp"), "Hyprland");
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            name_and_role("chrome", &args(&["/opt/chrome", "--type=renderer"]), ""),
            ("chrome".into(), "Renderer".into())
        );
        assert_eq!(
            name_and_role("python3", &args(&["python3", "-u", "/srv/server.py"]), ""),
            ("python3".into(), "server.py".into())
        );
        assert_eq!(name_and_role("sshd", &args(&["sshd: callen@pts/0"]), "/run/sshd"), ("sshd".into(), String::new()));
        assert_eq!(name_and_role("kworker/0:1", &[], ""), ("kworker/0:1".into(), String::new()));
    }

    #[test]
    fn children_and_units_group_but_managers_do_not() {
        let nodes = vec![
            node(1, 0, "systemd", "/init.scope"),
            Node { kernel: true, ..node(2, 0, "kthreadd", "") },
            Node { kernel: true, ..node(3, 2, "kworker", "") },
            node(10, 1, "aizome", "/user.slice/aizome.service"),
            node(20, 10, "foot", "/user.slice/app-aizome-foot-1.scope"),
            node(21, 20, "fish", "/user.slice/app-aizome-foot-1.scope"),
            node(30, 1, "chrome", "/user.slice/app-chrome-5.scope"),
            node(31, 30, "chrome", "/user.slice/app-chrome-5.scope"),
            // A helper moved into a scope of its own still joins its parent.
            node(32, 30, "chrome", "/user.slice/app-chrome-helper-9.scope"),
        ];
        let groups = group(&nodes);
        let pids: Vec<Vec<u32>> = groups.iter().map(|g| g.iter().map(|&i| nodes[i].pid).collect()).collect();
        assert_eq!(pids, [vec![1], vec![2, 3], vec![10], vec![20, 21], vec![30, 31, 32]]);
        let id = identify(&nodes, &groups[1], &[], None, 1000);
        assert_eq!((id.kind, id.name.as_str()), (Kind::Kernel, "Kernel"));
        let w = [Window { pid: 20, app_id: "foot".into(), title: "~/work".into(), focused: true }];
        let foot = identify(&nodes, &groups[3], &w, None, 1000);
        assert_eq!((foot.kind, foot.detail.as_str(), foot.windows, foot.focused), (Kind::App, "~/work", 1, true));
        let theirs = identify(&nodes, &groups[0], &[], None, 0);
        assert_eq!(theirs.kind, Kind::System, "another user's, seen by root");
        let mine = identify(&nodes, &groups[0], &[], None, 1000);
        assert_eq!(mine.kind, Kind::Service, "your own, without a window or launcher entry");
    }

    #[test]
    fn shell_launches_and_opened_applications_keep_their_own_groups() {
        let nodes = vec![
            node(1, 0, "systemd", "/init.scope"),
            node(10, 1, "aizome", "/user.slice/aizome.service"),
            node(11, 10, "aizome-shell", "/user.slice/aizome.service"),
            // The shell starts Settings directly, without a scope.
            node(12, 11, "aizome-settei", "/user.slice/aizome.service"),
            node(20, 11, "aizome-hako", "/user.slice/app-aizome-org.aizome.Hako-a.scope"),
            // Opening a document starts Sumi as Hako's child in its own scope.
            node(21, 20, "aizome-sumi", "/user.slice/app-aizome-org.aizome.Sumi-b.scope"),
            node(22, 21, "helper", "/user.slice/app-aizome-org.aizome.Sumi-b.scope"),
            node(30, 11, "firefox", "/user.slice/app-aizome-firefox-c.scope"),
            node(31, 30, "firefox", "/user.slice/app-aizome-firefox-c.scope"),
            node(40, 11, "aizome-myaku", "/user.slice/app-aizome-org.aizome.Myaku-d.scope"),
        ];
        let pids: Vec<Vec<u32>> = group(&nodes).iter().map(|g| g.iter().map(|&i| nodes[i].pid).collect()).collect();
        assert_eq!(pids, [vec![1], vec![10], vec![11], vec![12], vec![20], vec![21, 22], vec![30, 31], vec![40]]);
    }

    #[test]
    fn explicit_app_scopes_bound_parent_edges_but_helpers_still_follow() {
        let nodes = vec![
            node(1, 0, "systemd", "/init.scope"),
            node(20, 1, "foot", "/user.slice/app-aizome-foot-a.scope"),
            node(21, 20, "fish", "/user.slice/app-aizome-foot-a.scope"),
            // An independently scoped launch is a new instance, even for the same app.
            node(22, 21, "foot", "/user.slice/app-aizome-foot-b.scope"),
            // An ordinary unscoped child remains part of the terminal's work.
            node(23, 21, "sleep", "/user.slice/app-aizome-foot-a.scope"),
            node(30, 1, "chrome", "/user.slice/app-aizome-chrome-c.scope"),
            // Browsers move their helpers into non-launcher scopes of their own.
            node(31, 30, "chrome", "/user.slice/app-chrome-helper-d.scope"),
            node(32, 31, "chrome", "/user.slice/app-chrome-helper-d.scope"),
        ];
        let pids: Vec<Vec<u32>> = group(&nodes).iter().map(|g| g.iter().map(|&i| nodes[i].pid).collect()).collect();
        assert_eq!(pids, [vec![1], vec![20, 21, 23], vec![22], vec![30, 31, 32]]);
    }
}
