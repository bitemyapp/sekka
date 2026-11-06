//! The agent loop for one user turn: ask the model, run the tools it calls
//! (asking the person where the policy says so), until it answers without
//! a tool call, the step budget or time runs out, or the person stops it.
//!
//! Everything is written to the session as it happens: the person's text,
//! the events (streamed text coalesced), and the model's messages.

use std::time::Instant;

use crate::agent::Control;
use crate::chat::{Content, Message, Part, RawTurn, Role, WireCall};
use crate::context;
use crate::desktop::Desktop;
use crate::event::{Approval, Decision, Event, FinishReason, Level, ToolCall, ToolOutput, ToolStatus, Usage};
use crate::ids::{CallId, Timestamp, Tokens, TurnNumber, random_hex};
use crate::llm::{Client, Delta, LlmError};
use crate::memory::Memory;
use crate::options::Options;
use crate::session::{Record, Session, Writer};
use crate::textcalls;
use crate::tools;

/// Malformed tool calls the model is told about before the turn gives up.
const MALFORMED_LIMIT: u32 = 3;
/// Screenshots kept in the context.
const KEEP_IMAGES: usize = 2;
/// Tool outputs kept whole when the context is trimmed.
const KEEP_OUTPUTS: usize = 4;
/// Trim old outputs above this share of the context; summarize above the next.
const TRIM_AT: f64 = 0.55;
const SUMMARIZE_AT: f64 = 0.75;

/// Sends events and writes them to the session. Streamed text is written
/// as one event per run of deltas, so a long answer is a line, not a
/// thousand.
struct Out<'a> {
    ctl: &'a Control,
    writer: Option<Writer>,
    turn: TurnNumber,
    pending: Option<Event>,
}

impl Out<'_> {
    fn send(&mut self, event: Event) {
        self.ctl.send(event.clone());
        match (&mut self.pending, &event) {
            (Some(Event::TextDelta { text: a }), Event::TextDelta { text: b }) => a.push_str(b),
            (Some(Event::ReasoningDelta { text: a }), Event::ReasoningDelta { text: b }) => a.push_str(b),
            _ => {
                self.flush();
                if matches!(event, Event::TextDelta { .. } | Event::ReasoningDelta { .. }) {
                    self.pending = Some(event);
                } else {
                    self.record(&Record::Event { turn: self.turn, event });
                }
            }
        }
    }

    fn flush(&mut self) {
        if let Some(e) = self.pending.take() {
            self.record(&Record::Event { turn: self.turn, event: e });
        }
    }

    fn record(&mut self, record: &Record) {
        if let Some(w) = &mut self.writer
            && let Err(e) = w.append(record)
        {
            log::warn!("tasuke: writing the session: {e}");
            self.writer = None;
        }
    }

    fn message(&mut self, history: &mut Vec<Message>, m: Message) {
        self.flush();
        self.record(&Record::Message { message: m.clone() });
        history.push(m);
    }

    fn finish(&mut self, reason: FinishReason, usage: Usage, decode_tps: Option<f32>) {
        self.send(Event::TurnFinished { reason, usage, decode_tps });
        self.flush();
        if let Some(w) = &mut self.writer {
            let _ = w.sync();
        }
    }
}

pub(crate) fn run(session: &Session, text: &str, mut options: Options, ctl: &Control) {
    options.session = Some(session.id().clone());
    // A cloud key is looked up here, on the turn's thread: a locked keyring may
    // ask to be unlocked, and the window mustn't wait for that.
    options.resolve_key();
    let (mut history, last) = session.history().unwrap_or_else(|_| (Vec::new(), TurnNumber(0)));
    let turn = last.next();
    let writer = match Writer::open(session) {
        Ok(w) => Some(w),
        Err(e) => {
            ctl.send(Event::Notice { level: Level::Warning, text: format!("This conversation can't be saved: {e}") });
            None
        }
    };
    let mut out = Out { ctl, writer, turn, pending: None };
    out.record(&Record::User { turn, text: text.to_owned(), at: Timestamp::now() });
    out.send(Event::TurnStarted { turn });

    let first = history.is_empty();
    let user = if first { format!("{}\n\n{text}", context::environment_block(&options)) } else { text.to_owned() };
    out.message(&mut history, Message::user(user));

    let mut usage = Usage::default();
    let (reason, tps) = Loop::new(&options, ctl, session).run(&mut out, &mut history, &mut usage);
    out.finish(reason, usage, tps);
}

