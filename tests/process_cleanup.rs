#!/usr/bin/env -S rust-script --force
//! Verify timeout/error cleanup without starting a VM or touching a disk.
//!
//! ```cargo
//! [dependencies]
//! anyhow = "=1.0.100"
//! libc = "=0.2.177"
//! respin-tools = { path = "../rust" }
//! ```
use anyhow::{Result, ensure};
use respin_tools::support::Process;
use std::{
    process::Command,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--help") {
        println!(
            "Usage: process_cleanup.rs (tests ownership and timeout cleanup of disposable child processes)"
        );
        return Ok(());
    }
    let start = Instant::now();
    let pid;
    {
        let mut child = Process(Command::new("sleep").arg("60").spawn()?);
        pid = child.0.id();
        ensure!(
            child.wait(Duration::from_millis(50)).is_err(),
            "Expected timeout"
        );
        // Drop must terminate and reap only this owned child after the error.
    }
    ensure!(
        start.elapsed() < Duration::from_secs(5),
        "Cleanup did not promptly terminate the child"
    );
    ensure!(
        unsafe { libc::kill(pid as i32, 0) } == -1,
        "Child survived cleanup"
    );
    println!("PASS: timed-out child was terminated and reaped");
    let mut completed = Process(Command::new("true").spawn()?);
    ensure!(
        completed.wait(Duration::from_secs(2))?.success(),
        "Successful child failed"
    );
    drop(completed);
    println!("PASS: already-exited child is handled cleanly");
    Ok(())
}
