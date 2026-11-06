//! API keys in the Secret Service, the desktop's keyring, through
//! libsecret's `secret-tool`. Tasuke keeps no secret in its own files: one
//! service holds them, unlocked with the session, and Settings and other
//! tools see the same items.
//!
//! The tool is `$TASUKE_SECRET_TOOL`, the path the package was built with
//! (Sekka's libsecret, with the Secret Service key fix), or `secret-tool` on
//! the PATH.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::cloud::{ApiKey, CloudProvider};

fn tool() -> PathBuf {
    if let Some(p) = std::env::var_os("TASUKE_SECRET_TOOL").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(p) = option_env!("TASUKE_SECRET_TOOL_PATH") {
        return PathBuf::from(p);
    }
    PathBuf::from("secret-tool")
}

/// The item's attributes: what finds it again.
fn attributes(provider: CloudProvider) -> [&'static str; 4] {
    ["application", "tasuke", "provider", provider.as_str()]
}

/// Keep `key` for `provider`, replacing any earlier one.
pub fn store(provider: CloudProvider, key: &ApiKey) -> io::Result<()> {
    let label = format!("Tasuke: {} API key", provider.company());
    let mut child = Command::new(tool())
        .arg("store")
        .arg(format!("--label={label}"))
        .args(attributes(provider))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    // secret-tool reads the secret from stdin; no newline, or it becomes part of it.
    child.stdin.take().expect("piped").write_all(key.expose().as_bytes())?;
    let out = child.wait_with_output()?;
    if out.status.success() { Ok(()) } else { Err(io::Error::other(failure("store", &out.stderr))) }
}

/// The key kept for `provider`, if any (the keyring may be locked or absent).
pub fn lookup(provider: CloudProvider) -> Option<ApiKey> {
    let out = Command::new(tool())
        .arg("lookup")
        .args(attributes(provider))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    ApiKey::new(&String::from_utf8_lossy(&out.stdout))
}

/// The key for `provider`: the keyring's, else the provider's environment
/// variable (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`), for the command line.
pub fn key_for(provider: CloudProvider) -> Option<ApiKey> {
    lookup(provider).or_else(|| std::env::var(provider.env_var()).ok().and_then(|k| ApiKey::new(&k)))
}

/// Forget the key kept for `provider`. Forgetting none is not an error.
pub fn clear(provider: CloudProvider) -> io::Result<()> {
    let out = Command::new(tool()).arg("clear").args(attributes(provider)).stdin(Stdio::null()).output()?;
    // secret-tool clear exits 1 when nothing matched.
    if out.status.success() || out.status.code() == Some(1) && out.stderr.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(failure("clear", &out.stderr)))
    }
}

fn failure(what: &str, stderr: &[u8]) -> String {
    let e = String::from_utf8_lossy(stderr);
    let e = e.trim();
    if e.is_empty() {
        format!("the keyring couldn't {what} the key")
    } else {
        format!("the keyring couldn't {what} the key: {e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    /// Tests here set process-wide variables.
    static ENV: Mutex<()> = Mutex::new(());

    /// A stand-in secret-tool keeping items as files named by their
    /// attributes, and logging its arguments.
    fn fake(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("secret-tool");
        let script = format!(
            r#"#!/bin/sh
echo "$@" >> {dir}/args
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

    #[test]
    fn keys_round_trip_through_the_keyring_tool() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("tasuke-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        // SAFETY: tests that set the environment hold ENV.
        unsafe { std::env::set_var("TASUKE_SECRET_TOOL", fake(&dir)) };
        let key = ApiKey::new("sk-ant-api03-secret-1234").expect("a key");
        assert_eq!(lookup(CloudProvider::Anthropic), None);
        store(CloudProvider::Anthropic, &key).expect("stored");
        assert_eq!(lookup(CloudProvider::Anthropic), Some(key));
        assert_eq!(lookup(CloudProvider::OpenAi), None, "kept apart by provider");
        clear(CloudProvider::Anthropic).expect("cleared");
        assert_eq!(lookup(CloudProvider::Anthropic), None);
        clear(CloudProvider::Anthropic).expect("clearing nothing is fine");
        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        assert!(args.contains("--label=Tasuke: Anthropic API key application tasuke provider anthropic"), "{args}");
        assert!(!args.contains("secret-1234"), "the key goes on stdin, never the command line");
        // SAFETY: as above.
        unsafe { std::env::remove_var("TASUKE_SECRET_TOOL") };
        let _ = std::fs::remove_dir_all(&dir);
    }
}
