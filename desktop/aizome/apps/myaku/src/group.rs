//! Applications: which processes belong together, and what to call them.
//!
//! Window identities establish application boundaries. Helpers and commands
//! follow valid parentage within those boundaries, even when Chromium moves
//! them between scopes. Session managers and the desktop shell never own the
//! applications they launch.
//!
//! A dedicated application/service unit can reunite orphaned workers only
//! when it has one unambiguous application owner. Session scopes, slices and
//! units containing a manager are not evidence that processes form one app.
//! Labels use explicit window/unit identities or unique executable matches;
//! neither focus nor window stacking order decides a group's name.
//!
//! Ported from Tatami's Activity (`apps.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
                | "aizome-session"
                | "dbus-daemon"
                | "dbus-broker"
                | "dbus-broker-launch"
                | "gnome-session-binary"
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
#[derive(Clone, Debug, Default, Hash)]
pub struct Node {
    pub pid: u32,
    pub ppid: u32,
    pub started: u64,
    pub uid: u32,
    pub kernel: bool,
    pub name: String,
    pub comm: String,
    pub role: String,
    pub exe: String,
    pub cgroup: String,
    /// The unit's application ID (`app-aizome-org.aizome.Sumi-12.scope`).
    pub app_id: Option<String>,
}

impl Node {
    fn unit_path(&self) -> Option<&str> {
        let mut path = self.cgroup.as_str();
        loop {
            let (parent, name) = path.rsplit_once('/')?;
            if name.ends_with(".service") || name.ends_with(".scope") {
                return Some(path);
            }
            path = parent;
        }
    }

    fn unit(&self) -> &str {
        self.unit_path().and_then(|p| p.rsplit('/').next()).unwrap_or("")
    }

    /// A deliberate desktop launch is independent, even for another instance
    /// of the same app. Private Chromium/Electron helper scopes are not launches.
    fn launch_scope(&self) -> bool {
        let unit = self.unit();
        unit.starts_with("app-aizome-") && unit.ends_with(".scope")
    }

    fn dedicated_unit(&self) -> Option<&str> {
        let unit = self.unit();
        ((unit.ends_with(".service") && !unit.starts_with("user@"))
            || (unit.starts_with("app-") && unit.ends_with(".scope")))
        .then(|| self.unit_path())
        .flatten()
    }
}

fn application_id(id: &str, apps: Option<&AppIndex>) -> String {
    let id = apps.and_then(|a| a.match_window_identity(id)).map_or(id, |a| a.id.as_str());
    let lower = id.trim().to_lowercase();
    lower.strip_suffix(".desktop").unwrap_or(&lower).to_owned()
}

fn window_owners(windows: &[Window], apps: Option<&AppIndex>) -> HashMap<u32, String> {
    let mut identities: HashMap<u32, BTreeSet<String>> = HashMap::new();
    for w in windows {
        identities.entry(w.pid).or_default().insert(application_id(&w.app_id, apps));
    }
    identities
        .into_iter()
        .map(|(pid, ids)| {
            let identity = if ids.len() == 1 && !ids.contains("") {
                ids.into_iter().next().unwrap()
            } else {
                // A shared process or missing app ID cannot identify another app.
                format!("pid:{pid}")
            };
            (pid, identity)
        })
        .collect()
}

