//! B5 slice integration tests: serial monitor sessions — open/chunk_batch/
//! close/disconnected over the shared port arbiter, plus the post-flash
//! handoff window that reserves a just-flashed port for the flashing
//! connection's monitor.
//!
//! The session source is injected (fake backend records configs and exposes
//! the chunk/disconnect callbacks for the test to drive); the real
//! tyutool-core SerialDebugSession path is verified on a physical board.
//!
//! The handoff-window tests drive `run_job`, which would trip the B7
//! confirmation gate, so these servers run with the shared approving prompt
//! (the monitor itself is ungated — see `local_auth.rs`).

mod common;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tyutool_bridge::{
    DebugSessionHandle, FlashBackend, FlashJobSpec, JobError, PortEnumerator, PortProbe,
};
use tyutool_core::{DebugChunk, DebugConfig, Direction};

const ALLOWED_DEV_ORIGIN: &str = "http://localhost:3000";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ── Fake backend with a drivable session source ──────────────────────────────

type ChunkFn = Box<dyn Fn(DebugChunk) + Send + Sync>;
type DisconnectFn = Box<dyn Fn(String) + Send + Sync>;

#[derive(Default)]
struct SessionHooks {
    on_chunk: Option<ChunkFn>,
    on_disconnect: Option<DisconnectFn>,
}

struct FakeBackend {
    cfgs: Arc<Mutex<Vec<DebugConfig>>>,
    hooks: Arc<Mutex<SessionHooks>>,
    closed: Arc<AtomicUsize>,
    gate: Gate,
    /// Shared with the handles this backend hands out, so a test can hold the
    /// session teardown still.
    close_gate: Arc<Gate>,
    /// When set, every open fails with it *after* passing the gate — a real
    /// backend does the same when the device vanished before it got there.
    open_failure: Option<JobError>,
}

/// Holds a backend call still so a test can act while it is in flight, and can
/// pin the outcome instead of racing for it. Used at the two blocking points
/// that own a race window: `open_debug_session` (the Opening/Aborting window)
/// and the session handle's `close` (the disconnect-teardown window, between
/// the session leaving the slot and `serial_debug_disconnected` reaching the
/// wire). Arms the *first* call only; later ones find `None` and pass straight
/// through.
#[derive(Default)]
struct Gate {
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    /// Announces that the guarded call has entered the gate.
    started: Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>,
}

impl Gate {
    /// Build an armed gate plus its test-side control.
    fn armed() -> (Self, GateControl) {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (started_tx, started_rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                release: Mutex::new(Some(release_rx)),
                started: Mutex::new(Some(started_tx)),
            },
            GateControl {
                release: release_tx,
                started: started_rx,
            },
        )
    }

    /// Backend side: announce arrival, then block until released.
    fn wait(&self) {
        if let Some(started) = self.started.lock().expect("gate lock").take() {
            let _ = started.send(());
        }
        if let Some(release) = self.release.lock().expect("gate lock").take() {
            let _ = release.recv();
        }
    }
}

/// Test-side end of a [`Gate`].
struct GateControl {
    release: std::sync::mpsc::Sender<()>,
    started: tokio::sync::mpsc::UnboundedReceiver<()>,
}

impl GateControl {
    /// Wait until the guarded call is inside the gate.
    async fn wait_started(&mut self) {
        tokio::time::timeout(Duration::from_secs(2), self.started.recv())
            .await
            .expect("gated call must start within 2s")
            .expect("gate sender must stay alive");
    }

    /// Let the blocked call finish.
    fn release(&self) {
        self.release
            .send(())
            .expect("gated call must still be waiting");
    }
}

