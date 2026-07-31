//! B7 slice integration tests: local-transport hardening.
//!
//! `Origin` alone protects nothing against a native local process (it is a
//! header the browser adds, not one a program has to tell the truth about), so
//! dangerous operations (`run_job` / `run_auth`) sit behind a human-in-the-loop
//! confirmation, and that one click is persisted as a token so the user is not
//! asked again. Everything low-risk (hello / ports / serial monitor) stays open
//! so "插线即就绪" survives.
//!
//! The confirmation UI and the token store are injected, so these tests state
//! what the user answered instead of popping a real dialog.

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
    AuditSink, AuthJobSpec, AuthPrompt, DangerousOp, DebugSessionHandle, EnumeratedPort,
    FileTokenStore, FlashBackend, FlashJobSpec, Grant, GrantPolicy, JobError, MemoryTokenStore,
    PortEnumerator, PortProbe, TokenStore,
};

/// Both are in the compile-time Origin allowlist.
const ORIGIN_A: &str = "http://localhost:3000";
const ORIGIN_B: &str = "http://127.0.0.1:3000";

const PORT_A: &str = "/dev/tty.fakeA";
/// base64 for "hello" — 5 decoded bytes, which the confirmation must report.
const FIRMWARE_B64: &str = "aGVsbG8=";
const FIRMWARE_BYTES: u64 = 5;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ── Fake backend ─────────────────────────────────────────────────────────────

/// Records everything it is handed, so a test can assert the device was never
/// touched. Jobs block until `finish` is set (or cancelled) unless the backend
/// was built with `finishing()`.
#[derive(Default)]
struct FakeBackend {
    flash_specs: Mutex<Vec<FlashJobSpec>>,
    auth_specs: Mutex<Vec<AuthJobSpec>>,
    opened: Mutex<Vec<String>>,
    finish: AtomicBool,
}

impl FakeBackend {
    /// Jobs return success immediately.
    fn finishing() -> Arc<Self> {
        let backend = Self::default();
        backend.finish.store(true, Ordering::Relaxed);
        Arc::new(backend)
    }

    /// Let a blocking job finish.
    fn release(&self) {
        self.finish.store(true, Ordering::Relaxed);
    }

    fn flash_specs(&self) -> Vec<FlashJobSpec> {
        self.flash_specs.lock().expect("flash specs lock").clone()
    }

    fn auth_specs(&self) -> Vec<AuthJobSpec> {
        self.auth_specs.lock().expect("auth specs lock").clone()
    }

