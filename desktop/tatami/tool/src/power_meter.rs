// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tatami power-meter`: the processor's energy counters for the Activity
//! window. The kernel lets only root read RAPL's counters
//! (/sys/class/powercap/intel-rapl:N/energy_uj, AMD's processors' too),
//! since sampled quickly they can leak what the processor computes
//! (CVE-2020-8694). This service, started by its socket when the window
//! asks and gone after a minute without a question, reads them at most once
//! a second and answers every question in between with that reading: too
//! coarse for the attack, enough for watts. Each answer is one line per
//! package, "NAME ENERGY_UJ RANGE_UJ".
use std::{
    io::Write,
    os::unix::{io::FromRawFd, net::UnixListener},
    path::Path,
    process::ExitCode,
    time::{Duration, Instant},
};

/// A fresh reading at most this often.
const EVERY: Duration = Duration::from_secs(1);
/// Without a question for this long, the service ends; its socket stays.
const IDLE_MS: i32 = 60_000;

/// The package domains' counters under `root` (/sys/class/powercap).
pub fn counters(root: &Path) -> String {
    let mut domains: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("intel-rapl:") && name.matches(':').count() == 1
        })
        .map(|entry| entry.path())
        .collect();
    domains.sort();
    let read = |path: &Path| {
        std::fs::read_to_string(path)
            .ok()
            .map(|text| text.trim().to_owned())
    };
    domains
        .iter()
        .filter_map(|domain| {
            let energy: u64 = read(&domain.join("energy_uj"))?.parse().ok()?;
            let range: u64 = read(&domain.join("max_energy_range_uj"))
                .and_then(|text| text.parse().ok())
                .unwrap_or(0);
            let name = read(&domain.join("name"))
                .unwrap_or_else(|| "package".into())
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-");
            Some(format!("{name} {energy} {range}\n"))
        })
        .collect()
}

/// Serve the socket systemd passes (fd 3, socket activation).
pub fn serve() -> ExitCode {
    // SAFETY: getpid has no preconditions.
    let pid = unsafe { libc::getpid() };
    let passed = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|p| p.parse::<i32>().ok())
        == Some(pid)
        && std::env::var("LISTEN_FDS")
            .ok()
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0)
            >= 1;
    if !passed {
        eprintln!("tatami: power-meter is started by tatami-power-meter.socket");
        return ExitCode::FAILURE;
    }
    // SAFETY: systemd passed fd 3, a listening Unix socket, to this process.
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    let mut last: Option<(Instant, String)> = None;
    loop {
        let mut wait = libc::pollfd {
            fd: 3,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut wait, 1, IDLE_MS) };
        if ready == 0 {
            return ExitCode::SUCCESS;
        }
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return ExitCode::FAILURE;
        }
        let Ok((mut stream, _)) = listener.accept() else {
            continue;
        };
        let reading = match &last {
            Some((at, text)) if at.elapsed() < EVERY => text.clone(),
            _ => {
                let text = counters(Path::new("/sys/class/powercap"));
                last = Some((Instant::now(), text.clone()));
                text
            }
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = stream.write_all(reading.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_domains_only() {
        let root = std::env::temp_dir().join(format!("tatami-power-meter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (domain, name, energy) in [
            ("intel-rapl:0", "package-0", "123456"),
            ("intel-rapl:0:0", "core", "999"),
            ("intel-rapl:1", "package 1", "42"),
        ] {
            let dir = root.join(domain);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("name"), format!("{name}\n")).unwrap();
            std::fs::write(dir.join("energy_uj"), format!("{energy}\n")).unwrap();
            std::fs::write(dir.join("max_energy_range_uj"), "262143328850\n").unwrap();
        }
        // A domain whose counter can't be read is left out.
        std::fs::create_dir_all(root.join("intel-rapl:2")).unwrap();
        assert_eq!(
            counters(&root),
            "package-0 123456 262143328850\npackage-1 42 262143328850\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