impl FakeBackend {
    #[allow(clippy::type_complexity)]
    fn new() -> (
        Arc<Self>,
        Arc<Mutex<Vec<DebugConfig>>>,
        Arc<Mutex<SessionHooks>>,
        Arc<AtomicUsize>,
    ) {
        let cfgs = Arc::new(Mutex::new(Vec::new()));
        let hooks = Arc::new(Mutex::new(SessionHooks::default()));
        let closed = Arc::new(AtomicUsize::new(0));
        let backend = Arc::new(Self {
            cfgs: Arc::clone(&cfgs),
            hooks: Arc::clone(&hooks),
            closed: Arc::clone(&closed),
            gate: Gate::default(),
            close_gate: Arc::new(Gate::default()),
            open_failure: None,
        });
        (backend, cfgs, hooks, closed)
    }

    /// Like [`FakeBackend::new`], but the first open blocks until the returned
    /// control releases it.
    #[allow(clippy::type_complexity)]
    fn gated() -> (
        Arc<Self>,
        Arc<Mutex<SessionHooks>>,
        Arc<AtomicUsize>,
        GateControl,
    ) {
        Self::gated_with(None)
    }

    /// Fake whose first session teardown (`DebugSessionHandle::close`) blocks
    /// until released — the window a disconnect's terminal frame lives in.
    fn close_gated() -> (Arc<Self>, Arc<Mutex<SessionHooks>>, GateControl) {
        let (gate, control) = Gate::armed();
        let hooks = Arc::new(Mutex::new(SessionHooks::default()));
        let backend = Arc::new(Self {
            cfgs: Arc::new(Mutex::new(Vec::new())),
            hooks: Arc::clone(&hooks),
            closed: Arc::new(AtomicUsize::new(0)),
            gate: Gate::default(),
            close_gate: Arc::new(gate),
            open_failure: None,
        });
        (backend, hooks, control)
    }

    /// As [`FakeBackend::gated`], but the gated open ends in failure.
    #[allow(clippy::type_complexity)]
    fn gated_failing(
        error_code: &str,
        message: &str,
    ) -> (
        Arc<Self>,
        Arc<Mutex<SessionHooks>>,
        Arc<AtomicUsize>,
        GateControl,
    ) {
        Self::gated_with(Some(JobError {
            error_code: error_code.to_string(),
            message: message.to_string(),
        }))
    }

    #[allow(clippy::type_complexity)]
    fn gated_with(
        open_failure: Option<JobError>,
    ) -> (
        Arc<Self>,
        Arc<Mutex<SessionHooks>>,
        Arc<AtomicUsize>,
        GateControl,
    ) {
        let (gate, control) = Gate::armed();
        let hooks = Arc::new(Mutex::new(SessionHooks::default()));
        let closed = Arc::new(AtomicUsize::new(0));
        let backend = Arc::new(Self {
            cfgs: Arc::new(Mutex::new(Vec::new())),
            hooks: Arc::clone(&hooks),
            closed: Arc::clone(&closed),
            gate,
            close_gate: Arc::new(Gate::default()),
            open_failure,
        });
        (backend, hooks, closed, control)
    }
}

struct FakeHandle {
    closed: Arc<AtomicUsize>,
    gate: Arc<Gate>,
}

impl DebugSessionHandle for FakeHandle {
    fn close(self: Box<Self>) {
        // Runs on the blocking pool, so holding still here is safe — and it is
        // what lets a test occupy the teardown window deterministically.
        self.gate.wait();
        self.closed.fetch_add(1, Ordering::Relaxed);
    }
}

impl FlashBackend for FakeBackend {
    fn run_job(
        &self,
        _spec: FlashJobSpec,
        _cancel: Arc<std::sync::atomic::AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        // Immediate success: enough to trigger the post-flash handoff window.
        progress(serde_json::json!({ "phase": "write", "percent": 100 }));
        Ok(())
    }

