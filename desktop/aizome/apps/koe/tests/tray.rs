//! Real SNI/DBusMenu roundtrip on a private bus, never the login session bus.
use anyhow::{Context, Result, ensure};
use koe::{
    config::{Backend, Config, OutputMode},
    ipc,
    protocol::{Action, Phase, Request},
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::sync::mpsc;
use zbus::{
    Proxy,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Watcher(mpsc::Sender<String>);
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(&self, service: String) {
        let _ = self.0.send(service).await;
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
}
async fn wait_phase(socket: &Path, phase: Phase) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(response) = ipc::request(socket, &Request::new(Action::Status)).await
                && response.status.phase == phase
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .context("daemon phase timeout")
}
async fn click(menu: &Proxy<'_>, label: &str) -> Result<()> {
    let properties: Vec<(i32, HashMap<String, OwnedValue>)> =
        menu.call("GetGroupProperties", &(Vec::<i32>::new(), vec!["label", "enabled"])).await?;
    let (id, props) = properties
        .iter()
        .find(|(_, props)| props.get("label").and_then(|v| <&str>::try_from(v).ok()) == Some(label))
        .with_context(|| format!("missing menu item {label}"))?;
    ensure!(props.get("enabled").map(|v| bool::try_from(v).unwrap()).unwrap_or(true), "menu item disabled: {label}");
    menu.call::<_, _, ()>("Event", &(*id, "clicked", Value::I32(0), 0u32)).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires dbus-daemon; uses an isolated session bus and fake microphone"]
async fn tray_registers_updates_controls_and_recovers_after_host_restart() -> Result<()> {
    exercise_tray(None).await
}

#[tokio::test]
#[ignore = "requires dbus-daemon; uses an isolated session bus and fake microphone"]
async fn tray_turns_off_when_settings_cannot_be_saved() -> Result<()> {
    exercise_tray(Some(SettingsFailure::Write)).await
}

#[tokio::test]
#[ignore = "requires dbus-daemon; uses an isolated session bus and fake microphone"]
async fn tray_turns_off_when_settings_cannot_be_read() -> Result<()> {
    exercise_tray(Some(SettingsFailure::Read)).await
}

#[derive(Clone, Copy)]
enum SettingsFailure {
    Write,
    Read,
}

async fn exercise_tray(failure: Option<SettingsFailure>) -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut bus = Process(
        Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut address = String::new();
    BufReader::new(bus.0.stdout.take().unwrap()).read_line(&mut address)?;
    let address = address.trim();
    let connection = zbus::connection::Builder::address(address)?.build().await?;
    let (registered, mut registrations) = mpsc::channel(4);
    let host = zbus::connection::Builder::address(address)?
        .name("org.kde.StatusNotifierWatcher")?
        .serve_at("/StatusNotifierWatcher", Watcher(registered.clone()))?
        .build()
        .await?;
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin)?;
    let shell =
        std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|p| p.join("sh")).find(|p| p.is_file()).unwrap();
    let microphone = bin.join("pw-record");
    std::fs::write(&microphone, format!("#!{}\nexec sleep 60\n", shell.display()))?;
    std::fs::set_permissions(microphone, std::fs::Permissions::from_mode(0o700))?;
    let mut config = Config { enabled: true, ..Config::default() };
    config.engine.backend = Backend::Http;
    config.output.mode = OutputMode::None;
    config.output.notify = false;
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, toml::to_string(&config)?)?;
    let socket = dir.path().join("runtime/control.sock");
    let mut daemon = Process(
        Command::new(env!("CARGO_BIN_EXE_aizome-koe"))
            .arg("--config")
            .arg(&config_path)
            .arg("--socket")
            .arg(&socket)
            .arg("daemon")
            .env("DBUS_SESSION_BUS_ADDRESS", address)
            .env("XDG_DATA_HOME", dir.path().join("data"))
            .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH")?))
            .env_remove("AIZOME_SOCKET")
            .env_remove("AIZOME_SHELL_SOCKET")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let service =
        tokio::time::timeout(Duration::from_secs(5), registrations.recv()).await?.context("no tray registration")?;
    wait_phase(&socket, Phase::Idle).await?;
    // SNI uses NewTitle/NewIcon signals, not PropertiesChanged caching.
    let item: Proxy<'_> = zbus::proxy::Builder::new(&connection)
        .destination(service.as_str())?
        .path("/StatusNotifierItem")?
        .interface("org.kde.StatusNotifierItem")?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    ensure!(item.get_property::<String>("Id").await? == "org.aizome.Koe", "incorrect identity");
    ensure!(item.get_property::<bool>("ItemIsMenu").await?, "click must open menu");
    let menu_path = item.get_property::<OwnedObjectPath>("Menu").await?;
    let menu = Proxy::new(&connection, service.as_str(), menu_path.as_str(), "com.canonical.dbusmenu").await?;
    click(&menu, "Start Dictation").await?;
    wait_phase(&socket, Phase::Recording).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while item.get_property::<String>("Title").await.unwrap() != "Koe — Listening" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    click(&menu, "Pause Dictation").await?;
    wait_phase(&socket, Phase::Paused).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    click(&menu, "Resume Dictation").await?;
    wait_phase(&socket, Phase::Recording).await?;
    click(&menu, "Cancel Dictation").await?;
    wait_phase(&socket, Phase::Idle).await?;
    // The desktop can restart without losing Koe or requiring a daemon restart.
    host.close().await?;
    let _replacement = zbus::connection::Builder::address(address)?
        .name("org.kde.StatusNotifierWatcher")?
        .serve_at("/StatusNotifierWatcher", Watcher(registered))?
        .build()
        .await?;
    let again = tokio::time::timeout(Duration::from_secs(5), registrations.recv())
        .await?
        .context("tray did not re-register")?;
    ensure!(again == service, "tray identity changed");
    click(&menu, "Start Dictation").await?;
    wait_phase(&socket, Phase::Recording).await?;
    let broken = "[engine]\nlanguage = 3\n";
    match failure {
        Some(SettingsFailure::Write) => std::fs::create_dir(config_path.with_extension("toml.tmp"))?,
        Some(SettingsFailure::Read) => std::fs::write(&config_path, broken)?,
        None => {}
    }
    click(&menu, "Turn Off Dictation").await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = daemon.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    ensure!(!socket.exists(), "socket remains after tray quit");
    match failure {
        Some(SettingsFailure::Write) => ensure!(Config::load(&config_path)?.enabled, "settings were overwritten"),
        Some(SettingsFailure::Read) => {
            ensure!(std::fs::read_to_string(&config_path)? == broken, "settings were overwritten")
        }
        None => ensure!(!Config::load(&config_path)?.enabled, "turning off kept dictation on"),
    }
    Ok(())
}
