use crate::db::{IngestOutcome, Store};
use crate::events;
use anyhow::{Context, Result, ensure};
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::CONTENT_LENGTH;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use hmac::{Hmac, Mac};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::{Service, service_fn};
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use tokio::task::AbortHandle;
use tokio::time::{Instant, timeout_at};

const MAX_BODY: usize = 25 * 1024 * 1024;
// A body is the only proof of the secret, so it has to be read for anyone who
// sends one. What the receiver holds for senders it has not verified is still
// bounded: for each connection a request head and this much body, and for all
// bodies needing more one shared allowance. Together that is
// MAX_CONNECTIONS * (MAX_HEAD + UNSHARED_BODY) + SHARED_BODIES, or 90 MiB.
const MAX_HEAD: usize = 64 * 1024;
const UNSHARED_BODY: usize = 256 * 1024;
const SHARED_BODIES: usize = 2 * MAX_BODY;
// The unit allows 1,024 file descriptors (LimitNOFILE in
// nix/sekka-discord-service.nix); most are left to everything else.
const MAX_CONNECTIONS: usize = 128;
const _: () = assert!(MAX_CONNECTIONS <= 1024 / 4);
const MAX_IN_FLIGHT: usize = 2;
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);
const BODY_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);
const REJECTION_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct Receiver {
    secret: Arc<Vec<u8>>,
    store: Arc<Mutex<Store>>,
    in_flight: Arc<Semaphore>,
    shared_bodies: Arc<Semaphore>,
    rejections: Arc<AtomicU64>,
}

impl Receiver {
    fn new(secret: Vec<u8>, store: Arc<Mutex<Store>>) -> Self {
        Self {
            secret: Arc::new(secret),
            store,
            in_flight: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
            shared_bodies: Arc::new(Semaphore::new(SHARED_BODIES)),
            rejections: Arc::new(AtomicU64::new(0)),
        }
    }

    fn count_rejection(&self) {
        let mut count = self.rejections.load(Ordering::Relaxed);
        while let Err(current) = self.rejections.compare_exchange_weak(
            count,
            count.saturating_add(1),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            count = current;
        }
    }

    fn reject(&self, status: StatusCode, message: &'static str) -> (StatusCode, &'static str) {
        self.count_rejection();
        (status, message)
    }
}

#[derive(Clone, Copy)]
struct Limits {
    connections: usize,
    /// From accepting a connection to its complete request head.
    head: Duration,
    /// From accepting a connection to the end of its one request.
    connection: Duration,
}

const LIMITS: Limits = Limits {
    connections: MAX_CONNECTIONS,
    head: HEAD_TIMEOUT,
    connection: CONNECTION_TIMEOUT,
};

/// The connections the listener holds. Those already here never keep a new one
/// out: when every place is taken, the connection that has gone longest without
/// showing the secret is closed. An idle or stalled sender therefore costs a
/// signed delivery nothing, however many of them there are.
struct Gate {
    places: Arc<Semaphore>,
    unproven: Mutex<VecDeque<(u64, AbortHandle)>>,
}

impl Gate {
    fn new(connections: usize) -> Arc<Self> {
        Arc::new(Self {
            places: Arc::new(Semaphore::new(connections)),
            unproven: Mutex::new(VecDeque::new()),
        })
    }

    fn unproven(&self) -> MutexGuard<'_, VecDeque<(u64, AbortHandle)>> {
        self.unproven
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One connection's place, given up when the connection ends, however it ends.
struct Place {
    id: u64,
    gate: Arc<Gate>,
    head: AtomicBool,
    _place: OwnedSemaphorePermit,
}

impl Place {
    /// Its sender has shown the secret: it no longer gives way to newcomers.
    fn proven(&self) {
        self.gate.unproven().retain(|(id, _)| *id != self.id);
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.proven();
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
    let receiver = Receiver::new(secret, store);
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
                let listener = tokio::net::TcpListener::from_std(listener)?;
                serve(listener, receiver, LIMITS, Gate::new(LIMITS.connections)).await;
                Ok(())
            });
            eprintln!("Webhook listener exited: {result:?}");
            // A living worker without ingress is not a healthy webhook service.
            std::process::exit(1);
        })?;
    Ok(())
}