    fn open_debug_session(
        &self,
        cfg: DebugConfig,
        on_chunk: ChunkFn,
        on_disconnect: DisconnectFn,
    ) -> Result<Box<dyn DebugSessionHandle>, JobError> {
        self.cfgs.lock().expect("cfgs lock").push(cfg);
        {
            // Scoped: the gate below blocks, and a test firing a device
            // disconnect needs this lock while the open is still in flight.
            let mut hooks = self.hooks.lock().expect("hooks lock");
            hooks.on_chunk = Some(on_chunk);
            hooks.on_disconnect = Some(on_disconnect);
        }
        self.gate.wait();
        if let Some(failure) = &self.open_failure {
            return Err(JobError {
                error_code: failure.error_code.clone(),
                message: failure.message.clone(),
            });
        }
        Ok(Box::new(FakeHandle {
            closed: Arc::clone(&self.closed),
            gate: Arc::clone(&self.close_gate),
        }))
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

/// As [`start_server`], with the post-flash handoff window shrunk so a test can
/// observe it expiring without sitting out the production three seconds.
async fn start_server_with_handoff_window(
    backend: Arc<dyn FlashBackend>,
    window: Duration,
) -> SocketAddr {
    let enumerator: PortEnumerator = Arc::new(Vec::new);
    let server = tyutool_bridge::bind(0)
        .await
        .expect("bind ephemeral port")
        .with_handoff_window(window)
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
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send frame");
}

fn open_frame(port: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "serial_debug_open",
        "cfg": {
            "port": port,
            "baud_rate": 115200,
            "data_bits": 8,
            "stop_bits": 1,
            "parity": "none"
        }
    })
}

/// Next `serial_debug_*` frame, skipping unrelated traffic (`ports` snapshots
/// arrive whenever a claim changes). Unlike [`next_frame_of_type`] this does not
/// skip *other* session frames, so a test can assert **which** session frame
/// came first.
async fn next_serial_debug_frame(ws: &mut Ws) -> serde_json::Value {
    for _ in 0..20 {
        let v = next_json(ws, "serial_debug frame").await;
        if v["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("serial_debug"))
        {
            return v;
        }
    }
    panic!("no serial_debug frame within 20 frames");
}

/// Assert no `serial_debug_*` frame arrives for `window`; `what` names the
/// expectation being pinned (silence while an open is in flight, or the absence
/// of a second terminal frame).
async fn assert_no_serial_debug_frames(ws: &mut Ws, window: Duration, what: &str) {
    let deadline = tokio::time::Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return;
        }
        let polled = match tokio::time::timeout(remaining, ws.next()).await {
            // Quiet for the whole window: what we want.
            Err(_) => return,
            Ok(polled) => polled,
        };
        let msg = match polled {
            Some(Ok(msg)) => msg,
            Some(Err(e)) => panic!("ws read must succeed: {e}"),
            None => return,
        };
        let text = msg
            .into_text()
            .unwrap_or_else(|e| panic!("must be a text frame: {e}"));
        let v: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("must be JSON ({e}): {text}"));
        let kind = v["type"].as_str().unwrap_or_default();
        assert!(
            !kind.starts_with("serial_debug"),
            "{what}, but this session frame arrived: {v}"
        );
    }
}

fn emit_chunk(hooks: &Arc<Mutex<SessionHooks>>, chunk: DebugChunk) {
    let hooks = hooks.lock().expect("hooks lock");
    let on_chunk = hooks.on_chunk.as_ref().expect("session must be open");
    on_chunk(chunk);
}

