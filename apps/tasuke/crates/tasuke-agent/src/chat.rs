//! The conversation as the model sees it: chat messages, kept in the
//! session file and turned into the OpenAI wire format per request.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::CallId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
    },
    /// A PNG on disk; sent as a data URL.
    Image {
        path: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<Part>),
}

impl Default for Content {
    fn default() -> Content {
        Content::Text(String::new())
    }
}

impl Content {
    /// The text parts, joined.
    pub fn text(&self) -> String {
        match self {
            Content::Text(t) => t.clone(),
            Content::Parts(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    Part::Text { text } => Some(text.as_str()),
                    Part::Image { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    pub fn images(&self) -> usize {
        match self {
            Content::Text(_) => 0,
            Content::Parts(parts) => parts.iter().filter(|p| matches!(p, Part::Image { .. })).count(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Content::Text(t) => t.len(),
            Content::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    Part::Text { text } => text.len(),
                    Part::Image { .. } => 0,
                })
                .sum(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0 && self.images() == 0
    }
}

/// A tool call as the model sent it: the arguments are its JSON text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireCall {
    pub id: CallId,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub content: Content,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<WireCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<CallId>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Message {
        Message::new(Role::System, Content::Text(text.into()))
    }

    pub fn user(text: impl Into<String>) -> Message {
        Message::new(Role::User, Content::Text(text.into()))
    }

    pub fn assistant(text: impl Into<String>) -> Message {
        Message::new(Role::Assistant, Content::Text(text.into()))
    }

    pub fn tool(id: CallId, text: impl Into<String>) -> Message {
        Message { tool_call_id: Some(id), ..Message::new(Role::Tool, Content::Text(text.into())) }
    }

    pub fn new(role: Role, content: Content) -> Message {
        Message { role, content, reasoning: None, tool_calls: Vec::new(), tool_call_id: None }
    }
}
