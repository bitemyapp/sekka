//! Read-only attribution probe: `cargo run -p aizome-myaku --example groups
//! -- fusen claude chatgpt`. No window is opened and no process is signalled.
//! Optional arguments match group or member names, case-insensitively.
use myaku::{model, sampler::Sampler};

fn main() {
    let filters: Vec<_> = std::env::args().skip(1).map(|s| s.to_lowercase()).collect();
    let mut sampler = Sampler::new();
    let frame = sampler.sample();
    for group in &frame.groups {
        let matches = |name: &str| filters.iter().any(|q| name.to_lowercase().contains(q));
        if !filters.is_empty()
            && !matches(&group.name)
            && !group.members.iter().any(|&i| matches(&frame.processes[i].name))
        {
            continue;
        }
        println!(
            "{}: {} processes, {} windows, {} memory",
            group.name,
            group.members.len(),
            group.windows,
            model::bytes(group.usage.memory())
        );
        println!("  {}", group.attribution);
        for &i in &group.members {
            let p = &frame.processes[i];
            println!("  PID {:>7}  parent {:>7}  {} {}", p.pid, p.ppid, p.name, p.role);
        }
    }
    eprintln!(
        "{} processes in {} groups; grouping phase {:?}",
        frame.processes.len(),
        frame.groups.len(),
        sampler.phases.iter().find(|(name, _)| *name == "groups").map(|(_, time)| time)
    );
}