fn emit_disconnect(hooks: &Arc<Mutex<SessionHooks>>, reason: &str) {
    let hooks = hooks.lock().expect("hooks lock");
    let on_disconnect = hooks.on_disconnect.as_ref().expect("session must be open");
    on_disconnect(reason.to_string());
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn open_streams_chunk_batches_and_close_releases_the_port() {
    let (backend, cfgs, hooks, closed) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    let opened = next_frame_of_type(&mut ws, "serial_debug_opened").await;
    assert_eq!(opened["type"], "serial_debug_opened", "{opened}");

    {
        let recorded = cfgs.lock().expect("cfgs lock");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].port, "/dev/tty.fakeA");
        assert_eq!(recorded[0].baud_rate, 115_200);
    }

    // "Ym9vdA==" is base64 for "boot".
    emit_chunk(
        &hooks,
        DebugChunk {
            direction: Direction::Rx,
            ts_ms: 42,
            bytes: b"boot".to_vec(),
        },
    );
    let batch = next_frame_of_type(&mut ws, "serial_debug_chunk_batch").await;
    let chunks = batch["chunks"].as_array().expect("chunks array");
    assert_eq!(chunks.len(), 1, "{batch}");
    assert_eq!(chunks[0]["ts_ms"], 42, "{batch}");
    assert_eq!(chunks[0]["direction"], "rx", "{batch}");
    assert_eq!(chunks[0]["bytes_b64"], "Ym9vdA==", "{batch}");

    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    next_frame_of_type(&mut ws, "serial_debug_closed").await;
    assert_eq!(closed.load(Ordering::Relaxed), 1, "handle must be closed");

    // Close released the port: a fresh open succeeds.
    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws, "serial_debug_opened").await;
}

#[tokio::test]
async fn open_conflicts_with_sessions_and_jobs_on_the_same_port() {
    let (backend, _cfgs, _hooks, _closed) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws_a = connect_ready(&addr).await;
    let mut ws_b = connect_ready(&addr).await;

    send_json(&mut ws_a, open_frame("/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws_a, "serial_debug_opened").await;

    // Another connection cannot open a monitor on the held port...
    send_json(&mut ws_b, open_frame("/dev/tty.fakeA")).await;
    let refused = next_frame_of_type(&mut ws_b, "serial_debug_open_failed").await;
    assert_eq!(refused["error_code"], "port_busy", "{refused}");

    // ...nor flash it (same arbiter).
    send_json(
        &mut ws_b,
        serde_json::json!({
            "type": "run_job",
            "request_id": "j-901",
            "job": { "chip_id": "t5ai", "port": "/dev/tty.fakeA", "baud_rate": 2000000 },
            "file_content": "aGVsbG8="
        }),
    )
    .await;
    let busy = next_frame_of_type(&mut ws_b, "job_result").await;
    assert_eq!(busy["request_id"], "j-901", "{busy}");
    assert_eq!(busy["error_code"], "port_busy", "{busy}");
}

#[tokio::test]
async fn device_removal_pushes_disconnected_and_releases_the_port() {
    let (backend, _cfgs, hooks, _closed) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws, "serial_debug_opened").await;

    emit_disconnect(&hooks, "device_removed");
    let disconnected = next_frame_of_type(&mut ws, "serial_debug_disconnected").await;
    assert_eq!(disconnected["reason"], "device_removed", "{disconnected}");

    // The dead session no longer holds the port.
    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws, "serial_debug_opened").await;
}

