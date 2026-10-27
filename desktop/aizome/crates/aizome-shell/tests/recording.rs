//! Screen recordings in the shell, headless (no recorder runs: the shell
//! acts as if one started): the palette's commands, the systray's red dot
//! and its time, its tick repainting only itself, a click stopping it,
//! the picker's answer, one recording at a time, the Capture menu, and the
//! notification with the file.

mod common;

use std::time::Duration;

use aizome_ipc::{Event, Rect as IpcRect};
use aizome_shell::actions::Action;
use aizome_shell::ipc::IpcEvent;
use aizome_shell::recording::{Ask, Audio, Capture, Phase, Target};
use aizome_shell::shell::Msg;
use common::*;

fn record(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, target: Target, audio: Option<Audio>) {
    h.send(Msg::Run(Action::Record(Ask { target, audio })));
}

fn titles(h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>) -> Vec<String> {
    h.program().palette_items().into_iter().map(|i| i.title).collect()
}

#[test]
fn the_palette_offers_recordings_then_stop() {
    let mut h = scene(false);
    let t = titles(&h);
    for want in [
        "Record Screen",
        "Record Screen with Desktop Audio",
        "Record Screen with Microphone",
        "Record Area",
        "Record Area with Desktop Audio",
        "Record Area with Microphone",
    ] {
        assert!(t.iter().any(|x| x == want), "{want} in the palette");
    }
    assert!(!t.iter().any(|x| x == "Stop Recording"));
    record(&mut h, Target::Screen, None);
    let t = titles(&h);
    assert!(t.iter().any(|x| x == "Stop Recording"));
    assert!(!t.iter().any(|x| x.starts_with("Record ")), "no second recording: {t:?}");
}

#[test]
fn the_indicator_shows_the_time_and_a_click_stops_and_tells_where_the_file_is() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    assert!(h.find(bar, "Recording 0:00").is_none());
    record(&mut h, Target::Screen, Some(Audio { desktop: true, microphone: false }));
    {
        let r = h.program().recording.as_ref().expect("recording");
        assert_eq!(r.phase, Phase::Recording);
        let path = r.path.clone().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("screenrecording-") && name.ends_with(".mp4"), "{name}");
        assert!(path.parent().unwrap().ends_with("Videos/Recordings"), "{path:?}");
    }
    let dot = h.find(bar, "Recording 0:00").expect("the indicator");
    // It sits in the systray, left of the clock.
    let clock = h.find(bar, "21:42").unwrap();
    assert!(dot.x1 < clock.x0, "{dot:?} {clock:?}");
    // A second recording doesn't start.
    record(&mut h, Target::Pick, None);
    assert_eq!(h.program().recording.as_ref().unwrap().phase, Phase::Recording);
    // A click stops it, and the file is announced with Open and Show in Files.
    assert!(h.click_text(bar, "Recording 0:00"));
    assert!(h.program().recording.is_none());
    assert!(h.find(bar, "Recording 0:00").is_none());
    let notes = h.program().notes.unread("Screen Recording");
    let n = notes.first().expect("a notification");
    assert_eq!(n.summary, "Screen recording saved");
    assert!(n.body.starts_with("screenrecording-"));
    let labels: Vec<&str> = n.actions.iter().map(|(_, l)| l.as_str()).collect();
    assert!(labels.contains(&"Open") && labels.contains(&"Show in Files"), "{labels:?}");
}

#[test]
fn the_tick_repaints_only_the_indicator() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    record(&mut h, Target::Screen, None);
    h.take_damage(bar);
    // The indicator's clock is the wall clock's; its timer the harness's.
    std::thread::sleep(Duration::from_millis(1050));
    h.advance(Duration::from_millis(1010));
    assert!(h.find(bar, "Recording 0:01").is_some(), "{:?}", h.texts(bar));
    let damage = h.take_damage(bar);
    let area: f64 = damage.iter().map(|r| r.area()).sum();
    assert!(area > 0.0 && area < 50.0 * 24.0, "only the indicator: {damage:?}");
    let dot = h.find(bar, "Recording 0:01").unwrap().inflate(14.0, 4.0);
    assert!(damage.iter().all(|d| d.intersect(dot).area() > 0.0), "{damage:?} vs {dot:?}");
}

#[test]
fn record_area_asks_the_picker_and_records_what_it_picked() {
    let mut h = scene(false);
    // ⌥Print.
    h.send(Msg::Ipc(IpcEvent::Event(Event::Binding { action: "record".into(), pointer: None, output: None })));
    assert_eq!(h.program().recording.as_ref().unwrap().phase, Phase::Picking);
    let bar = window_titled(&h, "Menu bar");
    assert!(h.find(bar, "Recording 0:00").is_none(), "no indicator while picking");
    let picked = |area: Option<IpcRect>| {
        Msg::Ipc(IpcEvent::Event(Event::AreaPicked {
            tag: "record".into(),
            area,
            output: Some("eDP-1".into()),
            window: None,
            toplevel: None,
        }))
    };
    // Esc: nothing.
    h.send(picked(None));
    assert!(h.program().recording.is_none());
    // An area.
    h.send(Msg::Ipc(IpcEvent::Event(Event::Binding { action: "record".into(), pointer: None, output: None })));
    h.send(picked(Some(IpcRect::new(100, 120, 400, 300))));
    assert_eq!(h.program().recording.as_ref().unwrap().phase, Phase::Recording);
    assert!(h.find(bar, "Recording 0:00").is_some());
    // ⌥Print again stops it.
    h.send(Msg::Ipc(IpcEvent::Event(Event::Binding { action: "record".into(), pointer: None, output: None })));
    assert!(h.program().recording.is_none());
    // Picks for others aren't ours.
    assert_eq!(
        aizome_shell::recording::capture_for(IpcRect::new(0, 0, 1440, 900), Some("eDP-1"), &h.program().wm.outputs),
        Some(Capture::Output("eDP-1".into()))
    );
}

#[test]
fn the_capture_menu_has_screenshots_recordings_and_stop() {
    let mut h = scene(false);
    let labels = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| -> Vec<String> {
        h.program().capture_menu().items.iter().map(|e| e.label.clone()).collect()
    };
    let l = labels(&h);
    for want in ["Screenshot of the Screen", "Screenshot of an Area", "Record the Screen", "Record an Area"] {
        assert!(l.iter().any(|x| x == want), "{want} in {l:?}");
    }
    assert!(!l.iter().any(|x| x == "Stop Recording"));
    // ❖⇧5 opens it under the bar.
    let bar = window_titled(&h, "Menu bar");
    h.send(Msg::Ipc(IpcEvent::Event(Event::Binding {
        action: "screenshot-tool".into(),
        pointer: None,
        output: Some("eDP-1".into()),
    })));
    assert_eq!(h.popups(bar).len(), 1, "the Capture menu");
    record(&mut h, Target::Screen, None);
    assert_eq!(labels(&h)[0], "Stop Recording");
}

#[test]
fn pictures_of_the_indicator() {
    for day in [false, true] {
        let mut h = scene(day);
        record(&mut h, Target::Screen, None);
        let bar = window_titled(&h, "Menu bar");
        let name = if day { "recording-day.png" } else { "recording-night.png" };
        h.save_png(bar, shots().join(name), 2.0);
    }
}
