//! Cloud models a person can use with their own account: Anthropic's Claude
//! and OpenAI's GPT, each through an API key kept in the desktop's keyring
//! ([`crate::secrets`]). Tasuke stays local first: a cloud model runs only
//! when the person picks one, and then what it's sent leaves the computer.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where a cloud model runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CloudProvider {
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
}

impl CloudProvider {
    pub const ALL: [CloudProvider; 2] = [CloudProvider::Anthropic, CloudProvider::OpenAi];

    /// As in config.toml and on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            CloudProvider::Anthropic => "anthropic",
            CloudProvider::OpenAi => "openai",
        }
    }

    pub fn parse(s: &str) -> Option<CloudProvider> {
        match s.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Some(CloudProvider::Anthropic),
            "openai" | "gpt" | "chatgpt" => Some(CloudProvider::OpenAi),
            _ => None,
        }
    }

    /// The company, as people know it.
    pub fn company(self) -> &'static str {
        match self {
            CloudProvider::Anthropic => "Anthropic",
            CloudProvider::OpenAi => "OpenAI",
        }
    }

    /// The models' family name.
    pub fn family(self) -> &'static str {
        match self {
            CloudProvider::Anthropic => "Claude",
            CloudProvider::OpenAi => "GPT",
        }
    }

    /// The API's root, `…/v1`. `TASUKE_ANTHROPIC_BASE_URL` and
    /// `TASUKE_OPENAI_BASE_URL` point it elsewhere (tests, a proxy).
    pub fn api_base(self) -> String {
        let (var, default) = match self {
            CloudProvider::Anthropic => ("TASUKE_ANTHROPIC_BASE_URL", "https://api.anthropic.com/v1"),
            CloudProvider::OpenAi => ("TASUKE_OPENAI_BASE_URL", "https://api.openai.com/v1"),
        };
        std::env::var(var).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_owned())
    }

    /// Where a person makes a key.
    pub fn keys_page(self) -> &'static str {
        match self {
            CloudProvider::Anthropic => "https://console.anthropic.com/settings/keys",
            CloudProvider::OpenAi => "https://platform.openai.com/api-keys",
        }
    }

    /// How the provider's keys start, to catch a paste of the wrong thing.
    pub fn key_prefix(self) -> &'static str {
        match self {
            CloudProvider::Anthropic => "sk-ant-",
            CloudProvider::OpenAi => "sk-",
        }
    }

    /// The environment variable the provider's own tools read; Tasuke's
    /// command line takes a key from it when the keyring has none.
    pub fn env_var(self) -> &'static str {
        match self {
            CloudProvider::Anthropic => "ANTHROPIC_API_KEY",
            CloudProvider::OpenAi => "OPENAI_API_KEY",
        }
    }

    /// A context window to assume when the model list doesn't say.
    pub fn default_context(self) -> u64 {
        match self {
            CloudProvider::Anthropic => 1_000_000,
            CloudProvider::OpenAi => 272_000,
        }
    }
}

impl fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.company())
    }
}

/// An API key. It never prints: [`ApiKey::hint`] shows its ends.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// A pasted key, trimmed of whitespace and quotes; `None` when empty.
    pub fn new(s: &str) -> Option<ApiKey> {
        let k = s.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        (!k.is_empty() && !k.contains(char::is_whitespace)).then(|| ApiKey(k.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    /// "sk-ant-…3f9a": enough to tell keys apart.
    pub fn hint(&self) -> String {
        let k = &self.0;
        let head: String = k.chars().take_while(|c| *c != '-').chain(std::iter::once('-')).collect();
        let head = if k.starts_with("sk-ant-") { "sk-ant-".to_owned() } else { head };
        let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{head}…{tail}")
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApiKey({})", self.hint())
    }
}

/// A model the account can use, as the provider lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudModelInfo {
    pub id: String,
    pub name: String,
    /// Its context window, when the list says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
}

/// Why a key didn't check out, in words a person can act on.
#[derive(Clone, Debug, PartialEq)]
pub enum CheckError {
    /// The provider doesn't know the key.
    Rejected,
    /// The key works but the account can't pay for requests.
    Billing(String),
    /// Nothing answered.
    Unreachable(String),
    Other {
        status: u16,
        message: String,
    },
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Rejected => write!(f, "the key wasn't accepted; check that it was copied whole"),
            CheckError::Billing(m) => write!(f, "the account can't pay for requests yet: {m}"),
            CheckError::Unreachable(e) => write!(f, "couldn't reach the service: {e}"),
            CheckError::Other { status, message } => write!(f, "the service answered {status}: {message}"),
        }
    }
}

/// Request headers that carry the key.
pub fn auth_headers(provider: CloudProvider, key: &ApiKey) -> Vec<(&'static str, String)> {
    match provider {
        CloudProvider::Anthropic => {
            vec![("x-api-key", key.expose().to_owned()), ("anthropic-version", ANTHROPIC_VERSION.to_owned())]
        }
        CloudProvider::OpenAi => vec![("Authorization", format!("Bearer {}", key.expose()))],
    }
}

/// The Messages API version Tasuke speaks.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";

