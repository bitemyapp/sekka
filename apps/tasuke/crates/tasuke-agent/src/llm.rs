//! An OpenAI-compatible chat client with streaming (llama.cpp's
//! llama-server, vLLM and SGLang serve this API), with llama.cpp's extras:
//! `cache_prompt`, `timings`, `/props`, and router mode's 503 while a
//! model loads.

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::chat::{Content, Message, Part, Role, WireCall};
use crate::ids::{CallId, Tokens};

/// A piece of the reply as it streams.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Delta {
    Reasoning(String),
    Text(String),
}

/// A whole reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Reply {
    pub content: String,
    pub reasoning: String,
    pub calls: Vec<WireCall>,
    pub finish_reason: Option<String>,
    pub prompt_tokens: Tokens,
    pub completion_tokens: Tokens,
    pub cached_tokens: Tokens,
    pub decode_tps: Option<f32>,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LlmError {
    Cancelled,
    /// The server couldn't be reached (after retries).
    Unreachable(String),
    /// The server couldn't parse the model's tool call (llama-server's 500).
    ToolCallParse(String),
    /// The prompt is longer than the context.
    ContextFull(String),
    Http { status: u16, body: String },
    /// The stream broke off or made no sense.
    Stream(String),
    /// The model got stuck repeating itself (twice in a row).
    Degenerate(String),
}

impl LlmError {
    pub(crate) fn retryable(&self) -> bool {
        matches!(self, LlmError::Unreachable(_) | LlmError::Stream(_) | LlmError::Degenerate(_))
            || matches!(self, LlmError::Http { status, .. } if *status >= 500)
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Cancelled => write!(f, "cancelled"),
            LlmError::Unreachable(e) => write!(f, "the model server can't be reached: {e}"),
            LlmError::ToolCallParse(e) => write!(f, "the server couldn't parse the model's tool call: {e}"),
            LlmError::ContextFull(e) => write!(f, "the conversation is longer than the model's context: {e}"),
            LlmError::Http { status, body } => write!(f, "the model server answered {status}: {body}"),
            LlmError::Stream(e) => write!(f, "the model's reply broke off: {e}"),
            LlmError::Degenerate(e) => write!(f, "the model got stuck repeating itself ({e})"),
        }
    }
}

/// Things the client reports while it waits (the model is loading).
pub(crate) type NoticeFn<'a> = &'a mut dyn FnMut(String);

#[derive(Clone)]
pub(crate) struct Client {
    agent: ureq::Agent,
    base_url: String,
    api_key: Option<String>,
    pub model: String,
}

/// How long router mode may take to load a model before Tasuke gives up.
const LOADING_LIMIT: Duration = Duration::from_secs(600);
const CONNECT_RETRIES: u32 = 4;

