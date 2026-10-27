//! The inspector: everything about the selected application or process,
//! in sections, with Quit, Force Quit and Pause.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::app::{Msg, Myaku, RowRef, Sel};
use crate::model::{self, Frame, Kind, Meter, Usage};

pub const WIDTH: f64 = 320.0;

fn pair(name: &str, value: String) -> Element<Msg> {
    row![
        text(name.to_owned()).size(12.0).role(Role::Text3).width(112.0),
        text(value).size(12.0).tabular().selectable().width(Length::Fill)
    ]
    .spacing(8.0)
    .align(Align::Start)
    .width(Length::Fill)
    .into()
}

fn section(title: &str, rows: Vec<Element<Msg>>) -> Element<Msg> {
    let mut v = vec![text(title.to_owned()).mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3).into()];
    v.extend(rows);
    aizome_kit::widgets::layout::column(v).spacing(4.0).width(Length::Fill).into()
}

fn grouping(group: &model::Group, member: bool) -> Option<Element<Msg>> {
    let mut rows = Vec::new();
    if member {
        rows.push(
            row![
                text(if group.kind == Some(Kind::App) { "Application" } else { "Group" })
                    .size(12.0)
                    .role(Role::Text3)
                    .width(112.0),
                button(group.name.clone())
                    .plain()
                    .compact()
                    .on_press(Msg::Select(Sel::Group(group.key)))
                    .id("inspector-group"),
            ]
            .spacing(8.0)
            .into(),
        );
    }
    if !group.attribution.trim().is_empty() {
        rows.push(
            text(group.attribution.clone())
                .size(12.0)
                .wrap()
                .selectable()
                .width(Length::Fill)
                .id("group-attribution")
                .into(),
        );
    }
    (!rows.is_empty()).then(|| section("Grouping", rows))
}

fn opt<T>(v: Option<T>, f: impl Fn(T) -> String) -> String {
    v.map(f).unwrap_or_else(|| "—".into())
}

fn energy_note(f: &Frame) -> &'static str {
    let cards = f.power.graphics.is_some();
    match f.power.meter {
        Meter::None if f.power.battery.is_none() && !f.power.on_battery => {
            "Not measured: the processor's power meter (RAPL) is readable only by root here, and the battery isn't discharging."
        }
        Meter::None => "Not measured: no power meter is readable here.",
        Meter::Battery if cards => {
            "Estimated from the battery's draw: the graphics cards' part by share of each card, the rest by share of the processor."
        }
        Meter::Battery => "Estimated from the battery's draw, by share of the processor.",
        Meter::Rapl | Meter::Package if cards => {
            "Estimated from the power meters of the processor, by share of it, and of the graphics cards, by share of each card."
        }
        Meter::Rapl | Meter::Package => "Estimated from the processor's power meter, by share of the processor.",
        Meter::Graphics => {
            "Estimated from the graphics cards' power meters, by share of each card. The processor's (RAPL) is readable only by root here: what uses no card isn't measured."
        }
    }
}

/// What `members` use of the graphics cards. With several cards, its use of
/// each it uses, whichever the table shows: "GeForce RTX 5090 · 2.0% · 876
/// MiB".
fn graphics(f: &Frame, members: &[usize]) -> Vec<Element<Msg>> {
    let procs = || members.iter().map(|&i| &f.processes[i]);
    let all = model::graphics_of(procs(), &f.gpus, None);
    let percent = |n: Option<f64>| opt(n, |n| format!("{}%", model::percent(n)));
    let mut rows = Vec::new();
    if f.gpus.len() > 1 {
        let names = model::gpu_names(&f.gpus);
        for d in model::gpu_order(&f.gpus) {
            // Shown where it's a client, or might be: the card's clients
            // can't be read.
            if procs().any(|p| p.uses(d)) || f.gpus[d].clients_unknown() {
                let on = model::graphics_of(procs(), &f.gpus, Some(d));
                rows.push(pair(&names[d], format!("{} · {}", percent(on.busy), opt(on.vram, model::bytes))));
            }
        }
        if rows.is_empty() {
            return vec![text("Uses no GPU.").size(12.0).role(Role::Text3).into()];
        }
    } else {
        rows.push(pair("% GPU", percent(all.busy)));
    }
    rows.push(pair("% Video", percent(all.video)));
    if f.gpus.len() == 1 {
        rows.push(pair("Video memory", opt(all.vram, model::bytes)));
    }
    if f.gpus.iter().any(|g| g.reports_shared()) {
        rows.push(pair("Shared GPU", opt(all.shared, model::bytes)));
    }
    rows
}