    /// Block until released or cancelled — but never forever: a blocking task
    /// that outlives its runtime would hang the whole test binary at shutdown
    /// (dropping a tokio runtime waits for its blocking pool), turning any
    /// assertion failure into a timeout instead of a readable diff.
    fn wait_for_finish(&self, cancel: &Arc<AtomicBool>) -> Result<(), JobError> {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
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
            if std::time::Instant::now() >= deadline {
                return Err(JobError {
                    error_code: "internal".to_string(),
                    message: "fake backend was never released".to_string(),
                });
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

struct NoopSession;

impl DebugSessionHandle for NoopSession {
    fn close(self: Box<Self>) {}
}

impl FlashBackend for FakeBackend {
    fn run_job(
        &self,
        spec: FlashJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        self.flash_specs
            .lock()
            .expect("flash specs lock")
            .push(spec);
        progress(serde_json::json!({ "phase": "write", "percent": 1 }));
        self.wait_for_finish(&cancel)
    }

    fn run_auth(
        &self,
        spec: AuthJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        self.auth_specs.lock().expect("auth specs lock").push(spec);
        progress(serde_json::json!({ "step": "writing_auth" }));
        self.wait_for_finish(&cancel)
    }

    fn open_debug_session(
        &self,
        cfg: tyutool_core::DebugConfig,
        _on_chunk: Box<dyn Fn(tyutool_core::DebugChunk) + Send + Sync>,
        _on_disconnect: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn DebugSessionHandle>, JobError> {
        self.opened.lock().expect("opened lock").push(cfg.port);
        Ok(Box::new(NoopSession))
    }

    fn probe_port(&self, _port: &str) -> PortProbe {
        PortProbe {
            available: true,
            reason: None,
        }
    }
}

// ── Server / client helpers ──────────────────────────────────────────────────

/// One port on offer, so `ports` frames are non-trivial.
fn fake_enumerator() -> PortEnumerator {
    Arc::new(|| {
        vec![EnumeratedPort {
            path: PORT_A.to_string(),
            vid: Some(0x1A86),
            pid: Some(0x55D2),
            vendor: Some("WCH".to_string()),
            busy: false,
            serial_number: None,
            usb_interface: None,
        }]
    })
}

struct ServerBuilder {
    backend: Arc<dyn FlashBackend>,
    prompt: Arc<dyn AuthPrompt>,
    tokens: Arc<dyn TokenStore>,
    audit: Option<Arc<dyn AuditSink>>,
    confirm_timeout: Duration,
    grant_policy: Option<GrantPolicy>,
}

// `tokens` / `audit` are seams the later B7 cycles assert through (grant
// persistence, the stable audit line format); cycle 1 only needs them wired.
#[allow(dead_code)]
impl ServerBuilder {
    fn new(backend: Arc<dyn FlashBackend>, prompt: Arc<dyn AuthPrompt>) -> Self {
        Self {
            backend,
            prompt,
            tokens: Arc::new(MemoryTokenStore::default()),
            audit: None,
            confirm_timeout: Duration::from_secs(5),
            grant_policy: None,
        }
    }

    fn grant_policy(mut self, policy: GrantPolicy) -> Self {
        self.grant_policy = Some(policy);
        self
    }

    fn tokens(mut self, tokens: Arc<dyn TokenStore>) -> Self {
        self.tokens = tokens;
        self
    }

    fn audit(mut self, audit: Arc<dyn AuditSink>) -> Self {
        self.audit = Some(audit);
        self
    }

    fn confirm_timeout(mut self, timeout: Duration) -> Self {
        self.confirm_timeout = timeout;
        self
    }

    async fn start(self) -> SocketAddr {
        let mut server = tyutool_bridge::bind(0)
            .await
            .expect("bind ephemeral port")
            .with_auth_prompt(self.prompt)
            .with_token_store(self.tokens)
            .with_confirm_timeout(self.confirm_timeout);
        if let Some(audit) = self.audit {
            server = server.with_audit_sink(audit);
        }
        if let Some(policy) = self.grant_policy {
            server = server.with_grant_policy(policy);
        }
        let addr = server.local_addr().expect("local addr");
        tokio::spawn(server.run_with(fake_enumerator(), Duration::from_millis(20), self.backend));
        addr
    }
}

/// Connect with an allowlisted Origin, optionally presenting a previously
/// granted token, and swallow hello + the initial ports frame.
async fn connect(addr: &SocketAddr, origin: &str, token: Option<&str>) -> Ws {
    let mut ws = connect_raw(addr, origin, token).await;
    let hello = next_json(&mut ws, "hello").await;
    assert_eq!(hello["type"], "hello", "{hello}");
    let ports = next_json(&mut ws, "initial ports").await;
    assert_eq!(ports["type"], "ports", "{ports}");
    ws
}

async fn connect_raw(addr: &SocketAddr, origin: &str, token: Option<&str>) -> Ws {
    let url = match token {
        Some(token) => format!("ws://{addr}/?token={token}"),
        None => format!("ws://{addr}/"),
    };
    let mut request = url.into_client_request().expect("build client request");
    request.headers_mut().insert(
        "Origin",
        HeaderValue::from_str(origin).expect("valid origin header"),
    );
    let (ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("an allowlisted origin must connect, token or not");
    ws
}

async fn next_json(ws: &mut Ws, what: &str) -> serde_json::Value {
    let polled = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap_or_else(|_| panic!("{what}: frame must arrive within 3s"));
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

/// Next frame of the given type, skipping unrelated pushes (`ports` updates).
async fn next_frame_of_type(ws: &mut Ws, kind: &str) -> serde_json::Value {
    for _ in 0..20 {
        let v = next_json(ws, kind).await;
        if v["type"] == kind {
            return v;
        }
    }
    panic!("no {kind} frame within 20 frames");
}

/// Every frame up to and including the first one of `kind`, so a test can assert
/// what did *not* arrive along the way.
async fn frames_until(ws: &mut Ws, kind: &str) -> Vec<serde_json::Value> {
    let mut seen = Vec::new();
    for _ in 0..20 {
        let v = next_json(ws, kind).await;
        let done = v["type"] == kind;
        seen.push(v);
        if done {
            return seen;
        }
    }
    panic!("no {kind} frame within 20 frames, saw: {seen:?}");
}

fn frame_types(frames: &[serde_json::Value]) -> Vec<String> {
    frames
        .iter()
        .map(|f| f["type"].as_str().unwrap_or("?").to_string())
        .collect()
}

async fn send_json(ws: &mut Ws, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string()))
        .await
        .expect("send frame");
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
        "file_content": FIRMWARE_B64
    })
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

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn unauthorized_connection_still_receives_hello_and_the_device_list() {
    let addr = ServerBuilder::new(FakeBackend::finishing(), common::rejecting())
        .start()
        .await;

    // No token: the device list must arrive anyway, or "插线即就绪" is gone.
    let mut ws = connect_raw(&addr, ORIGIN_A, None).await;
    let hello = next_json(&mut ws, "hello").await;
    assert_eq!(hello["type"], "hello", "{hello}");
    let ports = next_json(&mut ws, "ports").await;
    assert_eq!(ports["type"], "ports", "{ports}");
    assert_eq!(ports["ports"][0]["port"], PORT_A, "{ports}");
}

#[tokio::test]
async fn unauthorized_connection_may_open_the_serial_monitor() {
    let addr = ServerBuilder::new(FakeBackend::finishing(), common::rejecting())
        .start()
        .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    // Read-only observation of a port the user already plugged in: low risk, so
    // it stays outside the confirmation gate.
    send_json(
        &mut ws,
        serde_json::json!({ "type": "serial_debug_open", "cfg": { "port": PORT_A } }),
    )
    .await;

    let opened = next_frame_of_type(&mut ws, "serial_debug_opened").await;
    assert_eq!(opened["type"], "serial_debug_opened", "{opened}");
}

#[tokio::test]
async fn unauthorized_run_job_asks_the_user_before_touching_the_device() {
    let backend = FakeBackend::finishing();
    let prompt = common::hanging();
    let addr = ServerBuilder::new(Arc::clone(&backend) as Arc<dyn FlashBackend>, {
        let prompt: Arc<dyn AuthPrompt> = Arc::clone(&prompt) as Arc<dyn AuthPrompt>;
        prompt
    })
    .confirm_timeout(Duration::from_millis(300))
    .start()
    .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;

    let asked = prompt.first_request(Duration::from_secs(2)).await;
    assert_eq!(asked.op, DangerousOp::Flash, "{asked:?}");
    assert_eq!(asked.origin, ORIGIN_A, "{asked:?}");
    assert_eq!(asked.chip_id, "t5ai", "{asked:?}");
    assert_eq!(asked.port, PORT_A, "{asked:?}");
    assert_eq!(asked.firmware_bytes, Some(FIRMWARE_BYTES), "{asked:?}");

    // Nothing may reach the device while the dialog is still up.
    assert!(
        backend.flash_specs().is_empty(),
        "the backend must not run before the user answered: {:?}",
        backend.flash_specs()
    );

    // A dialog nobody answers is a refusal, not an open door.
    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert!(
        backend.flash_specs().is_empty(),
        "a timed-out confirmation must not flash: {:?}",
        backend.flash_specs()
    );
}

#[tokio::test]
async fn refused_run_job_answers_user_rejected_and_leaves_the_port_free() {
    let backend = FakeBackend::finishing();
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        common::rejecting(),
    )
    .start()
    .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["request_id"], "j-1", "{result}");
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert!(
        backend.flash_specs().is_empty(),
        "backend must be untouched"
    );

    // A refusal must not leave the port claimed: the fake OS probe says free, so
    // "occupied_by_bridge_job" here would mean the gate leaked a claim.
    send_json(
        &mut ws,
        serde_json::json!({ "type": "check_port", "port": PORT_A }),
    )
    .await;
    let checked = next_frame_of_type(&mut ws, "check_port_result").await;
    assert_eq!(checked["available"], true, "{checked}");
}

#[tokio::test]
async fn approved_run_job_hands_back_a_token_and_runs() {
    let backend = FakeBackend::finishing();
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        common::approving(),
    )
    .start()
    .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;

