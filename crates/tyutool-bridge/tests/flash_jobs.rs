//! B3 slice integration tests: run_job orchestration with progress streaming,
//! port arbitration (immediate busy on conflict, no queuing), cancel releasing
//! the held port, check_port semantics, and arbitration truth surfacing as
//! `busy` in the `ports` frames.
//!
//! The flash execution surface is injected (fake backend) so orchestration is
//! exercised without real hardware; the real tyutool-core path is compiled in
//! production code and verified on a physical board separately.

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
    EnumeratedPort, FlashBackend, FlashJobSpec, JobError, PortEnumerator, PortProbe,
};

const ALLOWED_DEV_ORIGIN: &str = "http://localhost:3000";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ── Fake backend ─────────────────────────────────────────────────────────────

/// Records incoming specs; blocks each job until `finish` is set or the job is
/// cancelled, so tests control exactly when a port is held and released.
struct FakeBackend {
    specs: Arc<Mutex<Vec<FlashJobSpec>>>,
    finish: Arc<AtomicBool>,
}

impl FakeBackend {
    fn new() -> (Arc<Self>, Arc<Mutex<Vec<FlashJobSpec>>>, Arc<AtomicBool>) {
        let specs = Arc::new(Mutex::new(Vec::new()));
        let finish = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(Self {
            specs: Arc::clone(&specs),
            finish: Arc::clone(&finish),
        });
        (backend, specs, finish)
    }
}