async fn serve(
    listener: tokio::net::TcpListener,
    receiver: Receiver,
    limits: Limits,
    gate: Arc<Gate>,
) {
    let router = Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route("/github/webhook", post(receive))
        .with_state(receiver.clone());
    let service = TowerToHyperService::new(router);
    let mut id = 0;
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            // A connection that was gone before it was accepted, or no
            // descriptor to accept it with: neither ends the listener.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        let place = match Arc::clone(&gate.places).try_acquire_owned() {
            Ok(place) => place,
            Err(_) => {
                let oldest = gate.unproven().pop_front();
                if let Some((_, oldest)) = oldest {
                    oldest.abort();
                    receiver.count_rejection();
                }
                // Free as soon as the connection closed above is gone. When
                // every connection is a verified delivery, each ends within
                // its deadline.
                let Ok(place) = Arc::clone(&gate.places).acquire_owned().await else {
                    return;
                };
                place
            }
        };
        id += 1;
        let place = Arc::new(Place {
            id,
            gate: Arc::clone(&gate),
            head: AtomicBool::new(false),
            _place: place,
        });
        // A connection is handed its place only once it is listed, so it cannot
        // leave the list before it is on it. Nothing is spawned or dropped
        // under the list's lock: a runtime that is shutting down drops what it
        // is asked to spawn there and then, and a place dropped by the thread
        // that holds the lock would wait for that lock for ever.
        let (listed, place_when_listed) = oneshot::channel();
        let (service, receiver) = (service.clone(), receiver.clone());
        let task = tokio::spawn(async move {
            if let Ok(place) = place_when_listed.await {
                connection(stream, service, place, receiver, limits).await;
            }
        });
        gate.unproven().push_back((id, task.abort_handle()));
        // A task that is already gone hands the place straight back, and
        // dropping it here takes the connection off the list again.
        let _ = listed.send(place);
    }
}

