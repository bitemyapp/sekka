//! The account commands as a person runs them: connect Anthropic with a
//! pasted key, choose a Claude model, time it, disconnect. A stand-in
//! Messages API answers, and a stand-in secret-tool keeps the key.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const KEY: &str = "sk-ant-api03-fixture-key-7a3f";

/// Read one request: its first line and headers (the body is skipped).
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

/// A stand-in Anthropic API: `/models` for the key check, `/messages` for timing.
fn fake_anthropic() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { continue };
            let (first, headers) = request(&mut s);
            if !headers.contains(&format!("x-api-key: {KEY}")) {
                let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
                let _ = write!(s, "HTTP/1.1 401 Unauthorized\r\ncontent-length: {}\r\n\r\n{body}", body.len());
                continue;
            }
            if first.starts_with("GET /v1/models") {
                let body = r#"{"data":[{"type":"model","id":"claude-haiku-5-5","display_name":"Claude Haiku 5.5","max_input_tokens":1000000},{"type":"model","id":"claude-sonnet-5-5","display_name":"Claude Sonnet 5.5"}],"has_more":false}"#;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                    body.len()
                );
                continue;
            }
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n");
            let mut send = |v: &str| {
                let _ = s.write_all(format!("event: x\ndata: {v}\n\n").as_bytes());
                let _ = s.flush();
            };
            send(r#"{"type":"message_start","message":{"usage":{"input_tokens":30,"output_tokens":1}}}"#);
            send(r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#);
            for w in ["Gears ", "trade ", "speed ", "for ", "force."] {
                std::thread::sleep(Duration::from_millis(30));
                send(&format!(
                    r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{w}"}}}}"#
                ));
            }
            send(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":41}}"#);
            send(r#"{"type":"message_stop"}"#);
        }
    });
    format!("http://{addr}/v1")
}

/// A stand-in secret-tool keeping items as files.
fn fake_keyring(dir: &Path) -> PathBuf {
    let path = dir.join("secret-tool");
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
    std::fs::write(&path, script).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

struct Home {
    dir: PathBuf,
    api: String,
}

impl Home {
    fn tasuke(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tasuke"))
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.dir)
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("XDG_DATA_HOME", self.dir.join("data"))
            .env("XDG_RUNTIME_DIR", self.dir.join("run"))
            .env("TASUKE_SECRET_TOOL", self.dir.join("secret-tool"))
            .env("TASUKE_ANTHROPIC_BASE_URL", &self.api)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("tasuke runs");
        child.stdin.take().expect("stdin").write_all(stdin.as_bytes()).expect("write");
        child.wait_with_output().expect("tasuke finishes")
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn connect_choose_time_and_disconnect_a_claude_account() {
    let dir = std::env::temp_dir().join(format!("tasuke-accounts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("run")).expect("dir");
    fake_keyring(&dir);
    let home = Home { dir: dir.clone(), api: fake_anthropic() };

    let o = home.tasuke(&["account"], "");
    assert!(o.status.success() && text(&o).contains("Anthropic: not connected"), "{}", text(&o));

    let o = home.tasuke(&["account", "add", "anthropic"], "sk-ant-api03-wrong\n");
    assert!(!o.status.success(), "a wrong key isn't kept");
    assert!(text(&o).contains("didn't accept that key"), "{}", text(&o));

    let o = home.tasuke(&["account", "add", "claude"], &format!("{KEY}\n"));
    assert!(o.status.success(), "{}", text(&o));
    let said = text(&o);
    assert!(said.contains("Connected Anthropic (key sk-ant-…7a3f)"), "{said}");
    assert!(said.contains("Claude Haiku 5.5, Claude Sonnet 5.5"), "{said}");
    assert!(said.contains("is sent to Anthropic"), "{said}");
    assert!(!said.contains("fixture-key"), "the key never prints whole");

    let o = home.tasuke(&["account", "list", "--json"], "");
    let list: serde_json::Value = serde_json::from_slice(&o.stdout).expect("json");
    assert_eq!(list[0]["connected"], true);
    assert_eq!(list[0]["from"], "the keyring");
    assert_eq!(list[1]["connected"], false, "OpenAI is apart");

    let o = home.tasuke(&["use", "anthropic/claude-haiku-5-5"], "");
    assert!(o.status.success(), "{}", text(&o));
    let config = std::fs::read_to_string(dir.join("config/tasuke/config.toml")).expect("config");
    assert!(config.contains(r#"id = "claude-haiku-5-5""#) && config.contains(r#"provider = "anthropic""#), "{config}");

    // No model named: Tasuke's own, now the Claude model.
    let o = home.tasuke(&["bench", "--json"], "");
    assert!(o.status.success(), "{}", text(&o));
    let m: serde_json::Value = serde_json::from_slice(&o.stdout).expect("json");
    assert_eq!(m["model"], "anthropic/claude-haiku-5-5");
    assert_eq!(m["measured"]["tokens"], 41);
    assert_eq!(m["measured"]["on"], "Anthropic");
    let speeds = std::fs::read_to_string(dir.join("data/tasuke/speeds.toml")).expect("speeds");
    assert!(speeds.contains(r#"["anthropic/claude-haiku-5-5"]"#), "{speeds}");

    let o = home.tasuke(&["account", "remove", "anthropic"], "");
    assert!(o.status.success(), "{}", text(&o));
    let o = home.tasuke(&["bench", "anthropic/claude-haiku-5-5"], "");
    assert!(!o.status.success());
    assert!(text(&o).contains("no Anthropic account is connected"), "{}", text(&o));
    let _ = std::fs::remove_dir_all(&dir);
}
