//! OpenAI's Responses API (`POST /v1/responses`, streamed): Tasuke's
//! conversation as input items, and the response's events as a reply.
//!
//! Nothing is stored at OpenAI (`store: false`): each request carries the
//! conversation, and the model's own output items go back as they came,
//! its reasoning encrypted (`reasoning.encrypted_content`) so it carries
//! over between steps without OpenAI keeping it.

use serde_json::{Map, Value, json};

use crate::chat::{Content, Message, Part, Role};
use crate::ids::Tokens;
use crate::llm::{Accumulator, Delta, StreamFailure, base64};
use crate::provider::Wire;

/// Output room: reasoning comes out of it.
const MAX_OUTPUT_TOKENS: u64 = 32_000;

/// GPT-5 and later, and the o-series, reason; they take `reasoning`.
fn reasons(model: &str) -> bool {
    let m = model.trim_start_matches("gpt-");
    m.split(['-', '.']).next().and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v >= 5)
        || (model.starts_with('o') && model[1..].starts_with(|c: char| c.is_ascii_digit()))
}

pub(crate) fn body(
    model: &str,
    messages: &[Message],
    tools: &[Value],
    max_tokens: Tokens,
    params: &Map<String, Value>,
) -> Value {
    let instructions: Vec<String> = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.text())
        .filter(|t| !t.is_empty())
        .collect();
    let mut input = Vec::new();
    for m in messages.iter().filter(|m| m.role != Role::System) {
        match m.role {
            Role::User => {
                let content = user_content(&m.content);
                if !content.is_empty() {
                    input.push(json!({"role": "user", "content": content}));
                }
            }
            Role::Tool => input.push(json!({
                "type": "function_call_output",
                "call_id": m.tool_call_id.as_ref().map(|i| i.as_str()).unwrap_or("call"),
                "output": m.content.text(),
            })),
            Role::Assistant => input.extend(assistant_items(m)),
            Role::System => unreachable!("filtered"),
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    if !instructions.is_empty() {
        body.insert("instructions".into(), json!(instructions.join("\n\n")));
    }
    body.insert("input".into(), Value::Array(input));
    body.insert("stream".into(), json!(true));
    body.insert("store".into(), json!(false));
    body.insert("max_output_tokens".into(), json!(max_tokens.0.max(MAX_OUTPUT_TOKENS)));
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(tools.iter().filter_map(tool).collect()));
        body.insert("parallel_tool_calls".into(), json!(true));
    }
    if reasons(model) {
        let mut reasoning = json!({"summary": "auto"});
        if let Some(effort) = params.get("effort").and_then(Value::as_str) {
            reasoning["effort"] = json!(effort);
        }
        body.insert("reasoning".into(), reasoning);
        body.insert("include".into(), json!(["reasoning.encrypted_content"]));
    }
    Value::Object(body)
}

/// A tool's chat-completions schema as a Responses function tool.
fn tool(t: &Value) -> Option<Value> {
    let f = t.get("function")?;
    Some(json!({
        "type": "function",
        "name": f.get("name")?,
        "description": f.get("description").cloned().unwrap_or(json!("")),
        "parameters": f.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}})),
        "strict": false,
    }))
}

fn user_content(content: &Content) -> Vec<Value> {
    match content {
        Content::Text(t) if t.is_empty() => Vec::new(),
        Content::Text(t) => vec![json!({"type": "input_text", "text": t})],
        Content::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } if text.is_empty() => None,
                Part::Text { text } => Some(json!({"type": "input_text", "text": text})),
                Part::Image { path } => Some(match std::fs::read(path) {
                    Ok(png) => {
                        json!({"type": "input_image", "image_url": format!("data:image/png;base64,{}", base64(&png))})
                    }
                    Err(_) => json!({"type": "input_text", "text": "[a screenshot that is no longer on disk]"}),
                }),
            })
            .collect(),
    }
}

