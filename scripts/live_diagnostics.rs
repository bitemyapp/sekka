#!/usr/bin/env -S rust-script --force
//! Read-only graphics/input diagnostics; never reads input events or Wi-Fi profiles.
use std::{
    fs,
    io::{self, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn command(out: &mut impl Write, program: &str, args: &[&str]) -> io::Result<()> {
    writeln!(out, "\n## {program} {}", args.join(" "))?;
    let mut child = match Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return writeln!(out, "Unavailable: {error}"),
    };
    // Drain both pipes concurrently, so a verbose journal cannot deadlock.
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let collect = |mut pipe: Box<dyn io::Read + Send>| {
        let mut bytes = Vec::new();
        let result = io::Read::read_to_end(&mut pipe, &mut bytes);
        (result, bytes)
    };
    let output = thread::spawn(move || collect(Box::new(stdout)));
    let errors = thread::spawn(move || collect(Box::new(stderr)));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            writeln!(out, "Command timed out")?;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    for worker in [output, errors] {
        let (result, bytes) = worker
            .join()
            .map_err(|_| io::Error::other("capture worker failed"))?;
        result?;
        out.write_all(&bytes)?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Live graphics/input report. May contain hardware identifiers and kernel logs.\nReview before sharing. No upload, raw input-event capture or network-profile reads."
    )?;
    for file in [
        "/proc/version",
        "/proc/cmdline",
        "/proc/bus/input/devices",
        "/etc/calamares-nixos/settings.json",
    ] {
        writeln!(out, "\n## {file}")?;
        match fs::read_to_string(file) {
            Ok(text) => writeln!(out, "{text}")?,
            Err(e) => writeln!(out, "Unavailable: {e}")?,
        }
    }
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            for name in ["status", "enabled", "modes"] {
                let path = entry.path().join(name);
                if let Ok(text) = fs::read_to_string(&path) {
                    writeln!(out, "\n## {}\n{text}", path.display())?;
                }
            }
        }
    }
    for (program, args) in [
        ("lspci", vec!["-nnk"]),
        ("lsusb", vec!["-t"]),
        ("loginctl", vec!["list-sessions", "--no-pager"]),
        ("loginctl", vec!["seat-status", "seat0", "--no-pager"]),
        ("libinput", vec!["list-devices"]),
        ("systemctl", vec!["--failed", "--no-pager"]),
        (
            "systemctl",
            vec!["status", "display-manager", "--no-pager", "--lines=30"],
        ),
        ("journalctl", vec!["-b", "-k", "--no-pager", "-n", "800"]),
        (
            "journalctl",
            vec![
                "-b",
                "--no-pager",
                "-n",
                "200",
                "-u",
                "display-manager",
                "-u",
                "plasmalogin",
            ],
        ),
    ] {
        command(&mut out, program, &args)?;
    }
    Ok(())
}