impl Client {
    pub(crate) fn new(base_url: &str, model: &str, api_key: Option<String>, timeout: Duration) -> Client {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(timeout))
            .build();
        Client {
            agent: ureq::Agent::new_with_config(config),
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: api_key.filter(|k| !k.is_empty()),
            model: model.to_owned(),
        }
    }

    /// The server's root (without `/v1`), where llama.cpp's `/props` is.
    fn root(&self) -> &str {
        self.base_url.strip_suffix("/v1").unwrap_or(&self.base_url)
    }

    /// Whether anything answers at the server's address (any HTTP status).
    pub(crate) fn reachable(&self) -> bool {
        let url = format!("{}/health", self.root());
        self.agent.get(&url).config().timeout_global(Some(Duration::from_secs(2))).build().call().is_ok()
    }

    /// The context size the server gives each request (`n_ctx` in `/props`).
    pub(crate) fn context_size(&self) -> Option<Tokens> {
        let urls = [format!("{}/props?model={}", self.root(), url_escape(&self.model)), format!("{}/props", self.root())];
        for url in urls {
            let mut req = self.agent.get(&url).config().timeout_global(Some(Duration::from_secs(5))).build();
            if let Some(k) = &self.api_key {
                req = req.header("Authorization", &format!("Bearer {k}"));
            }
            let Ok(resp) = req.call() else { continue };
            if resp.status() != 200 {
                continue;
            }
            let Ok(text) = resp.into_body().read_to_string() else { continue };
            let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
            let n = v.pointer("/default_generation_settings/n_ctx").or_else(|| v.get("n_ctx")).and_then(Value::as_u64);
            if let Some(n) = n.filter(|&n| n > 0) {
                return Some(Tokens(n));
            }
        }
        None
    }

    /// Send one chat request and stream its reply. Retries what is worth
    /// retrying: connection failures, a model still loading (503, reported
    /// once as a notice), one resample after an unparseable tool call.
    pub(crate) fn chat(
        &self,
        body: &Value,
        cancel: &AtomicBool,
        on_delta: &mut dyn FnMut(Delta),
        on_notice: NoticeFn<'_>,
    ) -> Result<Reply, LlmError> {
        let mut connect_failures = 0;
        let mut server_errors = 0;
        let mut parse_failures = 0;
        let mut degenerate = 0;
        let mut loading_since: Option<Instant> = None;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(LlmError::Cancelled);
            }
            let started = Instant::now();
            match self.attempt(body, cancel, on_delta) {
                Ok(mut reply) => {
                    reply.elapsed = started.elapsed();
                    return Ok(reply);
                }
                Err(Attempt::Fatal(e)) => return Err(e),
                Err(Attempt::Connect(e)) => {
                    connect_failures += 1;
                    if connect_failures > CONNECT_RETRIES {
                        return Err(LlmError::Unreachable(e));
                    }
                    // 1, 2, 4, 8 s: long enough for a server that is starting, short
                    // enough that a stopped one is reported soon.
                    sleep_unless_cancelled(Duration::from_millis(500u64 << connect_failures.min(4)), cancel)?;
                }
                Err(Attempt::Loading(msg)) => {
                    let since = *loading_since.get_or_insert_with(|| {
                        on_notice(format!("Loading the model {} …", self.model));
                        Instant::now()
                    });
                    if since.elapsed() > LOADING_LIMIT {
                        return Err(LlmError::Http { status: 503, body: msg });
                    }
                    sleep_unless_cancelled(Duration::from_secs(2), cancel)?;
                }
                Err(Attempt::Degenerate(msg)) => {
                    // A small model can fall into a loop (Gemma 4 12B repeats its
                    // channel markers until the token limit): sample again once.
                    degenerate += 1;
                    if degenerate > 1 {
                        return Err(LlmError::Degenerate(msg));
                    }
                }
                Err(Attempt::ParseToolCall(msg)) => {
                    // One resample; then the loop tells the model.
                    parse_failures += 1;
                    if parse_failures > 1 {
                        return Err(LlmError::ToolCallParse(msg));
                    }
                }
                Err(Attempt::Server { status, body: text }) => {
                    server_errors += 1;
                    if server_errors > 2 {
                        return Err(LlmError::Http { status, body: text });
                    }
                    sleep_unless_cancelled(Duration::from_secs(3), cancel)?;
                }
            }
        }
    }

    fn attempt(&self, body: &Value, cancel: &AtomicBool, on_delta: &mut dyn FnMut(Delta)) -> Result<Reply, Attempt> {
        let (tx, rx) = mpsc::channel::<Chunk>();
        let url = format!("{}/chat/completions", self.base_url);
        let agent = self.agent.clone();
        let key = self.api_key.clone();
        let bytes = serde_json::to_vec(body).map_err(|e| Attempt::Fatal(LlmError::Stream(e.to_string())))?;
        // The request runs on its own thread so a cancel needn't wait for
        // the next token (prefill can take minutes); dropping the receiver
        // makes the thread drop the connection, which stops the server.
        std::thread::Builder::new()
            .name("tasuke-llm".into())
            .spawn(move || stream_request(agent, url, key, bytes, tx))
            .map_err(|e| Attempt::Fatal(LlmError::Stream(e.to_string())))?;
        let mut acc = Accumulator::default();
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Attempt::Fatal(LlmError::Cancelled));
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Chunk::Data(data)) => {
                    let v: Value = match serde_json::from_str(&data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if let Some(err) = v.get("error") {
                        return Err(classify_error(500, &error_text(err)));
                    }
                    for d in acc.feed(&v) {
                        on_delta(d);
                    }
                    if let Some(unit) = acc.degenerate() {
                        return Err(Attempt::Degenerate(format!("it kept writing {unit:?}")));
                    }
                }
                Ok(Chunk::StreamError(text)) => return Err(classify_error(500, &text)),
                Ok(Chunk::Status(status, text)) => return Err(classify_error(status, &text)),
                Ok(Chunk::Connect(e)) => return Err(Attempt::Connect(e)),
                Ok(Chunk::Broken(e)) => {
                    return Err(Attempt::Fatal(LlmError::Stream(e)));
                }
                Ok(Chunk::Done) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Ok(acc.finish())
    }
}

