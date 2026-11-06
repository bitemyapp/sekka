//! A model's speed, measured here: one fixed request, timed from sending to
//! the first token and from there to the last. A local model is loaded
//! first, so loading isn't counted; a cloud model is timed as this computer
//! sees it, network included. The same request for every model, so the
//! numbers compare.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::json;

use crate::chat::Message;
use crate::ids::Tokens;
use crate::llm::{Client, LlmError};
use crate::options::Options;

/// The request: a few hundred words of plain prose.
pub const PROMPT: &str = "Explain in about 200 words how a bicycle's gears make climbing a hill easier. \
                          Write plain prose, no lists or headings.";

/// What the request measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Speed {
    /// Output tokens a second once they flow.
    pub decode: f64,
    /// From sending to the first token (thinking included).
    pub first_token: Duration,
    /// Prompt tokens a second, when the server says (llama.cpp).
    pub prefill: Option<f64>,
    /// Tokens written, thinking included.
    pub tokens: u64,
}

impl Speed {
    /// The measurement as `speeds.toml` keeps it, dated today. `on` is where
    /// it ran: a GPU's name, or the company.
    pub fn measured(&self, on: &str) -> tasuke_models::Measured {
        tasuke_models::Measured {
            decode: self.decode,
            first_token_ms: u64::try_from(self.first_token.as_millis()).unwrap_or(u64::MAX),
            prefill: self.prefill,
            tokens: self.tokens,
            date: tasuke_models::speeds::today(),
            on: on.to_owned(),
        }
    }
}

/// Measure the model `options` names. `step` hears what's happening. A cloud
/// model needs its key resolved first ([`Options::resolve_key`]).
pub fn measure(options: &Options, cancel: &AtomicBool, step: &mut dyn FnMut(&str)) -> Result<Speed, String> {
    if let Some(p) = options.provider
        && options.api_key.is_none()
    {
        return Err(format!("no {} account is connected", p.company()));
    }
    let client = Client::new(
        &options.base_url,
        &options.model,
        options.api_key.clone(),
        options.request_timeout,
        options.wire(),
    );
    let mut params = options.request_params.clone();
    if options.provider.is_some() {
        // Least thinking: the rate of writing is what's measured.
        params.insert("effort".into(), json!("low"));
    } else {
        params.insert("chat_template_kwargs".into(), json!({"enable_thinking": false}));
        start_server(options, &client, cancel, step);
        step("Loading the model…");
        let warm = client.body(&[Message::user("Say OK.")], &[], Tokens(4), &params, false);
        client.chat(&warm, cancel, &mut |_| {}, &mut |_| {}).map_err(|e| failed(&e))?;
    }
    step("Measuring…");
    let body = client.body(&[Message::user(PROMPT)], &[], Tokens(400), &params, false);
    let reply = client.chat(&body, cancel, &mut |_| {}, &mut |_| {}).map_err(|e| failed(&e))?;
    let decode = reply.decode_tps.map(f64::from).ok_or("the answer was too short to time")?;
    Ok(Speed {
        decode,
        first_token: reply.first_token.unwrap_or(reply.elapsed),
        prefill: reply.prefill_tps.map(f64::from),
        tokens: reply.completion_tokens.0,
    })
}

fn failed(e: &LlmError) -> String {
    e.to_string()
}

/// Tasuke's own model server, started when it isn't answering.
fn start_server(options: &Options, client: &Client, cancel: &AtomicBool, step: &mut dyn FnMut(&str)) {
    let own = format!("http://127.0.0.1:{}/v1", crate::options::DEFAULT_PORT);
    if options.base_url.trim_end_matches('/') != own || client.reachable() {
        return;
    }
    step("Starting the model server…");
    let argv: Vec<String> =
        ["systemctl", "--user", "start", "tasuke-llm.service"].iter().map(|s| (*s).to_owned()).collect();
    let _ = crate::exec::run(&argv, None, &[], None, Duration::from_secs(20), cancel);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use tasuke_models::CloudProvider;

    /// A stand-in Messages API streaming a short answer, slowly enough to time.
    fn fake_anthropic() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 65536];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n");
            let send = |s: &mut std::net::TcpStream, v: serde_json::Value| {
                let _ = s.write_all(format!("event: x\ndata: {v}\n\n").as_bytes());
                let _ = s.flush();
            };
            send(
                &mut s,
                json!({"type": "message_start", "message": {"usage": {"input_tokens": 30, "output_tokens": 1}}}),
            );
            send(
                &mut s,
                json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            );
            for w in ["Gears ", "trade ", "speed ", "for ", "force."] {
                std::thread::sleep(Duration::from_millis(40));
                send(
                    &mut s,
                    json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": w}}),
                );
            }
            send(
                &mut s,
                json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 21}}),
            );
            send(&mut s, json!({"type": "message_stop"}));
        });
        format!("http://{addr}/v1")
    }

    #[test]
    fn a_cloud_model_is_timed_from_its_stream() {
        let mut o = Options::new(fake_anthropic(), "claude-haiku-5-5");
        o.provider = Some(CloudProvider::Anthropic);
        o.api_key = Some("sk-ant-test".into());
        let mut steps = Vec::new();
        let s = measure(&o, &AtomicBool::new(false), &mut |t| steps.push(t.to_owned())).expect("measured");
        assert_eq!(steps, ["Measuring…"], "no loading for a cloud model");
        assert_eq!(s.tokens, 21);
        // 20 tokens over about 160 ms of streaming.
        assert!(s.decode > 40.0 && s.decode < 400.0, "{}", s.decode);
        assert!(s.first_token < Duration::from_secs(2));
        assert_eq!(s.prefill, None);
    }

    #[test]
    fn no_account_no_measurement() {
        let mut o = Options::new("http://127.0.0.1:9/v1", "gpt-6-luna");
        o.provider = Some(CloudProvider::OpenAi);
        o.api_key = None;
        let e = measure(&o, &AtomicBool::new(false), &mut |_| {}).expect_err("no key");
        assert!(e.contains("OpenAI"), "{e}");
    }
}