fn assistant_items(m: &Message) -> Vec<Value> {
    if let Some(raw) = m.raw.as_ref().filter(|r| r.wire == Wire::Responses) {
        // Unstored items can't be referred to by id; they go back whole.
        return raw
            .items
            .iter()
            .map(|item| {
                let mut item = item.clone();
                if let Some(o) = item.as_object_mut() {
                    o.remove("id");
                    o.remove("status");
                }
                item
            })
            .collect();
    }
    let mut items = Vec::new();
    let text = m.content.text();
    if !text.trim().is_empty() {
        items.push(json!({"role": "assistant", "content": [{"type": "output_text", "text": text}]}));
    }
    for c in &m.tool_calls {
        items
            .push(json!({"type": "function_call", "call_id": c.id.as_str(), "name": c.name, "arguments": c.arguments}));
    }
    items
}

/// One streamed event.
pub(crate) fn feed(acc: &mut Accumulator, v: &Value) -> Result<Vec<Delta>, StreamFailure> {
    let mut out = Vec::new();
    let index = v.get("output_index").and_then(Value::as_u64).unwrap_or(0);
    match v.get("type").and_then(Value::as_str).unwrap_or("") {
        "response.output_item.added" => {
            let item = v.get("item").cloned().unwrap_or(json!({}));
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                let slot = acc.call_at(index);
                slot.1 = item.get("call_id").and_then(Value::as_str).map(str::to_owned);
                slot.2 = item.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
            }
        }
        "response.output_text.delta" => {
            let t = v.get("delta").and_then(Value::as_str).unwrap_or("");
            if !t.is_empty() {
                acc.reply.content.push_str(t);
                out.push(Delta::Text(t.to_owned()));
            }
        }
        "response.reasoning_summary_text.delta" => {
            let t = v.get("delta").and_then(Value::as_str).unwrap_or("");
            if !t.is_empty() {
                acc.reply.reasoning.push_str(t);
                out.push(Delta::Reasoning(t.to_owned()));
            }
        }
        "response.reasoning_summary_part.added" if !acc.reply.reasoning.is_empty() => {
            acc.reply.reasoning.push_str("\n\n");
        }
        "response.function_call_arguments.delta" => {
            let d = v.get("delta").and_then(Value::as_str).unwrap_or("").to_owned();
            acc.call_at(index).3.push_str(&d);
        }
        "response.output_item.done" => {
            let item = v.get("item").cloned().unwrap_or(json!({}));
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                let slot = acc.call_at(index);
                if let Some(id) = item.get("call_id").and_then(Value::as_str) {
                    slot.1 = Some(id.to_owned());
                }
                if let Some(name) = item.get("name").and_then(Value::as_str) {
                    slot.2 = name.to_owned();
                }
                if let Some(args) = item.get("arguments").and_then(Value::as_str) {
                    slot.3 = args.to_owned();
                }
            }
            acc.raw.insert(index, item);
        }
        "response.completed" | "response.incomplete" => {
            let r = v.get("response").cloned().unwrap_or(json!({}));
            if let Some(u) = r.get("usage") {
                let n = |p: &str| u.pointer(p).and_then(Value::as_u64);
                if let Some(i) = n("/input_tokens") {
                    acc.reply.prompt_tokens = Tokens(i);
                }
                if let Some(c) = n("/input_tokens_details/cached_tokens") {
                    acc.reply.cached_tokens = Tokens(c);
                }
                if let Some(o) = n("/output_tokens") {
                    acc.reply.completion_tokens = Tokens(o);
                }
            }
            let cut = r.pointer("/incomplete_details/reason").and_then(Value::as_str) == Some("max_output_tokens");
            acc.reply.finish_reason = Some(
                if cut {
                    "length"
                } else if acc.calls.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                }
                .to_owned(),
            );
        }
        "response.failed" => {
            let e = v.pointer("/response/error").cloned().unwrap_or(json!({}));
            return Err(failure(&e));
        }
        "error" => return Err(failure(v)),
        _ => {}
    }
    Ok(out)
}

