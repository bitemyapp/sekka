use crate::db::{IngestOutcome, Store};
use crate::events;
use anyhow::{Context, Result, ensure};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::timeout::TimeoutLayer;

const MAX_BODY: usize = 25 * 1024 * 1024;

#[derive(Clone)]
struct Receiver {
    secret: Arc<Vec<u8>>,
    store: Arc<Mutex<Store>>,
}

pub fn spawn(address: SocketAddr, secret: Vec<u8>, store: Arc<Mutex<Store>>) -> Result<()> {
    ensure!(
        address.ip().is_loopback(),
        "Webhook listener must bind a loopback address; use Cloudflare Tunnel for ingress"
    );
    ensure!(
        secret.len() >= 32,
        "Webhook secret must contain at least 32 bytes"
    );
    let listener = TcpListener::bind(address).context("Binding GitHub webhook listener")?;
    listener.set_nonblocking(true)?;
    // Bind synchronously: the listener exists before the startup feed request.
    eprintln!(
        "Webhook listener bound at http://{}/github/webhook",
        listener.local_addr()?
    );
    let receiver = Receiver {
        secret: Arc::new(secret),
        store,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("Starting webhook runtime")?;
    std::thread::Builder::new()
        .name("github-webhooks".into())
        .spawn(move || {
            let result: Result<()> = runtime.block_on(async move {
                let router = Router::new()
                    .route("/healthz", get(|| async { "ok\n" }))
                    .route("/github/webhook", post(receive))
                    .layer(DefaultBodyLimit::max(MAX_BODY))
                    .layer(ConcurrencyLimitLayer::new(32))
                    .layer(TimeoutLayer::with_status_code(
                        StatusCode::REQUEST_TIMEOUT,
                        Duration::from_secs(20),
                    ))
                    .with_state(receiver);
                axum::serve(tokio::net::TcpListener::from_std(listener)?, router).await?;
                Ok(())
            });
            eprintln!("Webhook listener exited: {result:?}");
            // A living worker without ingress is not a healthy webhook service.
            std::process::exit(1);
        })?;
    Ok(())
}

fn verify_signature(secret: &[u8], signature: &str, body: &[u8]) -> bool {
    let Some(encoded) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let mut expected = [0u8; 32];
    if hex::decode_to_slice(encoded, &mut expected).is_err() {
        return false;
    }
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

async fn receive(
    State(receiver): State<Receiver>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, &'static str) {
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify_signature(&receiver.secret, signature, &body) {
        return (StatusCode::UNAUTHORIZED, "invalid signature\n");
    }
    let Some(delivery) = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
    else {
        return (StatusCode::BAD_REQUEST, "missing delivery ID\n");
    };
    let Some(kind) = headers.get("x-github-event").and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing event type\n");
    };
    if delivery.is_empty()
        || delivery.len() > 128
        || !delivery
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || kind.is_empty()
        || kind.len() > 64
        || !kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return (StatusCode::BAD_REQUEST, "invalid event headers\n");
    }
    let kind = kind.to_owned();
    let delivery = delivery.to_owned();
    // SQLite fsync and payload normalization never block the async listener.
    match tokio::task::spawn_blocking(move || ingest(receiver, &kind, &delivery, &body)).await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Webhook ingestion worker failed: {error}");
            (StatusCode::SERVICE_UNAVAILABLE, "storage unavailable\n")
        }
    }
}

fn ingest(
    receiver: Receiver,
    kind: &str,
    delivery: &str,
    body: &[u8],
) -> (StatusCode, &'static str) {
    let payload: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid JSON\n"),
    };
    let repo = &payload["repository"];
    if repo["id"].as_u64() != Some(1412948279)
        || repo["full_name"].as_str() != Some(events::REPO)
        || repo["private"].as_bool() != Some(false)
    {
        return (
            StatusCode::FORBIDDEN,
            "only the public Sekka repository is accepted\n",
        );
    }
    let notification = match events::normalize_webhook(kind, &payload) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Rejected malformed {kind} delivery {delivery}: {error:#}");
            return (StatusCode::BAD_REQUEST, "unsupported payload shape\n");
        }
    };
    let fingerprint = hex::encode(Sha256::digest(body));
    let result = receiver
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("Database mutex poisoned"))
        .and_then(|mut store| {
            store.ingest(
                "webhook",
                delivery,
                kind,
                &fingerprint,
                notification.as_ref(),
                chrono::Utc::now().timestamp(),
            )
        });
    match result {
        Ok(outcome) => {
            let state = match outcome {
                IngestOutcome::Queued => "queued",
                IngestOutcome::Duplicate => "duplicate",
                IngestOutcome::Ignored => "ignored",
            };
            eprintln!("Webhook {delivery} ({kind}): {state}");
            (StatusCode::ACCEPTED, "recorded\n")
        }
        Err(error) => {
            eprintln!("Cannot commit webhook {delivery}: {error:#}");
            (StatusCode::SERVICE_UNAVAILABLE, "receipt not committed\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_authenticates_exact_bytes_and_secret() {
        let secret = b"a webhook secret kept outside the repository";
        let body = br#"{"action":"opened"}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_signature(secret, &signature, body));
        assert!(!verify_signature(
            secret,
            &signature,
            br#"{"action":"closed"}"#
        ));
        assert!(!verify_signature(b"different secret", &signature, body));
        assert!(!verify_signature(secret, "sha256=00", body));
        assert!(!verify_signature(
            secret,
            "sha1=0000000000000000000000000000000000000000",
            body
        ));
    }
}
