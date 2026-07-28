//! B4 slice integration tests: run_auth authorization writes — same wire
//! contract as run_job (progress + terminal job_result, request_id keyed),
//! mutual exclusion with flash jobs on the same port through the shared
//! arbiter, cancel support, and credential validation (bad_request).
//!
//! The authorize execution surface is injected (fake backend); the real
//! tyutool-core `run_batch_auth_slot` path is verified on a physical board
//! separately.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tyutool_bridge::{
    AuthJobSpec, FlashBackend, FlashJobSpec, JobError, PortEnumerator, PortProbe,
};

const ALLOWED_DEV_ORIGIN: &str = "http://localhost:3000";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ── Fake backend (flash + auth) ──────────────────────────────────────────────

/// Records auth specs; both job kinds block until `finish` is set or the job
/// is cancelled, so tests control when a port is held and released.
struct FakeBackend {
    auth_specs: Arc<Mutex<Vec<AuthJobSpec>>>,
    finish: Arc<AtomicBool>,
}

impl FakeBackend {
    fn new() -> (Arc<Self>, Arc<Mutex<Vec<AuthJobSpec>>>, Arc<AtomicBool>) {
        let auth_specs = Arc::new(Mutex::new(Vec::new()));
        let finish = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(Self {
            auth_specs: Arc::clone(&auth_specs),
            finish: Arc::clone(&finish),
        });
        (backend, auth_specs, finish)
    }

    fn block_until_released(&self, cancel: &AtomicBool) -> Result<(), JobError> {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(JobError {
                    error_code: "cancelled".to_string(),
                    message: "cancelled by user".to_string(),
                });
            }
            if self.finish.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl FlashBackend for FakeBackend {
    fn run_job(
        &self,
        _spec: FlashJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        progress(serde_json::json!({ "phase": "write", "percent": 10 }));
        self.block_until_released(&cancel)
    }

    fn run_auth(
        &self,
        spec: AuthJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        self.auth_specs.lock().expect("auth specs lock").push(spec);
        progress(serde_json::json!({ "step": "writing_auth" }));
        self.block_until_released(&cancel)
    }

    fn probe_port(&self, _port: &str) -> PortProbe {
        PortProbe {
            available: true,
            reason: None,
        }
    }
}

// ── Server / client helpers ──────────────────────────────────────────────────

async fn start_server(backend: Arc<dyn FlashBackend>) -> SocketAddr {
    let enumerator: PortEnumerator = Arc::new(Vec::new);
    let server = tyutool_bridge::bind(0).await.expect("bind ephemeral port");
    let addr = server.local_addr().expect("local addr");
    tokio::spawn(server.run_with(enumerator, Duration::from_millis(20), backend));
    addr
}

async fn connect_ready(addr: &SocketAddr) -> Ws {
    let mut request = format!("ws://{addr}/")
        .into_client_request()
        .expect("build client request");
    request.headers_mut().insert(
        "Origin",
        HeaderValue::from_str(ALLOWED_DEV_ORIGIN).expect("valid origin header"),
    );
    let (mut ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("whitelisted origin must connect");
    let hello = next_json(&mut ws, "hello").await;
    assert_eq!(hello["type"], "hello", "{hello}");
    let ports = next_json(&mut ws, "initial ports").await;
    assert_eq!(ports["type"], "ports", "{ports}");
    ws
}

async fn next_json(ws: &mut Ws, what: &str) -> serde_json::Value {
    let polled = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .unwrap_or_else(|_| panic!("{what}: frame must arrive within 2s"));
    // `match` instead of `unwrap_or_else` chaining: a closure whose inferred
    // return type is the full Result<Message, tungstenite::Error> would trip
    // clippy::result_large_err.
    let msg = match polled {
        Some(Ok(msg)) => msg,
        Some(Err(e)) => panic!("{what}: ws read must succeed: {e}"),
        None => panic!("{what}: stream must not end"),
    };
    let text = msg
        .into_text()
        .unwrap_or_else(|e| panic!("{what}: must be a text frame: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{what}: must be JSON ({e}): {text}"))
}

async fn next_frame_of_type(ws: &mut Ws, kind: &str) -> serde_json::Value {
    for _ in 0..20 {
        let v = next_json(ws, kind).await;
        if v["type"] == kind {
            return v;
        }
    }
    panic!("no {kind} frame within 20 frames");
}

async fn send_json(ws: &mut Ws, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string()))
        .await
        .expect("send frame");
}

fn run_auth_frame(request_id: &str, port: &str, uuid: &str, auth_key: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "run_auth",
        "request_id": request_id,
        "auth": {
            "port": port,
            "chip_id": "t5ai",
            "uuid": uuid,
            "auth_key": auth_key,
            "baud_rate": 921600
        }
    })
}

