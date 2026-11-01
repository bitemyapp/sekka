//! Utsushi in the kit's headless harness, playing the engine's fixtures
//! for real (decoded in software, sound taken by the null output, frames
//! shown by a detached sink): what the window shows, where its video is
//! placed and whether it's under the window's drawing, the keys, full
//! screen's controls, subtitles in the letterbox, and a song.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::{Point, Size, WindowKey, WindowState};
use aizome_utsushi::Utsushi;
use aizome_utsushi::app::{Msg, Status, VIDEO};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/aizome-media/tests/fixtures").join(name)
}

fn harness() -> Harness<Utsushi> {
    let mut app = Utsushi::new();
    app.null_audio = true;
    let dir =
        std::env::temp_dir().join(format!("utsushi-test-{}-{:?}", std::process::id(), std::thread::current().id()));
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
    // The clip's subtitles go over the picture from 0.2 s (the next test
    // shows them), and on a loaded machine one is up by the time it plays.
    play_without_subtitles(&mut h, "clip.mp4");
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

/// Fixture `name` playing, with its subtitles off: a cue over the picture
/// puts it under the drawing whatever the window and its controls do, and
/// when one shows depends on how fast the machine is (on a loaded one, a
/// cue had come up by the time a test looked, or the controls went).
fn play_without_subtitles(h: &mut Harness<Utsushi>, name: &str) {
    h.send(Msg::Open(vec![fixture(name)]));
    until(h, 5.0, |a| a.status == Status::Playing);
    h.send(Msg::Subtitles(None));
    until(h, 2.0, |a| a.sub_track.is_none());
    assert!(h.program().cues.is_empty(), "no cues with the subtitles off");
}

#[test]
fn full_screen_controls_cover_the_picture_until_they_hide() {
    let mut h = harness();
    let w = window(&h);
    play_without_subtitles(&mut h, "chaptered.mkv");
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
    play_without_subtitles(&mut h, "chaptered.mkv");
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

/// The pointer moves over the picture, which shows the full-screen
/// controls, then straight onto `name` in them.
fn pointer_onto(h: &mut Harness<Utsushi>, w: WindowKey, name: &str) {
    h.move_to(w, Point::new(100.0, 100.0));
    assert!(h.program().controls, "a move over the picture shows them");
    let at = h.find_id(w, name).unwrap_or_else(|| panic!("{name} in the controls")).center();
    h.move_to(w, at);
}

#[test]
fn full_screen_controls_go_after_the_pointer_leaves_a_control_for_the_picture() {
    let (mut h, w) = full_screen_with_the_controls_gone();
    pointer_onto(&mut h, w, "volume");
    h.advance(Duration::from_secs(3));
    assert!(h.program().controls, "they stay while the pointer is over them");
    // Off the volume onto the picture in one move, not crossing the bar's
    // own background: the bar never hears the pointer leave it.
    h.move_to(w, Point::new(100.0, 100.0));
    controls_went_again(&mut h, "after the pointer went from a control to the picture");
}

#[test]
fn full_screen_controls_go_after_the_pointer_leaves_the_window_from_a_control() {
    // To another output, say.
    let (mut h, w) = full_screen_with_the_controls_gone();
    pointer_onto(&mut h, w, "volume");
    h.advance(Duration::from_secs(3));
    assert!(h.program().controls, "they stay while the pointer is over them");
    h.leave(w);
    controls_went_again(&mut h, "after the pointer left the window from a control");
}

#[test]
fn full_screen_controls_stay_while_the_pointer_rests_on_the_timeline() {
    // Reading the times along it: the timeline is part of the controls.
    let (mut h, w) = full_screen_with_the_controls_gone();
    pointer_onto(&mut h, w, "timeline");
    h.advance(Duration::from_secs(3));
    assert!(h.program().controls, "they stay while the pointer is over the timeline");
    h.move_to(w, Point::new(100.0, 100.0));
    controls_went_again(&mut h, "after the pointer went from the timeline to the picture");
}

#[test]
fn full_screen_controls_go_after_full_screen_left_with_the_pointer_on_them() {
    // Escape and F with the pointer resting where the volume was: the
    // floating bar went with the pointer on it, and heard nothing.
    let (mut h, w) = full_screen_with_the_controls_gone();
    pointer_onto(&mut h, w, "volume");
    h.send(Msg::Escape);
    h.window_state(w, WINDOWED);
    h.send(Msg::ToggleFullscreen);
    h.window_state(w, FULL_SCREEN);
    controls_went_again(&mut h, "after full screen came again");
}

#[test]
fn full_screen_controls_stay_when_the_pointer_comes_straight_back_onto_them() {
    // Off a control onto the picture and straight back onto one, within
    // the 2.5 s: a button, then the volume.
    let (mut h, w) = full_screen_with_the_controls_gone();
    for name in ["tracks", "volume"] {
        pointer_onto(&mut h, w, name);
        h.advance(Duration::from_secs(3));
        assert!(h.program().controls, "they stay while the pointer is over {name}");
        h.move_to(w, Point::new(100.0, 100.0));
        let at = h.find_id(w, name).expect("the control").center();
        h.move_to(w, at);
        h.advance(Duration::from_secs(3));
        assert!(h.program().controls, "they went from under the pointer back on {name}");
    }
}

#[test]
fn full_screen_controls_stay_while_their_menu_is_open() {
    use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
    let (mut h, w) = full_screen_with_the_controls_gone();
    pointer_onto(&mut h, w, "tracks");
    let at = h.find_id(w, "tracks").expect("tracks").center();
    h.click(w, at);
    assert_eq!(h.popups(w).len(), 1, "Subtitles and Audio opened");
    // Into the menu, a surface of its own: the pointer left the window.
    h.leave(w);
    h.advance(Duration::from_secs(3));
    assert!(h.program().controls, "the controls stay under their menu");
    // Another in its place: the first closes once the second has opened.
    h.send(Msg::TracksMenu);
    let popups = h.popups(w);
    assert_eq!(popups.len(), 1, "one in its place: {popups:?}");
    h.advance(Duration::from_secs(3));
    assert!(h.program().controls, "the controls stay under the next menu");
    h.popup_keyboard(popups[0].0, Key::Named(NamedKey::Escape), Modifiers::NONE);
    assert!(h.popups(w).is_empty(), "Escape closed it");
    controls_went_again(&mut h, "after their menu closed");
}

#[test]
fn the_keys_stay_with_the_picture_after_the_volume() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    let vol = h.find_id(w, "volume").expect("the volume");
    h.click(w, Point::new(vol.x0 + vol.width() * 0.25, vol.center().y));
    assert!(h.program().volume < 0.5, "the click set the volume: {}", h.program().volume);
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"), "after a click on the volume");
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Playing);
    // In full screen, with the pointer then off the window.
    h.window_state(w, FULL_SCREEN);
    let vol = h.find_id(w, "volume").expect("the volume over the picture");
    h.click(w, vol.center());
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"), "after a click on the volume in full screen");
    h.leave(w);
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Paused);
}

