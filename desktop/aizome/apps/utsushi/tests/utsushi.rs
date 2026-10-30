//! Utsushi in the kit's headless harness, playing the engine's fixtures
//! for real (decoded in software, sound taken by the null output, frames
//! shown by a detached sink): what the window shows, where its video is
//! placed and whether it's under the window's drawing, the keys, full
//! screen's controls, subtitles in the letterbox, and a song.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::{Size, WindowKey, WindowState};
use aizome_utsushi::Utsushi;
use aizome_utsushi::app::{Msg, Status, VIDEO};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/aizome-media/tests/fixtures").join(name)
}

fn harness() -> Harness<Utsushi> {
    let mut app = Utsushi::new();
    app.null_audio = true;
    let dir = std::env::temp_dir().join(format!("utsushi-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.state_dir = Some(dir);
    Harness::new(app)
}

/// Take the player's messages until `f` holds, within `secs` on an idle
/// machine. The player's threads decode in real time, so a loaded one
/// (the workspace's tests all at once, or a busy build host) gets five
/// times as long; a pass returns as soon as `f` holds.
fn until(h: &mut Harness<Utsushi>, secs: f64, mut f: impl FnMut(&Utsushi) -> bool) {
    let end = Instant::now() + Duration::from_secs_f64(secs * 5.0);
    loop {
        h.send_all(std::iter::empty());
        if f(h.program()) {
            return;
        }
        let a = h.program();
        assert!(Instant::now() < end, "timed out: status {:?}, error {:?}, at {}", a.status, a.error, a.pos);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn window(h: &Harness<Utsushi>) -> WindowKey {
    h.program().window
}

/// Video `VIDEO`'s place: x, y, width, height, and under the drawing.
fn placed(h: &Harness<Utsushi>) -> Option<([f64; 4], bool)> {
    h.platform.videos.iter().find(|((_, v), _)| *v == VIDEO).map(|(_, p)| (p.bounds(), p.below))
}

#[test]
fn an_empty_window_offers_to_open() {
    let mut h = harness();
    let w = window(&h);
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Open a video or some music"), "{texts:?}");
    assert!(placed(&h).is_none());
    assert_golden(&mut h, w, "tests/golden/empty.png", 1.0);
}

#[test]
fn a_video_plays_with_its_picture_over_the_window() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("clip.mp4")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    assert_eq!(h.title(w), "Test Card");
    let (rect, below) = placed(&h).expect("the video placed");
    // 16:9 fitted in the stage, centred, on whole points.
    let stage = h.program().stage;
    assert!((rect[2] / rect[3] - 16.0 / 9.0).abs() < 0.02, "{rect:?} in {stage:?}");
    assert!(rect.iter().all(|v| v.fract() == 0.0));
    assert!(!below, "nothing covers it");
    until(&mut h, 5.0, |a| a.status == Status::Ended);
    assert!(h.platform.video_sinks.values().any(|(_, d)| d.shown() >= 40));
}

#[test]
fn subtitles_go_over_the_picture_or_into_the_letterbox() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("clip.mp4")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing && !a.cues.is_empty());
    // The first cue shows from 0.2 s.
    until(&mut h, 3.0, |a| a.player.as_ref().is_some_and(|p| p.position() > 0.35));
    h.send(Msg::CueTick);
    assert!(h.texts(w).iter().any(|t| t == "First line"), "{:?}", h.texts(w));
    // The window is about as wide as the picture: the cue is over it, so
    // the video goes under the window's drawing.
    assert_eq!(placed(&h).map(|p| p.1), Some(true));
    // A tall window: the cue fits in the bar under the picture, which
    // stays over everything.
    h.resize(w, Size::new(640.0, 900.0));
    h.send(Msg::CueTick);
    assert!(h.texts(w).iter().any(|t| t == "First line"));
    assert_eq!(placed(&h).map(|p| p.1), Some(false));
}

#[test]
fn full_screen_controls_cover_the_picture_until_they_hide() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    h.window_state(w, WindowState { fullscreen: true, maximized: false, suspended: false });
    assert!(h.program().controls);
    assert_eq!(placed(&h).map(|p| p.1), Some(true), "the controls are over the picture");
    h.advance(Duration::from_secs(3));
    assert!(!h.program().controls);
    assert_eq!(placed(&h).map(|p| p.1), Some(false), "back over everything");
    // The pointer brings them back.
    h.send(Msg::Pointer);
    assert!(h.program().controls);
    assert_eq!(placed(&h).map(|p| p.1), Some(true));
}

const FULL_SCREEN: WindowState = WindowState { fullscreen: true, maximized: false, suspended: false };
const WINDOWED: WindowState = WindowState { fullscreen: false, maximized: false, suspended: false };

/// A video playing in full screen, its controls gone after the pointer
/// rested.
fn full_screen_with_the_controls_gone() -> (Harness<Utsushi>, WindowKey) {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    h.window_state(w, FULL_SCREEN);
    h.advance(Duration::from_secs(3));
    assert!(!h.program().controls);
    (h, w)
}

/// The controls that came back went again without the pointer moving,
/// and the picture is over everything again.
fn controls_went_again(h: &mut Harness<Utsushi>, after: &str) {
    assert!(h.program().controls, "the controls show {after}");
    h.advance(Duration::from_secs(3));
    assert!(!h.program().controls, "the controls stayed {after}");
    assert_eq!(placed(h).map(|p| p.1), Some(false), "the picture under the drawing {after}");
}

