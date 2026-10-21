// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Activity window, after macOS's Activity Monitor: the views (CPU,
//! Memory, Energy, GPU, Disk) in the header, the search and the grouping
//! (applications, every process, the tree) under it, the list, the summary
//! of the system below, and the inspector beside it.
//!
//! The sampler reads the system on its own thread and sends a frame every
//! few seconds; applying one updates rows in place. Typing anywhere
//! searches. Delete quits the selected process or application after asking,
//! as Force Quit does; Escape clears the search, then closes the window as
//! it does Tatami's other floating windows.
use crate::{
    entry::{self, Entry},
    inspector::Inspector,
    model::{Frame, Icon, Lens},
    sampler::{self, Command},
    summary::Summary,
    table::{Mode, Table},
};
use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

struct State {
    lens: Lens,
    mode: Mode,
    processes: HashMap<String, Entry>,
    groups: HashMap<String, Entry>,
    apps: gio::ListStore,
    all: gio::ListStore,
    tree: gio::ListStore,
    uid: u32,
    /// The latest frame's processes by PID, for actions.
    pids: HashMap<i32, String>,
    /// A process to select once the first reading is in, exactly or by its
    /// application.
    pending: Option<(i32, bool)>,
}

pub struct Ui {
    pub window: adw::ApplicationWindow,
    table: Rc<Table>,
    summary: Summary,
    inspector: Rc<Inspector>,
    split: adw::OverlaySplitView,
    search: gtk::SearchEntry,
    lenses: adw::ToggleGroup,
    modes: adw::ToggleGroup,
    toasts: adw::ToastOverlay,
    commands: mpsc::Sender<Command>,
    state: RefCell<State>,
    presented: Cell<bool>,
}

/// What a window action does.
type Run = Box<dyn Fn(&Rc<Ui>)>;

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// Processes that take the desktop with them.
const SESSION: [&str; 6] = [
    "Hyprland",
    "start-hyprland",
    "systemd",
    "dbus-broker",
    "Xwayland",
    "uwsm",
];

/// What a launch asks for: a view, a process, a search.
#[derive(Default)]
pub struct Request {
    pub lens: Option<Lens>,
    pub pid: Option<i32>,
    pub search: Option<String>,
}

/// The window, made the first time; shown when the first reading is in.
/// It opens on the process asked for, or on the application whose window
/// had the focus: what you were using when you asked what it is doing.
pub fn open(app: &adw::Application, request: Request) -> Rc<Ui> {
    if let Some(ui) = UI.with_borrow(Clone::clone) {
        if let Some(lens) = request.lens {
            ui.lenses.set_active_name(Some(lens.id()));
        }
        if let Some(search) = &request.search {
            ui.search.set_text(search);
        }
        if let Some(pid) = request.pid {
            select_pid(&ui, pid, true);
        }
        ui.window.present();
        return ui;
    }
    let pending = request
        .pid
        .map(|pid| (pid, true))
        .or_else(|| crate::apps::focused_pid().map(|pid| (pid, false)));
    let ui = build(app, request.lens.unwrap_or(Lens::Cpu));
    ui.state.borrow_mut().pending = pending;
    if let Some(search) = &request.search {
        ui.search.set_text(search);
    }
    UI.with_borrow_mut(|slot| *slot = Some(ui.clone()));
    let soon = ui.clone();
    glib::timeout_add_local_once(Duration::from_millis(600), move || present(&soon));
    ui
}

fn present(ui: &Ui) {
    if !ui.presented.replace(true) {
        ui.window.present();
    }
}

fn toast(ui: &Ui, text: &str) {
    let toast = adw::Toast::new(text);
    toast.set_use_markup(false);
    ui.toasts.add_toast(toast);
}

fn toggle(name: &str, label: &str, tooltip: &str) -> adw::Toggle {
    adw::Toggle::builder()
        .name(name)
        .label(label)
        .tooltip(tooltip)
        .build()
}

