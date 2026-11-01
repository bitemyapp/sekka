//! Real routing on the private daemon started by pipewire-output.sh.
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aizome_media::{AudioChoice, Event, Options, PlayState, Player, StreamKind};

fn dump() -> Vec<serde_json::Value> {
    let output = Command::new("pw-dump").output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn node(objects: &[serde_json::Value], name: &str) -> Option<u64> {
    objects.iter().find(|o| o["info"]["props"]["node.name"] == name).and_then(|o| o["id"].as_u64())
}

fn linked(target: &str) -> bool {
    let objects = dump();
    let (Some(source), Some(target)) = (node(&objects, "utsushioutputtest"), node(&objects, target)) else {
        return false;
    };
    objects.iter().any(|o| {
        let props = &o["info"]["props"];
        props["link.output.node"].as_u64() == Some(source) && props["link.input.node"].as_u64() == Some(target)
    })
}

#[track_caller]
fn wait(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !f() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for PipeWire routing: {:?}",
            dump()
                .into_iter()
                .find(|o| o["info"]["props"]["node.name"] == "utsushioutputtest")
                .map(|o| o["info"]["params"]["Props"].clone())
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn volume_is(muted: bool) -> bool {
    dump()
        .iter()
        .find(|o| o["info"]["props"]["node.name"] == "utsushioutputtest")
        .and_then(|o| o["info"]["params"]["Props"].as_array())
        .is_some_and(|params| {
            params.iter().any(|p| {
                p["mute"].as_bool() == Some(muted)
                    && p["channelVolumes"].as_array().is_some_and(|channels| {
                        !channels.is_empty()
                            && channels.iter().all(|v| v.as_f64().is_some_and(|v| (v - 0.35_f64.powi(3)).abs() < 0.001))
                    })
            })
        })
}

#[test]
#[ignore = "run tests/pipewire-output.sh; requires a private PipeWire/WirePlumber session"]
fn changing_outputs_routes_only_this_stream_and_preserves_playback() {
    assert_eq!(std::env::var("AIZOME_MEDIA_TEST_PIPEWIRE").as_deref(), Ok("1"));
    let (tx, rx) = mpsc::channel();
    let player = Player::new(
        Options {
            sink: None,
            hardware: false,
            caps: None,
            audio: AudioChoice::PipeWire(StreamKind {
                latency: 0.040,
                role: "Music",
                app_name: "UtsushiOutputTest".into(),
                app_id: "org.aizome.Utsushi.Test".into(),
            }),
        },
        move |event| {
            let _ = tx.send(event);
        },
    );
    let events_until = |predicate: &dyn Fn(&Event) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let event = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
            assert!(!matches!(&event, Event::Failed(_)), "{event:?}");
            if predicate(&event) {
                break;
            }
        }
    };
    events_until(&|e| {
        matches!(e, Event::AudioDevices(devices) if devices.len() == 2
        && devices.iter().any(|d| d.name == "utsushi-test-a")
        && devices.iter().any(|d| d.name == "utsushi-test-b"))
    });
    let media = std::env::var("AIZOME_MEDIA_TEST_AUDIO").unwrap();
    player.set_volume(0.35, false);
    player.open(media.clone(), 0.0, true);
    events_until(&|e| matches!(e, Event::State(PlayState::Playing)));
    wait(|| linked("utsushi-test-a"));
    wait(|| volume_is(false));
    wait(|| player.position() > 0.2);
    let before = player.position();
    player.set_audio_device(Some("utsushi-test-b".into()));
    wait(|| linked("utsushi-test-b"));
    wait(|| volume_is(false));
    wait(|| player.position() > before + 0.1);
    assert!(!linked("utsushi-test-a"));
    player.pause();
    events_until(&|e| matches!(e, Event::State(PlayState::Paused)));
    let paused = player.position();
    player.set_volume(0.35, true);
    player.set_audio_device(Some("utsushi-test-a".into()));
    events_until(&|e| matches!(e, Event::Pipeline(p) if p.audio_output.ends_with("utsushi-test-a")));
    std::thread::sleep(Duration::from_millis(250));
    wait(|| volume_is(true));
    assert!((player.position() - paused).abs() < 0.03, "switching resumed or sought paused playback");
    player.play();
    wait(|| linked("utsushi-test-a"));
    wait(|| player.position() > paused + 0.1);
    player.set_volume(0.35, false);
    wait(|| volume_is(false));
    player.set_audio_device(Some("utsushi-test-b".into()));
    wait(|| linked("utsushi-test-b"));
    // The preference applies to the next item too.
    player.open(media, 0.0, true);
    events_until(&|e| matches!(e, Event::Opened(_)));
    wait(|| linked("utsushi-test-b"));
    // Returning to System Default must undo the per-stream choice.
    player.set_audio_device(None);
    wait(|| linked("utsushi-test-a"));
    player.set_audio_device(Some("utsushi-test-b".into()));
    wait(|| linked("utsushi-test-b"));
    let id = node(&dump(), "utsushi-test-b").unwrap();
    assert!(Command::new("pw-cli").args(["destroy", &id.to_string()]).status().unwrap().success());
    events_until(
        &|e| matches!(e, Event::AudioDevices(devices) if devices.len() == 1 && devices[0].name == "utsushi-test-a"),
    );
    wait(|| linked("utsushi-test-a"));
    let before = player.position();
    wait(|| player.position() > before + 0.1);
}
