//! Which API a turn speaks: the local server's OpenAI-compatible chat, or a
//! cloud provider's own (Anthropic's Messages, OpenAI's Responses), and the
//! raw replies kept to send back as they came.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tasuke_models::CloudProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Wire {
    /// `/chat/completions` on llama-server (or any compatible server).
    Chat,
    /// Anthropic's `/messages`.
    Anthropic,
    /// OpenAI's `/responses`.
    Responses,
}

impl Wire {
    pub fn of(provider: Option<CloudProvider>) -> Wire {
        match provider {
            None => Wire::Chat,
            Some(CloudProvider::Anthropic) => Wire::Anthropic,
            Some(CloudProvider::OpenAi) => Wire::Responses,
        }
    }

    pub fn is_cloud(self) -> bool {
        self != Wire::Chat
    }

    /// Whether earlier turns must go back exactly as they were: Claude's
    /// thinking is bound to the conversation before it, so trimming an old
    /// screenshot or output would void every later thinking block.
    pub fn append_only(self) -> bool {
        self == Wire::Anthropic
    }
}

/// A cloud model's reply as its API gave it, kept in the session so the
/// next request replays it unchanged: Claude's content blocks (thinking is
/// signed) or OpenAI's output items (reasoning travels encrypted).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawTurn {
    pub wire: Wire,
    pub model: String,
    pub items: Vec<Value>,
}
