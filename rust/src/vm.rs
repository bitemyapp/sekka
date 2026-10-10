use crate::support::*;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const SHORT: Duration = Duration::from_secs(120);
static IDS: AtomicU64 = AtomicU64::new(1);
pub struct Rpc {
    reader: BufReader<UnixStream>,
}
impl Rpc {
    pub fn connect(path: &Path, qmp: bool, seconds: u64) -> Result<Self> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_secs(seconds)))?;
        stream.set_write_timeout(Some(Duration::from_secs(seconds)))?;
        let mut rpc = Self {
            reader: BufReader::new(stream),
        };
        if qmp {
            rpc.read()?;
            rpc.call("qmp_capabilities", json!({}))?;
        }
        Ok(rpc)
    }
    fn read(&mut self) -> Result<Value> {
        let mut line = String::new();
        ensure!(
            self.reader.read_line(&mut line)? > 0,
            "QEMU RPC connection closed"
        );
        Ok(serde_json::from_str(line.trim())?)
    }
    fn send(&mut self, command: &str, arguments: Value) -> Result<u64> {
        let id = IDS.fetch_add(1, Ordering::Relaxed);
        writeln!(
            self.reader.get_mut(),
            "{}",
            json!({"execute":command,"arguments":arguments,"id":id})
        )?;
        Ok(id)
    }
    pub fn request_shutdown(&mut self) -> Result<()> {
        // guest-shutdown deliberately has no success response. Vm::poweroff
        // confirms success by waiting for QEMU to exit with status zero.
        // https://www.qemu.org/docs/master/interop/qemu-ga-ref.html#command-guest-shutdown
        self.send("guest-shutdown", json!({"mode":"powerdown"}))?;
        Ok(())
    }
    pub fn screenshot(&mut self, dest: &Path) -> Result<()> {
        self.call("screendump", json!({"filename":dest,"format":"png"}))?;
        Ok(())
    }
    pub fn call(&mut self, command: &str, arguments: Value) -> Result<Value> {
        let id = self.send(command, arguments)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            check_interrupt()?;
            ensure!(Instant::now() < deadline, "QEMU RPC response timed out");
            let response = self.read()?;
            if response["id"] != id {
                continue;
            }
            ensure!(
                response.get("error").is_none(),
                "QEMU RPC error: {response}"
            );
            return response
                .get("return")
                .cloned()
                .context("Missing QEMU RPC result");
        }
    }
}

