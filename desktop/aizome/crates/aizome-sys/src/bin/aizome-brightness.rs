//! `aizome-brightness`: external monitors' brightness as Aizome sees it.
//!
//! ```text
//! aizome-brightness [list]               what can be set, and why not
//! aizome-brightness set OUTPUT PERCENT   set the monitor on OUTPUT (DP-1)
//! ```
//!
//! `list` only reads: sysfs, then one DDC/CI Get VCP Feature (luminance)
//! per external monitor and one HID feature report per Studio Display.
//! `set` writes the level to the monitor, as Settings' slider does, and
//! remembers a Studio Display's. The laptop's own panel isn't here: it's
//! the backlight's (`/sys/class/backlight`).

use std::process::ExitCode;
use std::time::{Duration, Instant};

use aizome_sys::Wake;
use aizome_sys::monitors::{Control, Monitors, MonitorsState};

const USAGE: &str = "usage: aizome-brightness [list]\n       aizome-brightness set OUTPUT PERCENT";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["list"] => list(),
        ["set", output, percent] => set(output, percent),
        ["-h" | "--help" | "help"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The monitors, once the first look has finished.
fn look() -> Option<(Monitors, std::sync::Arc<MonitorsState>)> {
    let monitors = Monitors::new(Wake::none());
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let s = monitors.snapshot();
        if s.ready {
            return Some((monitors, s));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    eprintln!("aizome-brightness: the monitors didn't answer in 30 seconds");
    None
}

fn through(c: &Control) -> String {
    match c {
        Control::Ddc { bus } => format!("DDC/CI on /dev/i2c-{bus}"),
        Control::StudioDisplay { node, product } => format!("USB HID on /dev/{node} (05ac:{product:04x})"),
        Control::Sample => "a sample".into(),
    }
}

fn list() -> ExitCode {
    let Some((_m, s)) = look() else { return ExitCode::FAILURE };
    for m in &s.monitors {
        println!(
            "{}\t{}\t{:.0}%\t{} (level {} of {}–{})",
            m.connectors.join(","),
            m.title(),
            m.fraction() * 100.0,
            through(&m.control),
            m.level,
            m.min,
            m.max
        );
    }
    for u in &s.unavailable {
        println!("{}\t{}\t-\t{}", u.connectors.join(","), u.title(), u.why.explain());
    }
    if s.monitors.is_empty() && s.unavailable.is_empty() {
        println!("No external monitors are connected.");
    }
    ExitCode::SUCCESS
}

fn set(output: &str, percent: &str) -> ExitCode {
    let Some(percent) = percent.trim_end_matches('%').parse::<f32>().ok().filter(|p| (0.0..=100.0).contains(p)) else {
        eprintln!("aizome-brightness: PERCENT is 0 to 100");
        return ExitCode::from(2);
    };
    let Some((m, s)) = look() else { return ExitCode::FAILURE };
    let Some(monitor) = s.on(output) else {
        match s.unavailable_on(output) {
            Some(u) => eprintln!("aizome-brightness: {}: {}", u.title(), u.why.explain()),
            None => eprintln!("aizome-brightness: no external monitor on {output}"),
        }
        return ExitCode::FAILURE;
    };
    let result = m.set_fraction(&monitor.id, percent / 100.0).wait().and_then(|()| m.flush().wait());
    match result {
        Ok(()) => {
            let level = m.snapshot().get(&monitor.id).map_or(0, |x| x.level);
            println!("{}: {percent:.0}% (level {level})", monitor.title());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("aizome-brightness: {}: {e}", monitor.title());
            ExitCode::FAILURE
        }
    }
}
