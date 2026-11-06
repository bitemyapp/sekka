//! Anthropic's Messages API (`POST /v1/messages`, streamed): Tasuke's
//! conversation as Claude's request, and Claude's events as a reply.
//!
//! Claude's turns go back exactly as they came ([`crate::provider::RawTurn`]):
//! its thinking is signed and bound to the conversation before it, so the
//! history sent is append-only (the turn loop doesn't trim it for Claude).
//! Tool results for one assistant turn travel together in one user message,
//! first, with any screenshot after them.

use serde_json::{Map, Value, json};

use crate::chat::{Content, Message, Part, Role};
use crate::ids::Tokens;
use crate::llm::{Accumulator, Delta, StreamFailure, base64};
use crate::provider::Wire;

/// Output room: Claude's thinking comes out of `max_tokens`, so the local
/// models' budget is too small for it.
const MAX_TOKENS: u64 = 32_000;

/// Claude 4.6 and later think adaptively; older models take a budget,
/// which Tasuke doesn't set (they answer without thinking).
fn adaptive(model: &str) -> bool {
    let tail = model.trim_start_matches("claude-");
    ["opus-4-6", "opus-4-7", "opus-4-8", "sonnet-4-6"].iter().any(|m| tail.starts_with(m))
        || tail.split('-').nth(1).and_then(|v| v.parse::<u32>().ok()).is_some_and(|major| major >= 5)
}

/// The models whose safety checks may decline a request and which can
/// hand it to another model instead (server-side fallback): Claude Fable
/// 5.1, Claude Opus 5.5, Claude Sonnet 5.5.
fn falls_back(model: &str) -> bool {
    ["claude-fable-5-1", "claude-opus-5-5", "claude-sonnet-5-5"].iter().any(|m| model.starts_with(m))
}

/// The `anthropic-beta` header the request needs, if any.
pub(crate) fn betas(model: &str) -> Option<&'static str> {
    falls_back(model).then_some("server-side-fallback-2026-07-01")
}

pub(crate) fn body(
    model: &str,
    messages: &[Message],
    tools: &[Value],
    max_tokens: Tokens,
    params: &Map<String, Value>,
) -> Value {
    let system: Vec<String> = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.text())
        .filter(|t| !t.is_empty())
        .collect();
    let mut turns: Vec<(&'static str, Vec<Value>)> = Vec::new();
    for m in messages.iter().filter(|m| m.role != Role::System) {
        let (role, blocks) = match m.role {
            Role::User => ("user", user_blocks(&m.content)),
            Role::Tool => ("user", vec![tool_result(m)]),
            Role::Assistant => ("assistant", assistant_blocks(m)),
            Role::System => unreachable!("filtered"),
        };
        if blocks.is_empty() {
            continue;
        }
        // Consecutive messages of one role are one turn: a turn's results,
        // then the screenshot taken after them.
        match turns.last_mut() {
            Some((r, b)) if *r == role => b.extend(blocks),
            _ => turns.push((role, blocks)),
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert("max_tokens".into(), json!(max_tokens.0.max(MAX_TOKENS)));
    body.insert("stream".into(), json!(true));
    if !system.is_empty() {
        body.insert("system".into(), json!(system.join("\n\n")));
    }
    body.insert(
        "messages".into(),
        Value::Array(turns.into_iter().map(|(role, content)| json!({"role": role, "content": content})).collect()),
    );
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(tools.iter().filter_map(tool).collect()));
    }
    if adaptive(model) {
        // A readable summary for the folded "Thinking" line.
        body.insert("thinking".into(), json!({"type": "adaptive", "display": "summarized"}));
    }
    if let Some(effort) = params.get("effort").and_then(Value::as_str) {
        body.insert("output_config".into(), json!({"effort": effort}));
    }
    if falls_back(model) {
        body.insert("fallbacks".into(), json!("default"));
    }
    // Cache the whole prefix: each step resends the conversation so far.
    body.insert("cache_control".into(), json!({"type": "ephemeral"}));
    Value::Object(body)
}