pub struct Vm {
    work: PathBuf,
    process: Option<Process>,
    agent: Option<Rpc>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputDevices {
    Ps2,
    Usb,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskBus {
    Virtio,
    Nvme,
    Nvme4k,
}
impl DiskBus {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "virtio" => Ok(Self::Virtio),
            "nvme" => Ok(Self::Nvme),
            "nvme4k" => Ok(Self::Nvme4k),
            _ => bail!("Disk bus must be virtio, nvme or nvme4k"),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Virtio => "virtio",
            Self::Nvme => "nvme",
            Self::Nvme4k => "nvme4k",
        }
    }
    fn device(self) -> &'static str {
        match self {
            Self::Virtio => "virtio-blk-pci,drive=target,serial=RESPIN_TEST_ONLY,bootindex=2",
            Self::Nvme => "nvme,drive=target,serial=RESPIN_TEST_ONLY,bootindex=2",
            Self::Nvme4k => {
                "nvme,drive=target,serial=RESPIN_TEST_ONLY,bootindex=2,logical_block_size=4096,physical_block_size=4096"
            }
        }
    }
}
fn filesystem(value: &str, allow_blank: bool) -> Result<String> {
    ensure!(
        ["ext4", "btrfs", "xfs"].contains(&value) || (allow_blank && value == "blank"),
        "Unsupported test filesystem: {value}"
    );
    Ok(value.into())
}
fn acceleration() -> Result<String> {
    let value = std::env::var("RESPIN_QEMU_ACCEL").unwrap_or_else(|_| "kvm".into());
    ensure!(
        ["kvm", "tcg", "tcg-single"].contains(&value.as_str()),
        "RESPIN_QEMU_ACCEL must be kvm, tcg or tcg-single"
    );
    Ok(value)
}
impl Vm {
    pub fn start(
        work: &Path,
        firmware: &str,
        iso: Option<&Path>,
        disk: Option<&Path>,
        share: Option<&Path>,
    ) -> Result<Self> {
        Self::start_with_input(work, firmware, iso, disk, share, InputDevices::Usb)
    }
    pub fn start_with_input(
        work: &Path,
        firmware: &str,
        iso: Option<&Path>,
        disk: Option<&Path>,
        share: Option<&Path>,
        input: InputDevices,
    ) -> Result<Self> {
        Self::start_with_devices(work, firmware, iso, disk, share, input, DiskBus::Virtio)
    }
    pub fn start_with_devices(
        work: &Path,
        firmware: &str,
        iso: Option<&Path>,
        disk: Option<&Path>,
        share: Option<&Path>,
        input: InputDevices,
        disk_bus: DiskBus,
    ) -> Result<Self> {
        ensure!(["bios", "uefi"].contains(&firmware), "Unknown firmware");
        fs::create_dir_all(work)?;
        for socket in ["qga.sock", "qmp.sock"] {
            ensure!(
                !work.join(socket).exists(),
                "Socket already exists; check for a running VM: {socket}"
            );
        }
        let accelerator = acceleration()?;
        let number = |name: &str, default: &str| -> Result<String> {
            let value = std::env::var(name).unwrap_or_else(|_| default.into());
            ensure!(
                !value.is_empty() && value.len() <= 12 && value.bytes().all(|c| c.is_ascii_digit()),
                "{name} must be a number"
            );
            Ok(value)
        };
        // Defaults keep earlier results comparable; timing runs can model a laptop.
        let memory = number("RESPIN_QEMU_MEMORY", "8192")?;
        let cpus = number("RESPIN_QEMU_CPUS", "4")?;
        // Optional read limit modelling a USB stick, in bytes per second.
        let usb = std::env::var("RESPIN_USB_BPS")
            .ok()
            .map(|_| number("RESPIN_USB_BPS", "0"))
            .transpose()?
            .map(|bps| format!(",throttling.bps-read={bps}"))
            .unwrap_or_default();
        let machine = format!(
            "q35,vmport=off,i8042={}",
            if input == InputDevices::Usb {
                "off"
            } else {
                "on"
            }
        );
        let mut command = Command::new("qemu-system-x86_64");
        command
            .args([
                "-name",
                "determinate-respin-test",
                "-smbios",
                "type=1,serial=RESPIN_VM_ONLY",
                "-machine",
                // Isolate each input path: USB runs have no emulated PS/2
                // controller; PS/2 runs have no VMware absolute-pointer shim.
                &machine,
                "-accel",
                if accelerator == "tcg-single" {
                    "tcg,thread=single"
                } else {
                    &accelerator
                },
                "-cpu",
                if accelerator == "kvm" { "host" } else { "max" },
                "-smp",
                &cpus,
                "-m",
                &memory,
                "-display",
                "none",
                "-vga",
                "std",
                "-no-reboot",
                "-serial",
            ])
            .arg(format!("file:{}", work.join("serial.log").display()))
            .arg("-qmp")
            .arg(format!(
                "unix:{},server=on,wait=off",
                qpath(&work.join("qmp.sock"))?
            ))
            .args(["-device", "virtio-serial-pci", "-chardev"])
            .arg(format!(
                "socket,path={},server=on,wait=off,id=qga",
                qpath(&work.join("qga.sock"))?
            ))
            .args([
                "-device",
                "virtserialport,chardev=qga,name=org.qemu.guest_agent.0",
                "-netdev",
                "user,id=net0",
                "-device",
                "virtio-net-pci,netdev=net0",
            ]);
        if firmware == "uefi" {
            let code = PathBuf::from(
                std::env::var_os("OVMF_CODE")
                    .unwrap_or("/usr/share/edk2/x64/OVMF_CODE.4m.fd".into()),
            );
            let source = PathBuf::from(
                std::env::var_os("OVMF_VARS")
                    .unwrap_or("/usr/share/edk2/x64/OVMF_VARS.4m.fd".into()),
            );
            let vars = work.join("OVMF_VARS.fd");
            if !vars.exists() {
                fs::copy(source, &vars)?;
                // Firmware from the Nix store is read-only; UEFI variables are not.
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&vars, fs::Permissions::from_mode(0o644))?;
            }
            command
                .arg("-drive")
                .arg(format!(
                    "if=pflash,format=raw,readonly=on,file={}",
                    qpath(&code)?
                ))
                .arg("-drive")
                .arg(format!("if=pflash,format=raw,file={}", qpath(&vars)?));
        }
        if iso.is_some() || input == InputDevices::Usb {
            command.args(["-device", "qemu-xhci,id=xhci"]);
        }
        if input == InputDevices::Usb {
            command.args([
                "-device",
                "usb-kbd,bus=xhci.0",
                "-device",
                "usb-tablet,bus=xhci.0",
            ]);
        }
        if let Some(iso) = iso {
            regular(iso)?;
            command
                .arg("-drive")
                .arg(format!(
                    "if=none,id=iso,format=raw,readonly=on,file={}{usb}",
                    qpath(iso)?
                ))
                .args(["-device", "usb-storage,drive=iso,bootindex=1"]);
        }
        if let Some(disk) = disk {
            regular(disk)?;
            command
                .arg("-drive")
                .arg(format!(
                    "if=none,id=target,format=raw,file={}",
                    qpath(disk)?
                ))
                .args(["-device", disk_bus.device()]);
        }
        if let Some(repo) = share {
            command.arg("-virtfs").arg(format!(
                "local,path={},mount_tag=project,security_model=none,id=project",
                qpath(repo)?
            ));
        }
        let log = File::create(work.join("qemu.log"))?;
        let process = Process(
            command
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log)
                .spawn()?,
        );
        Ok(Self {
            work: work.to_path_buf(),
            process: Some(process),
            agent: None,
        })
    }
    pub fn wait_agent(&mut self) -> Result<()> {
        let deadline =
            Instant::now() + Duration::from_secs(if acceleration()? == "kvm" { 240 } else { 900 });
        let mut last = String::new();
        while Instant::now() < deadline {
            ensure!(
                self.process
                    .as_mut()
                    .context("VM stopped")?
                    .0
                    .try_wait()?
                    .is_none(),
                "QEMU exited: {}",
                fs::read_to_string(self.work.join("qemu.log"))?
            );
            match Rpc::connect(&self.work.join("qga.sock"), false, 3).and_then(|mut rpc| {
                rpc.call("guest-ping", json!({}))?;
                Ok(rpc)
            }) {
                Ok(rpc) => {
                    rpc.reader
                        .get_ref()
                        .set_read_timeout(Some(Duration::from_secs(20)))?;
                    self.agent = Some(rpc);
                    return Ok(());
                }
                Err(error) => last = error.to_string(),
            }
            pause(Duration::from_secs(2))?;
        }
        bail!(
            "Guest agent did not start: {last}; see {}",
            self.work.join("serial.log").display()
        )
    }
    pub fn execute(&mut self, command: &str, timeout: Duration) -> Result<String> {
        let agent = self.agent.as_mut().context("Guest agent not connected")?;
        let pid = agent.call("guest-exec", json!({"path":"/run/current-system/sw/bin/bash","arg":["-lc", command],"capture-output":true}))?["pid"].clone();
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            let status = agent.call("guest-exec-status", json!({"pid":pid}))?;
            if status["exited"] == true {
                let mut text = String::new();
                for key in ["out-data", "err-data"] {
                    text.push_str(&String::from_utf8_lossy(
                        &STANDARD.decode(status[key].as_str().unwrap_or(""))?,
                    ));
                }
                ensure!(
                    status["exitcode"] == 0,
                    "Guest command failed: {command}\n{text}"
                );
                return Ok(text);
            }
            pause(Duration::from_secs(1))?;
        }
        bail!("Guest command timed out: {command}")
    }
    pub fn screenshot(&self, dest: &Path) -> Result<()> {
        Rpc::connect(&self.work.join("qmp.sock"), true, 20)?.screenshot(dest)
    }
    pub fn poweroff(&mut self) -> Result<()> {
        self.agent
            .as_mut()
            .context("No guest agent")?
            .request_shutdown()?;
        let status = self
            .process
            .as_mut()
            .context("No VM")?
            .wait(Duration::from_secs(60))?;
        ensure!(status.success(), "QEMU poweroff failed");
        self.stop();
        Ok(())
    }
    pub fn stop(&mut self) {
        self.agent = None;
        self.process.take();
        for path in ["qga.sock", "qmp.sock"] {
            let _ = fs::remove_file(self.work.join(path));
        }
    }
    fn diagnostics(&mut self, artifacts: &Path) {
        let _ = self.screenshot(&artifacts.join("failure.png"));
        if self.agent.is_none() {
            return;
        }
        if let Ok(text) = self.execute("systemctl --failed; ps -eo uid,pid,comm,args; journalctl -b -p warning --no-pager | tail -150", SHORT) {
            let _ = fs::write(artifacts.join("diagnostics.log"), text);
        }
    }
}
impl Drop for Vm {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn fixture(repo: &Path) -> Result<String> {
    // Development runs: a static fixture built from a local installer checkout.
    if let Some(local) = std::env::var_os("RESPIN_FIXTURE") {
        let dir = repo.join(".work/native-fixture/local/bin");
        fs::create_dir_all(&dir)?;
        fs::copy(
            regular(Path::new(&local))?,
            dir.join("calamares-vm-fixture"),
        )?;
        return Ok("/workspace/.work/native-fixture/local/bin/calamares-vm-fixture".into());
    }
    let root = repo.join(".work/native-fixture/local");
    run(Command::new("cargo")
        .args(["install", "--path"])
        .arg(repo.join("installer/genkan/tests/vm-fixture"))
        .args([
            "--locked",
            "--target",
            "x86_64-unknown-linux-musl",
            "--root",
        ])
        .arg(&root)
        .arg("--target-dir")
        .arg(repo.join(".work/fixture-target")))?;
    regular(&root.join("bin/calamares-vm-fixture"))?;
    Ok("/workspace/.work/native-fixture/local/bin/calamares-vm-fixture".into())
}

/// Optional fixture choices passed through from the host environment.
fn fixture_options() -> Result<String> {
    let mut options = String::new();
    for name in [
        "CALAMARES_TEST_DESKTOPS",
        "CALAMARES_TEST_SWAP",
        "CALAMARES_TEST_TUNING",
        "CALAMARES_TEST_SESSION",
    ] {
        if let Ok(value) = std::env::var(name) {
            ensure!(
                value.len() < 256
                    && value.bytes().all(|c| c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || c == b','
                        || c == b'-'),
                "Invalid {name}"
            );
            options.push_str(&format!("{name}={value} "));
        }
    }
    Ok(options)
}

/// Stage durations reported by the helper and the fixture's click-to-complete time.
fn timing(log: &str) -> Value {
    let mut stages = serde_json::Map::new();
    let mut result = json!({});
    for line in log.lines() {
        if let Ok(event) = serde_json::from_str::<Value>(line)
            && event["kind"] == "timing"
            && let (Some(stage), Some(seconds)) =
                (event["stage"].as_str(), event["seconds"].as_f64())
        {
            stages.insert(stage.into(), json!(seconds));
        } else if let Ok(event) = serde_json::from_str::<Value>(line)
            && event["kind"] == "prepared"
        {
            result["prepared"] = event["summary"].clone();
        }
        for marker in ["PREPARED_SECONDS", "CLICK_TO_COMPLETE_SECONDS"] {
            if let Some(value) = line.strip_prefix(&format!("{marker}="))
                && let Ok(seconds) = value.trim().parse::<f64>()
            {
                result[marker.to_lowercase()] = json!(seconds);
            }
        }
    }
    result["stages"] = Value::Object(stages);
    result
}

fn verify_installed(
    vm: &mut Vm,
    artifacts: &Path,
    repo: &Path,
    fixture: &str,
    graphical_login: bool,
    filesystem: &str,
    applications: &str,
) -> Result<String> {
    vm.wait_agent()?;
    vm.execute("for i in $(seq 1 90); do systemctl is-active --quiet display-manager && exit 0; sleep 1; done; exit 1", SHORT)?;
    vm.execute(
        "mkdir -p /workspace; mount -t 9p -o trans=virtio,version=9p2000.L project /workspace",
        SHORT,
    )?;
    let output = vm.execute(
        &format!(
            "CALAMARES_TEST_FILESYSTEM={} CALAMARES_TEST_APPLICATIONS={} {} verify",
            shell_quote(filesystem),
            shell_quote(applications),
            shell_quote(fixture)
        ),
        Duration::from_secs(180),
    )?;
    ensure!(
        output.contains("nix (Determinate Nix "),
        "Installed Nix is not Determinate: {output}"
    );
    ensure!(
        output.contains(&sha256(&repo.join("flake.lock"))?),
        "Installed lock differs from repository"
    );
    pause(Duration::from_secs(10))?;
    if graphical_login {
        let mut qmp = Rpc::connect(&vm.work.join("qmp.sock"), true, 20)?;
        // Fixed public disposable-VM credentials, never the host's password.
        crate::gui_input::type_text(&mut qmp, "Qemu-Only-Test-123!")?;
        crate::gui_input::key(&mut qmp, &["ret"])?;
        drop(qmp);
        let session = output
            .lines()
            .find_map(|line| line.strip_prefix("DESKTOP_SESSION="))
            .context("Fixture did not report the default session")?;
        let process = match session {
            "plasma" => "plasmashell",
            "xfce" => "xfce4-session",
            "gnome" => "gnome-shell",
            "cinnamon" => "cinnamon-session",
            "mate" => "mate-session",
            "lxqt" => "lxqt-session",
            // Both Hyprland desktops run the same compositor binary. Images
            // from before Tatami's rename report "omarchy".
            "hyprland" | "tatami" | "omarchy" => "Hyprland",
            other => bail!("No login check for desktop session {other}"),
        };
        // Session managers can use a bare argv[0], a bin/libexec path, or a
        // Nix wrapper. Match the exact program token, not a hard-coded path.
        vm.execute(&format!("for i in $(seq 1 90); do pgrep -u 1000 -f '(^|/)[.]?{process}(-wrapped)?( |$)' && exit 0; sleep 1; done; exit 1"), SHORT)?;
        pause(Duration::from_secs(10))?;
    }
    vm.screenshot(&artifacts.join("installed-desktop.png"))?;
    Ok(output)
}

fn record(
    mut vm: Vm,
    work: &Path,
    artifacts: &Path,
    result: &mut Value,
    outcome: Result<()>,
) -> Result<()> {
    result["passed"] = json!(outcome.is_ok());
    if let Err(error) = &outcome {
        result["error"] = json!(format!("{error:#}"));
        vm.diagnostics(artifacts);
    }
    vm.stop();
    for file in ["serial.log", "qemu.log"] {
        optional_copy(&work.join(file), &artifacts.join(file))?;
    }
    write_json(&artifacts.join("result.json"), result)?;
    outcome?;
    println!("PASS: {}", artifacts.display());
    Ok(())
}

pub fn main(args: Vec<String>) -> Result<()> {
    let mut iso = None;
    let mut firmware = "uefi".to_string();
    let mut install = false;
    let mut disk_bus = DiskBus::Nvme;
    let mut root_filesystem = "ext4".to_string();
    let mut previous_filesystem = None;
    let mut gui = false;
    let mut applications = "firefox".to_string();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--firmware" => firmware = args.next().context("Missing firmware")?,
            "--install" => install = true,
            "--applications" => {
                applications = args.next().context("Missing applications")?;
                ensure!(
                    !applications.is_empty()
                        && applications.len() < 1024
                        && applications.bytes().all(|c| c.is_ascii_lowercase()
                            || c.is_ascii_digit()
                            || c == b'-'
                            || c == b','),
                    "Invalid application selection"
                );
            }
            "--disk-bus" => disk_bus = DiskBus::parse(&args.next().context("Missing disk bus")?)?,
            "--filesystem" => {
                root_filesystem = filesystem(&args.next().context("Missing filesystem")?, false)?
            }
            "--previous-filesystem" => {
                previous_filesystem = Some(filesystem(
                    &args.next().context("Missing previous filesystem")?,
                    true,
                )?)
            }
            "--gui" => {
                gui = true;
                install = true;
            }
            value if !value.starts_with('-') && iso.is_none() => iso = Some(PathBuf::from(value)),
            _ => bail!("Unknown argument: {arg}"),
        }
    }
    ensure!(
        ["bios", "uefi"].contains(&firmware.as_str()),
        "Firmware must be bios or uefi"
    );
    let previous_filesystem =
        previous_filesystem.unwrap_or_else(|| if gui { "blank" } else { "btrfs" }.into());
    ensure!(
        !gui || previous_filesystem == "blank",
        "GUI tests require --previous-filesystem blank; backend tests seed prior filesystems"
    );
    let iso = regular(&iso.context("Usage: qemu_test.rs ISO [--firmware bios|uefi] [--install] [--disk-bus virtio|nvme|nvme4k] [--filesystem ext4|btrfs|xfs] [--previous-filesystem blank|ext4|btrfs|xfs] [--applications all|none|ID,ID]")?)?;
    let digest = sha256(&iso)?;
    let repo = repo()?;
    let name = format!(
        "{firmware}-{}-{root_filesystem}-{}",
        disk_bus.name(),
        stamp()
    );
    let work = repo.join(".work").join(&name);
    let artifacts = repo.join("artifacts").join(&name);
    fs::create_dir_all(repo.join(".work"))?;
    fs::create_dir_all(repo.join("artifacts"))?;
    fs::create_dir(&work)?;
    fs::create_dir(&artifacts)?;
    let disk = work.join("target.raw");
    let disk_size_gib = if applications == "firefox" || applications == "none" {
        40u64
    } else {
        80
    };
    if install {
        sparse(&disk, disk_size_gib * 1024u64.pow(3))?;
    }
    let fixture = if install { Some(fixture(&repo)?) } else { None };
    let revision = output(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD"]),
    )?
    .trim()
    .to_owned();
    let mut result = json!({"firmware":firmware,"media":"usb-storage","iso":iso.file_name().unwrap().to_string_lossy(),"iso_sha256":digest,"install":install,"gui":gui,"passed":false,"implementation":"native-rust","sekka_revision":revision,"disk_bus":disk_bus.name(),"filesystem":root_filesystem,"previous_filesystem":previous_filesystem,"qemu_acceleration":acceleration()?});
    result["applications"] = json!(applications);
    result["disk_size_gib"] = json!(disk_size_gib);
    let mut vm = Vm::start_with_devices(
        &work,
        &firmware,
        Some(&iso),
        install.then_some(disk.as_path()),
        install.then_some(repo.as_path()),
        InputDevices::Usb,
        disk_bus,
    )?;
    let outcome = (|| -> Result<()> {
        println!("Booting {name} from USB mass storage");
        vm.wait_agent()?;
        let end = Instant::now() + Duration::from_secs(180);
        let live = loop {
            match vm.execute("set -e; systemctl is-active display-manager; pgrep -u 1000 -f '^/[^ ]+/bin/[.]?plasmashell'; pgrep -u 1000 -f '^/[^ ]+/bin/[.]?calamares-nixos(-wrapped)?( |$)'; nix --version; fh --version; systemctl is-active determinate-nixd.socket; test -s /etc/calamares-nixos/flake.lock; test ! -e /run/current-system/sw/bin/calamares; test ! -e /run/current-system/sw/bin/respin-tools; test ! -e /etc/nixos-generate-config.conf; cat /etc/calamares-nixos/settings.json; lsblk -o NAME,TRAN,FSTYPE,LABEL", SHORT) {
                Ok(text) => break text, Err(e) if Instant::now() >= end => return Err(e), Err(_) => pause(Duration::from_secs(3))?,
            }
        };
        ensure!(
            live.contains("nix (Determinate Nix "),
            "Live Nix is not Determinate"
        );
        println!("{live}");
        result["live"] = json!(live);
        let live_kernel = vm.execute("uname -r", SHORT)?.trim().to_owned();
        result["live_kernel"] = json!(live_kernel);
        result["media_config_permissions"] = json!(vm.execute(
            "stat -c '%u:%g %a %n' /nix/store; stat -Lc '%u:%g %a %n' /etc/calamares-nixos/settings.json",
            SHORT,
        )?);
        pause(Duration::from_secs(5))?;
        vm.screenshot(&artifacts.join("live-desktop.png"))?;
        if install {
            vm.execute("mkdir -p /workspace; mount -t 9p -o trans=virtio,version=9p2000.L project /workspace", SHORT)?;
            let fixture = fixture.as_deref().unwrap();
            println!("Running the pinned native fixture against the packaged Rust installer");
            let settings = vm.execute(
                &format!("{} prepare-integrated", shell_quote(fixture)),
                SHORT,
            )?;
            fs::write(artifacts.join("fixture-setup.log"), settings)?;
            // GUI mode is deliberately interactive: the QMP input script drives
            // the real form. No fixture request is sent to the backend.
            if gui {
                println!(
                    "GUI ready in {name}. Use tests/qmp_input.rs to complete the installation form. Waiting up to 10 minutes for the native helper."
                );
                vm.execute("for i in $(seq 1 600); do pgrep -u 0 -f '^/[^ ]+/bin/[.]?calamares-nixos-helper(-wrapped)? (install|session)( |$)' && exit 0; sleep 1; done; exit 1", Duration::from_secs(620))?;
                fs::write(
                    artifacts.join("install.log"),
                    "GUI_INSTALL_HELPER_STARTED\n",
                )?;
                // Application preparation and installation each have their own
                // two-hour deadline. Allow both phases to finish under TCG.
                vm.execute("while pgrep -u 0 -f '^/[^ ]+/bin/[.]?calamares-nixos-helper(-wrapped)? (install|session)( |$)' >/dev/null; do sleep 2; done", Duration::from_secs(15000))?;
                pause(Duration::from_secs(3))?;
                vm.screenshot(&artifacts.join("gui-finished.png"))?;
                // Exit alone is not a pass. Boot/authentication decide below.
                fs::write(
                    artifacts.join("install.log"),
                    "GUI_INSTALL_HELPER_STARTED\nGUI_INSTALL_HELPER_EXITED\n",
                )?;
            } else {
                // The fixture is shared from the host, never shipped on the ISO.
                vm.execute(
                    &format!(
                        "{}CALAMARES_TEST_FILESYSTEM={} CALAMARES_TEST_PREVIOUS_FILESYSTEM={} CALAMARES_TEST_APPLICATIONS={} {} install > {} 2>&1",
                        fixture_options()?, shell_quote(&root_filesystem), shell_quote(&previous_filesystem), shell_quote(&applications), shell_quote(fixture),
                        shell_quote(&format!("/workspace/artifacts/{name}/install.log"))
                    ),
                    Duration::from_secs(15000),
                )?;
            }
            if let Ok(log) = fs::read_to_string(artifacts.join("install.log")) {
                result["timing"] = timing(&log);
                println!("Timing: {}", result["timing"]);
            }
            let live_wifi = vm.execute("nmcli --get-values connection.permissions connection show uuid 135ea3d9-d456-44b1-ae42-1e7081f66666", SHORT)?;
            ensure!(
                live_wifi.contains("nixos") && !live_wifi.contains("rusttest"),
                "Wi-Fi migration modified the live connection instead of a copy"
            );
            result["live_wifi_unchanged"] = json!(true);
            result["backend_completed"] = json!(true);
            vm.poweroff()?;
            fs::copy(work.join("serial.log"), artifacts.join("live-serial.log"))?;
            vm = Vm::start_with_devices(
                &work,
                &firmware,
                None,
                Some(&disk),
                Some(&repo),
                InputDevices::Usb,
                disk_bus,
            )?;
            println!("Booting the installed disk with no ISO attached");
            let installed = verify_installed(
                &mut vm,
                &artifacts,
                &repo,
                fixture,
                gui,
                &root_filesystem,
                &applications,
            )?;
            let installed_kernel = vm.execute("uname -r", SHORT)?.trim().to_owned();
            ensure!(
                installed_kernel == live_kernel,
                "Installed kernel {installed_kernel} differs from selected live kernel {live_kernel}"
            );
            result["installed_kernel"] = json!(installed_kernel);
            println!("{installed}");
            result["installed"] = json!(installed);
            result["graphical_login"] = json!(gui);
            vm.poweroff()?;
        }
        Ok(())
    })();
    record(vm, &work, &artifacts, &mut result, outcome)
}

