//! Pure timing/protocol tests; no model, microphone, network or audio device.
use std::path::PathBuf;
use std::sync::Arc;

use aizome_kit::testing::Harness;
use aizome_media::{Cue, CueBody, MediaInfo};
use aizome_utsushi::{
    Utsushi,
    app::Status,
    captions::{Event, MAX_CUES, Mode, Source, Timeline, Update},
    lyrics,
};

fn words(cue: &Cue) -> &str {
    let CueBody::Text { text, .. } = &cue.body else { panic!("text cue") };
    text
}

fn update(generation: u64, start: f64, end: f64) -> Update {
    Update { generation, event: Event::Cue { start, end, text: "caption".into(), speaker: None } }
}

#[test]
fn lrc_multi_timestamp_and_out_of_order_lines_are_synchronized() {
    let cues = lyrics::parse("[00:04.125]last\n[00:01.20][00:03.5]refrain\n[00:02]middle", 6.0).unwrap();
    assert_eq!(
        cues.iter().map(|c| (c.start, c.end, words(c))).collect::<Vec<_>>(),
        vec![(1.2, 2.0, "refrain"), (2.0, 3.5, "middle"), (3.5, 4.125, "refrain"), (4.125, 6.0, "last"),]
    );
}

#[test]
fn lrc_positive_global_offset_advances_and_clips_only_intersecting_lines() {
    let cues = lyrics::parse("[00:00]expired\n[00:01]first\n[00:02]second\n[offset:+1500]", 4.0).unwrap();
    assert_eq!(cues.len(), 2);
    assert_eq!((cues[0].start, cues[0].end, words(&cues[0])), (0.0, 0.5, "first"));
    assert_eq!((cues[1].start, cues[1].end), (0.5, 4.0));
}

#[test]
fn lrc_negative_offset_delays_and_malformed_offset_does_not_override() {
    let cues = lyrics::parse("[offset:-250]\n[00:01]line\n[offset:NaN]", 3.0).unwrap();
    assert_eq!((cues[0].start, cues[0].end), (1.25, 3.0));
}

#[test]
fn lrc_empty_timestamp_clears_and_simultaneous_lines_share_an_end() {
    let cues = lyrics::parse("[00:01]one\n[00:01]two\n[00:02]\n[00:03]last", 5.0).unwrap();
    assert_eq!(cues.iter().map(|c| (c.start, c.end)).collect::<Vec<_>>(), vec![(1.0, 2.0), (1.0, 2.0), (3.0, 5.0)]);
}

#[test]
fn lrc_last_line_has_a_finite_fallback_and_never_passes_media_end() {
    let cue = &lyrics::parse("[00:03]last", f64::NAN).unwrap()[0];
    assert_eq!((cue.start, cue.end), (3.0, 8.0));
    let cues = lyrics::parse("[00:01]first\n[00:09]past end", 4.0).unwrap();
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].end, 4.0);
    assert!(lyrics::parse("[00:04]at end", 4.0).is_err());
}

#[test]
fn lrc_metadata_bom_and_malformed_timestamps_are_safe() {
    let cues = lyrics::parse(
        concat!(
            "\u{feff}[ar:Artist]\n[ti:Title]\n[00:NaN]bad\n[00:60]bad\n[-1:00]bad\n",
            "[99999999999999999999:00]bad\n[00:01.1234]bad\n[broken\n[00:02.12]valid\r\n",
        ),
        5.0,
    )
    .unwrap();
    assert_eq!(cues.len(), 1);
    assert_eq!(words(&cues[0]), "valid");
    assert!(lyrics::parse("untimed lyrics\n[ar:artist]", 3.0).is_err());
}

#[test]
fn lrc_input_and_timestamp_expansion_are_bounded() {
    assert!(lyrics::parse(&"x".repeat(1024 * 1024 + 1), 10.0).is_err());
    let text = format!("{}{}\n", "[00:01]".repeat(200), "x".repeat(2000)).repeat(12);
    assert!(lyrics::parse(&text, 10.0).is_err());
}

