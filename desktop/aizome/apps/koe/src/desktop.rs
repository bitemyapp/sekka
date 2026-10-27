use crate::{
    config::{Desktop, Output, OutputMode, PasteShortcut},
    ipc,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{env, path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

pub fn aizome_socket() -> Option<PathBuf> {
    env::var_os("AIZOME_SOCKET").map(PathBuf::from)
}
pub fn uses_aizome(output: &Output) -> bool {
    output.desktop == Desktop::Aizome || (output.desktop == Desktop::Auto && aizome_socket().is_some())
}
async fn compositor(value: Value) -> Result<Value> {
    let path = aizome_socket().context("AIZOME_SOCKET is unset")?;
    let response: Value = ipc::exchange(&path, &value).await?;
    ensure!(response["response"] != "error", "Aizome rejected the request");
    Ok(response)
}
pub async fn focused_window() -> Result<Option<u64>> {
    Ok(focused_window_info().await?["id"].as_u64())
}
async fn focused_window_info() -> Result<Value> {
    let value = compositor(json!({"request":"focused-window"})).await?;
    ensure!(value["response"] == "window", "unexpected Aizome focus response");
    Ok(value["window"].clone())
}

fn paste_chord(output: &Output, window: &Value) -> &'static str {
    match output.paste_shortcut {
        PasteShortcut::CtrlV => "ctrl+v",
        PasteShortcut::CtrlShiftV => "ctrl+shift+v",
        PasteShortcut::SuperV => "super+v",
        PasteShortcut::Auto => {
            let app = window["app_id"].as_str().unwrap_or_default();
            if window["kit_app"] == true || app.starts_with("org.aizome.") {
                "super+v"
            } else if [
                "foot",
                "footclient",
                "kitty",
                "Alacritty",
                "org.wezfurlong.wezterm",
                "com.mitchellh.ghostty",
                "org.gnome.Console",
                "org.gnome.Terminal",
                "org.gnome.Ptyxis",
                "org.kde.konsole",
                "xterm",
                "XTerm",
                "st",
                "st-256color",
                "urxvt",
                "rio",
            ]
            .contains(&app)
            {
                // Send real Ctrl+Shift modifiers. A translated command key can
                // reach terminal programs as Ctrl+V (Codex's image attachment).
                "ctrl+shift+v"
            } else {
                "ctrl+v"
            }
        }
    }
}
async fn check_destination(target: Option<u64>, guard: bool) -> Result<()> {
    let recovery = compositor(json!({"request":"recovery-state"})).await?;
    ensure!(recovery["state"]["locked"] == false, "desktop is locked; transcript remains in memory");
    if guard {
        ensure!(
            target.is_some() && focused_window().await? == target,
            "focused window changed; your words are kept: choose Paste Last Transcript from Koe in the menu bar"
        );
    }
    Ok(())
}

async fn wait_for_release(target: Option<u64>, guard: bool) -> Result<()> {
    // Recognition can finish before the person releases the stop shortcut.
    // Keep the microphone off and wait asynchronously, so Cancel still works.
    // Recheck destination on every poll: changing focus or locking must never
    // turn this into a delayed paste into a different application.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            check_destination(target, guard).await?;
            let input = compositor(json!({"request":"input-state"})).await?;
            ensure!(input["response"] == "input-state", "unexpected Aizome input response");
            let keys = input["state"]["pressed_keys"].as_array()
                .context("unexpected Aizome pressed keys response")?;
            if keys.is_empty() { return Ok(()); }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await.context("keyboard stayed pressed for 5 seconds; microphone is off and your words are kept: choose Paste Last Transcript from Koe in the menu bar")?
}
pub async fn copy(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", "text/plain;charset=utf-8"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("cannot start wl-copy; install wl-clipboard")?;
    let mut stdin = child.stdin.take().context("wl-copy has no stdin")?;
    tokio::time::timeout(Duration::from_secs(5), async {
        stdin.write_all(text.as_bytes()).await?;
        drop(stdin);
        let status = child.wait().await?;
        ensure!(status.success(), "wl-copy failed; check the Wayland session");
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("clipboard operation timed out")?
}
pub async fn deliver(text: &str, output: &Output, target: Option<u64>) -> Result<()> {
    if output.mode == OutputMode::None {
        return Ok(());
    }
    if output.mode == OutputMode::Paste && uses_aizome(output) {
        // Check before changing the clipboard, then again immediately before input.
        wait_for_release(target, output.require_same_window).await?;
    }
    copy(text).await?;
    if output.mode == OutputMode::Clipboard {
        return Ok(());
    }
    tokio::time::sleep(Duration::from_millis(output.paste_delay_ms)).await;
    if uses_aizome(output) {
        wait_for_release(target, output.require_same_window).await?;
        let window = focused_window_info().await?;
        ensure!(
            !output.require_same_window || window["id"].as_u64() == target,
            "focused window changed; your words are kept: choose Paste Last Transcript from Koe in the menu bar"
        );
        let reply =
            compositor(json!({"request":"synthesize","events":[{"type":"chord","keys":paste_chord(output, &window)}]}))
                .await?;
        ensure!(reply["response"] == "ok", "unexpected Aizome paste response");
    } else {
        let chord = paste_chord(output, &Value::Null);
        let args: &[&str] = match chord {
            "ctrl+shift+v" => &["-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl"],
            "super+v" => &["-M", "logo", "-k", "v", "-m", "logo"],
            _ => &["-M", "ctrl", "-k", "v", "-m", "ctrl"],
        };
        let mut child = Command::new("wtype")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("cannot start wtype; install it or use output.mode = 'clipboard'")?;
        let result = tokio::time::timeout(Duration::from_secs(5), child.wait()).await.context("wtype timed out")??;
        ensure!(result.success(), "wtype failed; compositor may not support virtual keyboards; use clipboard mode");
    }
    Ok(())
}

/// Open Koe's window with `args`. The compositor starts it, as it does an
/// app from the launcher, so it isn't one of the service's processes and
/// lives on when the service stops.
pub async fn open_window(args: Vec<String>) -> Result<()> {
    let exe = env::current_exe()?.display().to_string();
    let argv: Vec<String> = std::iter::once(exe.clone()).chain(args.clone()).collect();
    if aizome_socket().is_some()
        && compositor(json!({"request":"spawn","argv":argv,"in_desk_folder":false})).await.is_ok()
    {
        return Ok(());
    }
    let mut child = std::process::Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

pub fn notify(summary: &'static str, body: impl Into<String>) {
    let body = body.into();
    tokio::spawn(async move {
        if let Some(path) = env::var_os("AIZOME_SHELL_SOCKET") {
            let _: Result<Value> = ipc::exchange(
                &PathBuf::from(path),
                &json!({"type":"request","request":"notify","app":"Koe","summary":summary,"body":body}),
            )
            .await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_paste_uses_real_modifiers_and_native_apps_keep_command_key() {
        let mut output = Output::default();
        assert_eq!(paste_chord(&output, &json!({"app_id":"foot"})), "ctrl+shift+v");
        assert_eq!(paste_chord(&output, &json!({"app_id":"org.aizome.Sumi","kit_app":true})), "super+v");
        assert_eq!(paste_chord(&output, &json!({"app_id":"google-chrome"})), "ctrl+v");
        output.paste_shortcut = PasteShortcut::CtrlShiftV;
        assert_eq!(paste_chord(&output, &json!({"app_id":"custom-terminal"})), "ctrl+shift+v");
    }
}
