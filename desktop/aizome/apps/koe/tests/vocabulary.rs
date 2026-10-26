use koe::vocabulary::{Example, Library, Pack, Store, Word};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

fn example(id: &str) -> Example {
    Example {
        version: 1,
        id: id.into(),
        pack_id: "sekka".into(),
        word_id: "aizome".into(),
        spelling: "Aizome".into(),
        transcript: "Open Aizome settings.".into(),
        heard: "Open Isome settings.".into(),
        model: "ggml-base.bin".into(),
        language: "auto".into(),
        vocabulary_revision: 0,
        created_ms: 0,
        sample_rate: 0,
        duration_ms: 0,
        audio_sha256: String::new(),
    }
}

#[test]
fn active_packs_hint_and_correct_without_replacing_subwords() {
    let mut library = Library::default();
    library.validate().unwrap();
    let pipeline = library.pipeline("A desktop conversation", &[], false).unwrap();
    assert!(pipeline.prompt.contains("Aizome"));
    assert_eq!(pipeline.processor.apply("Isome, IZOMA, isome. Isomer."), "Aizome, Aizome, Aizome. Isomer.");
    library.enabled.clear();
    let pipeline = library.pipeline("", &[], false).unwrap();
    assert!(pipeline.prompt.is_empty());
    assert_eq!(pipeline.processor.apply("Isome"), "Isome");
}

#[test]
fn pack_round_trip_is_text_only_and_imports_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("source"));
    store.save_example(example("sample"), &[0.1; 1600]).unwrap();
    let path = dir.path().join("sekka.koe.json");
    store.export("sekka", &path).unwrap();
    let bytes = fs::read(&path).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["format"], "koe-vocabulary");
    assert_eq!(json["pack"]["words"][0]["aliases"][0], "Isome");
    assert_eq!(json.as_object().unwrap().len(), 3);
    assert!(!String::from_utf8(bytes).unwrap().contains("transcript"));
    assert!(store.export("sekka", &path).is_err());
    let receiver = Store::new(dir.path().join("receiver"));
    let mut empty = Library::default();
    empty.packs.clear();
    empty.enabled.clear();
    receiver.save(empty, 0).unwrap();
    let imported = receiver.import(&path, 1).unwrap();
    assert_eq!(imported.packs.len(), 1);
    assert!(imported.enabled.is_empty());
    assert!(receiver.import(&path, 2).is_err());
    assert!(!dir.path().join("receiver/examples").exists());
}

#[test]
fn rejects_conflicts_stale_edits_and_untrusted_pack_input() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("store"));
    let mut library = store.save(Library::default(), 0).unwrap();
    assert!(store.save(library.clone(), 0).is_err());
    library.packs.push(Pack {
        id: "conflict".into(),
        name: "Conflict".into(),
        description: String::new(),
        words: vec![Word { id: "name".into(), spelling: "Other".into(), aliases: vec!["ISOME".into()] }],
    });
    library.enabled.push("conflict".into());
    assert!(store.save(library.clone(), 1).unwrap_err().to_string().contains("different corrections"));
    library.enabled.pop();
    let saved = store.save(library, 1).unwrap();
    assert_eq!(saved.revision, 2);
    let path = dir.path().join("bad.json");
    fs::write(&path, r#"{"format":"koe-vocabulary","version":1,"pack":{"id":"../outside","name":"Bad","words":[]}}"#)
        .unwrap();
    assert!(store.import(&path, 2).is_err());
    fs::write(
        &path,
        r#"{"format":"koe-vocabulary","version":1,"pack":{"id":"new","name":"Bad","words":[]},"audio":"hidden"}"#,
    )
    .unwrap();
    assert!(store.import(&path, 2).is_err());
    fs::write(&path, vec![b' '; 33 * 1024]).unwrap();
    assert!(store.import(&path, 2).is_err());
    let link = dir.path().join("link");
    symlink(&path, &link).unwrap();
    assert!(store.import(&link, 2).is_err());
}

