// SPDX-License-Identifier: MIT OR Apache-2.0
//! The inspector beside the list: everything about the selected process or
//! application, kept up to date: its processor and memory over the last
//! few minutes, every measure of its memory, its energy, graphics and
//! storage, and what it is (command line, program, folder, parent, unit).
//! Quit, Force Quit and Pause are at the top, as the window's actions.
use crate::{
    entry::{self, Data, Entry},
    graph::{Graph, Meter},
    model,
    summary::short_name,
};
use adw::prelude::*;
use gtk::glib;
use std::{cell::RefCell, rc::Rc};

type Value = Box<dyn Fn(&Data) -> Option<String>>;
type Select = Box<dyn Fn(i32)>;

struct Row {
    row: gtk::ListBoxRow,
    value: gtk::Label,
    get: Value,
    /// A long value (a command line) goes under its name, selectable, and
    /// the row hides when there is none.
    long: bool,
}

/// A titled list of names and values, denser than preference rows: a
/// monitor shows many figures at once.
struct Section {
    root: gtk::Box,
    list: gtk::ListBox,
    note: gtk::Label,
}

impl Section {
    fn new(title: &str) -> Section {
        let heading = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list", "inspector-list"])
            .build();
        let note = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption", "dim-label"])
            .visible(false)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.append(&heading);
        root.append(&list);
        root.append(&note);
        Section { root, list, note }
    }

    fn describe(&self, text: &str) {
        self.note.set_label(text);
        self.note.set_visible(true);
    }
}

pub struct Inspector {
    pub root: gtk::Stack,
    icon: gtk::Image,
    title: gtk::Label,
    detail: gtk::Label,
    meta: gtk::Label,
    pause: gtk::Button,
    cpu: Graph,
    memory: Graph,
    breakdown: Meter,
    breakdown_label: gtk::Label,
    sections: gtk::Box,
    rows: RefCell<Vec<Row>>,
    current: RefCell<Option<(Entry, glib::SignalHandlerId)>>,
    /// Select the process with this PID (the parent row).
    pub on_select: RefCell<Option<Select>>,
    gpus: RefCell<Vec<String>>,
}

impl Inspector {
    pub fn new() -> Rc<Inspector> {
        let placeholder = adw::StatusPage::builder()
            .icon_name("view-list-symbolic")
            .title("Nothing Selected")
            .description("Select an application or process to see everything about it.")
            .css_classes(["compact"])
            .build();

        let icon = gtk::Image::builder()
            .pixel_size(40)
            .valign(gtk::Align::Start)
            .build();
        let title = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["title-3"])
            .build();
        let detail = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label"])
            .build();
        let meta = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption", "dim-label", "numeric"])
            .build();
        let names = gtk::Box::new(gtk::Orientation::Vertical, 2);
        names.set_hexpand(true);
        names.append(&title);
        names.append(&detail);
        names.append(&meta);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        heading.append(&icon);
        heading.append(&names);

        let button = |label: &str, action: &str| {
            gtk::Button::builder()
                .label(label)
                .action_name(action)
                .hexpand(true)
                .build()
        };
        let quit = button("Quit", "win.quit");
        let force = button("Force Quit", "win.force-quit");
        force.add_css_class("destructive-action");
        let pause = button("Pause", "win.pause");
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_homogeneous(true);
        actions.append(&quit);
        actions.append(&force);
        actions.append(&pause);

        let cpu = Graph::new(&["cpu-user"], None, false, 56);
        cpu.set_least(100.0);
        let memory = Graph::new(&["memory-apps"], None, false, 56);
        memory.set_least(64.0 * 1024.0 * 1024.0);
        let graphs = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        graphs.set_homogeneous(true);
        for (name, graph) in [("Processor", &cpu), ("Memory", &memory)] {
            let block = gtk::Box::new(gtk::Orientation::Vertical, 4);
            block.append(
                &gtk::Label::builder()
                    .label(name)
                    .xalign(0.0)
                    .css_classes(["caption-heading", "dim-label"])
                    .build(),
            );
            block.append(graph.widget());
            graphs.append(&block);
        }
        let breakdown = Meter::new(&["memory-private", "memory-shared", "memory-swap"]);
        let breakdown_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption", "dim-label"])
            .build();