    // The click is persisted as a token so the user is not asked again.
    let granted = next_frame_of_type(&mut ws, "auth_granted").await;
    let token = granted["token"].as_str().expect("token must be a string");
    assert!(
        token.len() >= 40,
        "32 CSPRNG bytes in base64url is 43 chars, got {} in {granted}",
        token.len()
    );
    assert!(
        token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "token must be base64url (URL-safe, no padding): {granted}"
    );

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(backend.flash_specs().len(), 1, "the job must have run");
}

#[tokio::test]
async fn refused_run_auth_never_reaches_the_device() {
    let backend = FakeBackend::finishing();
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        common::rejecting(),
    )
    .start()
    .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    // Overwriting an authorization code is irreversible, so it is gated exactly
    // like flashing.
    send_json(
        &mut ws,
        run_auth_frame(
            "a-1",
            PORT_A,
            "uuid-abcdef000000000",
            "key-0123456789000000000000000000",
        ),
    )
    .await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert!(
        backend.auth_specs().is_empty(),
        "no authorization write may reach the device: {:?}",
        backend.auth_specs()
    );
}

// ── Token reuse, origin binding, revocation (B7 cycle 2) ─────────────────────

#[tokio::test]
async fn a_granted_token_lets_a_later_connection_skip_the_confirmation() {
    let backend = FakeBackend::finishing();
    let prompt = common::approving();
    let tokens: Arc<dyn TokenStore> = Arc::new(MemoryTokenStore::default());
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens))
    .start()
    .await;

    // First connection: the user is asked, says yes, and gets the receipt.
    let mut first = connect(&addr, ORIGIN_A, None).await;
    send_json(&mut first, run_job_frame("j-1", PORT_A)).await;
    let granted = next_frame_of_type(&mut first, "auth_granted").await;
    let token = granted["token"]
        .as_str()
        .expect("token must be a string")
        .to_string();
    let done = next_frame_of_type(&mut first, "job_result").await;
    assert_eq!(done["ok"], true, "{done}");
    drop(first);

    // Second connection presents that receipt: asking again would defeat the
    // whole point of persisting the click.
    let mut second = connect(&addr, ORIGIN_A, Some(&token)).await;
    send_json(&mut second, run_job_frame("j-2", PORT_A)).await;
    let frames = frames_until(&mut second, "job_result").await;

    let last = frames.last().expect("job_result");
    assert_eq!(last["ok"], true, "{last}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "a pre-authorized connection must not raise a second dialog, saw {:?}",
        prompt.requests()
    );
    let types = frame_types(&frames);
    assert!(
        !types.iter().any(|t| t == "auth_granted"),
        "a connection that already holds a token must not be issued another: {types:?}"
    );
}