fn primary_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let interval = gio::Menu::new();
    interval.append(Some("Update Every Second"), Some("win.interval::1"));
    interval.append(Some("Update Every 2 Seconds"), Some("win.interval::2"));
    interval.append(Some("Update Every 5 Seconds"), Some("win.interval::5"));
    menu.append_section(None, &interval);
    let shown = gio::Menu::new();
    shown.append(Some("Show Other Users' Processes"), Some("win.everyone"));
    shown.append(Some("Show Kernel Threads"), Some("win.kernel"));
    menu.append_section(None, &shown);
    let shortcuts = gio::Menu::new();
    shortcuts.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
    menu.append_section(None, &shortcuts);
    menu
}

fn context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let signals = gio::Menu::new();
    signals.append(Some("Quit"), Some("win.quit"));
    signals.append(Some("Force Quit"), Some("win.force-quit"));
    signals.append(Some("Pause"), Some("win.pause"));
    signals.append(Some("Resume"), Some("win.resume"));
    menu.append_section(None, &signals);
    let copy = gio::Menu::new();
    copy.append(Some("Copy Command Line"), Some("win.copy-command"));
    copy.append(Some("Copy PID"), Some("win.copy-pid"));
    menu.append_section(None, &copy);
    let more = gio::Menu::new();
    more.append(Some("Show Details"), Some("win.inspect"));
    menu.append_section(None, &more);
    menu
}