fn usage_sections(m: &Myaku, members: &[usize], u: &Usage, readable: bool) -> Vec<Element<Msg>> {
    let f = &m.frame;
    let mut v = vec![section(
        "Processor",
        vec![
            pair("% CPU", format!("{}%", model::percent(u.cpu))),
            pair("CPU time", model::cpu_time(u.cpu_time)),
            pair("Threads", model::grouped(u.threads)),
            pair("Wake-ups", format!("{} a second", model::grouped(u.wakeups.round() as u64))),
        ],
    )];
    let mut mem = vec![
        pair("Memory", model::bytes(u.memory())),
        pair("Private", opt(u.private, model::bytes)),
        pair("Shared", opt(u.shared, model::bytes)),
        pair("Resident", model::bytes(u.resident)),
        pair("Swap", model::bytes(u.swap)),
        pair("Virtual", model::bytes(u.virt)),
    ];
    if !readable {
        mem.push(
            text("Another user's memory maps can't be read: Memory is its resident size.")
                .size(11.0)
                .role(Role::Text3)
                .wrap()
                .width(Length::Fill)
                .into(),
        );
    }
    v.push(section("Memory", mem));
    let mut energy = Vec::new();
    if f.power.meter != Meter::None {
        energy.push(pair("Power", opt(u.power, model::watts)));
        energy.push(pair("Average", opt(u.average_power(), model::watts)));
        energy.push(pair("Energy used", model::energy(u.energy)));
    }
    energy.push(text(energy_note(f)).size(11.0).role(Role::Text3).wrap().width(Length::Fill).into());
    v.push(section("Energy", energy));
    if !f.gpus.is_empty() {
        v.push(section("Graphics", graphics(f, members)));
    }
    let mut disk = vec![
        pair("Reading", opt(u.read_rate, model::rate)),
        pair("Writing", opt(u.write_rate, model::rate)),
        pair("Read", opt(u.read_total, model::bytes)),
        pair("Written", opt(u.write_total, model::bytes)),
    ];
    if !readable {
        disk.push(
            text("Another user's storage counters can't be read.")
                .size(11.0)
                .role(Role::Text3)
                .wrap()
                .width(Length::Fill)
                .into(),
        );
    }
    v.push(section("Disk", disk));
    v
}