        let sections = gtk::Box::new(gtk::Orientation::Vertical, 18);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(16);
        column.set_margin_bottom(24);
        column.set_margin_start(16);
        column.set_margin_end(16);
        column.append(&heading);
        column.append(&actions);
        column.append(&graphs);
        column.append(breakdown.widget());
        column.append(&breakdown_label);
        column.append(&sections);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build();

        let root = gtk::Stack::new();
        root.add_css_class("inspector");
        root.add_named(&placeholder, Some("empty"));
        root.add_named(&scroller, Some("details"));
        root.set_visible_child_name("empty");

        Rc::new(Inspector {
            root,
            icon,
            title,
            detail,
            meta,
            pause,
            cpu,
            memory,
            breakdown,
            breakdown_label,
            sections,
            rows: RefCell::new(Vec::new()),
            current: RefCell::new(None),
            on_select: RefCell::new(None),
            gpus: RefCell::new(Vec::new()),
        })
    }

    /// The graphics processors' names, for the Graphics rows.
    pub fn set_gpus(&self, names: Vec<String>) {
        *self.gpus.borrow_mut() = names;
    }

    /// Use the same whole-selection state as the window's actions. An
    /// application's aggregate row has no single `process` to inspect.
    pub fn set_paused(&self, paused: bool) {
        self.pause
            .set_label(if paused { "Resume" } else { "Pause" });
        self.pause
            .set_action_name(Some(if paused { "win.resume" } else { "win.pause" }));
    }

    pub fn show(self: &Rc<Inspector>, entry: Option<&Entry>) {
        if let Some((old, handler)) = self.current.borrow_mut().take() {
            old.disconnect(handler);
        }
        let Some(entry) = entry else {
            self.root.set_visible_child_name("empty");
            return;
        };
        self.build(&entry.data());
        let weak = Rc::downgrade(self);
        let handler = entry.connect_changed(move |entry| {
            if let Some(inspector) = weak.upgrade() {
                inspector.update(&entry.data(), true);
            }
        });
        *self.current.borrow_mut() = Some((entry.clone(), handler));
        self.update(&entry.data(), false);
        self.root.set_visible_child_name("details");
    }

    fn build(self: &Rc<Inspector>, data: &Data) {
        while let Some(child) = self.sections.first_child() {
            self.sections.remove(&child);
        }
        let mut rows = Vec::new();
        let mut add = |section: &Section, title: &str, long: bool, get: Value| {
            let name = gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .hexpand(!long)
                .build();
            let value = gtk::Label::builder()
                .xalign(if long { 0.0 } else { 1.0 })
                .css_classes(["numeric", "dim-label"])
                .build();
            let content = gtk::Box::new(
                if long {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                },
                if long { 2 } else { 12 },
            );
            content.add_css_class("inspector-row");
            if long {
                name.add_css_class("caption");
                name.add_css_class("dim-label");
                value.remove_css_class("dim-label");
                value.set_selectable(true);
                value.set_wrap(true);
                value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                value.add_css_class("monospace");
            }
            content.append(&name);
            content.append(&value);
            let row = gtk::ListBoxRow::builder()
                .activatable(false)
                .selectable(false)
                .child(&content)
                .build();
            section.list.append(&row);
            rows.push(Row {
                row,
                value,
                get,
                long,
            });
        };
        let gpus = self.gpus.borrow().clone();
        let single = !data.group || data.count == 1;

        let processor = Section::new("Processor");
        add(
            &processor,
            "% CPU",
            false,
            Box::new(|d| Some(model::percent(d.usage.cpu))),
        );
        add(
            &processor,
            "CPU time",
            false,
            Box::new(|d| Some(model::cpu_time(d.usage.cpu_time))),
        );
        add(
            &processor,
            "Threads",
            false,
            Box::new(|d| Some(model::grouped(d.usage.threads))),
        );
        add(
            &processor,
            "Wake-ups a second",
            false,
            Box::new(|d| Some(format!("{:.0}", d.usage.wakeups))),
        );
        if single {
            add(
                &processor,
                "Priority",
                false,
                Box::new(|d| {
                    let nice = d.process.as_ref()?.nice;
                    Some(match nice {
                        0 => "Normal".into(),
                        n if n < 0 => format!("Higher ({n})"),
                        n => format!("Lower (+{n})"),
                    })
                }),
            );
        }
        self.sections.append(&processor.root);

        let memory = Section::new("Memory");
        add(
            &memory,
            "Memory",
            false,
            Box::new(|d| Some(model::bytes(d.usage.memory()))),
        );
        add(
            &memory,
            "Private",
            false,
            Box::new(|d| d.usage.private.map(model::bytes)),
        );
        add(
            &memory,
            "Shared",
            false,
            Box::new(|d| d.usage.shared.map(model::bytes)),
        );
        add(
            &memory,
            "Resident",
            false,
            Box::new(|d| Some(model::bytes(d.usage.resident))),
        );
        add(
            &memory,
            "Swap",
            false,
            Box::new(|d| Some(model::bytes(d.usage.swap))),
        );
        add(
            &memory,
            "Virtual",
            false,
            Box::new(|d| Some(model::bytes(d.usage.virt))),
        );
        add(
            &memory,
            "Video memory",
            false,
            Box::new(|d| Some(model::bytes(d.usage.vram))),
        );
        memory.describe(if data.usage.pss.is_some() || data.group {
            "Memory is what it uses alone plus its share of what it shares (proportional set size)."
        } else {
            "Another user's memory maps can't be read, so Memory is its resident size."
        });
        self.sections.append(&memory.root);

        let energy = Section::new("Energy");
        add(
            &energy,
            "Power",
            false,
            Box::new(|d| d.usage.power.map(model::watts)),
        );
        add(
            &energy,
            "Average",
            false,
            Box::new(|d| d.usage.average_power().map(model::watts)),
        );
        add(
            &energy,
            "Used since opened",
            false,
            Box::new(|d| (d.usage.measured > 0.0).then(|| model::energy(d.usage.energy))),
        );
        energy.describe("Estimated: the measured power of the processor and graphics, shared by how much of each it uses.");
        self.sections.append(&energy.root);

        if !gpus.is_empty() {
            let graphics = Section::new("Graphics");
            if single {
                for (index, name) in gpus.iter().enumerate() {
                    add(
                        &graphics,
                        &short_name(name),
                        false,
                        Box::new(move |d| {
                            let gpu = d.process.as_ref()?.gpus.iter().find(|g| g.device == index);
                            Some(match gpu {
                                Some(gpu) => format!(
                                    "{}% · {}",
                                    model::percent(gpu.busy),
                                    model::bytes(gpu.vram)
                                ),
                                None => "Not in use".into(),
                            })
                        }),
                    );
                }
            }
            add(
                &graphics,
                "% GPU",
                false,
                Box::new(|d| Some(model::percent(d.usage.gpu))),
            );
            add(
                &graphics,
                "% Video",
                false,
                Box::new(|d| Some(model::percent(d.usage.video))),
            );
            add(
                &graphics,
                "Shared GPU memory",
                false,
                Box::new(|d| Some(model::bytes(d.usage.gpu_shared))),
            );
            self.sections.append(&graphics.root);
        }

        let disk = Section::new("Disk");
        add(
            &disk,
            "Reading",
            false,
            Box::new(|d| d.usage.read_rate.map(model::rate)),
        );
        add(
            &disk,
            "Writing",
            false,
            Box::new(|d| d.usage.write_rate.map(model::rate)),
        );
        add(
            &disk,
            "Read since it started",
            false,
            Box::new(|d| d.usage.read_total.map(model::bytes)),
        );
        add(
            &disk,
            "Written since it started",
            false,
            Box::new(|d| d.usage.write_total.map(model::bytes)),
        );
        self.sections.append(&disk.root);

        let about = Section::new(if single { "Process" } else { "Application" });
        if single {
            add(
                &about,
                "Command",
                true,
                Box::new(|d| Some(d.process.as_ref()?.command.clone())),
            );
            add(
                &about,
                "Program",
                true,
                Box::new(|d| Some(d.process.as_ref()?.exe.clone()).filter(|exe| !exe.is_empty())),
            );
            add(
                &about,
                "Folder",
                true,
                Box::new(|d| Some(d.process.as_ref()?.cwd.clone()).filter(|cwd| !cwd.is_empty())),
            );
            add(
                &about,
                "Started",
                false,
                Box::new(|d| {
                    let started = d.process.as_ref()?.started;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|since| since.as_secs())
                        .unwrap_or(0);
                    Some(model::ago(now.saturating_sub(started)))
                }),
            );
        } else {
            add(
                &about,
                "Processes",
                false,
                Box::new(|d| Some(d.count.to_string())),
            );
            add(
                &about,
                "Windows",
                false,
                Box::new(|d| {
                    Some(if d.windows > 0 {
                        d.windows.to_string()
                    } else {
                        "None".into()
                    })
                }),
            );
        }
        add(
            &about,
            "Unit",
            true,
            Box::new(|d| Some(d.process.as_ref()?.unit.clone()).filter(|unit| !unit.is_empty())),
        );
        // The parent, which selects it.
        if let Some(ppid) = data
            .process
            .as_ref()
            .filter(|_| single)
            .map(|p| p.ppid)
            .filter(|ppid| *ppid > 0)
        {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            content.add_css_class("inspector-row");
            content.append(
                &gtk::Label::builder()
                    .label("Parent")
                    .xalign(0.0)
                    .hexpand(true)
                    .build(),
            );
            content.append(
                &gtk::Label::builder()
                    .label(format!("PID {ppid}"))
                    .css_classes(["numeric", "dim-label"])
                    .build(),
            );
            content.append(&gtk::Image::from_icon_name("go-next-symbolic"));
            let parent = gtk::ListBoxRow::builder()
                .activatable(true)
                .child(&content)
                .build();
            parent.set_tooltip_text(Some("Select the process that started it"));
            about.list.append(&parent);
            let weak = Rc::downgrade(self);
            about.list.connect_row_activated(move |_, row| {
                if row == &parent
                    && let Some(inspector) = weak.upgrade()
                    && let Some(select) = inspector.on_select.borrow().as_ref()
                {
                    select(ppid);
                }
            });
        }
        self.sections.append(&about.root);
        *self.rows.borrow_mut() = rows;

        // History, all at once; then a reading at a time.
        let history =
            |f: fn(&entry::Point) -> f64| data.history.iter().map(f).collect::<Vec<f64>>();
        self.cpu.set(vec![history(|p| p.cpu)]);
        self.memory.set(vec![history(|p| p.memory)]);
    }

    fn update(&self, data: &Data, reading: bool) {
        match entry::gicon(&data.icon) {
            Some(icon) => self.icon.set_from_gicon(&icon),
            None => self
                .icon
                .set_icon_name(Some(if data.group && data.count > 1 {
                    "application-x-executable-symbolic"
                } else {
                    "system-run-symbolic"
                })),
        }
        self.title.set_label(&data.name);
        // A window titled as its application says nothing more.
        let detail = if data.detail == data.name {
            ""
        } else {
            data.detail.as_str()
        };
        self.detail.set_label(detail);
        self.detail.set_visible(!detail.is_empty());
        let mut meta = Vec::new();
        if let Some(pid) = data.pid {
            meta.push(format!("PID {pid}"));
        } else if data.count > 1 {
            meta.push(model::count(data.count, "process", "processes"));
        }
        if !data.user.is_empty() {
            meta.push(data.user.clone());
        }
        if let Some(process) = data
            .process
            .as_ref()
            .filter(|_| !data.group || data.count == 1)
        {
            meta.push(process.state_text().to_owned());
        }
        self.meta.set_label(&meta.join(" · "));
        for row in self.rows.borrow().iter() {
            let text = (row.get)(data);
            if row.long {
                row.row.set_visible(text.is_some());
            }
            row.value.set_label(text.as_deref().unwrap_or("—"));
        }

        if reading && let Some(point) = data.history.back() {
            self.cpu.push(&[point.cpu]);
            self.memory.push(&[point.memory]);
        }
        self.cpu
            .set_label(&format!("{}%", model::percent(data.usage.cpu)));
        self.memory.set_label(&model::bytes(data.usage.memory()));
        let u = &data.usage;
        let whole = (u.resident + u.swap).max(1) as f64;
        let private = u.private.unwrap_or(u.anon) as f64;
        let shared = u.shared.unwrap_or(u.resident.saturating_sub(u.anon)) as f64;
        self.breakdown
            .set(&[private / whole, shared / whole, u.swap as f64 / whole]);
        self.breakdown_label.set_label(&format!(
            "{} private · {} shared · {} in swap",
            model::bytes(private as u64),
            model::bytes(shared as u64),
            model::bytes(u.swap)
        ));
    }
}