#[test]
fn the_keys_come_back_to_the_picture_from_the_bar() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    let in_the_bar = |h: &Harness<Utsushi>| {
        let bottom = h.size(w).height;
        h.focused_rect(w).is_some_and(|r| r.y0 >= bottom - aizome_utsushi::ui::BAR - 24.0 - 1.0 && r.y1 <= bottom)
    };
    // Tab into the bar, then full screen (as from the View menu): the bar
    // that had the keys is gone, and they're the picture's.
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    assert!(in_the_bar(&h), "Tab went into the bar: {:?}", h.focused_rect(w));
    h.send(Msg::ToggleFullscreen);
    h.window_state(w, FULL_SCREEN);
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"), "into full screen");
    // And out of it from the floating bar.
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    assert!(in_the_bar(&h), "Tab went into the floating bar: {:?}", h.focused_rect(w));
    h.send(Msg::Escape);
    h.window_state(w, WINDOWED);
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"), "out of full screen");
    // The floating bar going as the pointer rests.
    h.send(Msg::ToggleFullscreen);
    h.window_state(w, FULL_SCREEN);
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    assert!(in_the_bar(&h), "Tab went into the floating bar again: {:?}", h.focused_rect(w));
    h.advance(Duration::from_secs(3));
    assert!(!h.program().controls);
    assert_eq!(h.focused_name(w).as_deref(), Some("stage"), "as the controls went");
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Paused);
}