pub fn view(m: &Myaku) -> Element<Msg> {
    let f = &m.frame;
    let body: Vec<Element<Msg>> = match m.selected() {
        None => vec![
            text("Nothing selected").size(15.0).semibold().into(),
            text("Select an application or a process to see what it uses.")
                .role(Role::Text3)
                .wrap()
                .width(Length::Fill)
                .into(),
        ],
        Some(RowRef::Group(g)) => {
            let grp = &f.groups[g];
            let mut v = header(m, &grp.name, &group_detail(f, g), grp.kind);
            v.extend(grouping(grp, false));
            v.extend(usage_sections(m, &grp.members, &grp.usage, grp.members.iter().all(|&i| f.processes[i].readable)));
            let mut list: Vec<Element<Msg>> = Vec::new();
            for &i in grp.members.iter().take(40) {
                let p = &f.processes[i];
                let label = if p.role.is_empty() {
                    format!("{} · {}", p.name, p.pid)
                } else {
                    format!("{} ({}) · {}", p.name, p.role, p.pid)
                };
                list.push(button(label).plain().compact().on_press(Msg::Select(Sel::Process(p.key()))).into());
            }
            if grp.members.len() > 40 {
                list.push(text(format!("and {} more", grp.members.len() - 40)).size(11.0).role(Role::Text3).into());
            }
            v.push(section(&model::count(grp.members.len(), "Process", "Processes"), list));
            v
        }
        Some(RowRef::Process(i, _)) => {
            let p = &f.processes[i];
            let meta = format!("PID {} · {} · {}", p.pid, p.user, p.state_text());
            let mut v = header(m, &p.name, &meta, f.groups[p.group].kind);
            v.extend(grouping(&f.groups[p.group], true));
            v.extend(usage_sections(m, &[i], &p.usage, p.readable));
            let mut about = Vec::new();
            if !p.role.is_empty() {
                about.push(pair("Role", p.role.clone()));
            }
            if let Some(parent) = f.processes.iter().find(|q| q.pid == p.ppid) {
                about.push(
                    row![
                        text("Parent").size(12.0).role(Role::Text3).width(112.0),
                        button(format!("{} · {}", parent.name, parent.pid))
                            .plain()
                            .compact()
                            .on_press(Msg::Select(Sel::Process(parent.key()))),
                    ]
                    .spacing(8.0)
                    .into(),
                );
            } else if p.ppid > 0 {
                about.push(pair("Parent", format!("PID {}", p.ppid)));
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            about.push(pair("Started", model::ago(now.saturating_sub(p.started))));
            about.push(pair("Priority", if p.nice == 0 { "normal".into() } else { format!("nice {}", p.nice) }));
            if !p.command.is_empty() {
                about.push(text("Command").size(12.0).role(Role::Text3).into());
                about.push(text(p.command.clone()).size(11.5).mono().wrap().selectable().width(Length::Fill).into());
            }
            if !p.exe.is_empty() {
                about.push(pair("Program", p.exe.clone()));
            }
            if !p.unit.is_empty() {
                about.push(pair("Unit", p.unit.clone()));
            }
            v.push(section("About", about));
            v
        }
    };
    container(
        scroll(aizome_kit::widgets::layout::column(body).spacing(16.0).padding([14.0, 16.0]).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .fill(Tone::Surface)
    .width(WIDTH)
    .height(Length::Fill)
    .id("inspector")
    .into()
}

fn group_detail(f: &Frame, g: usize) -> String {
    let grp = &f.groups[g];
    let mut parts = vec![grp.kind.map_or("", |k| k.words()).to_owned()];
    if grp.windows > 0 {
        parts.push(model::count(grp.windows, "window", "windows"));
    }
    if !grp.detail.is_empty() && grp.windows <= 1 {
        parts.push(grp.detail.clone());
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

fn header(m: &Myaku, name: &str, meta: &str, kind: Option<Kind>) -> Vec<Element<Msg>> {
    let can = m.can_quit();
    let paused = m
        .sel
        .map(|s| m.targets(s).0)
        .is_some_and(|ms| !ms.is_empty() && ms.iter().all(|&i| m.frame.processes[i].paused()));
    let (glyph, role) = match kind {
        Some(Kind::App) => (Icon::AppWindow, Role::Accent),
        Some(Kind::Service) => (Icon::Settings2, Role::Text2),
        Some(Kind::System) => (Icon::Server, Role::Text3),
        _ => (Icon::Cpu, Role::Text3),
    };
    let mut v: Vec<Element<Msg>> = vec![
        row![
            icon(glyph).size(22.0).role(role),
            column![
                text(name.to_owned()).size(15.0).semibold(),
                text(meta.to_owned()).size(11.5).role(Role::Text3).wrap().width(Length::Fill)
            ]
            .spacing(2.0)
            .width(Length::Fill)
        ]
        .spacing(10.0)
        .align(Align::Start)
        .width(Length::Fill)
        .into(),
        row![
            button("Quit").compact().on_press_maybe(can.is_ok().then_some(Msg::AskQuit(false))),
            button("Force Quit").compact().destructive().on_press_maybe(can.is_ok().then_some(Msg::AskQuit(true))),
            button(if paused { "Resume" } else { "Pause" })
                .compact()
                .on_press_maybe(can.is_ok().then_some(Msg::Pause(!paused))),
        ]
        .spacing(6.0)
        .into(),
    ];
    if let Err(e) = can {
        v.push(text(e).size(11.0).role(Role::Text3).wrap().width(Length::Fill).into());
    }
    v
}
