use crate::db::{Outbox, Store};
use crate::events;
use crate::http::{Api, Rejected};
use crate::{
    Config, configured_path, fetch_events, github, read_token, snowflake, validate_channel,
    validate_permissions,
};
use anyhow::{Context, Result, ensure};
use reqwest::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Database = Arc<Mutex<Store>>;

fn with_store<T>(database: &Database, f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    let mut store = database
        .lock()
        .map_err(|_| anyhow::anyhow!("Database mutex poisoned"))?;
    f(&mut store)
}

pub fn run(config: &Config) -> Result<()> {
    let channel = config
        .channel
        .as_deref()
        .context("Set DISCORD_CHANNEL_ID before running the webhook service")?;
    let activation = match env::var("SEKKA_ACTIVATED_AT") {
        Ok(value) => chrono::DateTime::parse_from_rfc3339(&value)
            .context("Invalid SEKKA_ACTIVATED_AT")?
            .timestamp(),
        Err(_) => chrono::Utc::now().timestamp(),
    };
    let database = Arc::new(Mutex::new(Store::open(
        &config.state_dir,
        channel,
        activation,
    )?));
    let secret_path = configured_path(
        "GITHUB_WEBHOOK_SECRET_FILE",
        ".config/sekka-discord/webhook-secret",
    )?;
    let secret = read_token(&secret_path)?.into_bytes();
    let address = env::var("SEKKA_LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()
        .context("Invalid SEKKA_LISTEN_ADDR")?;
    crate::webhook::spawn(address, secret, Arc::clone(&database))?;

    let ready = Arc::new(AtomicBool::new(false));
    let backfill_database = Arc::clone(&database);
    let backfill_ready = Arc::clone(&ready);
    std::thread::Builder::new().name("github-catch-up".into()).spawn(move || {
        loop {
            match catch_up(&backfill_database) {
                Ok(()) => {
                    backfill_ready.store(true, Ordering::Release);
                    break;
                }
                Err(error) => {
                    eprintln!("Startup catch-up failed; webhook receipts remain durable; retrying in 30s: {error:#}");
                    std::thread::sleep(Duration::from_secs(30));
                }
            }
        }
    })?;

    // The listener is already accepting durable receipts, even if either remote
    // API is offline. Finish alias migration before dispatching anything.
    while !ready.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_secs(1));
    }
    let (discord, bot) = loop {
        match validate_destination(config, channel) {
            Ok(value) => break value,
            Err(error) => {
                eprintln!(
                    "Discord destination unavailable; keeping notifications queued: {error:#}"
                );
                std::thread::sleep(Duration::from_secs(30));
            }
        }
    };
    let mut next_recovery = Instant::now();
    loop {
        if Instant::now() >= next_recovery {
            if let Err(error) = reconcile(&discord, channel, &bot, &database) {
                eprintln!(
                    "Cannot reconcile interrupted sends; no ambiguous notification will be reposted: {error:#}"
                );
            }
            next_recovery = Instant::now() + Duration::from_secs(60);
        }
        let Some(outgoing) = with_store(&database, |store| {
            store.claim_next(chrono::Utc::now().timestamp())
        })?
        else {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        match send(&discord, channel, &outgoing) {
            Ok(message_id) => {
                with_store(&database, |store| {
                    store.mark_sent(&outgoing.key, &message_id, chrono::Utc::now().timestamp())
                })?;
                eprintln!("Notified {} as Discord message {message_id}", outgoing.key);
            }
            Err(error) => {
                if error.downcast_ref::<Rejected>().is_some() {
                    with_store(&database, |store| store.retry_rejected(&outgoing.key))?;
                    eprintln!(
                        "Discord rejected {}; queued for retry: {error:#}",
                        outgoing.key
                    );
                    std::thread::sleep(Duration::from_secs(30));
                } else {
                    eprintln!(
                        "Uncertain Discord send for {}; retaining sending state for history reconciliation: {error:#}",
                        outgoing.key
                    );
                    next_recovery = Instant::now();
                }
            }
        }
    }
}

fn validate_destination(config: &Config, channel_id: &str) -> Result<(Api, String)> {
    let api = Api::new(false, Some(&read_token(&config.token_file)?))?;
    let identity = api.get("/users/@me")?;
    ensure!(
        identity["bot"].as_bool() == Some(true),
        "Discord credentials must identify a bot account"
    );
    let bot = snowflake(&identity["id"], "Discord bot ID")?;
    let channel = api.get(&format!("/channels/{channel_id}"))?;
    validate_channel(&channel, &config.guild)?;
    validate_permissions(&api, &config.guild, &bot, &channel)?;
    Ok((api, bot))
}

