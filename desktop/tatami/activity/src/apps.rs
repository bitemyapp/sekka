// SPDX-License-Identifier: MIT OR Apache-2.0
//! Applications: which processes belong together, and what to call them.
//!
//! A process belongs with its parent, unless the parent is a process
//! manager (init, a systemd instance, the kernel's thread spawner or the
//! compositor), and with every other process in its systemd unit, unless the
//! unit is a manager's own. Tatami starts applications as units of their own
//! (`tatami launch`, systemd-run), and Chromium and Electron move their
//! helpers into scopes of their own (app-com.google.Chrome-5203.scope), so
//! an application's processes come together however it was started.
//!
//! A group is named for its window (Hyprland's clients), else its first
//! program, else the application ID in its unit's name, as each matches a
//! launcher entry; otherwise for the program itself.
use crate::{
    model::{Icon, Kind},
    procfs,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// A launcher (.desktop) entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub icon: Icon,
    pub hidden: bool,
    /// The program its Exec line runs, lowercased.
    pub exec: Option<String>,
    /// Its StartupWMClass, lowercased.
    pub class: Option<String>,
}

#[derive(Default)]
pub struct Desktop {
    entries: Vec<Entry>,
    ids: HashMap<String, usize>,
    classes: HashMap<String, usize>,
    programs: HashMap<String, usize>,
}

/// A name without NixOS's wrapper decoration (".firefox-wrapped"), whole
/// or cut short as the kernel's 15-character command names are.
pub fn tidy(name: &str) -> String {
    let name = name.strip_prefix('.').unwrap_or(name);
    ["-wrapped", "-wrappe", "-wrapp"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
        .to_owned()
}

/// A program's name as people know it, from its path.
pub fn program(path: &str) -> String {
    tidy(path.rsplit('/').next().unwrap_or(path))
}

/// The program an Exec line runs, past `env` and variable assignments.
pub fn exec_program(exec: &str) -> Option<String> {
    exec.split_whitespace()
        .map(|word| word.trim_matches('"'))
        .find(|word| *word != "env" && !word.contains('=') && !word.starts_with('-'))
        .map(|word| program(word).to_lowercase())
}

pub fn parse_entry(id: &str, text: &str) -> Option<Entry> {
    let mut in_entry = false;
    let mut entry = Entry {
        id: id.to_owned(),
        ..Default::default()
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_entry) else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Name" => entry.name = value.to_owned(),
            "Icon" if value.starts_with('/') => entry.icon = Icon::File(value.to_owned()),
            "Icon" if !value.is_empty() => entry.icon = Icon::Named(value.to_owned()),
            "Exec" => entry.exec = exec_program(value),
            "StartupWMClass" => entry.class = Some(value.to_lowercase()),
            "NoDisplay" | "Hidden" if value == "true" => entry.hidden = true,
            "Type" if value != "Application" => return None,
            _ => {}
        }
    }
    (!entry.name.is_empty()).then_some(entry)
}

/// Where launcher entries live: the user's data directory, then the
/// system's ($XDG_DATA_DIRS).
fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let user = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|dir| dir.starts_with('/'))
        .unwrap_or_else(|| format!("{home}/.local/share"));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    std::iter::once(user)
        .chain(system.split(':').map(str::to_owned))
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| Path::new(&dir).join("applications"))
        .collect()
}