#[test]
fn seeks_in_a_row_add_up() {
    // → pressed quickly, or held: each seek goes on from where the last
    // one goes, though the player hasn't got there yet.
    let mut h = harness();
    h.send(Msg::Open(vec![fixture("chaptered.mkv")]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    h.send(Msg::Pause);
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    h.send_all([Msg::SeekTo(0.0), Msg::SeekBy(1.0)]);
    assert_eq!(h.program().pos, 1.0);
    h.send_all([Msg::SeekBy(0.5), Msg::SeekBy(0.5)]);
    assert_eq!(h.program().pos, 2.0);
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
fn media_info_and_file_panel_cover_video_without_subtitles() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};

    let mut h = harness();
    let w = window(&h);
    play_without_subtitles(&mut h, "clip.mp4");
    h.send(Msg::Pause);
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    assert_eq!(placed(&h).map(|p| p.1), Some(false));
    h.command(w, 'i');
    assert!(h.program().sheet.is_some());
    assert_eq!(placed(&h).map(|p| p.1), Some(true), "Media Info must cover the native video surface");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().sheet.is_none());
    assert_eq!(placed(&h).map(|p| p.1), Some(false));
    h.command(w, 'o');
    assert_eq!(placed(&h).map(|p| p.1), Some(true), "the file panel must cover the native video surface");
    assert!(h.click_text(w, "Cancel"));
    assert_eq!(placed(&h).map(|p| p.1), Some(false));
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

#[test]
fn stage_recent_button_reopens_the_video_instead_of_opening_a_file_panel() {
    let mut h = harness();
    let w = window(&h);
    play_without_subtitles(&mut h, "clip.mp4");
    h.send(Msg::Close);
    assert_eq!(h.program().status, Status::Empty);
    let recent = h.texts(w).into_iter().find(|t| t.starts_with("Test Card")).expect("recent video button");
    assert!(h.click_text(w, &recent));
    assert_eq!(h.program().current_path(), Some(fixture("clip.mp4").as_path()));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
}

#[test]
fn stage_failed_media_next_button_plays_the_next_item() {
    let mut h = harness();
    let w = window(&h);
    h.send(Msg::Open(vec![fixture("missing.mp4"), fixture("clip.mp4")]));
    until(&mut h, 5.0, |a| a.status == Status::Failed);
    assert!(h.click_text(w, "Play the Next"));
    assert_eq!(h.program().current, Some(1));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
}

#[test]
fn stage_resumed_start_over_button_receives_the_pointer() {
    let mut h = harness();
    let w = window(&h);
    play_without_subtitles(&mut h, "clip.mp4");
    h.send(Msg::Pause);
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    // The short decoder fixture cannot produce the long-film resume notice;
    // present the same view state while retaining real playback and input.
    h.program_mut().resumed = Some(1.0);
    h.send(Msg::InfoTick);
    assert!(h.click_text(w, "Start Over"));
    assert!(h.program().resumed.is_none());
    assert_eq!(h.program().status, Status::Paused, "the button must not toggle playback underneath");
}

#[test]
fn stage_music_queue_row_receives_double_click() {
    use aizome_kit::sidestep_ui::PointerButton;

    let mut h = harness();
    let w = window(&h);
    let second = h.program().state_dir.as_ref().unwrap().join("second.flac");
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::copy(fixture("song.flac"), &second).unwrap();
    h.send(Msg::Open(vec![fixture("song.flac"), second]));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
    h.send(Msg::Pause);
    until(&mut h, 2.0, |a| a.status == Status::Paused);
    let at = h.find(w, "second").expect("second queue row").center();
    h.press(w, at, PointerButton::Left, 2);
    h.release(w, at, PointerButton::Left);
    assert_eq!(h.program().current, Some(1));
    until(&mut h, 5.0, |a| a.status == Status::Playing);
}

#[test]
fn stage_click_focuses_without_changing_playback() {
    use aizome_kit::sidestep_ui::PointerButton;

    let mut h = harness();
    let w = window(&h);
    play_without_subtitles(&mut h, "chaptered.mkv");
    let at = Point::new(100.0, 100.0);
    for paused in [false, true] {
        if paused {
            h.send(Msg::Toggle);
            until(&mut h, 2.0, |a| a.status == Status::Paused);
        }
        let state = h.program().status;
        h.focus_window(w, false);
        h.focus_window(w, true);
        h.click(w, at);
        assert_eq!(h.focused_name(w).as_deref(), Some("stage"));
        // Drain decoder replies: neither click may enqueue a toggle.
        std::thread::sleep(Duration::from_millis(80));
        h.send_all(std::iter::empty());
        assert_eq!(h.program().status, state);
        h.press(w, at, PointerButton::Left, 2);
        h.release(w, at, PointerButton::Left);
        assert_eq!(h.fullscreen_asked(w), Some(true));
        std::thread::sleep(Duration::from_millis(80));
        h.send_all(std::iter::empty());
        assert_eq!(h.program().status, state);
    }
    h.type_text(w, " ");
    until(&mut h, 2.0, |a| a.status == Status::Playing);
}