#[test]
fn sharing_personal_or_updated_packs_preserves_recipient_data() {
    let dir = tempfile::tempdir().unwrap();
    let sender = Store::new(dir.path().join("sender"));
    let mut library = sender.load().unwrap();
    library.packs[0].words.push(Word { id: "name".into(), spelling: "Turing".into(), aliases: vec!["Touring".into()] });
    sender.save(library, 0).unwrap();
    let path = dir.path().join("personal.koe.json");
    sender.export("personal", &path).unwrap();
    let receiver = Store::new(dir.path().join("receiver"));
    let library = receiver.import(&path, 0).unwrap();
    assert_eq!(library.packs.len(), 3);
    assert_eq!(library.packs[0].id, "personal");
    assert!(library.packs[0].words.is_empty());
    let imported = &library.packs[2];
    assert_eq!(imported.name, "Personal (imported)");
    assert_ne!(imported.id, "personal");
    assert!(!library.enabled.contains(&imported.id));
    assert_eq!(imported.words[0].spelling, "Turing");
}

#[test]
fn explicit_examples_export_complete_labels_audio_and_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let store = Store::new(root.clone());
    store.save_example(example("sample"), &[0.1; 16000]).unwrap();
    assert_eq!(fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
    let wav = root.join("examples/sample/audio.wav");
    assert_eq!(fs::metadata(&wav).unwrap().permissions().mode() & 0o777, 0o600);
    let reader = hound::WavReader::open(&wav).unwrap();
    assert_eq!(reader.spec().sample_rate, 16000);
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.len(), 16000);
    let data = dir.path().join("training");
    assert_eq!(store.export_examples("sekka", &data).unwrap(), 1);
    let manifest = fs::read_to_string(data.join("manifest.jsonl")).unwrap();
    assert_eq!(manifest.lines().count(), 1);
    let entry: serde_json::Value = serde_json::from_str(manifest.trim()).unwrap();
    assert_eq!(entry["transcript"], "Open Aizome settings.");
    assert_eq!(entry["heard"], "Open Isome settings.");
    assert_eq!(entry["model"], "ggml-base.bin");
    assert_eq!(entry["duration_ms"], 1000);
    assert_eq!(fs::read(data.join(entry["audio"].as_str().unwrap())).unwrap(), fs::read(&wav).unwrap());
    let mut library = store.load().unwrap();
    library.packs[1].words.remove(0);
    assert!(store.save(library.clone(), 0).is_err());
    fs::write(&wav, b"corrupt audio").unwrap();
    assert!(store.export_examples("sekka", &dir.path().join("bad-export")).is_err());
    assert!(!dir.path().join("bad-export").exists());
    store.delete_example("sample").unwrap();
    assert!(store.list_examples("sekka", "aizome").unwrap().is_empty());
    store.save(library, 0).unwrap();
}

#[test]
fn invalid_examples_never_create_saved_audio() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("store"));
    let mut invalid = example("sample");
    invalid.transcript.clear();
    assert!(store.save_example(invalid, &[0.1; 1600]).is_err());
    let mut invalid = example("sample");
    invalid.spelling = "Changed".into();
    assert!(store.save_example(invalid, &[0.1; 1600]).is_err());
    assert!(store.save_example(example("sample"), &[]).is_err());
    assert!(store.save_example(example("../outside"), &[0.1; 1600]).is_err());
    assert!(!dir.path().join("store/examples").exists());
}

/// What scripts and agents see: `koe` vocabulary commands print JSON and
/// act on the store without a daemon.
fn koe(dir: &std::path::Path, args: &[&str]) -> Result<serde_json::Value, String> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aizome-koe"))
        .arg("--config")
        .arg(dir.join("koe.toml"))
        .args(args)
        .current_dir(dir)
        .env("XDG_DATA_HOME", dir.join("data"))
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .unwrap();
    if output.status.success() {
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into())
    }
}

