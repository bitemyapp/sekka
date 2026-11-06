//! The window with a Claude account, on the real agent: Use on a Claude
//! model opens the sheet, a wrong key is turned away, the right one is
//! checked and kept, the model is timed, a question is answered through
//! it, and Disconnect forgets the key.
//!
//! Nothing leaves the computer: a stand-in Messages API answers on
//! localhost, and a stand-in secret-tool keeps the key as a file, both
//! under a scratch directory with the settings.

#![cfg(feature = "live")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aizome_kit::testing::Harness;
use aizome_kit::theme::Appearance;
use tasuke_models::{CloudModelRef, CloudProvider};
use tasuke_window::app::{Account, ConnectStep, ModelState, Msg, Page, Tasuke};
use tasuke_window::transcript::Body;

const KEY: &str = "sk-ant-api03-fixture-key-7a3f";

/// One request's first line and headers (its body read and dropped).
fn request(s: &mut TcpStream) -> (String, String) {
    let mut r = BufReader::new(s.try_clone().expect("clone"));
    let mut first = String::new();
    r.read_line(&mut first).expect("request line");
    let mut headers = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
        headers.push_str(&line);
    }
    let mut body = vec![0; length];
    let _ = r.read_exact(&mut body);
    (first, headers)
}

fn answer(mut s: TcpStream, seen: &Mutex<Vec<String>>) {
    let (first, headers) = request(&mut s);
    seen.lock().unwrap().push(first.trim().to_owned());
    if !headers.contains(&format!("x-api-key: {KEY}")) {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let _ = write!(s, "HTTP/1.1 401 Unauthorized\r\ncontent-length: {}\r\n\r\n{body}", body.len());
        return;
    }
    if first.starts_with("GET /v1/models") {
        let body = r#"{"data":[{"type":"model","id":"claude-sonnet-5-5","display_name":"Claude Sonnet 5.5"},{"type":"model","id":"claude-haiku-5-5","display_name":"Claude Haiku 5.5"}],"has_more":false}"#;
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        return;
    }
    let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n");
    let mut send = |v: String| {
        let _ = s.write_all(format!("event: x\ndata: {v}\n\n").as_bytes());
        let _ = s.flush();
    };
    send(r#"{"type":"message_start","message":{"usage":{"input_tokens":900,"output_tokens":1}}}"#.into());
    send(r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.into());
    for w in ["Your ", "sound ", "is ", "on."] {
        std::thread::sleep(Duration::from_millis(30));
        send(format!(r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{w}"}}}}"#));
    }
    send(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":24}}"#.into());
    send(r#"{"type":"message_stop"}"#.into());
}

/// A stand-in Anthropic API; what it was asked, by request line.
fn fake_anthropic() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let log = log.clone();
            std::thread::spawn(move || answer(s, &log));
        }
    });
    (format!("http://{addr}/v1"), seen)
}

/// A stand-in secret-tool keeping items as files.
fn fake_keyring(dir: &Path) {
    let script = format!(
        r#"#!/bin/sh
cmd=$1; shift
case $cmd in store) shift ;; esac
name=$(echo "$@" | tr ' ' '_')
case $cmd in
  store) cat > {dir}/item-$name ;;
  lookup) cat {dir}/item-$name 2>/dev/null || exit 1 ;;
  clear) [ -e {dir}/item-$name ] || exit 1; rm {dir}/item-$name ;;
