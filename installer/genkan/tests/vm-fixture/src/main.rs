// SPDX-License-Identifier: GPL-3.0-or-later
//! Separate test crate. Never shipped in the installer package.
//! Fixed public test credentials, only inside serial/size/live-root guarded VMs.
use anyhow::{Context, Result, ensure};
use calamares_nixos::{
    Desktop, Filesystem, Firmware, Hostname, Kernel, RawRequest, Settings, config, disk,
    filesystem::Identities, install::Event, memory, process::output, session::Confirmation,
};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

const PASSWORD: &str = "Qemu-Only-Test-123!";
/// A throwaway public key standing in for one fetched from GitHub.
const TEST_SSH_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF8y4b2kcB5eEPbrb5tU80+lphiIk3o7v09PPFOqRtH3";
// Public synthetic network, no real wireless credentials or host NM access.
const TEST_WIFI: &str = "[connection]\nid=Installer synthetic WiFi\nuuid=135ea3d9-d456-44b1-ae42-1e7081f66666\ntype=wifi\npermissions=user:nixos:;\n[wifi]\nssid=Installer synthetic WiFi\nmode=infrastructure\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=WiFi-Synthetic-Only-123!\npsk-flags=0\n[ipv4]\nmethod=auto\n[ipv6]\nmethod=auto\n";
// The host-side matrix now exercises the same parser as installation. Use real
// tzdata both on development hosts and the rootless Nix builder/live ISO.
fn zoneinfo() -> Result<std::path::PathBuf> {
    for root in ["/usr/share/zoneinfo", "/etc/zoneinfo"] {
        if Path::new(root).join("America/Chicago").is_file() {
            return Ok(root.into());
        }
    }
    fs::read_dir("/nix/store")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().contains("-tzdata-"))
        .map(|entry| entry.path().join("share/zoneinfo"))
        .find(|path| path.join("America/Chicago").is_file())
        .context("No tzdata found for the test fixture")
}

