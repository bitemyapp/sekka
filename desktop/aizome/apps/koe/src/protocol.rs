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
}

/// A request carries exactly the payload required by its command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "WireRequest", into = "WireRequest")]
pub struct Request {
    pub version: u32,
    pub command: RequestCommand,
}

#[derive(Debug, Clone)]
pub enum RequestCommand {
    Control(Action),
    Vocabulary(VocabularyRequest),
}

impl Request {
    pub fn new(action: Action) -> Self {
        Self { command: RequestCommand::Control(action), version: VERSION }
    }
    pub fn vocabulary(request: VocabularyRequest) -> Self {
        Self { command: RequestCommand::Vocabulary(request), version: VERSION }
    }
    pub fn action(&self) -> Option<Action> {
        match self.command {
            RequestCommand::Control(action) => Some(action),
            _ => None,
        }
    }
    pub fn vocabulary_command(&self) -> Option<&VocabularyRequest> {
        match &self.command {
            RequestCommand::Vocabulary(request) => Some(request),
            _ => None,
        }
    }
}

// Keep the existing JSON protocol while rejecting mismatched payloads during
// decoding. These transport types never enter the daemon's command queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum VocabularyAction {
    Vocabulary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum WireRequest {
    Vocabulary {
        action: VocabularyAction,
        #[serde(default = "version")]
        version: u32,
        vocabulary: VocabularyRequest,
    },
    Control {
        action: Action,
        #[serde(default = "version")]
        version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vocabulary: Option<()>,
    },
}
fn version() -> u32 {
    VERSION
}
impl From<WireRequest> for Request {
    fn from(wire: WireRequest) -> Self {
        match wire {
            WireRequest::Control { action, version, .. } => Self { version, command: RequestCommand::Control(action) },
            WireRequest::Vocabulary { vocabulary, version, .. } => {
                Self { version, command: RequestCommand::Vocabulary(vocabulary) }
            }
        }
    }
}
impl From<Request> for WireRequest {
    fn from(request: Request) -> Self {
        match request.command {
            RequestCommand::Control(action) => Self::Control { action, version: request.version, vocabulary: None },
            RequestCommand::Vocabulary(vocabulary) => {
                Self::Vocabulary { action: VocabularyAction::Vocabulary, version: request.version, vocabulary }
            }
        }
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

#[cfg(test)]
mod request_tests {
    use super::*;
    #[test]
    fn requests_preserve_wire_format_and_reject_mismatched_payloads() {
        let control = serde_json::json!({"action":"status","version":1});
        assert_eq!(serde_json::to_value(Request::new(Action::Status)).unwrap(), control);
        assert_eq!(serde_json::from_value::<Request>(control).unwrap().action(), Some(Action::Status));
        let vocabulary = serde_json::json!({"action":"vocabulary","version":1,"vocabulary":{"command":"library"}});
        assert_eq!(serde_json::to_value(Request::vocabulary(VocabularyRequest::Library)).unwrap(), vocabulary);
        assert!(matches!(
            serde_json::from_value::<Request>(vocabulary).unwrap().command,
            RequestCommand::Vocabulary(VocabularyRequest::Library)
        ));
        for invalid in [
            serde_json::json!({"action":"vocabulary"}),
            serde_json::json!({"action":"vocabulary","vocabulary":null}),
            serde_json::json!({"action":"status","vocabulary":{"command":"library"}}),
            serde_json::json!({"action":"status","unexpected":true}),
        ] {
            assert!(serde_json::from_value::<Request>(invalid).is_err());
        }
    }
}