impl Desktop {
    pub fn load() -> Desktop {
        let mut desktop = Desktop::default();
        for dir in application_dirs() {
            let Ok(files) = std::fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                let Some(id) = name.strip_suffix(".desktop") else {
                    continue;
                };
                // The first directory to have an entry wins, as for the
                // launcher.
                if desktop.ids.contains_key(&id.to_lowercase()) {
                    continue;
                }
                if let Some(entry) =
                    procfs::read(file.path()).and_then(|text| parse_entry(id, &text))
                {
                    desktop.add(entry);
                }
            }
        }
        desktop
    }

    pub fn add(&mut self, entry: Entry) {
        let index = self.entries.len();
        let entries = &self.entries;
        // A visible entry is preferred to a hidden one for the same key.
        let put = |map: &mut HashMap<String, usize>, key: String| {
            let replace = match map.get(&key) {
                None => true,
                Some(&other) => entries[other].hidden && !entry.hidden,
            };
            if replace {
                map.insert(key, index);
            }
        };
        put(&mut self.ids, entry.id.to_lowercase());
        if let Some(class) = &entry.class {
            put(&mut self.classes, class.clone());
        }
        if let Some(exec) = &entry.exec {
            put(&mut self.programs, exec.clone());
        }
        self.entries.push(entry);
    }

    fn get(&self, map: &HashMap<String, usize>, key: &str) -> Option<&Entry> {
        map.get(&key.to_lowercase())
            .map(|&index| &self.entries[index])
    }

    /// The entry for a window's class.
    pub fn by_class(&self, class: &str) -> Option<&Entry> {
        let class = class.to_lowercase();
        self.get(&self.classes, &class)
            .or_else(|| self.get(&self.ids, &class))
            .or_else(|| self.get(&self.programs, &class))
            .or_else(|| {
                // "org.gnome.Nautilus" for a window of class "nautilus".
                self.entries.iter().find(|entry| {
                    entry.id.to_lowercase().rsplit('.').next() == Some(class.as_str())
                })
            })
    }

    /// The entry for an application ID from a unit's name.
    pub fn by_app_id(&self, id: &str) -> Option<&Entry> {
        self.get(&self.ids, id)
    }

    /// The entry for a program: the one named for it first ("foot", not
    /// "foot-server", which runs foot too), then one that runs it.
    pub fn by_program(&self, name: &str) -> Option<&Entry> {
        self.get(&self.ids, name)
            .or_else(|| self.get(&self.programs, name))
            .or_else(|| self.get(&self.classes, name))
    }
}

/// The application IDs a unit's name may hold, as systemd's conventions
/// write them: app[-LAUNCHER]-ID-RANDOM.scope or app[-LAUNCHER]-ID@RANDOM.service.
/// Without dots in the ID, a launcher's name can't be told from the ID's
/// first word, so both readings come back, the whole one first.
pub fn unit_app_ids(unit: &str) -> Vec<String> {
    let unit = procfs::unescape_unit(unit);
    let Some(rest) = unit.strip_prefix("app-") else {
        return Vec::new();
    };
    let rest = if let Some(rest) = rest.strip_suffix(".scope") {
        match rest.rsplit_once('-') {
            Some((id, _)) => id,
            None => return Vec::new(),
        }
    } else if let Some(rest) = rest.strip_suffix(".service") {
        rest.split_once('@').map(|(id, _)| id).unwrap_or(rest)
    } else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    match rest.split_once('-') {
        // app-Hyprland-org.gnome.Nautilus-123.scope
        Some((launcher, id)) if !launcher.contains('.') && id.contains('.') => ids.push(id),
        Some((_, id)) => ids.extend([rest, id]),
        None => ids.push(rest),
    }
    ids.into_iter()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A window, from Hyprland's clients.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub pid: i32,
    pub class: String,
    pub title: String,
}