fn catch_up(database: &Database) -> Result<()> {
    eprintln!("Starting Events API catch-up after webhook listener bind");
    let api = github()?;
    let feed = fetch_events(&api)?;
    let mut overlap = false;
    for event in feed.iter().rev() {
        overlap |= with_store(database, |store| store.has_receipt("events_api", &event.id))?;
        let normalized = events::normalize_event(event)
            .with_context(|| format!("Normalizing Events API record {}", event.id))?;
        // The source ID is authoritative; mutable titles/actor profiles are not
        // a receipt collision. Webhooks instead fingerprint exact signed bytes.
        let identity = normalized
            .as_ref()
            .map(|n| n.key.as_str())
            .unwrap_or(&event.id);
        let fingerprint = hex::encode(Sha256::digest(identity.as_bytes()));
        with_store(database, |store| {
            store.ingest(
                "events_api",
                &event.id,
                &event.kind,
                &fingerprint,
                normalized.as_ref(),
                chrono::Utc::now().timestamp(),
            )
        })?;
    }
    if !overlap && feed.len() == 300 {
        eprintln!(
            "WARNING: catch-up reached the 300-event limit with no prior API receipt overlap; the retained feed cannot prove complete recovery"
        );
    }
    with_store(database, |store| {
        store.finish_backfill(chrono::Utc::now().timestamp())
    })?;
    let status = with_store(database, Store::status)?;
    eprintln!(
        "Startup catch-up complete: {} feed records; {status}. Webhook-only operation begins",
        feed.len()
    );
    Ok(())
}

fn send(api: &Api, channel: &str, outgoing: &Outbox) -> Result<String> {
    let response = api.request(
        Method::POST,
        &format!("/channels/{channel}/messages"),
        Some(&outgoing.message),
    )?;
    ensure!(
        response["channel_id"].as_str() == Some(channel),
        "Discord returned a different destination channel"
    );
    snowflake(&response["id"], "Discord notification message ID")
}

fn message_marker(message: &Value) -> Option<&str> {
    message["embeds"].as_array()?.iter().find_map(|embed| {
        embed["footer"]["text"]
            .as_str()?
            .split_whitespace()
            .find(|word| {
                word.strip_prefix("yuki:").is_some_and(|hash| {
                    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
                })
            })
    })
}

fn reconcile(api: &Api, channel: &str, bot: &str, database: &Database) -> Result<()> {
    let inflight = with_store(database, Store::inflight)?;
    if inflight.is_empty() {
        return Ok(());
    }
    // Alias migration may have preserved an already-attempted legacy marker.
    // Match the durable message's actual marker, not a newly computed key.
    let mut unmatched: HashMap<&str, &Outbox> = inflight
        .iter()
        .map(|row| {
            Ok((
                message_marker(&row.message)
                    .context("Interrupted notification has no reconciliation marker")?,
                row,
            ))
        })
        .collect::<Result<_>>()?;
    let oldest = inflight
        .iter()
        .map(|row| row.attempted_at)
        .min()
        .unwrap_or(0)
        .saturating_sub(60);
    let mut before: Option<String> = None;
    loop {
        let path = match &before {
            Some(id) => format!("/channels/{channel}/messages?limit=100&before={id}"),
            None => format!("/channels/{channel}/messages?limit=100"),
        };
        let response = api.get(&path)?;
        let messages = response
            .as_array()
            .context("Invalid Discord message history response")?;
        if messages.is_empty() {
            break;
        }
        let mut reached_start = false;
        for message in messages {
            let timestamp = chrono::DateTime::parse_from_rfc3339(
                message["timestamp"]
                    .as_str()
                    .context("Missing Discord message timestamp")?,
            )?
            .timestamp();
            if timestamp < oldest {
                reached_start = true;
            }
            if message["author"]["id"].as_str() != Some(bot) {
                continue;
            }
            if let Some(marker) = message_marker(message)
                && let Some(row) = unmatched.remove(marker)
            {
                let id = snowflake(&message["id"], "recovered Discord message ID")?;
                with_store(database, |store| {
                    store.mark_sent(&row.key, &id, chrono::Utc::now().timestamp())
                })?;
                eprintln!(
                    "Recovered interrupted notification {} as Discord message {id}",
                    row.key
                );
            }
        }
        if unmatched.is_empty() || reached_start || messages.len() < 100 {
            break;
        }
        let next = snowflake(
            &messages.last().context("Empty Discord history page")?["id"],
            "history cursor",
        )?;
        ensure!(
            before.as_ref() != Some(&next),
            "Discord history cursor did not advance"
        );
        before = Some(next);
    }
    for row in unmatched.values() {
        eprintln!(
            "UNRESOLVED send {} attempted at {}; no matching Discord history message; retained without reposting to avoid duplicates",
            row.key, row.attempted_at
        );
    }
    Ok(())
}