#[test]
fn command_line_manages_packs_words_and_saved_examples() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let pack = koe(dir, &["packs", "create", "Desktop terms", "--description", "Team names", "--enable"]).unwrap();
    assert_eq!(pack["id"], "desktop-terms");
    assert_eq!(pack["enabled"], true);
    let word = koe(dir, &["words", "add", "desktop-terms", "Fusen", "--alias", "who sen", "--alias", "Fuzen"]).unwrap();
    assert_eq!(word["id"], "fusen");
    assert_eq!(word["pack"], "desktop-terms");
    // Packs go by name and words by spelling, ignoring case, as well as by ID.
    assert!(koe(dir, &["words", "add", "desktop terms", "fusen"]).unwrap_err().contains("already has"));
    let word =
        koe(dir, &["words", "edit", "desktop-terms", "FUSEN", "--add-alias", "Fyusen", "--remove-alias", "fuzen"])
            .unwrap();
    assert_eq!(word["aliases"], serde_json::json!(["who sen", "Fyusen"]));
    assert!(koe(dir, &["words", "edit", "desktop-terms", "fusen", "--remove-alias", "nope"]).is_err());
    // Validation is the window's: an alias may not replace an active word.
    assert!(
        koe(dir, &["words", "add", "desktop-terms", "Isome"]).unwrap_err().contains("would replace the active word")
    );

    let store = Store::new(dir.join("data/aizome/koe/vocabulary"));
    let mut saved = example("taught");
    saved.pack_id = "desktop-terms".into();
    saved.word_id = "fusen".into();
    saved.spelling = "Fusen".into();
    saved.transcript = "Open Fusen.".into();
    store.save_example(saved, &[0.1; 1600]).unwrap();
    assert_eq!(koe(dir, &["packs", "show", "Desktop terms"]).unwrap()["words"][0]["examples"], 1);
    let examples = koe(dir, &["examples", "list"]).unwrap();
    assert_eq!(examples.as_array().unwrap().len(), 1);
    assert_eq!(examples[0]["transcript"], "Open Fusen.");
    assert_eq!(examples[0]["heard"], "Open Isome settings.");
    let audio = std::path::PathBuf::from(examples[0]["audio"].as_str().unwrap());
    use sha2::Digest;
    assert_eq!(format!("{:x}", sha2::Sha256::digest(fs::read(&audio).unwrap())), examples[0]["audio_sha256"]);
    assert_eq!(koe(dir, &["examples", "list", "sekka"]).unwrap(), serde_json::json!([]));
    assert!(
        koe(dir, &["words", "remove", "desktop-terms", "fusen"]).unwrap_err().contains("delete the saved examples")
    );
    koe(dir, &["examples", "delete", "taught"]).unwrap();
    assert!(!audio.exists());
    assert!(koe(dir, &["examples", "delete", "taught"]).is_err());
    koe(dir, &["words", "remove", "desktop-terms", "fusen"]).unwrap();

    let pack = koe(dir, &["packs", "disable", "desktop-terms"]).unwrap();
    assert_eq!(pack["enabled"], false);
    let pack = koe(dir, &["packs", "edit", "desktop-terms", "--name", "Team terms"]).unwrap();
    assert_eq!(pack["name"], "Team terms");
    koe(dir, &["packs", "export", "team terms", "team.koe.json"]).unwrap();
    assert!(dir.join("team.koe.json").exists());
    koe(dir, &["packs", "remove", "team terms"]).unwrap();
    let library = koe(dir, &["packs", "list"]).unwrap();
    assert_eq!(library["packs"].as_array().unwrap().len(), 2);
    let imported = koe(dir, &["packs", "import", "team.koe.json"]).unwrap();
    assert_eq!(imported["id"], "desktop-terms");
    assert_eq!(imported["enabled"], false);
}
