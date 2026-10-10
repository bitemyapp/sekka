use crate::support::*;
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::TcpListener,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const BOOTSTRAP_SHA256: &str = "80588c226d84e16fe11b2e4afa9fc4add02902e7041dcb220960df5a6cde5fb5";
const GUEST_TOOL: &str =
    "/workspace/.work/guest-target/x86_64-unknown-linux-musl/release/respin-tools";

struct RemoveOnDrop(PathBuf);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn boot_entry(config: &str) -> Result<(String, String, String)> {
    let entry = config
        .split_once("LABEL boot\n")
        .context("Missing default ISO boot entry")?
        .1
        .split("\nLABEL ")
        .next()
        .unwrap();
    let value = |key: &str| -> Result<String> {
        entry
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .map(str::to_owned)
            .with_context(|| format!("Missing {key}"))
    };
    let safe = |text: String| -> Result<String> {
        let path = text.trim_start_matches('/').replace("//", "/");
        ensure!(
            Path::new(&path)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "Unsafe boot image member"
        );
        Ok(path)
    };
    Ok((
        safe(value("LINUX ")?)?,
        safe(value("INITRD ")?)?,
        format!("{} console=ttyS0,115200n8", value("APPEND ")?),
    ))
}

pub fn main(args: Vec<String>) -> Result<()> {
    let mut iso = None;
    let mut expected = BOOTSTRAP_SHA256.to_string();
    let mut rebuild_iso = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sha256" => expected = args.next().context("Missing SHA-256")?,
            "--rebuild-iso" => rebuild_iso = true,
            text if !text.starts_with('-') && iso.is_none() => iso = Some(PathBuf::from(text)),
            _ => bail!("Unknown argument: {arg}"),
        }
    }
    let iso = regular(
        &iso.context("Usage: build_rootless.rs BOOTSTRAP_ISO [--sha256 HEX] [--rebuild-iso]")?,
    )?;
    ensure!(sha256(&iso)? == expected, "Bootstrap ISO SHA-256 mismatch");
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .context("KVM is not accessible to this user")?;
    let repo = repo()?;
    // Build the test-only generator from the exact installer revision. It is
    // shared into the builder, never included in the ISO.
    crate::vm::fixture(&repo)?;
    let work = repo.join(".work/rootless");
    fs::create_dir_all(&work)?;
    let lock = work.join("builder.lock");
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock)
        .context("Builder lock exists; check for an active builder before removing it")?;
    let _lock = RemoveOnDrop(lock);
    let serial_path = work.join("serial.sock");
    ensure!(
        !serial_path.exists(),
        "Builder serial socket already exists"
    );
    let _socket = RemoveOnDrop(serial_path.clone());
    println!("Compiling a static Rust guest helper (no guest-side toolchain required)");
    run(Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "x86_64-unknown-linux-musl",
            "--manifest-path",
        ])
        .arg(repo.join("rust/Cargo.toml"))
        .arg("--target-dir")
        .arg(repo.join(".work/guest-target")))?;
    let key = work.join("ssh-key");
    if !key.exists() {
        run(Command::new("ssh-keygen")
            .args([
                "-q",
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                "respin-builder",
                "-f",
            ])
            .arg(&key))?;
    }
    let disk = work.join("builder.raw");
    if !disk.exists() {
        sparse(&disk, 100 * 1024u64.pow(3))?;
    }
    regular(&disk)?;
    ensure!(
        fs::metadata(&disk)?.len() == 100 * 1024u64.pow(3),
        "Wrong builder disk size"
    );
    let config = output(
        Command::new("bsdtar")
            .arg("-xOf")
            .arg(&iso)
            .arg("isolinux/isolinux.cfg"),
    )?;
    let (kernel, initrd, append) = boot_entry(&config)?;
    run(Command::new("bsdtar")
        .arg("-xf")
        .arg(&iso)
        .arg("-C")
        .arg(&work)
        .args([&kernel, &initrd]))?;
    let reserve = TcpListener::bind(("127.0.0.1", 0))?;
    let port = reserve.local_addr()?.port();
    drop(reserve);
    let log = File::create(work.join("qemu.log"))?;
    let mut qemu = Process(
        Command::new("qemu-system-x86_64")
            .args([
                "-name",
                "determinate-respin-builder",
                "-machine",
                "q35,accel=kvm",
                "-cpu",
                "host",
                "-smp",
                "6",
                "-m",
                "24576",
                "-display",
                "none",
                "-monitor",
                "none",
                "-no-reboot",
                "-serial",
            ])
            .arg(format!("unix:{},server=on,wait=off", qpath(&serial_path)?))
            .arg("-kernel")
            .arg(work.join(kernel))
            .arg("-initrd")
            .arg(work.join(initrd))
            .arg("-append")
            .arg(append)
            .arg("-drive")
            .arg(format!(
                "file={},format=raw,media=cdrom,readonly=on",
                qpath(&iso)?
            ))
            .arg("-drive")
            .arg(format!(
                "file={},format=raw,if=none,id=builder",
                qpath(&disk)?
            ))
            .args([
                "-device",
                "virtio-blk-pci,drive=builder,serial=RESPIN_BUILDER_ONLY",
                "-netdev",
            ])
            .arg(format!("user,id=net0,hostfwd=tcp:127.0.0.1:{port}-:22"))
            .args(["-device", "virtio-net-pci,netdev=net0", "-virtfs"])
            .arg(format!(
                "local,path={},mount_tag=project,security_model=none,id=project",
                qpath(&repo)?
            ))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?,
    );
    println!("Booting builder; diagnostics: {}", work.display());
    let deadline = Instant::now() + Duration::from_secs(600);
    while !serial_path.exists() {
        ensure!(
            qemu.0.try_wait()?.is_none() && Instant::now() < deadline,
            "QEMU did not start; inspect qemu.log"
        );
        pause(Duration::from_millis(200))?;
    }
    let mut serial = UnixStream::connect(&serial_path)?;
    serial.set_read_timeout(Some(Duration::from_secs(1)))?;
    let mut log = File::create(work.join("serial.log"))?;
    let mut received = Vec::new();
    let mut sent = false;
    loop {
        check_interrupt()?;
        ensure!(Instant::now() < deadline, "Builder setup timed out");
        let mut buffer = [0; 65536];
        let count = match serial.read(&mut buffer) {
            Ok(n) => n,
            Err(e)
                if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                    .contains(&e.kind()) =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        ensure!(count > 0, "Builder serial console closed");
        log.write_all(&buffer[..count])?;
        log.flush()?;
        received.extend_from_slice(&buffer[..count]);
        if received.len() > 131072 {
            received.drain(..received.len() - 65536);
        }
        let text = String::from_utf8_lossy(&received);
        if !sent && text.contains("nixos@nixos:") {
            writeln!(
                serial,
                "sudo mkdir -p /workspace && sudo mount -t 9p -o trans=virtio,version=9p2000.L project /workspace && sudo {GUEST_TOOL} prepare-builder"
            )?;
            sent = true;
            received.clear();
        } else if sent && text.contains("RESPIN_BUILDER_READY") {
            break;
        }
    }
    // Drain during the whole build and shutdown, avoiding serial backpressure.
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let reader = thread::spawn(move || {
        let mut buffer = [0; 65536];
        while !thread_stop.load(Ordering::Relaxed) {
            match serial.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let _ = log.write_all(&buffer[..count]);
                }
                Err(e)
                    if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                        .contains(&e.kind()) => {}
                Err(_) => break,
            }
        }
    });
    let artifacts = repo.join("artifacts/native-rust");
    fs::create_dir_all(&artifacts)?;
    let ssh = |remote: &str| -> Command {
        let mut cmd = Command::new("ssh");
        cmd.arg("-i")
            .arg(&key)
            .arg("-p")
            .arg(port.to_string())
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=15",
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
            ])
            .arg(format!(
                "UserKnownHostsFile={}",
                work.join("known-hosts").display()
            ))
            .arg("root@127.0.0.1")
            .arg(remote);
        cmd
    };
    let outcome = (|| -> Result<()> {
        println!(
            "Building; full log: {}",
            artifacts.join("build.log").display()
        );
        let log = File::create(artifacts.join("build.log"))?;
        let rebuild = if rebuild_iso { " --rebuild" } else { "" };
        let build_result = run(ssh(&format!("{GUEST_TOOL} build-iso{rebuild}"))
            .stdout(log.try_clone()?)
            .stderr(log));
        // The builder's store is persistent: even a failed build must flush
        // pending writes and shut down before the QEMU process is reaped.
        let mut poweroff = Process(ssh("sync && poweroff").spawn()?);
        let _ = poweroff.wait_cleanup(Duration::from_secs(20));
        ensure!(
            qemu.wait_cleanup(Duration::from_secs(60))?.success(),
            "Builder shutdown failed"
        );
        build_result?;
        Ok(())
    })();
    stop.store(true, Ordering::Relaxed);
    let _ = reader.join();
    outcome?;
    println!("Build complete: {}", artifacts.display());
    Ok(())
}

