//! Real routing on the private daemon started by pipewire-output.sh.
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aizome_kit::testing::Harness;
use aizome_media::{AudioChoice, Event, Options, PlayState, Player, StreamKind};
use aizome_utsushi::app::Status;
use aizome_utsushi::{Msg, Utsushi};

fn dump() -> Vec<serde_json::Value> {
    let output = Command::new("pw-dump").output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn node(objects: &[serde_json::Value], name: &str) -> Option<u64> {
    objects.iter().find(|o| o["info"]["props"]["node.name"] == name).and_then(|o| o["id"].as_u64())
}

fn command(program: &str, args: &[&str]) {
    let result = Command::new(program).args(args).output().unwrap();
    assert!(result.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&result.stderr));
}

fn create_sink(name: &str) {
    command(
        "pw-cli",
        &[
            "create-node",
            "adapter",
            &format!(
                "{{ factory.name = support.null-audio-sink node.name = {name} node.description = {name} media.class = Audio/Sink audio.position = [ FL FR ] object.linger = true }}"
            ),
        ],
    );
    wait(|| node(&dump(), name).is_some());
}

fn remove_sink(name: &str) {
    command("pw-cli", &["destroy", &node(&dump(), name).unwrap().to_string()]);
}

fn set_default(name: &str) {
    command("wpctl", &["set-default", &node(&dump(), name).unwrap().to_string()]);
}