#[test]
fn caption_protocol_keeps_absolute_time_and_optional_speaker() {
    let event: Event = serde_json::from_str(r#"{"type":"cue","start":301.25,"end":303.0,"text":"hello"}"#).unwrap();
    let Event::Cue { start, end, speaker, .. } = event else { panic!("cue") };
    assert_eq!((start, end, speaker), (301.25, 303.0, None));
    assert!(serde_json::from_str::<Event>(r#"{"type":"cue","start":"NaN"}"#).is_err());
}

#[test]
fn generation_change_discards_old_captions_without_affecting_authored_tracks() {
    let mut app = Utsushi::new();
    app.cues = lyrics::parse("[00:00]authored", 5.0).unwrap();
    let old = app.generated.generation;
    assert!(app.generated.insert(update(old, 0.0, 3.0), 0.0, false).unwrap());
    app.generated.reset();
    assert!(!app.generated.insert(update(old, 0.0, 3.0), 0.0, false).unwrap());
    assert!(app.generated.cues.is_empty());
    assert_eq!(words(&app.cues[0]), "authored");
}

#[test]
fn caption_queue_backpressures_instead_of_discarding_future_cues() {
    let mut timeline = Timeline::default();
    assert!(timeline.insert(update(0, 91.0, 92.0), 0.0, false).unwrap());
    assert!(!timeline.wants_more(0.0));
    assert!(timeline.wants_more(2.0));
    assert_eq!(timeline.cues[0].start, 91.0);
    timeline.reset();
    let generation = timeline.generation;
    for i in 0..MAX_CUES {
        timeline.insert(update(generation, i as f64 / 100.0, 3.0), 0.0, false).unwrap();
    }
    assert!(!timeline.wants_more(0.0));
    assert_eq!(timeline.cues.len(), MAX_CUES);
    timeline.prune(3.0);
    assert!(timeline.cues.is_empty());
    assert!(timeline.wants_more(3.0));
}

#[test]
fn captions_reject_invalid_or_expired_timing_and_sort_late_arrivals() {
    let mut timeline = Timeline::default();
    for (start, end) in [(f64::NAN, 3.0), (0.0, f64::INFINITY), (-1.0, 2.0), (2.0, 2.0)] {
        assert!(timeline.insert(update(0, start, end), 0.0, false).is_err());
    }
    assert!(!timeline.insert(update(0, 0.0, 2.0), 2.0, false).unwrap());
    timeline.insert(update(0, 4.0, 5.0), 0.0, false).unwrap();
    timeline.insert(update(0, 2.0, 3.0), 0.0, false).unwrap();
    assert_eq!(timeline.cues.iter().map(|c| c.start).collect::<Vec<_>>(), vec![2.0, 4.0]);
}

#[test]
fn speakers_are_labeled_as_detected_turns_only_when_requested() {
    let mut timeline = Timeline::default();
    let event = Update {
        generation: 0,
        event: Event::Cue { start: 0.0, end: 2.0, text: "hello".into(), speaker: Some("Turn 2".into()) },
    };
    timeline.insert(event.clone(), 0.0, false).unwrap();
    timeline.insert(event, 0.0, true).unwrap();
    assert_eq!(words(&timeline.cues[0]), "hello");
    assert_eq!(words(&timeline.cues[1]), "Turn 2 (detected turn): hello");
}

#[test]
fn lyric_timeline_has_exclusive_ends_and_seek_selects_the_correct_line() {
    let mut app = Utsushi::new();
    app.subtitle_source = Source::Lyrics(PathBuf::from("song.lrc"));
    app.lyric_cues = lyrics::parse("[00:01]first\n[00:02]last", 4.0).unwrap();
    for (position, expected) in
        [(0.9, None), (1.0, Some("first")), (2.0, Some("last")), (4.0, None), (1.5, Some("first"))]
    {
        app.cue_pos = position;
        assert_eq!(app.active_cues().first().map(|cue| words(cue)), expected);
    }
    app.sub_delay = 0.5;
    app.cue_pos = 2.0;
    assert_eq!(words(app.active_cues()[0]), "first");
    app.speed = 2.0;
    assert_eq!(words(app.active_cues()[0]), "first", "speed affects clock, not cue timestamps");
    app.subtitle_source = Source::Generated(Mode::Captions);
    app.generated.insert(update(0, 2.0, 3.0), 2.0, false).unwrap();
    assert_eq!(words(app.active_cues()[0]), "caption", "authored delay does not retime generated captions");
}

#[test]
fn audio_only_window_renders_lyrics_and_generated_turns() {
    for source in [Source::Lyrics(PathBuf::from("song.lrc")), Source::Generated(Mode::Turns)] {
        let mut app = Utsushi::new();
        app.null_audio = true;
        app.info = Some(Arc::new(MediaInfo { title: "Song".into(), duration: Some(4.0), ..MediaInfo::default() }));
        app.status = Status::Paused;
        app.cue_pos = 1.0;
        app.subtitle_source = source.clone();
        app.lyric_cues = lyrics::parse("[00:00]local lyric", 4.0).unwrap();
        app.generated
            .insert(
                Update {
                    generation: 0,
                    event: Event::Cue {
                        start: 0.0,
                        end: 3.0,
                        text: "generated speech".into(),
                        speaker: Some("Turn 1".into()),
                    },
                },
                0.0,
                true,
            )
            .unwrap();
        let harness = Harness::new(app);
        let texts = harness.texts(harness.program().window);
        let expected = if matches!(source, Source::Lyrics(_)) {
            "local lyric"
        } else {
            "Turn 1 (detected turn): generated speech"
        };
        assert!(texts.iter().any(|text| text == expected), "{texts:?}");
        assert!(harness.platform.videos.is_empty(), "no video needed for text captions");
    }
}
