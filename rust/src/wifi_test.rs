//! Real NetworkManager scan/authentication over simulated radios, never host RF.
use crate::{
    gui_input,
    live_test::Profile,
    support::*,
    vm::{Rpc, Vm},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const GUEST_TOOL: &str =
    "/workspace/.work/guest-target/x86_64-unknown-linux-musl/release/respin-tools";
const SSID: &str = "RESPIN_TEST_ONLY";
const PASSWORD: &str = "Public-VM-Wifi-Test-123";

pub fn main(args: Vec<String>) -> Result<()> {
    let mut iso = None;
    let mut profile = Profile::Default;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => profile = Profile::parse(&args.next().context("Missing profile")?)?,
            value if !value.starts_with('-') && iso.is_none() => iso = Some(PathBuf::from(value)),
            _ => bail!("Unknown argument: {arg}"),
        }
    }
    let repo = repo()?;
    let iso = regular(&iso.context("Supply an ISO")?)?;
    let manifest: Value =
        serde_json::from_slice(&fs::read(repo.join("artifacts/wifi-tools/manifest.json"))?)?;
    ensure!(
        manifest["sha256"] == sha256(&repo.join("artifacts/wifi-tools/closure.nar"))?,
        "Test tools archive changed"
    );
    ensure!(
        manifest["lock_sha256"] == sha256(&repo.join("flake.lock"))?,
        "Test tools use a different system lock"
    );
    let store_path = manifest["store_path"]
        .as_str()
        .context("Missing test tools path")?;
    ensure!(
        store_path.starts_with("/nix/store/") && !store_path.contains(char::is_whitespace),
        "Invalid tools path"
    );
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
    let id = stamp();
    let work = repo.join(".work").join(format!("wf-{id}"));
    let artifacts = repo
        .join("artifacts")
        .join(format!("wifi-{profile:?}-{id}"));
    fs::create_dir_all(&artifacts)?;
    let mut result = json!({"passed":false,"iso_sha256":sha256(&iso)?,"profile":format!("{profile:?}"),
        "radio":"mac80211_hwsim","host_radio_attached":false,"target_disk":false,"tools":manifest});
    let mut vm = Vm::start(&work, "uefi", Some(&iso), None, Some(&repo))?;
    let outcome = (|| -> Result<()> {
        println!("Booting {profile:?} Wi-Fi test; no physical radio or target disk attached");
        let end = Instant::now() + Duration::from_secs(90);
        while !fs::read_to_string(work.join("serial.log"))
            .unwrap_or_default()
            .contains("Loading graphical boot menu")
        {
            ensure!(Instant::now() < end, "Boot menu did not appear");
            pause(Duration::from_millis(200))?;
        }
        pause(Duration::from_secs(2))?;
        let mut qmp = Rpc::connect(&work.join("qmp.sock"), true, 10)?;
        gui_input::key(&mut qmp, &["down"])?;
        gui_input::key(&mut qmp, &["home"])?;
        for _ in 0..profile.index() {
            gui_input::key(&mut qmp, &["down"])?;
        }
        gui_input::key(&mut qmp, &["ret"])?;
        drop(qmp);
        vm.wait_agent()?;
        let settings: Value = serde_json::from_str(&vm.execute(
            "cat /etc/calamares-nixos/settings.json",
            Duration::from_secs(15),
        )?)?;
        ensure!(settings["kernel"] == profile.kernel(), "Wrong boot profile");
        let desktop = if profile == Profile::Compatibility {
            "xfce4-session"
        } else {
            "plasmashell"
        };
        vm.execute(&format!("for i in $(seq 1 90); do pgrep -u 1000 -f '(^|/)[.]?{desktop}(-wrapped)?( |$)' && exit 0; sleep 1; done; exit 1"), Duration::from_secs(100))?;
        result["desktop"] = json!(desktop);
        result["kernel"] = json!(vm.execute("uname -r", Duration::from_secs(15))?.trim());
        vm.execute("mkdir -p /workspace; mount -t 9p -o ro,trans=virtio,version=9p2000.L project /workspace", Duration::from_secs(20))?;
        // The tools are imported only into this guest's temporary store. The
        // public ISO neither contains nor starts an AP/test helper.
        vm.execute(
            "nix-store --import < /workspace/artifacts/wifi-tools/closure.nar",
            Duration::from_secs(120),
        )?;
        let command = format!(
            "PATH={}:{}:/run/current-system/sw/bin {} guest-wifi-test",
            shell_quote(&format!("{store_path}/bin")),
            shell_quote(&format!("{store_path}/sbin")),
            shell_quote(GUEST_TOOL)
        );
        let output = vm.execute(&command, Duration::from_secs(300));
        // Retain diagnostic output for both regressions and successful tests.
        let log = vm.execute("cat /run/respin-wifi-test/report.log /run/respin-wifi-test/ap.log 2>/dev/null; journalctl -b -u NetworkManager -u wpa_supplicant --no-pager -n 60; true", Duration::from_secs(20))?;
        fs::write(artifacts.join("wifi.log"), log)?;
        let output = output?;
        ensure!(
            output.contains("WIFI_SCAN_CONNECT_RECONNECT_PASS"),
            "Guest did not complete its Wi-Fi assertions"
        );
        result["wifi"] = json!(output);
        vm.screenshot(&artifacts.join("desktop.png"))?;
        vm.poweroff()?;
        result["clean_shutdown"] = json!(true);
        Ok(())
    })();
    result["passed"] = json!(outcome.is_ok());
    if let Err(error) = &outcome {
        result["error"] = json!(format!("{error:#}"));
    }
    vm.stop();
    for file in ["serial.log", "qemu.log"] {
        optional_copy(&work.join(file), &artifacts.join(file))?;
    }
    write_json(&artifacts.join("result.json"), &result)?;
    outcome?;
    println!("PASS: {}", artifacts.display());
    Ok(())
}

