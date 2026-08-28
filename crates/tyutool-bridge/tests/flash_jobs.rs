//! B3 slice integration tests: run_job orchestration with progress streaming,
//! port arbitration (immediate busy on conflict, no queuing), cancel releasing
//! the held port, check_port semantics, and arbitration truth surfacing as
//! `busy` in the `ports` frames.
//!
//! The flash execution surface is injected (fake backend) so orchestration is
//! exercised without real hardware; the real tyutool-core path is compiled in
//! production code and verified on a physical board separately.
//!
//! Every `run_job` here would trip the B7 confirmation gate, so these servers
//! run with the shared approving prompt: this file is about orchestration, not
//! about what the user answered (that is `local_auth.rs`). B7 cycle 3 also made
//! dangerous operations globally exclusive, so a conflicting `run_job` now
//! answers `execution_busy` before port arbitration is consulted.

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
            occupied_by: None,
        }
    }
}

// ── Server / client helpers ──────────────────────────────────────────────────

async fn start_server(
    backend: Arc<dyn FlashBackend>,
    initial_ports: Vec<EnumeratedPort>,
) -> SocketAddr {
    let enumerator: PortEnumerator = Arc::new(move || initial_ports.clone());
    let server = tyutool_bridge::bind(0)
        .await
        .expect("bind ephemeral port")
        .with_auth_prompt(common::approving() as Arc<dyn tyutool_bridge::AuthPrompt>);
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
    ws.send(Message::Text(value.to_string().into()))
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
async fn second_job_while_one_runs_answers_busy_immediately() {
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
    // Since B7 cycle 3 the single-active-execution rule refuses a second
    // dangerous operation before the port table is even consulted, so this
    // conflict answers `execution_busy` rather than `port_busy` — the
    // "immediately, no queuing" contract is the same. `port_busy` for a
    // `run_job` is still reachable (and covered in `serial_debug.rs`) when the
    // port is held by a serial monitor session or another connection's handoff
    // reservation.
    assert_eq!(
        busy["error_code"], "execution_busy",
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
        serial_number: None,
        usb_interface: None,
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
async fn two_ports_flash_one_after_the_other_and_finish_independently() {
    // B3 let these two run at the same time (one holder per *port*); B7 cycle 3
    // deliberately took that away — at most one dangerous operation runs in the
    // whole process, another port included (see `local_auth.rs`). What still has
    // to hold is that each port's job is independent: the second one runs on its
    // own port and reports its own terminal frame once the first is done.
    let (backend, specs, finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, run_job_frame("j-A", "/dev/tty.fakeA", "aGVsbG8=")).await;
    let progress = next_frame_of_type(&mut ws, "progress").await;
    assert_eq!(progress["request_id"], "j-A", "{progress}");

    // While j-A holds the execution right, the other port is refused.
    send_json(&mut ws, run_job_frame("j-B", "/dev/tty.fakeB", "aGVsbG8=")).await;
    let refused = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(refused["request_id"], "j-B", "{refused}");
    assert_eq!(refused["error_code"], "execution_busy", "{refused}");

    finish.store(true, Ordering::Relaxed);
    let first = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(first["request_id"], "j-A", "{first}");
    assert_eq!(first["ok"], true, "{first}");

    // Retried after the first one finished: the execution right is free again.
    send_json(&mut ws, run_job_frame("j-B", "/dev/tty.fakeB", "aGVsbG8=")).await;
    let second = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(second["request_id"], "j-B", "{second}");
    assert_eq!(second["ok"], true, "{second}");

    let recorded = specs.lock().expect("specs lock");
    let ports: Vec<&str> = recorded.iter().map(|spec| spec.port.as_str()).collect();
    assert_eq!(
        ports,
        vec!["/dev/tty.fakeA", "/dev/tty.fakeB"],
        "each job must have reached the backend on its own port"
    );
}

#[tokio::test]
async fn a_closed_connection_releases_its_port_and_the_execution_right() {
    let (backend, _specs, finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;

    // B3 proved the per-connection `request_id` namespace by running two jobs at
    // once from two connections; B7 cycle 3 made that impossible (one dangerous
    // operation process-wide), so the namespace now only lives inside the
    // arbiter's job key and is no longer observable on the wire. What is
    // observable — and what matters more — is that a tab closing mid-flash frees
    // *both* the port and the execution right, or one abandoned tab would leave
    // the helper dead for everyone. The same `request_id` is reused across the
    // two connections here so the namespace still gets exercised.
    let mut ws_a = connect_ready(&addr).await;
    let mut ws_b = connect_ready(&addr).await;
    send_json(
        &mut ws_a,
        run_job_frame("j-1", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    next_frame_of_type(&mut ws_a, "progress").await;

    // While A is flashing, B is refused (nothing sticky about it, see below).
    send_json(
        &mut ws_b,
        run_job_frame("j-1", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    let refused = next_frame_of_type(&mut ws_b, "job_result").await;
    assert_eq!(refused["error_code"], "execution_busy", "{refused}");

    // A goes away mid-job: its job is cancelled, its port released, and the
    // execution right handed back.
    drop(ws_a);
    tokio::time::sleep(Duration::from_millis(50)).await;
    finish.store(true, Ordering::Relaxed);

    send_json(
        &mut ws_b,
        run_job_frame("j-1", "/dev/tty.fakeA", "aGVsbG8="),
    )
    .await;
    let reclaim = next_frame_of_type(&mut ws_b, "job_result").await;
    assert_eq!(reclaim["request_id"], "j-1", "{reclaim}");
    assert_eq!(
        reclaim["ok"], true,
        "disconnect cleanup must release the dead connection's port and \
         execution right: {reclaim}"
    );
}

// ── B12: unparsable frames must answer, not hang the client ──────────────────

#[tokio::test]
async fn run_job_missing_baud_rate_answers_bad_request_instead_of_silence() {
    let (backend, specs, finish) = FakeBackend::new();
    // Let the fake job complete immediately: if the frame ever were accepted,
    // this test must fail on the assertions below rather than hang waiting for
    // a job that never finishes.
    finish.store(true, Ordering::Relaxed);
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    // The exact frame from the pre-environment incident: the web client omitted
    // `baud_rate`, which `run_job` requires (unlike run_auth / serial_debug_open,
    // whose baud rate defaults). The bridge used to log a warning and drop the
    // frame, so the page sat on "等待确认…0%" forever.
    send_json(
        &mut ws,
        serde_json::json!({
            "type": "run_job",
            "request_id": "j-777",
            "job": {
                "chip_id": "t5ai",
                "port": "/dev/tty.fakeA",
                "mode": "write",
                "start_addr": 0
            },
            "file_content": "aGVsbG8="
        }),
    )
    .await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["request_id"], "j-777", "{result}");
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(
        result["error_code"], "bad_request",
        "an undecodable request must fail fast, not be dropped: {result}"
    );
    let message = result["message"].as_str().unwrap_or_else(|| {
        panic!("bad_request must carry a message naming the offending field: {result}")
    });
    assert!(
        message.contains("baud_rate"),
        "the message must name the missing field so the client can fix it: {message}"
    );

    assert!(
        specs.lock().expect("specs lock").is_empty(),
        "a frame that failed to decode must never reach the flash backend"
    );
}

#[tokio::test]
async fn bad_request_message_never_echoes_the_frame_contents() {
    let (backend, _specs, _finish) = FakeBackend::new();
    let addr = start_server(backend, vec![]).await;
    let mut ws = connect_ready(&addr).await;

    // A double `JSON.stringify` on the client side turns `job` into a string.
    // serde's own error text quotes the whole offending value back, and that
    // text travels to the client *and* into the log file — where a real frame's
    // base64 firmware, device uuid and auth key would land with it.
    const SECRET: &str = "c2VjcmV0LWZpcm13YXJlLXV1aWQtYXV0aGtleQ";
    send_json(
        &mut ws,
        serde_json::json!({
            "type": "run_job",
            "request_id": "j-778",
            "job": format!("{{\"chip_id\":\"t5ai\",\"auth_key\":\"{SECRET}\"}}"),
            "file_content": SECRET
        }),
    )
    .await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["request_id"], "j-778", "{result}");
    assert_eq!(result["error_code"], "bad_request", "{result}");
    let message = result["message"]
        .as_str()
        .unwrap_or_else(|| panic!("bad_request must carry a message: {result}"));
    assert!(
        !message.contains(SECRET),
        "the message must describe the shape of the failure, never echo frame \
         contents (firmware / uuid / auth_key travel in these frames): {message}"
    );
}
