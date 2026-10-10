//! Full-media live boots and end-to-end input delivery, without a target disk.
use crate::{
    gui_input,
    support::*,
    vm::{InputDevices, Rpc, Vm},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    Default,
    Compatibility,
    Lts,
}
impl Profile {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "default" => Ok(Self::Default),
            "compatibility" => Ok(Self::Compatibility),
            "lts" => Ok(Self::Lts),
            _ => bail!("Unknown live profile: {value}"),
        }
    }
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Default => 0,
            Self::Compatibility => 1,
            Self::Lts => 2,
        }
    }
    pub(crate) fn kernel(self) -> &'static str {
        if self == Self::Lts { "lts" } else { "latest" }
    }
    fn session(self) -> &'static str {
        if self == Self::Compatibility {
            "x11"
        } else {
            "wayland"
        }
    }
}

/// A successful QMP send alone proves nothing about the guest input stack.
/// Require ButtonPress and the actual ordered KeyPress keysyms seen by xev.
fn received_input(log: &str, expected: &str) -> bool {
    let mut event = "";
    let mut keys = String::new();
    let mut clicked = false;
    for line in log.lines() {
        if !line.starts_with(' ') && line.contains(" event,") {
            event = line.split_whitespace().next().unwrap_or("");
        }
        if event == "ButtonPress" && line.contains("button 1,") {
            clicked = true;
        }
        if event == "KeyPress"
            && let Some((_, tail)) = line.split_once("(keysym ")
            && let Some((_, name)) = tail.split_once(", ")
        {
            let name = name.split(')').next().unwrap_or("");
            if name.len() == 1 {
                keys.push_str(name);
            }
        }
    }
    clicked && keys == expected
}

fn probe_focused(log: &str) -> bool {
    log.lines()
        .rev()
        .find(|line| line.starts_with("FocusIn event,") || line.starts_with("FocusOut event,"))
        .is_some_and(|line| line.starts_with("FocusIn event,"))
}

// Read only the GUI connection variables, never the whole live process
// environment. These commands execute inside our disposable VM, not the host.
const USER_ENV: &str = r#"
pid=$(pgrep -u 1000 -f '(^|/)[.]?calamares-nixos(-wrapped)?( |$)' | head -1)
test -n "$pid"
display=$(tr '\0' '\n' < /proc/$pid/environ | sed -n 's/^DISPLAY=//p')
authority=$(tr '\0' '\n' < /proc/$pid/environ | sed -n 's/^XAUTHORITY=//p')
test -n "$display"
"#;