pub fn parse_windows(json: &str) -> Vec<Window> {
    let Ok(serde_json::Value::Array(clients)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    clients
        .iter()
        .filter_map(|client| {
            let pid = client["pid"].as_i64()? as i32;
            let class = client["class"]
                .as_str()
                .filter(|class| !class.is_empty())
                .or_else(|| client["initialClass"].as_str())
                .unwrap_or("")
                .to_owned();
            let title = client["title"].as_str().unwrap_or("").to_owned();
            (pid > 0).then_some(Window { pid, class, title })
        })
        .collect()
}

/// One request on Hyprland's command socket, as hyprctl makes it; nothing
/// outside Hyprland. Bounded, so a stuck compositor can't stall readings.
fn hyprland(request: &str) -> Option<String> {
    use std::{
        io::{Read, Write},
        net::Shutdown,
        os::unix::net::UnixStream,
        time::Duration,
    };
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .ok()
        .filter(|signature| !signature.is_empty() && !signature.contains('/'))?;
    let mut stream =
        UnixStream::connect(format!("{runtime}/hypr/{signature}/.socket.sock")).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    stream.shutdown(Shutdown::Write).ok()?;
    let mut reply = String::new();
    stream.take(4 << 20).read_to_string(&mut reply).ok()?;
    Some(reply)
}

/// Hyprland's windows.
pub fn windows() -> Vec<Window> {
    hyprland("j/clients")
        .map(|reply| parse_windows(&reply))
        .unwrap_or_default()
}

/// The process whose window has the focus.
pub fn focused_pid() -> Option<i32> {
    let reply = hyprland("j/activewindow")?;
    let window: serde_json::Value = serde_json::from_str(&reply).ok()?;
    window["pid"]
        .as_i64()
        .map(|pid| pid as i32)
        .filter(|pid| *pid > 0)
}

/// What Chromium and Electron helpers do, from their --type switches.
fn chromium_role(args: &[String]) -> Option<String> {
    let value = |flag: &str| args.iter().find_map(|arg| arg.strip_prefix(flag));
    let role = match value("--type=")? {
        "renderer" if args.iter().any(|arg| arg == "--extension-process") => "Extension",
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
            Some("on_device_model") => "On-device model",
            _ => "Utility",
        },
        other => return Some(other.to_owned()),
    };
    Some(role.to_owned())
}

const INTERPRETERS: [&str; 12] = [
    "python", "python3", "node", "nodejs", "perl", "ruby", "bun", "deno", "lua", "luajit", "php",
    "java",
];

const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "fish", "dash", "nu"];

/// A process's name and role: "chrome" and "Renderer", "python3" and
/// "server.py". Chromium rewrites its title into a single argument, which
/// is split back into words.
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
        if exe.is_empty() {
            tidy(comm)
        } else {
            program(exe)
        }
    } else {
        program(first.split_whitespace().next().unwrap_or(first))
    };
    let name = if name.is_empty() {
        comm.to_owned()
    } else {
        name
    };
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let role = if let Some(role) = chromium_role(args) {
        role
    } else if INTERPRETERS.contains(&base) || INTERPRETERS.contains(&name.as_str()) {
        let mut rest = args.iter().skip(1);
        let mut role = String::new();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "-m" | "-jar" => {
                    role = rest.next().map(|next| program(next)).unwrap_or_default();
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
            .filter(|arg| !arg.starts_with('-') && (arg.contains('/') || arg.ends_with(".sh")))
            .map(|arg| program(arg))
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
                | "tatami"
                | "app2unit"
                | "bwrap"
                | "steam-run"
        )
}

/// Process managers: their children are applications of their own.
fn manager(pid: i32, name: &str, kernel: bool) -> bool {
    pid <= 2
        || kernel
        || matches!(
            name,
            "systemd"
                | "init"
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
        )
}

/// What grouping needs to know of a process.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub pid: i32,
    pub ppid: i32,
    pub started: u64,
    pub uid: u32,
    pub kernel: bool,
    pub name: String,
    pub comm: String,
    pub exe: String,
    pub cgroup: String,
}

