// SPDX-License-Identifier: MIT OR Apache-2.0
//! What the window asks of NetworkManager and changes in it, through nmcli.
//! These calls block: the window runs them on worker threads. Reading never
//! waits for a Wi-Fi scan; a scan is requested and its results arrive with
//! the next reading. Passwords reach nmcli in a file only the user can read
//! (`passwd-file`), never on a command line.
//!
//! With TATAMI_NETWORK_DRY_RUN set, changes are printed, not made.
use crate::model::{self, Snapshot};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

fn dry_run() -> bool {
    std::env::var_os("TATAMI_NETWORK_DRY_RUN").is_some()
}

fn output(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("nmcli: {error}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// A change; the error is nmcli's message.
fn change(args: &[&str]) -> Result<(), String> {
    if dry_run() {
        println!("nmcli {}", args.join(" "));
        return Ok(());
    }
    output(args).map(drop)
}

/// Everything the window shows, in two rounds of questions asked at once:
/// devices, connections, the Wi-Fi switch and the networks in range, then
/// the saved networks' names and the devices' addresses.
pub fn snapshot() -> Snapshot {
    let ask = |args: &'static [&'static str]| move || output(args).unwrap_or_default();
    let (radio, devices, saved, list) = std::thread::scope(|scope| {
        let radio = scope.spawn(ask(&["-t", "-f", "WIFI-HW,WIFI", "radio"]));
        let devices = scope.spawn(ask(&[
            "-t",
            "-f",
            "DEVICE,TYPE,STATE,CONNECTION,CON-UUID",
            "device",
        ]));
        let saved = scope.spawn(ask(&[
            "-t",
            "-f",
            "NAME,UUID,TYPE,AUTOCONNECT,ACTIVE,DEVICE,STATE,TIMESTAMP",
            "connection",
            "show",
        ]));
        let list = scope.spawn(ask(&[
            "-t",
            "-f",
            "IN-USE,SSID,SECURITY,SIGNAL",
            "device",
            "wifi",
            "list",
            "--rescan",
            "no",
        ]));
        let join =
            |thread: std::thread::ScopedJoinHandle<String>| thread.join().unwrap_or_default();
        (join(radio), join(devices), join(saved), join(list))
    });
    let radio = model::fields(radio.trim());
    let mut saved = model::saved(&saved);
    let wifi: Vec<String> = saved
        .iter()
        .filter(|saved| saved.kind == model::Kind::Wifi)
        .map(|saved| saved.uuid.clone())
        .collect();
    let (settings, addresses) = std::thread::scope(|scope| {
        let settings = scope.spawn(|| {
            if wifi.is_empty() {
                return String::new();
            }
            let mut args = vec![
                "-t",
                "-f",
                "connection.uuid,802-11-wireless.ssid,802-11-wireless-security.key-mgmt",
                "connection",
                "show",
            ];
            args.extend(wifi.iter().map(String::as_str));
            output(&args).unwrap_or_default()
        });
        let addresses = scope.spawn(|| {
            output(&[
                "-t",
                "-f",
                "GENERAL.DEVICE,GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,IP6.ADDRESS,IP6.DNS",
                "device",
                "show",
            ])
            .unwrap_or_default()
        });
        (
            settings.join().unwrap_or_default(),
            addresses.join().unwrap_or_default(),
        )
    });
    model::wifi_settings(&mut saved, &settings);
    let wifi_enabled = radio.get(1).is_some_and(|state| state == "enabled");
    Snapshot {
        wifi_hardware: radio.first().is_none_or(|state| state == "enabled"),
        networks: if wifi_enabled {
            model::networks(&list)
        } else {
            Vec::new()
        },
        devices: model::devices(&devices),
        addresses: model::addresses(&addresses),
        wifi_enabled,
        saved,
    }
}

/// Ask for a fresh scan; its results come with a later snapshot.
pub fn rescan() {
    let _ = output(&["device", "wifi", "rescan"]);
}

pub fn settings(uuid: &str) -> Result<model::Settings, String> {
    output(&[
        "-t",
        "-f",
        model::SETTINGS_FIELDS,
        "connection",
        "show",
        "uuid",
        uuid,
    ])
    .map(|text| model::settings(&text))
}

pub fn set_wifi(on: bool) -> Result<(), String> {
    change(&["radio", "wifi", if on { "on" } else { "off" }])
}

/// A secret for `nmcli … passwd-file`: in the user's runtime directory
/// (memory only, mode 0700), readable by them alone, removed when dropped.
pub struct Secret(PathBuf);

impl Secret {
    /// `setting` is the secret's property, such as
    /// `802-11-wireless-security.psk`.
    pub fn new(setting: &str, value: &str) -> Result<Secret, String> {
        use std::os::unix::fs::OpenOptionsExt;
        if value.contains('\n') {
            return Err("A password cannot contain a line break".into());
        }
        let dir = std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .filter(|dir| dir.starts_with('/'))
            .ok_or("No runtime directory for the password")?;
        let nonce = fs::read_to_string("/proc/sys/kernel/random/uuid").unwrap_or_default();
        let path = PathBuf::from(dir).join(format!(
            "tatami-network-{}-{}",
            std::process::id(),
            nonce.trim()
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| error.to_string())?;
        let secret = Secret(path);
        file.write_all(format!("{setting}:{value}\n").as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(secret)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap_or_default()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub const PSK: &str = "802-11-wireless-security.psk";
pub const EAP_PASSWORD: &str = "802-1x.password";

/// Bring a connection up, on `device` when given, with a secret when given.
/// Waits for the outcome, up to 45 seconds.
pub fn connect(uuid: &str, device: Option<&str>, secret: Option<&Secret>) -> Result<(), String> {
    let mut args = vec!["--wait", "45", "connection", "up", "uuid", uuid];
    if let Some(device) = device {
        args.extend(["ifname", device]);
    }
    if let Some(secret) = secret {
        args.extend(["passwd-file", secret.path()]);
    }
    change(&args)
}

pub fn disconnect(uuid: &str) -> Result<(), String> {
    change(&["connection", "down", "uuid", uuid])
}

pub fn forget(uuid: &str) -> Result<(), String> {
    change(&["connection", "delete", "uuid", uuid])
}

/// Our own UUID for a new connection: nmcli reports the one it chose only in
/// prose.
fn new_uuid() -> Result<String, String> {
    fs::read_to_string("/proc/sys/kernel/random/uuid")
        .map(|uuid| uuid.trim().to_owned())
        .map_err(|error| error.to_string())
}

/// What an enterprise (802.1X) network asks for.
#[derive(Clone, Debug, Default)]
pub struct Enterprise {
    /// `peap` or `ttls`.
    pub method: String,
    /// The inner authentication: `mschapv2` or `pap`.
    pub inner: String,
    pub identity: String,
    pub anonymous: String,
    /// The server's domain, checked against its certificate.
    pub domain: String,
    /// Check the server against the system's certificate authorities.
    pub validate: bool,
}

/// How a new Wi-Fi connection is secured.
pub enum Join {
    Open,
    Personal { key_mgmt: &'static str },
    Enterprise(Enterprise),
}

/// Make a connection for a network and join it, with its password when it
/// has one. A new connection that fails to join is removed again, so a
/// mistyped password leaves nothing behind.
pub fn join(
    ssid: &str,
    hidden: bool,
    how: &Join,
    password: Option<&str>,
    device: Option<&str>,
) -> Result<(), String> {
    let uuid = new_uuid()?;
    let mut args: Vec<String> = [
        "connection",
        "add",
        "type",
        "wifi",
        "con-name",
        ssid,
        "ssid",
        ssid,
        "connection.uuid",
        &uuid,
    ]
    .map(str::to_owned)
    .into();
    if hidden {
        args.extend(["802-11-wireless.hidden".into(), "yes".into()]);
    }
    let secret = match how {
        Join::Open => None,
        Join::Personal { key_mgmt } => {
            args.extend(["wifi-sec.key-mgmt".into(), (*key_mgmt).into()]);
            Some(Secret::new(PSK, password.unwrap_or_default())?)
        }
        Join::Enterprise(enterprise) => {
            args.extend(
                [
                    ("wifi-sec.key-mgmt", "wpa-eap"),
                    ("802-1x.eap", enterprise.method.as_str()),
                    ("802-1x.phase2-auth", enterprise.inner.as_str()),
                    ("802-1x.identity", enterprise.identity.as_str()),
                ]
                .map(|(name, value)| [name.to_owned(), value.to_owned()])
                .concat(),
            );
            if !enterprise.anonymous.is_empty() {
                args.extend([
                    "802-1x.anonymous-identity".into(),
                    enterprise.anonymous.clone(),
                ]);
            }
            if enterprise.validate {
                args.extend(["802-1x.system-ca-certs".into(), "yes".into()]);
                if !enterprise.domain.is_empty() {
                    args.extend([
                        "802-1x.domain-suffix-match".into(),
                        enterprise.domain.clone(),
                    ]);
                }
            }
            Some(Secret::new(EAP_PASSWORD, password.unwrap_or_default())?)
        }
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    change(&args)?;
    connect(&uuid, device, secret.as_ref()).inspect_err(|_| {
        let _ = forget(&uuid);
    })
}

/// Join a saved network with a new password: its key is replaced only when
/// joining with it works.
pub fn rejoin(
    uuid: &str,
    setting: &str,
    password: &str,
    device: Option<&str>,
) -> Result<(), String> {
    let secret = Secret::new(setting, password)?;
    connect(uuid, device, Some(&secret))
}

/// Change settings of a connection, then apply them to it if it is in use:
/// at once where NetworkManager can (`device reapply`), or by reconnecting.
pub fn modify(
    uuid: &str,
    changes: &[(&str, String)],
    device: Option<&str>,
    reconnect: bool,
) -> Result<(), String> {
    let mut args = vec!["connection", "modify", "uuid", uuid];
    for (name, value) in changes {
        args.extend([*name, value.as_str()]);
    }
    change(&args)?;
    match device {
        Some(_) if reconnect => connect(uuid, device, None),
        Some(device) => change(&["device", "reapply", device]),
        None => Ok(()),
    }
}

/// Add a VPN from a configuration file (WireGuard's `.conf`).
pub fn import(kind: &str, path: &str) -> Result<(), String> {
    change(&["connection", "import", "type", kind, "file", path])
}

static MONITOR: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

/// Each change NetworkManager reports, as it happens: `nmcli monitor` runs
/// until stop_monitor, and `changed` is called per line.
pub fn monitor(changed: impl Fn() + Send + 'static) {
    let Ok(mut child) = Command::new("nmcli")
        .arg("monitor")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        return;
    };
    if let Ok(mut monitor) = MONITOR.lock() {
        *monitor = Some(child);
    }
    std::thread::spawn(move || {
        for _ in BufReader::new(stdout).lines().map_while(Result::ok) {
            changed();
        }
    });
}

pub fn stop_monitor() {
    if let Ok(mut monitor) = MONITOR.lock()
        && let Some(mut child) = monitor.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}
