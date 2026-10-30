//! The engine playing real files (`tests/fixtures`, made by `make.sh`)
//! with no sound server and no window: audio taken at its rate by the
//! null output, frames shown at once by a detached video sink, decoded
//! in software. What's checked is what the interface relies on: what's
//! opened, events in order, the clock, seeking, pausing, subtitles, and
//! video stopping out of sight while audio plays on.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aizome_media::{AudioChoice, CueBody, Event, FramePath, Options, PlayState, Player};
use sidestep_ui::video::{VideoCaps, VideoSink};

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    p.to_string_lossy().into_owned()
}

struct Run {
    player: Player,
    rx: mpsc::Receiver<Event>,
    detached: std::sync::Arc<sidestep_ui::video::Detached>,
}

fn player() -> Run {
    let (sink, detached) = VideoSink::detached();
    let (tx, rx) = mpsc::channel();
    let caps = VideoCaps { known: true, shm: vec![0, 1], presentation: true, monotonic: true, ..VideoCaps::default() };
    let player = Player::new(
        Options { sink: Some(sink), audio: AudioChoice::Null, hardware: false, caps: Some(caps) },
        move |e| {
            let _ = tx.send(e);
        },
    );
    Run { player, rx, detached }
}

impl Run {
    /// Events until one matches, within `secs`.
    fn until(&self, secs: f64, mut f: impl FnMut(&Event) -> bool) -> Vec<Event> {
        let end = Instant::now() + Duration::from_secs_f64(secs);
        let mut seen = Vec::new();
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(e) => {
                    let hit = f(&e);
                    seen.push(e);
                    if hit {
                        return seen;
                    }
                }
                Err(_) => panic!("timed out; saw {seen:#?}"),
            }
        }
    }

    fn state(&self, secs: f64, want: PlayState) -> Vec<Event> {
        self.until(secs, |e| matches!(e, Event::State(s) if *s == want))
    }
}

#[test]
fn a_video_opens_plays_and_ends() {
    let r = player();
    r.player.open(fixture("clip.mp4"), 0.0, true);
    let seen = r.state(5.0, PlayState::Playing);
    let info = seen
        .iter()
        .find_map(|e| match e {
            Event::Opened(i) => Some(i.clone()),
            _ => None,
        })
        .expect("opened");
    assert_eq!(info.title, "Test Card");
    let d = info.duration.expect("a duration");
    assert!((d - 2.0).abs() < 0.1, "{d}");
    let v = info.video.as_ref().expect("video");
    assert_eq!((v.width, v.height), (160, 90));
    assert_eq!(v.codec, "h264");
    assert_eq!(info.audio.len(), 1);
    assert_eq!(info.audio[0].channels, Some(2));
    // The SubRip file beside it is found and shown.
    assert_eq!(info.subtitles.len(), 1);
    assert!(info.subtitles[0].file.is_some());
    assert!(seen.iter().any(|e| matches!(e, Event::Selected { audio: Some(0), subtitles: Some(0) })));
    let cues = seen.iter().find_map(|e| match e {
        Event::Cues(c) => Some(c.clone()),
        _ => None,
    });
    let cues = cues.expect("the file's cues");
    assert_eq!(cues.len(), 2);
    assert!(matches!(&cues[0].body, CueBody::Text { text, italic: true, .. } if text == "First line"));
    assert!((cues[1].start - 1.0).abs() < 1e-6 && (cues[1].end - 1.8).abs() < 1e-6);

    let started = Instant::now();
    r.state(6.0, PlayState::Ended);
    let took = started.elapsed().as_secs_f64();
    assert!((1.5..4.0).contains(&took), "played in {took} s");
    let s = r.player.stats();
    assert_eq!(s.path, FramePath::Software);
    // 50 frames, give or take the first (the poster) and a late one.
    assert!(s.shown >= 45 && s.shown <= 51, "{s:?}");
    assert!(r.detached.shown() >= 45);
    assert_eq!(r.detached.last_size(), Some((160, 90)));
    assert!((r.player.position() - 2.0).abs() < 0.15, "{}", r.player.position());
}