impl Node {
    fn unit(&self) -> &str {
        self.cgroup.rsplit('/').next().unwrap_or("")
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
    let index: HashMap<i32, usize> = nodes.iter().enumerate().map(|(i, n)| (n.pid, i)).collect();
    let mut sets = Sets((0..nodes.len()).collect());
    let managers: Vec<bool> = nodes
        .iter()
        .map(|node| manager(node.pid, &node.name, node.kernel))
        .collect();
    // A manager's unit holds what it started without systemd's help: those
    // are not one application.
    let managed: HashSet<&str> = nodes
        .iter()
        .zip(&managers)
        .filter(|(node, manager)| **manager && !node.kernel)
        .map(|(node, _)| node.cgroup.as_str())
        .collect();
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
        {
            sets.join(i, parent);
        }
        let cgroup = node.cgroup.as_str();
        if !cgroup.is_empty()
            && cgroup != "/"
            && !managed.contains(cgroup)
            && !node.unit().ends_with("init.scope")
        {
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
    for group in &mut groups {
        group.sort_by_key(|&i| (nodes[i].started, nodes[i].pid));
    }
    groups.sort_by_key(|group| (nodes[group[0]].started, nodes[group[0]].pid));
    groups
}

/// How a group shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub kind: Kind,
    pub name: String,
    pub icon: Icon,
    pub detail: String,
    pub windows: usize,
}

pub fn identify(
    nodes: &[Node],
    members: &[usize],
    windows: &[Window],
    desktop: &Desktop,
    uid: u32,
) -> Identity {
    let first = &nodes[members[0]];
    if first.kernel || first.pid == 2 {
        return Identity {
            kind: Kind::Kernel,
            name: "Kernel".into(),
            icon: Icon::None,
            detail: "The kernel's own threads".into(),
            windows: 0,
        };
    }
    let pids: HashSet<i32> = members.iter().map(|&i| nodes[i].pid).collect();
    let own: Vec<&Window> = windows.iter().filter(|w| pids.contains(&w.pid)).collect();
    // The program it runs, past any launcher.
    let representative = members
        .iter()
        .map(|&i| &nodes[i])
        .find(|node| !launcher(&node.name))
        .unwrap_or(first);
    let mine = members.iter().any(|&i| nodes[i].uid == uid);
    let unit = procfs::unescape_unit(first.unit());
    let detail = match own.as_slice() {
        [] if unit.ends_with(".service") && !unit.starts_with("run-") => unit.clone(),
        [] => String::new(),
        [only] => only.title.clone(),
        many => format!("{} windows", many.len()),
    };
    let entry = own
        .iter()
        .find_map(|window| desktop.by_class(&window.class))
        .or_else(|| {
            [
                representative.name.clone(),
                program(&representative.exe),
                tidy(&representative.comm),
            ]
            .iter()
            .filter(|name| !name.is_empty())
            .find_map(|name| desktop.by_program(name))
        })
        .or_else(|| {
            members
                .iter()
                .flat_map(|&i| unit_app_ids(nodes[i].unit()))
                .find_map(|id| desktop.by_app_id(&id))
        });
    let kind = if !mine {
        Kind::System
    } else if own.is_empty() && entry.is_none_or(|entry| entry.hidden) {
        Kind::Service
    } else {
        Kind::App
    };
    match entry {
        Some(entry) => Identity {
            kind,
            name: entry.name.clone(),
            icon: entry.icon.clone(),
            detail,
            windows: own.len(),
        },
        None => Identity {
            kind,
            name: representative.name.clone(),
            icon: match own.first() {
                Some(window) if !window.class.is_empty() => {
                    Icon::Named(window.class.to_lowercase())
                }
                _ => Icon::None,
            },
            detail,
            windows: own.len(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(pid: i32, ppid: i32, name: &str, cgroup: &str) -> Node {
        Node {
            pid,
            ppid,
            started: pid as u64,
            uid: 1000,
            name: name.into(),
            comm: name.into(),
            cgroup: cgroup.into(),
            ..Default::default()
        }
    }

    const USER: &str = "/user.slice/user-1000.slice/user@1000.service";

    #[test]
    fn applications_come_together_however_started() {
        let session = format!("{USER}/session.slice/wayland-wm@tatami.service");
        let launched = format!("{USER}/app.slice/app-graphical.slice/run-p5202-i412.service");
        let terminal = format!("{USER}/app.slice/app-graphical.slice/run-p5999-i1.service");
        let nodes = vec![
            node(1, 0, "systemd", "/init.scope"),
            Node {
                kernel: true,
                ..node(2, 0, "kthreadd", "/")
            },
            Node {
                kernel: true,
                ..node(3, 2, "kworker/0:1", "/")
            },
            node(900, 1, "systemd", &format!("{USER}/init.scope")),
            // Hyprland, started by uwsm, and Xwayland, which it starts.
            node(1000, 900, "start-hyprland", &session),
            node(1001, 1000, "Hyprland", &session),
            node(1002, 1001, "Xwayland", &session),
            // Chrome, started with `tatami launch`, which moves itself into
            // a scope; and a crash handler reparented to systemd.
            node(
                5203,
                900,
                "chrome",
                &format!("{USER}/app.slice/app-com.google.Chrome-5203.scope"),
            ),
            node(5222, 5203, "chrome", &launched),
            node(5210, 900, "chrome_crashpad_handler", &launched),
            // A terminal and what runs in it.
            node(6000, 900, "foot", &terminal),
            node(6001, 6000, "bash", &terminal),
            node(6002, 6001, "btm", &terminal),
            node(
                700,
                900,
                "waybar",
                &format!("{USER}/app.slice/tatami-waybar.service"),
            ),
        ];
        let groups = group(&nodes);
        let pids =
            |group: &Vec<usize>| -> Vec<i32> { group.iter().map(|&i| nodes[i].pid).collect() };
        let find = |pid: i32| groups.iter().map(pids).find(|g| g.contains(&pid)).unwrap();
        assert_eq!(find(3), vec![2, 3]);
        assert_eq!(find(5222), vec![5203, 5210, 5222]);
        assert_eq!(find(6002), vec![6000, 6001, 6002]);
        assert_eq!(find(1001), vec![1000, 1001]);
        assert_eq!(find(1002), vec![1002]);
        assert_eq!(find(700), vec![700]);
        assert_eq!(find(900), vec![900]);
        assert_eq!(find(1), vec![1]);
    }

    #[test]
    fn groups_are_named_for_windows_programs_and_unit_ids() {
        let mut desktop = Desktop::default();
        let chrome = parse_entry(
            "google-chrome",
            "[Desktop Entry]\nType=Application\nName=Google Chrome\n\
             Exec=/nix/store/x/bin/google-chrome-stable %U\nStartupWMClass=google-chrome\n\
             Icon=google-chrome\n",
        )
        .unwrap();
        assert_eq!(chrome.exec.as_deref(), Some("google-chrome-stable"));
        desktop.add(chrome);
        let claude = parse_entry(
            "com.anthropic.Claude",
            "[Desktop Entry]\nName=Claude\nExec=claude-desktop %U\nIcon=claude-desktop\n\
             [Desktop Action new]\nName=New chat\n",
        )
        .unwrap();
        assert_eq!(claude.name, "Claude");
        desktop.add(claude);
        let discord =
            parse_entry("discord", "[Desktop Entry]\nName=Discord\nExec=Discord\n").unwrap();
        desktop.add(discord);
        // Two entries run foot; the one named for it wins.
        for (id, name, exec) in [
            ("foot-server", "Foot Server", "foot --server"),
            ("foot", "Foot", "foot"),
        ] {
            let text = format!("[Desktop Entry]\nName={name}\nExec={exec}\n");
            desktop.add(parse_entry(id, &text).unwrap());
        }
        assert_eq!(
            desktop.by_program("foot").map(|e| e.name.as_str()),
            Some("Foot")
        );
        assert_eq!(
            parse_entry("link", "[Desktop Entry]\nType=Link\nName=Docs\n"),
            None
        );
        let nodes = vec![
            node(
                5203,
                900,
                "chrome",
                &format!("{USER}/app.slice/app-com.google.Chrome-5203.scope"),
            ),
            node(
                29265,
                900,
                "claude-desktop",
                &format!("{USER}/app.slice/run-p29264-i9.service"),
            ),
            node(
                700,
                900,
                "waybar",
                &format!("{USER}/app.slice/tatami-waybar.service"),
            ),
            node(
                9299,
                900,
                "electron",
                &format!("{USER}/app.slice/app-discord-9299.scope"),
            ),
        ];
        let windows = vec![Window {
            pid: 5203,
            class: "google-chrome".into(),
            title: "Inbox".into(),
        }];
        let chrome = identify(&nodes, &[0], &windows, &desktop, 1000);
        assert_eq!(chrome.kind, Kind::App);
        assert_eq!(chrome.name, "Google Chrome");
        assert_eq!(chrome.icon, Icon::Named("google-chrome".into()));
        assert_eq!(chrome.detail, "Inbox");
        assert_eq!(identify(&nodes, &[1], &[], &desktop, 1000).name, "Claude");
        assert_eq!(identify(&nodes, &[3], &[], &desktop, 1000).name, "Discord");
        let waybar = identify(&nodes, &[2], &[], &desktop, 1000);
        assert_eq!(waybar.kind, Kind::Service);
        assert_eq!(waybar.name, "waybar");
        assert_eq!(waybar.detail, "tatami-waybar.service");
        let root = Node {
            uid: 0,
            ..node(800, 1, "sshd", "/system.slice/sshd.service")
        };
        assert_eq!(
            identify(&[root], &[0], &[], &desktop, 1000).kind,
            Kind::System
        );
    }

    #[test]
    fn unit_application_ids() {
        assert_eq!(
            unit_app_ids("app-com.google.Chrome-5203.scope"),
            ["com.google.Chrome"]
        );
        assert_eq!(
            unit_app_ids("app-Hyprland-org.gnome.Nautilus-123.scope"),
            ["org.gnome.Nautilus"]
        );
        assert_eq!(
            unit_app_ids("app-Hyprland-foot@abc.service"),
            ["Hyprland-foot", "foot"]
        );
        assert_eq!(unit_app_ids("app-discord-9299.scope"), ["discord"]);
        assert_eq!(
            unit_app_ids("app-google\\x2dchrome-1.scope"),
            ["google-chrome", "chrome"]
        );
        assert!(unit_app_ids("tatami-waybar.service").is_empty());
    }

    #[test]
    fn names_and_roles() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let named = |comm: &str, list: &[&str], exe: &str| {
            let (name, role) = name_and_role(comm, &args(list), exe);
            (name, role)
        };
        let pair = |name: &str, role: &str| (name.to_owned(), role.to_owned());
        assert_eq!(
            named("chrome", &["/nix/store/x/chrome", "--type=renderer"], ""),
            pair("chrome", "Renderer")
        );
        assert_eq!(
            named(
                "chrome",
                &[
                    "/opt/google/chrome/chrome --type=utility --utility-sub-type=network.mojom.NetworkService"
                ],
                ""
            ),
            pair("chrome", "Network service")
        );
        assert_eq!(
            named(
                ".Hyprland-wrapp",
                &["/nix/store/x/bin/Hyprland", "--watchdog-fd", "4"],
                ""
            ),
            pair("Hyprland", "")
        );
        assert_eq!(
            named(".waybar-wrapped", &["/nix/store/x/bin/.waybar-wrapped"], ""),
            pair("waybar", "")
        );
        assert_eq!(
            named("python3", &["python3", "-u", "/srv/app/server.py"], ""),
            pair("python3", "server.py")
        );
        assert_eq!(
            named("python3.13", &["python3.13", "-m", "http.server"], ""),
            pair("python3.13", "http.server")
        );
        assert_eq!(
            named("bash", &["bash", "./build.sh"], ""),
            pair("bash", "build.sh")
        );
        assert_eq!(named("bash", &["bash", "-c", "make"], ""), pair("bash", ""));
        assert_eq!(named("kworker/0:1", &[], ""), pair("kworker/0:1", ""));
        assert_eq!(
            named("sshd", &["sshd: callen@pts/0"], "/nix/store/x/bin/sshd"),
            pair("sshd", "")
        );
        assert_eq!(
            exec_program("env FOO=1 /nix/store/x/bin/zed --new"),
            Some("zed".into())
        );
        assert_eq!(program("/nix/store/x/bin/.ChatGPT-wrapped"), "ChatGPT");
        assert_eq!(tidy(".Discord-wrappe"), "Discord");
    }

    #[test]
    fn windows_from_hyprland() {
        let json = r#"[{"pid": 5203, "class": "google-chrome", "title": "Inbox"},
                       {"pid": 77, "class": "", "initialClass": "foot", "title": "~"},
                       {"pid": -1, "class": "x", "title": ""}]"#;
        let windows = parse_windows(json);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[1].class, "foot");
        assert!(parse_windows("not json").is_empty());
    }
}