fn build(app: &adw::Application, lens: Lens) -> Rc<Ui> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Activity")
        .icon_name("utilities-system-monitor")
        .default_width(1160)
        .default_height(720)
        .build();

    // The views, in the header.
    let lenses = adw::ToggleGroup::new();
    lenses.add_css_class("lenses");
    for (lens, tip) in [
        (Lens::Cpu, "Processor use (Ctrl+1)"),
        (Lens::Memory, "Memory use (Ctrl+2)"),
        (Lens::Energy, "Estimated power and energy (Ctrl+3)"),
        (Lens::Gpu, "Graphics processors (Ctrl+4)"),
        (Lens::Disk, "Storage reads and writes (Ctrl+5)"),
    ] {
        lenses.add(toggle(lens.id(), lens.title(), tip));
    }
    lenses.set_active_name(Some(lens.id()));
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&lenses));
    let menu = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&primary_menu())
        .tooltip_text("Main Menu")
        .primary(true)
        .build();
    header.pack_end(&menu);
    let details = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-right-symbolic")
        .tooltip_text("Inspector (Ctrl+I)")
        .active(true)
        .build();
    header.pack_end(&details);

    // The search and the grouping, under it.
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search applications and processes")
        .hexpand(true)
        .build();
    let modes = adw::ToggleGroup::new();
    modes.add(toggle(
        Mode::Apps.id(),
        "Applications",
        "Each application with its processes",
    ));
    modes.add(toggle(Mode::All.id(), "Processes", "Every process"));
    modes.add(toggle(
        Mode::Tree.id(),
        "Tree",
        "Processes under the processes that started them",
    ));
    modes.set_active_name(Some(Mode::Apps.id()));
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    bar.add_css_class("toolbar");
    bar.set_margin_start(6);
    bar.set_margin_end(6);
    bar.append(&search);
    bar.append(&modes);

    let table = Table::new(context_menu().upcast_ref());
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&table.view)
        .build();
    // The summary spans the window under the list and the inspector, as
    // Activity Monitor's does.
    let summary = Summary::new();

    let inspector = Inspector::new();
    let split = adw::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::End)
        .content(&scroller)
        .sidebar(&inspector.root)
        .min_sidebar_width(280.0)
        .max_sidebar_width(360.0)
        .sidebar_width_fraction(0.28)
        .show_sidebar(true)
        .build();
    details
        .bind_property("active", &split, "show-sidebar")
        .bidirectional()
        .sync_create()
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&split));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.add_top_bar(&bar);
    view.set_content(Some(&toasts));
    view.add_bottom_bar(&summary.stack);
    view.set_bottom_bar_style(adw::ToolbarStyle::Raised);
    window.set_content(Some(&view));
    // A narrow window lays the inspector over the list.
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        880.0,
        adw::LengthUnit::Sp,
    ));
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    narrow.add_setter(&split, "show-sidebar", Some(&false.to_value()));
    window.add_breakpoint(narrow);
    search.set_key_capture_widget(Some(&window));

    let (frames, receiver) = async_channel::bounded::<Frame>(2);
    let commands = sampler::spawn(frames, Duration::from_secs(2));
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let state = State {
        lens,
        mode: Mode::Apps,
        processes: HashMap::new(),
        groups: HashMap::new(),
        apps: gio::ListStore::new::<Entry>(),
        all: gio::ListStore::new::<Entry>(),
        tree: gio::ListStore::new::<Entry>(),
        uid,
        pids: HashMap::new(),
        pending: None,
    };
    table.filter.borrow_mut().uid = uid;
    table.set_root(&state.apps, Mode::Apps);
    table.set_lens(lens);
    summary.set_lens(lens);
    let ui = Rc::new(Ui {
        window: window.clone(),
        table: table.clone(),
        summary,
        inspector: inspector.clone(),
        split,
        search: search.clone(),
        lenses: lenses.clone(),
        modes: modes.clone(),
        toasts,
        commands,
        state: RefCell::new(state),
        presented: Cell::new(false),
    });

    // Frames as they come.
    let weak = Rc::downgrade(&ui);
    glib::spawn_future_local(async move {
        while let Ok(frame) = receiver.recv().await {
            let Some(ui) = weak.upgrade() else {
                break;
            };
            apply(&ui, &frame);
            let pending = ui.state.borrow_mut().pending.take();
            if let Some((pid, exact)) = pending
                && pid != std::process::id() as i32
            {
                select_pid(&ui, pid, exact);
            }
            present(&ui);
        }
    });

    let weak = Rc::downgrade(&ui);
    lenses.connect_active_name_notify(move |group| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let lens = group
            .active_name()
            .as_deref()
            .and_then(Lens::from_id)
            .unwrap_or(Lens::Cpu);
        ui.state.borrow_mut().lens = lens;
        ui.table.set_lens(lens);
        ui.summary.set_lens(lens);
        focus(&ui);
    });
    let weak = Rc::downgrade(&ui);
    modes.connect_active_name_notify(move |group| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mode = Mode::from_id(group.active_name().as_deref().unwrap_or("apps"));
        ui.state.borrow_mut().mode = mode;
        if let Some(action) = ui
            .window
            .lookup_action("mode")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_state(&mode.id().to_variant());
        }
        if mode == Mode::Tree {
            sync_tree(&ui);
        }
        let root = {
            let state = ui.state.borrow();
            match mode {
                Mode::Apps => state.apps.clone(),
                Mode::All => state.all.clone(),
                Mode::Tree => state.tree.clone(),
            }
        };
        ui.table.set_root(&root, mode);
    });
    let weak = Rc::downgrade(&ui);
    search.connect_search_changed(move |search| {
        if let Some(ui) = weak.upgrade() {
            ui.table.filter.borrow_mut().query = search.text().trim().to_lowercase();
            ui.table.refilter();
        }
    });
    let weak = Rc::downgrade(&ui);
    search.connect_stop_search(move |search| {
        if search.text().is_empty() {
            if let Some(ui) = weak.upgrade() {
                ui.window.close();
            }
        } else {
            search.set_text("");
        }
    });
    let weak = Rc::downgrade(&ui);
    table.selection.connect_selection_changed(move |_, _, _| {
        if let Some(ui) = weak.upgrade() {
            selected(&ui);
        }
    });
    let weak = Rc::downgrade(&ui);
    table.view.connect_activate(move |_, _| {
        if let Some(ui) = weak.upgrade() {
            ui.split.set_show_sidebar(true);
        }
    });
    let weak = Rc::downgrade(&ui);
    *inspector.on_select.borrow_mut() = Some(Box::new(move |pid| {
        if let Some(ui) = weak.upgrade() {
            select_pid(&ui, pid, true);
        }
    }));

    actions(&ui);
    shortcuts(&ui);
    selected(&ui);
    ui
}

/// Tell the sampler what is showing, so it reads those figures each time.
fn focus(ui: &Ui) {
    let pids = ui
        .table
        .selected()
        .map(|entry| entry.data().members.clone())
        .unwrap_or_default();
    let lens = ui.state.borrow().lens;
    let _ = ui.commands.send(Command::Focus { lens, pids });
}

fn selected(ui: &Rc<Ui>) {
    let entry = ui.table.selected();
    ui.inspector.show(entry.as_ref());
    update_actions(ui);
    focus(ui);
}