#[tokio::test]
async fn post_flash_handoff_reserves_the_port_for_the_flashing_connection() {
    let (backend, _cfgs, _hooks, _closed) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws_flasher = connect_ready(&addr).await;
    let mut ws_other = connect_ready(&addr).await;

    send_json(
        &mut ws_flasher,
        serde_json::json!({
            "type": "run_job",
            "request_id": "j-501",
            "job": { "chip_id": "t5ai", "port": "/dev/tty.fakeA", "baud_rate": 2000000 },
            "file_content": "aGVsbG8="
        }),
    )
    .await;
    let done = next_frame_of_type(&mut ws_flasher, "job_result").await;
    assert_eq!(done["ok"], true, "{done}");

    // Within the handoff window, another connection is still refused...
    send_json(&mut ws_other, open_frame("/dev/tty.fakeA")).await;
    let refused = next_frame_of_type(&mut ws_other, "serial_debug_open_failed").await;
    assert_eq!(
        refused["error_code"], "port_busy",
        "handoff window must shield the port from other connections: {refused}"
    );

    // ...while the flashing connection takes over atomically.
    send_json(&mut ws_flasher, open_frame("/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws_flasher, "serial_debug_opened").await;
}

#[tokio::test]
async fn the_handoff_reservation_expires_and_frees_the_port_for_everyone() {
    let (backend, _cfgs, _hooks, _closed) = FakeBackend::new();
    let window = Duration::from_millis(150);
    let addr = start_server_with_handoff_window(backend, window).await;
    let mut ws_flasher = connect_ready(&addr).await;
    let mut ws_other = connect_ready(&addr).await;

    send_json(
        &mut ws_flasher,
        serde_json::json!({
            "type": "run_job",
            "request_id": "j-502",
            "job": { "chip_id": "t5ai", "port": "/dev/tty.fakeA", "baud_rate": 2000000 },
            "file_content": "aGVsbG8="
        }),
    )
    .await;
    let done = next_frame_of_type(&mut ws_flasher, "job_result").await;
    assert_eq!(done["ok"], true, "{done}");

    // Inside the window the other connection is shut out (the B5 guarantee)...
    send_json(&mut ws_other, open_frame("/dev/tty.fakeA")).await;
    let refused = next_frame_of_type(&mut ws_other, "serial_debug_open_failed").await;
    assert_eq!(refused["error_code"], "port_busy", "{refused}");

    // ...but the reservation is a short grace period, not a lock: once it has
    // lapsed the port belongs to whoever asks, with no timer having to fire.
    tokio::time::sleep(window * 2).await;
    send_json(&mut ws_other, open_frame("/dev/tty.fakeA")).await;
    let opened = next_serial_debug_frame(&mut ws_other).await;
    assert_eq!(
        opened["type"], "serial_debug_opened",
        "an expired handoff reservation must not keep shielding the port: {opened}"
    );
}

// ── Races against an in-flight open (Opening / Aborting) ─────────────────────
//
// The wire contract is that a terminal session frame means "the port is free
// again": a client may reopen the moment it sees one. When the close (or the
// device disconnect) arrives while the backend open is still blocked, the
// teardown can only happen once that open lands — so the terminal frame has to
// wait for it, and only one of the two racing parties may emit it.

/// How long a test listens for a stray second terminal frame.
const QUIET_WINDOW: Duration = Duration::from_millis(200);
/// Long enough for the server to have processed a frame the test just sent.
const SETTLE: Duration = Duration::from_millis(100);

#[tokio::test]
async fn close_during_an_in_flight_open_answers_once_after_the_port_is_released() {
    let (backend, _hooks, _closed, mut gate) = FakeBackend::gated();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    gate.wait_started().await;

    // The close lands while the backend open is still inside the gate.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    tokio::time::sleep(SETTLE).await;

    // Nothing may be answered yet: the session that is about to materialize
    // still has to be shut down and its port released, and the frame promises
    // both already happened.
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "the close must stay unanswered while the open is in flight",
    )
    .await;

    gate.release();
    let terminal = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        terminal["type"], "serial_debug_closed",
        "the close must be answered exactly once, with serial_debug_closed: {terminal}"
    );

    // The contract: serial_debug_closed means the port is usable again.
    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    let reopened = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        reopened["type"], "serial_debug_opened",
        "reopening right after serial_debug_closed must succeed: {reopened}"
    );
}

#[tokio::test]
async fn device_disconnect_during_an_in_flight_open_answers_once_after_the_port_is_released() {
    let (backend, hooks, _closed, mut gate) = FakeBackend::gated();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    gate.wait_started().await;

    // The board goes away before its own open finished.
    emit_disconnect(&hooks, "device_removed");
    tokio::time::sleep(SETTLE).await;

    // Same rule as the close path: the report waits for the in-flight open,
    // because only then can the port actually be released.
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "the disconnect must stay unreported while the open is in flight",
    )
    .await;

    gate.release();
    let terminal = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        terminal["type"], "serial_debug_disconnected",
        "the disconnect must be reported exactly once: {terminal}"
    );
    assert_eq!(terminal["reason"], "device_removed", "{terminal}");

    // Same contract as close: the port is free once the frame is out.
    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    let reopened = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        reopened["type"], "serial_debug_opened",
        "reopening right after serial_debug_disconnected must succeed: {reopened}"
    );
}