pub fn prepare() -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "Run only inside the builder VM as root"
    );
    ensure!(
        fs::read_to_string("/sys/class/block/vda/serial")?.trim() == "RESPIN_BUILDER_ONLY",
        "Wrong builder disk serial"
    );
    ensure!(
        output(Command::new("blockdev").args(["--getsize64", "/dev/vda"]))?.trim()
            == "107374182400",
        "Wrong builder disk size"
    );
    ensure!(
        output(Command::new("findmnt").args(["-n", "-o", "FSTYPE", "/"]))?.trim() == "tmpfs",
        "Not a live temporary root"
    );
    if !Command::new("blkid").arg("/dev/vda").status()?.success() {
        run(Command::new("mkfs.ext4").args(["-L", "respin-build", "/dev/vda"]))?;
    }
    ensure!(
        output(Command::new("blkid").args(["-s", "LABEL", "-o", "value", "/dev/vda"]))?.trim()
            == "respin-build",
        "Wrong builder disk label"
    );
    fs::create_dir_all("/build")?;
    run(Command::new("mount").args(["/dev/vda", "/build"]))?;
    run(Command::new("systemctl").args([
        "stop",
        "determinate-nixd.socket",
        "nix-daemon.socket",
        "nix-daemon.service",
    ]))?;
    if !Path::new("/build/store-ready").exists() {
        fs::create_dir_all("/build/nix")?;
        run(Command::new("cp").args(["-a", "/nix/store", "/nix/var", "/build/nix/"]))?;
        fs::write("/build/store-ready", "")?;
    }
    run(Command::new("mount").args(["--bind", "/build/nix", "/nix"]))?;
    run(Command::new("systemctl").args(["start", "determinate-nixd.socket", "nix-daemon.socket"]))?;
    fs::create_dir_all("/build/tmp")?;
    fs::create_dir_all("/root/.ssh")?;
    fs::copy(
        "/workspace/.work/rootless/ssh-key.pub",
        "/root/.ssh/authorized_keys",
    )?;
    fs::set_permissions("/root/.ssh", fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(
        "/root/.ssh/authorized_keys",
        fs::Permissions::from_mode(0o600),
    )?;
    run(Command::new("git").args([
        "config",
        "--global",
        "--add",
        "safe.directory",
        "/workspace",
    ]))?;
    println!("RESPIN_BUILDER_READY");
    Ok(())
}