/// Select a process: its own row (opening its application to show it), or
/// with `exact` false its application's row.
fn select_pid(ui: &Ui, pid: i32, exact: bool) {
    let key = ui.state.borrow().pids.get(&pid).cloned();
    let Some(key) = key else {
        toast(ui, &format!("Process {pid} has ended"));
        return;
    };
    let group = {
        let state = ui.state.borrow();
        state
            .processes
            .get(&key)
            .and_then(|entry| Some(entry.data().process.as_ref()?.group.clone()))
    };
    let selected = if exact {
        ui.table.select(&key)
            || group.as_ref().is_some_and(|group| {
                ui.table.expand(group) && ui.table.select(&key) || ui.table.select(group)
            })
    } else {
        group.as_ref().is_some_and(|group| ui.table.select(group)) || ui.table.select(&key)
    };
    if !selected {
        toast(
            ui,
            "That process is hidden by the search or the main menu's choices",
        );
    }
}

/// Show a frame. The rows are found or made with the state borrowed, then
/// updated and moved with it let go: updates and list changes run handlers
/// (the selection's, the inspector's) that read the state themselves.
fn apply(ui: &Rc<Ui>, frame: &Frame) {
    // A process shows its application's icon.
    let icons: HashMap<&str, Icon> = frame
        .groups
        .iter()
        .filter(|group| group.icon != Icon::None)
        .map(|group| (group.key.as_str(), group.icon.clone()))
        .collect();
    let (processes, groups, stores, mode) = {
        let mut state = ui.state.borrow_mut();
        let state = &mut *state;
        state.uid = frame.uid;
        state.pids = frame
            .processes
            .iter()
            .map(|p| (p.pid, p.key.clone()))
            .collect();
        let processes: Vec<Entry> = frame
            .processes
            .iter()
            .map(|process| {
                state
                    .processes
                    .entry(process.key.clone())
                    .or_insert_with(|| Entry::new(&process.key, false))
                    .clone()
            })
            .collect();
        let live: HashSet<&str> = frame.processes.iter().map(|p| p.key.as_str()).collect();
        state.processes.retain(|key, _| live.contains(key.as_str()));
        let groups: Vec<Entry> = frame
            .groups
            .iter()
            .map(|group| {
                state
                    .groups
                    .entry(group.key.clone())
                    .or_insert_with(|| Entry::new(&group.key, true))
                    .clone()
            })
            .collect();
        let live: HashSet<&str> = frame.groups.iter().map(|g| g.key.as_str()).collect();
        state.groups.retain(|key, _| live.contains(key.as_str()));
        (
            processes,
            groups,
            (state.apps.clone(), state.all.clone(), state.tree.clone()),
            state.mode,
        )
    };

    let mut by_pid: HashMap<i32, Entry> = HashMap::new();
    for (process, entry) in frame.processes.iter().zip(&processes) {
        let icon = icons
            .get(process.group.as_str())
            .cloned()
            .unwrap_or_default();
        entry.update_process(process, &icon);
        by_pid.insert(process.pid, entry.clone());
    }
    for (group, entry) in frame.groups.iter().zip(&groups) {
        let only = if group.members.len() == 1 {
            frame.processes.iter().find(|p| p.pid == group.members[0])
        } else {
            None
        };
        let members: Vec<Entry> = group
            .members
            .iter()
            .filter_map(|pid| by_pid.get(pid).cloned())
            .collect();
        entry::sync(&entry.children(), &members);
        entry.update_group(group, only);
    }
    let (apps, all, tree) = stores;
    entry::sync(&apps, &groups);
    entry::sync(&all, &processes);
    if mode == Mode::Tree {
        let parents: HashMap<i32, i32> = frame.processes.iter().map(|p| (p.pid, p.ppid)).collect();
        sync_tree_with(&tree, &parents, &by_pid);
    }
    ui.inspector
        .set_gpus(frame.gpus.iter().map(|gpu| gpu.name.clone()).collect());
    ui.summary.update(frame);
    ui.table.resort();
    update_actions(ui);
}