struct Loop<'a> {
    options: &'a Options,
    ctl: &'a Control,
    client: Client,
    desktop: Desktop<'a>,
    started: Instant,
}

impl<'a> Loop<'a> {
    fn new(options: &'a Options, ctl: &'a Control, session: &Session) -> Loop<'a> {
        let client = Client::new(
            &options.base_url,
            &options.model,
            options.api_key.clone(),
            options.request_timeout,
            options.wire(),
        );
        let desktop = Desktop::new(options, ctl.cancel_flag(), session.files_dir());
        Loop { options, ctl, client, desktop, started: Instant::now() }
    }

    /// Tasuke's own server is a user unit; if it isn't answering, start it
    /// (router mode loads the model on the first request).
    fn ensure_server(&self, out: &mut Out<'_>) {
        let own = format!("http://127.0.0.1:{}/v1", crate::options::DEFAULT_PORT);
        if self.options.provider.is_some()
            || self.options.base_url.trim_end_matches('/') != own
            || self.client.reachable()
        {
            return;
        }
        out.send(Event::Notice { level: Level::Info, text: "Starting the model server…".into() });
        let argv: Vec<String> =
            ["systemctl", "--user", "start", "tasuke-llm.service"].iter().map(|s| (*s).to_owned()).collect();
        let _ = crate::exec::run(&argv, None, &[], None, std::time::Duration::from_secs(20), self.ctl.cancel_flag());
    }

    fn fill(&self, history: &[Message], system: &str, n_ctx: Tokens) -> f64 {
        let est = context::estimate(history).0 + system.len() as u64 / 3 + 1500;
        est as f64 / n_ctx.0.max(1) as f64
    }

    fn run(&mut self, out: &mut Out<'_>, history: &mut Vec<Message>, usage: &mut Usage) -> (FinishReason, Option<f32>) {
        let o = self.options;
        if let Some(p) = o.provider
            && o.api_key.is_none()
        {
            out.send(Event::Error {
                text: format!(
                    "No {} account is connected: connect it in Models, or choose a local model.",
                    p.company()
                ),
                retryable: false,
            });
            return (FinishReason::Failed, None);
        }
        self.ensure_server(out);
        let n_ctx = self.client.context_size().unwrap_or(o.context_fallback);
        let memory = Memory::load(&o.data_dir).map(|m| m.prompt_block()).unwrap_or_default();
        // Fixed for the whole turn, so llama.cpp's prompt cache keeps hitting: a
        // note remembered now reaches the prompt from the next turn (the tool's
        // result already tells the model).
        let system = context::system_prompt(o, &memory);
        let mut desktop_asked = false;
        let mut malformed = 0;
        let mut tps = None;
        let mut summarized = false;
        // With Desktop control on, the request comes with a first look at
        // the screen, so the model starts from what the person sees.
        if o.desktop_control && o.act {
            let obs = self.desktop.observe();
            if let Some(last) = history.last_mut() {
                let mut parts = vec![Part::Text { text: format!("{}\n\n{}", last.content.text(), obs.text) }];
                if let Some(img) = obs.image {
                    parts.push(Part::Image { path: img });
                }
                last.content = Content::Parts(parts);
            }
        }
        loop {
            if self.ctl.cancelled() {
                return (FinishReason::Cancelled, tps);
            }
            if usage.tool_calls >= o.step_budget.0 {
                out.send(Event::Notice {
                    level: Level::Warning,
                    text: format!("Stopped after {} steps, this request's limit.", o.step_budget.0),
                });
                return (FinishReason::StepBudget, tps);
            }
            if let Some(limit) = o.time_limit
                && self.started.elapsed() > limit
            {
                out.send(Event::Notice { level: Level::Warning, text: "Stopped at this request's time limit.".into() });
                return (FinishReason::TimeLimit, tps);
            }

            // Keep inside the context: drop old screenshots always, trim
            // old outputs once the context is half full, summarize at 3/4.
            // Not for Claude: an edited history voids its later thinking,
            // and its context is a million tokens, cached.
            if !o.wire().append_only() {
                context::drop_old_images(history, KEEP_IMAGES);
                if self.fill(history, &system, n_ctx) > TRIM_AT && context::stub_old_outputs(history, KEEP_OUTPUTS) > 0
                {
                    out.send(Event::Notice {
                        level: Level::Info,
                        text: "Trimmed older command output to make room.".into(),
                    });
                }
                if self.fill(history, &system, n_ctx) > SUMMARIZE_AT && !summarized {
                    summarized = true;
                    self.summarize(out, history, &system, usage);
                }
            }

            let offered = tools::schemas(o, self.screen_size());
            let names = tools::names(&offered);
            let mut messages = Vec::with_capacity(history.len() + 1);
            messages.push(Message::system(system.clone()));
            messages.extend(history.iter().cloned());
            let body = self.client.body(&messages, &offered, o.max_output_tokens, &o.request_params, o.keep_reasoning);
            let reply = {
                let ctl = self.ctl;
                let mut deltas = Vec::new();
                let mut on_delta = |d: Delta| {
                    let e = match d {
                        Delta::Text(t) => Event::TextDelta { text: t },
                        Delta::Reasoning(t) => Event::ReasoningDelta { text: t },
                    };
                    ctl.send(e.clone());
                    deltas.push(e);
                };
                let mut notices = Vec::new();
                let mut on_notice = |t: String| {
                    ctl.send(Event::Notice { level: Level::Info, text: t.clone() });
                    notices.push(t);
                };
                let r = self.client.chat(&body, ctl.cancel_flag(), &mut on_delta, &mut on_notice);
                // Recorded after the fact (sent live above), coalesced.
                for t in notices {
                    out.record(&Record::Event { turn: out.turn, event: Event::Notice { level: Level::Info, text: t } });
                }
                for e in deltas {
                    out.coalesce(e);
                }
                r
            };
            let reply = match reply {
                Ok(r) => r,
                Err(LlmError::Cancelled) => return (FinishReason::Cancelled, tps),
                Err(LlmError::ToolCallParse(e)) if malformed < MALFORMED_LIMIT => {
                    malformed += 1;
                    log::info!("tasuke: unparseable tool call: {e}");
                    out.message(history, Message::user(context::PARSE_RETRY));
                    continue;
                }
                Err(LlmError::ContextFull(_)) if !summarized && !o.wire().append_only() => {
                    summarized = true;
                    self.summarize(out, history, &system, usage);
                    continue;
                }
                Err(e) => {
                    out.send(Event::Error { text: e.to_string(), retryable: e.retryable() });
                    return (FinishReason::Failed, tps);
                }
            };
            usage.requests += 1;
            usage.prompt_tokens += reply.prompt_tokens;
            usage.cached_tokens += reply.cached_tokens;
            usage.completion_tokens += reply.completion_tokens;
            usage.max_prompt_tokens = usage.max_prompt_tokens.max(reply.prompt_tokens);
            usage.llm_seconds += reply.elapsed.as_secs_f32();
            tps = reply.decode_tps.or(tps);

            // Calls: the API's, else ones written in the text (a local model's
            // habit; a cloud model's reply goes back as it came).
            let mut calls = reply.calls.clone();
            let mut content = reply.content.clone();
            if calls.is_empty() && !o.wire().is_cloud() {
                let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let found = textcalls::find(&reply.content, &name_refs);
                if !found.is_empty() {
                    content = strip_text_calls(&reply.content);
                    calls = found
                        .into_iter()
                        .map(|c| WireCall {
                            id: CallId::new(format!("call_{}", random_hex(4))),
                            name: c.name,
                            arguments: c.arguments,
                        })
                        .collect();
                }
            }
            for c in &mut calls {
                if c.id.as_str().is_empty() {
                    c.id = CallId::new(format!("call_{}", random_hex(4)));
                }
            }
            let mut assistant = Message::assistant(content.clone());
            assistant.reasoning = (!reply.reasoning.is_empty()).then(|| reply.reasoning.clone());
            assistant.tool_calls = calls.clone();
            assistant.raw = reply.raw.clone().map(|items| RawTurn { wire: o.wire(), model: o.model.clone(), items });
            out.message(history, assistant);

            if reply.finish_reason.as_deref() == Some("refusal") {
                out.send(Event::Error {
                    text: "The model declined this request. Try rephrasing it, or another model.".into(),
                    retryable: false,
                });
                return (FinishReason::Failed, tps);
            }
            if calls.is_empty() {
                if reply.finish_reason.as_deref() == Some("length") {
                    out.send(Event::Notice {
                        level: Level::Warning,
                        text: "The answer was cut off: it reached the model's length limit.".into(),
                    });
                    return (FinishReason::Length, tps);
                }
                if content.trim().is_empty() {
                    out.send(Event::Error { text: "The model gave an empty answer.".into(), retryable: true });
                    return (FinishReason::Failed, tps);
                }
                return (FinishReason::Answered, tps);
            }

            let mut images = Vec::new();
            for c in calls {
                if self.ctl.cancelled() {
                    // Every call needs a result, or the next request is malformed.
                    out.message(history, Message::tool(c.id.clone(), "Not run: the person stopped the request."));
                    continue;
                }
                usage.tool_calls += 1;
                let call = tools::parse(&c.name, &c.arguments, &names, &self.desktop);
                if matches!(call, ToolCall::Invalid { .. }) {
                    malformed += 1;
                }
                let needs = tools::approval(&call, o, desktop_asked);
                out.send(Event::ToolCall { id: c.id.clone(), call: call.clone(), needs: needs.clone() });
                let decided = match &needs {
                    Approval::Refused { reason } => Some((
                        ToolOutput::with_status(ToolStatus::Refused, reason.clone()),
                        format!("Not run: {reason} Tell the person what you wanted to do, or find another way."),
                    )),
                    a if a.asks() => match self.ctl.wait_decision(&c.id) {
                        None => Some((
                            ToolOutput::with_status(ToolStatus::Cancelled, "stopped"),
                            "Not run: the person stopped the request.".into(),
                        )),
                        Some(Decision::Deny { note }) => {
                            let mut text = "The person chose not to run this.".to_owned();
                            if let Some(n) = note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                                text.push_str(&format!(" They said: {n}"));
                            }
                            Some((ToolOutput::with_status(ToolStatus::Denied, text.clone()), text))
                        }
                        Some(Decision::Allow) => {
                            if call.is_desktop_action() {
                                desktop_asked = true;
                            }
                            None
                        }
                    },
                    _ => None,
                };
                let (output, for_model, image) = match decided {
                    Some((output, for_model)) => (output, for_model, None),
                    None => {
                        out.send(Event::ToolStarted { id: c.id.clone() });
                        let r = tools::execute(&call, o, &mut self.desktop, self.ctl.cancel_flag());
                        if r.memory_changed {
                            out.send(Event::MemoryChanged);
                        }
                        (r.output, r.for_model, r.image)
                    }
                };
                out.send(Event::ToolOutput { id: c.id.clone(), output });
                out.message(history, Message::tool(c.id.clone(), for_model));
                if let Some(img) = image {
                    images.push(img);
                }
            }
            if malformed > MALFORMED_LIMIT * 2 {
                out.send(Event::Error {
                    text: "The model kept making tool calls that don't work.".into(),
                    retryable: false,
                });
                return (FinishReason::Failed, tps);
            }
            // Screenshots go after the results: chat templates take images
            // in user messages.
            if let Some(img) = images.pop() {
                let parts = vec![Part::Text { text: context::SCREEN_NOW.into() }, Part::Image { path: img }];
                out.message(history, Message::new(Role::User, Content::Parts(parts)));
            }
        }
    }

    fn screen_size(&self) -> (u32, u32) {
        self.desktop.frame.map_or((1440, 900), |f| (f.width, f.height))
    }

    /// Replace the older part of the conversation with the model's summary of it.
    fn summarize(&mut self, out: &mut Out<'_>, history: &mut Vec<Message>, system: &str, usage: &mut Usage) {
        let Some(cut) = context::summary_cut(history, 6) else { return };
        let mut messages = vec![Message::system(system.to_owned())];
        messages.extend(history[..cut].iter().cloned());
        messages.push(Message::user(context::SUMMARY_REQUEST));
        let mut params = self.options.request_params.clone();
        // Thinking only slows a summary down.
        params.insert("chat_template_kwargs".into(), serde_json::json!({"enable_thinking": false}));
        let body = self.client.body(&messages, &[], Tokens(800), &params, false);
        let reply = self.client.chat(&body, self.ctl.cancel_flag(), &mut |_| {}, &mut |_| {});
        // The model's summary, or one built here when it fails (the context is
        // full, the server is struggling): a turn must not die of a long conversation.
        let summary = match reply {
            Ok(reply) => {
                usage.requests += 1;
                usage.prompt_tokens += reply.prompt_tokens;
                usage.completion_tokens += reply.completion_tokens;
                Some(reply.content).filter(|c| !c.trim().is_empty())
            }
            Err(LlmError::Cancelled) => return,
            Err(_) => None,
        };
        let summary = summary.unwrap_or_else(|| context::fallback_summary(&history[..cut]));
        let keep = history.len() - cut;
        let text = context::summary_message(&summary);
        out.flush();
        out.record(&Record::Summary { text: text.clone(), keep });
        let tail = history.split_off(cut);
        *history = vec![Message::user(text)];
        history.extend(tail);
        out.send(Event::Notice {
            level: Level::Info,
            text: "Summarized the earlier part of this conversation to make room.".into(),
        });
    }
}