#[tokio::test]
async fn an_unknown_token_downgrades_to_unauthorized_instead_of_refusing_the_connection() {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .start()
    .await;

    // A stale token from a previous install must not lock the user out of the
    // device list — it downgrades the connection, it does not refuse it.
    let mut ws = connect(&addr, ORIGIN_A, Some("Ab3-xYz_not_a_real_token")).await;

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "an unknown token must fall back to asking the user"
    );
}

#[tokio::test]
async fn a_token_granted_to_one_origin_is_not_honoured_from_another() {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let tokens = Arc::new(MemoryTokenStore::default());
    tokens.insert(Grant {
        token: "granted-to-origin-a".to_string(),
        origin: ORIGIN_A.to_string(),
        granted_at_ms: 1_784_800_000_000,
    });
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .start()
    .await;

    // Control: from the origin it was granted to, the token works (the prompt
    // would refuse, so a success proves it was never consulted).
    let mut same = connect(&addr, ORIGIN_A, Some("granted-to-origin-a")).await;
    send_json(&mut same, run_job_frame("j-1", PORT_A)).await;
    let allowed = next_frame_of_type(&mut same, "job_result").await;
    assert_eq!(allowed["ok"], true, "{allowed}");
    assert!(
        prompt.requests().is_empty(),
        "a valid token must not raise a dialog"
    );
    drop(same);

    // Same token, different origin: the grant does not travel.
    let mut other = connect(&addr, ORIGIN_B, Some("granted-to-origin-a")).await;
    send_json(&mut other, run_job_frame("j-2", PORT_A)).await;
    let refused = next_frame_of_type(&mut other, "job_result").await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error_code"], "user_rejected", "{refused}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "a token from another origin must fall back to asking the user"
    );
}

#[tokio::test]
async fn revoking_all_grants_invalidates_an_issued_token() {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let tokens = Arc::new(MemoryTokenStore::default());
    tokens.insert(Grant {
        token: "still-valid-for-now".to_string(),
        origin: ORIGIN_A.to_string(),
        granted_at_ms: 1_784_800_000_000,
    });
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .start()
    .await;

    // "撤销所有授权" (tray menu) clears both the file and memory.
    tokens.revoke_all();

    let mut ws = connect(&addr, ORIGIN_A, Some("still-valid-for-now")).await;
    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "a revoked token must fall back to asking the user"
    );
}

// ── Persisted grants (B7 cycle 2) ────────────────────────────────────────────

/// Unique scratch path per test run; the store must create the parent itself.
fn scratch_store_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!("tyutool-bridge-test-{}-{tag}", std::process::id()))
        .join("grants.json")
}

