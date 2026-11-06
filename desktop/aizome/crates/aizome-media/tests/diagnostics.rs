//! The real C variadic callback, with a logger confined to this test process.
use std::sync::Mutex;

use aizome_media::av::{self, ff};

struct Logger(Mutex<Vec<(log::Level, String)>>);
static LOGGER: Logger = Logger(Mutex::new(Vec::new()));

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.0.lock().unwrap().push((record.level(), record.args().to_string()));
        }
    }
    fn flush(&self) {}
}

#[test]
fn decoder_diagnostics_are_opt_in_but_fatal_errors_remain_visible() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Error);
    av::quiet();
    // SAFETY: format strings and their arguments match C's printf types.
    unsafe {
        ff::av_log(std::ptr::null_mut(), ff::AV_LOG_ERROR, c"mmco: unref short failure\n".as_ptr());
        ff::av_log(std::ptr::null_mut(), ff::AV_LOG_FATAL, c"fatal diagnostic %d\n".as_ptr(), 7);
    }
    assert_eq!(*LOGGER.0.lock().unwrap(), [(log::Level::Error, "fatal diagnostic 7".into())]);
    LOGGER.0.lock().unwrap().clear();
    log::set_max_level(log::LevelFilter::Debug);
    std::thread::scope(|scope| {
        for i in 0..8 {
            scope.spawn(move || unsafe {
                ff::av_log(std::ptr::null_mut(), ff::AV_LOG_ERROR, c"reference diagnostic %d\n".as_ptr(), i);
            });
        }
    });
    let lines = LOGGER.0.lock().unwrap();
    assert_eq!(lines.len(), 8);
    for i in 0..8 {
        assert!(lines.contains(&(log::Level::Debug, format!("reference diagnostic {i}"))));
    }
    drop(lines);
    LOGGER.0.lock().unwrap().clear();
    // This fixture has real non-IDR recovery points, not just synthetic
    // log calls. Seeks and rate changes still decode and return pictures.
    exercise_open_gop();
    assert!(LOGGER.0.lock().unwrap().iter().any(|(_, line)| line.contains("mmco: unref short failure")));
    LOGGER.0.lock().unwrap().clear();
    log::set_max_level(log::LevelFilter::Error);
    exercise_open_gop();
    assert!(LOGGER.0.lock().unwrap().is_empty());
}

fn exercise_open_gop() {
    use aizome_media::{AudioChoice, Event, Options, Player};
    use sidestep_ui::video::{VideoCaps, VideoSink};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let (sink, _detached) = VideoSink::detached();
    let (tx, rx) = mpsc::channel();
    let player = Player::new(
        Options {
            sink: Some(sink), audio: AudioChoice::Null, hardware: false,
            caps: Some(VideoCaps { known: true, shm: vec![0, 1], ..VideoCaps::default() }),
        },
        move |event| { let _ = tx.send(event); },
    );
    let landed = || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap() {
                Event::Seeked(_) => break,
                Event::Failed(error) => panic!("{error}"),
                _ => {}
            }
        }
    };
    player.open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/open-gop.mkv"), 0.0, false);
    landed();
    for (i, position) in [6.3, 2.2, 10.4, 4.5, 8.6, 12.3, 3.1, 9.7].into_iter().enumerate() {
        player.seek(position, true);
        landed();
        player.set_speed(if i % 2 == 0 { 2.0 } else { 1.0 });
        landed();
        let (tx, rx) = mpsc::channel();
        player.snapshot(move |picture| { let _ = tx.send(picture); });
        let picture = rx.recv_timeout(Duration::from_secs(3)).unwrap().expect("a decoded frame after seeking");
        assert_eq!((picture.width, picture.height), (160, 90));
    }
}
