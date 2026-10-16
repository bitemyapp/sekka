// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tatami-activity`: the Activity window of the Tatami session, after
//! macOS's Activity Monitor. It shows what each application and process
//! uses of the processor, memory (proportional, private, resident, shared
//! and swapped), energy, graphics (NVIDIA through NVML, AMD and Intel
//! through the kernel's DRM statistics) and storage, and quits, force quits
//! or pauses them. It opens on the application whose window had the focus;
//! `tatami-activity --pid PID` opens it on a process, `--lens memory` on a
//! view and `--search TEXT` searching, in the open window too; `--print`
//! prints one reading instead.
mod apps;
mod entry;
mod gpu;
mod graph;
mod inspector;
mod model;
mod nvml;
mod power;
mod procfs;
mod sampler;
mod summary;
mod table;
mod theme;
mod window;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::time::Duration;

/// The window's own look; Tatami's stylesheet (theme.rs) goes over it.
const CSS: &str = "
.graph {
  background-color: alpha(currentColor, 0.04);
  border-radius: 8px;
}
.graph-label {
  margin: 5px 8px;
  font-weight: bold;
}
.graph-series.cpu-user, .graph-series.memory-apps, .graph-series.power-cpu,
.core-bars, .meter-part.memory-private {
  color: var(--accent-color);
}
.graph-series.cpu-system, .graph-series.power-gpu, .graph-series.gpu-busy,
.graph-series.disk-write, .meter-part.memory-swap {
  color: #c061cb;
}
.graph-series.memory-cached {
  color: alpha(#2190a4, 0.55);
}
.graph-series.gpu-video, .graph-series.disk-read, .meter-part.gpu-memory,
.meter-part.memory-shared {
  color: #2190a4;
}
.graph-series.pressure-low, .summary-value.pressure-low {
  color: #26a269;
}
.graph-series.pressure-moderate, .summary-value.pressure-moderate {
  color: #e5a50a;
}
.graph-series.pressure-high, .summary-value.pressure-high {
  color: #e01b24;
}
.meter-track {
  border-radius: 4px;
}
.summary-stats .summary-value {
  font-weight: 600;
}
.gpu-card {
  min-width: 220px;
}
columnview.processes > listview > row > cell {
  padding-top: 3px;
  padding-bottom: 3px;
}
.row-detail {
  font-size: 0.9em;
}
.inspector-row {
  padding: 6px 12px;
}
.paused {
  font-style: italic;
  opacity: 0.6;
}
";

/// `tatami-activity --print`: one reading as text, the busiest first.
fn print() {
    let (sender, receiver) = async_channel::bounded(1);
    let _commands = sampler::spawn(sender, Duration::from_millis(1500));
    let _first = receiver.recv_blocking();
    let Ok(frame) = receiver.recv_blocking() else {
        return;
    };
    let system = &frame.system;
    println!(
        "{}: {}% ({}% user, {}% system), {} processes, {} threads",
        system.cpu_name,
        model::percent(system.cpu),
        model::percent(system.user),
        model::percent(system.kernel),
        system.processes,
        system.threads
    );
    println!(
        "Memory: {} of {} used, pressure {:?}",
        model::bytes(system.memory.used()),
        model::bytes(system.memory.total),
        system.pressure.memory_strain()
    );
    let dash = || "—".to_owned();
    for gpu in &frame.gpus {
        println!(
            "{} ({}, {}): {}% busy, {} of {} video memory, {}",
            gpu.name,
            gpu.driver,
            if gpu.integrated {
                "integrated"
            } else {
                "discrete"
            },
            gpu.busy.map(model::percent).unwrap_or_else(dash),
            gpu.vram_used.map(model::bytes).unwrap_or_else(dash),
            gpu.vram_total.map(model::bytes).unwrap_or_else(dash),
            gpu.power
                .map(model::watts)
                .unwrap_or_else(|| "no meter".into()),
        );
    }
    let power = &frame.power;
    println!(
        "Power: processor {} ({:?}), measured {}, battery {}",
        power.cpu.map(model::watts).unwrap_or_else(dash),
        power.meter,
        power.total.map(model::watts).unwrap_or_else(dash),
        power.battery.map(model::watts).unwrap_or_else(dash),
    );
    let mut groups = frame.groups.clone();
    groups.sort_by(|a, b| b.usage.cpu.total_cmp(&a.usage.cpu));
    println!(
        "\n{:<26} {:>7} {:>10} {:>10} {:>9} {:>6} {:>10}  {:<8} Detail",
        "Application", "% CPU", "Memory", "Private", "Power", "% GPU", "VRAM", "Kind"
    );
    for group in groups.iter().take(25) {
        let usage = &group.usage;
        println!(
            "{:<26} {:>7} {:>10} {:>10} {:>9} {:>6} {:>10}  {:<8} {}",
            group.name.chars().take(26).collect::<String>(),
            model::percent(usage.cpu),
            model::bytes(usage.memory()),
            usage.private.map(model::bytes).unwrap_or_else(dash),
            usage.power.map(model::watts).unwrap_or_else(dash),
            model::percent(usage.gpu),
            model::bytes(usage.vram),
            format!("{:?}", group.kind.unwrap_or(model::Kind::Service)),
            model::count(group.members.len(), "process", "processes"),
        );
    }
}

fn main() -> glib::ExitCode {
    if std::env::args().any(|arg| arg == "--print") {
        print();
        return glib::ExitCode::SUCCESS;
    }
    let app = adw::Application::builder()
        .application_id("org.tatami.Activity")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_startup(|_| {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.load_from_string(CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        // The desktop's look (Tatami's), over the window's own style.
        if let Some(theme) = theme::from_environment() {
            let provider = gtk::CssProvider::new();
            provider.load_from_path(&theme.path);
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
            if theme.dark {
                adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
            }
        }
    });
    // A second launch brings the open window forward, on the view it names.
    app.connect_command_line(|app, line| {
        let args: Vec<String> = line
            .arguments()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let value = |flag: &str| {
            args.windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1].clone())
        };
        window::open(
            app,
            window::Request {
                lens: value("--lens").and_then(|id| model::Lens::from_id(&id)),
                pid: value("--pid").and_then(|pid| pid.parse().ok()),
                search: value("--search"),
            },
        );
        glib::ExitCode::SUCCESS
    });
    app.run()
}