impl FlashBackend for FakeBackend {
    fn run_job(
        &self,
        spec: FlashJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        self.specs.lock().expect("specs lock").push(spec);
        progress(serde_json::json!({
            "phase": "write",
            "percent": 63,
            "log": "write 0x0032c000 ok"
        }));
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

    fn probe_port(&self, _port: &str) -> PortProbe {
        // Fake OS probe: everything the bridge does not hold looks free.
        PortProbe {
            available: true,
            reason: None,
        }
    }
}

// ── Server / client helpers ──────────────────────────────────────────────────

async fn start_server(
    backend: Arc<dyn FlashBackend>,
    initial_ports: Vec<EnumeratedPort>,
) -> SocketAddr {
    let enumerator: PortEnumerator = Arc::new(move || initial_ports.clone());
    let server = tyutool_bridge::bind(0).await.expect("bind ephemeral port");
    let addr = server.local_addr().expect("local addr");
    tokio::spawn(server.run_with(enumerator, Duration::from_millis(20), backend));
    addr
}

/// Connect with an allowlisted Origin and swallow hello + the initial ports
/// frame so tests start from a quiet stream.
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

/// Next frame of the given type, skipping unrelated pushes (e.g. `ports`
/// updates interleaving with job traffic).
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

fn run_job_frame(request_id: &str, port: &str, firmware_b64: &str) -> serde_json::Value {
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
        "file_content": firmware_b64
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn run_job_streams_progress_then_job_result() {
    let (backend, specs, finish) = FakeBackend::new();
    finish.store(true, Ordering::Relaxed); // job completes right after progress
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    // "aGVsbG8=" is base64 for "hello".
    send_json(
        &mut ws,
        run_job_frame("j-001", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;

    let progress = next_frame_of_type(&mut ws, "progress").await;
    assert_eq!(progress["request_id"], "j-001", "{progress}");
    assert_eq!(progress["payload"]["phase"], "write", "{progress}");
    assert_eq!(progress["payload"]["percent"], 63, "{progress}");

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["request_id"], "j-001", "{result}");
    assert_eq!(result["ok"], true, "{result}");
    assert!(
        result["elapsed_ms"].is_u64(),
        "elapsed_ms must be a u64: {result}"
    );

    let recorded = specs.lock().expect("specs lock");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].chip_id, "t5ai");
    assert_eq!(recorded[0].port, "/dev/tty.fakeA");
    assert_eq!(recorded[0].baud_rate, 2_000_000);
    assert_eq!(
        recorded[0].firmware, b"hello",
        "firmware must be base64-decoded before reaching the backend"
    );
}

#[tokio::test]
async fn second_job_on_held_port_answers_busy_immediately() {
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        run_job_frame("j-001", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    // Progress proves the first job claimed the port and is in flight.
    next_frame_of_type(&mut ws, "progress").await;

    send_json(
        &mut ws,
        run_job_frame("j-002", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    let busy = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(busy["request_id"], "j-002", "{busy}");
    assert_eq!(busy["ok"], false, "{busy}");
    assert_eq!(
        busy["error_code"], "port_busy",
        "conflict must answer busy immediately, no queuing: {busy}"
    );

    // The first job is unaffected and still finishes fine.
    finish.store(true, Ordering::Relaxed);
    let done = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(done["request_id"], "j-001", "{done}");
    assert_eq!(done["ok"], true, "{done}");
}

#[tokio::test]
async fn cancel_fails_the_job_and_releases_the_port() {
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        run_job_frame("j-101", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    next_frame_of_type(&mut ws, "progress").await;

    send_json(
        &mut ws,
        serde_json::json!({ "type": "cancel", "request_id": "j-101" }),
    )
    .await;
    let cancelled = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(cancelled["request_id"], "j-101", "{cancelled}");
    assert_eq!(cancelled["ok"], false, "{cancelled}");
    assert_eq!(cancelled["error_code"], "cancelled", "{cancelled}");

    // Port must be claimable again after the cancel released it.
    finish.store(true, Ordering::Relaxed);
    send_json(
        &mut ws,
        run_job_frame("j-102", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    let rerun = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(rerun["request_id"], "j-102", "{rerun}");
    assert_eq!(rerun["ok"], true, "cancel must release the port: {rerun}");
}

#[tokio::test]
async fn check_port_reports_bridge_held_and_free_ports() {
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    send_json(
        &mut ws,
        run_job_frame("j-201", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    next_frame_of_type(&mut ws, "progress").await;

    send_json(
        &mut ws,
        serde_json::json!({ "type": "check_port", "port": "/dev/tty.fakeA" }),
    )
    .await;
    let held = next_frame_of_type(&mut ws, "check_port_result").await;
    assert_eq!(held["port"], "/dev/tty.fakeA", "{held}");
    assert_eq!(held["available"], false, "{held}");
    assert_eq!(held["reason"], "occupied_by_bridge_job", "{held}");

    send_json(
        &mut ws,
        serde_json::json!({ "type": "check_port", "port": "/dev/tty.fakeB" }),
    )
    .await;
    let free = next_frame_of_type(&mut ws, "check_port_result").await;
    assert_eq!(free["port"], "/dev/tty.fakeB", "{free}");
    assert_eq!(free["available"], true, "{free}");
    assert!(
        free.get("reason").is_none_or(serde_json::Value::is_null),
        "free port carries no reason: {free}"
    );

    finish.store(true, Ordering::Relaxed);
}

#[tokio::test]
async fn ports_frames_carry_arbitration_busy_truth() {
    let (backend, _specs, finish) = FakeBackend::new();
    let enumerated = vec![EnumeratedPort {
        path: "/dev/tty.fakeA".to_string(),
        vid: Some(0x1A86),
        pid: Some(0x55D2),
        vendor: Some("WCH".to_string()),
        busy: false,
    }];
    let addr = start_server(backend, enumerated).await;

    let mut ws = {
        // connect_ready already checked the initial frame is a ports frame;
        // re-assert its busy truth here.
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
        next_json(&mut ws, "hello").await;
        let initial = next_json(&mut ws, "initial ports").await;
        assert_eq!(initial["type"], "ports", "{initial}");
        assert_eq!(
            initial["ports"][0]["busy"], false,
            "no job in flight yet: {initial}"
        );
        ws
    };

    send_json(
        &mut ws,
        run_job_frame("j-301", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    next_frame_of_type(&mut ws, "progress").await;

    // The next diff-driven ports push must flip busy to true for the held port.
    let busy_frame = wait_ports_where(&mut ws, true).await;
    assert_eq!(busy_frame["ports"][0]["port"], "/dev/tty.fakeA");

    // Job completes: the following push flips busy back to false.
    finish.store(true, Ordering::Relaxed);
    next_frame_of_type(&mut ws, "job_result").await;
    wait_ports_where(&mut ws, false).await;
}

/// Wait for a `ports` frame whose first entry has the wanted busy value,
/// skipping job traffic and stale ports frames.
async fn wait_ports_where(ws: &mut Ws, busy: bool) -> serde_json::Value {
    for _ in 0..20 {
        let v = next_json(ws, "ports push").await;
        if v["type"] == "ports" && v["ports"][0]["busy"] == busy {
            return v;
        }
    }
    panic!("no ports frame with busy={busy} within 20 frames");
}

#[tokio::test]
async fn two_ports_flash_concurrently_and_finish_independently() {
    let (backend, specs, finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, run_job_frame("j-A", "/dev/tty.fakeA", "aGVsbG8=")).await;
    send_json(&mut ws, run_job_frame("j-B", "/dev/tty.fakeB", "aGVsbG8=")).await;

    // Both jobs must be in flight at the same time (progress from each).
    let mut in_flight = std::collections::HashSet::new();
    while in_flight.len() < 2 {
        let v = next_frame_of_type(&mut ws, "progress").await;
        in_flight.insert(v["request_id"].as_str().expect("request_id").to_string());
    }
    assert!(
        in_flight.contains("j-A") && in_flight.contains("j-B"),
        "{in_flight:?}"
    );
    assert_eq!(specs.lock().expect("specs lock").len(), 2);

    finish.store(true, Ordering::Relaxed);
    let mut done = std::collections::HashSet::new();
    while done.len() < 2 {
        let v = next_frame_of_type(&mut ws, "job_result").await;
        assert_eq!(v["ok"], true, "{v}");
        done.insert(v["request_id"].as_str().expect("request_id").to_string());
    }
    assert!(done.contains("j-A") && done.contains("j-B"), "{done:?}");
}

#[tokio::test]
async fn connections_have_isolated_request_id_namespaces() {
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;

    // Two connections reuse the same request_id on different ports: both jobs
    // must run (per-connection namespace, not a process-global one).
    let mut ws_a = connect_ready(&addr).await;
    let mut ws_b = connect_ready(&addr).await;
    send_json(
        &mut ws_a,
        run_job_frame("j-1", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    next_frame_of_type(&mut ws_a, "progress").await;
    send_json(
        &mut ws_b,
        run_job_frame("j-1", "/dev/tty.fakeB", "aGVsbG8="),
    )
    .await;
    let progress = next_frame_of_type(&mut ws_b, "progress").await;
    assert_eq!(
        progress["request_id"], "j-1",
        "same request_id on another connection must not collide: {progress}"
    );

    // Closing connection A (cancels its own jobs) must not touch B's job.
    drop(ws_a);
    tokio::time::sleep(Duration::from_millis(50)).await;
    finish.store(true, Ordering::Relaxed);
    let done = next_frame_of_type(&mut ws_b, "job_result").await;
    assert_eq!(done["request_id"], "j-1", "{done}");
    assert_eq!(
        done["ok"], true,
        "closing another connection must not cancel this job: {done}"
    );

    // And connection A's port was released by its disconnect cleanup: B can
    // claim it right away.
    send_json(
        &mut ws_b,
        run_job_frame("j-2", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    let reclaim = next_frame_of_type(&mut ws_b, "job_result").await;
    assert_eq!(reclaim["request_id"], "j-2", "{reclaim}");
    assert_eq!(
        reclaim["ok"], true,
        "disconnect cleanup must release the dead connection's port: {reclaim}"
    );
}
