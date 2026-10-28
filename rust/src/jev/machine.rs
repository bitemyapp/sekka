use super::*;
use crate::vm::Rpc;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub iso: PathBuf,
    pub iso_sha256: String,
    pub source_revision: String,
    pub lock_sha256: String,
    pub sidestep_revision: String,
    pub qemu: PathBuf,
    pub firmware: String,
    pub ovmf_code: Option<PathBuf>,
    pub ovmf_vars: Option<PathBuf>,
    pub media: String,
    pub disk_bus: String,
    pub disk_gib: u64,
    pub ram_mib: u64,
    pub cpus: u64,
}

fn safe_path(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    ensure!(
        fs::metadata(&path)?.is_file(),
        "Only regular files are supported"
    );
    ensure!(
        !path.to_string_lossy().contains([',', '\n', '\r']),
        "Path contains QEMU option delimiters"
    );
    Ok(path)
}

fn config(mut c: Config) -> Result<Config> {
    ensure!(
        ["bios", "uefi"].contains(&c.firmware.as_str()),
        "Invalid firmware"
    );
    ensure!(
        ["usb", "optical"].contains(&c.media.as_str()),
        "Invalid media"
    );
    ensure!(
        ["virtio", "nvme", "nvme4k"].contains(&c.disk_bus.as_str()),
        "Invalid disk bus"
    );
    ensure!(
        (20..=200).contains(&c.disk_gib)
            && (2048..=16384).contains(&c.ram_mib)
            && (1..=8).contains(&c.cpus),
        "VM resources outside experiment limits"
    );
    for (label, s, n) in [
        ("ISO SHA256", &c.iso_sha256, 64),
        ("lock SHA256", &c.lock_sha256, 64),
        ("source revision", &c.source_revision, 40),
        ("Sidestep revision", &c.sidestep_revision, 40),
    ] {
        ensure!(
            s.len() == n && s.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid {label}"
        );
    }
    c.iso = safe_path(&c.iso)?;
    c.qemu = safe_path(&c.qemu)?;
    if c.firmware == "uefi" {
        c.ovmf_code = Some(safe_path(
            c.ovmf_code.as_deref().context("UEFI needs ovmf_code")?,
        )?);
        c.ovmf_vars = Some(safe_path(
            c.ovmf_vars.as_deref().context("UEFI needs ovmf_vars")?,
        )?);
    }
    Ok(c)
}

pub fn lock(run: &Path) -> Result<fs::File> {
    use std::os::fd::AsRawFd;
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(run.join("driver.lock"))?;
    // The lock lives for the complete scenario, including API calls and waits.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    ensure!(result == 0, "Another driver owns this VM");
    Ok(file)
}

pub fn start(run: &Path, path: &Path) -> Result<()> {
    let c = config(serde_json::from_value(read(path)?)?)?;
    ensure!(
        hash(&c.iso)? == c.iso_sha256,
        "ISO SHA256 mismatch; VM not started"
    );
    let run = private_dir(run)?;
    let _lock = lock(&run)?;
    let disk = run.join("target.raw");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&disk)?;
    file.set_len(c.disk_gib * 1024u64.pow(3))?;
    let metadata = file.metadata()?;
    if c.firmware == "uefi" {
        fs::copy(c.ovmf_vars.as_ref().unwrap(), run.join("OVMF_VARS.fd"))?;
        fs::set_permissions(run.join("OVMF_VARS.fd"), fs::Permissions::from_mode(0o600))?;
    }
    let sockets = tempfile::Builder::new()
        .prefix("sekka-jev-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?
        .keep();
    let version = Command::new(&c.qemu).arg("--version").output()?;
    save(
        &run.join("manifest.json"),
        &json!({"schema":1,"kind":"sekka-jev-disposable-vm","created_ms":now(),"config":c,
        "iso_bytes":fs::metadata(&c.iso)?.len(),"disk_inode":metadata.ino(),"disk_device":metadata.dev(),"sockets":sockets,
        "harness_sha256":hash(&std::env::current_exe()?)?,"qemu_version":String::from_utf8_lossy(&version.stdout),
        "host":fs::read_to_string("/proc/sys/kernel/hostname")?.trim(),"host_meminfo":fs::read_to_string("/proc/meminfo")?,
        "qualification":"experimental; full checklist and independent evidence review required","publication_enabled":false}),
    )?;
    launch(&run, &c, 0, true)?;
    println!("{}", run.display());
    Ok(())
}

