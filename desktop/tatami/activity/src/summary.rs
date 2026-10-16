// SPDX-License-Identifier: MIT OR Apache-2.0
//! The summary under the list, as Activity Monitor's: for each view, the
//! system as a whole. CPU: user and system time, each core, load, speed
//! and temperature. Memory: what holds it, and how hard pressed it is.
//! Energy: each meter, and how much of it the processes account for. GPU:
//! each graphics processor. Disk: reading and writing.
use crate::{
    graph::{Bars, Graph, Meter},
    model::{self, Frame, Lens, Meter as Source, Strain},
};
use gtk::prelude::*;
use std::cell::RefCell;

/// Pairs of names and values, in columns of `per_column`.
struct Stats {
    grid: gtk::Grid,
    count: i32,
    per_column: i32,
}

impl Stats {
    fn new(per_column: i32) -> Stats {
        let grid = gtk::Grid::builder()
            .row_spacing(4)
            .column_spacing(10)
            .valign(gtk::Align::Center)
            .css_classes(["summary-stats"])
            .build();
        Stats {
            grid,
            count: 0,
            per_column,
        }
    }

    /// A named value; the label it returns is updated with each reading.
    fn row(&mut self, name: &str) -> (gtk::Label, gtk::Label) {
        let (column, row) = (self.count / self.per_column, self.count % self.per_column);
        let key = gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        let value = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["numeric", "summary-value"])
            .build();
        self.grid.attach(&key, column * 2, row, 1, 1);
        self.grid.attach(&value, column * 2 + 1, row, 1, 1);
        if column > 0 {
            key.set_margin_start(12);
        }
        self.count += 1;
        (key, value)
    }

    fn value(&mut self, name: &str) -> gtk::Label {
        self.row(name).1
    }
}

fn pane() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build()
}

fn graph_block(title: &str, graph: &Graph) -> gtk::Box {
    let block = gtk::Box::new(gtk::Orientation::Vertical, 6);
    block.set_hexpand(true);
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["heading", "summary-title"])
        .build();
    block.append(&label);
    block.append(graph.widget());
    block
}

struct Cpu {
    graph: Graph,
    bars: Bars,
    user: gtk::Label,
    system: gtk::Label,
    idle: gtk::Label,
    load: gtk::Label,
    processes: gtk::Label,
    threads: gtk::Label,
    speed: gtk::Label,
    temperature: gtk::Label,
}

struct Memory {
    graph: Graph,
    pressure_graph: Graph,
    used: gtk::Label,
    apps: gtk::Label,
    cached: gtk::Label,
    kernel: gtk::Label,
    swap: gtk::Label,
    compressed: (gtk::Label, gtk::Label),
    pressure: gtk::Label,
}

struct Energy {
    graph: Graph,
    stats: RefCell<Stats>,
    cpu: gtk::Label,
    gpus: RefCell<Vec<gtk::Label>>,
    battery: gtk::Label,
    total: gtk::Label,
    attributed: gtk::Label,
    idle: gtk::Label,
    used: gtk::Label,
    note: gtk::Label,
}

struct GpuCard {
    graph: Graph,
    meter: Meter,
    memory: gtk::Label,
    line: gtk::Label,
}

struct Gpus {
    cards: gtk::Box,
    list: RefCell<Vec<GpuCard>>,
    none: gtk::Label,
}

struct Disk {
    graph: Graph,
    reading: gtk::Label,
    writing: gtk::Label,
    read: gtk::Label,
    written: gtk::Label,
    disks: gtk::Label,
}

pub struct Summary {
    pub stack: gtk::Stack,
    cpu: Cpu,
    memory: Memory,
    energy: Energy,
    gpus: Gpus,
    disk: Disk,
}

