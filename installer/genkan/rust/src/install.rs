// SPDX-License-Identifier: GPL-3.0-or-later
//! Installation in two phases.
//!
//! `prepare` makes no disk writes. While the user reviews the plan it chooses
//! filesystem identities, detects hardware, evaluates and builds the complete
//! installed system in the live store, and warms the page cache with that
//! system's closure. `execute` accepts only a `ConfirmedInstall`; after the
//! confirmation it partitions, formats, copies the prepared system to the
//! target and installs the bootloader, so the time after clicking Install is
//! dominated by writing the target disk.
use crate::{
    ConfirmedInstall, Firmware, InstallPlan, Settings,
    config::{self, Template},
    disk::{self, Layout},
    filesystem::{self, Identities},
    memory, nixlog, precache,
    process::{self, output, output_lines},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha_crypt::{Sha512Params, sha512_simple};
use std::{
    cell::Cell,
    fs,
    io::Write,
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

/// Helper → GUI messages, one JSON object per line on stdout.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// Non-destructive preparation status before confirmation.
    Preparing {
        message: String,
    },
    /// Page-cache warming of the prepared system, in bytes.
    Caching {
        read: u64,
        total: u64,
    },
    /// Preparation finished. The helper now waits for the typed confirmation.
    Prepared {
        summary: Summary,
    },
    /// Destructive installation step 1–6 after confirmation.
    Progress {
        step: u8,
        message: String,
    },
    /// Copying the prepared system to the target, in NAR bytes.
    Copying {
        bytes: u64,
        total: u64,
    },
    /// A notable line of tool activity, for an optional detail view.
    Log {
        line: String,
    },
    /// Duration of a completed stage.
    Timing {
        stage: String,
        seconds: f64,
    },
    Complete,
    /// The GUI withdrew the request before confirming; nothing was written.
    Cancelled,
    Failed {
        message: String,
    },
}

/// What preparation established, shown on the review page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// Store size of the complete installed system.
    pub closure_bytes: u64,
    /// Bytes of that system read into the page cache during preparation.
    pub cached_bytes: u64,
    /// Page-cache warming stopped early to keep memory free.
    pub cache_limited: bool,
    /// Unpacked bytes downloaded into the live store during preparation.
    pub downloaded_bytes: u64,
    /// The system could not be built in RAM; it is built on the target after
    /// formatting instead (slower, but never exceeds live memory).
    pub deferred: bool,
    pub root_bytes: u64,
    pub swap_bytes: u64,
    pub seconds: f64,
}

pub fn event(event: Event) {
    // A closed UI pipe must not panic and interrupt a destructive installation.
    if let Ok(text) = serde_json::to_string(&event) {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{text}");
        let _ = stdout.flush();
    }
}
fn progress(step: u8, message: &str) {
    event(Event::Progress {
        step,
        message: message.into(),
    });
}
fn preparing(message: &str) {
    event(Event::Preparing {
        message: message.into(),
    });
}
/// Stage durations of this process, also saved on the installed system.
static TIMINGS: Mutex<Vec<(String, f64)>> = Mutex::new(Vec::new());
fn timing(stage: &str, seconds: f64) {
    TIMINGS.lock().unwrap().push((stage.into(), seconds));
    event(Event::Timing {
        stage: stage.into(),
        seconds,
    });
}
struct Stage(&'static str, Instant);
impl Stage {
    fn start(name: &'static str) -> Self {
        Self(name, Instant::now())
    }
    fn done(self) {
        timing(self.0, self.1.elapsed().as_secs_f64());
    }
}

pub fn live_guard(settings: &Settings) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "The install helper requires root (use the GUI's authorization dialog)"
    );
    ensure!(
        fs::read_to_string("/etc/os-release")?
            .lines()
            .any(|l| l == "ID=nixos" || l == "ID=\"nixos\""),
        "Only NixOS live installation media are supported"
    );
    ensure!(
        output(
            "findmnt",
            &["--noheadings", "--output", "FSTYPE", "--mountpoint", "/"],
            15
        )?
        .trim()
            == "tmpfs",
        "Refusing to run on an installed host: root is not temporary"
    );
    ensure!(
        output(
            "findmnt",
            &["--noheadings", "--output", "FSTYPE", "--mountpoint", "/iso"],
            15
        )?
        .trim()
            == "iso9660",
        "No read-only live ISO filesystem at /iso"
    );
    if settings.test_diagnostics {
        disk::vm_test_disk()?;
    }
    Ok(())
}