pub fn build_iso(rebuild: bool) -> Result<()> {
    ensure!(
        fs::read_to_string("/sys/class/block/vda/serial")?.trim() == "RESPIN_BUILDER_ONLY",
        "Not the builder VM"
    );
    check_selections()?;
    run(Command::new("nix")
        .current_dir("/workspace")
        .env("TMPDIR", "/build/tmp")
        .args([
            "flake",
            "check",
            "--no-update-lock-file",
            "--max-jobs",
            "1",
            "--cores",
            "4",
            "-L",
        ]))?;
    export_wifi_tools()?;
    let mut build = Command::new("nix");
    build
        .current_dir("/workspace")
        .env("TMPDIR", "/build/tmp")
        .args([
            "build",
            ".#iso",
            "--out-link",
            "/build/result",
            "--no-update-lock-file",
            "--cores",
            "4",
            "--max-jobs",
            "1",
            "-L",
        ]);
    if rebuild {
        build.arg("--rebuild");
    }
    run(&mut build)?;
    let dest = Path::new("/workspace/artifacts/native-rust");
    fs::create_dir_all(dest)?;
    let mut found = false;
    for entry in fs::read_dir("/build/result/iso")? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) != Some("iso") {
            continue;
        }
        found = true;
        let mut temp = tempfile::NamedTempFile::new_in(dest)?;
        std::io::copy(&mut File::open(entry.path())?, temp.as_file_mut())?;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))?;
        temp.as_file().sync_all()?;
        let target = dest.join(entry.file_name());
        temp.persist(&target)?;
        let line = format!(
            "{}  {}\n",
            sha256(&target)?,
            entry.file_name().to_string_lossy()
        );
        fs::write(target.with_extension("iso.sha256"), &line)?;
        print!("{line}");
    }
    ensure!(found, "Build output contained no ISO");
    run(&mut Command::new("sync"))?;
    Ok(())
}