#[test]
fn a_persisted_grant_survives_a_restart_and_is_cleared_by_revoking() {
    let path = scratch_store_path("persist");
    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));

    let store = FileTokenStore::open_at(&path).expect("open a fresh grant store");
    store.insert(Grant {
        token: "persisted-token".to_string(),
        origin: ORIGIN_A.to_string(),
        granted_at_ms: 1_784_800_000_000,
    });
    assert!(store.is_granted("persisted-token", ORIGIN_A));
    assert!(!store.is_granted("persisted-token", ORIGIN_B));
    drop(store);

    // A restarted helper must not ask the user again, or every reboot costs a
    // confirmation.
    let reopened = FileTokenStore::open_at(&path).expect("reopen the grant store");
    assert!(
        reopened.is_granted("persisted-token", ORIGIN_A),
        "the grant must survive the process that issued it"
    );

    reopened.revoke_all();
    assert!(!reopened.is_granted("persisted-token", ORIGIN_A));
    drop(reopened);
    let after_revoke = FileTokenStore::open_at(&path).expect("reopen after revoke");
    assert!(
        !after_revoke.is_granted("persisted-token", ORIGIN_A),
        "revoking must clear the file, not just the in-memory copy"
    );

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[cfg(unix)]
#[test]
fn the_grant_file_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let path = scratch_store_path("perms");
    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));

    let store = FileTokenStore::open_at(&path).expect("open a fresh grant store");
    store.insert(Grant {
        token: "secret-token".to_string(),
        origin: ORIGIN_A.to_string(),
        granted_at_ms: 1_784_800_000_000,
    });

    // A token is a standing "yes" from the user; another local account must not
    // be able to read it off the disk.
    let mode = std::fs::metadata(&path)
        .expect("the grant file must exist after an insert")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o600,
        "grants must be owner read/write only, got {mode:o}"
    );

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

// ── Single active execution + audit trail (B7 cycle 3) ───────────────────────

/// A grant seeded straight into the store, so a test can open an already
/// authorized connection without going through a dialog.
fn seeded_grant(token: &str) -> Arc<MemoryTokenStore> {
    let tokens = Arc::new(MemoryTokenStore::default());
    tokens.insert(Grant {
        token: token.to_string(),
        origin: ORIGIN_A.to_string(),
        granted_at_ms: 1_784_800_000_000,
    });
    tokens
}

#[tokio::test]
async fn only_one_dangerous_operation_runs_at_a_time_across_connections() {
    // Jobs block until released, so the first one is still running when the
    // second client asks.
    let backend = Arc::new(FakeBackend::default());
    let tokens = seeded_grant("shared-grant");
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        common::approving(),
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .start()
    .await;

    let mut first = connect(&addr, ORIGIN_A, Some("shared-grant")).await;
    send_json(&mut first, run_job_frame("j-1", PORT_A)).await;
    // Progress means the job is live on the device.
    let progress = next_frame_of_type(&mut first, "progress").await;
    assert_eq!(progress["request_id"], "j-1", "{progress}");

    // A second tab may connect and watch, but not drive a second flash — even on
    // a different port, which the per-port arbiter would happily allow.
    let mut second = connect(&addr, ORIGIN_A, Some("shared-grant")).await;
    send_json(&mut second, run_job_frame("j-2", "/dev/tty.fakeB")).await;
    let refused = next_frame_of_type(&mut second, "job_result").await;
    assert_eq!(refused["request_id"], "j-2", "{refused}");
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error_code"], "execution_busy", "{refused}");
    assert_eq!(
        backend.flash_specs().len(),
        1,
        "the second client must not have reached the device"
    );

    // Once the first one is done the execution right is free again: the rule is
    // "one at a time", not "first connection owns the helper".
    backend.release();
    let done = next_frame_of_type(&mut first, "job_result").await;
    assert_eq!(done["ok"], true, "{done}");

    send_json(&mut second, run_job_frame("j-3", "/dev/tty.fakeB")).await;
    let allowed = next_frame_of_type(&mut second, "job_result").await;
    assert_eq!(allowed["request_id"], "j-3", "{allowed}");
    assert_eq!(allowed["ok"], true, "{allowed}");
}