#[test]
fn full_screen_controls_go_again_after_a_pause() {
    let (mut h, _) = full_screen_with_the_controls_gone();
    // As the menus and the desktop's media controls pause.
    h.send(Msg::Toggle);
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    h.advance(Duration::from_secs(3));
    assert!(h.program().controls, "paused, the controls stay");
    h.send(Msg::Toggle);
    until(&mut h, 2.0, |a| a.status == Status::Playing);
    controls_went_again(&mut h, "after playing resumed");
}

#[test]
fn full_screen_controls_go_again_after_space_and_space() {
    // No pointer over the window: the keys go to the picture, which kept
    // the keyboard into full screen.
    let (mut h, w) = full_screen_with_the_controls_gone();
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Playing);
    controls_went_again(&mut h, "after Space resumed");
}

#[test]
fn full_screen_controls_go_again_after_another_desk() {
    let (mut h, w) = full_screen_with_the_controls_gone();
    h.window_state(w, WindowState { suspended: true, ..FULL_SCREEN });
    h.window_state(w, FULL_SCREEN);
    controls_went_again(&mut h, "after the window came back");
}

#[test]
fn the_picture_keeps_the_keyboard_through_full_screen() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"));
    // By the keyboard alone, with no pointer over the window.
    h.type_text(w, "f");
    assert_eq!(h.fullscreen_asked(w), Some(true));
    h.window_state(w, FULL_SCREEN);
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"), "in full screen");
    let stage = h.find_id(w, "stage").expect("the picture");
    assert!((stage.height() - h.size(w).height).abs() < 0.5, "the picture fills the screen: {stage:?}");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.fullscreen_asked(w), Some(false), "Escape leaves full screen");
    h.window_state(w, WINDOWED);
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"), "back in the window");
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Paused);
}

#[test]
fn keys_play_pause_and_seek() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    h.named(w, NamedKey::Home, Modifiers::NONE);
    until(&mut h, 2.0, |a| a.pos == 0.0);
    // ⇧→ is a second; → five (past the end of this one: just before it).
    h.named(w, NamedKey::ArrowRight, Modifiers::SHIFT);
    until(&mut h, 2.0, |a| (a.pos - 1.0).abs() < 0.01);
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    until(&mut h, 2.0, |a| a.pos > 2.9);
    h.named(w, NamedKey::Home, Modifiers::NONE);
    until(&mut h, 2.0, |a| a.pos == 0.0);
    // Chapters: Page Down goes to the next.
    h.named(w, NamedKey::PageDown, Modifiers::NONE);
    until(&mut h, 2.0, |a| (a.pos - 1.5).abs() < 0.01);
    h.type_text(w, "k");
    until(&mut h, 2.0, |a| a.status == Status::Playing);
}

#[test]
fn hidden_windows_stop_decoding_video() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    h.window_state(w, WindowState { fullscreen: false, maximized: false, suspended: true });
    until(&mut h, 2.0, |a| a.player.as_ref().is_some_and(|p| p.stats().suspended));
    h.window_state(w, WindowState { fullscreen: false, maximized: false, suspended: false });
    until(&mut h, 2.0, |a| a.player.as_ref().is_some_and(|p| !p.stats().suspended));
}

#[test]
fn a_song_shows_its_cover_and_who_made_it() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("song.flac")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing && a.cover.is_some());
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Indigo"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "The Dyers — Aizome · 2026"), "{texts:?}");
    assert!(placed(&h).is_none(), "no video for a song");
    h.send(Msg::Pause);
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    h.send(Msg::SeekTo(0.0));
    until(&mut h, 2.0, |a| a.pos == 0.0);
    assert_golden(&mut h, w, "tests/golden/song.png", 1.0);
    // In a window sized for a small video the cover shrinks; what it is
    // stays in sight above the controls.
    h.resize(w, Size::new(480.0, 224.0));
    let title = h.find(w, "Indigo").expect("the title shows");
    assert!(title.y1 <= 224.0 - aizome_utsushi::ui::BAR, "{title:?}");
    assert!(h.find(w, "The Dyers — Aizome · 2026").is_some());
}

#[test]
fn media_info_says_how_it_plays() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("clip.mp4")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing && a.pipeline.video_decoder.is_some());
    h.command(w, 'i');
    let sheet = h.program().sheet.expect("the info sheet");
    let texts = h.texts(sheet);
    for want in ["Video decoding", "h264 in software", "Frames to the screen", "No audio output"] {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
}

#[test]
fn a_folder_plays_in_order_and_goes_on_to_the_next() {
    let mut h = harness();
    let dir = std::env::temp_dir().join(format!("utsushi-folder-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    for (from, to) in [("song.flac", "Track 10.flac"), ("song.flac", "Track 2.flac"), ("clip.srt", "notes.srt")] {
        let _ = std::fs::copy(fixture(from), dir.join(to));
    }
    h.send(Msg::Open(vec![dir.clone()]));
    let names: Vec<String> =
        h.program().queue.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Track 2.flac", "Track 10.flac"]);
    until(&mut h, 8.0, |a| a.current == Some(1) && a.status == Status::Playing);
    let _ = std::fs::remove_dir_all(dir);
}