fn sync_tree(ui: &Ui) {
    let (tree, entries) = {
        let state = ui.state.borrow();
        (
            state.tree.clone(),
            state.processes.values().cloned().collect::<Vec<_>>(),
        )
    };
    let mut parents = HashMap::new();
    let mut by_pid = HashMap::new();
    for entry in entries {
        let process = entry.data().process.as_ref().map(|p| (p.pid, p.ppid));
        if let Some((pid, ppid)) = process {
            parents.insert(pid, ppid);
            by_pid.insert(pid, entry);
        }
    }
    sync_tree_with(&tree, &parents, &by_pid);
}

/// Processes under their parents; those whose parent isn't there at the
/// top.
fn sync_tree_with(
    tree: &gio::ListStore,
    parents: &HashMap<i32, i32>,
    by_pid: &HashMap<i32, Entry>,
) {
    let mut children: HashMap<i32, Vec<Entry>> = HashMap::new();
    let mut roots = Vec::new();
    let mut pids: Vec<&i32> = by_pid.keys().collect();
    pids.sort();
    for pid in pids {
        let entry = &by_pid[pid];
        match parents.get(pid).filter(|ppid| by_pid.contains_key(ppid)) {
            Some(ppid) => children.entry(*ppid).or_default().push(entry.clone()),
            None => roots.push(entry.clone()),
        }
    }
    for (pid, entry) in by_pid {
        let wanted = children.remove(pid).unwrap_or_default();
        if !wanted.is_empty() || entry.has_children() {
            entry::sync(&entry.children(), &wanted);
        }
    }
    entry::sync(tree, &roots);
}

/// What the actions would act on.
struct Target {
    name: String,
    /// Every PID, for Force Quit and Pause.
    all: Vec<i32>,
    /// The first processes, for Quit: their children follow them.
    roots: Vec<i32>,
    mine: bool,
    paused: bool,
    session: bool,
    command: String,
    pid: Option<i32>,
}

fn target(ui: &Ui) -> Option<Target> {
    let entry = ui.table.selected()?;
    let data = entry.data();
    let state = ui.state.borrow();
    let members: HashSet<i32> = data.members.iter().copied().collect();
    let processes: Vec<_> = data
        .members
        .iter()
        .filter_map(|pid| state.pids.get(pid))
        .filter_map(|key| state.processes.get(key))
        .filter_map(|entry| entry.data().process.clone())
        .collect();
    let roots = processes
        .iter()
        .filter(|p| !members.contains(&p.ppid))
        .map(|p| p.pid)
        .collect();
    let mine = !processes.is_empty() && processes.iter().all(|p| p.uid == state.uid);
    let paused = !processes.is_empty() && processes.iter().all(|p| p.state == 'T');
    let me = std::process::id() as i32;
    let session = processes
        .iter()
        .any(|p| SESSION.contains(&p.name.as_str()) || p.pid == me);
    Some(Target {
        name: data.name.clone(),
        all: data.members.clone(),
        roots,
        mine,
        paused,
        session,
        command: data
            .process
            .as_ref()
            .map(|p| p.command.clone())
            .unwrap_or_else(|| data.name.clone()),
        pid: data.pid,
    })
}

fn update_actions(ui: &Ui) {
    let target = target(ui);
    let set = |name: &str, enabled: bool| {
        if let Some(action) = ui
            .window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(enabled);
        }
    };
    let mine = target.as_ref().is_some_and(|t| t.mine);
    let paused = target.as_ref().is_some_and(|t| t.paused);
    ui.inspector.set_paused(paused);
    set("quit", mine);
    set("force-quit", mine);
    set("pause", mine && !paused);
    set("resume", mine && paused);
    set("copy-command", target.is_some());
    set("copy-pid", target.as_ref().is_some_and(|t| t.pid.is_some()));
    set("inspect", target.is_some());
}

fn signal(ui: &Ui, pids: &[i32], signal: i32, what: &str, name: &str) {
    let mut failed = 0;
    for &pid in pids {
        // SAFETY: kill has no memory-safety preconditions.
        if unsafe { libc::kill(pid, signal) } != 0 {
            failed += 1;
        }
    }
    if failed == pids.len() && !pids.is_empty() {
        toast(ui, &format!("Could not {what} {name}"));
    }
}