/// A tool's OpenAI-style schema as Claude's.
fn tool(t: &Value) -> Option<Value> {
    let f = t.get("function")?;
    Some(json!({
        "name": f.get("name")?,
        "description": f.get("description").cloned().unwrap_or(json!("")),
        "input_schema": f.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}})),
        // Arguments stream as written (a file's whole text arrives as it's
        // generated); the tools parse and check them before running.
        "eager_input_streaming": true,
    }))
}

fn image_block(path: &std::path::Path) -> Value {
    match std::fs::read(path) {
        Ok(png) => {
            json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64(&png)}})
        }
        Err(_) => json!({"type": "text", "text": "[a screenshot that is no longer on disk]"}),
    }
}

fn user_blocks(content: &Content) -> Vec<Value> {
    match content {
        Content::Text(t) if t.is_empty() => Vec::new(),
        Content::Text(t) => vec![json!({"type": "text", "text": t})],
        Content::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } if text.is_empty() => None,
                Part::Text { text } => Some(json!({"type": "text", "text": text})),
                Part::Image { path } => Some(image_block(path)),
            })
            .collect(),
    }
}

/// Claude's tool-use ids allow letters, digits, `_` and `-`.
fn tool_id(id: &str) -> String {
    let s: String =
        id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "call".into() } else { s }
}

fn tool_result(m: &Message) -> Value {
    let id = m.tool_call_id.as_ref().map(|i| tool_id(i.as_str())).unwrap_or_else(|| "call".into());
    let text = m.content.text();
    json!({"type": "tool_result", "tool_use_id": id, "content": if text.is_empty() { "(no output)".into() } else { text }})
}

fn assistant_blocks(m: &Message) -> Vec<Value> {
    if let Some(raw) = m.raw.as_ref().filter(|r| r.wire == Wire::Anthropic) {
        return echo(&raw.items);
    }
    // A turn another model wrote: its text and calls, rebuilt.
    let mut blocks = Vec::new();
    let text = m.content.text();
    if !text.trim().is_empty() {
        blocks.push(json!({"type": "text", "text": text}));
    }
    for c in &m.tool_calls {
        let input: Value = serde_json::from_str(&c.arguments).ok().filter(Value::is_object).unwrap_or(json!({}));
        blocks.push(json!({"type": "tool_use", "id": tool_id(c.id.as_str()), "name": c.name, "input": input}));
    }
    if blocks.is_empty() {
        blocks.push(json!({"type": "text", "text": "(no answer)"}));
    }
    blocks
}

/// Claude's blocks as they go back. After a fallback (another model took
/// over mid-answer), what the declining model wrote before the last
/// `fallback` marker goes back as text only: its thinking and tool calls
/// are dropped.
fn echo(items: &[Value]) -> Vec<Value> {
    let kind = |b: &Value| b.get("type").and_then(Value::as_str).unwrap_or("").to_owned();
    let Some(boundary) = items.iter().rposition(|b| kind(b) == "fallback") else {
        return items.to_vec();
    };
    items.iter().enumerate().filter(|(i, b)| *i >= boundary || kind(b) == "text").map(|(_, b)| b.clone()).collect()
}