async fn connection(
    stream: TcpStream,
    service: TowerToHyperService<Router>,
    place: Arc<Place>,
    receiver: Receiver,
    limits: Limits,
) {
    let accepted = Instant::now();
    let requested = Arc::clone(&place);
    let service = service_fn(move |mut request: hyper::Request<Incoming>| {
        requested.head.store(true, Ordering::Release);
        request.extensions_mut().insert(Arc::clone(&requested));
        service.call(request)
    });
    // One request for each connection. A connection kept open between requests
    // would have to be closed when idle, and a client that reuses it at that
    // moment loses a delivery that cannot be sent again.
    let mut served = std::pin::pin!(
        http1::Builder::new()
            .keep_alive(false)
            .max_buf_size(MAX_HEAD)
            .serve_connection(TokioIo::new(stream), service)
    );
    if timeout_at(accepted + limits.head, &mut served)
        .await
        .is_ok()
    {
        return;
    }
    if !place.head.load(Ordering::Acquire) {
        receiver.count_rejection();
        return;
    }
    let _ = timeout_at(accepted + limits.connection, served).await;
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

/// A body the receiver holds, with what it took of the shared allowance.
struct Held {
    bytes: Vec<u8>,
    shared: Option<OwnedSemaphorePermit>,
}

/// Reads a body within the bounds above and returns it only if its exact bytes
/// carry the signature. Reading waits for nobody and keeps nobody waiting: a
/// sender that stalls holds only what it has sent, until the deadline.
async fn read_body(
    receiver: &Receiver,
    mut body: Body,
    signature: &[u8; 32],
    deadline: Instant,
) -> Result<Held, (StatusCode, &'static str)> {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&receiver.secret) else {
        return Err((StatusCode::UNAUTHORIZED, "invalid signature\n"));
    };
    let mut held = Held {
        bytes: Vec::new(),
        shared: None,
    };
    loop {
        let frame = match timeout_at(deadline, body.frame()).await {
            Err(_) => return Err((StatusCode::REQUEST_TIMEOUT, "body read timed out\n")),
            Ok(None) => break,
            Ok(Some(Err(_))) => return Err((StatusCode::BAD_REQUEST, "cannot read body\n")),
            Ok(Some(Ok(frame))) => frame,
        };
        let Ok(data) = frame.into_data() else {
            continue;
        };
        let before = held.bytes.len();
        let after = before + data.len();
        if after > MAX_BODY {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "payload too large\n"));
        }
        let shared = after.saturating_sub(UNSHARED_BODY) - before.saturating_sub(UNSHARED_BODY);
        if shared > 0 {
            let more = u32::try_from(shared).ok().and_then(|more| {
                Arc::clone(&receiver.shared_bodies)
                    .try_acquire_many_owned(more)
                    .ok()
            });
            let Some(more) = more else {
                return Err((StatusCode::SERVICE_UNAVAILABLE, "receiver busy\n"));
            };
            match &mut held.shared {
                Some(shared) => shared.merge(more),
                None => held.shared = Some(more),
            }
        }
        mac.update(&data);
        held.bytes.extend_from_slice(&data);
    }
    if mac.verify_slice(signature).is_err() {
        return Err((StatusCode::UNAUTHORIZED, "invalid signature\n"));
    }
    Ok(held)
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
    let headers = &parts.headers;
    // Never poll an unauthenticated body's stream merely to discover that its
    // signature header is absent or malformed.
    let Some(signature) = header_signature(headers) else {
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
    let declared = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|length| length > MAX_BODY as u64) {
        return receiver.reject(StatusCode::PAYLOAD_TOO_LARGE, "payload too large\n");
    }
    let body_deadline = (Instant::now() + BODY_TIMEOUT).min(deadline);
    let held = match read_body(&receiver, body, &signature, body_deadline).await {
        Ok(held) => held,
        Err((status, message)) => return receiver.reject(status, message),
    };
    if let Some(place) = parts.extensions.get::<Arc<Place>>() {
        place.proven();
    }
    // Only a verified delivery waits for one of the workers that parse and
    // commit. Its body stays counted against the allowances until that is done.
    let Ok(Ok(permit)) =
        timeout_at(deadline, Arc::clone(&receiver.in_flight).acquire_owned()).await
    else {
        return receiver.reject(StatusCode::SERVICE_UNAVAILABLE, "receiver busy\n");
    };
    let kind = kind.to_owned();
    let delivery = delivery.to_owned();
    let worker_receiver = receiver.clone();
    // Keep the permit INSIDE the blocking job: disconnects and deadline expiry
    // cannot cancel spawn_blocking or release capacity while it still owns a
    // payload or waits for SQLite. At most two jobs can exist. The exact bytes
    // were authenticated as they were read, before any of them is parsed.
    let worker = tokio::task::spawn_blocking(move || {
        // Both whole: the body's share of the allowance stays with its bytes.
        let (_permit, held) = (permit, held);
        let result = ingest(&worker_receiver, &kind, &delivery, &held.bytes);
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
    use std::io::{Read, Write};
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
            let store = Store::open(&directory, "channel", 100).unwrap();
            Self {
                receiver: Receiver::new(SECRET.to_vec(), Arc::new(Mutex::new(store))),
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

    /// Sends its first bytes and then nothing more.
    struct StalledBody {
        sent: Option<Bytes>,
        probe: Arc<BodyProbe>,
    }

    impl HttpBody for StalledBody {
        type Data = Bytes;
        type Error = std::convert::Infallible;

        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut TaskContext<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            let body = self.get_mut();
            body.probe.polls.fetch_add(1, Ordering::Relaxed);
            match body.sent.take() {
                Some(sent) => Poll::Ready(Some(Ok(Frame::data(sent)))),
                None => Poll::Pending,
            }
        }
    }

    impl Drop for StalledBody {
        fn drop(&mut self) {
            self.probe.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn signature_authenticates_exact_bytes_and_secret() {
        let fixture = Fixture::new();
        let body = br#"{"action":"opened"}"#;
        let signature = signature_bytes(&sign(body)).unwrap();
        let read = |receiver: &Receiver, sent: &'static [u8]| {
            let deadline = Instant::now() + BODY_TIMEOUT;
            runtime()
                .block_on(read_body(receiver, Body::from(sent), &signature, deadline))
                .map(|held| held.bytes)
                .map_err(|refused| refused.0)
        };
        assert_eq!(read(&fixture.receiver, body), Ok(body.to_vec()));
        assert_eq!(
            read(&fixture.receiver, br#"{"action":"closed"}"#),
            Err(StatusCode::UNAUTHORIZED)
        );
        let mut other = fixture.receiver.clone();
        other.secret = Arc::new(b"different secret".to_vec());
        assert_eq!(read(&other, body), Err(StatusCode::UNAUTHORIZED));
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
            assert_eq!(
                fixture.receiver.shared_bodies.available_permits(),
                SHARED_BODIES
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
    fn cancellation_while_reading_returns_what_the_body_held() {
        let fixture = Fixture::new();
        let probe = Arc::new(BodyProbe::default());
        runtime().block_on(async {
            let body = StalledBody {
                sent: Some(Bytes::from(vec![b' '; UNSHARED_BODY + 4096])),
                probe: probe.clone(),
            };
            let mut pending = Box::pin(receive(
                State(fixture.receiver.clone()),
                request(Body::new(body), Some(&sign(b""))),
            ));
            poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            // Reading takes none of the workers, and of the shared allowance
            // only what the body has sent beyond its unshared part.
            assert_eq!(
                fixture.receiver.in_flight.available_permits(),
                MAX_IN_FLIGHT
            );
            assert_eq!(
                fixture.receiver.shared_bodies.available_permits(),
                SHARED_BODIES - 4096
            );
            drop(pending);
            assert_eq!(
                fixture.receiver.shared_bodies.available_permits(),
                SHARED_BODIES
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

    #[test]
    fn busy_workers_refuse_only_a_verified_delivery_and_only_at_its_deadline() {
        let fixture = Fixture::new();
        let permits = Arc::clone(&fixture.receiver.in_flight)
            .try_acquire_many_owned(MAX_IN_FLIGHT as u32)
            .unwrap();
        runtime().block_on(async {
            // An unverified body is read and refused without waiting for a worker.
            assert_eq!(
                receive(
                    State(fixture.receiver.clone()),
                    request(Body::from(PAYLOAD), Some(&sign(b"different bytes"))),
                )
                .await
                .0,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                receive_before(
                    fixture.receiver.clone(),
                    request(Body::from(PAYLOAD), Some(&sign(PAYLOAD))),
                    Instant::now() + Duration::from_millis(50),
                )
                .await,
                (StatusCode::SERVICE_UNAVAILABLE, "receiver busy\n")
            );
            // A worker that frees up before the deadline takes the delivery.
            let waiting = tokio::spawn(receive(
                State(fixture.receiver.clone()),
                request(Body::from(PAYLOAD), Some(&sign(PAYLOAD))),
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(!waiting.is_finished());
            drop(permits);
            assert_eq!(waiting.await.unwrap().0, StatusCode::ACCEPTED);
        });
        assert_eq!(fixture.receiver.rejections.load(Ordering::Relaxed), 2);
        assert_eq!(
            fixture.receiver.in_flight.available_permits(),
            MAX_IN_FLIGHT
        );
    }

    #[test]
    fn signed_delivery_is_taken_while_unverified_senders_withhold_their_bodies() {
        let fixture = Fixture::new();
        runtime().block_on(async {
            let probe = Arc::new(BodyProbe::default());
            let unverified = format!("sha256={}", "0".repeat(64));
            let mut stalled: Vec<_> = (0..MAX_CONNECTIONS)
                .map(|_| {
                    Box::pin(receive(
                        State(fixture.receiver.clone()),
                        request(Body::new(PendingBody(probe.clone())), Some(&unverified)),
                    ))
                })
                .collect();
            poll_fn(|cx| {
                for sender in &mut stalled {
                    assert!(sender.as_mut().poll(cx).is_pending());
                }
                Poll::Ready(())
            })
            .await;
            assert_eq!(probe.polls.load(Ordering::Relaxed), MAX_CONNECTIONS as u64);
            for _ in 0..40 {
                assert_eq!(
                    receive(
                        State(fixture.receiver.clone()),
                        request(Body::from(PAYLOAD), Some(&sign(PAYLOAD))),
                    )
                    .await
                    .0,
                    StatusCode::ACCEPTED
                );
            }
            assert_eq!(fixture.receiver.rejections.load(Ordering::Relaxed), 0);
        });
    }

    #[test]
    fn large_unverified_bodies_share_one_allowance_and_never_crowd_out_a_small_delivery() {
        let fixture = Fixture::new();
        let shared = &fixture.receiver.shared_bodies;
        runtime().block_on(async {
            let probe = Arc::new(BodyProbe::default());
            let unverified = format!("sha256={}", "0".repeat(64));
            let sender = |sent: usize| {
                let body = StalledBody {
                    sent: Some(Bytes::from(vec![b' '; sent])),
                    probe: probe.clone(),
                };
                Box::pin(receive(
                    State(fixture.receiver.clone()),
                    request(Body::new(body), Some(&unverified)),
                ))
            };
            // Up to its unshared part a body costs the others nothing.
            let mut small = sender(UNSHARED_BODY);
            // Two of the largest size and a third take all that is shared, then stall.
            let rest = SHARED_BODIES - 2 * (MAX_BODY - UNSHARED_BODY);
            let mut large = [
                sender(MAX_BODY),
                sender(MAX_BODY),
                sender(UNSHARED_BODY + rest),
            ];
            poll_fn(|cx| {
                assert!(small.as_mut().poll(cx).is_pending());
                for sender in &mut large {
                    assert!(sender.as_mut().poll(cx).is_pending());
                }
                Poll::Ready(())
            })
            .await;
            assert_eq!(shared.available_permits(), 0);
            // Another has nowhere to put its first shared byte and is refused at once,
            assert_eq!(
                sender(UNSHARED_BODY + 1).await,
                (StatusCode::SERVICE_UNAVAILABLE, "receiver busy\n")
            );
            // while a delivery that fits its unshared part is taken as ever.
            assert_eq!(
                receive(
                    State(fixture.receiver.clone()),
                    request(Body::from(PAYLOAD), Some(&sign(PAYLOAD))),
                )
                .await
                .0,
                StatusCode::ACCEPTED
            );
            // What a sender held returns when it goes, by deadline or disconnect.
            drop(large);
            assert_eq!(shared.available_permits(), SHARED_BODIES);
            drop(small);
        });
    }

    #[test]
    fn declared_oversize_is_refused_without_reading() {
        let fixture = Fixture::new();
        runtime().block_on(async {
            for (declared, expected) in [
                (MAX_BODY + 1, StatusCode::PAYLOAD_TOO_LARGE),
                (MAX_BODY, StatusCode::REQUEST_TIMEOUT),
            ] {
                let probe = Arc::new(BodyProbe::default());
                let mut request = request(Body::new(PendingBody(probe.clone())), Some(&sign(b"")));
                request
                    .headers_mut()
                    .insert(CONTENT_LENGTH, declared.to_string().parse().unwrap());
                let deadline = Instant::now() + Duration::from_millis(10);
                assert_eq!(
                    receive_before(fixture.receiver.clone(), request, deadline)
                        .await
                        .0,
                    expected
                );
                assert_eq!(
                    probe.polls.load(Ordering::Relaxed) == 0,
                    declared > MAX_BODY
                );
            }
        });
    }

    struct Listening {
        address: SocketAddr,
        gate: Arc<Gate>,
        _runtime: tokio::runtime::Runtime,
        fixture: Fixture,
    }

    /// The real listener on a port of its own, with the limits a test asks for.
    fn listen(limits: Limits) -> Listening {
        let fixture = Fixture::new();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let address = listener.local_addr().unwrap();
        let gate = Gate::new(limits.connections);
        runtime.spawn(serve(
            listener,
            fixture.receiver.clone(),
            limits,
            Arc::clone(&gate),
        ));
        Listening {
            address,
            gate,
            _runtime: runtime,
            fixture,
        }
    }

    fn connect(address: SocketAddr) -> std::net::TcpStream {
        let stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        stream
    }

    fn head(signature: &str, length: usize) -> String {
        format!(
            "POST /github/webhook HTTP/1.1\r\nhost: yuki\r\nx-hub-signature-256: {signature}\r\n\
             x-github-delivery: test-delivery\r\nx-github-event: ping\r\ncontent-length: {length}\r\n\r\n"
        )
    }

    /// The status line of the answer; nothing if the connection closed without one.
    fn answer(mut stream: std::net::TcpStream) -> String {
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer.lines().next().unwrap_or_default().to_owned()
    }

    fn deliver(address: SocketAddr) -> String {
        let mut stream = connect(address);
        stream
            .write_all(head(&sign(PAYLOAD), PAYLOAD.len()).as_bytes())
            .unwrap();
        stream.write_all(PAYLOAD).unwrap();
        answer(stream)
    }

    /// How many of these connections, set not to block, the listener still holds.
    fn still_open(streams: &[std::net::TcpStream]) -> usize {
        streams
            .iter()
            .filter(|stream| {
                let mut stream: &std::net::TcpStream = stream;
                let waiting = stream.read(&mut [0]);
                waiting.is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
            })
            .count()
    }

    #[test]
    fn unfinished_request_head_is_closed_at_its_deadline() {
        let listening = listen(Limits {
            connections: 8,
            head: Duration::from_millis(300),
            connection: Duration::from_secs(30),
        });
        for sent in [
            b"".as_slice(),
            b"POST /github/webhook HTTP/1.1\r\nhost: yuki\r\n",
        ] {
            let started = std::time::Instant::now();
            let mut stream = connect(listening.address);
            stream.write_all(sent).unwrap();
            assert_eq!(answer(stream), "");
            let elapsed = started.elapsed();
            assert!(
                elapsed >= Duration::from_millis(250) && elapsed < Duration::from_secs(20),
                "{elapsed:?}"
            );
        }
        assert_eq!(
            listening
                .fixture
                .receiver
                .rejections
                .load(Ordering::Relaxed),
            2
        );
        // A request sent whole is answered, and so is the health check the
        // monitor and deployer make.
        assert_eq!(deliver(listening.address), "HTTP/1.1 202 Accepted");
        let mut health = connect(listening.address);
        health
            .write_all(b"GET /healthz HTTP/1.1\r\nhost: yuki\r\n\r\n")
            .unwrap();
        assert_eq!(answer(health), "HTTP/1.1 200 OK");
    }

    #[test]
    fn idle_connections_give_way_to_new_ones_within_the_limit() {
        // Deadlines far beyond the test: only giving way can make room here.
        let limits = Limits {
            connections: 16,
            head: Duration::from_secs(600),
            connection: Duration::from_secs(600),
        };
        let listening = listen(limits);
        let idle: Vec<_> = (0..200).map(|_| connect(listening.address)).collect();
        for _ in 0..40 {
            assert_eq!(deliver(listening.address), "HTTP/1.1 202 Accepted");
        }
        // Of the idle connections the listener still holds no more than it has
        // places; it closed the others, longest waiting first, without an answer.
        for stream in &idle {
            stream.set_nonblocking(true).unwrap();
        }
        let settled = std::time::Instant::now() + Duration::from_secs(20);
        while still_open(&idle) > limits.connections && std::time::Instant::now() < settled {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(still_open(&idle) <= limits.connections);
    }

    #[test]
    fn verified_delivery_keeps_its_place_while_it_waits_for_a_worker() {
        let limits = Limits {
            connections: 4,
            head: Duration::from_secs(600),
            connection: Duration::from_secs(600),
        };
        let listening = listen(limits);
        let workers = Arc::clone(&listening.fixture.receiver.in_flight)
            .try_acquire_many_owned(MAX_IN_FLIGHT as u32)
            .unwrap();
        let mut delivery = connect(listening.address);
        delivery
            .write_all(head(&sign(PAYLOAD), PAYLOAD.len()).as_bytes())
            .unwrap();
        delivery.write_all(PAYLOAD).unwrap();
        // Once verified it is no longer among the connections that give way.
        let settled = std::time::Instant::now() + Duration::from_secs(20);
        while (listening.gate.places.available_permits() != limits.connections - 1
            || !listening.gate.unproven().is_empty())
            && std::time::Instant::now() < settled
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(listening.gate.unproven().is_empty());
        // Newcomers take the other places and push each other out of them.
        let idle: Vec<_> = (0..40).map(|_| connect(listening.address)).collect();
        for stream in &idle {
            stream.set_nonblocking(true).unwrap();
        }
        while still_open(&idle) >= limits.connections && std::time::Instant::now() < settled {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(still_open(&idle), limits.connections - 1);
        drop(workers);
        assert_eq!(answer(delivery), "HTTP/1.1 202 Accepted");
    }

    #[test]
    fn stalled_bodies_over_the_wire_never_refuse_a_signed_delivery() {
        let listening = listen(LIMITS);
        let unverified = format!("sha256={}", "0".repeat(64));
        // More senders than there are places, each with a whole head and no body.
        let stalled: Vec<_> = (0..MAX_CONNECTIONS + 8)
            .map(|_| {
                let mut stream = connect(listening.address);
                stream
                    .write_all(head(&unverified, 1000).as_bytes())
                    .unwrap();
                stream
            })
            .collect();
        for _ in 0..40 {
            assert_eq!(deliver(listening.address), "HTTP/1.1 202 Accepted");
        }
        // An oversized declaration is answered without waiting for its body.
        let mut oversized = connect(listening.address);
        oversized
            .write_all(head(&unverified, MAX_BODY + 1).as_bytes())
            .unwrap();
        assert_eq!(answer(oversized), "HTTP/1.1 413 Payload Too Large");
        drop(stalled);
    }
}