/// Run one shell command in a running test guest through its guest agent,
/// printing its output and exiting with its status.
pub fn guest_exec(args: Vec<String>) -> Result<()> {
    ensure!(args.len() == 2, "Usage: guest-exec RUN-NAME COMMAND");
    let name = &args[0];
    ensure!(
        Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name)
            && ![".", ".."].contains(&name.as_str()),
        "Supply a run name, not a path"
    );
    let mut agent = Rpc::connect(
        &repo()?.join(".work").join(name).join("qga.sock"),
        false,
        20,
    )?;
    agent.call("guest-ping", json!({}))?;
    let pid = agent.call(
        "guest-exec",
        json!({"path":"/run/current-system/sw/bin/bash","arg":["-lc", &args[1]],"capture-output":true}),
    )?["pid"]
        .clone();
    loop {
        let status = agent.call("guest-exec-status", json!({"pid":pid}))?;
        if status["exited"] == true {
            for key in ["out-data", "err-data"] {
                print!(
                    "{}",
                    String::from_utf8_lossy(&STANDARD.decode(status[key].as_str().unwrap_or(""))?)
                );
            }
            let code = status["exitcode"].as_i64().unwrap_or(1);
            ensure!(code == 0, "Guest command exited with status {code}");
            return Ok(());
        }
        pause(Duration::from_millis(500))?;
    }
}