/// One streamed event.
pub(crate) fn feed(acc: &mut Accumulator, v: &Value) -> Result<Vec<Delta>, StreamFailure> {
    let mut out = Vec::new();
    let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
    match v.get("type").and_then(Value::as_str).unwrap_or("") {
        "message_start" => {
            if let Some(u) = v.pointer("/message/usage") {
                usage(acc, u);
            }
        }
        "content_block_start" => {
            let block = v.get("content_block").cloned().unwrap_or(json!({}));
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                let id = block.get("id").and_then(Value::as_str).map(str::to_owned);
                let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
                let slot = acc.call_at(index);
                slot.1 = id;
                slot.2 = name;
            }
            acc.raw.insert(index, block);
        }
        "content_block_delta" => {
            let delta = v.get("delta").cloned().unwrap_or(json!({}));
            let block = acc.raw.entry(index).or_insert_with(|| json!({}));
            let append = |block: &mut Value, key: &str, s: &str| {
                let cur = block.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
                block[key] = json!(cur + s);
            };
            match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                "text_delta" => {
                    let t = delta.get("text").and_then(Value::as_str).unwrap_or("");
                    append(block, "text", t);
                    if !t.is_empty() {
                        acc.reply.content.push_str(t);
                        out.push(Delta::Text(t.to_owned()));
                    }
                }
                "thinking_delta" => {
                    let t = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                    append(block, "thinking", t);
                    if !t.is_empty() {
                        acc.reply.reasoning.push_str(t);
                        out.push(Delta::Reasoning(t.to_owned()));
                    }
                }
                "signature_delta" => {
                    let s = delta.get("signature").and_then(Value::as_str).unwrap_or("");
                    append(block, "signature", s);
                }
                "input_json_delta" => {
                    let s = delta.get("partial_json").and_then(Value::as_str).unwrap_or("").to_owned();
                    acc.call_at(index).3.push_str(&s);
                }
                _ => {}
            }
        }
        "content_block_stop" => {
            // A call's input, whole: into its block as JSON for the replay.
            if let Some(slot) = acc.calls.iter().find(|c| c.0 == index) {
                let input: Value = if slot.3.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&slot.3).ok().filter(Value::is_object).unwrap_or(json!({}))
                };
                if let Some(block) = acc.raw.get_mut(&index) {
                    block["input"] = input;
                }
            }
        }
        "message_delta" => {
            if let Some(reason) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                acc.reply.finish_reason = Some(
                    match reason {
                        "end_turn" | "stop_sequence" | "pause_turn" => "stop",
                        "tool_use" => "tool_calls",
                        "max_tokens" | "model_context_window_exceeded" => "length",
                        other => other,
                    }
                    .to_owned(),
                );
            }
            if let Some(u) = v.get("usage") {
                usage(acc, u);
            }
        }
        "error" => {
            let e = v.get("error").cloned().unwrap_or(json!({}));
            let kind = e.get("type").and_then(Value::as_str).unwrap_or("");
            let status = match kind {
                "overloaded_error" => 529,
                "rate_limit_error" => 429,
                "authentication_error" => 401,
                "permission_error" => 403,
                "invalid_request_error" => 400,
                _ => 500,
            };
            let message = e.get("message").and_then(Value::as_str).unwrap_or(kind).to_owned();
            return Err(StreamFailure { status, message });
        }
        _ => {}
    }
    Ok(out)
}