#[test]
fn paused_means_still_and_seeks_land_where_asked() {
    let r = player();
    r.player.open(fixture("clip.mp4"), 0.0, false);
    // Opened paused: the first frame shows, and nothing plays.
    r.until(5.0, |e| matches!(e, Event::Seeked(_)));
    r.state(1.0, PlayState::Paused);
    let shown = r.detached.shown();
    assert!(shown >= 1);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.player.position(), 0.0);
    assert_eq!(r.detached.shown(), shown);

    r.player.seek(1.2, true);
    r.until(3.0, |e| matches!(e, Event::Seeked(t) if (*t - 1.2).abs() < 1e-9));
    assert!((r.player.position() - 1.2).abs() < 1e-6);
    std::thread::sleep(Duration::from_millis(200));
    assert!((r.player.position() - 1.2).abs() < 1e-6, "still while paused");

    r.player.play();
    r.state(2.0, PlayState::Playing);
    std::thread::sleep(Duration::from_millis(300));
    let p = r.player.position();
    assert!(p > 1.3 && p < 1.8, "{p}");
    r.player.pause();
    r.state(2.0, PlayState::Paused);
    let at = r.player.position();
    std::thread::sleep(Duration::from_millis(300));
    assert!((r.player.position() - at).abs() < 1e-6, "paused at {at}");
    // It goes on from where it stopped.
    r.player.play();
    std::thread::sleep(Duration::from_millis(150));
    let p = r.player.position();
    assert!(p > at && p < at + 0.4, "{at} then {p}");
}