impl Summary {
    pub fn new() -> Summary {
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .vhomogeneous(false)
            .css_classes(["summary"])
            .build();

        // CPU.
        let graph = Graph::new(&["cpu-system", "cpu-user"], Some(100.0), true, 84);
        let bars = Bars::new(84);
        let mut stats = Stats::new(4);
        let user = stats.value("User");
        let system = stats.value("System");
        let idle = stats.value("Idle");
        let load = stats.value("Load");
        let processes = stats.value("Processes");
        let threads = stats.value("Threads");
        let speed = stats.value("Speed");
        let temperature = stats.value("Temperature");
        let row = pane();
        row.append(&graph_block("Processor load", &graph));
        let cores = gtk::Box::new(gtk::Orientation::Vertical, 6);
        cores.append(
            &gtk::Label::builder()
                .label("Cores")
                .xalign(0.0)
                .css_classes(["heading", "summary-title"])
                .build(),
        );
        cores.append(bars.widget());
        row.append(&cores);
        row.append(&stats.grid);
        stack.add_named(&row, Some(Lens::Cpu.id()));
        let cpu = Cpu {
            graph,
            bars,
            user,
            system,
            idle,
            load,
            processes,
            threads,
            speed,
            temperature,
        };

        // Memory.
        let graph = Graph::new(&["memory-apps", "memory-cached"], Some(100.0), true, 84);
        let pressure_graph = Graph::new(&["pressure-low"], None, false, 84);
        pressure_graph.set_least(10.0);
        let mut stats = Stats::new(4);
        let used = stats.value("Used");
        let apps = stats.value("Applications");
        let cached = stats.value("Cached files");
        let kernel = stats.value("Kernel");
        let swap = stats.value("Swap used");
        let pressure = stats.value("Pressure");
        // Last, as it shows only with zram.
        let compressed = stats.row("Compressed");
        let row = pane();
        row.append(&graph_block("Memory", &graph));
        let block = graph_block("Memory pressure", &pressure_graph);
        block.set_hexpand(false);
        block.set_size_request(200, -1);
        row.append(&block);
        row.append(&stats.grid);
        stack.add_named(&row, Some(Lens::Memory.id()));
        let memory = Memory {
            graph,
            pressure_graph,
            used,
            apps,
            cached,
            kernel,
            swap,
            compressed,
            pressure,
        };

        // Energy.
        let graph = Graph::new(&["power-cpu", "power-gpu"], None, true, 84);
        graph.set_least(10.0);
        let mut stats = Stats::new(4);
        let cpu_power = stats.value("Processor");
        let battery = stats.value("Battery");
        let total = stats.value("Measured");
        let attributed = stats.value("Applications");
        let idle = stats.value("Idle and other");
        let used = stats.value("Since opened");
        let note = gtk::Label::builder()
            .wrap(true)
            .xalign(0.0)
            .max_width_chars(36)
            .css_classes(["dim-label", "caption"])
            .visible(false)
            .build();
        let side = gtk::Box::new(gtk::Orientation::Vertical, 6);
        side.set_valign(gtk::Align::Center);
        side.append(&stats.grid);
        side.append(&note);
        let row = pane();
        row.append(&graph_block("Power", &graph));
        row.append(&side);
        stack.add_named(&row, Some(Lens::Energy.id()));
        let energy = Energy {
            graph,
            stats: RefCell::new(stats),
            cpu: cpu_power,
            gpus: RefCell::new(Vec::new()),
            battery,
            total,
            attributed,
            idle,
            used,
            note,
        };

        // GPU.
        let cards = pane();
        cards.set_homogeneous(true);
        let none = gtk::Label::builder()
            .label("No graphics processor reports its use.")
            .css_classes(["dim-label"])
            .hexpand(true)
            .visible(false)
            .build();
        cards.append(&none);
        stack.add_named(&cards, Some(Lens::Gpu.id()));
        let gpus = Gpus {
            cards,
            list: RefCell::new(Vec::new()),
            none,
        };

        // Disk.
        let graph = Graph::new(&["disk-read", "disk-write"], None, false, 84);
        graph.set_least(1024.0 * 1024.0);
        let mut stats = Stats::new(4);
        let reading = stats.value("Reading");
        let writing = stats.value("Writing");
        let read = stats.value("Read since opened");
        let written = stats.value("Written since opened");
        let disks = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        let side = gtk::Box::new(gtk::Orientation::Vertical, 6);
        side.set_valign(gtk::Align::Center);
        side.append(&stats.grid);
        side.append(&disks);
        let row = pane();
        row.append(&graph_block("Storage", &graph));
        row.append(&side);
        stack.add_named(&row, Some(Lens::Disk.id()));
        let disk = Disk {
            graph,
            reading,
            writing,
            read,
            written,
            disks,
        };

        Summary {
            stack,
            cpu,
            memory,
            energy,
            gpus,
            disk,
        }
    }

    pub fn set_lens(&self, lens: Lens) {
        self.stack.set_visible_child_name(lens.id());
    }