fn helper() -> Result<PathBuf> {
    let package = fs::read_to_string("/run/calamares-package-path")?;
    Ok(if std::env::var_os("CALAMARES_VM_DEV_BACKEND").is_some() {
        Path::new("/workspace/installer/genkan/rust/target/x86_64-unknown-linux-musl/release/calamares-nixos-helper")
            .to_path_buf()
    } else {
        Path::new(package.trim()).join("bin/calamares-nixos-helper")
    })
}
/// Drive the helper exactly as the GUI does: send the reviewed plan, wait for
/// preparation, then send the typed confirmation. Reports the time from the
/// confirmation (the Install click) to completion.
fn invoke_session(mut request: RawRequest) -> Result<()> {
    let phrase = std::mem::take(&mut request.confirmation);
    let started = Instant::now();
    let mut child = Command::new(helper()?)
        .arg("session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut line = serde_json::to_vec(&request)?;
    line.push(b'\n');
    stdin.write_all(&line)?;
    let mut confirmed = None;
    let mut complete = false;
    for text in BufReader::new(child.stdout.take().unwrap()).lines() {
        let text = text?;
        println!("{text}");
        match serde_json::from_str::<Event>(&text) {
            Ok(Event::Prepared { .. }) => {
                println!("PREPARED_SECONDS={:.1}", started.elapsed().as_secs_f64());
                let mut line = serde_json::to_vec(&Confirmation {
                    confirmation: phrase.clone(),
                })?;
                line.push(b'\n');
                stdin.write_all(&line)?;
                confirmed = Some(Instant::now());
            }
            Ok(Event::Complete) => complete = true,
            _ => {}
        }
    }
    ensure!(
        child.wait()?.success() && complete,
        "Helper session did not complete"
    );
    println!(
        "CLICK_TO_COMPLETE_SECONDS={:.1}",
        confirmed
            .context("Preparation never finished")?
            .elapsed()
            .as_secs_f64()
    );
    Ok(())
}
fn invoke(request: RawRequest, preflight: bool) -> Result<()> {
    let mut child = Command::new(helper()?)
        .arg(if preflight { "preflight" } else { "install" })
        .stdin(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request)?)?;
    ensure!(
        child.wait()?.success(),
        "Packaged installation helper failed"
    );
    Ok(())
}
fn pam_authenticate(password: &str) -> Result<bool> {
    // Run su from an ordinary account, not root (which could bypass PAM).
    // Testing a wrong password first proves this is not a passwordless path.
    let mut child = Command::new("/run/current-system/sw/bin/runuser")
        .args([
            "--pty",
            "--user",
            "rusttest",
            "--",
            "/run/wrappers/bin/su",
            "--login",
            "rusttest",
            "--command",
            "id -un",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    use std::{
        io::Read,
        os::fd::AsRawFd,
        time::{Duration, Instant},
    };
    let mut input = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    unsafe {
        libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
    }
    let mut text = Vec::new();
    let mut sent = false;
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let mut buffer = [0; 1024];
        match stdout.read(&mut buffer) {
            Ok(n) => text.extend_from_slice(&buffer[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        };
        if !sent && String::from_utf8_lossy(&text).contains("Password:") {
            writeln!(input, "{password}")?;
            input.flush()?;
            sent = true;
        }
        if let Some(status) = child.try_wait()? {
            return Ok(sent
                && status.success()
                && String::from_utf8_lossy(&text)
                    .lines()
                    .any(|l| l.trim() == "rusttest"));
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("PAM test timed out waiting for an interactive response");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn guard() -> Result<()> {
    ensure!(unsafe { libc::geteuid() } == 0, "VM fixture requires root");
    disk::vm_test_disk()?;
    Ok(())
}
fn selected_filesystem() -> Result<Filesystem> {
    Ok(serde_json::from_value(serde_json::Value::String(
        std::env::var("CALAMARES_TEST_FILESYSTEM").unwrap_or_else(|_| "ext4".into()),
    ))?)
}
fn selected_applications() -> Vec<String> {
    match std::env::var("CALAMARES_TEST_APPLICATIONS").as_deref() {
        Ok("all") => calamares_nixos::applications::catalog()
            .iter()
            .map(|app| app.id.clone())
            .collect(),
        Ok("none") => vec![],
        Ok(ids) => ids
            .split(',')
            .filter(|id| !id.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => calamares_nixos::applications::default_selection(),
    }
}
fn selected_desktops() -> Result<Vec<Desktop>> {
    std::env::var("CALAMARES_TEST_DESKTOPS")
        .unwrap_or_else(|_| "plasma,xfce".into())
        .split(',')
        .filter(|id| !id.is_empty())
        .map(|id| {
            Ok(serde_json::from_value(serde_json::Value::String(
                id.into(),
            ))?)
        })
        .collect()
}
fn flag(name: &str) -> bool {
    !matches!(std::env::var(name).as_deref(), Ok("0" | "false"))
}
fn request() -> Result<RawRequest> {
    Ok(RawRequest {
        disk: disk::vm_test_disk()?,
        firmware: Firmware::current(),
        filesystem: selected_filesystem()?,
        hostname: "rust-test".into(),
        username: "rusttest".into(),
        full_name: "Rust ${literal} Test".into(),
        password: PASSWORD.into(),
        locale: "en_US.UTF-8".into(),
        timezone: "America/Chicago".into(),
        keyboard: "us".into(),
        default_desktop: *selected_desktops()?
            .first()
            .context("Select a test desktop")?,
        desktops: selected_desktops()?,
        applications: selected_applications(),
        copy_wifi: true,
        wifi_profiles: vec![TEST_WIFI.into()],
        allow_unfree: calamares_nixos::DEFAULT_ALLOW_UNFREE,
        swap: flag("CALAMARES_TEST_SWAP"),
        tuning: flag("CALAMARES_TEST_TUNING"),
        graphics: Default::default(),
        github_user: github_field("rust-test"),
        ssh_keys: github_test()
            .then(|| TEST_SSH_KEY.into())
            .into_iter()
            .collect(),
        ssh_server: github_test(),
        git_name: github_field("Rust ${literal} Test"),
        git_email: github_field("rusttest@example.com"),
        confirmation: format!("ERASE {}", disk::vm_test_disk()?.path),
    })
}
/// The GitHub step's keys, SSH server and Git identity (CALAMARES_TEST_GITHUB).
fn github_test() -> bool {
    flag("CALAMARES_TEST_GITHUB")
}
fn github_field(value: &str) -> String {
    if github_test() {
        value.into()
    } else {
        String::new()
    }
}
fn seed_previous_filesystem(device: &str) -> Result<()> {
    let previous =
        std::env::var("CALAMARES_TEST_PREVIOUS_FILESYSTEM").unwrap_or_else(|_| "blank".into());
    if previous == "blank" {
        return Ok(());
    }
    let filesystem: Filesystem = serde_json::from_value(serde_json::Value::String(previous))?;
    let uefi = Firmware::current() == Firmware::Uefi;
    let start = if uefi { "1025MiB" } else { "3MiB" };
    output(
        "parted",
        &[
            "--script", device, "mklabel", "gpt", "mkpart", "old-boot", "1MiB", start, "mkpart",
            "old-root", start, "100%",
        ],
        60,
    )?;
    output("udevadm", &["settle", "--timeout=60"], 70)?;
    let root = disk::partition(device, 2);
    let program = format!("mkfs.{}", filesystem.name());
    output(
        &program,
        &[
            if filesystem == Filesystem::Ext4 {
                "-F"
            } else {
                "-f"
            },
            &root,
        ],
        300,
    )?;
    if uefi {
        // A stale, lexically earlier alias must never win over the new UUID.
        let boot = disk::partition(device, 1);
        output("mkfs.fat", &["-F", "32", "-i", "00000001", &boot], 60)?;
        output("udevadm", &["trigger", "--action=change", &boot], 30)?;
    }
    output("udevadm", &["trigger", "--action=change", &root], 30)?;
    output("udevadm", &["settle", "--timeout=60"], 70)?;
    ensure!(
        output(
            "blkid",
            &["--probe", "--output", "value", "--match-tag", "TYPE", &root],
            30
        )?
        .trim()
            == filesystem.name(),
        "Could not seed previous filesystem"
    );
    println!("SEEDED_PREVIOUS_FILESYSTEM={}", filesystem.name());
    println!(
        "{}",
        output("udevadm", &["info", "--query=property", &root], 15)?
    );
    Ok(())
}

fn main() -> Result<()> {
    let operation = std::env::args().nth(1);
    if operation.as_deref() == Some("validate-template") {
        let args: Vec<_> = std::env::args().skip(2).collect();
        ensure!(args.len() == 2, "Usage: validate-template TEMPLATE LOCK");
        let template = config::Template::parse(
            fs::read_to_string(&args[0])?,
            fs::read(&args[1])?,
            &Hostname::parse("sekka-test")?,
        )?;
        print!("{}", template.flake);
        return Ok(());
    }
    if matches!(
        operation.as_deref(),
        Some("desktop-configurations" | "application-configurations")
    ) {
        // Non-destructive host-side generation for Nix module evaluation.
        // Produces the actual backend output, never a second implementation.
        let settings = Settings {
            template_dir: "/unused".into(),
            zoneinfo: zoneinfo()?.to_string_lossy().into_owned(),
            state_version: "26.11".into(),
            kernel: Kernel::Lts,
            test_diagnostics: false,
        };
        let mut cases = Vec::new();
        if operation.as_deref() == Some("application-configurations") {
            let catalog = calamares_nixos::applications::catalog();
            for app in catalog {
                cases.push((
                    app.id.clone(),
                    vec![Desktop::Plasma],
                    vec![app.id.clone()],
                    app.unfree,
                ));
            }
            cases.push(("none".into(), vec![Desktop::Plasma], vec![], false));
            cases.push((
                "all".into(),
                vec![Desktop::Plasma],
                catalog.iter().map(|app| app.id.clone()).collect(),
                true,
            ));
            cases.push((
                "free-only".into(),
                vec![Desktop::Plasma],
                catalog
                    .iter()
                    .filter(|app| !app.unfree)
                    .map(|app| app.id.clone())
                    .collect(),
                false,
            ));
        } else {
            for bits in 1u16..(1 << Desktop::ALL.len()) {
                let desktops: Vec<_> = Desktop::ALL
                    .iter()
                    .enumerate()
                    .filter_map(|(i, d)| (bits & (1 << i) != 0).then_some(*d))
                    .collect();
                let name = desktops
                    .iter()
                    .map(|d| d.id())
                    .collect::<Vec<_>>()
                    .join("-");
                cases.push((
                    name,
                    desktops,
                    calamares_nixos::applications::default_selection(),
                    true,
                ));
            }
        }
        let mut configs = std::collections::BTreeMap::new();
        for (name, desktops, applications, allow_unfree) in cases {
            // The all-inclusive case also takes every GitHub step option.
            let github = name == "all";
            let text = |value: &str| if github { value.into() } else { String::new() };
            let r = RawRequest {
                disk: disk::Identity {
                    path: "/dev/vda".into(),
                    major_minor: "252:0".into(),
                    bytes: 40 * 1024u64.pow(3),
                    serial: "test".into(),
                    wwn: "".into(),
                    model: "test".into(),
                },
                firmware: Firmware::Uefi,
                filesystem: Filesystem::Ext4,
                hostname: "desktop-test".into(),
                username: "alice".into(),
                full_name: "Test".into(),
                password: "Public-Test-Only!".into(),
                locale: "en_US.UTF-8".into(),
                timezone: "America/Chicago".into(),
                keyboard: "us".into(),
                default_desktop: desktops[0],
                applications,
                desktops,
                copy_wifi: false,
                wifi_profiles: vec![],
                allow_unfree,
                swap: true,
                tuning: true,
                graphics: Default::default(),
                github_user: text("alice"),
                ssh_keys: github.then(|| TEST_SSH_KEY.into()).into_iter().collect(),
                ssh_server: github,
                git_name: text("Alice"),
                git_email: text("alice@example.com"),
                confirmation: "ERASE /dev/vda".into(),
            };
            configs.insert(
                name,
                config::configuration(
                    &r.parse(&settings)?,
                    &Identities {
                        root: "11111111-2222-4333-8444-555555555555".into(),
                        efi: Some("A1B2-C3D4".into()),
                        swap: Some("66666666-7777-4888-9999-aaaaaaaaaaaa".into()),
                    },
                )?,
            );
        }
        println!("{}", serde_json::to_string_pretty(&configs)?);
        return Ok(());
    }
    guard()?;
    let test_disk = disk::vm_test_disk()?.path;
    match std::env::args().nth(1).as_deref() {
        Some("prepare-integrated") => {
            ensure!(
                output("findmnt", &["-n", "-o", "FSTYPE", "/"], 10)?.trim() == "tmpfs",
                "Not live media"
            );
            // Exercise the ISO's real configuration and package. Only enable
            // diagnostics in this guarded VM; never mutate a Nix store file.
            let mut settings = Settings::load()?;
            ensure!(!settings.test_diagnostics, "Diagnostics shipped enabled");
            config::Template::load(&settings, &Hostname::parse("rust-test")?)?;
            let helper =
                Path::new("/run/current-system/sw/bin/calamares-nixos-helper").canonicalize()?;
            let package = helper.parent().unwrap().parent().unwrap();
            ensure!(package.starts_with("/nix/store"), "Unpackaged helper");
            fs::write(
                "/run/calamares-package-path",
                package.as_os_str().as_encoded_bytes(),
            )?;
            settings.test_diagnostics = true;
            let pending = Path::new("/etc/calamares-nixos/vm-settings.json");
            use std::fs::OpenOptions;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .open(pending)?;
            file.write_all(&serde_json::to_vec(&settings)?)?;
            file.sync_all()?;
            fs::rename(pending, calamares_nixos::SETTINGS)?;
            let wifi =
                Path::new("/etc/NetworkManager/system-connections/installer-fixture.nmconnection");
            let mut profile = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(wifi)?;
            profile.write_all(TEST_WIFI.as_bytes())?;
            profile.sync_all()?;
            output("nmcli", &["connection", "load", wifi.to_str().unwrap()], 15)?;
            output("timedatectl", &["set-timezone", "America/Chicago"], 15)?;
            println!("PASS: original media settings validated; VM diagnostics enabled");
            println!("PASS: synthetic Wi-Fi profile seeded; live zone set to America/Chicago");
        }
        Some("prepare") => {
            ensure!(
                output("findmnt", &["-n", "-o", "FSTYPE", "/"], 10)?.trim() == "tmpfs",
                "Not live media"
            );
            let package = std::env::args()
                .nth(2)
                .ok_or_else(|| anyhow::anyhow!("Missing package path"))?;
            ensure!(
                package.starts_with("/nix/store/") && Path::new(&package).is_dir(),
                "Invalid package path"
            );
            fs::write("/run/calamares-package-path", package)?;
            let dir = Path::new("/etc/calamares-nixos");
            fs::create_dir_all(dir)?;
            fs::write(
                dir.join("flake.nix.in"),
                fs::read("/etc/determinate-installer/flake.nix.in")?,
            )?;
            fs::write(
                dir.join("flake.lock"),
                fs::read("/etc/determinate-installer/flake.lock")?,
            )?;
            let settings = Settings {
                template_dir: dir.to_string_lossy().into_owned(),
                zoneinfo: "/etc/zoneinfo".into(),
                state_version: "26.11".into(),
                kernel: Kernel::Lts,
                test_diagnostics: true,
            };
            // Locate the live tzdata through the root-owned localtime link.
            // The live ISO can leave /etc/localtime unset. Find its trusted
            // store tzdata for this fixture; production settings pin the path.
            let zones = zoneinfo()?;
            let settings = Settings {
                zoneinfo: zones.to_string_lossy().into_owned(),
                ..settings
            };
            fs::write(dir.join("settings.json"), serde_json::to_vec(&settings)?)?;
            println!("{}", serde_json::to_string(&request()?)?);
        }
        Some("install") => {
            ensure!(
                output("findmnt", &["-n", "-o", "FSTYPE", "/"], 10)?.trim() == "tmpfs",
                "Not live media"
            );
            ensure!(
                output("blkid", &["-p", &test_disk], 10).is_err(),
                "Refusing an initialized test disk"
            );
            let before = output("sha256sum", &["/etc/calamares-nixos/flake.lock"], 10)?;
            // Invalid wire inputs must fail before creating a partition table.
            for invalid in [
                "username",
                "desktops",
                "wifi",
                "unknown-application",
                "unfree-application",
                "duplicate-application",
                "ssh-key",
                "github-user",
            ] {
                let mut bad = request()?;
                match invalid {
                    "ssh-key" => {
                        bad.github_user = "rust-test".into();
                        bad.ssh_keys = vec!["ssh-ed25519 AAAA\nssh-rsa AAAA".into()];
                    }
                    "github-user" => {
                        bad.github_user = "rust_test".into();
                        bad.ssh_keys = vec![TEST_SSH_KEY.into()];
                    }
                    "username" => bad.username = "root".into(),
                    "desktops" => bad.desktops.clear(),
                    "wifi" => bad.copy_wifi = false,
                    "unknown-application" => bad.applications = vec!["not-an-application".into()],
                    "unfree-application" => {
                        bad.applications = vec!["google-chrome".into()];
                        bad.allow_unfree = false;
                    }
                    "duplicate-application" => bad.applications = vec!["firefox".into(); 2],
                    _ => unreachable!(),
                }
                ensure!(invoke(bad, false).is_err(), "Accepted invalid {invalid}");
                ensure!(
                    output("blkid", &["-p", &test_disk], 10).is_err(),
                    "Negative {invalid} test wrote to disk"
                );
            }
            let mut bad = request()?;
            bad.confirmation = "ERASE /dev/not-the-disk".into();
            ensure!(
                invoke(bad, false).is_err(),
                "Accepted mismatched erase confirmation"
            );
            ensure!(
                output("blkid", &["-p", &test_disk], 10).is_err(),
                "Negative test wrote to disk"
            );
            invoke(request()?, true)?;
            ensure!(
                output("blkid", &["-p", &test_disk], 10).is_err(),
                "Preflight wrote to disk"
            );
            seed_previous_filesystem(&test_disk)?;
            if flag("CALAMARES_TEST_SESSION") {
                invoke_session(request()?)?;
            } else {
                invoke(request()?, false)?;
            }
            ensure!(
                before == output("sha256sum", &["/etc/calamares-nixos/flake.lock"], 10)?,
                "Media lock changed"
            );
        }
        Some("verify") => {
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read("/etc/installer-applications.json")?)?;
            let expected = calamares_nixos::applications::ApplicationSelection::parse(
                selected_applications(),
                true,
            )?;
            ensure!(
                manifest["selected"] == serde_json::to_value(expected.ids())?,
                "Installed applications differ from the reviewed selection"
            );
            for package in manifest["packages"]
                .as_array()
                .context("Missing application package manifest")?
            {
                ensure!(
                    Path::new(
                        package["path"]
                            .as_str()
                            .context("Missing application path")?
                    )
                    .is_dir(),
                    "Missing selected package: {}",
                    package["name"]
                );
            }
            println!("APPLICATIONS={}", expected.ids().join(","));
            ensure!(
                output("findmnt", &["-n", "-o", "FSTYPE", "/"], 10)?.trim()
                    == selected_filesystem()?.name(),
                "Installed root filesystem differs from the requested choice"
            );
            println!("ROOT_FILESYSTEM={}", selected_filesystem()?.name());
            // Hardware detection runs before erasure without filesystems; the
            // configuration pins identities that the installer chose, requested
            // when formatting and verified. They must be the booted ones.
            let hardware = fs::read_to_string("/etc/nixos/hardware-configuration.nix")?;
            ensure!(
                !hardware.contains("fileSystems.") && !hardware.contains("swapDevices"),
                "Hardware detection unexpectedly declared filesystems"
            );
            let installed = fs::read_to_string("/etc/nixos/configuration.nix")?;
            let root_uuid = output("findmnt", &["-n", "-o", "UUID", "/"], 10)?;
            ensure!(
                installed.contains(&format!("/dev/disk/by-uuid/{}", root_uuid.trim()))
                    && installed
                        .contains(&format!("fsType = \"{}\"", selected_filesystem()?.name())),
                "Configuration does not pin the booted root filesystem"
            );
            ensure!(
                output("findmnt", &["-n", "-o", "OPTIONS", "--mountpoint", "/"], 10)?
                    .contains("noatime"),
                "Root is not mounted noatime"
            );
            if selected_filesystem()? == Filesystem::Btrfs {
                ensure!(
                    output("findmnt", &["-n", "-o", "OPTIONS", "--mountpoint", "/"], 10)?
                        .contains("compress=zstd"),
                    "Btrfs compression did not survive reboot"
                );
            }
            ensure!(
                output("hostname", &[], 10)?.trim() == "rust-test",
                "Wrong hostname"
            );
            ensure!(
                output("nix", &["--version"], 15)?.contains("Determinate Nix"),
                "Wrong Nix distribution"
            );
            println!("{}", output("nix", &["--version"], 15)?);
            println!("{}", output("fh", &["--version"], 15)?);
            output(
                "systemctl",
                &["is-active", "determinate-nixd.socket", "display-manager"],
                30,
            )?;
            let shadow = fs::read_to_string("/etc/shadow")?;
            let hash = shadow
                .lines()
                .find_map(|l| l.strip_prefix("rusttest:"))
                .unwrap()
                .split(':')
                .next()
                .unwrap();
            sha_crypt::sha512_check(PASSWORD, hash)
                .map_err(|_| anyhow::anyhow!("Installed password hash does not authenticate"))?;
            ensure!(
                !pam_authenticate("deliberately-wrong-password")?,
                "PAM unexpectedly accepted a wrong password"
            );
            ensure!(
                pam_authenticate(PASSWORD)?,
                "PAM rejected the installed user's password"
            );
            let root_hash = shadow
                .lines()
                .find_map(|l| l.strip_prefix("root:"))
                .unwrap()
                .split(':')
                .next()
                .unwrap();
            ensure!(
                root_hash.starts_with('!') || root_hash.starts_with('*'),
                "Root account is not locked"
            );
            ensure!(
                !shadow.lines().any(|l| l.starts_with("nixos:")),
                "Live user leaked into target"
            );
            let secret = Path::new("/etc/nixos-secrets/user-password.hash");
            let metadata = fs::metadata(secret)?;
            ensure!(
                metadata.uid() == 0 && metadata.permissions().mode() & 0o777 == 0o600,
                "Password file permissions"
            );
            ensure!(
                fs::read_to_string(secret)?.trim() == hash,
                "Runtime hash mismatch"
            );
            ensure!(
                !Path::new("/etc/nixos/secrets").exists(),
                "Secret inside flake source"
            );
            let configuration = fs::read_to_string("/etc/nixos/configuration.nix")?;
            ensure!(
                configuration.contains("hardware.enableRedistributableFirmware = true;"),
                "Installed firmware policy missing"
            );
            ensure!(
                configuration.contains("nixpkgs.config.allowUnfree = true;"),
                "Installed hardware-friendly unfree default missing"
            );
            if github_test() {
                ensure!(
                    fs::read_to_string("/etc/ssh/authorized_keys.d/rusttest")?
                        .lines()
                        .any(|line| line == TEST_SSH_KEY),
                    "The GitHub key is not authorized for the installed user"
                );
                output("systemctl", &["is-active", "sshd.service"], 30)?;
                let sshd = output("sshd", &["-T"], 30)?;
                ensure!(
                    sshd.lines().any(|l| l == "passwordauthentication no"),
                    "The SSH server accepts passwords although keys are authorized"
                );
                ensure!(
                    output("git", &["config", "--system", "user.name"], 10)?.trim()
                        == "Rust ${literal} Test"
                        && output("git", &["config", "--system", "user.email"], 10)?.trim()
                            == "rusttest@example.com",
                    "The Git identity is not configured"
                );
                println!("GITHUB=authorized key, key-only sshd, Git identity");
            }
            output(
                "busctl",
                &[
                    "--system",
                    "call",
                    "org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                    "StartServiceByName",
                    "su",
                    "fi.w1.wpa_supplicant1",
                    "0",
                ],
                30,
            )?;
            output("systemctl", &["is-active", "wpa_supplicant"], 15)?;
            println!("HARDWARE_DEFAULTS=redistributable-firmware,unfree,wpa_supplicant");
            let option = |name: &str| {
                configuration
                    .lines()
                    .find_map(|line| line.trim().strip_prefix(name))
                    .map(|value| value.trim_end_matches(';').trim().to_owned())
            };
            let default =
                option("calamares.defaultDesktop = ").context("No explicit default desktop")?;
            let selected = Desktop::ALL
                .iter()
                .find(|d| default == format!("\"{}\"", d.id()))
                .context("Unknown default desktop")?;
            println!("DESKTOP_SESSION={}", selected.id());
            let desktops = option("calamares.desktops = ").context("No desktop selection")?;
            let enabled = Desktop::ALL
                .iter()
                .filter(|d| desktops.contains(&format!("\"{}\"", d.id())))
                .map(|d| d.id())
                .collect::<Vec<_>>()
                .join(",");
            println!("DESKTOPS={enabled}");
            if configuration.contains("calamares.zswap.enable = true;") {
                let swaps = output(
                    "swapon",
                    &["--show=TYPE,SIZE", "--bytes", "--noheadings"],
                    10,
                )?;
                let size: u64 = swaps
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("partition"))
                    .context("Swap partition is not active")?
                    .trim()
                    .parse()?;
                let expected = memory::swap_bytes(memory::read()?.total);
                // swapon excludes the one-page swap header.
                ensure!(
                    expected - size <= 1024 * 1024,
                    "Swap is {size} bytes; RAM-matched size is {expected}"
                );
                ensure!(
                    fs::read_to_string("/sys/module/zswap/parameters/enabled")?.trim() == "Y",
                    "zswap is not enabled"
                );
                ensure!(
                    fs::read_to_string("/proc/sys/vm/swappiness")?.trim() == "100",
                    "Swappiness is not 100"
                );
                ensure!(
                    fs::read_to_string("/proc/cmdline")?.contains("resume="),
                    "Hibernation resume device missing"
                );
                println!("SWAP=partition,{size},zswap,swappiness=100,resume");
            }
            // The helper leaves its stage timings for hardware measurements.
            let record: serde_json::Value =
                serde_json::from_slice(&fs::read("/var/log/calamares-nixos/install.json")?)?;
            ensure!(
                record["stages"]["storage"].is_number() && record["prepared"].is_object(),
                "Installation timing record is incomplete"
            );
            println!(
                "INSTALL_RECORD={}",
                serde_json::to_string(&record["stages"])?
            );
            if configuration.contains("calamares.tuning.enable = true;") {
                output("systemctl", &["is-active", "ananicy-cpp"], 15)?;
                ensure!(
                    fs::read_to_string("/proc/sys/vm/dirty_bytes")?.trim() == "268435456",
                    "CachyOS dirty limits missing"
                );
                println!("TUNING=ananicy-cpp,dirty_bytes");
            }
            ensure!(
                output(
                    "timedatectl",
                    &["show", "--property=Timezone", "--value"],
                    10
                )?
                .trim()
                    == "America/Chicago",
                "Installed time zone is not US Central"
            );
            let winter = output("date", &["--date=2026-01-15 12:00:00 UTC", "+%z %Z"], 10)?;
            let summer = output("date", &["--date=2026-07-15 12:00:00 UTC", "+%z %Z"], 10)?;
            ensure!(
                winter.trim() == "-0600 CST" && summer.trim() == "-0500 CDT",
                "Central daylight saving rules were not preserved"
            );
            let wifi =
                Path::new("/etc/NetworkManager/system-connections/installer-wifi-0.nmconnection");
            let metadata = fs::metadata(wifi)?;
            ensure!(
                metadata.uid() == 0 && metadata.permissions().mode() & 0o777 == 0o600,
                "Wi-Fi profile not root-only"
            );
            let profile = fs::read_to_string(wifi)?;
            ensure!(
                profile.contains("psk=WiFi-Synthetic-Only-123!")
                    && profile.contains("user:rusttest:;")
                    && !profile.contains("user:nixos:"),
                "Wi-Fi credentials or permissions not migrated"
            );
            ensure!(
                !configuration.contains("WiFi-Synthetic")
                    && !configuration.contains("Installer synthetic"),
                "Wi-Fi leaked into generated flake"
            );
            let known = output(
                "nmcli",
                &[
                    "-g",
                    "connection.type",
                    "connection",
                    "show",
                    "uuid",
                    "135ea3d9-d456-44b1-ae42-1e7081f66666",
                ],
                15,
            )?;
            ensure!(
                known.trim() == "802-11-wireless",
                "Installed NetworkManager did not load transferred Wi-Fi"
            );
            println!(
                "PASS: America/Chicago, winter CST/summer CDT; Wi-Fi credentials and user restrictions persisted root-only and loaded by NetworkManager"
            );
            ensure!(
                !configuration.contains(hash) && !configuration.contains(PASSWORD),
                "Secret in configuration"
            );
            ensure!(
                configuration.contains(&config::nix_string("Rust ${literal} Test")),
                "Nix interpolation escaping"
            );
            for path in [
                "/etc/determinate-installer",
                "/etc/calamares-nixos",
                "/run/current-system/sw/bin/calamares",
                "/run/current-system/sw/bin/calamares-nixos",
            ] {
                ensure!(!Path::new(path).exists(), "Live installer leaked: {path}");
            }
            output("id", &["rusttest"], 10)?;
            println!("{}", output("sha256sum", &["/etc/nixos/flake.lock"], 10)?);
            ensure!(
                output("id", &["-nG", "rusttest"], 10)?
                    .split_whitespace()
                    .any(|g| g == "wheel"),
                "User cannot administer machine"
            );
            println!(
                "PASS: installed system, password hash and PAM authentication, locked root, escaped configuration, desktop and Determinate services"
            );
        }
        _ => anyhow::bail!("prepare|prepare-integrated|install|verify"),
    }
    Ok(())
}