#[tokio::test]
async fn a_request_arriving_while_a_confirmation_is_pending_raises_no_second_dialog() {
    let backend = FakeBackend::finishing();
    let prompt = common::hanging();
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    // Long enough that the second frame is answered while the dialog is still up.
    .confirm_timeout(Duration::from_secs(30))
    .start()
    .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let first_dialog = prompt.first_request(Duration::from_secs(2)).await;
    assert_eq!(first_dialog.port, PORT_A, "{first_dialog:?}");

    // A client that retries (or a rogue process hammering the port) must not be
    // able to stack dialogs on the user.
    send_json(&mut ws, run_job_frame("j-2", PORT_A)).await;
    let refused = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(refused["request_id"], "j-2", "{refused}");
    assert_eq!(refused["error_code"], "execution_busy", "{refused}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "a pending confirmation must absorb further requests, saw {:?}",
        prompt.requests()
    );
}

#[tokio::test]
async fn the_audit_trail_records_the_operation_but_never_a_credential_or_a_full_token() {
    const SECRET_UUID: &str = "uuid-supersecret-9f1";
    const SECRET_KEY: &str = "authkey-supersecret-3c7000000000";

    let backend = FakeBackend::finishing();
    let audit = common::capturing_audit();
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        common::approving(),
    )
    .audit(Arc::clone(&audit) as Arc<dyn AuditSink>)
    .start()
    .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let granted = next_frame_of_type(&mut ws, "auth_granted").await;
    let token = granted["token"]
        .as_str()
        .expect("token must be a string")
        .to_string();
    let flashed = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(flashed["ok"], true, "{flashed}");

    send_json(
        &mut ws,
        run_auth_frame("a-1", PORT_A, SECRET_UUID, SECRET_KEY),
    )
    .await;
    let authorized = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(authorized["ok"], true, "{authorized}");

    let lines = audit.lines();
    let joined = lines.join("\n");

    // What an incident review needs: who asked, for what, on which device.
    assert!(
        lines.iter().any(|l| l.contains("op=flash")
            && l.contains(&format!("origin={ORIGIN_A}"))
            && l.contains("chip=t5ai")
            && l.contains(&format!("port={PORT_A}"))
            && l.contains(&format!("firmware_bytes={FIRMWARE_BYTES}"))
            && l.contains("decision=approved")),
        "no complete flash confirmation line in the audit trail:\n{joined}"
    );
    // The second operation rode on the grant the first one earned, so no dialog
    // was shown — but a dangerous operation must leave a trace either way, or the
    // trail only ever shows the first flash of a session.
    assert!(
        lines.iter().any(|l| l.contains("op=authorize")
            && l.contains(&format!("port={PORT_A}"))
            && l.contains("decision=preauthorized")),
        "no authorization line in the audit trail:\n{joined}"
    );

    // What must never be there.
    assert!(
        !joined.contains(SECRET_UUID),
        "the audit trail leaked the authorization uuid:\n{joined}"
    );
    assert!(
        !joined.contains(SECRET_KEY),
        "the audit trail leaked the authorization key:\n{joined}"
    );
    assert!(
        !joined.contains(&token),
        "the audit trail leaked the full grant token:\n{joined}"
    );
    // The redacted fingerprint is still expected, so the trail can be correlated.
    assert!(
        joined.contains("(len="),
        "the grant should still be traceable through a redacted fingerprint:\n{joined}"
    );
}

// ── Revocation reaches live connections (B7 cycle 4) ─────────────────────────

#[tokio::test]
async fn revoking_deauthorizes_live_connections_and_pushes_auth_revoked() {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let tokens = seeded_grant("grant-until-revoked");
    // `authority()` borrows, so the server needs no `mut` here.
    let server = tyutool_bridge::bind(0)
        .await
        .expect("bind ephemeral port")
        .with_auth_prompt(Arc::clone(&prompt) as Arc<dyn AuthPrompt>)
        .with_token_store(Arc::clone(&tokens) as Arc<dyn TokenStore>);
    // The tray's "撤销所有授权" item drives this handle from the UI thread.
    let authority = server.authority();
    let addr = server.local_addr().expect("local addr");
    tokio::spawn(server.run_with(
        fake_enumerator(),
        Duration::from_millis(20),
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
    ));

    let mut ws = connect(&addr, ORIGIN_A, Some("grant-until-revoked")).await;

    authority.revoke_all();

    // Without a push the web client only finds out by failing a flash, which
    // costs the user a wasted attempt.
    let revoked = next_frame_of_type(&mut ws, "auth_revoked").await;
    assert_eq!(revoked["type"], "auth_revoked", "{revoked}");

    // Revocation must reach the connection that is already open, not just the
    // file: otherwise "撤销所有授权" leaves the current tab fully privileged.
    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "a revoked connection must be asked again"
    );
    assert!(
        backend.flash_specs().is_empty(),
        "nothing may reach the device after a revocation"
    );
}

