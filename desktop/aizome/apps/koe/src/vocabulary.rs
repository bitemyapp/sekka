//! Portable vocabulary packs and explicitly saved teaching examples.
//! A pack contains text only. Audio never enters pack imports or exports.
use crate::{audio, config::Replacement, text::Processor};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_LIBRARY: usize = 32 * 1024;
pub const MAX_CLIP_SECONDS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Word {
    pub id: String,
    pub spelling: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub version: u32,
    pub revision: u64,
    pub enabled: Vec<String>,
    pub packs: Vec<Pack>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            enabled: vec!["personal".into(), "sekka".into()],
            packs: vec![
                Pack {
                    id: "personal".into(),
                    name: "Personal".into(),
                    description: "Your names and vocabulary".into(),
                    words: vec![],
                },
                Pack {
                    id: "sekka".into(),
                    name: "Sekka".into(),
                    description: "Names from the Sekka desktop".into(),
                    words: vec![
                        Word {
                            id: "aizome".into(),
                            spelling: "Aizome".into(),
                            aliases: vec!["Isome".into(), "IZOMA".into()],
                        },
                        Word { id: "sekka".into(), spelling: "Sekka".into(), aliases: vec![] },
                        Word { id: "koe".into(), spelling: "Koe".into(), aliases: vec![] },
                    ],
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    format: String,
    version: u32,
    pack: Pack,
}

pub struct Pipeline {
    pub prompt: String,
    pub processor: Processor,
}

impl Library {
    pub fn word(&self, pack: &str, word: &str) -> Result<&Word> {
        self.packs
            .iter()
            .find(|p| p.id == pack)
            .and_then(|p| p.words.iter().find(|w| w.id == word))
            .context("word no longer exists; refresh vocabulary")
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported vocabulary version");
        ensure!(self.packs.len() <= 16, "at most 16 vocabulary packs are supported");
        ensure!(self.packs.iter().map(|p| p.words.len()).sum::<usize>() <= 256, "at most 256 words are supported");
        let mut ids = BTreeSet::new();
        for pack in &self.packs {
            valid_id(&pack.id)?;
            ensure!(ids.insert(&pack.id), "duplicate pack ID");
            valid_text(&pack.name, 80)?;
            ensure!(pack.description.len() <= 512 && !pack.description.contains('\0'), "pack description is too long");
            let mut word_ids = BTreeSet::new();
            for word in &pack.words {
                valid_id(&word.id)?;
                ensure!(word_ids.insert(&word.id), "duplicate word ID");
                valid_text(&word.spelling, 128)?;
                ensure!(word.aliases.len() <= 16, "at most 16 approved corrections per word");
                for alias in &word.aliases {
                    valid_text(alias, 128)?;
                }
            }
        }
        let mut enabled = BTreeSet::new();
        for id in &self.enabled {
            ensure!(ids.contains(id) && enabled.insert(id), "invalid active pack selection");
        }
        ensure!(serde_json::to_vec(self)?.len() <= MAX_LIBRARY, "vocabulary library exceeds 32 KiB");
        self.rules()?;
        Ok(())
    }

    fn rules(&self) -> Result<(Vec<String>, Vec<Replacement>)> {
        // Conflicts are rejected instead of silently letting import order decide.
        let mut canonical = BTreeMap::<String, String>::new();
        let mut mapping = BTreeMap::<String, String>::new();
        for pack in self.packs.iter().filter(|p| self.enabled.contains(&p.id)) {
            for word in &pack.words {
                if let Some(previous) = canonical.insert(word.spelling.to_lowercase(), word.spelling.clone()) {
                    ensure!(previous == word.spelling, "active packs disagree on capitalization for {}", word.spelling);
                }
                for alias in &word.aliases {
                    if let Some(previous) = mapping.insert(alias.to_lowercase(), word.spelling.clone()) {
                        ensure!(previous == word.spelling, "active packs give different corrections for {alias}");
                    }
                }
            }
        }
        for (alias, spelling) in &mapping {
            if let Some(canonical) = canonical.get(alias) {
                ensure!(canonical == spelling, "a correction would replace the active word {canonical}");
            }
        }
        Ok((
            canonical.into_values().collect(),
            mapping.into_iter().map(|(from, to)| Replacement { from, to }).collect(),
        ))
    }

    pub fn pipeline(&self, base_prompt: &str, replacements: &[Replacement], append_space: bool) -> Result<Pipeline> {
        let (words, mut rules) = self.rules()?;
        rules.sort_by_key(|r| std::cmp::Reverse(r.from.len()));
        // A deliberately small hint list; don't silently drop an arbitrary tail.
        let hints = words.join(", ");
        ensure!(hints.len() <= 1024, "active vocabulary hints exceed 1024 bytes; disable a pack or shorten its words");
        rules.extend_from_slice(replacements);
        let prompt =
            [base_prompt.trim(), hints.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(". ");
        Ok(Pipeline { prompt, processor: Processor::new(&rules, append_space)? })
    }
}

pub fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
/// A readable ID for scripts to name ("Desktop terms" → `desktop-terms`),
/// or a random one when `text` has no ASCII letters or digits or it's taken.
pub fn readable_id(text: &str, taken: impl Fn(&str) -> bool) -> Result<String> {
    let mut id = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    id.truncate(64);
    let id = id.trim_end_matches('-');
    if id.is_empty() || taken(id) { new_id() } else { Ok(id.into()) }
}
pub fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "invalid vocabulary ID"
    );
    Ok(())
}
fn valid_text(text: &str, max: usize) -> Result<()> {
    ensure!(
        !text.trim().is_empty() && text.len() <= max && text.trim() == text && !text.chars().any(char::is_control),
        "enter nonempty text without control characters (at most {max} bytes)"
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub version: u32,
    pub id: String,
    pub pack_id: String,
    pub word_id: String,
    pub spelling: String,
    pub transcript: String,
    pub heard: String,
    pub model: String,
    pub language: String,
    pub vocabulary_revision: u64,
    pub created_ms: u64,
    pub sample_rate: u32,
    pub duration_ms: u64,
    pub audio_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleSummary {
    pub id: String,
    pub transcript: String,
    pub duration_ms: u64,
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn user() -> Result<Self> {
        Ok(Self::new(crate::config::data_dir()?.join("vocabulary")))
    }
    pub fn dispatch(
        &self,
        request: crate::protocol::VocabularyRequest,
        config: &crate::config::Config,
    ) -> Result<crate::protocol::VocabularyReply> {
        use crate::protocol::{VocabularyReply, VocabularyRequest as V};
        let mut reply = VocabularyReply::default();
        match request {
            V::Library => {
                reply.library = Some(self.load()?);
                reply.recognition = Some(match config.engine.backend {
                    crate::config::Backend::Local => "Recognition runs locally on this computer.".into(),
                    crate::config::Backend::Http => "Recognition sends audio to your configured HTTP server.".into(),
                });
            }
            V::SaveLibrary { library } => {
                library.pipeline(&config.engine.prompt, &config.replacements, config.output.append_space)?;
                let revision = library.revision;
                reply.library = Some(self.save(library, revision)?);
            }
            V::Import { path, revision } => reply.library = Some(self.import(&path, revision)?),
            V::Export { pack, path } => {
                self.export(&pack, &path)?;
                reply.message = Some("Vocabulary exported. No recordings are included.".into());
            }
            V::Examples { pack, word } => reply.examples = Some(self.list_examples(&pack, &word)?),
            V::DeleteExample { id } => {
                self.delete_example(&id)?;
                reply.message = Some("Saved example deleted.".into());
            }
            V::ExportExamples { pack, path } => {
                let count = self.export_examples(&pack, &path)?;
                reply.message = Some(format!("Exported {count} recordings with corrected transcripts."));
            }
            _ => anyhow::bail!("not a vocabulary storage command"),
        }
        Ok(reply)
    }
    pub fn load(&self) -> Result<Library> {
        let path = self.root.join("library.json");
        if !path.exists() {
            return Ok(Library::default());
        }
        let library: Library =
            serde_json::from_slice(&read_limited(&path, MAX_LIBRARY)?).context("cannot read vocabulary library")?;
        library.validate()?;
        Ok(library)
    }
    pub fn save(&self, mut library: Library, expected_revision: u64) -> Result<Library> {
        let _lock = self.lock()?;
        let previous = self.load()?;
        ensure!(previous.revision == expected_revision, "vocabulary changed in another window; refresh before saving");
        for pack in &previous.packs {
            for word in &pack.words {
                if library.word(&pack.id, &word.id).is_err() {
                    ensure!(
                        self.examples(&pack.id, Some(&word.id))?.is_empty(),
                        "delete the saved examples before removing {}",
                        word.spelling
                    );
                }
            }
        }
        library.revision = expected_revision.checked_add(1).context("vocabulary revision overflow")?;
        library.validate()?;
        library.pipeline("", &[], false)?;
        atomic_write(&self.root.join("library.json"), &serde_json::to_vec(&library)?)?;
        Ok(library)
    }
    pub fn import(&self, path: &Path, expected_revision: u64) -> Result<Library> {
        let mut imported: PackFile =
            serde_json::from_slice(&read_limited(path, MAX_LIBRARY)?).context("not a Koe vocabulary pack")?;
        ensure!(imported.format == "koe-vocabulary" && imported.version == 1, "unsupported vocabulary pack");
        let mut library = self.load()?;
        // Validate the original ID and contents before assigning any local ID.
        Library { version: 1, revision: 0, enabled: vec![], packs: vec![imported.pack.clone()] }.validate()?;
        if let Some(existing) = library.packs.iter().find(|p| p.id == imported.pack.id) {
            ensure!(existing != &imported.pack, "this exact pack is already installed");
            // Sharing Personal or an updated domain pack must not overwrite
            // the recipient's edits or break their saved example references.
            imported.pack.id = new_id()?;
            let suffix = " (imported)";
            while imported.pack.name.len() + suffix.len() > 80 {
                imported.pack.name.pop();
            }
            imported.pack.name.push_str(suffix);
        }
        // Imported packs are reviewed before the user enables their corrections.
        library.packs.push(imported.pack);
        self.save(library, expected_revision)
    }
    pub fn export(&self, pack_id: &str, path: &Path) -> Result<()> {
        let library = self.load()?;
        let pack = library.packs.into_iter().find(|p| p.id == pack_id).context("pack no longer exists")?;
        let file = PackFile { format: "koe-vocabulary".into(), version: 1, pack };
        write_new(path, &serde_json::to_vec_pretty(&file)?)
    }
    pub fn save_example(&self, mut example: Example, samples: &[f32]) -> Result<()> {
        valid_id(&example.id)?;
        let _lock = self.lock()?;
        let library = self.load()?;
        ensure!(
            library.word(&example.pack_id, &example.word_id)?.spelling == example.spelling,
            "word changed; record a new example"
        );
        ensure!(
            !example.transcript.trim().is_empty()
                && example.transcript.len() <= 4096
                && !example.transcript.contains('\0'),
            "enter the complete, corrected transcript (at most 4096 bytes)"
        );
        ensure!(
            !samples.is_empty() && samples.len() <= MAX_CLIP_SECONDS as usize * audio::RATE,
            "invalid teaching audio length"
        );
        let audio = audio::wav_bytes(samples)?;
        use sha2::Digest;
        example.audio_sha256 = format!("{:x}", sha2::Sha256::digest(&audio));
        example.created_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
        example.sample_rate = audio::RATE as u32;
        example.duration_ms = (samples.len() * 1000 / audio::RATE) as u64;
        let examples = self.root.join("examples");
        private_dir(&examples)?;
        let temp = tempfile::Builder::new().prefix(".saving-").tempdir_in(&examples)?;
        write_new(&temp.path().join("audio.wav"), &audio)?;
        write_new(&temp.path().join("example.json"), &serde_json::to_vec_pretty(&example)?)?;
        fs::rename(temp.path(), examples.join(&example.id))?;
        Ok(())
    }
    /// A saved example's recording, mono 16-bit PCM WAV.
    pub fn example_audio(&self, id: &str) -> Result<PathBuf> {
        valid_id(id)?;
        Ok(self.root.join("examples").join(id).join("audio.wav"))
    }
    /// Complete saved examples, newest first.
    pub fn examples(&self, pack: &str, word: Option<&str>) -> Result<Vec<Example>> {
        valid_id(pack)?;
        let root = self.root.join("examples");
        if !root.exists() {
            return Ok(vec![]);
        }
        let mut result = vec![];
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if id.starts_with('.') || !entry.file_type()?.is_dir() {
                continue;
            }
            valid_id(&id)?;
            let example: Example =
                serde_json::from_slice(&read_limited(&entry.path().join("example.json"), 16 * 1024)?)?;
            ensure!(example.version == 1 && example.id == id, "invalid saved example");
            if example.pack_id == pack && word.is_none_or(|w| example.word_id == w) {
                result.push(example);
            }
        }
        result.sort_by_key(|e| std::cmp::Reverse(e.created_ms));
        Ok(result)
    }
    pub fn list_examples(&self, pack: &str, word: &str) -> Result<Vec<ExampleSummary>> {
        Ok(self
            .examples(pack, Some(word))?
            .into_iter()
            .take(50)
            .map(|e| ExampleSummary {
                id: e.id,
                transcript: e.transcript.chars().take(100).collect(),
                duration_ms: e.duration_ms,
            })
            .collect())
    }
    pub fn delete_example(&self, id: &str) -> Result<()> {
        valid_id(id)?;
        let _lock = self.lock()?;
        let path = self.root.join("examples").join(id);
        ensure!(fs::symlink_metadata(&path)?.is_dir(), "invalid example directory");
        fs::remove_dir_all(path)?;
        Ok(())
    }
    pub fn export_examples(&self, pack: &str, destination: &Path) -> Result<usize> {
        let _lock = self.lock()?;
        let examples = self.examples(pack, None)?;
        ensure!(!examples.is_empty(), "this pack has no saved examples");
        ensure!(!destination.exists(), "choose a new folder for the training export");
        let parent = destination.parent().context("export has no parent folder")?;
        let temp = tempfile::Builder::new().prefix(".koe-export-").tempdir_in(parent)?;
        private_dir(&temp.path().join("audio"))?;
        let mut manifest = Vec::new();
        for example in &examples {
            let audio = read_limited(&self.example_audio(&example.id)?, 2 * 1024 * 1024)?;
            use sha2::Digest;
            ensure!(
                format!("{:x}", sha2::Sha256::digest(&audio)) == example.audio_sha256,
                "saved teaching audio failed its integrity check"
            );
            let audio_name = format!("audio/{}.wav", example.id);
            write_new(&temp.path().join(&audio_name), &audio)?;
            let mut record = serde_json::to_value(example)?;
            record["audio"] = audio_name.into();
            serde_json::to_writer(&mut manifest, &record)?;
            manifest.push(b'\n');
        }
        write_new(&temp.path().join("manifest.jsonl"), &manifest)?;
        fs::rename(temp.path(), destination)?;
        Ok(examples.len())
    }
    fn lock(&self) -> Result<File> {
        private_dir(&self.root)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.root.join("library.lock"))?;
        lock.lock()?;
        Ok(lock)
    }
}

pub fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path)?;
    ensure!(file.metadata()?.is_file(), "expected a regular file");
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "file exceeds the supported size");
    Ok(bytes)
}
fn private_dir(path: &Path) -> Result<()> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    ensure!(fs::symlink_metadata(path)?.is_dir(), "expected a private directory");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("file has no parent")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .context("cannot create export; choose a filename that does not already exist")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