esac
"#,
        dir = dir.display()
    );
    let path = dir.join("secret-tool");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_claude_account_is_connected_timed_used_and_disconnected() {
    let root = std::env::temp_dir().join(format!("tasuke-window-cloud-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (config, data) = (root.join("config"), root.join("data"));
    std::fs::create_dir_all(config.join("tasuke")).unwrap();
    fake_keyring(&root);
    let (api, seen) = fake_anthropic();
    // SAFETY: this test binary has this one test, so nothing else reads
    // the environment while it's set.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &config);
        std::env::set_var("XDG_DATA_HOME", &data);
        std::env::set_var("TASUKE_DATA_DIR", data.join("tasuke"));
        std::env::remove_var("XDG_RUNTIME_DIR");
        std::env::set_var("TASUKE_SECRET_TOOL", root.join("secret-tool"));
        std::env::set_var("TASUKE_ANTHROPIC_BASE_URL", &api);
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
    }
    std::fs::write(config.join("tasuke/config.toml"), "[server]\nport = 9\n").unwrap();

    let mut h = Harness::with(Tasuke::live(), Appearance::default(), false);
    let looked = h.settle(Duration::from_secs(10), |t| t.models.account(CloudProvider::Anthropic) != &Account::Unknown);
    assert!(looked, "the keyring was read after the window opened");
    assert_eq!(h.program().models.account(CloudProvider::Anthropic), &Account::Absent);

    // Use, with no account yet: the sheet, to use it once connected.
    let sonnet = CloudModelRef { provider: CloudProvider::Anthropic, id: "claude-sonnet-5-5".into() };
    h.send(Msg::UseCloud(sonnet.clone()));
    assert!(h.program().connect.as_ref().is_some_and(|c| c.then_use.as_ref() == Some(&sonnet)));

    h.send(Msg::ConnectKey("sk-ant-api03-not-this-one".into()));
    h.send(Msg::ConnectCheck);
    let turned_away = h.settle(Duration::from_secs(10), |t| {
        t.connect.as_ref().is_some_and(|c| matches!(&c.step, ConnectStep::Editing { error: Some(_) }))
    });
    assert!(turned_away);
    let ConnectStep::Editing { error: Some(e) } = &h.program().connect.as_ref().unwrap().step else { unreachable!() };
    assert!(e.contains("didn't accept that key"), "{e}");

    h.send(Msg::ConnectKey(format!("  {KEY}\n")));
    h.send(Msg::ConnectCheck);
    let connected = h.settle(Duration::from_secs(10), |t| {
        t.connect.as_ref().is_some_and(|c| matches!(c.step, ConnectStep::Connected { .. }))
    });
    assert!(connected);
    let t = h.program();
    let ConnectStep::Connected { models } = &t.connect.as_ref().unwrap().step else { unreachable!() };
    assert_eq!(models, &["Claude Sonnet 5.5", "Claude Haiku 5.5"]);
    assert_eq!(
        t.models.account(CloudProvider::Anthropic),
        &Account::Connected { hint: "sk-ant-…7a3f".into(), from_env: false }
    );
    assert_eq!(t.models.cloud.as_ref(), Some(&sonnet), "the model Use was pressed for");
    assert_eq!(t.chip.state, ModelState::Cloud(CloudProvider::Anthropic));
    let item = std::fs::read_to_string(root.join("item-application_tasuke_provider_anthropic")).unwrap();
    assert_eq!(item, KEY, "kept in the keyring, trimmed");
    let settings = std::fs::read_to_string(config.join("tasuke/config.toml")).unwrap();
    assert!(settings.contains("provider = \"anthropic\""), "{settings}");
    assert!(!settings.contains("fixture-key"), "the key is in no file of Tasuke's");
    h.send(Msg::ConnectClose);
    assert!(h.program().connect.is_none());

    // Timed through the account, kept in speeds.toml.
    h.send(Msg::Measure(sonnet.key()));
    let timed = h.settle(Duration::from_secs(20), |t| t.models.timing.is_none());
    assert!(timed);
    let m = h.program().models.speeds.get(&sonnet.key()).cloned();
    assert!(
        m.as_ref().is_some_and(|m| m.tokens == 24 && m.on == "Anthropic"),
        "{m:?} {:?}",
        h.program().models.timing_failed
    );
    let speeds = std::fs::read_to_string(data.join("tasuke/speeds.toml")).unwrap();
    assert!(speeds.contains("anthropic/claude-sonnet-5-5"), "{speeds}");

    // A question, answered through it.
    let Page::Conversation(id) = h.program().page else { panic!("a conversation") };
    h.send(Msg::Suggest(id, "Is my sound on?".into()));
    let ended = h.settle(Duration::from_secs(30), |t| t.conversation(id).is_some_and(|c| c.turn.is_none()));
    assert!(ended);
    let c = h.program().conversation(id).unwrap();
    assert!(
        c.transcript.items.iter().any(|i| matches!(&i.body, Body::Answer(a) if a.text.contains("Your sound is on."))),
        "{:?}",
        c.transcript.items
    );
    let asked: Vec<String> = seen.lock().unwrap().clone();
    assert!(asked.iter().filter(|l| l.starts_with("POST /v1/messages")).count() >= 2, "timed and asked: {asked:?}");

    // Disconnected: the key is gone, and the chip says so.
    h.send(Msg::Disconnect(CloudProvider::Anthropic));
    h.settle(Duration::from_secs(10), |_| false);
    assert_eq!(h.program().models.account(CloudProvider::Anthropic), &Account::Absent);
    assert_eq!(h.program().chip.state, ModelState::NoAccount(CloudProvider::Anthropic));
    assert!(!root.join("item-application_tasuke_provider_anthropic").exists());
    let _ = std::fs::remove_dir_all(&root);
}