// ── Token query-parameter decoding (B7 cycle 4) ──────────────────────────────

/// Seed a grant for `token` and report whether presenting `presented` verbatim
/// in the query string authorizes the connection.
async fn presenting_authorizes(token: &str, presented: &str) -> bool {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let tokens = Arc::new(MemoryTokenStore::default());
    tokens.insert(Grant {
        token: token.to_string(),
        origin: ORIGIN_A.to_string(),
        granted_at_ms: 1_784_800_000_000,
    });
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .start()
    .await;

    let mut ws = connect(&addr, ORIGIN_A, Some(presented)).await;
    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let result = next_frame_of_type(&mut ws, "job_result").await;
    // A refusing prompt means "the token was not honoured"; a run means it was.
    result["ok"] == true
}

#[tokio::test]
async fn a_percent_encoded_token_is_decoded_before_it_is_matched() {
    // The web client percent-encodes the query value, so the bridge has to decode
    // it — a token is a credential, and a credential must not depend on which
    // side happened to escape it.
    assert!(
        presenting_authorizes("tok en", "tok%20en").await,
        "%20 must decode to a space"
    );
    assert!(
        presenting_authorizes("tok+en", "tok%2Ben").await,
        "%2B must decode to a plus"
    );
}

#[tokio::test]
async fn a_plus_in_the_query_stays_a_plus() {
    // Deliberately *not* form-encoding: `+` is a literal here, so a token
    // containing one round-trips unescaped and never silently becomes a space.
    assert!(
        presenting_authorizes("tok+en", "tok+en").await,
        "a literal + must match a token containing +"
    );
    assert!(
        !presenting_authorizes("tok en", "tok+en").await,
        "+ must not be read as a space"
    );
}

#[tokio::test]
async fn a_live_connection_loses_its_privilege_as_soon_as_the_grant_is_gone() {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let tokens = seeded_grant("grant-until-revoked");
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .start()
    .await;

    let mut ws = connect(&addr, ORIGIN_A, Some("grant-until-revoked")).await;

    // The grant is withdrawn straight in the store, without the notification path
    // that `Authority::revoke_all` also drives. Privilege that was granted by a
    // token must be re-derived from the store, not latched at handshake time:
    // otherwise a connection that the notification misses — or one opened in the
    // same instant a revocation lands — keeps flashing after the user revoked.
    tokens.revoke_all();

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert!(
        backend.flash_specs().is_empty(),
        "a withdrawn grant must not still reach the device"
    );
}

// ── Cancelling inside the confirmation window (B8) ───────────────────────────

#[tokio::test]
async fn a_cancel_arriving_during_the_confirmation_window_stops_the_job() {
    let backend = FakeBackend::finishing();
    let prompt = common::hanging();
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    // Long enough that only the cancel can end the wait.
    .confirm_timeout(Duration::from_secs(30))
    .start()
    .await;
    let mut ws = connect(&addr, ORIGIN_A, None).await;

    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;
    let asked = prompt.first_request(Duration::from_secs(2)).await;
    assert_eq!(asked.port, PORT_A, "{asked:?}");

    // The user hit 取消 in the browser while the confirmation was still up. The
    // old behaviour dropped this frame (no job had claimed a port yet), so the
    // page said "已取消" while an approval seconds later still wrote the device.
    send_json(
        &mut ws,
        serde_json::json!({ "type": "cancel", "request_id": "j-1" }),
    )
    .await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["request_id"], "j-1", "{result}");
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_code"], "cancelled", "{result}");
    assert!(
        backend.flash_specs().is_empty(),
        "a cancelled request must never reach the device: {:?}",
        backend.flash_specs()
    );
}

#[tokio::test]
async fn closing_the_tab_during_the_confirmation_window_releases_the_execution_right() {
    let backend = FakeBackend::finishing();
    let prompt = common::hanging();
    let tokens = seeded_grant("shared-grant");
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .confirm_timeout(Duration::from_secs(30))
    .start()
    .await;

    // An unauthorized tab asks, then the user closes it while the dialog is up.
    let mut abandoned = connect(&addr, ORIGIN_A, None).await;
    send_json(&mut abandoned, run_job_frame("j-1", PORT_A)).await;
    prompt.first_request(Duration::from_secs(2)).await;
    drop(abandoned);

    // The pending confirmation held the single execution right; a disconnect has
    // to give it back, or one closed tab locks the helper for the whole 30s
    // window (and for 60s in production).
    let mut next = connect(&addr, ORIGIN_A, Some("shared-grant")).await;
    send_json(&mut next, run_job_frame("j-2", PORT_A)).await;
    let result = next_frame_of_type(&mut next, "job_result").await;
    assert_eq!(result["request_id"], "j-2", "{result}");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "the surviving connection was pre-authorized and must not be asked"
    );
}

