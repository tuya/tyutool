//! B4 slice integration tests: run_auth authorization writes — same wire
//! contract as run_job (progress + terminal job_result, request_id keyed),
//! mutual exclusion with flash jobs (since B7 cycle 3 a global one: one
//! dangerous operation at a time), cancel support, and credential validation
//! (bad_request).
//!
//! The authorize execution surface is injected (fake backend); the real
//! tyutool-core `FlashMode::Authorize` path (`run_authorize`) is verified on a
//! physical board separately.
//!
//! Every `run_auth` / `run_job` here would trip the B7 confirmation gate, so
//! these servers run with the shared approving prompt: this file is about
//! orchestration, not about what the user answered (that is `local_auth.rs`).

mod common;

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
            occupied_by: None,
        }
    }
}

// ── Server / client helpers ──────────────────────────────────────────────────

async fn start_server(backend: Arc<dyn FlashBackend>) -> SocketAddr {
    let enumerator: PortEnumerator = Arc::new(Vec::new);
    let server = tyutool_bridge::bind(0)
        .await
        .expect("bind ephemeral port")
        .with_auth_prompt(common::approving() as Arc<dyn tyutool_bridge::AuthPrompt>);
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
            "uuidxxxxxxxxxxxxxxxx",
            "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
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
    assert_eq!(recorded[0].uuid, "uuidxxxxxxxxxxxxxxxx");
    assert_eq!(recorded[0].auth_key, "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
}

/// The web client deliberately omits `baud_rate` and relies on the protocol
/// default, so that default *is* the rate every browser-driven authorization
/// runs at. It must be the firmware console rate (115200 — the value every
/// entry of `src/features/firmware-flash/chip-manifests.ts` carries as
/// `defaultAuthBaudRate`, and the one the GUI batch pipeline, `tyutool-cli
/// authorize` and the direct-vendor web path all use), not a flash bootloader
/// rate.
#[tokio::test]
async fn an_omitted_baud_rate_authorizes_at_the_firmware_console_rate() {
    let (backend, auth_specs, finish) = FakeBackend::new();
    finish.store(true, Ordering::Relaxed);
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        serde_json::json!({
            "type": "run_auth",
            "request_id": "a-baud",
            "auth": {
                "port": "/dev/tty.fakeA",
                "chip_id": "t5ai",
                "uuid": "uuidxxxxxxxxxxxxxxxx",
                "auth_key": "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
            }
        }),
    )
    .await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], true, "{result}");

    let recorded = auth_specs.lock().expect("auth specs lock");
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].baud_rate, 115_200,
        "an omitted baud_rate must authorize at the console rate"
    );
}

#[tokio::test]
async fn flash_and_auth_are_mutually_exclusive() {
    // The exclusion used to be per port (one holder per port, B3); since B7
    // cycle 3 it is global — one dangerous operation at a time in the whole
    // process — so the refusal code is `execution_busy` and holds for any port,
    // not just the one in flight. Still bidirectional, which is what this test
    // is here for.
    //
    // Direction 1: a flash job is in flight, run_auth answers busy.
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
            "uuidxxxxxxxxxxxxxxxx",
            "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ),
    )
    .await;
    let busy = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(busy["request_id"], "a-002", "{busy}");
    assert_eq!(busy["ok"], false, "{busy}");
    assert_eq!(busy["error_code"], "execution_busy", "{busy}");
    finish.store(true, Ordering::Relaxed);
    let done = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(done["request_id"], "j-001", "{done}");

    // Direction 2 (fresh server): an auth job is in flight, run_job answers busy.
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        run_auth_frame(
            "a-101",
            "/dev/tty.fakeA",
            "uuidxxxxxxxxxxxxxxxx",
            "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ),
    )
    .await;
    next_frame_of_type(&mut ws, "progress").await;
    send_json(&mut ws, run_job_frame("j-102", "/dev/tty.fakeA")).await;
    let busy = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(busy["request_id"], "j-102", "{busy}");
    assert_eq!(busy["ok"], false, "{busy}");
    assert_eq!(busy["error_code"], "execution_busy", "{busy}");
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
            "uuidxxxxxxxxxxxxxxxx",
            "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
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
            "uuidxxxxxxxxxxxxxxxx",
            "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
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
        run_auth_frame(
            "a-301",
            "/dev/tty.fakeA",
            "",
            "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ),
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
                "uuid": "uuidxxxxxxxxxxxxxxxx",
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
            "uuidxxxxxxxxxxxxxxxx",
            "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
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

/// Credential *lengths* are a firmware protocol constraint, not something the
/// device gets to answer: `tuya_authorize.c` accepts a UUID of 16 or 20
/// characters and an AuthKey of exactly 32, and rejects anything else after the
/// write command has already been sent. Sending it anyway spends a real
/// authorization code and reports back as `auth_failed` — "we touched your
/// device and it went wrong" — for a request that was malformed before it left
/// the bridge. So the length check happens here, on the same footing as the
/// empty-credential check: `bad_request`, no port claim, no byte on the wire.
#[tokio::test]
async fn malformed_credential_lengths_answer_bad_request_without_touching_the_device() {
    let (backend, auth_specs, finish) = FakeBackend::new();
    finish.store(true, Ordering::Relaxed);
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    // 19-character UUID: one short of the firmware's 20, well past its 16.
    send_json(
        &mut ws,
        run_auth_frame("a-401", "/dev/tty.fakeA", &"u".repeat(19), &"k".repeat(32)),
    )
    .await;
    let rejected = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(rejected["request_id"], "a-401", "{rejected}");
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(rejected["error_code"], "bad_request", "{rejected}");

    // 31-character AuthKey with a well-formed UUID.
    send_json(
        &mut ws,
        run_auth_frame("a-402", "/dev/tty.fakeA", &"u".repeat(20), &"k".repeat(31)),
    )
    .await;
    let rejected = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(rejected["request_id"], "a-402", "{rejected}");
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(rejected["error_code"], "bad_request", "{rejected}");

    assert!(
        auth_specs.lock().expect("auth specs lock").is_empty(),
        "a malformed credential must never reach the execution layer — \
         the device may not be written to at all"
    );
}