#[track_caller]
fn app_wait(h: &mut Harness<Utsushi>, mut predicate: impl FnMut(&Utsushi) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        h.send_all(std::iter::empty());
        if predicate(h.program()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "status {:?}, route {:?}, error {:?}",
            h.program().status,
            h.program().audio_routed,
            h.program().error
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn routed(app: &Utsushi, name: &str) -> bool {
    app.audio_routed.iter().any(|device| device.name == name)
}

fn stays_paused(h: &mut Harness<Utsushi>) {
    let position = h.program().player.as_ref().unwrap().position();
    let until = Instant::now() + Duration::from_millis(1800);
    while Instant::now() < until {
        h.send_all(std::iter::empty());
        assert_eq!(h.program().status, Status::Paused);
        assert!((h.program().player.as_ref().unwrap().position() - position).abs() < 0.03);
        std::thread::sleep(Duration::from_millis(20));
    }
    let shown: u64 = h.platform.video_sinks.values().map(|(_, sink)| sink.shown()).sum();
    std::thread::sleep(Duration::from_millis(250));
    h.send_all(std::iter::empty());
    assert_eq!(
        h.platform.video_sinks.values().map(|(_, sink)| sink.shown()).sum::<u64>(),
        shown,
        "video continued after pausing"
    );
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

fn volume_is(volume: f64, muted: bool) -> bool {
    dump()
        .iter()
        .find(|o| o["info"]["props"]["node.name"] == "utsushioutputtest")
        .and_then(|o| o["info"]["params"]["Props"].as_array())
        .is_some_and(|params| {
            params.iter().any(|p| {
                p["mute"].as_bool() == Some(muted)
                    && p["channelVolumes"].as_array().is_some_and(|channels| {
                        !channels.is_empty()
                            && channels.iter().all(|v| v.as_f64().is_some_and(|v| (v - volume.powi(3)).abs() < 0.001))
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
    wait(|| volume_is(0.35, false));
    wait(|| player.position() > 0.2);
    // Changes made in the desktop mixer must survive ordinary playback
    // transitions, including changes made while the stream is inactive.
    let id = node(&dump(), "utsushioutputtest").unwrap();
    let set_controls = |props: &str| {
        let result = Command::new("pw-cli").args(["set-param", &id.to_string(), "Props", props]).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    };
    set_controls("{ mute: true, channelVolumes: [ 0.125, 0.125 ] }");
    wait(|| volume_is(0.5, true));
    player.pause();
    events_until(&|e| matches!(e, Event::State(PlayState::Paused)));
    std::thread::sleep(Duration::from_millis(250));
    assert!(volume_is(0.5, true), "pausing overwrote the desktop mixer's volume or mute");
    player.play();
    events_until(&|e| matches!(e, Event::State(PlayState::Playing)));
    let resumed = player.position();
    wait(|| player.position() > resumed + 0.1);
    assert!(volume_is(0.5, true), "resuming overwrote the desktop mixer's volume or mute");
    player.pause();
    events_until(&|e| matches!(e, Event::State(PlayState::Paused)));
    set_controls("{ mute: false, channelVolumes: [ 0.008, 0.008 ] }");
    wait(|| volume_is(0.2, false));
    player.play();
    events_until(&|e| matches!(e, Event::State(PlayState::Playing)));
    let resumed = player.position();
    wait(|| player.position() > resumed + 0.1);
    assert!(volume_is(0.2, false), "resuming overwrote controls changed while paused");
    set_controls("{ mute: true, channelVolumes: [ 0.125, 0.125 ] }");
    wait(|| volume_is(0.5, true));
    let before = player.position();
    player.set_audio_device(Some("utsushi-test-b".into()));
    wait(|| linked("utsushi-test-b"));
    wait(|| volume_is(0.5, true));
    wait(|| player.position() > before + 0.1);
    assert!(!linked("utsushi-test-a"));
    player.pause();
    events_until(&|e| matches!(e, Event::State(PlayState::Paused)));
    let paused = player.position();
    player.set_volume(0.35, true);
    player.set_audio_device(Some("utsushi-test-a".into()));
    events_until(&|e| matches!(e, Event::AudioRouted(devices) if devices.iter().any(|d| d.name == "utsushi-test-a")));
    std::thread::sleep(Duration::from_millis(250));
    wait(|| volume_is(0.35, true));
    assert!((player.position() - paused).abs() < 0.03, "switching resumed or sought paused playback");
    player.play();
    wait(|| linked("utsushi-test-a"));
    wait(|| player.position() > paused + 0.1);
    player.set_volume(0.35, false);
    wait(|| volume_is(0.35, false));
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
    events_until(&|e| matches!(e, Event::AudioOutputLost));
    wait(|| linked("utsushi-test-a"));
    let paused = player.position();
    std::thread::sleep(Duration::from_millis(1800));
    assert!((player.position() - paused).abs() < 0.03, "fallback resumed playback or enabled the null output");
    player.set_audio_device(None);
    wait(|| linked("utsushi-test-a"));
    std::thread::sleep(Duration::from_millis(250));
    assert!((player.position() - paused).abs() < 0.03, "selecting an output resumed after loss");
    player.play();
    wait(|| player.position() > paused + 0.1);
}

#[test]
#[ignore = "run tests/pipewire-output.sh; requires a private PipeWire/WirePlumber session"]
fn utsushi_mpris_reroutes_and_loss_leave_the_ui_and_transport_in_agreement() {
    assert_eq!(std::env::var("AIZOME_MEDIA_TEST_PIPEWIRE").as_deref(), Ok("1"));
    // The backend-only test removes B. Keep this test runnable alone too.
    if node(&dump(), "utsushi-test-b").is_none() {
        create_sink("utsushi-test-b");
    }
    set_default("utsushi-test-a");
    let mut app = Utsushi::new();
    app.state_dir = Some(std::path::PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap()).join("utsushi"));
    let mut h = Harness::new(app);
    app_wait(&mut h, |a| a.audio_devices.len() == 2);
    h.send(Msg::Open(vec![std::env::var("AIZOME_MEDIA_TEST_VIDEO").unwrap().into()]));
    app_wait(&mut h, |a| a.status == Status::Playing && routed(a, "utsushi-test-a"));
    let bus = zbus::blocking::Connection::session().unwrap();
    let proxy = zbus::blocking::Proxy::new(
        &bus,
        "org.mpris.MediaPlayer2.utsushi",
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .unwrap();
    app_wait(&mut h, |_| proxy.get_property::<String>("PlaybackStatus").as_deref() == Ok("Playing"));

    // A desktop mixer move is an intentional change, not a disappearance.
    let objects = dump();
    let source = node(&objects, "utsushi").unwrap();
    let target = objects.iter().find(|o| o["info"]["props"]["node.name"] == "utsushi-test-b").unwrap()["info"]["props"]
        ["object.serial"]
        .as_u64()
        .unwrap();
    command("pw-cli", &[
        "set-param", &source.to_string(), "Props", "{ volume: 0.5, mute: true, channelVolumes: [ 0.125, 0.008 ] }",
    ]);
    app_wait(&mut h, |a| a.muted && (a.volume - 0.0625_f32.cbrt()).abs() < 0.001);
    command("pw-metadata", &["-n", "default", &source.to_string(), "target.object", &target.to_string(), "Spa:Id"]);
    app_wait(&mut h, |a| routed(a, "utsushi-test-b"));
    assert_eq!(h.program().status, Status::Playing);
    assert!(!h.program().audio_output_lost);
    assert!(h.program().store.prefs.audio_device.is_none());

    // The system default can also change while both outputs remain present.
    h.send(Msg::AudioDevice(None));
    app_wait(&mut h, |a| routed(a, "utsushi-test-a"));
    set_default("utsushi-test-b");
    app_wait(&mut h, |a| routed(a, "utsushi-test-b"));
    assert_eq!(h.program().status, Status::Playing);
    assert!(h.program().muted);
    let stream = dump().into_iter().find(|o| o["info"]["props"]["node.name"] == "utsushi").unwrap();
    let volumes = &stream["info"]["params"]["Props"][0]["channelVolumes"];
    assert!(
        (stream["info"]["params"]["Props"][0]["volume"].as_f64().unwrap() - 0.5).abs() < 0.001,
        "routing reset the master volume",
    );
    assert!((volumes[0].as_f64().unwrap() - 0.125).abs() < 0.001);
    assert!((volumes[1].as_f64().unwrap() - 0.008).abs() < 0.001, "routing flattened channel balance");
    remove_sink("utsushi-test-a");
    app_wait(&mut h, |a| a.audio_devices.len() == 1);
    assert_eq!(h.program().status, Status::Playing, "unrelated output removal paused Utsushi");
    create_sink("utsushi-test-a");

    // Exercise the actual public MPRIS Pause method, including duplicates.
    proxy.call::<_, _, ()>("Pause", &()).unwrap();
    app_wait(&mut h, |a| a.status == Status::Paused);
    proxy.call::<_, _, ()>("Pause", &()).unwrap();
    h.send(Msg::AudioDevice(Some("utsushi-test-a".into())));
    app_wait(&mut h, |a| routed(a, "utsushi-test-a"));
    stays_paused(&mut h);
    proxy.call::<_, _, ()>("Play", &()).unwrap();
    app_wait(&mut h, |a| a.status == Status::Playing);

    // A default change CAUSED by loss must still pause. WirePlumber and
    // the backend may each request Pause, in either order.
    set_default("utsushi-test-a");
    h.send(Msg::AudioDevice(None));
    app_wait(&mut h, |a| routed(a, "utsushi-test-a"));
    remove_sink("utsushi-test-a");
    app_wait(&mut h, |a| a.status == Status::Paused && a.audio_output_lost && routed(a, "utsushi-test-b"));
    let window = h.program().window;
    assert!(h.texts(window).iter().any(|t| t == "Paused because the audio output became unavailable."));
    assert!(h.platform.videos.values().any(|place| place.below), "the loss notice must cover the video surface");
    if let Some(path) = std::env::var_os("AIZOME_MEDIA_TEST_LOSS_SCREENSHOT") {
        h.save_png(window, path, 1.0);
    }
    app_wait(&mut h, |_| proxy.get_property::<String>("PlaybackStatus").as_deref() == Ok("Paused"));
    stays_paused(&mut h);
    create_sink("utsushi-test-a");
    app_wait(&mut h, |a| a.audio_devices.len() == 2);
    stays_paused(&mut h);

    // Already paused: remember the unavailable preference, reconnect,
    // and choose another device without ever resuming implicitly.
    h.send(Msg::AudioDevice(Some("utsushi-test-b".into())));
    app_wait(&mut h, |a| routed(a, "utsushi-test-b"));
    remove_sink("utsushi-test-b");
    app_wait(&mut h, |a| a.audio_devices.len() == 1 && routed(a, "utsushi-test-a"));
    assert_eq!(h.program().store.prefs.audio_device.as_deref(), Some("utsushi-test-b"));
    stays_paused(&mut h);
    create_sink("utsushi-test-b");
    app_wait(&mut h, |a| a.audio_devices.len() == 2);
    stays_paused(&mut h);
    h.send(Msg::AudioDevice(Some("utsushi-test-a".into())));
    app_wait(&mut h, |a| routed(a, "utsushi-test-a"));
    stays_paused(&mut h);
    proxy.call::<_, _, ()>("Play", &()).unwrap();
    app_wait(&mut h, |a| a.status == Status::Playing && !a.audio_output_lost);
    // No fallback at all: keep both audio and video stopped, then recover
    // when an output reappears without silently starting playback.
    remove_sink("utsushi-test-b");
    remove_sink("utsushi-test-a");
    app_wait(&mut h, |a| a.status == Status::Paused && a.audio_devices.is_empty());
    stays_paused(&mut h);
    create_sink("utsushi-test-a");
    app_wait(&mut h, |a| routed(a, "utsushi-test-a"));
    stays_paused(&mut h);
}