/// Check a key by listing the account's models: free, and proof that the
/// key works. The models come back newest first, chat models only.
pub fn check(provider: CloudProvider, key: &ApiKey) -> Result<Vec<CloudModelInfo>, CheckError> {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut req = agent.get(format!("{}/models?limit=100", provider.api_base()));
    for (k, v) in auth_headers(provider, key) {
        req = req.header(k, &v);
    }
    let resp = req.call().map_err(|e| CheckError::Unreachable(e.to_string()))?;
    let status = resp.status().as_u16();
    let text = resp.into_body().read_to_string().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let message = v
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| text.chars().take(200).collect());
    match status {
        200 => Ok(models_of(provider, &v)),
        401 | 403 => Err(CheckError::Rejected),
        402 => Err(CheckError::Billing(message)),
        429 if v.pointer("/error/code").and_then(Value::as_str) == Some("insufficient_quota") => {
            Err(CheckError::Billing(message))
        }
        _ => Err(CheckError::Other { status, message }),
    }
}

/// The chat models in a `/v1/models` answer.
pub fn models_of(provider: CloudProvider, v: &Value) -> Vec<CloudModelInfo> {
    let items = v.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out: Vec<(i64, CloudModelInfo)> = items
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?.to_owned();
            if !chat_model(provider, &id) {
                return None;
            }
            let name = m.get("display_name").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| id.clone());
            let context = m.get("max_input_tokens").and_then(Value::as_u64);
            let created = m.get("created").and_then(Value::as_i64).unwrap_or(0);
            Some((created, CloudModelInfo { id, name, context }))
        })
        .collect();
    // Anthropic lists newest first; OpenAI by nothing in particular.
    if provider == CloudProvider::OpenAi {
        out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    }
    out.into_iter().map(|(_, m)| m).collect()
}

/// Whether a listed model is one to chat with (OpenAI lists speech,
/// images and embeddings too).
fn chat_model(provider: CloudProvider, id: &str) -> bool {
    match provider {
        CloudProvider::Anthropic => id.starts_with("claude-"),
        CloudProvider::OpenAi => {
            const NOT: [&str; 9] =
                ["audio", "tts", "transcribe", "realtime", "embedding", "image", "search", "moderation", "instruct"];
            (id.starts_with("gpt-") || id.starts_with('o')) && !NOT.iter().any(|n| id.contains(n))
        }
    }
}

/// What a person's config names: a provider and its model id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CloudModelRef {
    pub provider: CloudProvider,
    pub id: String,
}

impl CloudModelRef {
    /// "anthropic/claude-sonnet-5-5": the key measured speeds are kept under.
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider.as_str(), self.id)
    }

    /// "anthropic/claude-sonnet-5-5" or "anthropic:claude-sonnet-5-5".
    pub fn parse(s: &str) -> Option<CloudModelRef> {
        let (p, id) = s.split_once(['/', ':'])?;
        let provider = CloudProvider::parse(p)?;
        let id = id.trim();
        (!id.is_empty()).then(|| CloudModelRef { provider, id: id.to_owned() })
    }
}

impl fmt::Display for CloudModelRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_key_never_prints_whole() {
        let k = ApiKey::new("  sk-ant-api03-abcdefghijklmnop1234\n").expect("a key");
        assert_eq!(k.expose(), "sk-ant-api03-abcdefghijklmnop1234");
        assert_eq!(k.hint(), "sk-ant-…1234");
        assert!(!format!("{k:?}").contains("abcdefgh"));
        assert_eq!(ApiKey::new("sk-proj-xyz9876").expect("a key").hint(), "sk-…9876");
        assert_eq!(ApiKey::new("   "), None);
        assert_eq!(ApiKey::new("two words"), None);
    }

    #[test]
    fn the_model_lists_keep_chat_models() {
        let anthropic = json!({"data": [
            {"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "max_input_tokens": 1000000},
            {"id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5"},
        ]});
        let m = models_of(CloudProvider::Anthropic, &anthropic);
        assert_eq!(m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["claude-opus-5-5", "claude-haiku-5-5"]);
        assert_eq!(m[0].name, "Claude Opus 5.5");
        assert_eq!(m[0].context, Some(1_000_000));
        let openai = json!({"data": [
            {"id": "gpt-6-luna", "created": 2},
            {"id": "text-embedding-3-large", "created": 9},
            {"id": "gpt-6.1-terra", "created": 3},
            {"id": "gpt-4o-transcribe", "created": 5},
            {"id": "dall-e-3", "created": 1},
        ]});
        let m = models_of(CloudProvider::OpenAi, &openai);
        assert_eq!(m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["gpt-6.1-terra", "gpt-6-luna"]);
    }

    #[test]
    fn refs_parse_both_ways_people_write_them() {
        let r = CloudModelRef::parse("anthropic/claude-sonnet-5-5").expect("a ref");
        assert_eq!(r, CloudModelRef { provider: CloudProvider::Anthropic, id: "claude-sonnet-5-5".into() });
        assert_eq!(CloudModelRef::parse("openai:gpt-6-luna").map(|r| r.key()).as_deref(), Some("openai/gpt-6-luna"));
        assert_eq!(CloudModelRef::parse("gemma-4-31b"), None);
    }
}