pub fn boot_installed(args: Vec<String>) -> Result<()> {
    ensure!(args.len() == 1, "Usage: boot_installed.rs RUN-NAME");
    let name = &args[0];
    ensure!(
        Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name)
            && ![".", ".."].contains(&name.as_str()),
        "Supply a run name, not a path"
    );
    let repo = repo()?;
    let previous = repo.join("artifacts").join(name);
    let previous_work = repo.join(".work").join(name);
    let mut result: Value = serde_json::from_slice(&fs::read(previous.join("result.json"))?)?;
    ensure!(
        result["install"] == true
            && result["backend_completed"] == true
            && fs::read_to_string(previous.join("install.log"))?
                .lines()
                .any(|line| line == r#"{"kind":"complete"}"#
                    || (result["gui"] == true && line == "GUI_INSTALL_HELPER_EXITED")),
        "No successful Calamares installation"
    );
    let firmware = result["firmware"]
        .as_str()
        .context("Missing firmware")?
        .to_string();
    ensure!(
        ["bios", "uefi"].contains(&firmware.as_str())
            && result["live"].is_string()
            && result["iso_sha256"].is_string(),
        "Incomplete original evidence"
    );
    let disk_bus = DiskBus::parse(result["disk_bus"].as_str().unwrap_or("virtio"))?;
    let root_filesystem = filesystem(result["filesystem"].as_str().unwrap_or("ext4"), false)?;
    let disk = regular(&previous_work.join("target.raw"))?;
    ensure!(
        [40 * 1024u64.pow(3), 80 * 1024u64.pow(3)].contains(&fs::metadata(&disk)?.len()),
        "Wrong test disk size"
    );
    // Keep AF_UNIX socket paths short; lineage is recorded in resumed_from.
    let name = format!("boot-{firmware}-{}", stamp());
    let work = repo.join(".work").join(&name);
    let artifacts = repo.join("artifacts").join(&name);
    fs::create_dir(&work)?;
    fs::create_dir(&artifacts)?;
    if firmware == "uefi" {
        fs::copy(
            previous_work.join("OVMF_VARS.fd"),
            work.join("OVMF_VARS.fd"),
        )?;
    }
    for file in ["live-desktop.png", "install.log"] {
        fs::copy(previous.join(file), artifacts.join(file))?;
    }
    result["passed"] = json!(false);
    if let Some(error) = result
        .as_object_mut()
        .context("Invalid original result")?
        .remove("error")
    {
        result["prior_error"] = error;
    }
    result["resumed_from"] = json!(args[0]);
    result["disk_only_boot"] = json!(true);
    result["qemu_acceleration"] = json!(acceleration()?);
    result["implementation"] = json!("native-rust");
    let fixture = fixture(&repo)?;
    let mut vm = Vm::start_with_devices(
        &work,
        &firmware,
        None,
        Some(&disk),
        Some(&repo),
        InputDevices::Usb,
        disk_bus,
    )?;
    println!("Booting installed {firmware} disk; no ISO attached");
    let gui = result["gui"] == true;
    let applications = result["applications"]
        .as_str()
        .unwrap_or("firefox")
        .to_owned();
    let outcome = verify_installed(
        &mut vm,
        &artifacts,
        &repo,
        &fixture,
        gui,
        &root_filesystem,
        &applications,
    )
    .and_then(|text| {
        println!("{text}");
        result["installed"] = json!(text);
        vm.poweroff()
    });
    record(vm, &work, &artifacts, &mut result, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_collects_helper_stages_and_fixture_markers() {
        let log = "{\"kind\":\"timing\",\"stage\":\"copy\",\"seconds\":12.5}\n{\"kind\":\"prepared\",\"summary\":{\"closure_bytes\":7}}\nPREPARED_SECONDS=40.0\nCLICK_TO_COMPLETE_SECONDS=31.2\nnoise\n";
        let value = timing(log);
        assert_eq!(value["stages"]["copy"], 12.5);
        assert_eq!(value["prepared"]["closure_bytes"], 7);
        assert_eq!(value["click_to_complete_seconds"], 31.2);
        assert_eq!(value["prepared_seconds"], 40.0);
    }
    #[test]
    fn storage_options_are_closed_and_nvme_sector_size_is_explicit() {
        assert!(DiskBus::parse("sata").is_err());
        assert!(filesystem("ntfs", false).is_err());
        assert!(filesystem("blank", false).is_err());
        assert!(filesystem("blank", true).is_ok());
        assert!(
            DiskBus::Nvme4k
                .device()
                .contains("logical_block_size=4096,physical_block_size=4096")
        );
        assert!(DiskBus::Nvme.device().starts_with("nvme,"));
        for bus in [DiskBus::Virtio, DiskBus::Nvme, DiskBus::Nvme4k] {
            assert_eq!(DiskBus::parse(bus.name()).unwrap(), bus);
            assert!(bus.device().contains("serial=RESPIN_TEST_ONLY"));
        }
    }
    #[test]
    fn shutdown_uses_process_status_without_rpc_reply() {
        for success in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("rpc");
            let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            let server = std::thread::spawn(move || {
                let (socket, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(socket).read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["execute"], "guest-shutdown");
                assert_eq!(request["arguments"]["mode"], "powerdown");
                // An agent shuts down without replying; ordinary RPC EOF
                // remains an error, as checked separately below.
            });
            let mut vm = Vm {
                work: temp.path().to_path_buf(),
                process: Some(Process(
                    Command::new(if success { "true" } else { "false" })
                        .spawn()
                        .unwrap(),
                )),
                agent: Some(Rpc::connect(&path, false, 1).unwrap()),
            };
            assert_eq!(vm.poweroff().is_ok(), success);
            server.join().unwrap();
        }
    }
    #[test]
    fn rpc_skips_events_and_old_ids() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rpc");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            writeln!(socket, "{}", json!({"event":"READY"})).unwrap();
            writeln!(socket, "{}", json!({"id":0,"return":"stale"})).unwrap();
            writeln!(socket, "{}", json!({"id":request["id"],"return":"ok"})).unwrap();
        });
        assert_eq!(
            Rpc::connect(&path, false, 1)
                .unwrap()
                .call("ping", json!({}))
                .unwrap(),
            "ok"
        );
        server.join().unwrap();
    }
    #[test]
    fn rpc_eof_is_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rpc");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
        });
        assert!(
            Rpc::connect(&path, false, 1)
                .unwrap()
                .call("ping", json!({}))
                .is_err()
        );
        server.join().unwrap();
    }
}
