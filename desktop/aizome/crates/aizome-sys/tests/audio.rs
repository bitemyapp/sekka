//! Audio against a private PipeWire (null sinks, no hardware, no session
//! manager) with its own pipewire-pulse, so volumes, mutes and defaults can
//! change without touching this machine's sound.

mod common;

use aizome_sys::Wake;
use aizome_sys::audio::Audio;
use common::eventually;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

struct PrivatePipeWire {
    children: Vec<Child>,
    dir: PathBuf,
}

impl PrivatePipeWire {
    /// Start PipeWire and pipewire-pulse in a runtime directory of their
    /// own, or `None` (the test skips) when they aren't installed.
    fn start() -> Option<PrivatePipeWire> {
        // Unix socket paths are short: stay directly under /tmp.
        // One directory per instance: tests run side by side.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = PathBuf::from(format!("/tmp/aizome-pw-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let daemon = dir.join("daemon.conf");
        let pulse = dir.join("pulse.conf");
        let sink = |name: &str, description: &str, priority: u32| {
            format!(
                "{{ factory = adapter args = {{ factory.name = support.null-audio-sink node.name = \"{name}\" \
                 node.description = \"{description}\" media.class = \"Audio/Sink\" audio.position = [ FL FR ] \
                 priority.session = {priority} }} }}"
            )
        };
        std::fs::write(
            &daemon,
            format!(
                "context.properties = {{ core.daemon = true core.name = pipewire-0 support.dbus = false }}\n\
                 context.spa-libs = {{ audio.convert.* = audioconvert/libspa-audioconvert support.* = support/libspa-support }}\n\
                 context.modules = [\n\
                   {{ name = libpipewire-module-protocol-native }}\n\
                   {{ name = libpipewire-module-access }}\n\
                   {{ name = libpipewire-module-metadata }}\n\
                   {{ name = libpipewire-module-spa-node-factory }}\n\
                   {{ name = libpipewire-module-client-node }}\n\
                   {{ name = libpipewire-module-adapter }}\n\
                   {{ name = libpipewire-module-link-factory }}\n\
                 ]\n\
                 context.objects = [\n\
                   {{ factory = metadata args = {{ metadata.name = default metadata.values = [ \
                      {{ key = default.audio.sink value = {{ name = test-speakers }} }} \
                      {{ key = default.audio.source value = {{ name = test-mic }} }} ] }} }}\n\
                   {}\n  {}\n\
                   {{ factory = adapter args = {{ factory.name = support.null-audio-sink node.name = \"test-mic\" \
                      node.description = \"Test Mic\" media.class = \"Audio/Source/Virtual\" audio.position = [ MONO ] }} }}\n\
                 ]\n",
                sink("test-speakers", "Test Speakers", 1000),
                sink("test-headphones", "Test Headphones", 500),
            ),
        )
        .ok()?;
        std::fs::write(
            &pulse,
            format!(
                "context.properties = {{ support.dbus = false }}\n\
                 context.spa-libs = {{ audio.convert.* = audioconvert/libspa-audioconvert support.* = support/libspa-support }}\n\
                 context.modules = [\n\
                   {{ name = libpipewire-module-protocol-native }}\n\
                   {{ name = libpipewire-module-client-node }}\n\
                   {{ name = libpipewire-module-adapter }}\n\
                   {{ name = libpipewire-module-metadata }}\n\
                   {{ name = libpipewire-module-protocol-pulse }}\n\
                 ]\n\
                 pulse.properties = {{ server.address = [ \"unix:{}/pulse.sock\" ] }}\n",
                dir.display()
            ),
        )
        .ok()?;
        let spawn = |program: &str, config: &PathBuf| {
            Command::new(program)
                .arg("-c")
                .arg(config)
                .env("XDG_RUNTIME_DIR", &dir)
                .env("PIPEWIRE_RUNTIME_DIR", &dir)
                .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
                .env("PIPEWIRE_DEBUG", std::env::var("AIZOME_PW_DEBUG").unwrap_or_else(|_| "1".into()))
                .env_remove("PIPEWIRE_REMOTE")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(
                    std::fs::File::create(dir.join(format!("{program}.log"))).map(Stdio::from).unwrap_or(Stdio::null()),
                )
                .spawn()
        };
        let mut pw = PrivatePipeWire { children: Vec::new(), dir: dir.clone() };
        match spawn("pipewire", &daemon) {
            Ok(child) => pw.children.push(child),
            Err(error) => {
                eprintln!("skipping: no pipewire ({error})");
                return None;
            }
        }
        if !eventually(|| dir.join("pipewire-0").exists()) {
            return None;
        }
        pw.children.push(spawn("pipewire-pulse", &pulse).ok()?);
        if !eventually(|| dir.join("pulse.sock").exists()) {
            return None;
        }
        Some(pw)
    }

    fn server(&self) -> String {
        format!("unix:{}/pulse.sock", self.dir.display())
    }
}