fn valid_parent(child: &Node, parent: &Node) -> bool {
    child.pid != parent.pid
        && child.uid == parent.uid
        && parent.started <= child.started
        && !manager(child.pid, &child.name, child.kernel)
        && !manager(parent.pid, &parent.name, parent.kernel)
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
pub fn group(nodes: &[Node], windows: &[Window], apps: Option<&AppIndex>) -> Vec<Vec<usize>> {
    let index: HashMap<u32, usize> = nodes.iter().enumerate().map(|(i, n)| (n.pid, i)).collect();
    let owners = window_owners(windows, apps);
    let mut parents: Vec<Option<usize>> =
        nodes.iter().map(|n| index.get(&n.ppid).copied().filter(|&p| valid_parent(n, &nodes[p]))).collect();
    let mut roots: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut boundaries = HashSet::new();
    let mut identities: Vec<Option<String>> = vec![None; nodes.len()];
    // Resolve ancestors before descendants without depending on /proc order
    // or recursing through an arbitrarily deep build/process tree.
    for start in 0..nodes.len() {
        let mut path = Vec::new();
        let mut visited = HashMap::new();
        let mut next = Some(start);
        while let Some(i) = next {
            if roots[i].is_some() {
                break;
            }
            if let Some(&cycle) = visited.get(&i) {
                for &j in &path[cycle..] {
                    parents[j] = None;
                }
                break;
            }
            visited.insert(i, path.len());
            path.push(i);
            next = parents[i];
        }
        for i in path.into_iter().rev() {
            let n = &nodes[i];
            let own = owners.get(&n.pid);
            let scope_id = n.app_id.as_deref().map(|id| application_id(id, apps));
            let parent = parents[i].filter(|&p| {
                let root = roots[p].unwrap();
                // A separately identified GUI app owns its own descendants,
                // including when it was launched from a terminal's scope.
                if own.is_some_and(|id| identities[root].as_ref() != Some(id)) {
                    return false;
                }
                // Explicit launcher scopes are independent instances even
                // when both windows have the same app identity.
                let different_unit = n.unit_path() != nodes[p].unit_path();
                if different_unit && n.launch_scope() {
                    return false;
                }
                // Headless apps/services are boundaries too. Same-program
                // helpers may move between differently named non-launcher
                // scopes, even before their command line exposes a role.
                let same_program =
                    !n.exe.is_empty() && n.exe == nodes[p].exe || (!n.name.is_empty() && n.name == nodes[p].name);
                let different_app = scope_id.is_some() && scope_id != identities[root];
                !(own.is_none()
                    && different_unit
                    && n.dedicated_unit().is_some()
                    && (n.unit().ends_with(".service") || different_app && !same_program))
            });
            if let Some(p) = parent {
                let root = roots[p].unwrap();
                roots[i] = Some(root);
                // A window identifies its tree more reliably than a unit's
                // generic runtime ID. It can only refine a compatible owner.
                if own.is_some() {
                    identities[root] = own.cloned();
                }
            } else {
                roots[i] = Some(i);
                if let Some(p) = parents[i] {
                    // Preserve both sides of an explicit boundary even if
                    // the parent currently has no windows. A shared unit
                    // must never undo the parent-edge decision above.
                    boundaries.insert(roots[p].unwrap());
                    boundaries.insert(i);
                }
                identities[i] = own.cloned().or(scope_id);
            }
        }
    }
    let mut sets = Sets((0..nodes.len()).collect());
    for (i, root) in roots.iter().enumerate() {
        sets.join(i, root.unwrap());
    }
    let mut kernel = None;
    let mut anchored = boundaries;
    for (i, n) in nodes.iter().enumerate() {
        if n.kernel || n.pid == 2 {
            if let Some(first) = kernel {
                sets.join(first, i);
            } else {
                kernel = Some(i);
            }
        } else if owners.contains_key(&n.pid) {
            anchored.insert(roots[i].unwrap());
        }
    }
    let managed: HashSet<_> = nodes
        .iter()
        .filter(|n| manager(n.pid, &n.name, n.kernel))
        .filter_map(|n| n.unit_path().map(|u| (n.uid, u)))
        .collect();
    let mut units: BTreeMap<(u32, &str), BTreeSet<usize>> = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.kernel || manager(n.pid, &n.name, n.kernel) {
            continue;
        }
        if let Some(unit) = n.dedicated_unit().filter(|u| !managed.contains(&(n.uid, *u))) {
            units.entry((n.uid, unit)).or_default().insert(roots[i].unwrap());
        }
    }
    // First find every tree linked by candidate unit evidence. If that
    // evidence points to two owners, attach none of the ambiguous trees:
    // processing units one by one would arbitrarily award a bridge to the
    // first app and make attribution depend on unit/input ordering.
    let mut unit_sets = Sets((0..nodes.len()).collect());
    for candidates in units.values() {
        if let Some(&first) = candidates.first() {
            for &other in candidates {
                unit_sets.join(first, other);
            }
        }
    }
    let mut related: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &root) in roots.iter().enumerate() {
        if root == Some(i) {
            related.entry(unit_sets.find(i)).or_default().push(i);
        }
    }
    for candidates in related.values() {
        let owner_count = candidates.iter().filter(|r| anchored.contains(r)).count();
        let ids: BTreeSet<_> = candidates.iter().filter_map(|&r| identities[r].as_deref()).collect();
        if owner_count > 1 || owner_count == 0 && ids.len() > 1 {
            continue;
        }
        if let Some(&first) = candidates.first() {
            for &other in candidates {
                sets.join(first, other);
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..nodes.len() {
        groups.entry(sets.find(i)).or_default().push(i);
    }
    let mut groups: Vec<Vec<usize>> = groups.into_values().collect();
    for g in &mut groups {
        g.sort_by_key(|&i| (nodes[i].started, nodes[i].pid));
    }
    groups.sort_by_key(|g| (nodes[g[0]].started, nodes[g[0]].pid));
    groups
}

/// Explain membership without calling every member a direct child.
fn attribution(nodes: &[Node], members: &[usize]) -> String {
    let pids: HashMap<_, _> = members.iter().map(|&i| (nodes[i].pid, &nodes[i])).collect();
    let roots: Vec<_> =
        members.iter().map(|&i| &nodes[i]).filter(|n| pids.get(&n.ppid).is_none_or(|p| !valid_parent(n, p))).collect();
    if members.len() == 1 {
        return format!("One process: {} (PID {}).", nodes[members[0]].name, nodes[members[0]].pid);
    }
    if let [root] = roots.as_slice() {
        return format!(
            "Process tree rooted at {} (PID {}), including the helpers and commands it starts. Members need not be direct children.",
            root.name, root.pid
        );
    }
    let units: BTreeSet<_> = members.iter().map(|&i| nodes[i].unit()).filter(|s| !s.is_empty()).collect();
    format!(
        "Related process trees share dedicated application or service units: {}. Shared unit membership does not mean direct parenthood; each process shows its Parent and Unit.",
        units.into_iter().collect::<Vec<_>>().join(", ")
    )
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
    pub attribution: String,
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
            attribution: "Kernel threads are grouped separately from user-space applications.".into(),
            windows: 0,
            focused: false,
        };
    }
    let pids: HashSet<u32> = members.iter().map(|&i| nodes[i].pid).collect();
    let own: Vec<&Window> = windows.iter().filter(|w| pids.contains(&w.pid)).collect();
    // A window's actual process is a better representative than a wrapper
    // which happened to start first. Never choose by focus or stacking order.
    let representative = members
        .iter()
        .map(|&i| &nodes[i])
        .find(|n| own.iter().any(|w| w.pid == n.pid))
        .or_else(|| members.iter().map(|&i| &nodes[i]).find(|n| !launcher(&n.name)))
        .unwrap_or(first);
    let mine = members.iter().any(|&i| nodes[i].uid == uid);
    let unit = first.unit().to_owned();
    let ids: BTreeSet<_> = own.iter().map(|w| application_id(&w.app_id, apps)).filter(|id| !id.is_empty()).collect();
    let mixed = ids.len() > 1;
    let detail = match own.as_slice() {
        _ if mixed => format!("{} windows from {} application identities", own.len(), ids.len()),
        [] if unit.ends_with(".service") && !unit.starts_with("run-") => unit.clone(),
        [] => String::new(),
        [only] => only.title.clone(),
        many => format!("{} windows", many.len()),
    };
    let is_manager = manager(representative.pid, &representative.name, representative.kernel);
    let mut naming_units = BTreeSet::new();
    let entry = apps.filter(|_| !mixed && !is_manager).and_then(|apps| {
        ids.first()
            .and_then(|id| apps.match_window_identity(id))
            .or_else(|| {
                // A window with an unknown ID is still evidence of its own
                // app. It must not borrow a terminal/launcher's scope name.
                if !own.is_empty() {
                    return None;
                }
                let ids: BTreeSet<_> = members
                    .iter()
                    .filter_map(|&i| nodes[i].app_id.as_deref())
                    .map(|id| application_id(id, Some(apps)))
                    .collect();
                let entry =
                    (ids.len() == 1).then(|| ids.first().and_then(|id| apps.match_window_identity(id))).flatten();
                if entry.is_some() {
                    naming_units
                        .extend(members.iter().map(|&i| &nodes[i]).filter(|n| n.app_id.is_some()).map(Node::unit));
                }
                entry
            })
            .or_else(|| {
                [representative.exe.as_str(), representative.name.as_str(), representative.comm.as_str()]
                    .into_iter()
                    .filter(|n| !n.is_empty())
                    .find_map(|n| apps.match_program_identity(n))
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
    let mut explanation = if mixed {
        "One process hosts windows with different application identities; its program names this group.".into()
    } else {
        attribution(nodes, members)
    };
    if kind == Kind::App && !naming_units.is_empty() {
        let noun = if naming_units.len() == 1 { "unit" } else { "units" };
        explanation.push_str(&format!(
            " The application name comes from its dedicated {noun}: {}.",
            naming_units.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Identity {
        kind,
        name: if name.is_empty() { representative.comm.clone() } else { name },
        app_id: entry.map(|e| e.id.clone()),
        detail,
        attribution: explanation,
        windows: own.len(),
        focused: own.iter().any(|w| w.focused),
    }
}

/// Cache identities only while every membership/label input remains the same.
/// Equal member counts or a reused PID alone do not establish that identity.
pub fn fingerprint(nodes: &[Node], members: &[usize], windows: &[Window]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut members: Vec<_> = members.iter().map(|&i| &nodes[i]).collect();
    members.sort_by_key(|n| (n.started, n.pid));
    members.hash(&mut hash);
    let pids: HashSet<_> = members.iter().map(|n| n.pid).collect();
    let mut own: Vec<_> =
        windows.iter().filter(|w| pids.contains(&w.pid)).map(|w| (w.pid, &w.app_id, &w.title, w.focused)).collect();
    own.sort();
    own.hash(&mut hash);
    hash.finish()
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
            app_id: aizome_sys::sysinfo::SystemdUnit::from_cgroup(cgroup).and_then(|u| u.app_id()),
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
    fn the_desktop_shell_does_not_make_its_apps_one_application() {
        let nodes = vec![
            node(10, 1, "aizome", "/session.slice/aizome.service"),
            node(11, 10, "aizome-shell", "/session.slice/aizome.service"),
            node(20, 11, "aizome-fusen", "/app.slice/app-aizome-org.aizome.Fusen-1.scope"),
            node(30, 11, "chrome", "/app.slice/app-google\\x2dchrome-30.scope"),
            node(31, 30, "chrome", "/app.slice/app-google\\x2dchrome-30.scope"),
            node(40, 11, "claude-desktop", "/app.slice/app-com.anthropic.Claude-40.scope"),
            node(41, 40, "bash", "/app.slice/app-com.anthropic.Claude-40.scope"),
            node(42, 41, "cargo", "/app.slice/app-com.anthropic.Claude-40.scope"),
            node(43, 42, "rustc", "/app.slice/app-com.anthropic.Claude-40.scope"),
        ];
        let pids: Vec<Vec<u32>> =
            group(&nodes, &[], None).iter().map(|g| g.iter().map(|&i| nodes[i].pid).collect()).collect();
        assert_eq!(pids, [vec![10], vec![11], vec![20], vec![30, 31], vec![40, 41, 42, 43]]);
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
            Node { role: "Renderer".into(), ..node(32, 30, "chrome", "/user.slice/app-chrome-helper-9.scope") },
        ];
        let groups = group(&nodes, &[], None);
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

    fn window(pid: u32, app_id: &str) -> Window {
        Window { pid, app_id: app_id.into(), title: format!("{app_id} window"), focused: false }
    }

    fn partition(nodes: &[Node], windows: &[Window]) -> Vec<Vec<u32>> {
        let groups = group(nodes, windows, None);
        let mut indices: Vec<_> = groups.iter().flatten().copied().collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..nodes.len()).collect::<Vec<_>>(), "every process exactly once");
        let mut pids: Vec<Vec<_>> = groups
            .into_iter()
            .map(|g| {
                let mut pids: Vec<_> = g.into_iter().map(|i| nodes[i].pid).collect();
                pids.sort_unstable();
                pids
            })
            .collect();
        pids.sort();
        pids
    }

    #[test]
    fn electron_helpers_keep_their_owner_across_generic_and_branded_scopes() {
        let generic = "/app.slice/app-org.chromium.Chromium-20.scope";
        let branded = "/app.slice/app-chatgpt-abc.scope";
        let nodes = vec![
            node(20, 1, "ChatGPT", generic),
            Node { role: "Zygote".into(), ..node(21, 20, "ChatGPT", branded) },
            Node { role: "Renderer".into(), ..node(22, 21, "ChatGPT", branded) },
            node(23, 20, "codex", generic),
            node(24, 23, "bash", generic),
            node(25, 24, "cargo", generic),
            node(26, 25, "rustc", "/app.slice/run-build.scope"),
        ];
        assert_eq!(partition(&nodes, &[window(20, "chatgpt")]), [vec![20, 21, 22, 23, 24, 25, 26]]);
        assert_eq!(
            partition(&nodes, &[]),
            [vec![20, 21, 22, 23, 24, 25, 26]],
            "helpers also work without compositor IPC"
        );
    }

    #[test]
    fn a_gui_app_launched_from_a_terminal_owns_its_language_server() {
        let unit = "/app.slice/app-aizome-foot-1.scope";
        let nodes = vec![
            node(20, 1, "foot", unit),
            node(21, 20, "fish", unit),
            node(30, 21, "zed", unit),
            node(31, 30, "rust-analyzer", unit),
            node(32, 31, "rustc", unit),
        ];
        let windows = [window(20, "foot"), window(30, "dev.zed.Zed")];
        assert_eq!(partition(&nodes, &windows), [vec![20, 21], vec![30, 31, 32]]);
        assert_eq!(
            partition(&nodes, &windows[1..]),
            [vec![20, 21], vec![30, 31, 32]],
            "a windowless parent still cannot be rejoined across an application boundary"
        );
        // A dead parent and shared terminal scope cannot establish which app
        // owns an orphan. Do not award it to either app by iteration order.
        let mut orphaned = nodes.clone();
        orphaned[3].ppid = 1;
        assert_eq!(partition(&orphaned, &windows), [vec![20, 21], vec![30], vec![31, 32]]);
    }

    #[test]
    fn independent_gui_children_cannot_be_rejoined_by_their_shared_unit() {
        let unit = "/app.slice/app-aizome-editor-1.scope";
        let nodes = vec![node(20, 1, "editor", unit), node(30, 20, "browser", unit), node(31, 30, "renderer", unit)];
        assert_eq!(partition(&nodes, &[window(20, "editor"), window(30, "browser")]), [vec![20], vec![30, 31]]);
    }

    #[test]
    fn dedicated_headless_apps_and_services_are_boundaries_too() {
        let nodes = vec![
            node(20, 1, "runner", "/app.slice/app-runner-1.scope"),
            node(30, 20, "worker", "/app.slice/app-worker-2.scope"),
            node(31, 30, "helper", "/app.slice/app-worker-2.scope"),
            node(40, 20, "daemon", "/system.slice/daemon.service"),
        ];
        assert_eq!(partition(&nodes, &[]), [vec![20], vec![30, 31], vec![40]]);
    }

    #[test]
    fn unit_evidence_never_arbitrarily_assigns_a_bridge_between_apps() {
        let a = "/app.slice/app-a-1.scope";
        let b = "/app.slice/app-b-2.scope";
        let nodes = vec![
            node(20, 1, "first", a),
            node(30, 1, "second", b),
            node(40, 1, "electron", a),
            Node { role: "Renderer".into(), ..node(41, 40, "electron", b) },
        ];
        let windows = [window(20, "a"), window(30, "b")];
        assert_eq!(partition(&nodes, &windows), [vec![20], vec![30], vec![40, 41]]);
    }

    #[test]
    fn shared_sessions_and_slices_are_not_application_units() {
        for unit in
            ["/user.slice/session-3.scope", "/user.slice/app.slice", "/user.slice/user@1000.service/init.scope", "/"]
        {
            let nodes = vec![node(20, 1, "first", unit), node(30, 1, "second", unit)];
            assert_eq!(partition(&nodes, &[]), [vec![20], vec![30]], "{unit}");
        }
    }

    #[test]
    fn a_real_service_can_reunite_orphaned_workers_in_sub_cgroups() {
        let nodes = vec![
            node(20, 1, "worker", "/system.slice/render.service"),
            node(30, 1, "worker", "/system.slice/render.service/workers"),
        ];
        assert_eq!(partition(&nodes, &[]), [vec![20, 30]]);
        let id = identify(&nodes, &[0, 1], &[], None, 1000);
        assert!(id.attribution.contains("render.service"));
        assert!(id.attribution.contains("does not mean direct parenthood"));
    }

    #[test]
    fn ancestry_rejects_reused_parent_pids_other_users_and_cycles() {
        let nodes = vec![
            Node { started: 100, ..node(20, 1, "new-parent", "") },
            Node { started: 50, ..node(30, 20, "old-child", "") },
            Node { uid: 1001, ..node(40, 30, "other-user", "") },
            Node { started: 200, ..node(50, 51, "cycle-a", "") },
            Node { started: 200, ..node(51, 50, "cycle-b", "") },
        ];
        assert_eq!(partition(&nodes, &[]), [vec![20], vec![30], vec![40], vec![50], vec![51]]);
    }

    #[test]
    fn group_membership_and_names_ignore_input_order_and_window_focus() {
        let unit = "/app.slice/app-aizome-foot-1.scope";
        let nodes = vec![node(20, 1, "foot", unit), node(30, 20, "zed", unit), node(31, 30, "rust-analyzer", unit)];
        let windows = vec![window(20, "foot"), window(30, "dev.zed.Zed")];
        let expected = partition(&nodes, &windows);
        let labels = |nodes: &[Node], windows: &[Window]| {
            group(nodes, windows, None)
                .iter()
                .map(|g| {
                    let id = identify(nodes, g, windows, None, 1000);
                    (id.name, id.detail, id.attribution)
                })
                .collect::<Vec<_>>()
        };
        let before = labels(&nodes, &windows);
        let mut reversed = nodes.clone();
        reversed.reverse();
        let mut refocused = windows.clone();
        refocused.reverse();
        refocused[0].focused = true;
        assert_eq!(partition(&reversed, &refocused), expected);
        assert_eq!(labels(&reversed, &refocused), before);
    }

    #[test]
    fn shared_window_hosts_are_named_for_their_program() {
        let nodes = vec![node(20, 1, "electron", "")];
        let windows = [window(20, "first-app"), window(20, "second-app")];
        let id = identify(&nodes, &[0], &windows, None, 1000);
        assert_eq!(id.name, "electron");
        assert!(id.detail.contains("2 application identities"));
        assert!(id.attribution.contains("different application identities"));
    }

    fn desktop_index() -> AppIndex {
        use aizome_sys::apps::{Locale, XdgDirs};
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("myaku-identities-{}-{stamp}", std::process::id()));
        let dir = root.join("usr/share/applications");
        std::fs::create_dir_all(&dir).unwrap();
        for (id, name, exe) in [
            ("foot", "Foot", "foot"),
            ("com.anthropic.Claude", "Claude", "claude-desktop"),
            ("org.one.Client", "Unrelated Client", "unrelated-client"),
        ] {
            std::fs::write(
                dir.join(format!("{id}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exe}\n"),
            )
            .unwrap();
        }
        std::fs::write(
            dir.join("background.desktop"),
            "[Desktop Entry]\nType=Application\nName=Background Worker\nExec=background-worker\nNoDisplay=true\n",
        )
        .unwrap();
        let index = AppIndex::load_with(XdgDirs::rooted(&root), Locale::default());
        std::fs::remove_dir_all(root).unwrap();
        index
    }

    #[test]
    fn an_unknown_window_or_manager_never_borrows_its_launchers_label() {
        let apps = desktop_index();
        let nodes = vec![node(20, 1, "custom-app", "/app.slice/app-aizome-foot-1.scope")];
        let id = identify(&nodes, &[0], &[window(20, "org.other.Client")], Some(&apps), 1000);
        assert_eq!(id.name, "custom-app", "neither the launcher's unit nor a reverse-DNS suffix identifies the window");
        assert_eq!(id.app_id, None);
        let nodes = vec![node(20, 1, "dbus-daemon", "/app.slice/app-aizome-com.anthropic.Claude-1.scope")];
        let id = identify(&nodes, &[0], &[], Some(&apps), 1000);
        assert_eq!((id.name.as_str(), id.kind, id.app_id), ("dbus-daemon", Kind::Service, None));
    }

    #[test]
    fn a_unit_based_label_explains_its_evidence() {
        let apps = desktop_index();
        let nodes = vec![node(20, 1, "crashpad_handler", "/app.slice/app-aizome-com.anthropic.Claude-1.scope")];
        let id = identify(&nodes, &[0], &[], Some(&apps), 1000);
        assert_eq!(id.name, "Claude");
        assert!(id.attribution.contains("name comes from its dedicated unit"));

        let nodes = vec![
            node(20, 1, "chrome", ""),
            Node { role: "Renderer".into(), ..node(30, 20, "chrome", "/app.slice/app-com.anthropic.Claude-1.scope") },
        ];
        assert_eq!(partition(&nodes, &[]), [vec![20, 30]]);
        let id = identify(&nodes, &[0, 1], &[], Some(&apps), 1000);
        assert_eq!(id.name, "Claude");
        assert!(id.attribution.contains("dedicated unit: app-com.anthropic.Claude-1.scope"));

        let nodes = vec![node(20, 1, "background-worker", "/app.slice/app-background-1.scope")];
        let id = identify(&nodes, &[0], &[], Some(&apps), 1000);
        assert_eq!((id.name.as_str(), id.kind), ("background-worker", Kind::Service));
        assert!(!id.attribution.contains("name comes from"), "the unit did not supply the displayed name");
    }

    #[test]
    fn the_identity_cache_tracks_membership_and_metadata_not_just_member_count() {
        let nodes = vec![node(20, 1, "app", ""), node(30, 20, "helper", "")];
        let key = fingerprint(&nodes, &[0, 1], &[]);
        let mut replacement = nodes.clone();
        replacement[1].pid = 31;
        assert_ne!(key, fingerprint(&replacement, &[0, 1], &[]));
        for change in [0, 1, 2, 3] {
            let mut edited = nodes.clone();
            match change {
                0 => edited[1].started += 1,
                1 => edited[1].exe = "/new/program".into(),
                2 => edited[1].cgroup = "/new/app.scope".into(),
                _ => edited[1].ppid = 1,
            }
            assert_ne!(key, fingerprint(&edited, &[0, 1], &[]));
        }
        assert_eq!(key, fingerprint(&nodes, &[1, 0], &[]));
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
        let pids: Vec<Vec<u32>> =
            group(&nodes, &[], None).iter().map(|g| g.iter().map(|&i| nodes[i].pid).collect()).collect();
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
        let pids: Vec<Vec<u32>> =
            group(&nodes, &[], None).iter().map(|g| g.iter().map(|&i| nodes[i].pid).collect()).collect();
        assert_eq!(pids, [vec![1], vec![20, 21, 23], vec![22], vec![30, 31, 32]]);
        assert_eq!(
            partition(&nodes, &[window(20, "foot"), window(22, "foot")]),
            [vec![1], vec![20, 21, 23], vec![22], vec![30, 31, 32]],
            "same-app windows must not collapse independently launched instances"
        );
    }
}