// ── Grant file fails closed (B8) ─────────────────────────────────────────────

#[test]
fn an_unreadable_grant_file_authorizes_nobody() {
    for (tag, content) in [
        ("garbage", "{ this is not json"),
        ("empty", ""),
        (
            "future_version",
            r#"{"version":99,"grants":[{"token":"tok","origin":"http://localhost:3000","granted_at_ms":1}]}"#,
        ),
        ("wrong_shape", r#"{"version":1,"grants":[{"tok":"tok"}]}"#),
    ] {
        let path = scratch_store_path(&format!("failclosed-{tag}"));
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(&path, content).expect("write the damaged grant file");

        let store = FileTokenStore::open_at(&path)
            .unwrap_or_else(|e| panic!("{tag}: a damaged grant file must not be fatal: {e:#}"));

        // Fail closed: an unreadable receipt is not a receipt. The user gets asked
        // again — never silently granted.
        assert!(
            !store.is_granted("tok", ORIGIN_A),
            "{tag}: a damaged grant file must authorize nobody"
        );

        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }
}

// ── Unattended posture ignores persisted grants (B9) ─────────────────────────

#[tokio::test]
async fn ignoring_persisted_grants_makes_a_valid_token_ask_again() {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let tokens = seeded_grant("granted-while-a-human-was-present");
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    // What --headless without the unattended opt-in runs with: a grant means
    // "somebody confirmed this at a keyboard", and that consent does not carry
    // over into a session where nobody is watching.
    .grant_policy(GrantPolicy::Ignore)
    .start()
    .await;

    let mut ws = connect(&addr, ORIGIN_A, Some("granted-while-a-human-was-present")).await;
    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["error_code"], "user_rejected", "{result}");
    assert_eq!(
        prompt.requests().len(),
        1,
        "the stored grant must not silently stand in for a confirmation"
    );
    assert!(
        backend.flash_specs().is_empty(),
        "nothing may reach the device: {:?}",
        backend.flash_specs()
    );
}

#[tokio::test]
async fn honouring_persisted_grants_stays_the_default() {
    let backend = FakeBackend::finishing();
    let prompt = common::rejecting();
    let tokens = seeded_grant("granted-while-a-human-was-present");
    // No explicit policy: the tray shell must keep skipping the dialog for a
    // token it already issued, or every page reload costs a confirmation.
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        Arc::clone(&prompt) as Arc<dyn AuthPrompt>,
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .start()
    .await;

    let mut ws = connect(&addr, ORIGIN_A, Some("granted-while-a-human-was-present")).await;
    send_json(&mut ws, run_job_frame("j-1", PORT_A)).await;

    let result = next_frame_of_type(&mut ws, "job_result").await;
    assert_eq!(result["ok"], true, "{result}");
    assert!(
        prompt.requests().is_empty(),
        "a valid token must not raise a dialog in an attended session"
    );
}

#[tokio::test]
async fn an_ignored_grant_is_recorded_as_unauthorized_in_the_audit_trail() {
    let backend = FakeBackend::finishing();
    let audit = common::capturing_audit();
    let tokens = seeded_grant("granted-while-a-human-was-present");
    let addr = ServerBuilder::new(
        Arc::clone(&backend) as Arc<dyn FlashBackend>,
        common::rejecting(),
    )
    .tokens(Arc::clone(&tokens) as Arc<dyn TokenStore>)
    .audit(Arc::clone(&audit) as Arc<dyn AuditSink>)
    .grant_policy(GrantPolicy::Ignore)
    .start()
    .await;

    let _ws = connect(&addr, ORIGIN_A, Some("granted-while-a-human-was-present")).await;

    // The audit trail is what an incident review reads. Under GrantPolicy::Ignore
    // the stored grant grants nothing, so a line claiming this connection arrived
    // pre-authorized would misreport the machine's actual security posture.
    let lines = audit.lines();
    let connect_line = lines
        .iter()
        .find(|l| l.starts_with("connect "))
        .unwrap_or_else(|| panic!("no connect line in the audit trail: {lines:?}"));
    assert!(
        connect_line.contains("pre_authorized=false"),
        "an ignored grant must not be logged as pre-authorization: {connect_line}"
    );
}