fn failure(e: &Value) -> StreamFailure {
    let code = e.get("code").and_then(Value::as_str).unwrap_or("");
    let status = match code {
        "rate_limit_exceeded" | "insufficient_quota" => 429,
        "server_error" => 500,
        "invalid_api_key" => 401,
        "context_length_exceeded" => 400,
        _ => 500,
    };
    let message = e.get("message").and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or(code).to_owned();
    StreamFailure { status, message }
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
               "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}}})
    }

    #[test]
    fn the_conversation_becomes_input_items() {
        let mut call = Message::assistant("");
        call.tool_calls = vec![WireCall {
            id: CallId::new("call_7"),
            name: "run_command".into(),
            arguments: r#"{"command":"df"}"#.into(),
        }];
        let messages = vec![
            Message::system("You are Tasuke."),
            Message::user("Disk?"),
            call,
            Message::tool(CallId::new("call_7"), "100%"),
        ];
        let b = body("gpt-6-luna", &messages, &[schema()], Tokens(4096), &Map::new());
        assert_eq!(b["instructions"], "You are Tasuke.");
        assert_eq!(b["store"], false);
        assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
        let input = b["input"].as_array().expect("input");
        assert_eq!(input[0], json!({"role": "user", "content": [{"type": "input_text", "text": "Disk?"}]}));
        assert_eq!(
            input[1],
            json!({"type": "function_call", "call_id": "call_7", "name": "run_command", "arguments": r#"{"command":"df"}"#})
        );
        assert_eq!(input[2], json!({"type": "function_call_output", "call_id": "call_7", "output": "100%"}));
        assert_eq!(b["tools"][0]["name"], "run_command");
        assert!(reasons("gpt-6.1-terra") && reasons("o4-mini") && !reasons("gpt-4o"));
    }

    #[test]
    fn its_own_items_go_back_without_ids() {
        let mut m = Message::assistant("");
        m.raw = Some(RawTurn {
            wire: Wire::Responses,
            model: "gpt-6-luna".into(),
            items: vec![json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAA"})],
        });
        assert_eq!(assistant_items(&m), vec![json!({"type": "reasoning", "summary": [], "encrypted_content": "gAAA"})]);
    }

    #[test]
    fn a_stream_with_reasoning_text_and_a_call_is_one_reply() {
        let mut acc = Accumulator::new(Wire::Responses, Instant::now());
        let events = [
            json!({"type": "response.output_item.added", "output_index": 0, "item": {"type": "reasoning", "id": "rs_1"}}),
            json!({"type": "response.reasoning_summary_text.delta", "output_index": 0, "delta": "Check the disk."}),
            json!({"type": "response.output_item.done", "output_index": 0, "item": {"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAA"}}),
            json!({"type": "response.output_item.added", "output_index": 1, "item": {"type": "message", "id": "msg_1"}}),
            json!({"type": "response.output_text.delta", "output_index": 1, "delta": "Looking."}),
            json!({"type": "response.output_item.done", "output_index": 1, "item": {"type": "message", "id": "msg_1", "role": "assistant", "content": [{"type": "output_text", "text": "Looking."}]}}),
            json!({"type": "response.output_item.added", "output_index": 2, "item": {"type": "function_call", "id": "fc_1", "call_id": "call_9", "name": "run_command", "arguments": ""}}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 2, "delta": "{\"command\":"}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 2, "delta": "\"df\"}"}),
            json!({"type": "response.output_item.done", "output_index": 2, "item": {"type": "function_call", "id": "fc_1", "call_id": "call_9", "name": "run_command", "arguments": "{\"command\":\"df\"}"}}),
            json!({"type": "response.completed", "response": {"usage": {"input_tokens": 1200, "input_tokens_details": {"cached_tokens": 1000}, "output_tokens": 55}}}),
        ];
        let mut deltas = Vec::new();
        for e in &events {
            deltas.extend(acc.feed(e).ok().expect("no failure"));
        }
        let r = acc.finish();
        assert_eq!(deltas, vec![Delta::Reasoning("Check the disk.".into()), Delta::Text("Looking.".into())]);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].id.as_str(), "call_9");
        assert_eq!(r.calls[0].arguments, r#"{"command":"df"}"#);
        assert_eq!(r.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!((r.prompt_tokens, r.cached_tokens, r.completion_tokens), (Tokens(1200), Tokens(1000), Tokens(55)));
        assert_eq!(r.raw.expect("items").len(), 3);
    }

    #[test]
    fn a_failed_response_says_why() {
        let mut acc = Accumulator::new(Wire::Responses, Instant::now());
        let e = acc
            .feed(&json!({"type": "response.failed", "response": {"error": {"code": "rate_limit_exceeded", "message": "Slow down"}}}))
            .expect_err("a failure");
        assert_eq!((e.status, e.message.as_str()), (429, "Slow down"));
    }
}