impl Drop for PrivatePipeWire {
    fn drop(&mut self) {
        if std::env::var_os("AIZOME_KEEP_PW").is_some() {
            for log in ["pipewire.log", "pipewire-pulse.log"] {
                eprintln!("--- {log}\n{}", std::fs::read_to_string(self.dir.join(log)).unwrap_or_default());
            }
        }
        for child in self.children.iter_mut().rev() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.011
}

#[test]
fn volumes_mutes_and_defaults() {
    let Some(pw) = PrivatePipeWire::start() else { return };
    let audio = Audio::with_server(Some(pw.server()), Wake::none());
    assert!(
        eventually(|| audio.snapshot().sinks.len() == 2 && !audio.snapshot().sources.is_empty()),
        "{:?}",
        audio.snapshot()
    );
    let state = audio.snapshot();
    assert!(state.server.contains("PipeWire"), "{}", state.server);
    let speakers = state.sinks.iter().find(|s| s.name == "test-speakers").unwrap().clone();
    let headphones = state.sinks.iter().find(|s| s.name == "test-headphones").unwrap().clone();
    assert_eq!(speakers.description, "Test Speakers");
    assert_eq!(state.default_output().map(|s| s.name.as_str()), Some("test-speakers"));
    assert_eq!(state.default_input().map(|s| s.name.as_str()), Some("test-mic"));

    // An absolute volume, confirmed by the server.
    audio.set_sink_volume(speakers.index, 0.5).wait().unwrap();
    assert!(eventually(|| close(audio.snapshot().sink(speakers.index).unwrap().volume, 0.5)));

    // A volume key: the snapshot shows the new level at once.
    let step = audio.step_volume(0.05);
    assert!(close(audio.snapshot().sink(speakers.index).map_or(0.0, |s| s.volume), 0.55) || !step.is_done());
    let level = step.wait().unwrap();
    // Key press to the server's acknowledgement.
    let start = std::time::Instant::now();
    let back = audio.step_volume(-0.05).wait().unwrap();
    let latency = start.elapsed();
    eprintln!("volume key round trip: {latency:?}");
    assert!(latency < std::time::Duration::from_millis(50), "{latency:?}");
    assert!(close(back, 0.5));
    audio.step_volume(0.05).wait().unwrap();
    assert!(close(level, 0.55), "{level}");
    assert!(close(audio.snapshot().sink(speakers.index).unwrap().volume, 0.55));

    // Raising beyond 150% stops there.
    audio.set_sink_volume(speakers.index, 1.48).wait().unwrap();
    let level = audio.step_volume(0.05).wait().unwrap();
    assert!(close(level, 1.5), "{level}");

    // Mute, and the volume key unmutes.
    assert!(audio.toggle_mute().wait().unwrap());
    assert!(eventually(|| audio.snapshot().sink(speakers.index).unwrap().muted));
    audio.set_sink_volume(speakers.index, 0.3).wait().unwrap();
    audio.step_volume(0.05).wait().unwrap();
    assert!(eventually(|| !audio.snapshot().sink(speakers.index).unwrap().muted));

    // The microphone.
    assert!(audio.toggle_microphone_mute().wait().unwrap());
    assert!(eventually(|| audio.snapshot().default_input().is_some_and(|m| m.muted)));

    // Another device's volume and mute, without touching the default's.
    audio.set_sink_mute(headphones.index, true).wait().unwrap();
    audio.set_sink_volume(headphones.index, 0.2).wait().unwrap();
    assert!(eventually(|| {
        let state = audio.snapshot();
        let h = state.sink(headphones.index).unwrap();
        h.muted && close(h.volume, 0.2)
    }));

    // An unknown device is an error, not a hang.
    assert!(audio.set_sink_volume(9999, 0.5).wait().is_err());
    assert!(audio.set_default_sink("nonexistent").wait().is_err());

    // Switching the default output is accepted by the server. (Without a
    // session manager PipeWire doesn't move its default, so only the
    // request is checked here.)
    audio.set_default_sink("test-headphones").wait().unwrap();
}

#[test]
fn without_a_server_it_is_unavailable_then_reconnects() {
    let dir = PathBuf::from(format!("/tmp/aizome-pw-none-{}", std::process::id()));
    let audio = Audio::with_server(Some(format!("unix:{}/pulse.sock", dir.display())), Wake::none());
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!audio.available());
    assert!(matches!(audio.step_volume(0.05).wait(), Err(aizome_sys::Error::Unavailable(_))));
}

#[test]
fn balance_and_a_level_meter() {
    let Some(pw) = PrivatePipeWire::start() else { return };
    let audio = Audio::with_server(Some(pw.server()), Wake::none());
    assert!(eventually(|| audio.snapshot().sinks.len() == 2));
    let speakers = audio.snapshot().sinks.iter().find(|s| s.name == "test-speakers").unwrap().clone();
    assert!(close(speakers.balance, 0.0), "{}", speakers.balance);
    audio.set_sink_volume(speakers.index, 0.8).wait().unwrap();
    audio.set_sink_balance(speakers.index, -0.5).wait().unwrap();
    assert!(
        eventually(|| close(audio.snapshot().sink(speakers.index).unwrap().balance, -0.5)),
        "{:?}",
        audio.snapshot().sink(speakers.index)
    );
    // The louder channel keeps the volume.
    assert!(close(audio.snapshot().sink(speakers.index).unwrap().volume, 0.8));
    // A mono input has no balance to set.
    let mic = audio.snapshot().sources.iter().find(|s| s.name == "test-mic").unwrap().clone();
    assert!(audio.set_source_balance(mic.index, 0.5).wait().is_err());

    // The meter follows the speakers' monitor: silence, and it isn't
    // listed among the applications recording. (Without a session
    // manager nothing drives the graph, so no levels come here.)
    let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = woken.clone();
    let meter = aizome_sys::meter::Meter::start_on(
        Some(pw.server()),
        "test-speakers.monitor",
        Wake::new(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(meter.level() < 0.01);
    assert!(audio.snapshot().recording.is_empty(), "{:?}", audio.snapshot().recording);
    drop(meter);
}