pub fn main(args: Vec<String>) -> Result<()> {
    let mut iso = None;
    let mut firmware = "uefi".to_string();
    let mut profile = Profile::Default;
    let mut input = InputDevices::Usb;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--firmware" => firmware = args.next().context("Missing firmware")?,
            "--profile" => profile = Profile::parse(&args.next().context("Missing profile")?)?,
            "--input" => {
                input = match args.next().as_deref() {
                    Some("usb") => InputDevices::Usb,
                    Some("ps2") => InputDevices::Ps2,
                    _ => bail!("Input must be usb or ps2"),
                }
            }
            value if !value.starts_with('-') && iso.is_none() => iso = Some(PathBuf::from(value)),
            _ => bail!("Unknown argument: {arg}"),
        }
    }
    ensure!(
        ["uefi", "bios"].contains(&firmware.as_str()),
        "Unknown firmware"
    );
    let iso = regular(&iso.context("Supply an ISO")?)?;
    let repo = repo()?;
    let id = stamp();
    let name = format!("live-{firmware}-{profile:?}-{input:?}-{id}");
    // Keep Unix socket paths below sockaddr_un's 108-byte limit, including
    // the repository path. Human-readable profile names belong in artifacts.
    let work = repo.join(".work").join(format!("lv-{id}"));
    let artifacts = repo.join("artifacts").join(&name);
    fs::create_dir_all(&artifacts)?;
    // Hash before starting the VM: media I/O must not consume the boot menu's
    // timeout on a slow host.
    let iso_sha256 = sha256(&iso)?;
    let mut vm = Vm::start_with_input(&work, &firmware, Some(&iso), None, None, input)?;
    let mut result = json!({"passed":false,"firmware":firmware,"profile":format!("{profile:?}"),"input":format!("{input:?}"),"iso_sha256":iso_sha256,"target_disk":false});
    let outcome = (|| -> Result<()> {
        println!("Booting {name}; no target disk attached");
        // USB-only firmware initialization varies. Wait for the actual loader,
        // not a fixed delay from QEMU startup that can send keys too early.
        // The menus have a ten-second timeout once they are displayed.
        let marker = if firmware == "uefi" {
            "Loading graphical boot menu"
        } else {
            "Automatic boot in"
        };
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let serial = fs::read(work.join("serial.log")).unwrap_or_default();
            if String::from_utf8_lossy(&serial).contains(marker) {
                break;
            }
            ensure!(Instant::now() < deadline, "Firmware did not reach its menu");
            pause(Duration::from_millis(200))?;
        }
        pause(Duration::from_secs(2))?;
        let mut qmp = Rpc::connect(&work.join("qmp.sock"), true, 10)?;
        gui_input::key(&mut qmp, &["down"])?;
        gui_input::key(&mut qmp, &["home"])?;
        // QEMU's monitor socket accepts one client. Reuse the input connection
        // for screenshots instead of opening another monitor and deadlocking.
        for _ in 0..profile.index() {
            gui_input::key(&mut qmp, &["down"])?;
        }
        qmp.screenshot(&artifacts.join("boot-menu.png"))?;
        gui_input::key(&mut qmp, &["ret"])?;
        vm.wait_agent()?;
        println!("Guest agent ready; waiting for the {profile:?} desktop and installer");
        let deadline = Instant::now() + Duration::from_secs(180);
        let desktop = if profile == Profile::Compatibility {
            "xfce4-session"
        } else {
            "plasmashell"
        };
        let live = loop {
            match vm.execute(&format!("set -e; systemctl is-active display-manager; pgrep -u 1000 -f '(^|/)[.]?{desktop}(-wrapped)?( |$)'; pgrep -u 1000 -f '(^|/)[.]?calamares-nixos(-wrapped)?( |$)'; uname -r; cat /etc/calamares-nixos/settings.json; test ! -e /etc/nixos-generate-config.conf; nix --version"), Duration::from_secs(30)) {
                Ok(text) => break text,
                Err(error) => { ensure!(Instant::now() < deadline, "Live desktop failed: {error}"); pause(Duration::from_secs(2))?; }
            }
        };
        ensure!(
            live.contains(&format!("\"kernel\":\"{}\"", profile.kernel())),
            "Wrong kernel profile: {live}"
        );
        ensure!(
            live.contains("\"test_diagnostics\":false") && live.contains("Determinate Nix"),
            "Wrong media settings"
        );
        let session = vm.execute("set -e; s=$(loginctl list-sessions --no-legend | awk '$2 == 1000 && $4 == \"seat0\" {print $1; exit}'); test -n \"$s\"; loginctl show-session \"$s\" -p Type -p Active -p Seat", Duration::from_secs(30))?;
        ensure!(
            session.contains(&format!("Type={}", profile.session()))
                && session.contains("Active=yes")
                && session.contains("Seat=seat0"),
            "Wrong/inactive seat: {session}"
        );
        result["live"] = json!(live);
        result["session"] = json!(session);
        if profile == Profile::Compatibility {
            let rendering = vm.execute(&format!("set -e; {USER_ENV}\ntr '\\0' '\\n' < /proc/$pid/environ | grep -E '^(LIBGL_ALWAYS_SOFTWARE|GSK_RENDERER)='; runuser -u nixos -- env DISPLAY=\"$display\" XAUTHORITY=\"$authority\" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus xfconf-query -c xfwm4 -p /general/use_compositing"), Duration::from_secs(30))?;
            ensure!(
                rendering.contains("LIBGL_ALWAYS_SOFTWARE=1")
                    && rendering.contains("GSK_RENDERER=cairo")
                    && rendering.lines().any(|s| s == "false"),
                "Software recovery settings not active: {rendering}"
            );
            result["recovery_rendering"] = json!(rendering);
        }
        println!("Live session verified; testing graphical input delivery");
        // Process existence precedes GTK's first mapped window. Let autostart
        // settle, then check focus explicitly so a late installer window cannot
        // steal the marker intended for the probe.
        pause(Duration::from_secs(5))?;
        qmp.screenshot(&artifacts.join("live-desktop.png"))?;
        // xev has a real graphical window. Start it unprivileged in the live
        // user's X11/Xwayland session and drain its output to a VM-only file.
        vm.execute(&format!("set -e; {USER_ENV}\nrunuser -u nixos -- env DISPLAY=\"$display\" XAUTHORITY=\"$authority\" XDG_RUNTIME_DIR=/run/user/1000 stdbuf -oL xev -name RespinInputProbe -geometry 1920x1200+0+0 -event keyboard -event mouse -event focus >/tmp/live-input.log 2>&1 </dev/null &"), Duration::from_secs(30))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let ready = vm.execute(&format!("{USER_ENV}\nrunuser -u nixos -- env DISPLAY=\"$display\" XAUTHORITY=\"$authority\" xwininfo -name RespinInputProbe"), Duration::from_secs(10));
            if ready.is_ok() {
                break;
            }
            ensure!(Instant::now() < deadline, "Input probe window did not open");
            pause(Duration::from_millis(500))?;
        }
        pause(Duration::from_secs(2))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let log = vm.execute("cat /tmp/live-input.log", Duration::from_secs(10))?;
            if probe_focused(&log) {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Input probe never received focus"
            );
            // Ordinary window switching, not synthetic X11 keyboard events.
            gui_input::key(&mut qmp, &["alt", "tab"])?;
            pause(Duration::from_millis(500))?;
        }
        let mice = qmp.call("query-mice", json!({}))?;
        let mouse = mice
            .as_array()
            .context("No input devices")?
            .iter()
            .find(|m| m["absolute"] == (input == InputDevices::Usb))
            .context("Requested pointer not found")?;
        qmp.call("human-monitor-command", json!({"command-line":format!("mouse_set {}", mouse["index"].as_u64().context("Mouse index")?)}))?;
        let selected = qmp.call("query-mice", json!({}))?;
        ensure!(
            selected
                .as_array()
                .context("No pointer inventory")?
                .iter()
                .any(|m| m["index"] == mouse["index"] && m["current"] == true),
            "QEMU did not select the requested pointer"
        );
        let motion = if input == InputDevices::Usb {
            "abs"
        } else {
            "rel"
        };
        let offset = if input == InputDevices::Usb {
            16384
        } else {
            20
        };
        qmp.call("input-send-event", json!({"events":[{"type":motion,"data":{"axis":"x","value":offset}},{"type":motion,"data":{"axis":"y","value":offset}}]}))?;
        pause(Duration::from_millis(500))?;
        for down in [true, false] {
            qmp.call(
                "input-send-event",
                json!({"events":[{"type":"btn","data":{"down":down,"button":"left"}}]}),
            )?;
            pause(Duration::from_millis(150))?;
        }
        gui_input::type_text(&mut qmp, "input42")?;
        pause(Duration::from_secs(1))?;
        let events = vm.execute("cat /tmp/live-input.log", Duration::from_secs(10))?;
        fs::write(artifacts.join("input-events.log"), &events)?;
        qmp.screenshot(&artifacts.join("input-probe.png"))?;
        ensure!(
            received_input(&events, "input42"),
            "Guest did not receive the click and exact keyboard sequence"
        );
        result["input_delivery"] = json!(true);
        result["pointer_devices"] = selected;
        gui_input::key(&mut qmp, &["alt", "f4"])?;
        let diagnostics = vm.execute("installer-live-diagnostics", Duration::from_secs(180))?;
        fs::write(artifacts.join("hardware-report.txt"), diagnostics)?;
        vm.poweroff()?;
        result["passed"] = json!(true);
        Ok(())
    })();
    if let Err(error) = &outcome {
        result["error"] = json!(error.to_string());
        let _ = vm.screenshot(&artifacts.join("failure.png"));
        if let Ok(text) = vm.execute(
            "systemctl status display-manager --no-pager; ps -eo uid,pid,comm,args; cat /tmp/live-input.log 2>/dev/null; journalctl -b -u display-manager --no-pager -n 100",
            Duration::from_secs(30),
        ) {
            fs::write(artifacts.join("failure-diagnostics.log"), text)?;
        }
    }
    vm.stop();
    for file in ["serial.log", "qemu.log"] {
        if work.join(file).exists() {
            fs::copy(work.join(file), artifacts.join(file))?;
        }
    }
    write_json(&artifacts.join("result.json"), &result)?;
    outcome?;
    println!(
        "PASS: boot, active seat, graphical pointer/keyboard delivery, diagnostics and clean shutdown: {}",
        artifacts.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_requires_real_press_events_and_order() {
        let log = "ButtonPress event, serial 1\n    button 1, same_screen YES\nKeyPress event, serial 2\n    keycode 30 (keysym 0x75, u), same_screen YES\nKeyRelease event, serial 3\n    keycode 30 (keysym 0x75, u), same_screen YES\nKeyPress event, serial 4\n    keycode 31 (keysym 0x69, i), same_screen YES\n";
        assert!(received_input(log, "ui"));
        assert!(!received_input(log, "iu"));
        assert!(!received_input(
            &log.replace("ButtonPress", "ButtonRelease"),
            "ui"
        ));
        assert!(!received_input(
            &log.replace("KeyPress", "KeyRelease"),
            "ui"
        ));
        assert!(!received_input("QMP success", "ui"));
    }
    #[test]
    fn profiles_are_explicit() {
        assert!(Profile::parse("unknown").is_err());
        assert_eq!(Profile::parse("compatibility").unwrap().kernel(), "latest");
        assert_eq!(Profile::parse("lts").unwrap().index(), 2);
    }
    #[test]
    fn probe_focus_uses_the_last_transition() {
        assert!(!probe_focused(""));
        assert!(probe_focused(
            "FocusOut event, serial 1\nFocusIn event, serial 2\n"
        ));
        assert!(!probe_focused(
            "FocusIn event, serial 1\nFocusOut event, serial 2\n"
        ));
    }
}