// Never recursively delete this directory: an unmount failure must not turn
// cleanup into deletion of an installed filesystem.
struct Target(PathBuf, Cell<bool>);
impl Target {
    fn new() -> Result<Self> {
        Ok(Self(
            tempfile::Builder::new()
                .prefix("calamares-target-")
                .tempdir_in("/run")?
                .keep(),
            Cell::new(false),
        ))
    }
    fn unmount(&self) -> Result<()> {
        output("umount", &["--recursive", self.0.to_str().unwrap()], 60)?;
        self.1.set(false);
        Ok(())
    }
}
impl Drop for Target {
    fn drop(&mut self) {
        if self.1.get() {
            let _ = output("umount", &["--recursive", self.0.to_str().unwrap()], 60);
        }
        let _ = fs::remove_dir(&self.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Preflight,
    Execute,
}

/// Everything established before confirmation. Only `execute` consumes it.
pub struct Prepared {
    // Held until the installation finishes: one installer at a time.
    _lock: fs::File,
    staging: tempfile::TempDir,
    template: Template,
    ids: Identities,
    layout: Layout,
    configuration: String,
    /// Toplevel derivation and output path of the installed system.
    derivation: String,
    system: String,
    summary: Summary,
}
impl Prepared {
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
}

fn installer_lock() -> Result<fs::File> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open("/run/calamares-nixos.lock")?;
    // A preparation the GUI just withdrew may still be exiting.
    let deadline = Instant::now() + Duration::from_secs(30);
    while unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        ensure!(
            Instant::now() < deadline,
            "Another installation is already running"
        );
        ensure!(!process::cancelled(), "Preparation was cancelled");
        thread::sleep(Duration::from_millis(200));
    }
    Ok(lock)
}

/// Replace a raw `--log-format internal-json` failure with Nix's own error
/// messages and the last build output, when Nix reported them.
fn nix_failure(error: anyhow::Error, tracker: &Mutex<nixlog::Tracker>) -> anyhow::Error {
    match tracker.lock().unwrap().failure_report() {
        Some(report) => anyhow::anyhow!("{report}"),
        None => error,
    }
}

/// Forward Nix's machine-readable log as bounded activity lines.
fn nix_activity(tracker: &Mutex<nixlog::Tracker>) -> impl Fn(&str) + Sync + '_ {
    move |line: &str| {
        if let Some(nixlog::Update::Activity(text)) = tracker.lock().unwrap().line(line) {
            event(Event::Log { line: text });
        }
    }
}

