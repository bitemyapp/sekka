//! `aizome-power-meter`: the processor package's energy counters for
//! Myaku, which runs as you. The kernel lets only root read RAPL's counters
//! (`/sys/class/powercap/intel-rapl:N/energy_uj`, AMD's processors' too):
//! sampled quickly, they can leak what the processor computes
//! (CVE-2020-8694). Root runs this as `aizome-power-meter.service`, started
//! by its socket (`/run/aizome/power.sock`) when Myaku asks and gone after a
//! minute without a question. It reads the counters at most once a second,
//! as Tatami's `tatami power-meter` does, and answers every question in
//! between with that reading: far too coarse for the attack, enough for
//! watts. Each answer is a line per package, "NAME ENERGY_UJ RANGE_UJ
//! READ_US": Tatami's three fields, then when they were read, in
//! microseconds of `CLOCK_BOOTTIME`, so that Myaku measures the time
//! between two readings, not between its questions.
//! `aizome_sys::sysinfo::power::meter_counters` reads it.
//!
//! It takes no arguments and opens nothing but the counters: the socket
//! comes from systemd.

use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// A fresh reading at most this often, in microseconds: once a second, as
/// Tatami's meter. When Myaku asks again sooner, it gets the same reading
/// and keeps its last rate.
const EVERY_US: u64 = 1_000_000;
/// Without a question for this long, the service ends; its socket stays.
const IDLE_MS: i32 = 60_000;

/// Now, in microseconds of `CLOCK_BOOTTIME`: it counts on while the
/// computer sleeps, as RAPL's counters do.
fn boottime_us() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a valid timespec to write into.
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    ts.tv_sec as u64 * 1_000_000 + ts.tv_nsec as u64 / 1_000
}

/// The package domains' counters under `root` (`/sys/class/powercap`), read
/// `at`: a line for each `intel-rapl:N` named `package…` (not their
/// sub-domains) whose counter can be read. A laptop's platform domain
/// (`psys`, beside `package-0` on Intel's) measures the whole computer, the
/// package included: added to the package, it would count the processor
/// two or three times over.
fn counters(root: &Path, at: u64) -> String {
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
            // One word: "package 1" would split into two fields.
            let name = read(&d.join("name"))?.split_whitespace().collect::<Vec<_>>().join("-");
            if !name.starts_with("package") {
                return None;
            }
            let energy: u64 = read(&d.join("energy_uj"))?.parse().ok()?;
            let range: u64 = read(&d.join("max_energy_range_uj")).and_then(|t| t.parse().ok()).unwrap_or(0);
            Some(format!("{name} {energy} {range} {at}\n"))
        })
        .collect()
}

/// The last reading, the answer to every question until the next may be
/// taken.
#[derive(Default)]
struct Cache(Option<(u64, String)>);

impl Cache {
    /// The answer `now` (microseconds of `CLOCK_BOOTTIME`): `read` takes a
    /// fresh reading at that time.
    fn reading(&mut self, now: u64, read: impl FnOnce(u64) -> String) -> String {
        match &self.0 {
            Some((at, text)) if now.saturating_sub(*at) < EVERY_US => text.clone(),
            _ => {
                let text = read(now);
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
        let reading = cache.reading(boottime_us(), |at| counters(Path::new("/sys/class/powercap"), at));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = stream.write_all(reading.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `/sys/class/powercap` of `domains` (directory, name, energy) in a
    /// fresh folder.
    fn powercap(test: &str, domains: &[(&str, &str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("aizome-power-meter-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (domain, name, energy) in domains {
            let dir = root.join(domain);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("name"), format!("{name}\n")).unwrap();
            std::fs::write(dir.join("energy_uj"), format!("{energy}\n")).unwrap();
            std::fs::write(dir.join("max_energy_range_uj"), "65532610987\n").unwrap();
        }
        root
    }

    #[test]
    fn package_domains_only() {
        let root = powercap(
            "packages",
            &[
                ("intel-rapl:0", "package-0", "123456"),
                ("intel-rapl:0:0", "core", "999"),
                ("intel-rapl:1", "package 1", "42"),
            ],
        );
        // A domain whose counter can't be read is left out.
        std::fs::create_dir_all(root.join("intel-rapl:2")).unwrap();
        assert_eq!(
            counters(&root, 7_000_000),
            "package-0 123456 65532610987 7000000\npackage-1 42 65532610987 7000000\n",
            "each line says when it was read"
        );
        assert_eq!(counters(&root.join("absent"), 7_000_000), "", "a VM has no RAPL: the answer is empty");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn platform_domain_is_not_the_processor() {
        // The X1 Carbon Gen 14's domains (the counters made up):
        // intel-rapl:0 is package-0, with core and dram beneath it;
        // intel-rapl:1 is psys, the whole computer's; and the MMIO interface
        // offers package-0 and its dram again.
        let root = powercap(
            "psys",
            &[
                ("intel-rapl:0", "package-0", "100"),
                ("intel-rapl:0:0", "core", "60"),
                ("intel-rapl:0:1", "dram", "10"),
                ("intel-rapl:1", "psys", "200"),
                ("intel-rapl-mmio:0", "package-0", "100"),
                ("intel-rapl-mmio:0:0", "dram", "10"),
            ],
        );
        // And the control types, which have no counter of their own.
        std::fs::create_dir_all(root.join("intel-rapl")).unwrap();
        std::fs::create_dir_all(root.join("intel-rapl-mmio")).unwrap();
        let got = counters(&root, 1);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(got, "package-0 100 65532610987 1\n");
    }

    #[test]
    fn readings_are_taken_at_most_once_a_second() {
        let mut cache = Cache::default();
        let mut reads = 0;
        let mut ask = |after_ms: u64| {
            cache.reading(5_000_000 + after_ms * 1000, |at| {
                reads += 1;
                format!("package-0 {reads} 0 {at}\n")
            })
        };
        assert_eq!(ask(0), "package-0 1 0 5000000\n");
        assert_eq!(ask(10), "package-0 1 0 5000000\n", "a burst of questions gets the same reading");
        assert_eq!(ask(999), "package-0 1 0 5000000\n", "and when it was read");
        assert_eq!(ask(1000), "package-0 2 0 6000000\n");
        assert_eq!(ask(1500), "package-0 2 0 6000000\n", "Myaku asking again a moment early");
        assert_eq!(ask(2100), "package-0 3 0 7100000\n");
        // Asleep for an hour: CLOCK_BOOTTIME went on, so the first question
        // after waking gets a fresh reading.
        assert_eq!(ask(3_602_100), "package-0 4 0 3607100000\n");
    }
}
