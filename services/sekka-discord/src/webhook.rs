use crate::db::{IngestOutcome, Store};
use crate::events;
use anyhow::{Context, Result, ensure};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::{Instant, timeout_at};

const MAX_BODY: usize = 25 * 1024 * 1024;
const MAX_IN_FLIGHT: usize = 2;
const BODY_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const REJECTION_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct Receiver {
    secret: Arc<Vec<u8>>,
    store: Arc<Mutex<Store>>,
    in_flight: Arc<Semaphore>,
    rejections: Arc<AtomicU64>,
}

impl Receiver {
    fn reject(&self, status: StatusCode, message: &'static str) -> (StatusCode, &'static str) {
        let mut count = self.rejections.load(Ordering::Relaxed);
        while let Err(current) = self.rejections.compare_exchange_weak(
            count,
            count.saturating_add(1),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            count = current;
        }
        (status, message)
    }
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
        in_flight: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
        rejections: Arc::new(AtomicU64::new(0)),
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
                let rejections = Arc::clone(&receiver.rejections);
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(REJECTION_INTERVAL).await;
                        let count = rejections.swap(0, Ordering::Relaxed);
                        if count != 0 {
                            eprintln!("SECURITY webhook_rejections count={count}");
                        }
                    }
                });
                let router = Router::new()
                    .route("/healthz", get(|| async { "ok\n" }))
                    .route("/github/webhook", post(receive))
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

fn signature_bytes(signature: &str) -> Option<[u8; 32]> {
    let encoded = signature.strip_prefix("sha256=")?;
    let mut expected = [0u8; 32];
    hex::decode_to_slice(encoded, &mut expected).ok()?;
    Some(expected)
}

fn header_signature(headers: &HeaderMap) -> Option<[u8; 32]> {
    let mut signatures = headers.get_all("x-hub-signature-256").iter();
    let expected = signature_bytes(signatures.next()?.to_str().ok()?)?;
    if signatures.next().is_some() {
        return None;
    }
    Some(expected)
}

fn verify_signature(secret: &[u8], expected: &[u8; 32], body: &[u8]) -> bool {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(expected).is_ok()
}

async fn read_body(
    body: Body,
    deadline: Instant,
) -> Result<axum::body::Bytes, (StatusCode, &'static str)> {
    match timeout_at(deadline, to_bytes(body, MAX_BODY)).await {
        Ok(Ok(body)) => Ok(body),
        Ok(Err(error)) => {
            use std::error::Error;
            if error
                .source()
                .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
            {
                Err((StatusCode::PAYLOAD_TOO_LARGE, "payload too large\n"))
            } else {
                Err((StatusCode::BAD_REQUEST, "cannot read body\n"))
            }
        }
        Err(_) => Err((StatusCode::REQUEST_TIMEOUT, "body read timed out\n")),
    }
}

async fn receive(State(receiver): State<Receiver>, request: Request) -> (StatusCode, &'static str) {
    receive_before(receiver, request, Instant::now() + REQUEST_TIMEOUT).await
}

async fn receive_before(
    receiver: Receiver,
    request: Request,
    deadline: Instant,
) -> (StatusCode, &'static str) {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    // Never poll an unauthenticated body's stream merely to discover that its
    // signature header is absent or malformed.
    let Some(signature) = header_signature(&headers) else {
        return receiver.reject(StatusCode::UNAUTHORIZED, "invalid signature\n");
    };
    let Some(delivery) = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
    else {
        return receiver.reject(StatusCode::BAD_REQUEST, "missing delivery ID\n");
    };
    let Some(kind) = headers.get("x-github-event").and_then(|v| v.to_str().ok()) else {
        return receiver.reject(StatusCode::BAD_REQUEST, "missing event type\n");
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
        return receiver.reject(StatusCode::BAD_REQUEST, "invalid event headers\n");
    }
    // try_acquire never queues request bodies behind busy workers.
    let Ok(permit) = Arc::clone(&receiver.in_flight).try_acquire_owned() else {
        return receiver.reject(StatusCode::SERVICE_UNAVAILABLE, "receiver busy\n");
    };
    let body = match read_body(body, (Instant::now() + BODY_TIMEOUT).min(deadline)).await {
        Ok(body) => body,
        Err((status, message)) => return receiver.reject(status, message),
    };
    let kind = kind.to_owned();
    let delivery = delivery.to_owned();
    let worker_receiver = receiver.clone();
    // Keep the permit INSIDE the blocking job: disconnects and deadline expiry
    // cannot cancel spawn_blocking or release capacity while it still owns a
    // payload or waits for SQLite. At most two jobs can exist, including queued
    // jobs. Exact-byte HMAC precedes JSON parsing on the same bounded worker.
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !verify_signature(&worker_receiver.secret, &signature, &body) {
            return worker_receiver.reject(StatusCode::UNAUTHORIZED, "invalid signature\n");
        }
        let result = ingest(&worker_receiver, &kind, &delivery, &body);
        if result.0 != StatusCode::ACCEPTED {
            worker_receiver.reject(result.0, result.1)
        } else {
            result
        }
    });
    match timeout_at(deadline, worker).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            eprintln!("Webhook ingestion worker failed: {error}");
            receiver.reject(StatusCode::SERVICE_UNAVAILABLE, "storage unavailable\n")
        }
        Err(_) => receiver.reject(StatusCode::REQUEST_TIMEOUT, "ingestion timed out\n"),
    }
}