/// Non-destructive preparation. No disk is written; the plan stays reviewable.
pub fn prepare(plan: &InstallPlan) -> Result<Prepared> {
    let started = Instant::now();
    let settings = plan.settings();
    let stage = Stage::start("checks");
    preparing("Checking the live system and the selected disk");
    live_guard(settings)?;
    ensure!(
        !fs::read_to_string("/etc/passwd")?
            .lines()
            .any(|l| l.split(':').next() == Some(plan.username().as_str())),
        "Username is already reserved by the live system"
    );
    ensure!(
        plan.firmware() == Firmware::current(),
        "Firmware changed since review"
    );
    let lock = installer_lock()?;
    disk::revalidate(plan.disk())?;
    preparing("Checking filesystem support before erasing the disk");
    plan.filesystem().preflight(plan.firmware())?;
    if plan.swap() {
        output("mkswap", &["--version"], 15)
            .context("The installation media is missing mkswap; no disk writes were made")?;
    }
    let ram = memory::read()?;
    let swap_bytes = plan.swap().then(|| memory::swap_bytes(ram.total));
    let layout = Layout::new(plan.disk().bytes, plan.firmware(), swap_bytes)?;
    let ids = Identities::generate(plan.firmware(), plan.swap())?;
    let template = Template::load(settings, plan.hostname())?;
    stage.done();

    let stage = Stage::start("configuration");
    preparing("Detecting hardware and writing the pinned Determinate flake");
    // Root-only (0700) staging directory: it receives no secrets, but its
    // configuration must not be replaced by another local user.
    let staging = tempfile::Builder::new()
        .prefix("calamares-prepare-")
        .tempdir_in("/run")?;
    template.write(staging.path())?;
    // Hardware detection does not depend on the target's future filesystems:
    // they are declared explicitly in configuration.nix with chosen identities.
    let hardware = output(
        "nixos-generate-config",
        &["--show-hardware-config", "--no-filesystems"],
        120,
    )?;
    fs::write(staging.path().join("hardware-configuration.nix"), &hardware)?;
    let configuration = config::configuration(plan, &ids)?;
    fs::write(staging.path().join("configuration.nix"), &configuration)?;
    stage.done();

    let stage = Stage::start("evaluation");
    preparing("Evaluating the installed system");
    let attribute = format!(
        "path:{}#nixosConfigurations.{}.config.system.build.toplevel",
        staging.path().display(),
        plan.hostname().as_str()
    );
    // One evaluation answers both what to build and how much it downloads.
    let report = Mutex::new(String::new());
    let dry = output_lines(
        "nix",
        &[
            "build",
            "--dry-run",
            "--json",
            "--no-link",
            "--no-write-lock-file",
            &attribute,
        ],
        1800,
        &|line: &str| {
            let mut report = report.lock().unwrap();
            if report.len() < 1 << 20 {
                report.push_str(line);
                report.push('\n');
            }
        },
    )
    .context(
        "The selected configuration could not be evaluated; the target disk has not been erased",
    )?;
    let (derivation, system) = nixlog::build_result(&dry)?;
    let missing = nixlog::dry_run(&report.into_inner().unwrap())?;
    ensure!(
        fs::read(staging.path().join("flake.lock"))? == template.lock,
        "Nix modified the pinned lock"
    );
    stage.done();

    // Downloads land in the live store, which is RAM. Build there only when
    // the unpacked size leaves the memory reserve free; otherwise build on
    // the target after formatting, where space is not memory.
    let budget = memory::read()?
        .available
        .saturating_sub(memory::reserve(ram.total));
    let deferred = missing.fetch_bytes > budget;
    let mut summary = Summary {
        root_bytes: layout.root_bytes(),
        swap_bytes: layout.swap_bytes().unwrap_or(0),
        deferred,
        ..Summary::default()
    };
    if deferred {
        preparing(&format!(
            "The selection needs {:.1} GiB of downloads, more than fits in memory: the system will be built on the target disk after formatting",
            missing.fetch_bytes as f64 / memory::GIB as f64
        ));
    } else {
        let stage = Stage::start("build");
        preparing(&format!(
            "Building the installed system ({} builds, {:.0} MiB to download)",
            missing.builds,
            missing.fetch_bytes as f64 / (1 << 20) as f64
        ));
        let tracker = Mutex::new(nixlog::Tracker::default());
        output_lines(
            "nix",
            &[
                "build",
                "--log-format",
                "internal-json",
                "--out-link",
                staging.path().join("system").to_str().unwrap(),
                &format!("{derivation}^out"),
            ],
            7200,
            &nix_activity(&tracker),
        )
        .map_err(|error| nix_failure(error, &tracker))
        .context("The installed system could not be built; the target disk has not been erased")?;
        ensure!(
            fs::canonicalize(staging.path().join("system"))? == Path::new(&system),
            "The built system differs from the evaluated system"
        );
        summary.downloaded_bytes = missing.fetch_bytes;
        let closure_info: serde_json::Value = serde_json::from_str(&output(
            "nix",
            &["path-info", "--json", "--closure-size", &system],
            300,
        )?)?;
        summary.closure_bytes = closure_size(&closure_info)?;
        // Reserve room for updates and the installation workspace.
        let required = summary
            .closure_bytes
            .checked_add(8 * memory::GIB)
            .context("Disk requirement overflow")?;
        ensure!(
            layout.root_bytes() >= required,
            "The selected system needs at least {:.1} GiB on the root filesystem; it would have {:.1} GiB. Select fewer applications, disable the swap partition, or choose a larger disk. No disk data has been changed.",
            required as f64 / memory::GIB as f64,
            layout.root_bytes() as f64 / memory::GIB as f64
        );
        stage.done();

        let stage = Stage::start("cache");
        preparing("Caching the installed system in memory");
        let closure: Vec<PathBuf> =
            process::output_full("nix", &["path-info", "--recursive", &system], 300)?
                .lines()
                .filter_map(precache::store_path)
                .collect();
        let outcome = precache::warm(&closure, &process::CANCEL, |read, total| {
            event(Event::Caching { read, total })
        })?;
        ensure!(!process::cancelled(), "Preparation was cancelled");
        summary.cached_bytes = outcome.bytes;
        summary.cache_limited = outcome.limited;
        stage.done();
    }
    summary.seconds = started.elapsed().as_secs_f64();
    Ok(Prepared {
        _lock: lock,
        staging,
        template,
        ids,
        layout,
        configuration,
        derivation,
        system,
        summary,
    })
}