#[tokio::test]
async fn a_close_arriving_while_the_disconnect_teardown_runs_is_swallowed() {
    let (backend, hooks, mut teardown) = FakeBackend::close_gated();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws, "serial_debug_opened").await;

    // The device disconnects. Its teardown takes the session out of the slot
    // and then blocks in the handle close — precisely the window between "the
    // session is gone from the slot" and "serial_debug_disconnected is on the
    // wire". Held open by a gate rather than by luck, so this pins the window
    // instead of racing for it.
    emit_disconnect(&hooks, "device_removed");
    teardown.wait_started().await;

    // The user hits stop inside that window.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    tokio::time::sleep(SETTLE).await;
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "a close inside the disconnect teardown window must not be answered",
    )
    .await;

    teardown.release();

    let terminal = next_serial_debug_frame(&mut ws).await;
    assert_eq!(terminal["type"], "serial_debug_disconnected", "{terminal}");
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "the swallowed close must not add a second terminal frame",
    )
    .await;

    // One-shot here as well: the next close is a fresh teardown and is answered.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    let idempotent = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        idempotent["type"], "serial_debug_closed",
        "a close with nothing open must still be answered: {idempotent}"
    );
}

#[tokio::test]
async fn a_disconnect_answers_the_close_that_was_already_on_its_way() {
    let (backend, _cfgs, hooks, _closed) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    next_frame_of_type(&mut ws, "serial_debug_opened").await;

    // The cable comes out just as the user hits stop. The disconnect takes the
    // live session and reports it...
    emit_disconnect(&hooks, "device_removed");
    let terminal = next_serial_debug_frame(&mut ws).await;
    assert_eq!(terminal["type"], "serial_debug_disconnected", "{terminal}");
    assert_eq!(terminal["reason"], "device_removed", "{terminal}");

    // ...so the close already in flight must not report the same session a
    // second time. Sequenced rather than raced on purpose: the outcome must not
    // depend on which of the two happens to reach the slot first, and a test
    // that relies on winning a race would flake.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "the disconnect already answered for this session",
    )
    .await;

    // That swallow is one-shot: a later close is a fresh teardown with nothing
    // open, and the documented idempotence still answers it — the web client's
    // teardown path stays branch-free.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    let idempotent = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        idempotent["type"], "serial_debug_closed",
        "a close with nothing open must still be answered: {idempotent}"
    );
}

#[tokio::test]
async fn a_disconnect_reported_by_a_failed_open_also_answers_the_pending_close() {
    let (backend, hooks, _closed, mut gate) =
        FakeBackend::gated_failing("open_failed", "device went away");
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    gate.wait_started().await;

    // The device vanishes while the open is still in flight, and the open then
    // fails because of it: the client gets its open answer plus the disconnect.
    emit_disconnect(&hooks, "device_removed");
    tokio::time::sleep(SETTLE).await;
    gate.release();

    let failure = next_serial_debug_frame(&mut ws).await;
    assert_eq!(failure["type"], "serial_debug_open_failed", "{failure}");
    let terminal = next_serial_debug_frame(&mut ws).await;
    assert_eq!(terminal["type"], "serial_debug_disconnected", "{terminal}");

    // Same rule as for an already-open session: the close already on its way
    // must not report this session a second time.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "the disconnect already answered for this session",
    )
    .await;

    // ...and the swallow is one-shot here too.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    let idempotent = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        idempotent["type"], "serial_debug_closed",
        "a close with nothing open must still be answered: {idempotent}"
    );
}