fn manifest(run: &Path) -> Result<Value> {
    let m = read(&run.join("manifest.json"))?;
    ensure!(
        m["schema"] == 1 && m["kind"] == "sekka-jev-disposable-vm",
        "Not a Jev disposable VM"
    );
    let disk = run.join("target.raw");
    let md = fs::symlink_metadata(&disk)?;
    ensure!(
        md.is_file()
            && md.nlink() == 1
            && Some(md.ino()) == m["disk_inode"].as_u64()
            && Some(md.dev()) == m["disk_device"].as_u64(),
        "Disposable disk identity changed"
    );
    Ok(m)
}

fn active(run: &Path) -> Result<Value> {
    manifest(run)?;
    read(&run.join("active.json"))
}

fn process_start(pid: u64) -> Result<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    Ok(stat
        .rsplit_once(") ")
        .context("proc stat")?
        .1
        .split_whitespace()
        .nth(19)
        .context("process start time")?
        .into())
}

fn alive(a: &Value) -> Result<bool> {
    let pid = a["pid"].as_u64().context("Missing PID")?;
    for _ in 0..4 {
        let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        let fields: Vec<_> = stat
            .rsplit_once(") ")
            .context("proc stat")?
            .1
            .split_whitespace()
            .collect();
        ensure!(
            fields.get(19).copied() == a["process_start"].as_str(),
            "PID reused; refusing to control another process"
        );
        if fields.first().is_some_and(|s| ["Z", "X"].contains(s)) {
            return Ok(false);
        }
        let cmdline = match fs::read(format!("/proc/{pid}/cmdline")) {
            Ok(cmdline) => cmdline,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        // exit_mm can empty cmdline before the kernel exposes the zombie state.
        if cmdline.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        ensure!(
            format!("{:x}", Sha256::digest(cmdline)) == a["cmdline_sha256"].as_str().unwrap_or(""),
            "QEMU process identity changed"
        );
        return Ok(true);
    }
    bail!("Process identity unavailable during exit; no control attempted")
}

fn launch(run: &Path, c: &Config, phase: u64, media: bool) -> Result<()> {
    let m = manifest(run)?;
    let socket = Path::new(m["sockets"].as_str().context("socket directory")?)
        .join(format!("qmp-{phase}.sock"));
    ensure!(socket.as_os_str().len() < 104, "QMP socket path too long");
    let pidfile = run.join(format!("qemu-{phase}.pid"));
    let args = arguments(run, c, phase, media, &socket, &pidfile);
    save(
        &run.join(format!("launch-{phase}.json")),
        &json!({"time_ms":now(),"program":c.qemu,"argv":args,"media_attached":media,"phase":phase}),
    )?;
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(run.join(format!("qemu-{phase}.log")))?;
    let result = Command::new(&c.qemu)
        .args(&args)
        .env_remove("JEV_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .status()?;
    ensure!(result.success(), "QEMU failed; inspect qemu-{phase}.log");
    let pid: u64 = fs::read_to_string(&pidfile)?.trim().parse()?;
    let a = json!({"pid":pid,"process_start":process_start(pid)?,"cmdline_sha256":format!("{:x}",Sha256::digest(fs::read(format!("/proc/{pid}/cmdline"))?)),"socket":socket,"phase":phase,"media_attached":media});
    let tmp = run.join(format!("active-{phase}.json"));
    save(&tmp, &a)?;
    fs::rename(tmp, run.join("active.json"))?;
    event(run, json!({"kind":"vm-start","active":a}))?;
    Ok(())
}

fn arguments(
    run: &Path,
    c: &Config,
    phase: u64,
    media: bool,
    socket: &Path,
    pidfile: &Path,
) -> Vec<String> {
    let mut args = vec![
        "-name".into(),
        format!("sekka-jev-{}", run.file_name().unwrap().to_string_lossy()),
        "-machine".into(),
        "q35,vmport=off".into(),
        "-accel".into(),
        "kvm".into(),
        "-cpu".into(),
        "host".into(),
        "-smp".into(),
        c.cpus.to_string(),
        "-m".into(),
        c.ram_mib.to_string(),
        "-display".into(),
        "none".into(),
        "-vga".into(),
        "none".into(),
        "-device".into(),
        "virtio-gpu-pci,xres=1280,yres=800".into(),
        "-serial".into(),
        format!("file:{}/serial-{phase}.log", run.display()),
        "-qmp".into(),
        format!("unix:{},server=on,wait=off", socket.display()),
        "-netdev".into(),
        "user,id=net0".into(),
        "-device".into(),
        "virtio-net-pci,netdev=net0".into(),
        "-device".into(),
        "qemu-xhci,id=xhci".into(),
        "-device".into(),
        "usb-kbd,bus=xhci.0".into(),
        "-device".into(),
        "usb-tablet,bus=xhci.0".into(),
        "-drive".into(),
        format!(
            "if=none,id=target,format=raw,file={}/target.raw",
            run.display()
        ),
        "-device".into(),
    ];
    args.push(match c.disk_bus.as_str() {
        "virtio"=>"virtio-blk-pci,drive=target,serial=SEKKA_JEV_TEST,bootindex=2",
        "nvme4k"=>"nvme,drive=target,serial=SEKKA_JEV_TEST,bootindex=2,logical_block_size=4096,physical_block_size=4096",
        _=>"nvme,drive=target,serial=SEKKA_JEV_TEST,bootindex=2"
    }.into());
    if c.firmware == "uefi" {
        args.extend([
            "-drive".into(),
            format!(
                "if=pflash,format=raw,readonly=on,file={}",
                c.ovmf_code.as_ref().unwrap().display()
            ),
            "-drive".into(),
            format!("if=pflash,format=raw,file={}/OVMF_VARS.fd", run.display()),
        ]);
    } else {
        args.extend([
            "-fw_cfg".into(),
            "name=opt/org.seabios/pci64,string=0".into(),
        ]);
    }
    if media {
        if c.media == "usb" {
            args.extend([
                "-drive".into(),
                format!(
                    "if=none,id=iso,format=raw,readonly=on,file={}",
                    c.iso.display()
                ),
                "-device".into(),
                "usb-storage,drive=iso,bootindex=1".into(),
            ]);
        } else {
            args.extend([
                "-drive".into(),
                format!(
                    "media=cdrom,format=raw,readonly=on,file={}",
                    c.iso.display()
                ),
            ]);
        }
    }
    args.extend([
        "-pidfile".into(),
        pidfile.display().to_string(),
        "-daemonize".into(),
    ]);
    args
}

pub fn connect(run: &Path) -> Result<Rpc> {
    let a = active(run)?;
    ensure!(alive(&a)?, "VM is not running");
    Rpc::connect(Path::new(a["socket"].as_str().context("socket")?), true, 10)
}

pub fn capture(run: &Path) -> Result<PathBuf> {
    let path = run.join(format!("screen-{}-{}.png", now(), std::process::id()));
    connect(run)?.screenshot(&path)?;
    Ok(path)
}

pub fn status(run: &Path) -> Result<Value> {
    let a = active(run)?;
    if !alive(&a)? {
        return Ok(json!({"running":false,"phase":a["phase"]}));
    }
    Ok(json!({"running":true,"active":a,"qmp":connect(run)?.call("query-status",json!({}))?}))
}

pub fn stopped(run: &Path) -> Result<bool> {
    Ok(!alive(&active(run)?)?)
}

pub fn boot_disk(run: &Path) -> Result<()> {
    let _lock = lock(run)?;
    let a = active(run)?;
    ensure!(
        !alive(&a)?,
        "Shut down and confirm QEMU exit before disk-only boot"
    );
    let c: Config = serde_json::from_value(manifest(run)?["config"].clone())?;
    launch(run, &c, a["phase"].as_u64().context("phase")? + 1, false)
}

pub fn shutdown(run: &Path) -> Result<()> {
    let _lock = lock(run)?;
    let a = active(run)?;
    if !alive(&a)? {
        return Ok(());
    }
    let screenshot = capture(run)?;
    event(
        run,
        json!({"kind":"shutdown-request","screenshot":screenshot,"method":"QMP ACPI system_powerdown; not UI coverage"}),
    )?;
    connect(run)?.call("system_powerdown", json!({}))?;
    let deadline = Instant::now() + Duration::from_secs(45);
    while alive(&a)? {
        ensure!(
            Instant::now() < deadline,
            "Guest did not shut down; VM retained for diagnosis"
        );
        crate::support::pause(Duration::from_millis(500))?;
    }
    event(run, json!({"kind":"shutdown-exited","phase":a["phase"]}))
}

pub fn click(run: &Path, x: u32, y: u32, w: u32, h: u32) -> Result<()> {
    ensure!(
        w > 1 && h > 1 && x < w && y < h,
        "Click outside captured screen"
    );
    let mut qmp = connect(run)?;
    qmp.call("input-send-event",json!({"events":[{"type":"abs","data":{"axis":"x","value":x*32767/(w-1)}},{"type":"abs","data":{"axis":"y","value":y*32767/(h-1)}},{"type":"btn","data":{"down":true,"button":"left"}}]}))?;
    qmp.call(
        "input-send-event",
        json!({"events":[{"type":"btn","data":{"down":false,"button":"left"}}]}),
    )?;
    Ok(())
}

/// Explicit operator assistance. Never labeled as a model-controlled action.
pub fn manual(run: &Path, args: &[String]) -> Result<()> {
    let run = run.canonicalize()?;
    let _lock = lock(&run)?;
    let before = capture(&run)?;
    event(
        &run,
        json!({"kind":"operator-assisted-input-intent","args":args,"before":before,"qualification":"diagnostic or assisted input; not autonomous Jev evidence"}),
    )?;
    match (args[0].as_str(), args.len()) {
        ("key", 2) => {
            crate::gui_input::key(&mut connect(&run)?, &args[1].split('+').collect::<Vec<_>>())?
        }
        ("type", 2) => crate::gui_input::type_text(&mut connect(&run)?, &args[1])?,
        ("click", 3) => {
            let (w, h) = super::api::dimensions(&before)?;
            click(&run, args[1].parse()?, args[2].parse()?, w, h)?;
        }
        _ => bail!("Invalid manual input operation"),
    }
    crate::support::pause(Duration::from_millis(500))?;
    let after = capture(&run)?;
    event(
        &run,
        json!({"kind":"operator-assisted-input-completed","after":after}),
    )?;
    println!("{}", after.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unreaped_exit_is_stopped_but_pid_reuse_is_rejected() {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id() as u64;
        let start = process_start(pid).unwrap();
        // Leave the child unreaped, reproducing the observed QEMU exit race.
        // SAFETY: waitid writes to an initialized siginfo_t for our own child;
        // WNOWAIT waits for exit without reaping the process under test.
        let result = unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        assert_eq!(result, 0);
        let a = json!({"pid":pid,"process_start":start,"cmdline_sha256":"previous-live-command"});
        let stopped = alive(&a);
        let mut wrong = a;
        wrong["process_start"] = json!("not-the-original-process");
        let reused = alive(&wrong);
        child.wait().unwrap();
        assert!(!stopped.unwrap());
        assert!(reused.is_err());
    }
    #[test]
    fn disk_boot_has_no_media_or_share() {
        let c = Config {
            iso: "/tmp/example.iso".into(),
            iso_sha256: "a".repeat(64),
            source_revision: "b".repeat(40),
            lock_sha256: "c".repeat(64),
            sidestep_revision: "d".repeat(40),
            qemu: "/bin/qemu".into(),
            firmware: "bios".into(),
            ovmf_code: None,
            ovmf_vars: None,
            media: "usb".into(),
            disk_bus: "nvme".into(),
            disk_gib: 100,
            ram_mib: 8192,
            cpus: 4,
        };
        let disk = arguments(
            Path::new("/tmp/vm"),
            &c,
            1,
            false,
            Path::new("/tmp/sock"),
            Path::new("/tmp/pid"),
        );
        assert!(!disk.iter().any(|s| s.contains("example.iso")
            || s.contains("9p")
            || s.contains("virtfs")
            || s.contains("hostfwd")));
        let live = arguments(
            Path::new("/tmp/vm"),
            &c,
            0,
            true,
            Path::new("/tmp/sock"),
            Path::new("/tmp/pid"),
        );
        assert!(
            live.iter()
                .any(|s| s == "usb-storage,drive=iso,bootindex=1")
        );
        assert!(
            live.iter()
                .any(|s| s.contains("readonly=on,file=/tmp/example.iso"))
        );
    }
    #[test]
    fn host_devices_and_symlink_replacements_rejected() {
        assert!(safe_path(Path::new("/dev/null")).is_err());
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("target.raw"), b"disk").unwrap();
        let md = fs::metadata(dir.path().join("target.raw")).unwrap();
        save(&dir.path().join("manifest.json"),&json!({"schema":1,"kind":"sekka-jev-disposable-vm","disk_inode":md.ino(),"disk_device":md.dev()})).unwrap();
        assert!(manifest(dir.path()).is_ok());
        fs::remove_file(dir.path().join("target.raw")).unwrap();
        std::os::unix::fs::symlink("/dev/null", dir.path().join("target.raw")).unwrap();
        assert!(manifest(dir.path()).is_err());
    }
}