#[test]
fn video_stops_out_of_sight_and_catches_up() {
    let r = player();
    r.player.open(fixture("chaptered.mkv"), 0.0, true);
    r.state(5.0, PlayState::Playing);
    std::thread::sleep(Duration::from_millis(400));
    // Frames stop reaching the screen: decoding stops; sound goes on.
    r.detached.set_hidden(true);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !r.player.stats().suspended {
        assert!(Instant::now() < deadline, "never suspended: {:?}", r.player.stats());
        std::thread::sleep(Duration::from_millis(20));
    }
    let decoded = r.player.stats().decoded;
    let p = r.player.position();
    std::thread::sleep(Duration::from_millis(500));
    let s = r.player.stats();
    assert!(s.decoded <= decoded + 1, "decoded {decoded} then {}", s.decoded);
    assert!(r.player.position() > p + 0.3, "the clock went on");
    // In sight again: it shows the frame for now within a moment.
    r.detached.set_hidden(false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while r.player.stats().suspended || r.player.stats().decoded <= decoded + 2 {
        assert!(Instant::now() < deadline, "never caught up: {:?}", r.player.stats());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(r.player.stats().suspensions, 1);
    r.state(5.0, PlayState::Ended);
}

#[test]
fn the_window_saying_its_hidden_stops_video_at_once() {
    let r = player();
    r.player.open(fixture("chaptered.mkv"), 0.0, true);
    r.state(5.0, PlayState::Playing);
    std::thread::sleep(Duration::from_millis(200));
    r.player.set_visible(false);
    std::thread::sleep(Duration::from_millis(150));
    assert!(r.player.stats().suspended);
    r.player.set_visible(true);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!r.player.stats().suspended);
    r.state(5.0, PlayState::Ended);
}

#[test]
fn embedded_subtitles_chapters_and_tracks() {
    let r = player();
    r.player.open(fixture("chaptered.mkv"), 0.0, true);
    let seen = r.until(5.0, |e| matches!(e, Event::Cues(_)));
    let info = seen
        .iter()
        .find_map(|e| match e {
            Event::Opened(i) => Some(i.clone()),
            _ => None,
        })
        .expect("opened");
    assert_eq!(info.title, "Chaptered");
    assert_eq!(info.chapters.len(), 2);
    assert_eq!(info.chapters[1].title.as_deref(), Some("Closing"));
    assert!((info.chapters[1].start - 1.5).abs() < 1e-6);
    assert_eq!(info.audio[0].label(1), "English · Stereo");
    assert_eq!(info.subtitles.len(), 1);
    assert_eq!(info.subtitles[0].language.as_deref(), Some("fra"));
    assert!(info.subtitles[0].default);
    assert_eq!(info.video.as_ref().map(|v| v.codec.as_str()), Some("vp9"));
    let Some(Event::Cues(cues)) = seen.last() else { unreachable!() };
    assert!(matches!(&cues[0].body, CueBody::Text { text, .. } if text == "First line"));
    // Subtitles off, then on again: their cues come again.
    r.player.select_subtitles(None);
    r.until(2.0, |e| matches!(e, Event::Selected { subtitles: None, .. }));
    r.player.seek(0.0, true);
    r.player.select_subtitles(Some(0));
    r.until(3.0, |e| matches!(e, Event::Cues(_)));
}

#[test]
fn music_has_its_tags_and_cover_and_plays_without_video() {
    let r = player();
    r.player.open(fixture("song.flac"), 0.0, true);
    let seen = r.state(5.0, PlayState::Playing);
    let info = seen
        .iter()
        .find_map(|e| match e {
            Event::Opened(i) => Some(i.clone()),
            _ => None,
        })
        .expect("opened");
    assert_eq!(info.title, "Indigo");
    assert_eq!(info.artist.as_deref(), Some("The Dyers"));
    assert_eq!(info.album.as_deref(), Some("Aizome"));
    assert_eq!(info.year.as_deref(), Some("2026"));
    assert!(info.video.is_none(), "the cover isn't video");
    let cover = info.cover.as_ref().expect("a cover");
    assert_eq!(&cover[1..4], b"PNG");
    let pipeline = seen.iter().find_map(|e| match e {
        Event::Pipeline(p) => Some(p.clone()),
        _ => None,
    });
    assert_eq!(pipeline.and_then(|p| p.audio_decoder).as_deref(), Some("flac"));
    r.state(5.0, PlayState::Ended);
    assert_eq!(r.detached.shown(), 0);
    // Playing at the end starts again.
    r.player.play();
    r.state(3.0, PlayState::Playing);
    assert!(r.player.position() < 0.5);
}

#[test]
fn faster_plays_faster() {
    let r = player();
    r.player.set_speed(2.0);
    r.player.open(fixture("song.flac"), 0.0, true);
    r.state(5.0, PlayState::Playing);
    let started = Instant::now();
    r.state(5.0, PlayState::Ended);
    let took = started.elapsed().as_secs_f64();
    assert!(took < 1.6, "two seconds at 2× took {took} s");
}

#[test]
fn what_cant_be_opened_says_so() {
    let r = player();
    r.player.open(fixture("make.sh"), 0.0, true);
    r.until(5.0, |e| matches!(e, Event::Failed(_)));
    r.player.open("/nonexistent/film.mkv", 0.0, true);
    let seen = r.until(5.0, |e| matches!(e, Event::Failed(_)));
    let Some(Event::Failed(msg)) = seen.last() else { unreachable!() };
    assert!(msg.contains("No such file"), "{msg}");
}

#[test]
fn a_seek_to_the_last_moments_lands() {
    let r = player();
    r.player.open(fixture("chaptered.mkv"), 0.0, false);
    r.until(5.0, |e| matches!(e, Event::Seeked(_)));
    r.player.seek(1.0, true);
    r.player.seek(3.0, true);
    r.until(3.0, |e| matches!(e, Event::Seeked(t) if *t > 2.9));
}

#[test]
fn a_seek_is_where_playback_is_from_when_its_asked() {
    let r = player();
    r.player.open(fixture("clip.mp4"), 0.0, true);
    r.state(5.0, PlayState::Playing);
    std::thread::sleep(Duration::from_millis(300));
    // The player's thread hasn't had them yet: a seek relative to now,
    // asked next (two presses of → at once), starts from the last one's
    // target, not from the clock before it.
    r.player.seek(0.1, true);
    assert_eq!(r.player.position(), 0.1);
    r.player.seek(1.5, true);
    assert_eq!(r.player.position(), 1.5);
    r.until(3.0, |e| matches!(e, Event::Seeked(t) if (*t - 1.5).abs() < 1e-9));
    let p = r.player.position();
    assert!((1.5 - 1e-6..1.9).contains(&p), "{p}");
}

#[test]
fn the_picture_showing_can_be_taken() {
    let r = player();
    r.player.open(fixture("clip.mp4"), 0.0, false);
    r.until(5.0, |e| matches!(e, Event::Seeked(_)));
    let (tx, rx) = mpsc::channel();
    r.player.snapshot(move |p| {
        let _ = tx.send(p);
    });
    let pic = rx.recv_timeout(Duration::from_secs(3)).expect("an answer").expect("a picture");
    assert_eq!((pic.width, pic.height), (160, 90));
    assert_eq!(pic.rgba.len(), 160 * 90 * 4);
    // testsrc2's frame isn't one colour, and it's opaque.
    assert!(pic.rgba.chunks(4).any(|p| p[..3] != pic.rgba[..3]));
    assert!(pic.rgba.chunks(4).all(|p| p[3] == 255));
}
