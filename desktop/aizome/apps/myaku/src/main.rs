//! `aizome-myaku [--pid PID] [--lens cpu|memory|energy|gpu|disk] [--search TEXT]`:
//! the Activity window, or in the open one, the view asked for. `--print`
//! prints one reading instead.

use myaku::app::{Myaku, Request};
use myaku::{instance, model};

fn print() {
    let mut s = myaku::sampler::Sampler::new();
    let _ = s.sample();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let f = s.sample();
    let sys = &f.system;
    println!(
        "{}: {}% ({}% user, {}% system), {}, {}; read in {:?}",
        if sys.cpu_name.is_empty() { "Processor" } else { &sys.cpu_name },
        model::percent(sys.cpu),
        model::percent(sys.user),
        model::percent(sys.kernel),
        model::count(sys.processes, "process", "processes"),
        model::count(sys.threads as usize, "thread", "threads"),
        f.cost
    );
    println!("Memory: {} of {} used", model::bytes(sys.memory.used()), model::bytes(sys.memory.total));
    for g in &f.gpus {
        println!(
            "{} ({}, {}): {}% busy; video memory {} / {}",
            g.name,
            g.driver,
            g.pci,
            g.busy.map(model::percent).unwrap_or_else(|| "—".into()),
            g.vram_used.map(model::bytes).unwrap_or_else(|| "—".into()),
            g.vram_total.map(model::bytes).unwrap_or_else(|| "—".into())
        );
    }
    let graphics = f.power.graphics.map(|w| format!("; graphics {}", model::watts(w))).unwrap_or_default();
    println!(
        "Power: {}{graphics}",
        match f.power.meter {
            model::Meter::Rapl => format!("processor {}", f.power.cpu.map(model::watts).unwrap_or_default()),
            model::Meter::Package => format!(
                "processor {} (its integrated graphics' meter)",
                f.power.cpu.map(model::watts).unwrap_or_default()
            ),
            model::Meter::Battery => format!("battery {}", f.power.battery.map(model::watts).unwrap_or_default()),
            model::Meter::Graphics => "graphics cards only (RAPL is readable only by root)".into(),
            model::Meter::None => "not measured (RAPL is readable only by root; the battery isn't discharging)".into(),
        }
    );
    let mut groups = f.groups.clone();
    groups.sort_by(|a, b| b.usage.cpu.total_cmp(&a.usage.cpu));
    println!(
        "\n{:<28} {:>7} {:>10} {:>10} {:>12} {:>11}  Kind",
        "Application", "% CPU", "Memory", "% GPU", "Video Memory", "Reading"
    );
    for g in groups.iter().take(25) {
        let u = &g.usage;
        println!(
            "{:<28} {:>7} {:>10} {:>10} {:>12} {:>11}  {}",
            g.name.chars().take(28).collect::<String>(),
            model::percent(u.cpu),
            model::bytes(u.memory()),
            u.graphics.busy.map(model::percent).unwrap_or_else(|| "—".into()),
            u.graphics.vram.map(model::bytes).unwrap_or_else(|| "—".into()),
            u.read_rate.map(model::rate).unwrap_or_else(|| "—".into()),
            g.kind.map_or("", |k| k.words())
        );
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: aizome-myaku [--pid PID] [--lens cpu|memory|energy|gpu|disk] [--search TEXT] [--print]\n\n\
             Activity: what each application and process uses, graphs over time, and quitting what misbehaves."
        );
        return;
    }
    if args.iter().any(|a| a == "--print") {
        print();
        return;
    }
    let mut app = Myaku::live(Request::parse(&args));
    match instance::claim(&args) {
        instance::Claim::Handed => return,
        instance::Claim::First(l) => app.listener = Some(l),
        instance::Claim::Alone => {}
    }
    aizome_kit::run(app);
}
