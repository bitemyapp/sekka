//! Local before/after measurement with real interactive shells and Hotaru's
//! native application runtime. No compositor or host shell configuration.
//! See docs/test-results/hotaru-osc7-latency.md at the repository root.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aizome_hotaru::app::{Hotaru, Msg};
use aizome_hotaru::go_to_tab::Event;
use aizome_kit::testing::Harness;
use hotaru_io::PtyCommand;
use serde_json::json;

fn cpu_us() -> f64 {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: clock_gettime initializes this timespec on success.
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, time.as_mut_ptr()) }, 0);
    let time = unsafe { time.assume_init() };
    time.tv_sec as f64 * 1e6 + time.tv_nsec as f64 / 1000.0
}

fn until(h: &mut Harness<Hotaru>, done: impl Fn(&Hotaru) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.send_all(std::iter::empty());
        if done(h.program()) {
            return;
        }
        assert!(Instant::now() < deadline, "shell/runtime did not reach the expected state");
        std::thread::sleep(Duration::from_micros(100));
    }
}

fn ready(app: &Hotaru, marker: &str, directory: Option<&Path>) -> bool {
    app.active_pane().is_some_and(|pane| {
        let text = pane.snapshot.plain_text();
        text.contains(marker)
            && text.trim_end().ends_with("BENCH_READY>")
            && directory.is_none_or(|directory| pane.working_directory.as_deref() == Some(directory))
    })
}

fn displayed(app: &Hotaru) -> bool {
    app.active_pane().is_some_and(|pane| {
        pane.displayed_snapshot().is_some_and(|shown| std::sync::Arc::ptr_eq(shown, &pane.snapshot))
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 5, "cwd_latency SCENARIO SHELL RESOURCES OUTPUT.json");
    let scenario = &args[1];
    assert!(["bash-reports", "bash-no-reports", "fish-reports"].contains(&scenario.as_str()));
    let reports = scenario != "bash-no-reports";
    let root = std::env::temp_dir().join(format!("hotaru-cwd-latency-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let dirs = [root.join("one"), root.join("two")];
    for dir in &dirs {
        std::fs::create_dir(dir).unwrap();
    }
    let mut command = PtyCommand::new(&args[2]);
    command.cwd = Some(root.clone());
    command.env.extend([
        ("HOTARU_RESOURCES_DIR".into(), PathBuf::from(&args[3]).into_os_string()),
        ("HISTFILE".into(), "/dev/null".into()),
        ("fish_history".into(), "".into()),
    ]);
    if scenario == "fish-reports" {
        command.args = [
            "--no-config",
            "--interactive",
            "--init-command",
            "stty -echo; function fish_prompt; printf 'BENCH_READY> '; end; source \"$HOTARU_RESOURCES_DIR/shell-integration/fish/hotaru.fish\"",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
    } else {
        let rc = root.join("bashrc");
        let hook = if reports { "source \"$HOTARU_RESOURCES_DIR/shell-integration/bash/hotaru.bash\"\n" } else { "" };
        std::fs::write(&rc, format!("stty -echo\nPS1='BENCH_READY> '\nPROMPT_COMMAND=\n{hook}")).unwrap();
        command.args = vec!["--noprofile".into(), "--rcfile".into(), rc.into_os_string(), "-i".into()];
    }
    // First-prompt readiness is observed from a real PTY, not a fabricated Msg.
    let start = Instant::now();
    let mut h = Harness::new(Hotaru::new(command));
    h.advance(Duration::ZERO);
    until(&mut h, |app| ready(app, "BENCH_READY>", None));
    let startup_us = start.elapsed().as_secs_f64() * 1e6;
    until(&mut h, displayed);
    let startup_shaped_us = start.elapsed().as_secs_f64() * 1e6;
    let pane = h.program().active_pane().unwrap().id;
    let mut commands_us = Vec::new();
    let mut shaped_us = Vec::new();
    let mut commands_cpu_us = Vec::new();
    // Ten warm-up commands, then sixty measured cd/prompt round trips.
    // A changed cwd exercises the OSC 7 hook on every reporting-shell command.
    for index in 0..70 {
        let directory = &dirs[index % 2];
        let marker = format!("ACK:{index:06}");
        let input = format!("cd -- '{}'; printf 'ACK:%06d\\n' {index}\n", directory.display());
        let cpu = cpu_us();
        let start = Instant::now();
        h.send(Msg::Input(pane, input.into_bytes()));
        until(&mut h, |app| ready(app, &marker, reports.then_some(directory.as_path())));
        let elapsed = start.elapsed().as_secs_f64() * 1e6;
        until(&mut h, displayed);
        if index >= 10 {
            commands_us.push(elapsed);
            shaped_us.push(start.elapsed().as_secs_f64() * 1e6);
            commands_cpu_us.push(cpu_us() - cpu);
        }
    }
    // Separately measure the operation that actually changes: opening the
    // palette and waiting for its on-demand background directory lookup.
    let mut palette_us = Vec::new();
    let mut wrong_folders = 0;
    for _ in 0..10 {
        let start = Instant::now();
        h.command(h.window(), 'p');
        until(&mut h, |app| app.go_to_tab.as_ref().is_some_and(|palette| palette.facts.is_some()));
        palette_us.push(start.elapsed().as_secs_f64() * 1e6);
        let facts = h.program().go_to_tab.as_ref().unwrap().facts.as_ref().unwrap();
        let expected = dirs[1].to_string_lossy();
        if facts.iter().find(|facts| facts.pane == pane).unwrap().folder.as_deref() != Some(expected.as_ref()) {
            wrong_folders += 1;
        }
        h.send(Msg::GoToTab(Event::Close));
    }
    let result = json!({
        "scenario": scenario, "shell": args[2], "startup_us": startup_us,
        "startup_shaped_us": startup_shaped_us, "commands_us": commands_us,
        "commands_shaped_us": shaped_us, "commands_cpu_us": commands_cpu_us, "palette_us": palette_us,
        "wrong_folders": wrong_folders,
    });
    h.send(Msg::Input(pane, b"exit\n".to_vec()));
    until(&mut h, |app| app.panes.is_empty());
    drop(h);
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::write(&args[4], serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
