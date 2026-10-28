//! `aizome-power-meter`: the processor package's energy counters for
//! Myaku, which runs as you. The kernel lets only root read RAPL's counters
//! (`/sys/class/powercap/intel-rapl:N/energy_uj`, AMD's processors' too):
//! sampled quickly, they can leak what the processor computes
//! (CVE-2020-8694). Root runs this as `aizome-power-meter.service`, started
//! by its socket (`/run/aizome/power.sock`) when Myaku asks and gone after a
//! minute without a question. It reads the counters at most twice a second
//! and answers every question in between with that reading: far too coarse
//! for the attack, enough for watts. Each answer is a line per package,
//! "NAME ENERGY_UJ RANGE_UJ", as Tatami's `tatami power-meter` gives them;
//! `aizome_sys::sysinfo::power::meter_counters` reads it.
//!
//! It takes no arguments and opens nothing but the counters: the socket
//! comes from systemd.

use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// A fresh reading at most this often. Myaku asks every second or less
/// often, so each of its questions gets a fresh one.
const EVERY: Duration = Duration::from_millis(500);
/// Without a question for this long, the service ends; its socket stays.
const IDLE_MS: i32 = 60_000;

/// The package domains' counters under `root` (`/sys/class/powercap`): a
/// line for each `intel-rapl:N` (not their sub-domains) whose counter can
/// be read.
fn counters(root: &Path) -> String {
    let mut domains: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("intel-rapl:") && name.matches(':').count() == 1
        })
        .map(|e| e.path())
        .collect();
    domains.sort();
    let read = |path: &Path| std::fs::read_to_string(path).ok().map(|t| t.trim().to_owned());
    domains
        .iter()
        .filter_map(|d| {
            let energy: u64 = read(&d.join("energy_uj"))?.parse().ok()?;
            let range: u64 = read(&d.join("max_energy_range_uj")).and_then(|t| t.parse().ok()).unwrap_or(0);
            // One word: "package 1" would split into two fields.
            let name = read(&d.join("name"))
                .unwrap_or_else(|| "package".into())
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-");
            Some(format!("{name} {energy} {range}\n"))
        })
        .collect()
}

/// The last reading, the answer to every question until the next may be
/// taken.
#[derive(Default)]
struct Cache(Option<(Instant, String)>);

impl Cache {
    fn reading(&mut self, now: Instant, read: impl FnOnce() -> String) -> String {
        match &self.0 {
            Some((at, text)) if now.saturating_duration_since(*at) < EVERY => text.clone(),
            _ => {
                let text = read();
                self.0 = Some((now, text.clone()));
                text
            }
        }
    }
}

fn main() -> ExitCode {
    // Socket activation: systemd passes the listening socket as fd 3.
    let number = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u32>().ok());
    if number("LISTEN_PID") != Some(std::process::id()) || number("LISTEN_FDS").unwrap_or(0) < 1 {
        eprintln!("aizome-power-meter: started by aizome-power-meter.socket");
        return ExitCode::FAILURE;
    }
    // SAFETY: systemd passed fd 3, a listening Unix socket, to this process.
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    let mut cache = Cache::default();
    loop {
        let mut wait = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd.
        match unsafe { libc::poll(&mut wait, 1, IDLE_MS) } {
            0 => return ExitCode::SUCCESS,
            n if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted => continue,
            n if n < 0 => return ExitCode::FAILURE,
            _ => {}
        }
        let Ok((mut stream, _)) = listener.accept() else { continue };
        let reading = cache.reading(Instant::now(), || counters(Path::new("/sys/class/powercap")));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = stream.write_all(reading.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_domains_only() {
        let root = std::env::temp_dir().join(format!("aizome-power-meter-{}", std::process::id()));
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
            std::fs::write(dir.join("max_energy_range_uj"), "65532610987\n").unwrap();
        }
        // A domain whose counter can't be read is left out.
        std::fs::create_dir_all(root.join("intel-rapl:2")).unwrap();
        assert_eq!(counters(&root), "package-0 123456 65532610987\npackage-1 42 65532610987\n");
        assert_eq!(counters(&root.join("absent")), "", "a VM has no RAPL: the answer is empty");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn readings_are_taken_at_most_twice_a_second() {
        let start = Instant::now();
        let mut cache = Cache::default();
        let mut reads = 0;
        let mut ask = |after: u64| {
            cache.reading(start + Duration::from_millis(after), || {
                reads += 1;
                format!("package-0 {reads} 0\n")
            })
        };
        assert_eq!(ask(0), "package-0 1 0\n");
        assert_eq!(ask(10), "package-0 1 0\n", "a burst of questions gets the same reading");
        assert_eq!(ask(499), "package-0 1 0\n");
        assert_eq!(ask(500), "package-0 2 0\n");
        assert_eq!(ask(1500), "package-0 3 0\n", "Myaku's every second is always fresh");
    }
}
