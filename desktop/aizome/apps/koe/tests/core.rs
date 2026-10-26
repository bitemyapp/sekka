use koe::{
    audio::{self, PcmDecoder},
    config::{Backend, Config, Replacement},
    ipc,
    protocol::{Action, Phase, Request},
    text::Processor,
};
use std::io::Cursor;
use tokio::io::{AsyncWriteExt, BufReader};

#[test]
fn pcm_reads_preserve_split_samples() {
    let expected = [i16::MIN, -1, 0, 1, i16::MAX];
    let bytes: Vec<_> = expected.iter().flat_map(|s| s.to_ne_bytes()).collect();
    for chunk_size in 1..=7 {
        let mut decoder = PcmDecoder::default();
        let actual: Vec<_> = bytes.chunks(chunk_size).flat_map(|b| decoder.decode(b)).collect();
        assert_eq!(actual, expected.iter().map(|s| *s as f32 / 32768.0).collect::<Vec<_>>());
    }
}
#[test]
fn wav_has_correct_rate_and_clamps_samples() {
    let bytes = audio::wav_bytes(&[-2.0, 0.0, 2.0]).unwrap();
    let reader = hound::WavReader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(reader.spec().sample_rate, 16000);
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.into_samples::<i16>().collect::<Result<Vec<_>, _>>().unwrap(), [-32767, 0, 32767]);
    assert_eq!(audio::rms(&[]), 0.0);
    assert!((audio::rms(&[0.5, -0.5]) - 0.5).abs() < 0.001);
}
#[test]
fn audible_input_survives_leading_and_trailing_silence() {
    let mut samples = vec![0.0; audio::RATE * 5];
    assert!(!audio::has_signal(&samples, 0.008));
    // A short click is not sufficient input for recognition.
    samples[audio::RATE..audio::RATE + 80].fill(0.5);
    assert!(!audio::has_signal(&samples, 0.008));
    samples.fill(0.0);
    samples[audio::RATE..audio::RATE + 4800].fill(0.012);
    assert!(audio::rms(&samples) < 0.008);
    assert!(audio::has_signal(&samples, 0.008));
}
#[test]
fn wav_rejects_wrong_sample_rate_and_nonfinite_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.wav");
    for (rate, sample) in [(48000, 0.0), (16000, f32::NAN)] {
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        writer.write_sample(sample).unwrap();
        writer.finalize().unwrap();
        assert!(audio::read_wav(&path, 10).is_err());
    }
}
#[test]
fn replacements_are_literal_unicode_and_word_scoped() {
    let processor = Processor::new(
        &[
            Replacement { from: "seka".into(), to: "Sekka".into() },
            Replacement { from: "price".into(), to: "$1".into() },
            Replacement { from: "日本".into(), to: "日本語".into() },
        ],
        true,
    )
    .unwrap();
    assert_eq!(processor.apply("  SEKA\nsekai price 日本 "), "Sekka sekai $1 日本語 ");
    assert_eq!(processor.apply("  \n"), "");
}
#[test]
fn config_rejects_typos_and_unbounded_capture() {
    assert!(toml::from_str::<Config>("[audio]\nmax_second = 5").is_err());
    let mut config = Config::default();
    config.audio.max_seconds = 0;
    assert!(config.validate().is_err());
    config.audio.max_seconds = 300;
    config.audio.silence_threshold = f32::NAN;
    assert!(config.validate().is_err());
    config.audio.silence_threshold = 0.008;
    config.engine.prompt = "bad\0prompt".into();
    assert!(config.validate().is_err());
}
#[test]
fn http_requires_tls_or_loopback_and_separate_credentials() {
    let mut config = Config::default();
    config.engine.backend = Backend::Http;
    for url in ["http://127.0.0.1:8080/inference", "http://[::1]/inference", "https://asr.example/inference"] {
        config.engine.http.endpoint = url.into();
        assert!(config.validate().is_ok(), "{url}");
    }
    for url in [
        "http://asr.example/inference",
        "https://user:secret@asr.example/inference",
        "https://asr.example/?key=secret",
        "file:///tmp/input",
    ] {
        config.engine.http.endpoint = url.into();
        assert!(config.validate().is_err(), "{url}");
    }
}
#[tokio::test]
async fn ipc_rejects_oversized_and_incomplete_frames() {
    let mut reader = BufReader::new(Cursor::new(vec![b'x'; koe::protocol::MAX_MESSAGE + 1]));
    assert!(ipc::read_line(&mut reader).await.is_err());
    let mut reader = BufReader::new(Cursor::new(b"{\"action\":\"start\"}"));
    assert!(ipc::read_line(&mut reader).await.is_err());
    let mut reader = BufReader::new(Cursor::new(b""));
    assert!(ipc::read_line(&mut reader).await.unwrap().is_none());
}
#[tokio::test]
async fn ipc_preserves_coalesced_and_fragmented_frames() {
    let (mut write, read) = tokio::io::duplex(32);
    tokio::spawn(async move {
        write.write_all(b"{\"action\":").await.unwrap();
        tokio::task::yield_now().await;
        write.write_all(b"\"start\"}\n{\"action\":\"stop\"}\n").await.unwrap();
    });
    let mut read = BufReader::new(read);
    for action in [Action::Start, Action::Stop] {
        let req: Request = serde_json::from_slice(&ipc::read_line(&mut read).await.unwrap().unwrap()).unwrap();
        assert_eq!(req.action, action);
    }
    assert!(ipc::read_line(&mut read).await.unwrap().is_none());
}
#[test]
fn status_does_not_serialize_transcripts() {
    let value = serde_json::to_value(koe::protocol::Response::status(koe::protocol::Status {
        phase: Phase::Recording,
        ..Default::default()
    }))
    .unwrap();
    assert!(value.get("text").is_none());
    assert_eq!(value["version"], 1);
}
#[test]
fn catalog_is_pinned_and_detects_corruption() {
    let catalog = koe::models::catalog();
    assert_eq!(catalog.revision.len(), 40);
    for model in catalog.models {
        assert_eq!(model.sha256.len(), 64);
        assert!(model.bytes > 1_000_000);
    }
    let temp = tempfile::NamedTempFile::new().unwrap();
    let fake = koe::models::Model { name: "fixture".into(), bytes: 0, sha256: "00".repeat(32) };
    assert!(koe::models::verify(temp.path(), &fake).is_err());
}
#[cfg(feature = "local")]
#[tokio::test]
#[ignore = "requires KOE_TEST_MODEL and KOE_TEST_WAV; runs actual offline recognition"]
async fn local_whisper_recognizes_jfk() {
    let mut config = Config::default();
    config.engine.model = std::env::var("KOE_TEST_MODEL").unwrap().into();
    config.engine.language = "en".into();
    let samples = audio::read_wav(std::path::Path::new(&std::env::var("KOE_TEST_WAV").unwrap()), 60).unwrap();
    let recognizer = koe::engine::Recognizer::new(config.engine);
    let text =
        recognizer.transcribe(samples, std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))).await.unwrap();
    assert!(text.to_lowercase().contains("ask not what your country"), "{text}");
    assert!(recognizer.loaded());
    recognizer.unload().unwrap();
    assert!(!recognizer.loaded());
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aizome-koe"))
        .arg("--config")
        .arg(dir.path().join("defaults.toml"))
        .arg("transcribe")
        .arg(std::env::var("KOE_TEST_WAV").unwrap())
        .arg("--model")
        .arg(std::env::var("KOE_TEST_MODEL").unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).to_lowercase().contains("ask not what your country"));
    assert!(
        output.stderr.is_empty(),
        "native inference must not write decoded tokens to the journal: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_annotated_example_is_a_valid_configuration() {
    let config: koe::config::Config = toml::from_str(include_str!("../data/koe.example.toml")).unwrap();
    config.validate().unwrap();
    assert!(!config.enabled, "the example must not turn dictation on");
}