    pub fn update(&self, frame: &Frame) {
        let system = &frame.system;

        let cpu = &self.cpu;
        cpu.graph.push(&[system.kernel, system.user]);
        cpu.graph
            .set_label(&format!("{}%", model::percent(system.cpu)));
        cpu.bars.set(&system.cores);
        cpu.user
            .set_label(&format!("{}%", model::percent(system.user)));
        cpu.system
            .set_label(&format!("{}%", model::percent(system.kernel)));
        cpu.idle.set_label(&format!(
            "{}%",
            model::percent((100.0 - system.cpu).max(0.0))
        ));
        cpu.load.set_label(&format!(
            "{:.2}  {:.2}  {:.2}",
            system.load[0], system.load[1], system.load[2]
        ));
        cpu.processes
            .set_label(&model::grouped(system.processes as u64));
        cpu.threads.set_label(&model::grouped(system.threads));
        let speed = if system.frequencies.is_empty() {
            "—".to_owned()
        } else {
            let average = system.frequencies.iter().sum::<f64>() / system.frequencies.len() as f64;
            model::frequency(average)
        };
        cpu.speed.set_label(&speed);
        cpu.temperature.set_label(
            &system
                .temperature
                .map(model::temperature)
                .unwrap_or("—".into()),
        );

        let memory = &self.memory;
        let m = &system.memory;
        let total = m.total.max(1) as f64;
        let apps = m.used().saturating_sub(m.cached.min(m.used())) as f64;
        memory.graph.push(&[
            m.apps.min(m.used()) as f64 / total * 100.0,
            (m.used() as f64 - m.apps.min(m.used()) as f64).max(0.0) / total * 100.0,
        ]);
        let _ = apps;
        memory.graph.set_label(&format!(
            "{} of {}",
            model::bytes(m.used()),
            model::bytes(m.total)
        ));
        memory.used.set_label(&model::bytes(m.used()));
        memory.apps.set_label(&model::bytes(m.apps));
        memory.cached.set_label(&model::bytes(m.cached));
        memory.kernel.set_label(&model::bytes(m.kernel));
        memory.swap.set_label(&format!(
            "{} of {}",
            model::bytes(m.swap_used()),
            model::bytes(m.swap_total)
        ));
        match m.compressed {
            Some((stored, takes)) => {
                memory.compressed.1.set_label(&format!(
                    "{} in {}",
                    model::bytes(stored),
                    model::bytes(takes)
                ));
                memory.compressed.0.set_visible(true);
                memory.compressed.1.set_visible(true);
            }
            None => {
                memory.compressed.0.set_visible(false);
                memory.compressed.1.set_visible(false);
            }
        }
        let pressure = &system.pressure;
        let (word, class) = match pressure.memory_strain() {
            Strain::Low => ("Low", "pressure-low"),
            Strain::Moderate => ("Moderate", "pressure-moderate"),
            Strain::High => ("High", "pressure-high"),
        };
        memory.pressure.set_label(word);
        for other in ["pressure-low", "pressure-moderate", "pressure-high"] {
            memory.pressure.remove_css_class(other);
        }
        memory.pressure.add_css_class(class);
        memory.pressure_graph.push(&[pressure.memory_some]);
        memory
            .pressure_graph
            .set_label(&format!("{} · {:.1}% stalled", word, pressure.memory_some));

        let energy = &self.energy;
        let power = &frame.power;
        let gpu_watts: f64 = frame
            .gpus
            .iter()
            .zip(&power.gpus)
            .filter(|(gpu, _)| !gpu.integrated)
            .filter_map(|(_, watts)| *watts)
            .sum();
        let package = power
            .total
            .map(|total| (total - gpu_watts).max(0.0))
            .unwrap_or(0.0);
        energy.graph.push(&[package, gpu_watts]);
        energy.graph.set_label(
            &power
                .total
                .map(model::watts)
                .unwrap_or_else(|| "Not measured".into()),
        );
        energy
            .cpu
            .set_label(&power.cpu.map(model::watts).unwrap_or_else(|| "—".into()));
        {
            let mut labels = energy.gpus.borrow_mut();
            if labels.len() != frame.gpus.len() {
                let mut stats = energy.stats.borrow_mut();
                for gpu in frame.gpus.iter().skip(labels.len()) {
                    labels.push(stats.value(&short_name(&gpu.name)));
                }
            }
            for ((label, gpu), watts) in labels.iter().zip(&frame.gpus).zip(&power.gpus) {
                let text = watts.map(model::watts).unwrap_or_else(|| "—".into());
                label.set_label(&if gpu.integrated {
                    format!("{text} (in processor)")
                } else {
                    text
                });
            }
        }
        energy
            .battery
            .set_label(&match (power.battery, power.on_battery) {
                (Some(watts), _) => model::watts(watts),
                (None, true) => "Discharging".into(),
                (None, false) => "—".into(),
            });
        energy
            .total
            .set_label(&power.total.map(model::watts).unwrap_or_else(|| "—".into()));
        energy.attributed.set_label(&model::watts(power.attributed));
        energy.idle.set_label(
            &power
                .total
                .map(|total| model::watts((total - power.attributed).max(0.0)))
                .unwrap_or_else(|| "—".into()),
        );
        energy.used.set_label(&if power.measured > 0.0 {
            model::energy(power.energy)
        } else {
            "—".into()
        });
        let note = match power.meter {
            Source::None if self::rapl_present() => {
                "The processor's meter is read by Tatami's power meter service, which isn't running."
            }
            Source::None => "This processor has no power meter the kernel can read.",
            _ => "",
        };
        energy.note.set_label(note);
        energy.note.set_visible(!note.is_empty());

        // GPUs: a card each.
        let gpus = &self.gpus;
        {
            let mut list = gpus.list.borrow_mut();
            if list.len() != frame.gpus.len() {
                while let Some(child) = gpus.cards.first_child() {
                    gpus.cards.remove(&child);
                }
                gpus.cards.append(&gpus.none);
                list.clear();
                for gpu in &frame.gpus {
                    let graph = Graph::new(&["gpu-busy", "gpu-video"], Some(100.0), false, 64);
                    let meter = Meter::new(&["gpu-memory"]);
                    let memory = gtk::Label::builder()
                        .xalign(0.0)
                        .css_classes(["caption", "numeric"])
                        .build();
                    let line = gtk::Label::builder()
                        .xalign(0.0)
                        .css_classes(["caption", "dim-label", "numeric"])
                        .build();
                    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
                    card.add_css_class("gpu-card");
                    card.append(&graph_block(&gpu.name, &graph));
                    card.append(meter.widget());
                    card.append(&memory);
                    card.append(&line);
                    gpus.cards.append(&card);
                    list.push(GpuCard {
                        graph,
                        meter,
                        memory,
                        line,
                    });
                }
            }
            gpus.none.set_visible(list.is_empty());
            for (card, gpu) in list.iter().zip(&frame.gpus) {
                let busy = gpu.busy.unwrap_or(0.0);
                card.graph.push(&[busy, gpu.video.unwrap_or(0.0)]);
                let video = gpu
                    .video
                    .filter(|video| *video >= 0.5)
                    .map(|video| format!(" · video {}%", model::percent(video)))
                    .unwrap_or_default();
                card.graph
                    .set_label(&format!("{}%{video}", model::percent(busy)));
                match (gpu.vram_used, gpu.vram_total) {
                    (Some(used), Some(total)) if total > 0 => {
                        card.meter.set(&[used as f64 / total as f64]);
                        let shared = if gpu.integrated {
                            " (from system memory)"
                        } else {
                            ""
                        };
                        card.memory.set_label(&format!(
                            "{} of {} video memory{shared}",
                            model::bytes(used),
                            model::bytes(total)
                        ));
                    }
                    _ => {
                        card.meter.set(&[0.0]);
                        card.memory.set_label("Video memory not reported");
                    }
                }
                let parts: Vec<String> = [
                    gpu.power.map(model::watts),
                    gpu.temperature.map(model::temperature),
                    gpu.clock.map(model::frequency),
                    Some(gpu.driver.clone()),
                ]
                .into_iter()
                .flatten()
                .collect();
                card.line.set_label(&parts.join(" · "));
            }
        }

        let disk = &self.disk;
        let reading: f64 = system.disks.iter().map(|d| d.read_rate).sum();
        let writing: f64 = system.disks.iter().map(|d| d.write_rate).sum();
        disk.graph.push(&[reading, writing]);
        disk.graph.set_label(&format!(
            "{} read · {} written",
            model::rate(reading),
            model::rate(writing)
        ));
        disk.reading.set_label(&model::rate(reading));
        disk.writing.set_label(&model::rate(writing));
        disk.read.set_label(&model::bytes(system.read_total));
        disk.written.set_label(&model::bytes(system.write_total));
        let disks: Vec<String> = system
            .disks
            .iter()
            .map(|d| format!("{} {:.0}% busy", d.name, d.busy))
            .collect();
        disk.disks.set_label(&disks.join("  ·  "));
    }
}

/// "NVIDIA GeForce RTX 5090" as "RTX 5090", for a short label.
pub fn short_name(name: &str) -> String {
    let name = name
        .trim_start_matches("NVIDIA ")
        .trim_start_matches("GeForce ")
        .trim_start_matches("AMD ")
        .trim_start_matches("Intel(R) ");
    name.to_owned()
}

fn rapl_present() -> bool {
    std::path::Path::new("/sys/class/powercap/intel-rapl:0").exists()
}