fn ingest(
    receiver: &Receiver,
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
        Err(_) => return (StatusCode::BAD_REQUEST, "unsupported payload shape\n"),
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
    use axum::body::Bytes;
    use http_body::{Body as HttpBody, Frame};
    use std::future::{Future, poll_fn};
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::task::{Context as TaskContext, Poll};

    const SECRET: &[u8] = b"a webhook secret kept outside the repository";
    const PAYLOAD: &[u8] =
        br#"{"repository":{"id":1412948279,"full_name":"bitemyapp/sekka","private":false}}"#;

    struct Fixture {
        receiver: Receiver,
        directory: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "sekka-discord-webhook-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let receiver = Receiver {
                secret: Arc::new(SECRET.to_vec()),
                store: Arc::new(Mutex::new(Store::open(&directory, "channel", 100).unwrap())),
                in_flight: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
                rejections: Arc::new(AtomicU64::new(0)),
            };
            Self {
                receiver,
                directory,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn sign(body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn request(body: Body, signature: Option<&str>) -> Request {
        let mut request = Request::builder()
            .method("POST")
            .uri("/github/webhook")
            .header("x-github-delivery", "test-delivery")
            .header("x-github-event", "ping");
        if let Some(signature) = signature {
            request = request.header("x-hub-signature-256", signature);
        }
        request.body(body).unwrap()
    }

    #[derive(Default)]
    struct BodyProbe {
        polls: AtomicU64,
        drops: AtomicU64,
    }

    struct PendingBody(Arc<BodyProbe>);

    impl HttpBody for PendingBody {
        type Data = Bytes;
        type Error = std::convert::Infallible;

        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut TaskContext<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            self.0.polls.fetch_add(1, Ordering::Relaxed);
            Poll::Pending
        }
    }

    impl Drop for PendingBody {
        fn drop(&mut self) {
            self.0.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn signature_authenticates_exact_bytes_and_secret() {
        let body = br#"{"action":"opened"}"#;
        let signature = signature_bytes(&sign(body)).unwrap();
        assert!(verify_signature(SECRET, &signature, body));
        assert!(!verify_signature(
            SECRET,
            &signature,
            br#"{"action":"closed"}"#
        ));
        assert!(!verify_signature(b"different secret", &signature, body));
        assert!(signature_bytes("sha256=00").is_none());
        assert!(signature_bytes("sha1=0000000000000000000000000000000000000000").is_none());
    }

    #[test]
    fn missing_malformed_and_duplicate_signatures_never_poll_body() {
        let fixture = Fixture::new();
        runtime().block_on(async {
            for signature in [None, Some("sha256=00"), Some("sha256=not-hex")] {
                let probe = Arc::new(BodyProbe::default());
                let response = receive(
                    State(fixture.receiver.clone()),
                    request(Body::new(PendingBody(probe.clone())), signature),
                )
                .await;
                assert_eq!(response.0, StatusCode::UNAUTHORIZED);
                assert_eq!(probe.polls.load(Ordering::Relaxed), 0);
                assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
            }
            let probe = Arc::new(BodyProbe::default());
            let signature = sign(b"");
            let mut request = request(Body::new(PendingBody(probe.clone())), Some(&signature));
            request
                .headers_mut()
                .append("x-hub-signature-256", signature.parse().unwrap());
            assert_eq!(
                receive(State(fixture.receiver.clone()), request).await.0,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(probe.polls.load(Ordering::Relaxed), 0);
            assert_eq!(fixture.receiver.rejections.swap(0, Ordering::Relaxed), 4);
            assert_eq!(fixture.receiver.rejections.swap(0, Ordering::Relaxed), 0);
        });
    }

    #[test]
    fn missing_and_invalid_delivery_and_event_headers_never_poll_body() {
        let fixture = Fixture::new();
        let signature = sign(PAYLOAD);
        let long_delivery = "d".repeat(129);
        let long_event = "e".repeat(65);
        let cases = [
            (None, Some("ping")),
            (Some("test-delivery"), None),
            (Some(""), Some("ping")),
            (Some("two words"), Some("ping")),
            (Some("../delivery"), Some("ping")),
            (Some(long_delivery.as_str()), Some("ping")),
            (Some("test-delivery"), Some("")),
            (Some("test-delivery"), Some("Push")),
            (Some("test-delivery"), Some(long_event.as_str())),
        ];
        runtime().block_on(async {
            for (delivery, event) in cases {
                let probe = Arc::new(BodyProbe::default());
                let mut request = request(Body::new(PendingBody(probe.clone())), Some(&signature));
                for (name, value) in [("x-github-delivery", delivery), ("x-github-event", event)] {
                    request.headers_mut().remove(name);
                    if let Some(value) = value {
                        request.headers_mut().insert(name, value.parse().unwrap());
                    }
                }
                assert_eq!(
                    receive(State(fixture.receiver.clone()), request).await.0,
                    StatusCode::BAD_REQUEST,
                    "{delivery:?} {event:?}"
                );
                assert_eq!(probe.polls.load(Ordering::Relaxed), 0);
            }
            assert_eq!(
                fixture.receiver.rejections.swap(0, Ordering::Relaxed),
                cases.len() as u64
            );
            // The longest identifiers the receiver takes still pass.
            let mut accepted = request(Body::from(PAYLOAD), Some(&signature));
            let longest = [
                ("x-github-delivery", "d".repeat(128)),
                ("x-github-event", "e".repeat(64)),
            ];
            for (name, value) in longest {
                accepted.headers_mut().insert(name, value.parse().unwrap());
            }
            assert_eq!(
                receive(State(fixture.receiver.clone()), accepted).await.0,
                StatusCode::ACCEPTED
            );
        });
    }

    #[test]
    fn overload_is_immediate_and_does_not_poll_body() {
        let fixture = Fixture::new();
        let _permits = fixture
            .receiver
            .in_flight
            .try_acquire_many(MAX_IN_FLIGHT as u32)
            .unwrap();
        let probe = Arc::new(BodyProbe::default());
        let response = runtime().block_on(receive(
            State(fixture.receiver.clone()),
            request(Body::new(PendingBody(probe.clone())), Some(&sign(b""))),
        ));
        assert_eq!(response.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(probe.polls.load(Ordering::Relaxed), 0);
        assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
        assert_eq!(fixture.receiver.rejections.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn bad_hmac_precedes_json_and_repository_validation() {
        let fixture = Fixture::new();
        runtime().block_on(async {
            for body in [b"not JSON".as_slice(), b"{}".as_slice()] {
                assert_eq!(
                    receive(
                        State(fixture.receiver.clone()),
                        request(Body::from(body.to_vec()), Some(&sign(b"different bytes"))),
                    )
                    .await
                    .0,
                    StatusCode::UNAUTHORIZED
                );
            }
            for (body, expected) in [
                (b"not JSON".as_slice(), StatusCode::BAD_REQUEST),
                (b"{}".as_slice(), StatusCode::FORBIDDEN),
            ] {
                assert_eq!(
                    receive(
                        State(fixture.receiver.clone()),
                        request(Body::from(body.to_vec()), Some(&sign(body))),
                    )
                    .await
                    .0,
                    expected
                );
            }
            assert_eq!(
                fixture.receiver.store.lock().unwrap().status().unwrap()["receipts"],
                0
            );
        });
    }

    #[test]
    fn exact_maximum_signed_body_is_committed_but_extra_byte_is_rejected() {
        let fixture = Fixture::new();
        runtime().block_on(async {
            let mut payload = PAYLOAD.to_vec();
            payload.resize(MAX_BODY, b' ');
            let signature = sign(&payload);
            assert_eq!(
                receive(
                    State(fixture.receiver.clone()),
                    request(Body::from(payload), Some(&signature)),
                )
                .await
                .0,
                StatusCode::ACCEPTED
            );
            assert_eq!(
                fixture.receiver.store.lock().unwrap().status().unwrap()["receipts"],
                1
            );
            assert_eq!(
                receive(
                    State(fixture.receiver.clone()),
                    request(Body::from(vec![b' '; MAX_BODY + 1]), Some(&signature)),
                )
                .await
                .0,
                StatusCode::PAYLOAD_TOO_LARGE
            );
            assert_eq!(
                fixture.receiver.in_flight.available_permits(),
                MAX_IN_FLIGHT
            );
        });
    }

    #[test]
    fn stalled_body_deadline_drops_stream_and_releases_capacity() {
        let fixture = Fixture::new();
        let probe = Arc::new(BodyProbe::default());
        let response = runtime().block_on(async {
            receive_before(
                fixture.receiver.clone(),
                request(Body::new(PendingBody(probe.clone())), Some(&sign(b""))),
                Instant::now() + Duration::from_millis(10),
            )
            .await
        });
        assert_eq!(response.0, StatusCode::REQUEST_TIMEOUT);
        assert!(probe.polls.load(Ordering::Relaxed) > 0);
        assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
        assert_eq!(
            fixture.receiver.in_flight.available_permits(),
            MAX_IN_FLIGHT
        );
        assert_eq!(fixture.receiver.rejections.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancellation_while_reading_releases_capacity() {
        let fixture = Fixture::new();
        let probe = Arc::new(BodyProbe::default());
        runtime().block_on(async {
            let mut pending = Box::pin(receive(
                State(fixture.receiver.clone()),
                request(Body::new(PendingBody(probe.clone())), Some(&sign(b""))),
            ));
            poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            assert_eq!(
                fixture.receiver.in_flight.available_permits(),
                MAX_IN_FLIGHT - 1
            );
            drop(pending);
            assert_eq!(
                fixture.receiver.in_flight.available_permits(),
                MAX_IN_FLIGHT
            );
            assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
        });
    }

    #[test]
    fn timeout_and_cancellation_keep_blocking_ingest_capacity_reserved() {
        let fixture = Fixture::new();
        runtime().block_on(async {
            for cancel in [false, true] {
                let store = Arc::clone(&fixture.receiver.store);
                let (locked_tx, locked_rx) = std::sync::mpsc::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let holder = std::thread::spawn(move || {
                    // Deliberately hold the production database mutex while
                    // the HTTP future is cancelled or reaches its deadline.
                    let _guard = store.lock().unwrap();
                    locked_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                });
                locked_rx.recv().unwrap();
                let mut pending = Box::pin(receive_before(
                    fixture.receiver.clone(),
                    request(Body::from(PAYLOAD), Some(&sign(PAYLOAD))),
                    Instant::now() + Duration::from_millis(100),
                ));
                if cancel {
                    poll_fn(|cx| {
                        assert!(pending.as_mut().poll(cx).is_pending());
                        Poll::Ready(())
                    })
                    .await;
                    drop(pending);
                } else {
                    assert_eq!(pending.await.0, StatusCode::REQUEST_TIMEOUT);
                }
                assert_eq!(
                    fixture.receiver.in_flight.available_permits(),
                    MAX_IN_FLIGHT - 1
                );
                release_tx.send(()).unwrap();
                // Completion, not the HTTP deadline or disconnect, returns
                // the permit and makes another blocking job admissible.
                let permits = tokio::time::timeout(
                    Duration::from_secs(5),
                    fixture
                        .receiver
                        .in_flight
                        .acquire_many(MAX_IN_FLIGHT as u32),
                )
                .await
                .unwrap()
                .unwrap();
                drop(permits);
                holder.join().unwrap();
            }
        });
    }
}