fn confirm_quit(ui: &Rc<Ui>, force: bool) {
    let Some(target) = target(ui) else {
        return;
    };
    if !target.mine {
        toast(ui, &format!("{} belongs to another user", target.name));
        return;
    }
    let count = target.all.len();
    let mut body = if count > 1 {
        format!("Its {count} processes end.")
    } else {
        String::new()
    };
    if target.session {
        body = "Quitting it ends your session, or this window: everything open now closes.".into();
    } else {
        if !body.is_empty() {
            body.push(' ');
        }
        body.push_str(if force {
            "Force quitting ends it at once: anything not saved is lost."
        } else {
            "It is asked to quit, and can save what it needs to first."
        });
    }
    let dialog = adw::AlertDialog::new(Some(&format!("Quit “{}”?", target.name)), Some(&body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("force", "Force Quit");
    dialog.set_response_appearance("force", adw::ResponseAppearance::Destructive);
    if !force {
        dialog.add_response("quit", "Quit");
        dialog.set_response_appearance("quit", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("quit"));
    } else {
        dialog.set_default_response(Some("force"));
    }
    dialog.set_close_response("cancel");
    let weak = Rc::downgrade(ui);
    dialog.connect_response(None, move |_, response| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        match response {
            "quit" => signal(&ui, &target.roots, libc::SIGTERM, "quit", &target.name),
            "force" => signal(&ui, &target.all, libc::SIGKILL, "force quit", &target.name),
            _ => {}
        }
    });
    dialog.present(Some(&ui.window));
}

fn actions(ui: &Rc<Ui>) {
    let window = &ui.window;
    let add = |name: &str, run: Run| {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(ui);
        action.connect_activate(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                run(&ui);
            }
        });
        window.add_action(&action);
    };
    add("quit", Box::new(|ui| confirm_quit(ui, false)));
    add("force-quit", Box::new(|ui| confirm_quit(ui, true)));
    add(
        "pause",
        Box::new(|ui| {
            let Some(target) = target(ui) else {
                return;
            };
            if target.session {
                toast(
                    ui,
                    &format!("Pausing {} would freeze the desktop", target.name),
                );
                return;
            }
            signal(ui, &target.all, libc::SIGSTOP, "pause", &target.name);
        }),
    );
    add(
        "resume",
        Box::new(|ui| {
            if let Some(target) = target(ui) {
                signal(ui, &target.all, libc::SIGCONT, "resume", &target.name);
            }
        }),
    );
    add(
        "copy-command",
        Box::new(|ui| {
            if let Some(target) = target(ui) {
                ui.window.clipboard().set_text(&target.command);
                toast(ui, "Command line copied");
            }
        }),
    );
    add(
        "copy-pid",
        Box::new(|ui| {
            if let Some(pid) = target(ui).and_then(|t| t.pid) {
                ui.window.clipboard().set_text(&pid.to_string());
                toast(ui, &format!("PID {pid} copied"));
            }
        }),
    );
    add("inspect", Box::new(|ui| ui.split.set_show_sidebar(true)));
    add(
        "toggle-inspector",
        Box::new(|ui| ui.split.set_show_sidebar(!ui.split.shows_sidebar())),
    );
    add(
        "find",
        Box::new(|ui| {
            ui.search.grab_focus();
        }),
    );
    add("menu", Box::new(|ui| ui.table.popup_menu()));
    add("shortcuts", Box::new(show_shortcuts));
    add(
        "escape",
        Box::new(|ui| {
            if !ui.search.text().is_empty() {
                ui.search.set_text("");
            } else if ui.split.is_collapsed() && ui.split.shows_sidebar() {
                ui.split.set_show_sidebar(false);
            } else {
                ui.window.close();
            }
        }),
    );
    for lens in Lens::ALL {
        add(
            &format!("lens-{}", lens.id()),
            Box::new(move |ui| ui.lenses.set_active_name(Some(lens.id()))),
        );
    }

    let interval = gio::SimpleAction::new_stateful(
        "interval",
        Some(glib::VariantTy::STRING),
        &"2".to_variant(),
    );
    let weak = Rc::downgrade(ui);
    interval.connect_activate(move |action, value| {
        let (Some(ui), Some(value)) = (weak.upgrade(), value.and_then(|v| v.get::<String>()))
        else {
            return;
        };
        let seconds: u64 = value.parse().unwrap_or(2);
        action.set_state(&value.to_variant());
        let _ = ui
            .commands
            .send(Command::Interval(Duration::from_secs(seconds)));
    });
    window.add_action(&interval);
    // Applications, every process or the tree, as the toggles under the
    // header choose.
    let mode = gio::SimpleAction::new_stateful(
        "mode",
        Some(glib::VariantTy::STRING),
        &Mode::Apps.id().to_variant(),
    );
    let weak = Rc::downgrade(ui);
    mode.connect_activate(move |_, value| {
        if let (Some(ui), Some(value)) = (weak.upgrade(), value.and_then(|v| v.get::<String>())) {
            ui.modes.set_active_name(Some(Mode::from_id(&value).id()));
        }
    });
    window.add_action(&mode);
    for (name, initial) in [("everyone", true), ("kernel", false)] {
        let action = gio::SimpleAction::new_stateful(name, None, &initial.to_variant());
        let weak = Rc::downgrade(ui);
        let key = name.to_owned();
        action.connect_activate(move |action, _| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let on = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&on.to_variant());
            {
                let mut filter = ui.table.filter.borrow_mut();
                if key == "everyone" {
                    filter.everyone = on;
                } else {
                    filter.kernel = on;
                }
            }
            ui.table.refilter();
        });
        window.add_action(&action);
    }
}

