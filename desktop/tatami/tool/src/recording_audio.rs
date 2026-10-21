// SPDX-License-Identifier: MIT OR Apache-2.0
//! A recorder-owned PipeWire source. The mix lives in pw-cli, never in the
//! session daemon, so closing/killing its client removes every temporary node.
use crate::util;
use serde_json::{Value, json};
use std::{
    io::Write,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

pub struct SessionChild(pub Child, i32);

impl SessionChild {
    pub fn spawn(command: &mut Command, death_signal: i32) -> util::Result<Self> {
        let parent = unsafe { libc::getpid() };
        // SAFETY: only async-signal-safe libc calls between fork and exec.
        // Check the parent again to cover death before prctl installs the hook.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, death_signal) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
        command
            .spawn()
            .map(|child| Self(child, death_signal))
            .map_err(|e| e.to_string())
    }
}

impl Drop for SessionChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        if self.1 != libc::SIGKILL {
            // Give the recorder a bounded chance to finalize its container on
            // ordinary errors too, not only when stopped from the menu.
            unsafe {
                libc::kill(self.0.id() as i32, self.1);
            }
            for _ in 0..50 {
                if matches!(self.0.try_wait(), Ok(Some(_))) {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn graph() -> util::Result<Value> {
    serde_json::from_str(&util::output("pw-dump", &[])?)
        .map_err(|e| format!("Cannot read PipeWire devices: {e}"))
}

fn default_node(graph: &Value, kind: &str, class: &str) -> util::Result<(u64, u64)> {
    let objects = graph.as_array().ok_or("Invalid PipeWire graph")?;
    let name = objects
        .iter()
        .filter(|o| o["props"]["metadata.name"] == "default")
        .filter_map(|o| o["metadata"].as_array())
        .flatten()
        .find(|m| m["subject"] == 0 && m["key"] == format!("default.audio.{kind}"))
        .and_then(|m| m["value"]["name"].as_str())
        .ok_or_else(|| format!("No default audio {kind}"))?;
    objects
        .iter()
        .find(|o| {
            o["type"] == "PipeWire:Interface:Node"
                && o["info"]["props"]["node.name"] == name
                && o["info"]["props"]["media.class"] == class
        })
        .and_then(|o| {
            Some((
                o["id"].as_u64()?,
                o["info"]["props"]["object.serial"].as_u64()?,
            ))
        })
        .ok_or_else(|| format!("Default audio {kind} is unavailable"))
}

fn mix_config(name: &str, sink: u64, source: u64) -> Value {
    json!({
        "combine.mode": "source",
        "node.name": name,
        "node.description": "Tatami recording audio",
        "combine.latency-compensate": true,
        "combine.props": {"audio.position": ["FL", "FR"], "priority.session": 0, "node.virtual": true},
        "stream.rules": [
            {"matches": [{"object.serial": sink}], "actions": {"create-stream": {
                "node.name": format!("{name}_desktop"), "stream.capture.sink": true
            }}},
            {"matches": [{"object.serial": source}], "actions": {"create-stream": {
                "node.name": format!("{name}_microphone")
            }}}
        ]
    })
}

fn connected(graph: &Value, name: &str, sink: u64, source: u64) -> bool {
    let Some(objects) = graph.as_array() else {
        return false;
    };
    let node = |name: &str| {
        objects
            .iter()
            .find(|o| {
                o["type"] == "PipeWire:Interface:Node" && o["info"]["props"]["node.name"] == name
            })
            .and_then(|o| o["id"].as_u64())
    };
    node(name).is_some()
        && [(sink, "desktop"), (source, "microphone")]
            .iter()
            .all(|(from, suffix)| {
                node(&format!("{name}_{suffix}")).is_some_and(|to| {
                    objects.iter().any(|o| {
                        o["type"] == "PipeWire:Interface:Link"
                            && o["info"]["output-node-id"] == *from
                            && o["info"]["input-node-id"] == to
                    })
                })
            })
}

pub struct AudioMix {
    pub source: String,
    client: SessionChild,
    sink_id: u64,
    source_id: u64,
}

impl AudioMix {
    pub fn start() -> util::Result<Self> {
        let snapshot = graph()?;
        let (sink_id, sink_serial) = default_node(&snapshot, "sink", "Audio/Sink")?;
        let (source_id, source_serial) = default_node(&snapshot, "source", "Audio/Source")?;
        let source = format!("tatami_recording_{}", std::process::id());
        let mut client = SessionChild::spawn(
            Command::new("pw-cli")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
            libc::SIGKILL,
        )?;
        // One JSON argument, written to pw-cli's stdin; never through a shell.
        writeln!(
            client.0.stdin.as_mut().ok_or("No mixer input")?,
            "load-module libpipewire-module-combine-stream {}",
            mix_config(&source, sink_serial, source_serial)
        )
        .map_err(|e| format!("Cannot start audio mixer: {e}"))?;
        let mut mix = Self {
            source,
            client,
            sink_id,
            source_id,
        };
        for _ in 0..30 {
            if !mix.alive() {
                return Err("Recording audio mixer exited".into());
            }
            if connected(&graph()?, &mix.source, sink_id, source_id) {
                return Ok(mix);
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err("Cannot connect both desktop and microphone audio".into())
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.client.0.try_wait(), Ok(None))
    }

    pub fn healthy(&mut self) -> bool {
        self.alive()
            && graph().is_ok_and(|g| connected(&g, &self.source, self.sink_id, self.source_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_real_graph_links_are_required() {
        // The property names are from PipeWire 1.6.9's actual pw-dump output.
        let mut graph = json!([
            {"type":"PipeWire:Interface:Node","id":10,"info":{"props":{"node.name":"mix"}}},
            {"type":"PipeWire:Interface:Node","id":11,"info":{"props":{"node.name":"mix_desktop"}}},
            {"type":"PipeWire:Interface:Node","id":12,"info":{"props":{"node.name":"mix_microphone"}}},
            {"type":"PipeWire:Interface:Link","info":{"output-node-id":50,"input-node-id":11}},
            {"type":"PipeWire:Interface:Link","info":{"output-node-id":51,"input-node-id":12}}
        ]);
        assert!(connected(&graph, "mix", 50, 51));
        graph.as_array_mut().unwrap().pop();
        assert!(!connected(&graph, "mix", 50, 51));
    }
    #[test]
    fn missing_or_unavailable_defaults_are_errors() {
        assert!(default_node(&json!([]), "source", "Audio/Source").is_err());
        let metadata = json!([{"props":{"metadata.name":"default"},"metadata":[
            {"subject":0,"key":"default.audio.source","value":{"name":"gone"}}
        ]}]);
        assert!(default_node(&metadata, "source", "Audio/Source").is_err());
    }
    #[test]
    fn mixing_targets_exact_serials_and_keeps_default_priority_low() {
        let config = mix_config("tatami_test", 123, 456);
        assert_eq!(
            config["stream.rules"][0]["matches"][0]["object.serial"],
            123
        );
        assert_eq!(
            config["stream.rules"][1]["matches"][0]["object.serial"],
            456
        );
        assert_eq!(
            config["stream.rules"][0]["actions"]["create-stream"]["stream.capture.sink"],
            true
        );
        assert_eq!(config["combine.props"]["priority.session"], 0);
    }
}
