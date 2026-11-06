use super::*;

fn chunk(index: u64) -> Chunk {
    Chunk { start_sample: index * CHUNK_SAMPLES as u64, samples: vec![0.25; 160] }
}

#[tokio::test]
async fn live_overflow_keeps_recent_audio_and_absolute_sample_positions() {
    let queue = Queue::default();
    for index in 0..10 { queue.push(chunk(index), true).await; }
    assert_eq!(queue.state.lock().await.chunks.len(), QUEUE_CHUNKS);
    assert_eq!(queue.next().await.unwrap().unwrap().start_sample, 8 * CHUNK_SAMPLES as u64);
    assert_eq!(queue.next().await.unwrap().unwrap().start_sample, 9 * CHUNK_SAMPLES as u64);
    queue.finish(Ok(())).await;
    assert!(queue.next().await.unwrap().is_none());
}

#[tokio::test]
async fn file_backpressure_does_not_drop_or_reorder_audio() {
    let queue = Queue::default();
    queue.push(chunk(0), false).await;
    queue.push(chunk(1), false).await;
    let blocked = queue.push(chunk(2), false);
    tokio::pin!(blocked);
    assert!(tokio::time::timeout(Duration::from_millis(20), &mut blocked).await.is_err());
    assert_eq!(queue.next().await.unwrap().unwrap().start_sample, 0);
    tokio::time::timeout(Duration::from_secs(1), blocked).await.unwrap();
    assert_eq!(queue.next().await.unwrap().unwrap().start_sample, CHUNK_SAMPLES as u64);
    assert_eq!(queue.next().await.unwrap().unwrap().start_sample, 2 * CHUNK_SAMPLES as u64);
}

#[tokio::test]
async fn decoder_error_follows_already_buffered_audio() {
    let queue = Queue::default();
    queue.push(chunk(0), false).await;
    queue.finish(Err(anyhow::anyhow!("decoder failed"))).await;
    assert!(queue.next().await.unwrap().is_some());
    assert!(queue.next().await.unwrap_err().to_string().contains("decoder failed"));
}

#[test]
fn timestamps_keep_media_seek_and_clip_padded_tail() {
    let segment = CaptionSegment { start: -0.05, end: 3.0, text: " final words ".into(), turn_next: false };
    let event = cue(segment, 125.25, 0.4, None).unwrap();
    let Event::Cue { start, end, text, speaker } = event else { panic!("expected cue") };
    assert_eq!(start, 125.25);
    assert!((end - 125.65).abs() < 0.000_001);
    assert_eq!(text, "final words");
    assert!(speaker.is_none());
    let padded = CaptionSegment { start: 0.5, end: 1.0, text: "padding hallucination".into(), turn_next: false };
    assert!(cue(padded, 125.25, 0.4, None).is_none());
}

#[tokio::test]
async fn cancellation_wakes_capture_and_consumer_including_late_waiters() {
    let cancel = Cancellation::default();
    let capture = cancel.cancelled();
    let consumer = cancel.cancelled();
    tokio::pin!(capture, consumer);
    assert!(tokio::time::timeout(Duration::from_millis(10), &mut capture).await.is_err());
    assert!(tokio::time::timeout(Duration::from_millis(10), &mut consumer).await.is_err());
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(capture, consumer);
        cancel.cancelled().await;
    }).await.unwrap();
}

#[tokio::test]
async fn cancellation_reaps_a_source_blocked_in_read() {
    let child = Command::new("sleep").arg("30")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true).spawn().unwrap();
    let pid = child.id().unwrap();
    let queue = Arc::new(Queue::default());
    let cancel = Arc::new(Cancellation::default());
    let task = tokio::spawn(capture(child, true, queue.clone(), cancel.clone()));
    tokio::task::yield_now().await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    assert!(!PathBuf::from(format!("/proc/{pid}")).exists(), "child must be reaped, not just signalled");
    assert!(queue.next().await.unwrap().is_none());
}


#[tokio::test]
async fn cancellation_reaps_a_decoder_blocked_on_file_backpressure() {
    let child = Command::new("dd")
        .args(["if=/dev/zero", &format!("bs={}", FILE_CHUNK_SAMPLES * 2), "count=8", "status=none"])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true).spawn().unwrap();
    let pid = child.id().unwrap();
    let queue = Arc::new(Queue::default());
    let cancel = Arc::new(Cancellation::default());
    let task = tokio::spawn(capture(child, false, queue.clone(), cancel.clone()));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if queue.state.lock().await.chunks.len() == QUEUE_CHUNKS { break; }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    assert!(!PathBuf::from(format!("/proc/{pid}")).exists());
    assert!(queue.state.lock().await.chunks.len() <= QUEUE_CHUNKS);
}

#[tokio::test]
async fn http_and_incompatible_diarization_fail_before_audio_capture() {
    let mut engine = crate::config::Engine::default();
    engine.backend = crate::config::Backend::Http;
    let error = Recognizer::new(engine.clone()).captions(vec![], false, Arc::new(AtomicBool::new(false)))
        .await.unwrap_err();
    assert!(error.to_string().contains("HTTP is unsupported"));
    engine.backend = crate::config::Backend::Local;
    engine.translate = true;
    let error = Recognizer::new(engine).captions(vec![], true, Arc::new(AtomicBool::new(false)))
        .await.unwrap_err();
    assert!(error.to_string().contains("translation disabled"));
}