fn shortcuts(ui: &Rc<Ui>) {
    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Managed);
    let bind = |trigger: &str, action: &str| {
        controller.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(trigger),
            Some(gtk::NamedAction::new(action)),
        ));
    };
    bind("Escape", "win.escape");
    bind("Delete", "win.quit");
    bind("<Shift>Delete", "win.force-quit");
    bind("<Control>i", "win.toggle-inspector");
    bind("<Control>f", "win.find");
    bind("<Control>c", "win.copy-command");
    bind("Menu|<Shift>F10", "win.menu");
    bind("<Control>question", "win.shortcuts");
    for (index, lens) in Lens::ALL.iter().enumerate() {
        bind(
            &format!("<Control>{}", index + 1),
            &format!("win.lens-{}", lens.id()),
        );
    }
    for (trigger, mode) in [
        ("<Control><Shift>a", Mode::Apps),
        ("<Control><Shift>p", Mode::All),
        ("<Control><Shift>t", Mode::Tree),
    ] {
        controller.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(trigger),
            Some(gtk::NamedAction::new("win.mode")),
        ));
        if let Some(shortcut) = controller
            .item(controller.n_items() - 1)
            .and_downcast::<gtk::Shortcut>()
        {
            shortcut.set_arguments(Some(&mode.id().to_variant()));
        }
    }
    ui.window.add_controller(controller);
}

fn show_shortcuts(ui: &Rc<Ui>) {
    let rows = [
        ("Search", "Type anywhere, or Ctrl+F"),
        ("Clear the search, then close", "Escape"),
        ("CPU, Memory, Energy, GPU, Disk", "Ctrl+1 to Ctrl+5"),
        ("Applications, Processes, Tree", "Ctrl+Shift+A, P, T"),
        ("Quit the selected", "Delete"),
        ("Force quit the selected", "Shift+Delete"),
        ("Show or hide the inspector", "Ctrl+I"),
        ("Copy the command line", "Ctrl+C"),
        ("Open the selected's menu", "Menu or Shift+F10"),
        ("Open or close an application", "+ and −"),
    ];
    let group = adw::PreferencesGroup::new();
    for (what, keys) in rows {
        let row = adw::ActionRow::builder()
            .title(what)
            .use_markup(false)
            .build();
        row.add_suffix(
            &gtk::Label::builder()
                .label(keys)
                .css_classes(["dim-label"])
                .build(),
        );
        group.add(&row);
    }
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let dialog = adw::Dialog::builder()
        .title("Keyboard Shortcuts")
        .content_width(460)
        .child(&{
            let view = adw::ToolbarView::new();
            view.add_top_bar(&adw::HeaderBar::new());
            view.set_content(Some(&page));
            view
        })
        .build();
    dialog.present(Some(&ui.window));
}