fn sleep_unless_cancelled(d: Duration, cancel: &AtomicBool) -> Result<(), LlmError> {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if cancel.load(Ordering::SeqCst) {
            return Err(LlmError::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(100).min(end.saturating_duration_since(Instant::now())));
    }
    Ok(())
}

enum Attempt {
    Connect(String),
    Loading(String),
    ParseToolCall(String),
    Degenerate(String),
    Server { status: u16, body: String },
    Fatal(LlmError),
}

fn error_text(err: &Value) -> String {
    match err {
        Value::String(s) => s.clone(),
        v => v.get("message").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| v.to_string()),
    }
}

/// What a failed response means for retrying.
fn classify_error(status: u16, text: &str) -> Attempt {
    let lower = text.to_lowercase();
    let message = text.chars().take(2000).collect::<String>();
    if lower.contains("parse tool call") || (lower.contains("tool call") && lower.contains("parse")) {
        return Attempt::ParseToolCall(message);
    }
    if status == 503 || lower.contains("loading model") {
        return Attempt::Loading(message);
    }
    if (lower.contains("context") && (lower.contains("exceed") || lower.contains("too long") || lower.contains("size")))
        || lower.contains("exceeds the available context")
    {
        return Attempt::Fatal(LlmError::ContextFull(message));
    }
    if status >= 500 {
        return Attempt::Server { status, body: message };
    }
    Attempt::Fatal(LlmError::Http { status, body: message })
}

enum Chunk {
    Data(String),
    /// An `error:` line in the stream (llama-server's way of failing mid-stream).
    StreamError(String),
    Status(u16, String),
    Connect(String),
    Broken(String),
    Done,
}

fn stream_request(agent: ureq::Agent, url: String, key: Option<String>, body: Vec<u8>, tx: mpsc::Sender<Chunk>) {
    let mut req = agent.post(&url).header("Content-Type", "application/json").header("Accept", "text/event-stream");
    if let Some(k) = &key {
        req = req.header("Authorization", &format!("Bearer {k}"));
    }
    let resp = match req.send(&body[..]) {
        Ok(r) => r,
        Err(e) => {
            let connect = matches!(
                e,
                ureq::Error::ConnectionFailed | ureq::Error::HostNotFound | ureq::Error::Io(_) | ureq::Error::Timeout(_)
            );
            let _ = tx.send(if connect { Chunk::Connect(e.to_string()) } else { Chunk::Broken(e.to_string()) });
            return;
        }
    };
    let status = resp.status().as_u16();
    if status != 200 {
        let mut text = String::new();
        let _ = resp.into_body().into_reader().take(64 * 1024).read_to_string(&mut text);
        let _ = tx.send(Chunk::Status(status, text));
        return;
    }
    let reader = BufReader::new(resp.into_body().into_reader());
    for line in reader.split(b'\n') {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let _ = tx.send(Chunk::Broken(e.to_string()));
                return;
            }
        };
        let line = String::from_utf8_lossy(&line);
        let line = line.trim_end_matches('\r');
        let chunk = if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim_start();
            if data == "[DONE]" {
                break;
            }
            Chunk::Data(data.to_owned())
        } else if let Some(err) = line.strip_prefix("error:") {
            let err = err.trim();
            let text = serde_json::from_str::<Value>(err).map(|v| error_text(&v)).unwrap_or_else(|_| err.to_owned());
            Chunk::StreamError(text)
        } else {
            continue;
        };
        if tx.send(chunk).is_err() {
            // Cancelled: dropping the reader closes the connection.
            return;
        }
    }
    let _ = tx.send(Chunk::Done);
}

