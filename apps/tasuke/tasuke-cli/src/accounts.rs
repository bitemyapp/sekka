//! `tasuke account`: Claude and OpenAI accounts, by API key. A key is checked
//! with its provider before it is kept, and kept only in the desktop's
//! keyring (the Secret Service), never in Tasuke's files.

use std::io::{BufRead, IsTerminal};

use serde_json::json;
use tasuke_models::{ApiKey, CheckError, CloudProvider, secrets};

use crate::args::Args;
use crate::models::{Result, failed, usage};

pub fn run(words: Vec<String>) -> Result {
    let mut args = Args::new(words);
    let verb = args.positional().unwrap_or_else(|| "list".into());
    match verb.as_str() {
        "list" => {
            let json = args.flag("--json");
            args.finish().map_err(usage)?;
            list(json)
        }
        "add" | "remove" => {
            let name = args.positional().ok_or_else(|| usage("which account? anthropic or openai"))?;
            let provider = CloudProvider::parse(&name)
                .ok_or_else(|| usage(format!("{name:?} isn't an account Tasuke knows (anthropic or openai)")))?;
            args.finish().map_err(usage)?;
            if verb == "add" { add(provider) } else { remove(provider) }
        }
        other => Err(usage(format!("unknown account command {other:?} (list, add or remove)"))),
    }
}

/// Where a provider's key comes from, if anywhere.
fn source(provider: CloudProvider) -> Option<(ApiKey, &'static str)> {
    if let Some(k) = secrets::lookup(provider) {
        return Some((k, "the keyring"));
    }
    let k = std::env::var(provider.env_var()).ok().and_then(|k| ApiKey::new(&k))?;
    Some((k, provider.env_var()))
}

fn list(json: bool) -> Result {
    if json {
        let all: Vec<_> = CloudProvider::ALL
            .iter()
            .map(|&p| {
                let s = source(p);
                json!({
                    "provider": p.as_str(),
                    "company": p.company(),
                    "connected": s.is_some(),
                    "key": s.as_ref().map(|(k, _)| k.hint()),
                    "from": s.as_ref().map(|(_, from)| *from),
                })
            })
            .collect();
        return crate::models::print_json(&all);
    }
    for p in CloudProvider::ALL {
        match source(p) {
            Some((k, from)) => println!("{}: connected, key {} (from {from})", p.company(), k.hint()),
            None => println!("{}: not connected (tasuke account add {})", p.company(), p.as_str()),
        }
    }
    Ok(())
}

fn add(provider: CloudProvider) -> Result {
    let company = provider.company();
    let prompt = format!("Paste your {company} API key (make one at {}): ", provider.keys_page());
    let pasted = read_secret(&prompt).map_err(failed)?;
    let key = ApiKey::new(&pasted).ok_or_else(|| failed("no key was pasted"))?;
    if !key.expose().starts_with(provider.key_prefix()) {
        eprintln!(
            "That doesn't look like an {company} key (they start {}); checking it anyway.",
            provider.key_prefix()
        );
    }
    eprintln!("Checking the key with {company}…");
    let models = match tasuke_models::cloud::check(provider, &key) {
        Ok(m) => m,
        Err(CheckError::Rejected) => {
            return Err(failed(format!("{company} didn't accept that key; check that it was copied whole")));
        }
        Err(e) => return Err(failed(e)),
    };
    secrets::store(provider, &key).map_err(|e| failed(format!("couldn't keep the key in the keyring: {e}")))?;
    let names: Vec<&str> = models.iter().take(4).map(|m| m.name.as_str()).collect();
    let more = models.len().saturating_sub(names.len());
    let more = if more > 0 { format!(" and {more} more") } else { String::new() };
    eprintln!("Connected {company} (key {}), kept in the keyring.", key.hint());
    eprintln!("The account can use {}{more}.", names.join(", "));
    eprintln!("What Tasuke sees in a conversation with one of these models is sent to {company}.");
    if let Some(m) = tasuke_models::Catalog::load_default().cloud.iter().find(|c| c.provider == provider) {
        eprintln!("To use one: tasuke use {}/{}", provider.as_str(), m.id);
    }
    Ok(())
}

fn remove(provider: CloudProvider) -> Result {
    secrets::clear(provider).map_err(|e| failed(format!("couldn't remove the key from the keyring: {e}")))?;
    eprintln!("Disconnected {}: its key is gone from the keyring.", provider.company());
    if std::env::var_os(provider.env_var()).is_some() {
        eprintln!("{} is still set in this environment, and Tasuke will use it.", provider.env_var());
    }
    Ok(())
}

/// A line from the terminal with echo off, or from stdin when it's a pipe.
fn read_secret(prompt: &str) -> std::io::Result<String> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    if !stdin.is_terminal() {
        stdin.lock().read_line(&mut line)?;
        return Ok(line);
    }
    eprint!("{prompt}");
    // SAFETY: tcgetattr and tcsetattr on stdin with a termios we own.
    let saved = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        (libc::tcgetattr(libc::STDIN_FILENO, &raw mut t) == 0).then_some(t)
    };
    if let Some(mut t) = saved {
        t.c_lflag &= !libc::ECHO;
        // SAFETY: as above.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const t) };
    }
    let read = stdin.lock().read_line(&mut line);
    if let Some(t) = saved {
        // SAFETY: as above, putting back what was there.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const t) };
    }
    eprintln!();
    read.map(|_| line)
}