/// Claude's usage: prompt tokens are the fresh input plus what the cache
/// read and wrote.
fn usage(acc: &mut Accumulator, u: &Value) {
    let n = |k: &str| u.get(k).and_then(Value::as_u64);
    if let Some(input) = n("input_tokens") {
        let read = n("cache_read_input_tokens").unwrap_or(0);
        let written = n("cache_creation_input_tokens").unwrap_or(0);
        acc.reply.prompt_tokens = Tokens(input + read + written);
        acc.reply.cached_tokens = Tokens(read);
    }
    if let Some(o) = n("output_tokens") {
        acc.reply.completion_tokens = Tokens(o);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::WireCall;
    use crate::ids::CallId;
    use crate::provider::RawTurn;
    use std::time::Instant;

    fn schema() -> Value {
        json!({"type": "function", "function": {"name": "run_command", "description": "Run a command.",
               "parameters": {"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}}})
    }

    #[test]
    fn the_conversation_becomes_claude_turns() {
        let mut call = Message::assistant("Let me look.");
        call.tool_calls = vec![WireCall {
            id: CallId::new("call_1"),
            name: "run_command".into(),
            arguments: r#"{"command":"df -h"}"#.into(),
        }];
        let messages = vec![
            Message::system("You are Tasuke."),
            Message::user("Why is my disk full?"),
            call,
            Message::tool(CallId::new("call_1"), "/dev/sda1 100%"),
            Message::new(Role::User, Content::Parts(vec![Part::Text { text: "The screen now:".into() }])),
        ];
        let b = body("claude-sonnet-5-5", &messages, &[schema()], Tokens(4096), &Map::new());
        assert_eq!(b["system"], "You are Tasuke.");
        assert_eq!(b["max_tokens"], 32_000, "room for thinking");
        assert_eq!(b["thinking"], json!({"type": "adaptive", "display": "summarized"}));
        assert_eq!(b["fallbacks"], "default");
        let m = b["messages"].as_array().expect("messages");
        assert_eq!(m.len(), 3, "user, assistant, then the results and the screen as one user turn");
        assert_eq!(
            m[1]["content"][1],
            json!({"type": "tool_use", "id": "call_1", "name": "run_command", "input": {"command": "df -h"}})
        );
        assert_eq!(m[2]["content"][0]["type"], "tool_result", "results first");
        assert_eq!(m[2]["content"][1]["text"], "The screen now:");
        assert_eq!(b["tools"][0]["input_schema"]["required"][0], "command");
        assert_eq!(b["tools"][0]["eager_input_streaming"], true);
        assert!(b.get("temperature").is_none(), "sampling settings are refused on current models");
        let haiku = body("claude-haiku-5-5", &messages, &[], Tokens(4096), &Map::new());
        assert!(haiku.get("fallbacks").is_none());
        assert_eq!(betas("claude-haiku-5-5"), None);
        assert_eq!(betas("claude-opus-5-5"), Some("server-side-fallback-2026-07-01"));
        assert!(body("claude-haiku-4-5", &messages, &[], Tokens(4096), &Map::new()).get("thinking").is_none());
    }

    #[test]
    fn claudes_own_turns_go_back_unchanged() {
        let blocks = vec![
            json!({"type": "thinking", "thinking": "Check the disk.", "signature": "sig=="}),
            json!({"type": "tool_use", "id": "toolu_1", "name": "run_command", "input": {"command": "df"}}),
        ];
        let mut m = Message::assistant("");
        m.raw = Some(RawTurn { wire: Wire::Anthropic, model: "claude-opus-5-5".into(), items: blocks.clone() });
        assert_eq!(assistant_blocks(&m), blocks);
        let fell_back = vec![
            json!({"type": "thinking", "thinking": "", "signature": "a"}),
            json!({"type": "text", "text": "Part one."}),
            json!({"type": "tool_use", "id": "toolu_x", "name": "run_command", "input": {}}),
            json!({"type": "fallback", "from": {"model": "claude-opus-5-5"}, "to": {"model": "claude-opus-4-8"}}),
            json!({"type": "text", "text": "Part two."}),
        ];
        let echoed = echo(&fell_back);
        let kinds: Vec<&str> = echoed.iter().map(|b| b["type"].as_str().unwrap_or("")).collect();
        assert_eq!(kinds, ["text", "fallback", "text"]);
    }

    #[test]
    fn a_stream_with_thinking_text_and_a_call_is_one_reply() {
        let mut acc = Accumulator::new(Wire::Anthropic, Instant::now());
        let events = [
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 12, "cache_read_input_tokens": 900, "output_tokens": 1}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Disk first."}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "EqQ="}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Looking."}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_9", "name": "run_command", "input": {}}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"command\": "}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "\"df -h\"}"}}),
            json!({"type": "content_block_stop", "index": 2}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 40}}),
            json!({"type": "message_stop"}),
        ];
        let mut deltas = Vec::new();
        for e in &events {
            deltas.extend(acc.feed(e).ok().expect("no failure"));
        }
        let r = acc.finish();
        assert_eq!(deltas, vec![Delta::Reasoning("Disk first.".into()), Delta::Text("Looking.".into())]);
        assert_eq!(r.content, "Looking.");
        assert_eq!(r.reasoning, "Disk first.");
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].id.as_str(), "toolu_9");
        assert_eq!(r.calls[0].arguments, r#"{"command": "df -h"}"#);
        assert_eq!(r.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(r.prompt_tokens, Tokens(912));
        assert_eq!(r.cached_tokens, Tokens(900));
        assert_eq!(r.completion_tokens, Tokens(40));
        let raw = r.raw.expect("raw blocks");
        assert_eq!(raw[0], json!({"type": "thinking", "thinking": "Disk first.", "signature": "EqQ="}));
        assert_eq!(raw[2]["input"], json!({"command": "df -h"}), "the input, whole, for the replay");
    }

    #[test]
    fn overload_mid_stream_is_retryable() {
        let mut acc = Accumulator::new(Wire::Anthropic, Instant::now());
        let e = acc
            .feed(&json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}))
            .expect_err("a failure");
        assert_eq!(e.status, 529);
    }
}