impl Out<'_> {
    /// Record a streamed delta that was already sent.
    fn coalesce(&mut self, event: Event) {
        match (&mut self.pending, &event) {
            (Some(Event::TextDelta { text: a }), Event::TextDelta { text: b }) => a.push_str(b),
            (Some(Event::ReasoningDelta { text: a }), Event::ReasoningDelta { text: b }) => a.push_str(b),
            _ => {
                self.flush();
                self.pending = Some(event);
            }
        }
    }
}

/// The message without the tool calls written into it.
fn strip_text_calls(content: &str) -> String {
    let mut s = content.to_owned();
    while let (Some(a), Some(b)) = (s.find("<tool_call>"), s.find("</tool_call>")) {
        if b < a {
            break;
        }
        s.replace_range(a..b + "</tool_call>".len(), "");
    }
    if let Some(a) = s.find("```") {
        s.truncate(a);
    }
    let t = s.trim();
    if t.starts_with('{') && t.ends_with('}') { String::new() } else { t.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::TurnHandle;
    use crate::options::DataDir;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    /// A server that answers each chat request with the next scripted SSE
    /// body (404 to anything else) and returns the request bodies it saw.
    fn serve(replies: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            let total = replies.len();
            let mut replies = replies.into_iter();
            for stream in l.incoming() {
                let mut s = stream.unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                r.read_line(&mut first).unwrap();
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                if first.starts_with("GET") {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                    continue;
                }
                seen.push(String::from_utf8(body).unwrap());
                let Some(reply) = replies.next() else {
                    let _ =
                        s.write_all(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 2\r\nconnection: close\r\n\r\nno");
                    break;
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(reply.as_bytes());
                if seen.len() == total {
                    break;
                }
            }
            seen
        });
        (url, h)
    }

    fn sse_text(text: &str) -> String {
        let chunk = serde_json::json!({"choices": [{"delta": {"content": text}, "finish_reason": null}]});
        let end = serde_json::json!({"choices": [{"delta": {}, "finish_reason": "stop"}],
                                      "usage": {"prompt_tokens": 100, "completion_tokens": 5}});
        format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n")
    }

    fn sse_call(name: &str, args: serde_json::Value) -> String {
        let chunk = serde_json::json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": format!("c{}", random_hex(2)),
            "type": "function", "function": {"name": name, "arguments": args.to_string()}}]}, "finish_reason": null}]});
        let end = serde_json::json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]});
        format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n")
    }

    fn options(url: &str) -> (Options, DataDir) {
        let dir = DataDir::new(std::env::temp_dir().join(format!("tasuke-turn-{}", random_hex(4))));
        let mut o = Options::new(url, "fake");
        o.data_dir = dir.clone();
        o.skills = crate::skills::Skills::default();
        o.request_timeout = Duration::from_secs(20);
        (o, dir)
    }

    fn drain(turn: &TurnHandle, mut decide: impl FnMut(&Event) -> Option<Decision>) -> Vec<Event> {
        let mut events = Vec::new();
        for e in turn.events() {
            if let Event::ToolCall { id, needs, .. } = &e
                && needs.asks()
                && let Some(d) = decide(&e)
            {
                turn.decide(id, d);
            }
            events.push(e);
        }
        events
    }

    /// A Claude reply as Anthropic streams it: thinking, then a call or text.
    fn sse_claude(call: Option<(&str, serde_json::Value)>, text: &str) -> String {
        let ev = |v: serde_json::Value| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap_or(""));
        let mut s = ev(
            serde_json::json!({"type": "message_start", "message": {"usage": {"input_tokens": 50, "output_tokens": 1}}}),
        );
        s += &ev(
            serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
        );
        s += &ev(
            serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Look first."}}),
        );
        s += &ev(
            serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "c2lnbmVk"}}),
        );
        s += &ev(serde_json::json!({"type": "content_block_stop", "index": 0}));
        let stop = match call {
            Some((name, args)) => {
                s += &ev(
                    serde_json::json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "toolu_01", "name": name, "input": {}}}),
                );
                s += &ev(
                    serde_json::json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": args.to_string()}}),
                );
                s += &ev(serde_json::json!({"type": "content_block_stop", "index": 1}));
                "tool_use"
            }
            None => {
                s += &ev(
                    serde_json::json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
                );
                s += &ev(
                    serde_json::json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": text}}),
                );
                s += &ev(serde_json::json!({"type": "content_block_stop", "index": 1}));
                "end_turn"
            }
        };
        s += &ev(
            serde_json::json!({"type": "message_delta", "delta": {"stop_reason": stop}, "usage": {"output_tokens": 30}}),
        );
        s += &ev(serde_json::json!({"type": "message_stop"}));
        s
    }

    #[test]
    fn a_claude_turn_replays_its_signed_thinking_unchanged() {
        let (url, server) = serve(vec![
            sse_claude(Some(("run_command", serde_json::json!({"command": "echo looked", "purpose": "look"}))), ""),
            sse_claude(None, "All done."),
        ]);
        let (mut o, dir) = options(&url);
        o.provider = Some(tasuke_models::CloudProvider::Anthropic);
        o.model = "claude-sonnet-5-5".into();
        o.api_key = Some("sk-ant-test".into());
        let session = Session::create(&dir).unwrap();
        let turn = crate::Agent::start(&session, "check things", o);
        let events = drain(&turn, |_| Some(Decision::Allow));
        assert!(
            events.iter().any(|e| matches!(e, Event::TurnFinished { reason: FinishReason::Answered, .. })),
            "{events:?}"
        );
        let seen = server.join().unwrap();
        let first: serde_json::Value = serde_json::from_str(&seen[0]).unwrap();
        assert!(first["system"].as_str().unwrap().contains("in Anthropic's cloud"), "the prompt says where it runs");
        assert!(!first["system"].as_str().unwrap().contains("nothing you see leaves it"));
        let second: serde_json::Value = serde_json::from_str(&seen[1]).unwrap();
        let m = second["messages"].as_array().unwrap();
        assert_eq!(m[1]["role"], "assistant");
        assert_eq!(
            m[1]["content"][0],
            serde_json::json!({"type": "thinking", "thinking": "Look first.", "signature": "c2lnbmVk"})
        );
        assert_eq!(m[1]["content"][1]["input"]["command"], "echo looked");
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(m[2]["content"][0]["tool_use_id"], "toolu_01");
        assert!(m[2]["content"][0]["content"].as_str().unwrap().contains("looked"));
        // The first request's turns are the second's prefix, byte for byte.
        let a = serde_json::to_string(&first["messages"][0]).unwrap();
        let b = serde_json::to_string(&m[0]).unwrap();
        assert_eq!(a, b, "append-only: the opening message unchanged");
        std::fs::remove_dir_all(dir.path()).ok();
    }

    /// A GPT reply as the Responses API streams it: reasoning, then a call or text.
    fn sse_gpt(call: Option<(&str, serde_json::Value)>, text: &str) -> String {
        let ev = |v: serde_json::Value| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap_or(""));
        let reasoning =
            serde_json::json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAA"});
        let mut s = ev(
            serde_json::json!({"type": "response.output_item.added", "output_index": 0, "item": {"type": "reasoning", "id": "rs_1"}}),
        );
        s += &ev(serde_json::json!({"type": "response.output_item.done", "output_index": 0, "item": reasoning}));
        match call {
            Some((name, args)) => {
                let item = serde_json::json!({"type": "function_call", "id": "fc_1", "call_id": "call_9", "name": name, "arguments": args.to_string(), "status": "completed"});
                s += &ev(
                    serde_json::json!({"type": "response.output_item.added", "output_index": 1, "item": {"type": "function_call", "id": "fc_1", "call_id": "call_9", "name": name, "arguments": ""}}),
                );
                s += &ev(
                    serde_json::json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": args.to_string()}),
                );
                s += &ev(serde_json::json!({"type": "response.output_item.done", "output_index": 1, "item": item}));
            }
            None => {
                s += &ev(
                    serde_json::json!({"type": "response.output_item.added", "output_index": 1, "item": {"type": "message", "id": "msg_1"}}),
                );
                s += &ev(serde_json::json!({"type": "response.output_text.delta", "output_index": 1, "delta": text}));
                s += &ev(
                    serde_json::json!({"type": "response.output_item.done", "output_index": 1, "item": {"type": "message", "id": "msg_1", "role": "assistant", "content": [{"type": "output_text", "text": text}]}}),
                );
            }
        }
        s += &ev(
            serde_json::json!({"type": "response.completed", "response": {"usage": {"input_tokens": 80, "output_tokens": 20}}}),
        );
        s
    }

    #[test]
    fn a_gpt_turn_sends_back_its_reasoning_and_the_call_output() {
        let (url, server) = serve(vec![
            sse_gpt(Some(("run_command", serde_json::json!({"command": "echo looked", "purpose": "look"}))), ""),
            sse_gpt(None, "All done."),
        ]);
        let (mut o, dir) = options(&url);
        o.provider = Some(tasuke_models::CloudProvider::OpenAi);
        o.model = "gpt-6-luna".into();
        o.api_key = Some("sk-test".into());
        let session = Session::create(&dir).unwrap();
        let turn = crate::Agent::start(&session, "check things", o);
        let events = drain(&turn, |_| Some(Decision::Allow));
        assert!(
            events.iter().any(|e| matches!(e, Event::TurnFinished { reason: FinishReason::Answered, .. })),
            "{events:?}"
        );
        let seen = server.join().unwrap();
        let first: serde_json::Value = serde_json::from_str(&seen[0]).unwrap();
        assert_eq!(first["store"], false);
        assert!(first["instructions"].as_str().unwrap().contains("in OpenAI's cloud"));
        let second: serde_json::Value = serde_json::from_str(&seen[1]).unwrap();
        let input = second["input"].as_array().unwrap();
        assert_eq!(input[1], serde_json::json!({"type": "reasoning", "summary": [], "encrypted_content": "gAAA"}));
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["call_id"], "call_9");
        assert!(input[2].get("id").is_none() && input[2].get("status").is_none(), "{}", input[2]);
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_9");
        assert!(input[3]["output"].as_str().unwrap().contains("looked"));
        std::fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn a_turn_runs_tools_asks_and_answers() {
        let (url, server) = serve(vec![
            sse_call("run_command", serde_json::json!({"command": "echo looked", "purpose": "look"})),
            sse_call("run_command", serde_json::json!({"command": "touch /nonexistent-dir/x", "purpose": "change"})),
            sse_text("All done."),
        ]);
        let (o, dir) = options(&url);
        let session = Session::create(&dir).unwrap();
        let turn = crate::Agent::start(&session, "check things", o);
        let events = drain(&turn, |_| Some(Decision::Deny { note: Some("not now".into()) }));
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match e {
                Event::TurnStarted { .. } => "start",
                Event::ToolCall { needs: Approval::None, .. } => "call",
                Event::ToolCall { .. } => "ask",
                Event::ToolStarted { .. } => "run",
                Event::ToolOutput { .. } => "out",
                Event::TextDelta { .. } => "text",
                Event::TurnFinished { reason: FinishReason::Answered, .. } => "end",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["start", "call", "run", "out", "ask", "out", "text", "end"], "{events:?}");
        let Event::ToolOutput { output, .. } = &events[3] else { panic!() };
        assert!(output.text.contains("looked"));
        let Event::ToolOutput { output, .. } = &events[5] else { panic!() };
        assert_eq!(output.status, ToolStatus::Denied);
        let seen = server.join().unwrap();
        assert!(seen[0].contains("<environment>"));
        assert!(seen[2].contains("looked") && seen[2].contains("They said: not now"));
        // The session can be read back and continued.
        let again = Session::load(&dir, session.id()).unwrap();
        assert_eq!(again.title(), "check things");
        let (history, last) = again.history().unwrap();
        assert_eq!(last, TurnNumber(1));
        assert_eq!(history.len(), 6);
        let transcript = again.transcript().unwrap();
        assert!(transcript.len() < 12, "deltas are coalesced: {}", transcript.len());
        std::fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn text_calls_run_and_cancel_stops_at_a_question() {
        let (url, _server) = serve(vec![
            sse_text(
                "<tool_call>{\"name\": \"run_command\", \"arguments\": {\"command\": \"echo hi\", \"purpose\": \"x\"}}</tool_call>",
            ),
            sse_call("run_as_root", serde_json::json!({"command": "systemctl restart x", "purpose": "fix"})),
        ]);
        let (o, dir) = options(&url);
        let session = Session::create(&dir).unwrap();
        let turn = crate::Agent::start(&session, "hi", o);
        let mut events = Vec::new();
        for e in turn.events() {
            if let Event::ToolCall { needs, .. } = &e
                && needs.asks()
            {
                turn.cancel();
            }
            events.push(e);
        }
        assert!(events.iter().any(|e| matches!(e, Event::ToolOutput { output, .. } if output.text.contains("hi"))));
        assert!(
            matches!(events.last(), Some(Event::TurnFinished { reason: FinishReason::Cancelled, .. })),
            "{events:?}"
        );
        std::fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn the_step_budget_ends_a_turn() {
        let call = || sse_call("run_command", serde_json::json!({"command": "true", "purpose": "x"}));
        let (url, _server) = serve(vec![call(), call(), call(), call()]);
        let (mut o, dir) = options(&url);
        o.step_budget = crate::StepBudget(2);
        let session = Session::create(&dir).unwrap();
        let events = drain(&crate::Agent::start(&session, "loop", o), |_| None);
        assert!(
            matches!(events.last(), Some(Event::TurnFinished { reason: FinishReason::StepBudget, .. })),
            "{events:?}"
        );
        std::fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn text_calls_are_stripped_from_the_answer() {
        assert_eq!(strip_text_calls("Let me look.\n<tool_call>{\"name\":\"x\"}</tool_call>"), "Let me look.");
        assert_eq!(strip_text_calls("{\"name\": \"x\"}"), "");
        assert_eq!(strip_text_calls("Checking.\n```json\n{}\n```"), "Checking.");
    }
}