fn capture(program: &str, args: &[&str]) -> Result<String> {
    output(Command::new("timeout").args(["40", program]).args(args))
}

fn wireless_interfaces() -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir("/sys/class/net")? {
        let entry = entry?;
        if entry.path().join("phy80211").exists() {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Non-UTF8 interface"))?;
            ensure!(
                name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "Unexpected interface name"
            );
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

pub fn guest() -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "Guest test requires guest root"
    );
    ensure!(
        fs::read_to_string("/sys/class/dmi/id/product_serial")?.trim() == "RESPIN_VM_ONLY",
        "Not our disposable VM"
    );
    ensure!(
        ["kvm", "qemu"].contains(&capture("systemd-detect-virt", &["--vm"])?.trim()),
        "Not a QEMU guest"
    );
    ensure!(
        capture("findmnt", &["-n", "-o", "FSTYPE", "/"])?.trim() == "tmpfs",
        "Not a disposable live root"
    );
    ensure!(
        wireless_interfaces()?.is_empty(),
        "Guest already has a radio; refusing to touch it"
    );
    let dir = Path::new("/run/respin-wifi-test");
    fs::create_dir(dir)?;
    let mut report = File::create(dir.join("report.log"))?;
    use std::io::Write;
    writeln!(report, "QEMU guest, temporary root, no pre-existing radios")?;
    let backend = capture(
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
    );
    if let Err(error) = &backend {
        writeln!(report, "SUPPLICANT_ACTIVATION_FAILED: {error}")?;
    }
    backend?;
    capture("systemctl", &["is-active", "wpa_supplicant"])?;
    capture("modprobe", &["mac80211_hwsim", "radios=2"])?;
    let names = wireless_interfaces()?;
    ensure!(
        names.len() == 2,
        "Expected exactly two simulated interfaces"
    );
    let station = &names[0];
    let ap = &names[1];
    for name in &names {
        let path = fs::canonicalize(format!("/sys/class/net/{name}/device"))?;
        ensure!(
            path.to_string_lossy().contains("mac80211_hwsim"),
            "Not a simulated radio"
        );
    }
    let phy_path = fs::canonicalize(format!("/sys/class/net/{ap}/phy80211"))?;
    let phy = phy_path
        .file_name()
        .and_then(|s| s.to_str())
        .context("No AP phy")?;
    capture("ip", &["netns", "add", "respin-ap"])?;
    let outcome = (|| -> Result<()> {
        capture("iw", &["phy", phy, "set", "netns", "name", "respin-ap"])?;
        capture("ip", &["-n", "respin-ap", "link", "set", "lo", "up"])?;
        capture(
            "ip",
            &[
                "-n",
                "respin-ap",
                "address",
                "add",
                "192.0.2.1/24",
                "dev",
                ap,
            ],
        )?;
        let config = dir.join("hostapd.conf");
        fs::write(
            &config,
            format!(
                "interface={ap}\ndriver=nl80211\nssid={SSID}\nhw_mode=g\nchannel=1\nwpa=2\nwpa_key_mgmt=WPA-PSK\nrsn_pairwise=CCMP\nwpa_passphrase={PASSWORD}\n"
            ),
        )?;
        let ap_log = File::create(dir.join("ap.log"))?;
        let mut process = Process(
            Command::new("ip")
                .args(["netns", "exec", "respin-ap", "hostapd"])
                .arg(&config)
                .stdin(Stdio::null())
                .stdout(ap_log.try_clone()?)
                .stderr(ap_log)
                .spawn()?,
        );
        let end = Instant::now() + Duration::from_secs(30);
        while !fs::read_to_string(dir.join("ap.log"))?.contains("AP-ENABLED") {
            ensure!(process.0.try_wait()?.is_none(), "AP exited");
            ensure!(Instant::now() < end, "AP did not start");
            pause(Duration::from_millis(200))?;
        }
        capture("nmcli", &["radio", "wifi", "on"])?;
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(scan) = capture(
                "nmcli",
                &[
                    "-w", "10", "-t", "-f", "SSID", "device", "wifi", "list", "ifname", station,
                    "--rescan", "yes",
                ],
            ) && scan.lines().any(|line| line == SSID)
            {
                writeln!(report, "SCAN_FOUND={SSID}")?;
                break;
            }
            ensure!(
                Instant::now() < end,
                "NetworkManager never discovered the test SSID"
            );
            pause(Duration::from_secs(2))?;
        }
        capture(
            "nmcli",
            &[
                "connection",
                "add",
                "type",
                "wifi",
                "ifname",
                station,
                "con-name",
                "respin-wifi",
                "ssid",
                SSID,
                "wifi-sec.key-mgmt",
                "wpa-psk",
                "wifi-sec.psk",
                PASSWORD,
                "ipv4.method",
                "manual",
                "ipv4.addresses",
                "192.0.2.2/24",
                "ipv4.never-default",
                "yes",
                "ipv6.method",
                "disabled",
                "connection.autoconnect",
                "no",
            ],
        )?;
        for attempt in 1..=2 {
            capture(
                "nmcli",
                &[
                    "-w",
                    "30",
                    "connection",
                    "up",
                    "id",
                    "respin-wifi",
                    "ifname",
                    station,
                ],
            )?;
            let link = capture("iw", &["dev", station, "link"])?;
            ensure!(
                link.contains(&format!("SSID: {SSID}")),
                "Station is not associated with the test AP"
            );
            let packets = capture("ping", &["-I", station, "-c", "3", "-W", "2", "192.0.2.1"])?;
            writeln!(report, "CONNECTION_ATTEMPT={attempt}\n{link}\n{packets}")?;
            capture(
                "nmcli",
                &["-w", "10", "connection", "down", "id", "respin-wifi"],
            )?;
        }
        ensure!(
            fs::read_to_string(dir.join("ap.log"))?.contains("AP-STA-CONNECTED"),
            "AP did not authenticate station"
        );
        writeln!(
            report,
            "SUPPLICANT_BEFORE_TEARDOWN={}",
            capture("systemctl", &["is-active", "wpa_supplicant"])?.trim()
        )?;
        drop(process);
        Ok(())
    })();
    if let Err(error) = &outcome {
        writeln!(report, "FAILED: {error:#}")?;
    }
    // Namespace/process cleanup remains bounded, including on failed assertions.
    let cleanup = capture("ip", &["netns", "delete", "respin-ap"]);
    outcome?;
    cleanup?;
    writeln!(
        report,
        "SUPPLICANT_AFTER_TEARDOWN\n{}",
        capture(
            "systemctl",
            &[
                "show",
                "wpa_supplicant",
                "-p",
                "ActiveState",
                "-p",
                "SubState",
                "-p",
                "NRestarts",
                "-p",
                "ExecMainStatus"
            ]
        )?
    )?;
    writeln!(report, "WIFI_SCAN_CONNECT_RECONNECT_PASS")?;
    println!("{}", fs::read_to_string(dir.join("report.log"))?);
    Ok(())
}