/// Assembles the streamed chunks into a reply.
#[derive(Default)]
struct Accumulator {
    reply: Reply,
    /// Tool calls by their stream index.
    calls: Vec<(u64, Option<String>, String, String)>,
    /// How much text had been looked at for repetition.
    checked: usize,
}

/// The tail that is looked at for a loop, and the longest repeating unit.
const LOOP_WINDOW: usize = 1200;
const LOOP_UNIT: usize = 80;

/// The repeating unit when `text`'s last `LOOP_WINDOW` bytes are one short
/// piece over and over (at least 12 times), as a model stuck in a loop
/// writes; prose, code and tables never are.
fn looping(text: &str) -> Option<String> {
    let b = text.as_bytes();
    if b.len() < LOOP_WINDOW {
        return None;
    }
    let tail = &b[b.len() - LOOP_WINDOW..];
    (1..=LOOP_UNIT).find(|&p| tail[p..].iter().zip(tail).all(|(a, b)| a == b)).map(|p| {
        let unit = String::from_utf8_lossy(&tail[tail.len() - p..]).into_owned();
        unit.chars().take(40).collect()
    })
}

impl Accumulator {
    /// Whether the reply has turned into a loop (checked every 256 bytes).
    fn degenerate(&mut self) -> Option<String> {
        let len = self.reply.content.len() + self.reply.reasoning.len();
        if len < self.checked + 256 {
            return None;
        }
        self.checked = len;
        looping(&self.reply.content).or_else(|| looping(&self.reply.reasoning))
    }

    fn feed(&mut self, v: &Value) -> Vec<Delta> {
        let mut out = Vec::new();
        if let Some(choice) = v.get("choices").and_then(|c| c.get(0)) {
            let delta = choice.get("delta").or_else(|| choice.get("message"));
            if let Some(d) = delta {
                let reasoning = d.get("reasoning_content").or_else(|| d.get("reasoning")).and_then(Value::as_str);
                if let Some(r) = reasoning.filter(|r| !r.is_empty()) {
                    self.reply.reasoning.push_str(r);
                    out.push(Delta::Reasoning(r.to_owned()));
                }
                if let Some(t) = d.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    self.reply.content.push_str(t);
                    out.push(Delta::Text(t.to_owned()));
                }
                for (pos, tc) in d.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    let index = tc.get("index").and_then(Value::as_u64).unwrap_or(pos as u64);
                    let i = match self.calls.iter().position(|c| c.0 == index) {
                        Some(i) => i,
                        None => {
                            self.calls.push((index, None, String::new(), String::new()));
                            self.calls.len() - 1
                        }
                    };
                    let slot = &mut self.calls[i];
                    if let Some(id) = tc.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                        slot.1 = Some(id.to_owned());
                    }
                    let f = tc.get("function");
                    if let Some(name) = f.and_then(|f| f.get("name")).and_then(Value::as_str) {
                        // Sent once by most servers; some repeat it whole.
                        if slot.2.is_empty() || slot.2 != name {
                            if slot.2.is_empty() {
                                slot.2 = name.to_owned();
                            } else if !slot.2.ends_with(name) {
                                slot.2.push_str(name);
                            }
                        }
                    }
                    match f.and_then(|f| f.get("arguments")) {
                        Some(Value::String(a)) => slot.3.push_str(a),
                        // A whole object (non-streaming servers).
                        Some(obj @ Value::Object(_)) => slot.3 = obj.to_string(),
                        _ => {}
                    }
                }
            }
            if let Some(fr) = choice.get("finish_reason").and_then(Value::as_str) {
                self.reply.finish_reason = Some(fr.to_owned());
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            let n = |k: &str| u.get(k).and_then(Value::as_u64);
            if let Some(p) = n("prompt_tokens") {
                self.reply.prompt_tokens = Tokens(p);
            }
            if let Some(c) = n("completion_tokens") {
                self.reply.completion_tokens = Tokens(c);
            }
            if let Some(c) = u.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64) {
                self.reply.cached_tokens = Tokens(c);
            }
        }
        if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
            let n = |k: &str| t.get(k).and_then(Value::as_f64);
            if let Some(tps) = n("predicted_per_second") {
                self.reply.decode_tps = Some(tps as f32);
            }
            if self.reply.prompt_tokens.0 == 0 {
                // llama.cpp's prompt_n counts only what it processed.
                let total = n("prompt_n").unwrap_or(0.0) + n("cache_n").unwrap_or(0.0);
                self.reply.prompt_tokens = Tokens(total as u64);
            }
            if self.reply.completion_tokens.0 == 0
                && let Some(p) = n("predicted_n")
            {
                self.reply.completion_tokens = Tokens(p as u64);
            }
            if self.reply.cached_tokens.0 == 0
                && let Some(c) = n("cache_n")
            {
                self.reply.cached_tokens = Tokens(c as u64);
            }
        }
        out
    }

    fn finish(mut self) -> Reply {
        self.calls.sort_by_key(|c| c.0);
        self.reply.calls = self
            .calls
            .into_iter()
            .filter(|c| !c.2.is_empty())
            .map(|(_, id, name, arguments)| WireCall {
                id: id.map(CallId::new).unwrap_or_else(CallId::generate),
                name,
                arguments,
            })
            .collect();
        self.reply
    }
}

