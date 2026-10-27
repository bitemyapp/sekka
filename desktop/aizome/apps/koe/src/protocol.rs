use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_MESSAGE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Start,
    Stop,
    Toggle,
    Pause,
    Resume,
    Cancel,
    Status,
    Subscribe,
    Last,
    CopyLast,
    PasteLast,
    ClearLast,
    Unload,
    Shutdown,
    #[value(skip)]
    Vocabulary,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: Action,
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocabulary: Option<VocabularyRequest>,
}
fn version() -> u32 {
    VERSION
}
impl Request {
    pub fn new(action: Action) -> Self {
        Self { action, version: VERSION, vocabulary: None }
    }
}

impl Request {
    pub fn vocabulary(request: VocabularyRequest) -> Self {
        Self { action: Action::Vocabulary, version: VERSION, vocabulary: Some(request) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
pub enum VocabularyRequest {
    Library,
    SaveLibrary { library: crate::vocabulary::Library },
    Import { path: std::path::PathBuf, revision: u64 },
    Export { pack: String, path: std::path::PathBuf },
    Examples { pack: String, word: String },
    DeleteExample { id: String },
    ExportExamples { pack: String, path: std::path::PathBuf },
    TeachStart { token: String, pack: String, word: String },
    TeachStop { token: String },
    TeachPoll { token: String },
    TeachCancel { token: String },
    TeachSave { token: String, transcript: String },
    TeachPlay { token: String },
}

impl VocabularyRequest {
    pub fn is_store_command(&self) -> bool {
        matches!(
            self,
            Self::Library
                | Self::SaveLibrary { .. }
                | Self::Import { .. }
                | Self::Export { .. }
                | Self::Examples { .. }
                | Self::DeleteExample { .. }
                | Self::ExportExamples { .. }
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VocabularyReply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recognition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<crate::vocabulary::Library>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<crate::vocabulary::ExampleSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<TeachingPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeachingPreview {
    pub heard: String,
    pub corrected: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    #[default]
    Idle,
    Recording,
    Paused,
    Finishing,
    Transcribing,
    Delivering,
    Error,
}

/// Status never contains audio or transcript text. `last` explicitly retrieves text.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub phase: Phase,
    pub sequence: u64,
    pub elapsed_ms: u64,
    /// Root mean square level, normalized to 0..1.
    pub level: f32,
    pub has_last: bool,
    pub model_loaded: bool,
    /// Recognition wall time for the last completed attempt, excluding capture.
    #[serde(default)]
    pub transcription_ms: Option<u64>,
    pub error: Option<String>,
    /// Only explicit teaching sessions retain audio for review in memory.
    #[serde(default)]
    pub teaching: bool,
    /// The resident model runs on a graphics card (else the processor).
    #[serde(default)]
    pub gpu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    pub ok: bool,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary: Option<VocabularyReply>,
}
impl Response {
    pub fn status(status: Status) -> Self {
        Self { version: VERSION, ok: true, status, text: None, error: None, vocabulary: None }
    }
}
