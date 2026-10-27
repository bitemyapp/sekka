//! Optional desktop regression against installed Aizome and foot. It owns an
//! isolated headless compositor, clipboard, runtime directory and fake mic.
use anyhow::{Context, Result, ensure};
use koe::{
    config::Config,
    ipc,
    protocol::{Action, Phase, Request},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn compositor(path: &Path, value: Value) -> Result<Value> {
    ipc::exchange(path, &value).await
}
async fn wait_file(path: &Path) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("waiting for test process")
}
fn script(path: &Path, body: &str) -> Result<()> {
    let shell = std::env::split_paths(&std::env::var_os("PATH").context("PATH unset")?)
        .map(|p| p.join("sh"))
        .find(|p| p.is_file())
        .context("sh missing")?;
    fs::write(path, format!("#!{}\n{body}\n", shell.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires installed Aizome, foot, wl-copy, KOE_TEST_MODEL and KOE_TEST_WAV"]
async fn two_real_aizome_shortcuts_recognize_and_paste_into_foot() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let home = root.join("home");
    fs::create_dir(&home)?;
    let config_home = root.join("config");
    fs::create_dir_all(config_home.join("aizome"))?;
    let socket = root.join("koe/control.sock");
    let comp_socket = root.join("aizome.koe-test.sock");
    let config_path = root.join("koe.toml");
    let captured_text = root.join("inserted.txt");
    let terminal_ready = root.join("terminal-ready");
    let model = PathBuf::from(std::env::var("KOE_TEST_MODEL")?);
    let wav = PathBuf::from(std::env::var("KOE_TEST_WAV")?);
    let samples = koe::audio::read_wav(&wav, 60)?;
    let raw: Vec<u8> = samples.iter().flat_map(|s| ((s * 32767.0) as i16).to_ne_bytes()).collect();
    fs::write(root.join("samples.raw"), raw)?;
    script(&bin.join("pw-record"), "cat \"$KOE_TEST_PCM\"\nexec sleep 60")?;
    // Test text goes only to a file via a terminal in the isolated compositor.
    script(
        &bin.join("receive-paste"),
        "stty raw -echo\nprintf '\\033[>1u\\033[?2004h'\ntouch \"$KOE_TEST_READY\"\nexec cat > \"$KOE_TEST_INSERTED\"",
    )?;
    let exe = std::env::var("KOE_TEST_KOE").unwrap_or_else(|_| env!("CARGO_BIN_EXE_aizome-koe").into());
    let binding = format!("spawn {exe} --config {} --socket {} record toggle", config_path.display(), socket.display());
    fs::write(
        config_home.join("aizome/settings.toml"),
        format!("[keys]\ncommand_key_for_all_apps = false\n[keys.bindings]\n\"Super+Alt+D\" = {binding:?}\n"),
    )?;
    let mut config = Config::default();
    config.engine.model = model;
    config.engine.language = std::env::var("KOE_TEST_LANGUAGE").unwrap_or_else(|_| "en".into());
    config.engine.gpu = std::env::var_os("KOE_TEST_GPU").is_some();
    config.output.notify = false;
    config.tray.enabled = false;
    fs::write(&config_path, toml::to_string(&config)?)?;
    let mut env = std::collections::BTreeMap::from([
        ("HOME".to_owned(), home.to_string_lossy().into_owned()),
        ("XDG_RUNTIME_DIR".into(), root.to_string_lossy().into_owned()),
        ("XDG_CONFIG_HOME".into(), config_home.to_string_lossy().into_owned()),
        ("XDG_STATE_HOME".into(), root.join("state").to_string_lossy().into_owned()),
        ("XDG_DATA_HOME".into(), root.join("data").to_string_lossy().into_owned()),
        ("XDG_CACHE_HOME".into(), root.join("cache").to_string_lossy().into_owned()),
        ("AIZOME_SOCKET".into(), comp_socket.to_string_lossy().into_owned()),
        ("WAYLAND_DISPLAY".into(), "koe-test".into()),
        ("PATH".into(), format!("{}:{}", bin.display(), std::env::var("PATH")?)),
        ("KOE_TEST_PCM".into(), root.join("samples.raw").to_string_lossy().into_owned()),
        ("KOE_TEST_READY".into(), terminal_ready.to_string_lossy().into_owned()),
        ("KOE_TEST_INSERTED".into(), captured_text.to_string_lossy().into_owned()),
        ("DBUS_SESSION_BUS_ADDRESS".into(), format!("unix:path={}/no-bus", root.display())),
    ]);
    let _comp = Process(
        Command::new(std::env::var("KOE_TEST_AIZOME").unwrap_or_else(|_| "aizome".into()))
            .args([
                "--backend",
                "headless",
                "--renderer",
                "pixman",
                "--socket",
                "koe-test",
                "--headless-output",
                "1024x768@1",
                "--no-session",
                "--no-shell",
                "--no-xwayland",
            ])
            .envs(&env)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY")
            .env_remove("AIZOME_SHELL_SOCKET")
            .stdout(Stdio::null())
            .stderr(fs::File::create(root.join("aizome.log"))?)
            .spawn()?,
    );
    wait_file(&comp_socket).await?;
    let _daemon = Process(
        Command::new(&exe)
            .arg("--config")
            .arg(&config_path)
            .arg("--socket")
            .arg(&socket)
            .arg("daemon")
            .envs(&env)
            .env_remove("AIZOME_SHELL_SOCKET")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    wait_file(&socket).await?;
    // Keep an inherited font/theme override from the login session from affecting foot.
    env.insert("TERM".into(), "xterm-256color".into());
    let _foot = Process(
        Command::new("foot")
            .args(["--app-id=foot", "--title=Koe isolated test", "--"])
            .arg(bin.join("receive-paste"))
            .envs(&env)
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY")
            .env_remove("AIZOME_SHELL_SOCKET")
            .stdout(Stdio::null())
            .stderr(fs::File::create(root.join("foot.log"))?)
            .spawn()?,
    );
    wait_file(&terminal_ready).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let focus = compositor(&comp_socket, json!({"request":"focused-window"})).await?;
            if focus["window"]["app_id"] == "foot" {
                break Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("test terminal did not map and take focus")??;
    // Allow startup warmup to finish before timing stop-to-paste latency.
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let status = ipc::request(&socket, &Request::new(Action::Status)).await?;
            if status.status.model_loaded {
                break Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .context("model did not warm up")??;
    for attempt in 1..=2 {
        let chord = json!({"request":"synthesize","events":[{"type":"chord","keys":"super+alt+d"}]});
        ensure!(compositor(&comp_socket, chord.clone()).await?["response"] == "ok", "first chord rejected");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = ipc::request(&socket, &Request::new(Action::Status)).await?;
                if status.status.phase == Phase::Recording
                    && status.status.elapsed_ms >= (samples.len() * 1000 / koe::audio::RATE) as u64
                {
                    break Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await??;
        let stopped = std::time::Instant::now();
        let before = fs::read(&captured_text)?;
        // Unlike a synthetic chord, real people do not release all three keys
        // instantly. Keep them down through recognition on the second attempt.
        let stop_event = if attempt == 2 {
            json!({"request":"synthesize","events":[
                {"type":"key","key":"super","pressed":true},
                {"type":"key","key":"alt","pressed":true},
                {"type":"key","key":"d","pressed":true}
            ]})
        } else {
            chord
        };
        ensure!(compositor(&comp_socket, stop_event).await?["response"] == "ok", "second chord rejected");
        if attempt == 2 {
            tokio::time::timeout(Duration::from_secs(40), async {
                loop {
                    let status = ipc::request(&socket, &Request::new(Action::Status)).await?.status;
                    ensure!(status.phase != Phase::Error, "held-key dictation failed: {:?}", status.error);
                    if status.phase == Phase::Delivering {
                        break Ok::<_, anyhow::Error>(());
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await??;
            tokio::time::sleep(Duration::from_millis(200)).await;
            ensure!(
                ipc::request(&socket, &Request::new(Action::Status)).await?.status.phase == Phase::Delivering,
                "did not wait for shortcut release"
            );
            ensure!(fs::read(&captured_text)? == before, "pasted before shortcut release");
            ensure!(
                compositor(
                    &comp_socket,
                    json!({"request":"synthesize","events":[
                        {"type":"key","key":"d","pressed":false},
                        {"type":"key","key":"alt","pressed":false},
                        {"type":"key","key":"super","pressed":false}
                    ]})
                )
                .await?["response"]
                    == "ok",
                "key release rejected"
            );
        }
        tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                let status = ipc::request(&socket, &Request::new(Action::Status)).await?;
                ensure!(status.status.phase != Phase::Error, "dictation failed: {:?}", status.status.error);
                if status.status.phase == Phase::Idle && status.status.has_last {
                    break Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await??;
        eprintln!("dictation {attempt}: stop-to-paste {} ms", stopped.elapsed().as_millis());
    }
    let text = fs::read_to_string(&captured_text)?;
    ensure!(
        text.contains("\u{1b}[200~") && text.contains("\u{1b}[201~"),
        "terminal did not receive a bracketed text paste"
    );
    ensure!(!text.contains('\u{16}') && !text.contains("118;5u"), "Ctrl+V leaked to terminal application");
    ensure!(
        text.to_lowercase().matches("ask not what your country").count() == 2,
        "both recognized transcripts must be inserted into the isolated terminal"
    );
    let logs = fs::read_to_string(root.join("aizome.log"))?;
    ensure!(!logs.contains("ask not what your country"), "transcript leaked into compositor log");
    ensure!(ipc::request(&socket, &Request::new(Action::Shutdown)).await?.ok, "shutdown failed");
    Ok(())
}
