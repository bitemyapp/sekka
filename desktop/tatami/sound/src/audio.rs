// SPDX-License-Identifier: MIT OR Apache-2.0
//! Blocking audio operations, always called off the GTK thread. pactl talks
//! to the session's pipewire-pulse; no privileged helper or shell is involved.
//! `pactl subscribe` reports each change while the window is open (monitor).
//!
//! A stream is moved with pw-metadata instead. pipewire-pulse's move only
//! sets the stream's target in PipeWire's default metadata, for WirePlumber
//! to follow, and acknowledges it before PipeWire has those changes:
//! pactl, which disconnects on the acknowledgement, can leave the stream
//! where it was (PipeWire 1.6.9). pw-metadata sets the same keys and waits
//! for PipeWire before it exits.
use crate::model::{self, Kind, Side, Snapshot};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};

/// A device's or stream's volume, in percent of its loudest channel, and
/// its balance over channels on these sides.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub volume: f64,
    pub balance: f64,
    pub sides: Vec<Side>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Volume(Kind, u32, Level),
    Mute(Kind, u32, bool),
    Default(Kind, String),
    Port(Kind, u32, String),
    /// Play a stream (its PipeWire node) through a device, or record it
    /// from one (the device's node and serial); None follows the default
    /// device, as pipewire-pulse's move to the default does.
    Move(u32, Option<(u32, u64)>),
    Profile(String, String),
}
impl Action {
    /// pactl's arguments for it; a move's are pw-metadata's (moves).
    pub fn args(&self) -> Vec<String> {
        match self {
            // Balanced: one volume for all channels. Otherwise each
            // channel's, so a volume change keeps the balance.
            Self::Volume(k, id, level) if level.balance == 0.0 || level.sides.len() < 2 => vec![
                format!("set-{}-volume", k.noun()),
                id.to_string(),
                format!("{}%", level.volume.clamp(0.0, model::MAX_VOLUME).round()),
            ],
            Self::Volume(k, id, level) => {
                let mut args = vec![format!("set-{}-volume", k.noun()), id.to_string()];
                args.extend(
                    model::channel_volumes(level.volume, level.balance, &level.sides)
                        .iter()
                        .map(u32::to_string),
                );
                args
            }
            Self::Mute(k, id, v) => vec![
                format!("set-{}-mute", k.noun()),
                id.to_string(),
                if *v { "1" } else { "0" }.into(),
            ],
            Self::Default(k, name) => vec![format!("set-default-{}", k.noun()), name.clone()],
            Self::Port(k, id, name) => vec![
                format!("set-{}-port", k.noun()),
                id.to_string(),
                name.clone(),
            ],
            Self::Move(..) => Vec::new(),
            Self::Profile(name, profile) => {
                vec!["set-card-profile".into(), name.clone(), profile.clone()]
            }
        }
    }
    /// pw-metadata's arguments for a move, a run for each key it sets in
    /// the default metadata, as pipewire-pulse sets them: WirePlumber reads
    /// target.object (the device's serial) first, target.node (its node)
    /// after; -1 for none.
    pub fn moves(&self) -> Vec<Vec<String>> {
        let Self::Move(stream, target) = self else {
            return Vec::new();
        };
        let (node, serial) = target.map_or(("-1".into(), "-1".into()), |(node, serial)| {
            (node.to_string(), serial.to_string())
        });
        [("target.object", serial), ("target.node", node)]
            .into_iter()
            // After "--", -1 is a value rather than an option.
            .map(|(key, value)| {
                [
                    "-n",
                    "default",
                    "--",
                    &stream.to_string(),
                    key,
                    &value,
                    "Spa:Id",
                ]
                .map(str::to_owned)
                .to_vec()
            })
            .collect()
    }
    /// What a later action replaces before it is sent: a slider's earlier
    /// values for the same device or stream.
    pub fn replaces(&self, earlier: &Action) -> bool {
        matches!((self, earlier), (Self::Volume(k, i, _), Self::Volume(j, l, _)) if k == j && i == l)
    }
}
fn output(args: &[&str]) -> Result<String, String> {
    run("pactl", args)
}
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("timeout")
        .args(["--kill-after=1", "4", program])
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not contact the audio service: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else if matches!(out.status.code(), Some(124 | 137)) {
        Err("The audio service did not respond in time".into())
    } else {
        let error = String::from_utf8_lossy(&out.stderr);
        Err(format!("Audio service: {}", error.trim()))
    }
}
fn list(name: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&output(&["--format=json", "list", name])?)
        .map_err(|e| format!("Could not read {name}: {e}"))
}
pub fn snapshot() -> Result<Snapshot, String> {
    let info: serde_json::Value =
        serde_json::from_str(&output(&["--format=json", "info"])?).map_err(|e| e.to_string())?;
    let mut s = Snapshot {
        output: model::string(&info, "default_sink_name"),
        input: model::string(&info, "default_source_name"),
        ..Default::default()
    };
    for kind in [Kind::Output, Kind::Input, Kind::Playback, Kind::Recording] {
        s.nodes.extend(model::nodes(kind, list(kind.list())?)?);
    }
    s.cards = model::cards(list("cards")?)?;
    Ok(s)
}
pub fn apply(action: &Action) -> Result<(), String> {
    let args = action.args();
    for key in action.moves() {
        run(
            "pw-metadata",
            &key.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
    }
    if args.is_empty() {
        return Ok(());
    }
    output(&args.iter().map(String::as_str).collect::<Vec<_>>()).map(drop)
}

/// The subscription running, and whether the window has stopped it.
static MONITOR: Mutex<(Option<Child>, bool)> = Mutex::new((None, false));

/// Whether a `pactl subscribe` line is a change the window shows: devices,
/// streams, cards and the defaults, not clients connecting (as each of the
/// window's own pactl commands does).
fn shown(line: &str) -> bool {
    let facility = line
        .split(" on ")
        .nth(1)
        .and_then(|rest| rest.split(" #").next())
        .unwrap_or_default();
    matches!(
        facility,
        "sink" | "source" | "sink-input" | "source-output" | "card" | "server"
    )
}

/// Each change the audio service reports, as it happens: `pactl subscribe`
/// runs until stop_monitor, and `changed` is called per change. When the
/// service goes away, `changed` is called too (so the window says so), and
/// the subscription is started again every two seconds until it is back.
pub fn monitor(changed: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            let stdout = {
                let Ok(mut monitor) = MONITOR.lock() else {
                    return;
                };
                if monitor.1 {
                    return;
                }
                let spawned = Command::new("pactl")
                    .arg("subscribe")
                    .env("LC_ALL", "C")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();
                match spawned {
                    Ok(mut child) => {
                        let stdout = child.stdout.take();
                        monitor.0 = Some(child);
                        stdout
                    }
                    Err(_) => None,
                }
            };
            if let Some(stdout) = stdout {
                // Subscribed: read now, for a service that has just returned.
                changed();
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if shown(&line) {
                        changed();
                    }
                }
            }
            if let Ok(mut monitor) = MONITOR.lock()
                && let Some(mut child) = monitor.0.take()
            {
                let _ = child.wait();
            }
            changed();
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

pub fn stop_monitor() {
    if let Ok(mut monitor) = MONITOR.lock() {
        monitor.1 = true;
        if let Some(mut child) = monitor.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_use_explicit_objects_and_absolute_bounded_volumes() {
        let level = |volume, balance, sides: &[Side]| Level {
            volume,
            balance,
            sides: sides.to_vec(),
        };
        assert_eq!(
            Action::Volume(
                Kind::Input,
                42,
                level(240.0, 0.0, &[Side::Left, Side::Right])
            )
            .args(),
            ["set-source-volume", "42", "150%"]
        );
        assert_eq!(
            Action::Volume(
                Kind::Output,
                3,
                level(50.0, -0.5, &[Side::Left, Side::Right])
            )
            .args(),
            ["set-sink-volume", "3", "32768", "16384"]
        );
        // Mono has no balance to keep.
        assert_eq!(
            Action::Volume(Kind::Playback, 9, level(30.0, 0.4, &[Side::Other])).args(),
            ["set-sink-input-volume", "9", "30%"]
        );
        assert_eq!(
            Action::Mute(Kind::Recording, 8, true).args(),
            ["set-source-output-mute", "8", "1"]
        );
        // pipewire-pulse's keys, the device's serial first; -1 follows the
        // default.
        let key = |key: &str, value: &str| {
            ["-n", "default", "--", "88", key, value, "Spa:Id"]
                .map(str::to_owned)
                .to_vec()
        };
        let routed = Action::Move(88, Some((60, 354)));
        assert_eq!(
            routed.moves(),
            [key("target.object", "354"), key("target.node", "60")]
        );
        assert!(routed.args().is_empty());
        assert_eq!(
            Action::Move(88, None).moves(),
            [key("target.object", "-1"), key("target.node", "-1")]
        );
        assert!(Action::Mute(Kind::Output, 1, true).moves().is_empty());
        assert_eq!(
            Action::Default(Kind::Output, "USB; speaker".into()).args(),
            ["set-default-sink", "USB; speaker"]
        );
        let volume = Action::Volume(Kind::Output, 3, level(50.0, 0.0, &[]));
        assert!(volume.replaces(&Action::Volume(Kind::Output, 3, level(20.0, 0.0, &[]))));
        assert!(!volume.replaces(&Action::Volume(Kind::Input, 3, level(20.0, 0.0, &[]))));
        assert!(!volume.replaces(&Action::Mute(Kind::Output, 3, true)));
    }

    #[test]
    fn the_window_follows_devices_streams_and_defaults_not_clients() {
        assert!(shown("Event 'change' on sink #64"));
        assert!(shown("Event 'new' on sink-input #120"));
        assert!(shown("Event 'remove' on source-output #7"));
        assert!(shown("Event 'change' on server #4294967295"));
        assert!(shown("Event 'change' on card #50"));
        assert!(!shown("Event 'new' on client #143"));
        assert!(!shown("Event 'remove' on client #143"));
        assert!(!shown("Event 'new' on module #30"));
        assert!(!shown(""));
    }
}