fn export_wifi_tools() -> Result<()> {
    let path = output(
        Command::new("nix")
            .current_dir("/workspace")
            .env("TMPDIR", "/build/tmp")
            .args([
                "build",
                ".#wifi-test-tools",
                "--no-link",
                "--print-out-paths",
                "--no-update-lock-file",
            ]),
    )?;
    let path = path.trim();
    ensure!(
        path.starts_with("/nix/store/") && !path.contains(char::is_whitespace),
        "Invalid test tools output"
    );
    let closure = output(Command::new("nix-store").args(["--query", "--requisites", path]))?;
    let dest = Path::new("/workspace/artifacts/wifi-tools");
    fs::create_dir_all(dest)?;
    let nar = dest.join("closure.nar");
    let lock_sha256 = sha256(Path::new("/workspace/flake.lock"))?;
    if let Ok(bytes) = fs::read(dest.join("manifest.json"))
        && let Ok(cached) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && cached["store_path"] == path
        && cached["lock_sha256"] == lock_sha256
        && nar.is_file()
        && cached["sha256"] == sha256(&nar)?
    {
        println!("Reusing verified separate Wi-Fi test tools: {path}");
        return Ok(());
    }
    let file = File::create(&nar)?;
    run(Command::new("nix-store")
        .arg("--export")
        .args(closure.lines())
        .stdout(file.try_clone()?))?;
    file.sync_all()?;
    write_json(
        &dest.join("manifest.json"),
        &serde_json::json!({
            "store_path": path, "sha256": sha256(&nar)?,
            "lock_sha256": lock_sha256,
        }),
    )?;
    println!("Exported separate Wi-Fi test tools: {path}");
    Ok(())
}

pub fn check_selections() -> Result<()> {
    // Nonempty subsets of the five desktops the installer offers.
    check_configurations("desktop", 31, 8)?;
    check_configurations("application", 34, 1)
}

fn check_configurations(kind: &str, expected: usize, jobs: usize) -> Result<()> {
    let repo = Path::new("/workspace");
    let fixture = std::env::var_os("RESPIN_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(".work/native-fixture/local/bin/calamares-vm-fixture"));
    let configurations = output(Command::new(fixture).arg(format!("{kind}-configurations")))?;
    let cases: std::collections::BTreeMap<String, String> = serde_json::from_str(&configurations)?;
    ensure!(
        cases.len() == expected,
        "Expected all {expected} supported {kind} configurations"
    );
    let path = repo.join(format!(".work/{kind}-configurations.json"));
    fs::write(&path, configurations)?;
    let modules = repo.join(format!(".work/{kind}-modules"));
    fs::create_dir_all(&modules)?;
    for (name, source) in &cases {
        ensure!(
            name.bytes().all(|c| c.is_ascii_lowercase() || c == b'-'),
            "Invalid configuration case name"
        );
        // The test expressions supply the modules from the Sekka flake
        // and their own filesystems in place of hardware detection.
        let source = source.replace("imports = [ ./hardware-configuration.nix ];", "");
        let mut file = File::create(modules.join(format!("{name}.nix")))?;
        file.write_all(source.as_bytes())?;
        file.sync_all()?;
    }
    let report = repo.join(format!("artifacts/native-rust/{kind}-matrix.json"));
    let expression = format!("tests/{kind}-matrix.nix");
    // A monorepo edit can change module semantics without changing the generated
    // selections. Always evaluate; Nix's own cache handles unchanged inputs.
    let mut results = std::collections::BTreeMap::<String, serde_json::Value>::new();
    let names: Vec<_> = cases.keys().collect();
    // Application cases import several package sets; evaluate those one at a
    // time. Desktop-only configurations can retain bounded parallelism.
    for batch in names.chunks(jobs) {
        let evaluated = thread::scope(|scope| -> Result<Vec<(String, serde_json::Value)>> {
            let jobs: Vec<_> = batch
                .iter()
                .map(|name| {
                    let expression = &expression;
                    scope.spawn(move || -> Result<_> {
                        println!("Evaluating {kind} selection: {name}");
                        let apply = format!(
                            "f: f {{ configurations = \"/workspace/.work/{kind}-configurations.json\"; caseName = \"{name}\"; }}"
                        );
                        let result = output(Command::new("nix").current_dir(repo).args([
                            "eval", "--impure", "--json", "--file", expression, "--apply", &apply,
                        ]))?;
                        Ok(((*name).clone(), serde_json::from_str(&result)?))
                    })
                })
                .collect();
            jobs.into_iter()
                .map(|job| {
                    job.join()
                        .map_err(|_| anyhow::anyhow!("Configuration evaluator worker panicked"))?
                })
                .collect()
        })?;
        results.extend(evaluated);
    }
    fs::write(
        report,
        serde_json::to_vec_pretty(&serde_json::json!({"cases":results}))?,
    )?;
    println!("PASS: all {expected} supported {kind} configurations evaluate against pinned NixOS");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_boot_entry() {
        let (kernel, initrd, args) = super::boot_entry("LABEL boot\nLINUX /boot//kernel\nINITRD /boot/initrd\nAPPEND init=/init\nLABEL other\n").unwrap();
        assert_eq!(kernel, "boot/kernel");
        assert_eq!(initrd, "boot/initrd");
        assert!(args.contains("console=ttyS0"));
        assert!(
            super::boot_entry("LABEL boot\nLINUX /../escape\nINITRD /initrd\nAPPEND a").is_err()
        );
    }
}