/// The request body for `messages` (OpenAI's format).
pub(crate) fn request_body(
    model: &str,
    messages: &[Message],
    tools: &[Value],
    max_tokens: Tokens,
    params: &Map<String, Value>,
    keep_reasoning: bool,
) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert("messages".into(), Value::Array(messages.iter().map(|m| wire_message(m, keep_reasoning)).collect()));
    body.insert("stream".into(), json!(true));
    body.insert("stream_options".into(), json!({"include_usage": true}));
    body.insert("cache_prompt".into(), json!(true));
    body.insert("max_tokens".into(), json!(max_tokens.0));
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(tools.to_vec()));
        body.insert("tool_choice".into(), json!("auto"));
    }
    for (k, v) in params {
        body.insert(k.clone(), v.clone());
    }
    Value::Object(body)
}

fn wire_message(m: &Message, keep_reasoning: bool) -> Value {
    let role = match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut v = Map::new();
    v.insert("role".into(), json!(role));
    let content = match &m.content {
        Content::Text(t) => json!(t),
        Content::Parts(parts) => Value::Array(
            parts
                .iter()
                .map(|p| match p {
                    Part::Text { text } => json!({"type": "text", "text": text}),
                    Part::Image { path } => match std::fs::read(path) {
                        Ok(png) => json!({"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", base64(&png))}}),
                        Err(_) => json!({"type": "text", "text": "[a screenshot that is no longer on disk]"}),
                    },
                })
                .collect(),
        ),
    };
    v.insert("content".into(), content);
    if m.role == Role::Assistant {
        if keep_reasoning && let Some(r) = m.reasoning.as_ref().filter(|r| !r.is_empty()) {
            v.insert("reasoning_content".into(), json!(r));
        }
        if !m.tool_calls.is_empty() {
            let calls: Vec<Value> = m
                .tool_calls
                .iter()
                .map(|c| {
                    json!({"id": c.id.as_str(), "type": "function",
                           "function": {"name": c.name, "arguments": c.arguments}})
                })
                .collect();
            v.insert("tool_calls".into(), Value::Array(calls));
        }
    }
    if let Some(id) = &m.tool_call_id {
        v.insert("tool_call_id".into(), json!(id.as_str()));
    }
    Value::Object(v)
}

pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn url_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn loops_are_told_from_text() {
        let stuck = format!("Let me check.{}", "<channel|><|channel>thought\n".repeat(80));
        assert_eq!(looping(&stuck).as_deref(), Some("<channel|><|channel>thought\n"));
        assert!(looping(&"x".repeat(2000)).is_some());
        let prose: String = (0..300).map(|i| format!("line {i} of a log\n")).collect();
        assert_eq!(looping(&prose), None);
        let table = "| a | b |\n|---|---|\n".to_owned() + &(0..200).map(|i| format!("| {i} | {} |\n", i * 7)).collect::<String>();
        assert_eq!(looping(&table), None);
        assert_eq!(looping("short"), None);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd, 0x00]), "//79AA==");
    }

    #[test]
    fn accumulates_content_reasoning_and_tool_calls_by_index() {
        let mut a = Accumulator::default();
        let chunks = [
            json!({"choices":[{"delta":{"reasoning_content":"Think"}}]}),
            json!({"choices":[{"delta":{"content":"Let me "}}]}),
            json!({"choices":[{"delta":{"content":"look."}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"run_command","arguments":"{\"comm"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"read_file","arguments":""}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"and\":\"ls\"}"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"{\"path\":\"/etc/hosts\"}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":30,"prompt_tokens_details":{"cached_tokens":100}},
                   "timings":{"predicted_per_second":88.5}}),
        ];
        let mut deltas = Vec::new();
        for c in &chunks {
            deltas.extend(a.feed(c));
        }
        assert_eq!(
            deltas,
            [Delta::Reasoning("Think".into()), Delta::Text("Let me ".into()), Delta::Text("look.".into())]
        );
        let r = a.finish();
        assert_eq!(r.content, "Let me look.");
        assert_eq!(r.reasoning, "Think");
        assert_eq!(r.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(r.calls.len(), 2);
        assert_eq!((r.calls[0].id.as_str(), r.calls[0].name.as_str()), ("c1", "run_command"));
        assert_eq!(r.calls[0].arguments, r#"{"command":"ls"}"#);
        assert_eq!(r.calls[1].arguments, r#"{"path":"/etc/hosts"}"#);
        assert_eq!((r.prompt_tokens, r.completion_tokens, r.cached_tokens), (Tokens(120), Tokens(30), Tokens(100)));
        assert_eq!(r.decode_tps, Some(88.5));
    }

    #[test]
    fn llama_timings_stand_in_for_usage() {
        let mut a = Accumulator::default();
        a.feed(&json!({"choices":[{"delta":{"content":"hi"},"finish_reason":"stop"}],
                       "timings":{"prompt_n":20,"cache_n":80,"predicted_n":5,"predicted_per_second":50.0}}));
        let r = a.finish();
        assert_eq!((r.prompt_tokens, r.completion_tokens, r.cached_tokens), (Tokens(100), Tokens(5), Tokens(80)));
    }

    #[test]
    fn errors_are_classified() {
        assert!(matches!(classify_error(500, "Failed to parse tool call arguments as JSON"), Attempt::ParseToolCall(_)));
        assert!(matches!(classify_error(503, "{\"error\":\"Loading model\"}"), Attempt::Loading(_)));
        assert!(matches!(
            classify_error(400, "the request exceeds the available context size, try increasing it"),
            Attempt::Fatal(LlmError::ContextFull(_))
        ));
        assert!(matches!(classify_error(502, "bad gateway"), Attempt::Server { status: 502, .. }));
        assert!(matches!(classify_error(401, "Invalid API Key"), Attempt::Fatal(LlmError::Http { status: 401, .. })));
    }

    #[test]
    fn wire_messages_carry_calls_images_and_reasoning() {
        let dir = std::env::temp_dir().join(format!("tasuke-llm-{}", crate::ids::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("s.png");
        std::fs::write(&png, b"abc").unwrap();
        let mut a = Message::assistant("");
        a.reasoning = Some("why".into());
        a.tool_calls = vec![WireCall { id: CallId::new("c1"), name: "x".into(), arguments: "{}".into() }];
        let u = Message::new(
            Role::User,
            Content::Parts(vec![Part::Text { text: "screen".into() }, Part::Image { path: png.clone() }]),
        );
        let body = request_body(
            "m",
            &[Message::system("s"), a.clone(), Message::tool(CallId::new("c1"), "out"), u],
            &[json!({"type":"function"})],
            Tokens(100),
            &Map::from_iter([("temperature".to_string(), json!(0.6))]),
            true,
        );
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[1]["reasoning_content"], "why");
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "x");
        assert_eq!(msgs[2]["tool_call_id"], "c1");
        assert_eq!(msgs[3]["content"][1]["image_url"]["url"], "data:image/png;base64,YWJj");
        assert_eq!(body["temperature"], 0.6);
        assert_eq!(body["stream"], true);
        assert_eq!(body["cache_prompt"], true);
        assert_eq!(body["tool_choice"], "auto");
        let without = request_body("m", &[a], &[], Tokens(1), &Map::new(), false);
        assert!(without["messages"][0].get("reasoning_content").is_none());
        assert!(without.get("tools").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A one-connection-per-response HTTP server on loopback.
    fn serve(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for resp in responses {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 65536];
                // Read the headers and the body (Content-Length).
                loop {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                requests.push(String::from_utf8_lossy(&buf).to_string());
                s.write_all(resp.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://127.0.0.1:{port}/v1"), h)
    }

    fn sse(events: &[&str]) -> String {
        let body: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn streams_a_reply_after_the_model_loads() {
        let loading = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 39\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"Loading model\"}}\n\n";
        let ok = sse(&[
            r#"{"choices":[{"delta":{"content":"Hello"}}]}"#,
            r#"{"choices":[{"delta":{"content":" there"},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ]);
        let (url, server) = serve(vec![loading.to_string(), ok]);
        let c = Client::new(&url, "gemma", Some("k".into()), Duration::from_secs(30));
        let cancel = AtomicBool::new(false);
        let mut text = String::new();
        let mut notices = Vec::new();
        let body = request_body("gemma", &[Message::user("hi")], &[], Tokens(10), &Map::new(), false);
        let r = c
            .chat(
                &body,
                &cancel,
                &mut |d| {
                    if let Delta::Text(t) = d {
                        text.push_str(&t)
                    }
                },
                &mut |n| notices.push(n),
            )
            .unwrap();
        assert_eq!(text, "Hello there");
        assert_eq!(r.content, "Hello there");
        assert_eq!(r.finish_reason.as_deref(), Some("stop"));
        assert_eq!(notices.len(), 1, "{notices:?}");
        let requests = server.join().unwrap();
        assert!(requests[1].contains("Authorization: Bearer k") || requests[1].contains("authorization: Bearer k"));
        assert!(requests[1].contains("\"stream\":true"));
    }

    #[test]
    fn an_unparseable_tool_call_is_resampled_once_then_reported() {
        let bad = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 46\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"Failed to parse tool call\"}}";
        let bad = bad.replace("Content-Length: 46", &format!("Content-Length: {}", bad.split("\r\n\r\n").nth(1).unwrap().len()));
        let (url, server) = serve(vec![bad.clone(), bad]);
        let c = Client::new(&url, "m", None, Duration::from_secs(30));
        let body = request_body("m", &[Message::user("hi")], &[], Tokens(10), &Map::new(), false);
        let err = c.chat(&body, &AtomicBool::new(false), &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(err, LlmError::ToolCallParse(_)), "{err:?}");
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[test]
    fn an_error_line_mid_stream_is_an_error() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\nerror: {\"code\":500,\"message\":\"Failed to parse tool call\"}\n\n";
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let (url, server) = serve(vec![resp.clone(), resp]);
        let c = Client::new(&url, "m", None, Duration::from_secs(30));
        let req = request_body("m", &[Message::user("hi")], &[], Tokens(10), &Map::new(), false);
        let err = c.chat(&req, &AtomicBool::new(false), &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(err, LlmError::ToolCallParse(_)), "{err:?}");
        server.join().unwrap();
    }

    #[test]
    fn cancel_returns_at_once_while_the_server_is_silent() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/v1", l.local_addr().unwrap().port());
        // Accepts and never answers (a long prefill).
        let _hold = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            std::thread::sleep(Duration::from_secs(5));
            drop(s);
        });
        let c = Client::new(&url, "m", None, Duration::from_secs(30));
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::SeqCst);
        });
        let t0 = Instant::now();
        let req = request_body("m", &[Message::user("hi")], &[], Tokens(10), &Map::new(), false);
        let err = c.chat(&req, &cancel, &mut |_| {}, &mut |_| {}).unwrap_err();
        assert_eq!(err, LlmError::Cancelled);
        assert!(t0.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn props_give_the_context_size() {
        let body = r#"{"default_generation_settings":{"n_ctx":49152}}"#;
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let (url, server) = serve(vec![resp]);
        let c = Client::new(&url, "m", None, Duration::from_secs(30));
        assert_eq!(c.context_size(), Some(Tokens(49152)));
        let req = &server.join().unwrap()[0];
        assert!(req.starts_with("GET /props?model=m "), "{req}");
    }
}
