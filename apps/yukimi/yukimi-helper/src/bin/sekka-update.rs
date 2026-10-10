// SPDX-License-Identifier: MIT OR Apache-2.0
//! Terminal entry point for the same transaction Yukimi uses.
use std::io::{BufRead, Read};
use std::process::{Command, ExitCode, Stdio};

// Ctrl-C goes to the foreground helper too. Let it restore the configuration
// and keep reading its result, instead of killing this reader halfway.
extern "C" fn wait_for_helper(_: libc::c_int) {}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--status"] || args == ["--status", "--json"] {
        let completion = yukimi_system::completion::read();
        if args.len() == 2 {
            println!("{}", serde_json::to_string(&completion).map_err(|e| e.to_string())?);
        } else {
            println!("{}. {}", completion.title(), completion.description());
        }
        return Ok(());
    }
    let mode = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => "auto",
        ["--boot"] => "boot",
        ["--switch"] => "switch",
        ["--print-flake"] => "preview",
        ["--help"] | ["-h"] => {
            println!(
                "Usage: sekka-update [--boot | --switch | --print-flake | --status [--json]]\n\nUpdate Sekka and apply safe changes automatically.\nUse --boot to prepare for restart, or --switch to request live activation. No automatic reboot.\nOlder installer configurations migrate to Sekka automatically.\nIndependent Nixpkgs and application pins stay unchanged; update those in Yukimi.\nUse --print-flake to review the proposed flake without changing anything.\nUse --status [--json] to read pending update completion without changing anything."
            );
            return Ok(());
        }
        _ => return Err("Usage: sekka-update [--boot | --switch | --print-flake | --status [--json]]".into()),
    };
    let setup = yukimi_system::setup::Setup::detect();
    let text = std::fs::read_to_string(setup.dir.join("flake.nix")).map_err(|e| format!("Reading your flake: {e}"))?;
    let prepared = yukimi_config::sekka::prepare(&text)?;
    if mode == "preview" {
        print!("{prepared}");
        return Ok(());
    }
    let helper = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("yukimi-helper");
    // SAFETY: the handler does nothing. Caught handlers reset in the child
    // at exec; the helper installs its own cancellation handler.
    unsafe {
        libc::signal(libc::SIGINT, wait_for_helper as *const () as libc::sighandler_t);
    }
    // SAFETY: geteuid has no preconditions. Never trust a helper from PATH.
    let mut command = if unsafe { libc::geteuid() } == 0 {
        Command::new(&helper)
    } else {
        let mut command = Command::new("/run/wrappers/bin/pkexec");
        command.arg(&helper);
        command
    };
    println!("Updating Sekka. Any restart needed will be reported after preparation finishes.");
    let mut child = command
        .args(["change", "--sekka", "--mode", mode])
        // Keeping this pipe open lets the helper recover if the CLI exits.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Starting the updater: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::BufReader::new(stdout).read_to_string(&mut text).map(|_| text)
    });
    for line in std::io::BufReader::new(child.stderr.take().unwrap()).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(json) = line.strip_prefix("@nix ") {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(json)
                && let Some(message) = value["msg"].as_str().or_else(|| value["text"].as_str())
                && !message.is_empty()
            {
                eprintln!("{message}");
            }
        } else {
            eprintln!("{line}");
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let out = reader.join().map_err(|_| "Reading the update result failed")?.map_err(|e| e.to_string())?;
    if !status.success() {
        let result: serde_json::Value = serde_json::from_str(out.trim()).unwrap_or_default();
        return Err(result["error"].as_str().unwrap_or("Update did not complete; see the messages above").into());
    }
    let mut report: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|e| format!("Reading update result: {e}"))?;
    yukimi_system::completion::finish_update(&mut report);
    let completion: yukimi_system::completion::Completion =
        serde_json::from_value(report["completion"].clone()).map_err(|e| format!("Reading completion status: {e}"))?;
    println!("{}. {}", completion.title(), completion.description());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