/// Characterization test: the behaviour already exists (`finish_failed_open`
/// hands the owed frame back and the open task sends it after the release). It
/// is pinned here because it is the one path that would silently hang a web
/// client — the client is awaiting `serial_debug_closed`, and the open it was
/// closing ended in failure rather than in a session.
#[tokio::test]
async fn a_close_waiting_on_a_failed_open_is_answered_after_the_failure() {
    let (backend, _hooks, _closed, mut gate) =
        FakeBackend::gated_failing("open_failed", "device went away");
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    gate.wait_started().await;

    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    tokio::time::sleep(SETTLE).await;
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "the close must stay unanswered while the open is in flight",
    )
    .await;

    gate.release();

    // The open request is answered first, ...
    let failure = next_serial_debug_frame(&mut ws).await;
    assert_eq!(failure["type"], "serial_debug_open_failed", "{failure}");
    assert_eq!(failure["error_code"], "open_failed", "{failure}");

    // ... and the close still gets the frame it has been waiting for.
    let terminal = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        terminal["type"], "serial_debug_closed",
        "a close waiting on an open that failed must still be answered: {terminal}"
    );

    // The failed open released its claim: this open reaches the backend (and
    // fails there for its own reason) instead of being refused as port_busy.
    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    let reopened = next_serial_debug_frame(&mut ws).await;
    assert_eq!(reopened["type"], "serial_debug_open_failed", "{reopened}");
    assert_eq!(
        reopened["error_code"], "open_failed",
        "port_busy here would mean the failed open never released the port: {reopened}"
    );
}

#[tokio::test]
async fn close_racing_a_disconnect_during_an_in_flight_open_yields_exactly_one_terminal_frame() {
    let (backend, hooks, _closed, mut gate) = FakeBackend::gated();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    gate.wait_started().await;

    // User closes the tab's monitor at the same moment the board is unplugged,
    // both while the open is still in flight. Whichever wins, the client must
    // see one terminal frame — not one per party.
    send_json(&mut ws, serde_json::json!({ "type": "serial_debug_close" })).await;
    emit_disconnect(&hooks, "device_removed");
    tokio::time::sleep(SETTLE).await;

    // Both parties stay silent while the open is in flight.
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "neither party may answer while the open is in flight",
    )
    .await;

    gate.release();
    let terminal = next_serial_debug_frame(&mut ws).await;
    let kind = terminal["type"].as_str().unwrap_or_default();
    assert!(
        kind == "serial_debug_closed" || kind == "serial_debug_disconnected",
        "the winner must answer with its own terminal frame: {terminal}"
    );
    assert_no_serial_debug_frames(
        &mut ws,
        QUIET_WINDOW,
        "the loser must not add a second terminal frame",
    )
    .await;

    send_json(&mut ws, open_frame("/dev/tty.fakeA")).await;
    let reopened = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        reopened["type"], "serial_debug_opened",
        "the port must be free once the single terminal frame is out: {reopened}"
    );
}

// ── B12: an unparsable open must answer too ──────────────────────────────────

#[tokio::test]
async fn an_unparsable_serial_debug_open_answers_open_failed_instead_of_silence() {
    let (backend, cfgs, _hooks, _closed) = FakeBackend::new();
    let addr = start_server(backend).await;
    let mut ws = connect_ready(&addr).await;

    // `baud_rate` as a string is the same client-side mistake class that broke
    // `run_job` in the pre environment. These frames carry no `request_id`, so
    // the answer is the session's own terminal frame.
    send_json(
        &mut ws,
        serde_json::json!({
            "type": "serial_debug_open",
            "cfg": { "port": "/dev/tty.fakeA", "baud_rate": "115200" }
        }),
    )
    .await;

    let failed = next_serial_debug_frame(&mut ws).await;
    assert_eq!(
        failed["type"], "serial_debug_open_failed",
        "an undecodable open must fail fast, not be dropped: {failed}"
    );
    assert_eq!(failed["error_code"], "bad_request", "{failed}");
    assert!(
        cfgs.lock().expect("cfgs lock").is_empty(),
        "a frame that failed to decode must never reach the session backend"
    );
}