fn run_job_frame(request_id: &str, port: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "run_job",
        "request_id": request_id,
        "job": {
            "chip_id": "t5ai",
            "port": port,
            "baud_rate": 2000000,
            "mode": "write",
            "start_addr": 0
        },
        "file_content": "aGVsbG8="
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn run_auth_streams_progress_then_job_result() {
    let (backend, auth_specs, finish) = FakeBackend::new();
    finish.store(true, Ordering::Relaxed); // completes right after progress
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        run_auth_frame(
            "a-001",
            "/dev/tty.fakeA",
            "uuidxxxxxxxx",
            "keyxxxxxxxxxxxxxxxx",
        ),
    )
    .await;

    let progress = next_frame_of_type(&mut ws, "progress").await;
    assert_eq!(progress["request_id"], "a-001", "{progress}");
    assert_eq!(progress["payload"]["step"], "writing_auth", "{progress}");

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["request_id"], "a-001", "{result}");
    assert_eq!(result["ok"], true, "{result}");
    assert!(
        result["elapsed_ms"].is_u64(),
        "elapsed_ms must be a u64: {result}"
    );

    let recorded = auth_specs.lock().expect("auth specs lock");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].chip_id, "t5ai");
    assert_eq!(recorded[0].port, "/dev/tty.fakeA");
    assert_eq!(recorded[0].baud_rate, 921_600);
    assert_eq!(recorded[0].uuid, "uuidxxxxxxxx");
    assert_eq!(recorded[0].auth_key, "keyxxxxxxxxxxxxxxxx");
}

#[tokio::test]
async fn flash_and_auth_are_mutually_exclusive_on_the_same_port() {
    // Direction 1: a flash job holds the port, run_auth answers busy.
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, run_job_frame("j-001", "/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws, "progress").await;
    send_json(
        &mut ws,
        run_auth_frame(
            "a-002",
            "/dev/tty.fakeA",
            "uuidxxxxxxxx",
            "keyxxxxxxxxxxxxxxxx",
        ),
    )
    .await;
    let busy = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(busy["request_id"], "a-002", "{busy}");
    assert_eq!(busy["ok"], false, "{busy}");
    assert_eq!(busy["error_code"], "port_busy", "{busy}");
    finish.store(true, Ordering::Relaxed);
    let done = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(done["request_id"], "j-001", "{done}");

    // Direction 2 (fresh server): an auth job holds the port, run_job answers busy.
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        run_auth_frame(
            "a-101",
            "/dev/tty.fakeA",
            "uuidxxxxxxxx",
            "keyxxxxxxxxxxxxxxxx",
        ),
    )
    .await;
    next_frame_of_type(&mut ws, "progress").await;
    send_json(&mut ws, run_job_frame("j-102", "/dev/tty.fakeA")).await;
    let busy = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(busy["request_id"], "j-102", "{busy}");
    assert_eq!(busy["ok"], false, "{busy}");
    assert_eq!(busy["error_code"], "port_busy", "{busy}");
    finish.store(true, Ordering::Relaxed);
    let done = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(done["request_id"], "a-101", "{done}");
    assert_eq!(done["ok"], true, "{done}");
}

#[tokio::test]
async fn cancel_fails_the_auth_and_releases_the_port() {
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        run_auth_frame(
            "a-201",
            "/dev/tty.fakeA",
            "uuidxxxxxxxx",
            "keyxxxxxxxxxxxxxxxx",
        ),
    )
    .await;
    next_frame_of_type(&mut ws, "progress").await;

    send_json(
        &mut ws,
        serde_json::json!({ "type": "cancel", "request_id": "a-201" }),
    )
    .await;
    let cancelled = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(cancelled["request_id"], "a-201", "{cancelled}");
    assert_eq!(cancelled["ok"], false, "{cancelled}");
    assert_eq!(cancelled["error_code"], "cancelled", "{cancelled}");

    // Port must be claimable again after the cancel released it.
    finish.store(true, Ordering::Relaxed);
    send_json(
        &mut ws,
        run_auth_frame(
            "a-202",
            "/dev/tty.fakeA",
            "uuidxxxxxxxx",
            "keyxxxxxxxxxxxxxxxx",
        ),
    )
    .await;
    let rerun = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(rerun["request_id"], "a-202", "{rerun}");
    assert_eq!(rerun["ok"], true, "cancel must release the port: {rerun}");
}

#[tokio::test]
async fn missing_or_empty_credentials_answer_bad_request_without_claiming() {
    let (backend, auth_specs, finish) = FakeBackend::new();
    finish.store(true, Ordering::Relaxed);
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    // Empty uuid.
    send_json(
        &mut ws,
        run_auth_frame("a-301", "/dev/tty.fakeA", "", "keyxxxxxxxxxxxxxxxx"),
    )
    .await;
    let rejected = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(rejected["request_id"], "a-301", "{rejected}");
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(rejected["error_code"], "bad_request", "{rejected}");

    // auth_key field entirely absent.
    send_json(
        &mut ws,
        serde_json::json!({
            "type": "run_auth",
            "request_id": "a-302",
            "auth": {
                "port": "/dev/tty.fakeA",
                "chip_id": "t5ai",
                "uuid": "uuidxxxxxxxx",
                "baud_rate": 921600
            }
        }),
    )
    .await;
    let rejected = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(rejected["request_id"], "a-302", "{rejected}");
    assert_eq!(rejected["error_code"], "bad_request", "{rejected}");

    // Neither rejection reached the backend or leaked a port claim: a valid
    // run_auth on the same port goes straight through.
    send_json(
        &mut ws,
        run_auth_frame(
            "a-303",
            "/dev/tty.fakeA",
            "uuidxxxxxxxx",
            "keyxxxxxxxxxxxxxxxx",
        ),
    )
    .await;
    let accepted = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(accepted["request_id"], "a-303", "{accepted}");
    assert_eq!(accepted["ok"], true, "{accepted}");
    assert_eq!(
        auth_specs.lock().expect("auth specs lock").len(),
        1,
        "rejected requests must never reach the backend"
    );
}