/// Destructive installation of a prepared, confirmed plan.
pub fn execute(confirmed: ConfirmedInstall, prepared: Prepared) -> Result<()> {
    let started = Instant::now();
    // From before the first disk write until the system is complete.
    let awake = crate::power::stay_awake("Installing NixOS");
    let mut request = confirmed.into_plan();
    // The prepared files must be exactly what this confirmed plan generates.
    ensure!(
        config::configuration(&request, &prepared.ids)? == prepared.configuration,
        "The confirmed plan differs from the prepared system"
    );
    let params = Sha512Params::new(100_000)
        .map_err(|e| anyhow::anyhow!("Password hashing parameters: {e:?}"))?;
    let password = request.take_password();
    let hash = Zeroizing::new(
        sha512_simple(&password, &params)
            .map_err(|_| anyhow::anyhow!("Password hashing failed"))?,
    );
    drop(password);
    let firmware = request.firmware();
    let ids = &prepared.ids;
    let layout = &prepared.layout;
    // Lock while editing the partition table. Re-probe its stable identity and
    // all mount/holder state immediately before the first write.
    let mut device = Some(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&request.disk().path)?,
    );
    ensure!(
        unsafe {
            libc::flock(
                device.as_ref().unwrap().as_raw_fd(),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        } == 0,
        "Disk is locked by another installer"
    );
    disk::revalidate(request.disk())?;
    let dev = request.disk().path.as_str();
    let target = Target::new()?;
    let mount = target.0.to_str().context("Target path encoding")?;
    let stage = Stage::start("storage");
    let storage = (|| -> Result<()> {
        progress(
            1,
            "Erasing the selected disk and creating a GPT partition table",
        );
        output("wipefs", &["--all", "--force", dev], 60)?;
        let script = layout.parted_script(firmware, request.filesystem().name());
        let mut args = vec!["--script", dev];
        args.extend(script.iter().map(String::as_str));
        output("parted", &args, 60)?;
        // Keep udev from observing partially written superblocks. Partition nodes
        // come from the kernel/devtmpfs; do not wait for udev while holding its lock.
        output("blockdev", &["--rereadpt", dev], 30)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match disk::verify_layout(request.disk(), layout) {
                Ok(()) => break,
                Err(error) if Instant::now() >= deadline => {
                    return Err(error.context("Kernel partition layout did not become ready"));
                }
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
        disk::revalidate(request.disk())?;
        progress(2, "Formatting, verifying and mounting the new filesystems");
        let boot = disk::partition(dev, 1);
        let root = disk::partition(dev, 2);
        let swap = disk::partition(dev, 3);
        request.filesystem().format(&root, &ids.root)?;
        match &ids.efi {
            Some(serial) => filesystem::format_efi(&boot, serial)?,
            // A BIOS boot partition must not retain an old filesystem signature.
            None => {
                output("wipefs", &["--all", "--force", &boot], 60)?;
            }
        }
        if let Some(uuid) = &ids.swap {
            filesystem::format_swap(&swap, uuid)?;
        }
        // udev postpones events for a locked disk: release the lock before
        // triggering. A fresh explicit change event updates UUID/type
        // information, including events skipped while locked. Never rely on
        // pre-format udev/blkid caches.
        if let Some(device) = device.take() {
            device.sync_all()?;
        }
        let mut changed = vec!["trigger", "--action=change", "--settle", &root, &boot];
        if ids.swap.is_some() {
            changed.push(&swap);
        }
        output("udevadm", &changed, 70)?;
        disk::revalidate(request.disk())?;
        request.filesystem().mount(&root, mount)?;
        target.1.set(true);
        if ids.efi.is_some() {
            fs::create_dir(target.0.join("boot"))?;
            filesystem::mount_efi(&boot, target.0.join("boot").to_str().unwrap())?;
        }
        Ok(())
    })();
    // Also releases the lock if storage preparation failed while holding it.
    drop(device);
    storage.map_err(|error| filesystem::diagnose(error, dev))?;
    stage.done();

    let stage = Stage::start("files");
    progress(3, "Writing the configuration, account and Wi-Fi settings");
    let dir = target.0.join("etc/nixos");
    fs::create_dir_all(&dir)?;
    prepared.template.write(&dir)?;
    fs::write(dir.join("configuration.nix"), &prepared.configuration)?;
    fs::copy(
        prepared.staging.path().join("hardware-configuration.nix"),
        dir.join("hardware-configuration.nix"),
    )?;
    // Do not put secrets inside the flake source: use a sibling under /etc.
    // configuration.nix uses this external absolute runtime path.
    let secret_dir = target.0.join("etc/nixos-secrets");
    fs::create_dir(&secret_dir)?;
    fs::set_permissions(&secret_dir, fs::Permissions::from_mode(0o700))?;
    config::write_secret(&secret_dir.join("user-password.hash"), &hash)?;
    request.wifi().write_to(&target.0)?;
    stage.done();

    let stage = Stage::start("copy");
    let store = format!("local?root={mount}");
    let tracker = Mutex::new(nixlog::Tracker::default());
    let total = prepared.summary.closure_bytes;
    let last = Mutex::new(Instant::now());
    let on_line = |line: &str| {
        let mut tracker = tracker.lock().unwrap();
        match tracker.line(line) {
            Some(nixlog::Update::Activity(text)) => event(Event::Log { line: text }),
            Some(nixlog::Update::Progress) => {
                let mut last = last.lock().unwrap();
                if last.elapsed() >= Duration::from_millis(250) {
                    *last = Instant::now();
                    event(Event::Copying {
                        bytes: tracker.copied_bytes,
                        total,
                    });
                }
            }
            None => {}
        }
    };
    if prepared.summary.deferred {
        progress(
            4,
            "Downloading and building the system on the new disk (this can take a while)",
        );
        output_lines(
            "nix",
            &[
                "build",
                "--store",
                &store,
                "--extra-substituters",
                "auto?trusted=1",
                "--log-format",
                "internal-json",
                "--no-link",
                "--option",
                "build-dir",
                "/nix/var/nix/builds",
                &format!("{}^out", prepared.derivation),
            ],
            7200,
            &on_line,
        )
        .map_err(|error| nix_failure(error, &tracker))
        .context("Building the system on the new disk failed")?;
    } else {
        progress(4, "Copying the prepared system to the new disk");
        // Measured against a direct parallel file copy with database
        // registration: nix copy was faster both in VMs and on the host.
        output_lines(
            "nix",
            &[
                "copy",
                "--no-check-sigs",
                "--log-format",
                "internal-json",
                "--to",
                &store,
                &prepared.system,
            ],
            7200,
            &on_line,
        )
        .map_err(|error| nix_failure(error, &tracker))
        .context("Copying the system to the new disk failed")?;
        event(Event::Copying {
            bytes: total,
            total,
        });
    }
    stage.done();

    let stage = Stage::start("bootloader");
    progress(5, "Installing the bootloader and activating the system");
    // The closure is already in the target store: nixos-install only sets the
    // system profile and installs the bootloader.
    output(
        "nixos-install",
        &[
            "--root",
            mount,
            "--system",
            &prepared.system,
            "--no-root-passwd",
            "--no-channel-copy",
        ],
        1800,
    )?;
    ensure!(
        fs::read(dir.join("flake.lock"))? == prepared.template.lock,
        "Installed lock differs from the installation media"
    );
    stage.done();

    // A record for measuring real hardware: read it after booting the
    // installed system. No secrets; flushing and unmounting follow.
    let record = serde_json::json!({
        "prepared": prepared.summary,
        "stages": TIMINGS
            .lock()
            .unwrap()
            .iter()
            .map(|(stage, seconds)| (stage.clone(), serde_json::json!(seconds)))
            .collect::<serde_json::Map<_, _>>(),
        "seconds_from_confirmation_before_flush": started.elapsed().as_secs_f64(),
        "kept_awake": awake.held(),
    });
    let log_dir = target.0.join("var/log/calamares-nixos");
    fs::create_dir_all(&log_dir)?;
    fs::write(
        log_dir.join("install.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;

    let stage = Stage::start("flush");
    progress(6, "Flushing writes and unmounting the installed system");
    output("sync", &["--file-system", mount], 300)?;
    target.unmount()?;
    stage.done();
    timing("install", started.elapsed().as_secs_f64());
    progress(
        6,
        "Installation complete. Shut down, remove the installer media, then boot the installed disk.",
    );
    event(Event::Complete);
    Ok(())
}

/// One-shot preflight or installation of an already-confirmed request.
pub fn install(confirmed: ConfirmedInstall, mode: InstallMode) -> Result<()> {
    let plan = confirmed.into_plan();
    let prepared = prepare(&plan)?;
    event(Event::Prepared {
        summary: prepared.summary.clone(),
    });
    if mode == InstallMode::Preflight {
        preparing("Preflight passed; no disk writes were made");
        return Ok(());
    }
    let phrase = format!("ERASE {}", plan.disk().path);
    execute(plan.confirm(&phrase)?, prepared)
}

fn closure_size(value: &serde_json::Value) -> Result<u64> {
    let entries: Vec<_> = match value {
        serde_json::Value::Array(entries) => entries.iter().collect(),
        serde_json::Value::Object(entries) => entries.values().collect(),
        _ => anyhow::bail!("Invalid closure information"),
    };
    ensure!(entries.len() == 1, "Expected one system closure");
    entries[0]["closureSize"]
        .as_u64()
        .context("Missing closure size")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closure_size_accepts_nix_json_formats_and_rejects_incomplete_results() {
        assert_eq!(
            closure_size(&serde_json::json!([{ "closureSize": 123 }])).unwrap(),
            123
        );
        assert_eq!(
            closure_size(&serde_json::json!({ "/nix/store/example": { "closureSize": 456 } }))
                .unwrap(),
            456
        );
        for value in [
            serde_json::json!([]),
            serde_json::json!([{}]),
            serde_json::json!([{"closureSize":-1}]),
            serde_json::json!([{}, {}]),
        ] {
            assert!(closure_size(&value).is_err());
        }
    }
    #[test]
    fn cleanup_never_recursively_deletes_contents() {
        let outer = tempfile::tempdir().unwrap();
        let path = outer.path().join("target");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), "important").unwrap();
        drop(Target(path.clone(), Cell::new(false)));
        assert_eq!(fs::read_to_string(path.join("keep")).unwrap(), "important");
    }
    #[test]
    fn events_round_trip_as_tagged_json() {
        let summary = Summary {
            closure_bytes: 5,
            ..Summary::default()
        };
        let text = serde_json::to_string(&Event::Prepared {
            summary: summary.clone(),
        })
        .unwrap();
        assert!(text.starts_with(r#"{"kind":"prepared""#));
        match serde_json::from_str(&text).unwrap() {
            Event::Prepared { summary: parsed } => assert_eq!(parsed, summary),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            serde_json::to_string(&Event::Complete).unwrap(),
            r#"{"kind":"complete"}"#
        );
    }
}
