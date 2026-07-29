//! Cobuilder Bridge (tyutool-bridge): headless resident process exposing a
//! local WebSocket server on 127.0.0.1 so cobuilder-web can flash devices
//! through the tyutool engine.
//!
//! B1 scope: WS server + Origin allowlist check + hello frame push.
//! B2 scope: device auto-discovery — full `ports` frame after hello plus
//! diff-driven pushes to every connected client.
//! B3 scope: flash job orchestration — run_job / cancel / check_port with a
//! port arbitration table (busy immediately on conflict, no queuing).
//! B4 scope: run_auth authorization writes sharing that same arbitration table,
//! so flash and auth are mutually exclusive on one port.
//! B5 scope: serial monitor sessions (`serial_debug_*`) on that same table,
//! plus the post-flash handoff window that reserves the port for the connection
//! whose flash or authorization job just succeeded on it.
//! B6 scope: runtime stats (connections / devices) published on a watch
//! channel for the resident tray shell.

pub mod status;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;

/// Fixed bridge port (compile-time constant, no drift on conflict).
/// Deliberately distinct from `tyutool-cli serve`'s default 9527 so a
/// developer's own tyutool never collides with the bridge.
pub const DEFAULT_PORT: u16 = 18730;

/// Local WS protocol version reported in the hello frame.
/// Integer; bump only on breaking protocol changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Handshake Origin allowlist (compile-time constant).
///
/// Currently local development addresses only (cobuilder-web dev server
/// defaults to port 3000; 5173 is the plain Vite default kept as fallback).
/// TODO: add the Cobuilder production domain list once confirmed during
/// integration (联调期确认线上域名清单后补充).
pub const ORIGIN_ALLOWLIST: &[&str] = &[
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "http://localhost:5173",
    "http://127.0.0.1:5173",
];

/// USB VID allowlist for the device selector (constant table, extended over
/// releases): WCH CH34x, Silicon Labs CP210x, FTDI. Ports with other VIDs are
/// still pushed with `whitelisted=false` so the web UI can gray them out.
pub const VID_ALLOWLIST: &[u16] = &[0x1A86, 0x10C4, 0x0403];

/// One enumerated serial port as seen by the enumeration source, before the
/// bridge derives wire-level fields (whitelisted / first_seen_ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumeratedPort {
    pub path: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub vendor: Option<String>,
    pub busy: bool,
}

/// Injectable enumeration source: production wires tyutool-core enumeration,
/// tests inject a fake so discovery logic runs without real hardware.
pub type PortEnumerator = Arc<dyn Fn() -> Vec<EnumeratedPort> + Send + Sync>;

/// Poll period of the production enumeration loop.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Buffered `ports` frames per subscriber before a slow client is marked lagged.
/// Frames are full snapshots, so a lagged client only loses intermediate states
/// and still converges on the newest list.
const PORTS_BROADCAST_CAPACITY: usize = 16;

/// Frames queued per connection before the client is declared dead.
///
/// Bounded on purpose: progress frames are high-frequency, so a client that
/// stops reading would otherwise grow this queue without limit for the whole
/// job. 256 absorbs any realistic scheduling hiccup; staying full past that is
/// a client that no longer consumes, and the connection is dropped.
const SINK_QUEUE_CAPACITY: usize = 256;

/// One decoded flash job handed to the execution backend (firmware already
/// base64-decoded by the bridge).
#[derive(Debug, Clone)]
pub struct FlashJobSpec {
    pub chip_id: String,
    pub port: String,
    pub baud_rate: u32,
    pub mode: String,
    pub start_addr: u64,
    pub firmware: Vec<u8>,
}

/// One authorization-write job handed to the execution backend
/// (credentials already validated as non-empty by the bridge).
#[derive(Debug, Clone)]
pub struct AuthJobSpec {
    pub chip_id: String,
    pub port: String,
    pub baud_rate: u32,
    pub uuid: String,
    pub auth_key: String,
}

/// Terminal failure from the execution backend; carried into `job_result`.
#[derive(Debug, Clone)]
pub struct JobError {
    pub error_code: String,
    pub message: String,
}

/// An open serial monitor session; dropping the box without `close` is a bug
/// (close releases the underlying reader thread deterministically).
pub trait DebugSessionHandle: Send {
    fn close(self: Box<Self>);
}

/// Outcome of an OS-level port availability probe (check_port).
#[derive(Debug, Clone)]
pub struct PortProbe {
    pub available: bool,
    pub reason: Option<String>,
}

/// Injectable flash execution surface: production wires tyutool-core
/// (`run_job` / `check_port_available`); tests inject fakes so job
/// orchestration and port arbitration run without real hardware.
pub trait FlashBackend: Send + Sync {
    /// Execute one flash job (blocking). `cancel` is the cooperative stop
    /// flag; `progress` receives payloads pushed verbatim to the client
    /// inside `progress` frames.
    fn run_job(
        &self,
        spec: FlashJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError>;

    /// Execute one authorization write (blocking). Default answers
    /// "unsupported" so job-only fakes need not implement it; the production
    /// backend overrides with the tyutool-core authorize flow.
    fn run_auth(
        &self,
        spec: AuthJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        let _ = (spec, cancel, progress);
        Err(JobError {
            error_code: "unsupported".to_string(),
            message: "run_auth not supported by this backend".to_string(),
        })
    }

    /// Open a serial monitor session (blocking open, then reader-driven
    /// callbacks). Default answers "unsupported" so job-only fakes need not
    /// implement it; the production backend overrides with tyutool-core's
    /// `SerialDebugSession`.
    fn open_debug_session(
        &self,
        cfg: tyutool_core::DebugConfig,
        on_chunk: Box<dyn Fn(tyutool_core::DebugChunk) + Send + Sync>,
        on_disconnect: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn DebugSessionHandle>, JobError> {
        let _ = (cfg, on_chunk, on_disconnect);
        Err(JobError {
            error_code: "unsupported".to_string(),
            message: "serial debug not supported by this backend".to_string(),
        })
    }

    /// OS-level availability probe for a port the bridge does not hold.
    fn probe_port(&self, port: &str) -> PortProbe;
}

/// Platform tag reported in the hello frame (kept in the wire vocabulary the
/// web client already uses: `darwin` / `windows` / `linux`).
#[cfg(target_os = "macos")]
const PLATFORM: &str = "darwin";
#[cfg(target_os = "windows")]
const PLATFORM: &str = "windows";
#[cfg(target_os = "linux")]
const PLATFORM: &str = "linux";
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
const PLATFORM: &str = "unknown";

/// First frame pushed to every accepted connection, so the web client can gate
/// features on bridge version / protocol version without an extra round trip.
#[derive(Debug, Serialize)]
struct Hello {
    #[serde(rename = "type")]
    kind: &'static str,
    app_version: &'static str,
    protocol_version: u32,
    platform: &'static str,
    os_version: String,
}

impl Hello {
    fn current() -> Self {
        Self {
            kind: "hello",
            app_version: env!("CARGO_PKG_VERSION"),
            protocol_version: PROTOCOL_VERSION,
            platform: PLATFORM,
            os_version: os_version(),
        }
    }
}

/// One port as pushed on the wire: enumeration data plus bridge-derived fields.
#[derive(Debug, Serialize)]
struct WirePort {
    port: String,
    /// Uppercase 4-digit hex, omitted for non-USB ports.
    #[serde(skip_serializing_if = "Option::is_none")]
    vid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vendor: Option<String>,
    whitelisted: bool,
    busy: bool,
    first_seen_ms: u64,
}

/// Full device list frame; always the complete list, never a delta.
#[derive(Debug, Serialize)]
struct PortsFrame {
    #[serde(rename = "type")]
    kind: &'static str,
    ports: Vec<WirePort>,
}

// ── B3 wire messages ─────────────────────────────────────────────────────────
//
// The bridge speaks its own snake_case protocol, deliberately *not* reusing
// `tyutool-serve`'s ClientMessage/ServerMessage: serve's `FlashJob` is camelCase
// and carries no `request_id`, while every bridge job frame is correlated by
// `request_id` so one connection can host several jobs.

/// Job parameters as sent by the web client (firmware travels separately, in
/// `file_content`, base64-encoded).
#[derive(Debug, Deserialize)]
struct WireJob {
    chip_id: String,
    port: String,
    baud_rate: u32,
    /// Only `write` is executed in B3; erase/read/authorize come later.
    #[serde(default = "default_job_mode")]
    mode: String,
    #[serde(default)]
    start_addr: u64,
}

fn default_job_mode() -> String {
    "write".to_string()
}

/// Authorization parameters as sent by the web client.
///
/// `uuid` / `auth_key` default to the empty string instead of being required:
/// a frame that simply omits one must reach the bridge's own validation and be
/// answered with a `bad_request` `job_result`, not be dropped as unparsable
/// (the wire contract has no error frame for undecodable input).
#[derive(Debug, Deserialize)]
struct WireAuth {
    chip_id: String,
    port: String,
    #[serde(default = "default_auth_baud_rate")]
    baud_rate: u32,
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    auth_key: String,
}

/// Authorization runs over the device's UART shell, not the flash bootloader,
/// so it uses the firmware console rate rather than the flash baud rate.
fn default_auth_baud_rate() -> u32 {
    921_600
}

/// Serial monitor parameters as sent by the web client.
///
/// Deliberately *not* `tyutool_core::DebugConfig`: core's serde shape is
/// camelCase with spelled-out enums (`"dataBits": "eight"`), while the bridge
/// wire is snake_case with the numeric widths the UI already shows
/// (`"data_bits": 8`). Every field but `port` defaults, and `port` defaults to
/// the empty string rather than being required, so an incomplete frame reaches
/// the bridge's own validation and is answered with `bad_request` instead of
/// being dropped as unparsable (same rationale as [`WireAuth`]).
#[derive(Debug, Deserialize)]
struct WireDebugCfg {
    #[serde(default)]
    port: String,
    #[serde(default = "default_debug_baud_rate")]
    baud_rate: u32,
    #[serde(default = "default_data_bits")]
    data_bits: u8,
    #[serde(default = "default_stop_bits")]
    stop_bits: u8,
    #[serde(default = "default_parity")]
    parity: String,
}

fn default_debug_baud_rate() -> u32 {
    115_200
}

fn default_data_bits() -> u8 {
    8
}

fn default_stop_bits() -> u8 {
    1
}

fn default_parity() -> String {
    "none".to_string()
}

impl WireDebugCfg {
    /// Validate and map onto core's `DebugConfig`. `Err` carries the message of
    /// a `bad_request` `serial_debug_open_failed`.
    ///
    /// `stop_bits` accepts 1 and 2 only: core also models 1.5, but the
    /// serialport crate cannot set it (core logs a warning and falls back to
    /// 1), so the wire does not offer a value it cannot honour.
    fn into_core(self) -> Result<tyutool_core::DebugConfig, String> {
        if self.port.trim().is_empty() {
            return Err("serial monitor requires a non-empty port".to_string());
        }
        let data_bits = match self.data_bits {
            5 => tyutool_core::DataBits::Five,
            6 => tyutool_core::DataBits::Six,
            7 => tyutool_core::DataBits::Seven,
            8 => tyutool_core::DataBits::Eight,
            other => return Err(format!("unsupported data_bits {other}, expected 5/6/7/8")),
        };
        let stop_bits = match self.stop_bits {
            1 => tyutool_core::StopBits::One,
            2 => tyutool_core::StopBits::Two,
            other => return Err(format!("unsupported stop_bits {other}, expected 1/2")),
        };
        let parity = match self.parity.as_str() {
            "none" => tyutool_core::Parity::None,
            "odd" => tyutool_core::Parity::Odd,
            "even" => tyutool_core::Parity::Even,
            other => {
                return Err(format!(
                    "unsupported parity '{other}', expected none/odd/even"
                ))
            }
        };
        Ok(tyutool_core::DebugConfig {
            port: self.port,
            baud_rate: self.baud_rate,
            data_bits,
            parity,
            stop_bits,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    RunJob {
        request_id: String,
        job: WireJob,
        /// Base64-encoded firmware image.
        file_content: String,
    },
    RunAuth {
        request_id: String,
        auth: WireAuth,
    },
    Cancel {
        request_id: String,
    },
    CheckPort {
        port: String,
    },
    SerialDebugOpen {
        cfg: WireDebugCfg,
    },
    SerialDebugClose,
}

/// One serial monitor chunk on the wire.
///
/// Core's `DebugChunk` serializes as camelCase with the payload as a JSON byte
/// array; the bridge wire is snake_case and base64, which is both an order of
/// magnitude smaller and what the web client already decodes for firmware.
#[derive(Debug, Serialize)]
struct WireChunk {
    ts_ms: u64,
    direction: &'static str,
    bytes_b64: String,
}

impl WireChunk {
    fn from_core(chunk: &tyutool_core::DebugChunk) -> Self {
        Self {
            ts_ms: chunk.ts_ms,
            direction: match chunk.direction {
                tyutool_core::Direction::Rx => "rx",
                tyutool_core::Direction::Tx => "tx",
            },
            bytes_b64: base64::engine::general_purpose::STANDARD.encode(&chunk.bytes),
        }
    }
}

/// Frames the bridge pushes in response to B3 commands.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerFrame {
    /// Backend progress, forwarded verbatim inside `payload`.
    Progress {
        request_id: String,
        payload: serde_json::Value,
    },
    JobResult {
        request_id: String,
        ok: bool,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    CheckPortResult {
        port: String,
        available: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    // ── B5 serial monitor ────────────────────────────────────────────────────
    //
    // No `request_id`: a connection has at most one session, so the frames need
    // no correlation key (unlike jobs, of which one connection can host many).
    SerialDebugOpened,
    SerialDebugOpenFailed {
        error_code: String,
        message: String,
    },
    SerialDebugChunkBatch {
        chunks: Vec<WireChunk>,
    },
    SerialDebugClosed,
    /// Session ended device-side (cable pulled, driver error); `reason` is the
    /// backend's text, forwarded verbatim.
    SerialDebugDisconnected {
        reason: String,
    },
}

/// Tracks first-seen timestamps so a port keeps its `first_seen_ms` across
/// re-enumeration, while a replug counts as a new device (PRD: 重插按新设备).
#[derive(Default)]
struct FirstSeenLedger {
    seen: HashMap<String, u64>,
}

impl FirstSeenLedger {
    /// Record the current snapshot: new paths get `now`, vanished paths are
    /// forgotten so a later replug is stamped fresh.
    fn sync(&mut self, ports: &[EnumeratedPort], now: u64) {
        self.seen
            .retain(|path, _| ports.iter().any(|p| &p.path == path));
        for port in ports {
            self.seen.entry(port.path.clone()).or_insert(now);
        }
    }

    fn first_seen(&self, path: &str) -> u64 {
        self.seen.get(path).copied().unwrap_or(0)
    }
}

// ── Port arbitration ─────────────────────────────────────────────────────────

/// One serial port held by one running job.
struct Held {
    port: String,
    cancel: Arc<AtomicBool>,
}

/// Identifies one job across the whole bridge.
///
/// `request_id` alone is not enough: it is a client-chosen string, and two
/// browser tabs picking the same one ("job-1") must not collide. Every
/// connection gets its own `conn_id`, so request ids are namespaced per
/// connection.
type JobKey = (u64, String);

/// How long a port stays reserved for the connection whose flash or
/// authorization job just succeeded on it (B5 post-flash handoff window), and
/// the default of [`BridgeServer::with_handoff_window`].
///
/// A successful job is almost always followed by "now show me the boot log",
/// and between `job_result` and the client's `serial_debug_open` the port is
/// otherwise up for grabs — another tab, or the user's own second window,
/// could take the board the job just prepared. Three seconds covers the
/// round trip with room to spare while staying far below the time a human
/// needs to start a competing action deliberately.
const HANDOFF_WINDOW: Duration = Duration::from_secs(3);

/// Who owns a port in the arbitration table.
enum PortHolder {
    /// A flash / auth job is running on it. Carries no key: which job it is
    /// lives in `by_request`, and this index only answers "is it taken".
    Job,
    /// A serial monitor session holds it (at most one per connection).
    Session(u64),
    /// Post-flash handoff window: nobody is driving the port, but until
    /// `until` only `conn_id` may claim it. See [`HANDOFF_WINDOW`].
    HandoffReserved { conn_id: u64, until: Instant },
}

/// Process-wide serial port ownership: at most one holder per port, conflicts
/// are refused immediately instead of queued (PRD: 占用即拒，不排队).
///
/// Two indexes behind one lock, kept in step: `by_request` answers cancel /
/// release by job key, `by_port` answers claim / busy lookups by port path.
/// Ports are the one truly global resource, so `by_port` spans connections
/// while job identity stays connection-local. Serial monitor sessions live in
/// `by_port` only — one per connection means they need no request namespace,
/// and keeping them out of `by_request` is also what makes a `cancel` frame
/// unable to reach (and kill) a monitor.
struct PortArbiter {
    inner: Mutex<ArbiterState>,
    next_conn_id: AtomicU64,
    /// Lifetime of the reservations [`PortArbiter::release`] creates;
    /// [`HANDOFF_WINDOW`] on every production path.
    handoff_window: Duration,
}

#[derive(Default)]
struct ArbiterState {
    by_request: HashMap<JobKey, Held>,
    by_port: HashMap<String, PortHolder>,
}

impl ArbiterState {
    /// May `conn_id` take `port` right now?
    ///
    /// Expiry of a handoff reservation is evaluated lazily here — the window is
    /// only ever observed at claim time, so no timer has to fire to end it.
    /// A reservation that is either this connection's own or already past its
    /// deadline is dropped and the port handed over.
    fn acquirable(&mut self, port: &str, conn_id: u64) -> bool {
        let takeover = match self.by_port.get(port) {
            None => return true,
            Some(PortHolder::Job | PortHolder::Session(_)) => false,
            Some(PortHolder::HandoffReserved {
                conn_id: owner,
                until,
            }) => *owner == conn_id || Instant::now() >= *until,
        };
        if takeover {
            self.by_port.remove(port);
        }
        takeover
    }
}

/// Why a port could not be claimed.
enum ClaimRefused {
    /// Another job or session already holds that port (or its handoff window
    /// belongs to a different connection).
    PortHeld,
    /// The client reused a `request_id` that is still running.
    DuplicateRequest,
}

impl PortArbiter {
    fn new(handoff_window: Duration) -> Self {
        Self {
            inner: Mutex::new(ArbiterState::default()),
            next_conn_id: AtomicU64::new(0),
            handoff_window,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ArbiterState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Hand out the next connection's job-id namespace.
    fn next_conn_id(&self) -> u64 {
        self.next_conn_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Take ownership of `port` for this connection's `request_id`, handing
    /// back the job's cooperative cancel flag.
    fn claim(
        &self,
        port: &str,
        conn_id: u64,
        request_id: &str,
    ) -> Result<Arc<AtomicBool>, ClaimRefused> {
        let mut state = self.lock();
        let key = (conn_id, request_id.to_string());
        // Reuse is only a conflict within one connection: the same id on
        // another connection is a different job. Checked before `acquirable`,
        // which consumes this connection's handoff reservation — a request the
        // arbiter is about to refuse must not spend it.
        if state.by_request.contains_key(&key) {
            return Err(ClaimRefused::DuplicateRequest);
        }
        if !state.acquirable(port, conn_id) {
            return Err(ClaimRefused::PortHeld);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        state.by_request.insert(
            key,
            Held {
                port: port.to_string(),
                cancel: Arc::clone(&cancel),
            },
        );
        state.by_port.insert(port.to_string(), PortHolder::Job);
        Ok(cancel)
    }

    /// Take `port` for this connection's serial monitor; `false` means it is
    /// held (or reserved for someone else).
    fn claim_session(&self, port: &str, conn_id: u64) -> bool {
        let mut state = self.lock();
        if !state.acquirable(port, conn_id) {
            return false;
        }
        state
            .by_port
            .insert(port.to_string(), PortHolder::Session(conn_id));
        true
    }

    /// Give a monitored port back; a no-op unless this connection's session
    /// still holds it (a close racing a device disconnect runs twice).
    fn release_session(&self, port: &str, conn_id: u64) {
        let mut state = self.lock();
        let mine = matches!(state.by_port.get(port), Some(PortHolder::Session(owner)) if *owner == conn_id);
        if mine {
            state.by_port.remove(port);
        }
    }

    /// Give the port back after a job; a no-op for an unknown / already
    /// released job.
    ///
    /// `reserve` downgrades the release to a `handoff_window`-long reservation
    /// for the same connection instead of freeing the port outright.
    fn release(&self, conn_id: u64, request_id: &str, reserve: bool) {
        let mut state = self.lock();
        let Some(held) = state.by_request.remove(&(conn_id, request_id.to_string())) else {
            return;
        };
        if reserve {
            state.by_port.insert(
                held.port,
                PortHolder::HandoffReserved {
                    conn_id,
                    until: Instant::now() + self.handoff_window,
                },
            );
        } else {
            state.by_port.remove(&held.port);
        }
    }

    /// Drop every handoff reservation of a departing connection: a closed tab
    /// must not shield a port for the rest of its window.
    fn clear_reservations(&self, conn_id: u64) {
        self.lock().by_port.retain(|_, holder| {
            !matches!(holder, PortHolder::HandoffReserved { conn_id: owner, .. } if *owner == conn_id)
        });
    }

    /// Raise the job's cancel flag. `false` means this connection is running no
    /// such job (it already finished, or the id was never claimed here).
    ///
    /// Cancelling across connections is impossible by construction — the
    /// natural boundary of the per-connection id namespace, and the behaviour
    /// we want: one tab must not be able to abort another tab's flash.
    fn cancel(&self, conn_id: u64, request_id: &str) -> bool {
        match self
            .lock()
            .by_request
            .get(&(conn_id, request_id.to_string()))
        {
            Some(held) => {
                held.cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Is someone actively driving `port`?
    ///
    /// A handoff reservation deliberately answers `false`: `ports.busy` means
    /// "occupied", and the B3 contract has it flip back to false as soon as
    /// `job_result` lands. The window is a short gate, not an occupancy — the
    /// trade-off is that a refused `port_busy` during the window has no
    /// matching `busy=true` in the device list, which beats flickering the
    /// whole list true→false→true around every flash.
    fn is_busy(&self, port: &str) -> bool {
        matches!(
            self.lock().by_port.get(port),
            Some(PortHolder::Job | PortHolder::Session(_))
        )
    }
}

/// Fold arbitration truth into an enumeration snapshot: a port is busy when the
/// enumeration source says so *or* the bridge itself holds it for a job.
///
/// Applied before diffing, so claim / release alone are enough to trigger the
/// next `ports` push.
fn apply_arbitration_busy(ports: &mut [EnumeratedPort], arbiter: &PortArbiter) {
    for port in ports.iter_mut() {
        port.busy = port.busy || arbiter.is_busy(&port.path);
    }
}

/// Milliseconds since the Unix epoch (0 only if the system clock predates it).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Serialize one enumeration snapshot into the wire `ports` frame.
fn ports_frame_json(ports: &[EnumeratedPort], ledger: &FirstSeenLedger) -> anyhow::Result<String> {
    let frame = PortsFrame {
        kind: "ports",
        ports: ports
            .iter()
            .map(|p| WirePort {
                port: p.path.clone(),
                vid: p.vid.map(|vid| format!("{vid:04X}")),
                pid: p.pid.map(|pid| format!("{pid:04X}")),
                vendor: p.vendor.clone(),
                whitelisted: p.vid.is_some_and(|vid| VID_ALLOWLIST.contains(&vid)),
                busy: p.busy,
                first_seen_ms: ledger.first_seen(&p.path),
            })
            .collect(),
    };
    serde_json::to_string(&frame).context("serialize ports frame")
}

/// Normalize an enumeration result for change detection: order from the
/// enumeration source is not guaranteed, so compare path-sorted snapshots.
fn normalized(ports: &[EnumeratedPort]) -> Vec<EnumeratedPort> {
    let mut sorted = ports.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    sorted
}

/// Diff state of the device list, guarded as one unit.
struct PortsState {
    /// Newest enumeration result (path-sorted), before ownership is folded in.
    enumerated: Vec<EnumeratedPort>,
    /// List last put on the wire (enumeration + ownership composed).
    published: Vec<EnumeratedPort>,
    ledger: FirstSeenLedger,
}

/// Owns the device-list publishing path: newest-frame slot, broadcast channel
/// and diff state.
///
/// Two publishers, deliberately separated so the client always sees the cause
/// before the effect: the poller announces enumeration changes, while a job
/// announces the `busy` flip of the port it holds — right after its first
/// `progress` frame and right after its `job_result` frame. A poller-driven
/// ownership push would instead interleave arbitrarily with the job's own frames
/// (`busy=true` could precede the progress frame that explains it).
struct PortsBroadcaster {
    state: Mutex<PortsState>,
    /// Newest frame; read by connections at accept time, replaced on publish.
    snapshot: RwLock<String>,
    tx: broadcast::Sender<String>,
    arbiter: Arc<PortArbiter>,
}

impl PortsBroadcaster {
    fn new(arbiter: Arc<PortArbiter>, enumerated: Vec<EnumeratedPort>) -> anyhow::Result<Self> {
        let enumerated = normalized(&enumerated);
        let mut composed = enumerated.clone();
        apply_arbitration_busy(&mut composed, &arbiter);
        let mut ledger = FirstSeenLedger::default();
        ledger.sync(&composed, now_ms());
        let text = ports_frame_json(&composed, &ledger)?;
        let (tx, _) = broadcast::channel::<String>(PORTS_BROADCAST_CAPACITY);
        Ok(Self {
            state: Mutex::new(PortsState {
                enumerated,
                published: composed,
                ledger,
            }),
            snapshot: RwLock::new(text),
            tx,
            arbiter,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PortsState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Fresh enumeration result: publish when the device list itself changed.
    fn publish_enumeration(&self, ports: Vec<EnumeratedPort>) {
        let ports = normalized(&ports);
        let mut state = self.lock();
        if ports == state.enumerated {
            return;
        }
        state.enumerated = ports;
        self.publish(&mut state);
    }

    /// A job claimed or released a port: publish when that flips a `busy` flag.
    fn publish_ownership(&self) {
        let mut state = self.lock();
        self.publish(&mut state);
    }

    fn publish(&self, state: &mut PortsState) {
        let mut composed = state.enumerated.clone();
        apply_arbitration_busy(&mut composed, &self.arbiter);
        if composed == state.published {
            return;
        }
        state.ledger.sync(&composed, now_ms());
        let text = match ports_frame_json(&composed, &state.ledger) {
            Ok(text) => text,
            Err(e) => {
                log::error!("bridge ports frame serialize failed: {e:#}");
                return;
            }
        };
        state.published = composed;

        let mut guard = self
            .snapshot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = text.clone();
        // Err only means "nobody connected"; the snapshot above still carries
        // the list for the next connection.
        let _ = self.tx.send(text);
    }

    /// Read the current frame and subscribe to later pushes.
    ///
    /// Subscribing while the read lock is held is what makes the handoff exact:
    /// [`PortsBroadcaster::publish`] replaces the snapshot and broadcasts under
    /// the write lock, so a new connection either sees the old frame and
    /// receives the push, or already sees the new frame — never both (duplicate)
    /// and never neither (dropped).
    fn snapshot_and_subscribe(&self) -> (String, broadcast::Receiver<String>) {
        let guard = self
            .snapshot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (guard.clone(), self.tx.subscribe())
    }
}

/// Devices the user can actually act on: allowlisted VIDs only, matching the
/// `whitelisted` flag on the wire and the web UI's device count.
fn allowlisted_device_count(ports: &[EnumeratedPort]) -> usize {
    ports
        .iter()
        .filter(|p| p.vid.is_some_and(|vid| VID_ALLOWLIST.contains(&vid)))
        .count()
}

/// Publishes the tray shell's runtime counters.
///
/// The two counters have independent producers — connection handlers own
/// `connections`, the discovery poller owns `devices` — so every update writes
/// exactly one field in place instead of replacing the whole snapshot, which
/// would race the other producer's value away.
struct StatsPublisher {
    tx: watch::Sender<status::StatsSnapshot>,
}

impl StatsPublisher {
    fn new(tx: watch::Sender<status::StatsSnapshot>) -> Self {
        Self { tx }
    }

    /// Handshake completed: count the connection until its guard drops.
    ///
    /// The guard (rather than a decrement at the end of the handler) is what
    /// keeps the counter honest: every early return of `handle_connection`
    /// releases it, including the ones that bail before the read loop.
    fn connection(self: &Arc<Self>) -> ConnectionCount {
        self.tx.send_modify(|stats| stats.connections += 1);
        ConnectionCount {
            stats: Arc::clone(self),
        }
    }

    /// Fresh enumeration result; only a changed count wakes the tray.
    fn set_devices(&self, devices: usize) {
        self.tx.send_if_modified(|stats| {
            if stats.devices == devices {
                return false;
            }
            stats.devices = devices;
            true
        });
    }
}

/// Decrements the connection counter when the connection handler unwinds.
struct ConnectionCount {
    stats: Arc<StatsPublisher>,
}

impl Drop for ConnectionCount {
    fn drop(&mut self) {
        self.stats.tx.send_modify(|stats| {
            // Saturating rather than `-= 1`: an underflow would be a bug, but a
            // resident tray must not panic over a status line.
            stats.connections = stats.connections.saturating_sub(1);
        });
    }
}

/// Run the (synchronous) enumeration source off the async worker: real serial
/// enumeration shells out to the OS and blocks for milliseconds.
///
/// `None` means the enumeration task itself failed (panic / cancellation) —
/// distinct from "no ports", so callers keep the previous snapshot.
async fn enumerate(enumerator: &PortEnumerator) -> Option<Vec<EnumeratedPort>> {
    let enumerator = Arc::clone(enumerator);
    match tokio::task::spawn_blocking(move || enumerator()).await {
        Ok(ports) => Some(ports),
        Err(e) => {
            log::warn!("bridge port enumeration task failed: {e}");
            None
        }
    }
}

/// Background poller: re-enumerate every `poll_interval` and push the full list
/// to every connected client whenever the enumeration changed.
async fn poll_ports(
    enumerator: PortEnumerator,
    poll_interval: Duration,
    ports: Arc<PortsBroadcaster>,
    stats: Arc<StatsPublisher>,
) {
    let mut ticker = tokio::time::interval(poll_interval);
    loop {
        // The first tick fires immediately and merely re-confirms the seeded
        // snapshot, so it produces no push.
        ticker.tick().await;
        if let Some(enumerated) = enumerate(&enumerator).await {
            // Before `publish_enumeration`, which returns early when the list
            // is unchanged; the stats side does its own change detection.
            stats.set_devices(allowlisted_device_count(&enumerated));
            ports.publish_enumeration(enumerated);
        }
    }
}

/// Production enumeration source backed by tyutool-core.
///
/// Trade-offs: vendor comes from `usb_port_survey()`'s manufacturer field
/// matched by port path — best effort, a failing survey only costs the vendor
/// label. A failing `list_serial_ports()` yields the last good list rather than
/// an empty one, so a transient probe error does not blink the device list in
/// the web UI.
/// Vendor label for a known USB VID.
///
/// Derived from the VID instead of `usb_port_survey()`'s OS-reported
/// manufacturer: the survey internally re-runs the whole port enumeration
/// (3 underlying scans per poll tick) and its strings vary by driver/locale —
/// two snapshots of the same hardware could differ and be diffed into spurious
/// full-list pushes. The VID mapping is a single constant lookup, fully
/// deterministic, and matches the wire vocabulary ("WCH"). Unknown VIDs carry
/// no vendor label; the web UI grays them out via `whitelisted=false` anyway.
fn vendor_for_vid(vid: u16) -> Option<String> {
    match vid {
        0x1A86 => Some("WCH".to_string()),
        0x10C4 => Some("Silicon Labs".to_string()),
        0x0403 => Some("FTDI".to_string()),
        _ => None,
    }
}

fn real_port_enumerator() -> PortEnumerator {
    let last_good: Arc<std::sync::Mutex<Vec<EnumeratedPort>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    Arc::new(move || {
        let entries = match tyutool_core::list_serial_ports() {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("bridge serial enumeration failed, keeping last known list: {e}");
                return last_good
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
            }
        };

        let ports: Vec<EnumeratedPort> = entries
            .into_iter()
            .map(|entry| EnumeratedPort {
                vid: entry.usb_vid,
                pid: entry.usb_pid,
                vendor: entry.usb_vid.and_then(vendor_for_vid),
                // Enumeration reports no ownership of its own; ports the bridge
                // holds for a job are folded in later by
                // `apply_arbitration_busy`.
                busy: false,
                path: entry.path,
            })
            .collect();

        *last_good
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = ports.clone();
        ports
    })
}

// ── Production flash backend ─────────────────────────────────────────────────

/// Production flash execution surface backed by tyutool-core.
#[derive(Debug, Default)]
pub struct RealFlashBackend;

impl FlashBackend for RealFlashBackend {
    /// Maps the wire job onto `tyutool_core::FlashJob` for a single-image write.
    ///
    /// Field combination follows the CLI's `write` command (the only in-repo
    /// single-file writer): `firmware_path` + `flash_start_hex` +
    /// `flash_end_hex`, where end = start + image size (cf. the CLI's
    /// `compute_end_from_file`). Both bounds are required — the Beken and
    /// LN882H plugins reject a job with `flash_end_hex` missing — while the ESP
    /// plugins ignore the end bound, so always supplying it is the safe union.
    ///
    /// The bridge receives already-decoded bytes, so the firmware is written to
    /// one temp file (core plugins read from a path) and removed afterwards.
    fn run_job(
        &self,
        spec: FlashJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        // Trade-off: match on the wire string instead of deserializing into
        // `FlashMode`, so an unsupported mode fails with an actionable
        // `bad_request` rather than a serde error. erase/read/authorize get
        // their own frames in later slices.
        if spec.mode != "write" {
            return Err(JobError {
                error_code: "bad_request".to_string(),
                message: format!("unsupported job mode '{}', expected 'write'", spec.mode),
            });
        }

        let end_addr = spec.start_addr.saturating_add(spec.firmware.len() as u64);
        let firmware = match TempFirmware::stage(&spec.firmware) {
            Ok(staged) => staged,
            Err(e) => {
                return Err(JobError {
                    error_code: "flash_failed".to_string(),
                    message: format!("staging firmware failed: {e:#}"),
                })
            }
        };

        let job = tyutool_core::FlashJob {
            mode: tyutool_core::FlashMode::Flash,
            chip_id: spec.chip_id,
            port: spec.port,
            baud_rate: spec.baud_rate,
            segments: None,
            flash_start_hex: Some(format!("0x{:08X}", spec.start_addr)),
            flash_end_hex: Some(format!("0x{end_addr:08X}")),
            erase_start_hex: None,
            erase_end_hex: None,
            read_start_hex: None,
            read_end_hex: None,
            read_file_path: None,
            firmware_path: Some(firmware.path().to_string_lossy().to_string()),
            authorize_uuid: None,
            authorize_key: None,
            authorize_storage: None,
            confirm_overwrite: None,
        };

        // FlashEvent is forwarded verbatim (same payload shape as serve's
        // `progress`), so the web client has one event vocabulary everywhere.
        let result =
            tyutool_core::run_job(&job, &cancel, |event| match serde_json::to_value(&event) {
                Ok(value) => progress(value),
                Err(e) => log::warn!("bridge progress serialize failed: {e}"),
            });
        // `firmware` removes the staged file when it drops here — including on
        // the panic path, which a manual cleanup call would miss.
        drop(firmware);

        result.map_err(|e| JobError {
            error_code: match e {
                tyutool_core::FlashError::Cancelled => "cancelled",
                _ => "flash_failed",
            }
            .to_string(),
            message: e.to_string(),
        })
    }

    /// Maps the wire auth request onto tyutool-core's batch-auth slot, the only
    /// in-repo flow that writes *and verifies* one device's credentials
    /// (`run_batch_auth_slot`, `crates/tyutool-core/src/authorize.rs`).
    ///
    /// Single-slot adaptation of its Excel-oriented row callbacks:
    /// - `find_by_mac` → always `None`: the bridge authorizes whatever device is
    ///   on the port with the credentials the request carries, it does not
    ///   re-bind a MAC to a previously issued row.
    /// - `allocate_row` → the request's own credentials at row 0 (the row index
    ///   is only echoed back through `update_row`).
    /// - `update_row` → the developer log channel; there is no sheet to update.
    ///
    /// `ConflictPolicy::Overwrite` matches the PRD ("覆盖不可撤销"): the caller
    /// already confirmed the overwrite in the web UI, so a device carrying
    /// other credentials must be rewritten rather than skipped.
    ///
    /// Intentionally always-write: with `find_by_mac` pinned to `None` the
    /// slot's `AlreadyDone` / `Skipped` / `InsufficientCodes` outcomes are
    /// unreachable by construction — every request writes the supplied
    /// credentials, which is safe for the KV storage this adapter targets.
    fn run_auth(
        &self,
        spec: AuthJobSpec,
        cancel: Arc<AtomicBool>,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        let config = tyutool_core::BatchAuthSlotConfig {
            auth_baud_rate: spec.baud_rate,
            conflict_policy: tyutool_core::ConflictPolicy::Overwrite,
            auth_storage: tyutool_core::AuthStorage::Kv,
        };
        let credentials = Some((0usize, spec.uuid.clone(), spec.auth_key.clone()));
        let port = spec.port.clone();

        let result = tyutool_core::run_batch_auth_slot(
            &spec.port,
            &spec.chip_id,
            &config,
            |_mac| None,
            move || credentials,
            |_row, mac, update| match update {
                tyutool_core::BatchAuthRowUpdate::StepFailed { step, error } => {
                    log::warn!(
                        "bridge auth step failed: port={port} mac={mac} step={step}: {error}"
                    )
                }
                other => log::info!("bridge auth progress: port={port} mac={mac} {other:?}"),
            },
            &cancel,
            // BatchAuthStep serializes to a bare snake_case string; wrap it as
            // {"step": "..."} so every progress payload on the wire is a JSON
            // object (run_job's FlashEvent payloads already are). Keeps the
            // client decoder uniform across job kinds (see PROTOCOL.md).
            |step| match serde_json::to_value(step) {
                Ok(value) => progress(serde_json::json!({ "step": value })),
                Err(e) => log::warn!("bridge auth progress serialize failed: {e}"),
            },
        );

        // Terminal states the slot reports as `Ok`: only a written (or already
        // present) credential counts as success on the wire.
        match result {
            Ok(tyutool_core::BatchAuthSlotResult::Done { .. })
            | Ok(tyutool_core::BatchAuthSlotResult::AlreadyDone { .. }) => Ok(()),
            Ok(tyutool_core::BatchAuthSlotResult::Cancelled) => Err(JobError {
                error_code: "cancelled".to_string(),
                message: "cancelled by user".to_string(),
            }),
            // The write command already left the bridge: the credential may be
            // on the device, so say so rather than reporting a clean abort.
            Ok(tyutool_core::BatchAuthSlotResult::CancelledAfterWrite { mac, uuid }) => {
                Err(JobError {
                    error_code: "cancelled".to_string(),
                    message: format!(
                        "cancelled after the auth write was sent (mac {mac}, uuid {uuid}); \
                         the credential may already be on the device"
                    ),
                })
            }
            Ok(tyutool_core::BatchAuthSlotResult::DefaultMac { mac }) => Err(JobError {
                error_code: "auth_failed".to_string(),
                message: format!("device still carries the factory default MAC {mac}"),
            }),
            // Unreachable with this adaptation (credentials always allocate,
            // Overwrite never skips), mapped anyway so a core change surfaces
            // as a failure instead of a silent success.
            Ok(other) => Err(JobError {
                error_code: "auth_failed".to_string(),
                message: format!("authorization did not complete: {other:?}"),
            }),
            Err(e) => Err(JobError {
                error_code: match e {
                    tyutool_core::FlashError::Cancelled => "cancelled",
                    _ => "auth_failed",
                }
                .to_string(),
                message: e.to_string(),
            }),
        }
    }

    /// Opens tyutool-core's `SerialDebugSession` — the same reader-thread
    /// session the GUI and `tyutool-cli serve` use, so the bridge sees the
    /// identical chunk / disconnect vocabulary.
    fn open_debug_session(
        &self,
        cfg: tyutool_core::DebugConfig,
        on_chunk: Box<dyn Fn(tyutool_core::DebugChunk) + Send + Sync>,
        on_disconnect: Box<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn DebugSessionHandle>, JobError> {
        let port = cfg.port.clone();
        match tyutool_core::SerialDebugSession::open(cfg, on_chunk, on_disconnect) {
            Ok(session) => Ok(Box::new(RealDebugSession { session })),
            Err(e) => Err(JobError {
                error_code: "open_failed".to_string(),
                message: format!("opening serial monitor on {port} failed: {e}"),
            }),
        }
    }

    fn probe_port(&self, port: &str) -> PortProbe {
        let checked = tyutool_core::check_port_available(port);
        if checked.available {
            return PortProbe {
                available: true,
                reason: None,
            };
        }
        // `reason` is a stable machine code for the web UI; the OS error text
        // and the offending process stay in the developer log channel.
        log::info!(
            "bridge probe: {port} unavailable ({:?}, process {:?})",
            checked.error_message,
            checked.process_info
        );
        PortProbe {
            available: false,
            reason: Some("occupied_by_other_process".to_string()),
        }
    }
}

/// Production serial monitor handle: a live tyutool-core session whose reader
/// thread is joined by `close`.
struct RealDebugSession {
    session: tyutool_core::SerialDebugSession,
}

impl DebugSessionHandle for RealDebugSession {
    fn close(self: Box<Self>) {
        self.session.close();
    }
}

/// Distinguishes two stagings that share a process id and a clock reading.
///
/// The nanosecond stamp alone is not enough: on Windows the system clock has a
/// ~15 ms granularity, so two concurrent jobs can read the identical value and
/// would then write, flash and delete the *same* file.
static TEMP_FIRMWARE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Firmware bytes staged on disk for the core plugins (which read from a path),
/// removed again when this value drops.
struct TempFirmware {
    path: std::path::PathBuf,
}

impl TempFirmware {
    /// Write `firmware` to a temp file whose name no concurrent staging can
    /// collide with.
    fn stage(firmware: &[u8]) -> anyhow::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = TEMP_FIRMWARE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tyutool_bridge_fw_{}_{stamp}_{seq}.bin",
            std::process::id()
        ));
        std::fs::write(&path, firmware).with_context(|| format!("write {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempFirmware {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            log::warn!(
                "bridge temp firmware cleanup failed for {}: {e}",
                self.path.display()
            );
        }
    }
}

/// A bound (but not yet running) bridge WS server.
pub struct BridgeServer {
    listener: TcpListener,
    handoff_window: Duration,
}

/// Bind the bridge WS server on 127.0.0.1:`port`.
///
/// Fails (instead of drifting to another port) when the port is occupied;
/// the error names the port so startup logs are actionable.
pub async fn bind(port: u16) -> anyhow::Result<BridgeServer> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("failed to bind 127.0.0.1:{port}"))?;
    Ok(BridgeServer {
        listener,
        handoff_window: HANDOFF_WINDOW,
    })
}

impl BridgeServer {
    /// Actual bound address (useful when binding port 0 in tests).
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Shorten (or lengthen) the post-flash handoff window.
    ///
    /// A test seam, nothing else: the window is only ever observed lazily at
    /// claim time, so a test cannot watch one expire without sitting out the
    /// production [`HANDOFF_WINDOW`] — the value every real server runs with.
    pub fn with_handoff_window(mut self, window: Duration) -> Self {
        self.handoff_window = window;
        self
    }

    /// The accept loop, additionally publishing runtime stats (active
    /// connections / discovered allowlisted devices) on a watch channel — the
    /// resident tray shell renders them in its status line.
    ///
    /// This is the single implementation of the serving path; the unobserved
    /// entry points below delegate here.
    ///
    /// Adds the B3 message surface on top of hello/ports: `run_job` (progress
    /// stream + terminal `job_result`), `cancel`, `check_port`, with a global
    /// port arbitration table (conflicts answer busy immediately, no queuing)
    /// whose held ports surface as `busy=true` in the `ports` frames.
    pub async fn run_observed(
        self,
        enumerator: PortEnumerator,
        poll_interval: std::time::Duration,
        backend: Arc<dyn FlashBackend>,
        stats_tx: watch::Sender<status::StatsSnapshot>,
    ) -> anyhow::Result<()> {
        let arbiter = Arc::new(PortArbiter::new(self.handoff_window));
        let stats = Arc::new(StatsPublisher::new(stats_tx));

        // Seed synchronously before accepting: the first client must get the
        // current device list in its initial ports frame, not an empty list
        // followed by a correction.
        let initial = enumerate(&enumerator).await.unwrap_or_default();
        stats.set_devices(allowlisted_device_count(&initial));
        let ports = Arc::new(PortsBroadcaster::new(Arc::clone(&arbiter), initial)?);

        tokio::spawn(poll_ports(
            enumerator,
            poll_interval,
            Arc::clone(&ports),
            Arc::clone(&stats),
        ));

        loop {
            match self.listener.accept().await {
                Ok((stream, peer)) => {
                    log::info!("bridge WS connection from {peer}");
                    tokio::spawn(handle_connection(
                        stream,
                        Arc::clone(&ports),
                        Arc::clone(&arbiter),
                        Arc::clone(&backend),
                        Arc::clone(&stats),
                    ));
                }
                Err(e) => log::warn!("bridge accept error: {e}"),
            }
        }
    }

    /// Fully injected variant: enumeration source, poll interval and flash
    /// execution backend (tests drive all of it without hardware).
    pub async fn run_with(
        self,
        enumerator: PortEnumerator,
        poll_interval: std::time::Duration,
        backend: Arc<dyn FlashBackend>,
    ) -> anyhow::Result<()> {
        // Nobody is watching the counters on this path (headless serve, tests).
        // A `watch::Sender` with no live receiver still accepts in-place
        // updates, so the observed path stays the only implementation instead
        // of being duplicated with the publishing removed.
        let (stats_tx, _stats_rx) = watch::channel(status::StatsSnapshot::default());
        self.run_observed(enumerator, poll_interval, backend, stats_tx)
            .await
    }

    /// Like [`BridgeServer::run`], but with an injected enumeration source and
    /// poll interval (tests use a fake enumerator and a short interval).
    ///
    /// Every accepted client receives a full `ports` frame right after hello;
    /// the background poller re-enumerates every `poll_interval` and pushes the
    /// full list to all connected clients whenever it changed.
    pub async fn run_with_enumerator(
        self,
        enumerator: PortEnumerator,
        poll_interval: Duration,
    ) -> anyhow::Result<()> {
        self.run_with(enumerator, poll_interval, Arc::new(RealFlashBackend))
            .await
    }

    /// Accept loop with the production enumeration source (1s poll).
    pub async fn run(self) -> anyhow::Result<()> {
        self.run_with_enumerator(real_port_enumerator(), DEFAULT_POLL_INTERVAL)
            .await
    }

    /// [`BridgeServer::run`] plus the runtime-stats channel: what the tray shell
    /// runs on its background runtime. Keeps the production enumeration source
    /// and flash backend private to the crate.
    pub async fn run_with_stats(
        self,
        stats_tx: watch::Sender<status::StatsSnapshot>,
    ) -> anyhow::Result<()> {
        self.run_observed(
            real_port_enumerator(),
            DEFAULT_POLL_INTERVAL,
            Arc::new(RealFlashBackend),
            stats_tx,
        )
        .await
    }
}

// ── Serial monitor plumbing (B5) ─────────────────────────────────────────────

/// Batching thresholds for `serial_debug_chunk_batch`, taken from the
/// `tyutool-serve` blueprint (12 ms / 32 KiB): one frame per burst instead of
/// one per 4 KiB reader chunk keeps a chatty board from filling the
/// connection's bounded sink queue and getting itself declared dead.
const DEBUG_CHUNK_FLUSH_MS: u64 = 12;
const DEBUG_CHUNK_FLUSH_BYTES: usize = 32 * 1024;
/// Chunks queued between the serial reader thread and the batching thread.
/// Bounded so a lagging batcher slows the reader instead of growing memory.
const DEBUG_CHUNK_QUEUE_CAPACITY: usize = 256;

enum PumpMessage {
    Chunk(tyutool_core::DebugChunk),
    Shutdown,
}

/// Batching thread sitting between the serial reader callback (which runs on
/// the backend's own thread and must not block on the WS sink) and the
/// connection's frame queue.
struct ChunkPump {
    tx: std::sync::mpsc::SyncSender<PumpMessage>,
    join: std::thread::JoinHandle<()>,
}

impl ChunkPump {
    fn spawn(ctx: Weak<ConnContext>) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<PumpMessage>(DEBUG_CHUNK_QUEUE_CAPACITY);
        let join = std::thread::spawn(move || {
            let flush_after = Duration::from_millis(DEBUG_CHUNK_FLUSH_MS);
            let mut pending = tyutool_core::SerialDebugChunkBatchBuffer::new();
            loop {
                match rx.recv_timeout(flush_after) {
                    Ok(PumpMessage::Chunk(chunk)) => {
                        pending.push(chunk);
                        if pending.should_flush_bytes(DEBUG_CHUNK_FLUSH_BYTES) {
                            flush_chunks(&ctx, pending.take());
                        }
                    }
                    Ok(PumpMessage::Shutdown) => {
                        flush_chunks(&ctx, pending.take());
                        return;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if pending.should_flush_elapsed(flush_after) {
                            flush_chunks(&ctx, pending.take());
                        }
                    }
                    // Every sender is gone; nothing more can arrive.
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        flush_chunks(&ctx, pending.take());
                        return;
                    }
                }
            }
        });
        Self { tx, join }
    }

    fn sender(&self) -> std::sync::mpsc::SyncSender<PumpMessage> {
        self.tx.clone()
    }

    /// Flush the tail and stop the thread. Blocking (bounded by one flush
    /// period), so callers run it off the async workers.
    fn shutdown(self) {
        let Self { tx, join } = self;
        // An explicit message rather than just dropping `tx`: the backend's
        // chunk callback holds its own clone, and with a fake backend that
        // callback can outlive the session, so the channel is not guaranteed
        // to disconnect on its own.
        let _ = tx.send(PumpMessage::Shutdown);
        if join.join().is_err() {
            log::warn!("bridge serial monitor batching thread panicked");
        }
    }
}

/// Emit one batch. A dropped connection simply ends the batching thread's work.
fn flush_chunks(ctx: &Weak<ConnContext>, chunks: Vec<tyutool_core::DebugChunk>) {
    if chunks.is_empty() {
        return;
    }
    let Some(ctx) = ctx.upgrade() else {
        return;
    };
    ctx.send(&ServerFrame::SerialDebugChunkBatch {
        chunks: chunks.iter().map(WireChunk::from_core).collect(),
    });
}

/// One open serial monitor: the backend handle plus its batching thread.
struct DebugSession {
    port: String,
    handle: Box<dyn DebugSessionHandle>,
    pump: ChunkPump,
}

impl DebugSession {
    /// Stop the reader first, then drain the batcher, so the last received
    /// bytes still reach the client ahead of the terminal frame. Both steps
    /// join a thread: blocking, and never to be called from the reader thread
    /// itself (that would self-join).
    fn shutdown(self) {
        self.handle.close();
        self.pump.shutdown();
    }
}

/// A connection's single serial monitor slot.
///
/// `Opening` / `Aborting` exist because the backend open is blocking: a close
/// (or a device disconnect) can arrive while it is still in flight, and the
/// session that materializes afterwards must then be torn down instead of
/// published.
enum SessionSlot {
    /// No session, no port claim.
    Idle,
    /// No session either, but the last one was taken over by a device
    /// disconnect that owns its terminal frame, and no close has been answered
    /// for it yet: the next close is that session's close, and gets swallowed.
    /// Left behind by the very step that removes the session, so the swallow
    /// covers the whole teardown — not only the time after the frame went out.
    EndedByDisconnect,
    /// Port claimed, backend open in flight.
    Opening {
        port: String,
    },
    /// As `Opening`, but a close (or a disconnect) already arrived: tear down on
    /// arrival, then send `terminal` — the frame that party is still owed.
    Aborting {
        port: String,
        terminal: PendingTerminal,
    },
    Open(DebugSession),
}

impl SessionSlot {
    /// The state a session leaves behind when `terminal` takes it over. A
    /// disconnect owes the frame for that session, so the close already on its
    /// way must be swallowed; a close is answering for itself and owes nothing.
    ///
    /// Used wherever the session leaves the slot, under the same lock, so there
    /// is no window in which a close sees a plain `Idle` and answers.
    fn vacated_by(terminal: &PendingTerminal) -> SessionSlot {
        match terminal {
            PendingTerminal::Closed => SessionSlot::Idle,
            PendingTerminal::Disconnected { .. } => SessionSlot::EndedByDisconnect,
        }
    }
}

/// The terminal frame a close or a device disconnect still owes the client after
/// it landed on an open that was still in flight. It may only go out once that
/// open has been torn down and its port released, because the frame is the
/// client's cue that the port is reusable.
enum PendingTerminal {
    Closed,
    Disconnected { reason: String },
}

impl PendingTerminal {
    fn into_frame(self) -> ServerFrame {
        match self {
            PendingTerminal::Closed => ServerFrame::SerialDebugClosed,
            PendingTerminal::Disconnected { reason } => {
                ServerFrame::SerialDebugDisconnected { reason }
            }
        }
    }
}

/// Result of pulling the session out of the slot.
enum TakenSession {
    /// The live session, now removed; the caller shuts it down, releases the
    /// port and sends its own terminal frame.
    Open(DebugSession),
    /// The open is still in flight and has been marked for teardown; the open
    /// task does the shutting down, the releasing *and* the answering, so the
    /// caller must stay silent.
    Pending,
    /// Nothing was open because a device disconnect took over the last session
    /// and owns its terminal frame: the caller stays silent. The slot goes back
    /// to `Idle`, so the *next* close is answered again.
    AlreadyReported,
    /// Nothing was open.
    None,
}

/// Result of publishing a freshly opened session.
enum FinishedOpen {
    /// The session went live; announce it.
    Published,
    /// A close or a device disconnect landed while the open was in flight: tear
    /// this session down, release the port, and only then send the frame that
    /// party is owed (`None` in the slot states that cannot be reached).
    Discard {
        session: DebugSession,
        owed: Option<PendingTerminal>,
    },
}

// ── Per-connection handler ───────────────────────────────────────────────────

/// Everything a command task needs: the connection's outbound queue plus the
/// shared arbitration table and flash backend.
struct ConnContext {
    /// This connection's `request_id` namespace in the shared arbiter.
    conn_id: u64,
    /// Feeds the single sink pump task; every frame of this connection (hello,
    /// ports, progress, results) goes through it, so frame order is stable even
    /// though jobs push from their own tasks.
    ///
    /// `None` once the connection is being torn down — dropping the sender is
    /// what stops the pump.
    sink_tx: Mutex<Option<mpsc::Sender<String>>>,
    /// Raised when the queue was abandoned, so the read loop stops too.
    shutdown: tokio::sync::Notify,
    arbiter: Arc<PortArbiter>,
    backend: Arc<dyn FlashBackend>,
    ports: Arc<PortsBroadcaster>,
    /// Jobs started by this connection and not yet finished, so a disconnect
    /// does not leave a serial port held forever.
    inflight: Mutex<HashSet<String>>,
    /// This connection's serial monitor, at most one at a time.
    session: Mutex<SessionSlot>,
}

impl ConnContext {
    fn send(&self, frame: &ServerFrame) {
        match serde_json::to_string(frame) {
            Ok(text) => {
                self.send_text(text);
            }
            Err(e) => log::error!("bridge frame serialize failed: {e}"),
        }
    }

    /// Queue one frame; `false` means the connection is gone or being dropped.
    ///
    /// Never blocks and never awaits: callers include blocking job threads, and
    /// the job lifecycle relies on `send(job_result)` and the release that
    /// follows it happening without an intervening suspension point.
    fn send_text(&self, text: String) -> bool {
        let mut guard = self
            .sink_tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(tx) = guard.as_ref() else {
            return false;
        };
        match tx.try_send(text) {
            Ok(()) => true,
            // The pump stopped, i.e. the socket already failed.
            Err(mpsc::error::TrySendError::Closed(_)) => {
                *guard = None;
                false
            }
            // A full queue is a client that stopped reading: keeping frames for
            // it would grow memory for the rest of the job, so the connection
            // is declared dead instead. Losing a `ports` snapshot alone would
            // be harmless (the next frame carries the full list), but the
            // per-connection backlog is not bounded by anything else.
            Err(mpsc::error::TrySendError::Full(_)) => {
                log::warn!(
                    "bridge client not consuming ({SINK_QUEUE_CAPACITY} frames queued), \
                     closing the connection"
                );
                *guard = None;
                self.shutdown.notify_one();
                false
            }
        }
    }

    fn lock_inflight(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.inflight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_session(&self) -> std::sync::MutexGuard<'_, SessionSlot> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reserve the slot for an open about to start; `false` means this
    /// connection already has a monitor open (or opening). A slot still holding
    /// the unanswered-close marker of a disconnected session counts as free —
    /// reopening right after `serial_debug_disconnected` is the main path.
    ///
    /// Dropping that pending one-shot here is correct, not a leak: a
    /// `serial_debug_close` carries no session identity (see PROTOCOL.md
    /// §serial_debug_close), so it always refers to the connection's *current*
    /// session. Once a new session exists, any close targets that new session
    /// and must be answered for it; the old session's acknowledgement is no
    /// longer owed to anyone.
    fn begin_session(&self, port: &str) -> bool {
        let mut slot = self.lock_session();
        if !matches!(*slot, SessionSlot::Idle | SessionSlot::EndedByDisconnect) {
            return false;
        }
        *slot = SessionSlot::Opening {
            port: port.to_string(),
        };
        true
    }

    /// The open failed: drop the reservation. `Some(..)` back means a close or
    /// a device disconnect is waiting on this open, so the caller still owes it
    /// that frame once the unwinding is done.
    fn abandon_opening(&self) -> Option<PendingTerminal> {
        let mut slot = self.lock_session();
        match std::mem::replace(&mut *slot, SessionSlot::Idle) {
            SessionSlot::Aborting { terminal, .. } => {
                *slot = SessionSlot::vacated_by(&terminal);
                Some(terminal)
            }
            _ => None,
        }
    }

    /// Publish a freshly opened session. `Discard` back means a close or a
    /// device disconnect landed while the open was still running, so the caller
    /// must tear the session down instead of announcing it.
    fn finish_session(&self, session: DebugSession) -> FinishedOpen {
        let mut slot = self.lock_session();
        match std::mem::replace(&mut *slot, SessionSlot::Idle) {
            SessionSlot::Opening { .. } => {
                *slot = SessionSlot::Open(session);
                FinishedOpen::Published
            }
            SessionSlot::Aborting { terminal, .. } => {
                *slot = SessionSlot::vacated_by(&terminal);
                FinishedOpen::Discard {
                    session,
                    owed: Some(terminal),
                }
            }
            SessionSlot::Idle | SessionSlot::EndedByDisconnect => FinishedOpen::Discard {
                session,
                owed: None,
            },
            // Unreachable: `begin_session` admits one open at a time. Put the
            // live session back rather than dropping it on the floor.
            other @ SessionSlot::Open(_) => {
                *slot = other;
                FinishedOpen::Discard {
                    session,
                    owed: None,
                }
            }
        }
    }

    /// Pull the session out of the slot, recording `terminal` as the frame the
    /// caller wants sent when the session is only now materializing. Whoever
    /// gets `TakenSession::Open` owns the shutdown *and* the answer — that is
    /// what makes a client close racing a device disconnect safe: the loser sees
    /// `Pending` / `None` and stays quiet.
    fn take_session(&self, terminal: PendingTerminal) -> TakenSession {
        let mut slot = self.lock_session();
        match std::mem::replace(&mut *slot, SessionSlot::Idle) {
            // The swallow marker is left behind here, in the same locked step
            // that removes the session — the teardown that follows takes two
            // thread joins and a port release, and a close landing in there must
            // not find a bare `Idle` and answer for a session already claimed.
            SessionSlot::Open(session) => {
                *slot = SessionSlot::vacated_by(&terminal);
                TakenSession::Open(session)
            }
            SessionSlot::Opening { port } => {
                *slot = SessionSlot::Aborting { port, terminal };
                TakenSession::Pending
            }
            // First taker decides which single terminal frame the client gets;
            // `terminal` is dropped so a close racing a disconnect cannot turn
            // into two frames.
            SessionSlot::Aborting {
                port,
                terminal: first,
            } => {
                *slot = SessionSlot::Aborting {
                    port,
                    terminal: first,
                };
                TakenSession::Pending
            }
            // The disconnect recorded here owns the terminal frame for the
            // session this caller is tearing down; replacing the state with
            // `Idle` above is what keeps that swallow one-shot — a client
            // sending close twice must keep getting the documented
            // `serial_debug_closed` rather than hang on a frame never sent.
            SessionSlot::EndedByDisconnect => TakenSession::AlreadyReported,
            SessionSlot::Idle => TakenSession::None,
        }
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    ports: Arc<PortsBroadcaster>,
    arbiter: Arc<PortArbiter>,
    backend: Arc<dyn FlashBackend>,
    stats: Arc<StatsPublisher>,
) {
    let ws = match tokio_tungstenite::accept_hdr_async(stream, check_origin).await {
        Ok(ws) => ws,
        Err(e) => {
            // Covers both a rejected Origin and a malformed handshake.
            log::warn!("bridge WS handshake not completed: {e}");
            return;
        }
    };
    // Counted from here on: a refused Origin never becomes a "connection" the
    // user sees in the tray. Released by the guard on every exit below.
    let _counted = stats.connection();
    let (mut sink, mut inbound) = ws.split();

    // Sink pump: one task owns the sink and drains the queue, so job tasks can
    // push frames without holding it.
    let (sink_tx, mut sink_rx) = mpsc::channel::<String>(SINK_QUEUE_CAPACITY);
    tokio::spawn(async move {
        while let Some(text) = sink_rx.recv().await {
            if let Err(e) = sink.send(Message::Text(text)).await {
                log::warn!("bridge WS send failed: {e}");
                break;
            }
        }
    });

    let ctx = Arc::new(ConnContext {
        conn_id: arbiter.next_conn_id(),
        sink_tx: Mutex::new(Some(sink_tx)),
        shutdown: tokio::sync::Notify::new(),
        arbiter,
        backend,
        ports,
        inflight: Mutex::new(HashSet::new()),
        session: Mutex::new(SessionSlot::Idle),
    });

    match serde_json::to_string(&Hello::current()) {
        Ok(text) => {
            ctx.send_text(text);
        }
        Err(e) => {
            log::error!("bridge hello serialize failed: {e}");
            return;
        }
    }

    let (initial_ports, mut ports_rx) = ctx.ports.snapshot_and_subscribe();
    ctx.send_text(initial_ports);

    // Two sources to serve: inbound commands and broadcast ports pushes. Command
    // handling is dispatched to its own task, so a running job never stops this
    // loop from answering `cancel` / `check_port`.
    loop {
        tokio::select! {
            // A frame push gave up on this client (see `send_text`); stop
            // reading so the cleanup below runs.
            () = ctx.shutdown.notified() => break,
            frame = inbound.next() => match frame {
                None | Some(Ok(Message::Close(_))) => break,
                Some(Ok(Message::Text(text))) => dispatch(&ctx, &text),
                Some(Ok(_)) => continue,
                Some(Err(e)) => {
                    log::warn!("bridge WS read error: {e}");
                    break;
                }
            },
            pushed = ports_rx.recv() => match pushed {
                Ok(text) => {
                    if !ctx.send_text(text) {
                        break;
                    }
                }
                // Frames are full snapshots: a lagged client loses intermediate
                // states but the next push carries the complete current list.
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    log::warn!("bridge client lagged, {skipped} ports frames skipped");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }

    // The client is gone: cancel whatever it left running so the held ports are
    // released (the job task itself performs the release once it returns).
    // Only this connection's own jobs: the id namespace makes that automatic.
    let abandoned: Vec<String> = ctx.lock_inflight().iter().cloned().collect();
    for request_id in abandoned {
        log::warn!("bridge connection closed with job {request_id} in flight, cancelling");
        ctx.arbiter.cancel(ctx.conn_id, &request_id);
    }

    // A serial monitor holds its port until told otherwise, so it has to be
    // torn down here rather than merely cancelled. Blocking (two thread joins),
    // hence the blocking pool. An open still in flight is marked as closed
    // instead; its own task performs the teardown (and sends into a sink nobody
    // reads any more, which is a no-op).
    if let TakenSession::Open(session) = ctx.take_session(PendingTerminal::Closed) {
        log::warn!("bridge connection closed with a serial monitor open, closing it");
        let closing = Arc::clone(&ctx);
        let _ = tokio::task::spawn_blocking(move || {
            let port = session.port.clone();
            session.shutdown();
            closing.arbiter.release_session(&port, closing.conn_id);
            closing.ports.publish_ownership();
        })
        .await;
    }
    ctx.arbiter.clear_reservations(ctx.conn_id);

    log::info!("bridge WS connection closed");
}

/// Parse and route one inbound text frame.
///
/// Unparsable frames and unknown `type` values are logged and dropped: B3 has no
/// error frame in the wire contract, so answering is not an option yet.
fn dispatch(ctx: &Arc<ConnContext>, text: &str) {
    let message: ClientMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(e) => {
            log::warn!("bridge ignoring unparsable client frame: {e}");
            return;
        }
    };

    match message {
        ClientMessage::RunJob {
            request_id,
            job,
            file_content,
        } => {
            tokio::spawn(run_job_task(Arc::clone(ctx), request_id, job, file_content));
        }
        ClientMessage::RunAuth { request_id, auth } => {
            tokio::spawn(run_auth_task(Arc::clone(ctx), request_id, auth));
        }
        ClientMessage::Cancel { request_id } => {
            if !ctx.arbiter.cancel(ctx.conn_id, &request_id) {
                log::info!("bridge cancel for unknown or finished job {request_id}, ignored");
            }
        }
        ClientMessage::CheckPort { port } => {
            tokio::spawn(check_port_task(Arc::clone(ctx), port));
        }
        ClientMessage::SerialDebugOpen { cfg } => {
            tokio::spawn(open_debug_session_task(Arc::clone(ctx), cfg));
        }
        ClientMessage::SerialDebugClose => {
            tokio::spawn(close_debug_session_task(Arc::clone(ctx)));
        }
    }
}

/// One flash job end to end: claim the port, then decode and flash off the
/// async worker (base64 decoding a multi-MB image is CPU work that belongs on
/// the blocking pool, like every other synchronous step here).
async fn run_job_task(
    ctx: Arc<ConnContext>,
    request_id: String,
    job: WireJob,
    file_content: String,
) {
    let started = Instant::now();
    let port = job.port.clone();
    let backend = Arc::clone(&ctx.backend);

    drive_port_job(ctx, request_id, port, started, move |cancel, progress| {
        let firmware = match base64::engine::general_purpose::STANDARD.decode(&file_content) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Err(JobError {
                    error_code: "bad_request".to_string(),
                    message: format!("firmware base64 decode failed: {e}"),
                })
            }
        };
        // The base64 text is ~1.33× the image; free it now instead of holding
        // both copies for the whole flash.
        drop(file_content);

        let spec = FlashJobSpec {
            chip_id: job.chip_id,
            port: job.port,
            baud_rate: job.baud_rate,
            mode: job.mode,
            start_addr: job.start_addr,
            firmware,
        };
        backend.run_job(spec, cancel, progress)
    })
    .await;
}

/// One authorization write end to end. Credentials are validated up front so a
/// malformed request neither claims the port nor reaches the device.
async fn run_auth_task(ctx: Arc<ConnContext>, request_id: String, auth: WireAuth) {
    let started = Instant::now();

    if auth.uuid.trim().is_empty() || auth.auth_key.trim().is_empty() {
        ctx.send(&failed(
            &request_id,
            started,
            "bad_request",
            "authorization requires a non-empty uuid and auth_key".to_string(),
        ));
        return;
    }

    let port = auth.port.clone();
    let backend = Arc::clone(&ctx.backend);

    drive_port_job(ctx, request_id, port, started, move |cancel, progress| {
        let spec = AuthJobSpec {
            chip_id: auth.chip_id,
            port: auth.port,
            baud_rate: auth.baud_rate,
            uuid: auth.uuid,
            auth_key: auth.auth_key,
        };
        backend.run_auth(spec, cancel, progress)
    })
    .await;
}

/// Shared lifecycle of every port-holding job (flash and auth alike): claim the
/// port → run `execute` off the async worker while streaming progress → answer
/// → release.
///
/// `execute` receives the job's cooperative cancel flag and the progress
/// forwarder; everything before and after it is identical for both job kinds,
/// including the frame ordering the client depends on.
async fn drive_port_job<F>(
    ctx: Arc<ConnContext>,
    request_id: String,
    port: String,
    started: Instant,
    execute: F,
) where
    F: FnOnce(Arc<AtomicBool>, &(dyn Fn(serde_json::Value) + Send + Sync)) -> Result<(), JobError>
        + Send
        + 'static,
{
    let cancel = match ctx.arbiter.claim(&port, ctx.conn_id, &request_id) {
        Ok(cancel) => cancel,
        Err(ClaimRefused::PortHeld) => {
            ctx.send(&failed(
                &request_id,
                started,
                "port_busy",
                format!("port {port} is already in use by another job"),
            ));
            return;
        }
        Err(ClaimRefused::DuplicateRequest) => {
            ctx.send(&failed(
                &request_id,
                started,
                "bad_request",
                "a job with this request_id is still running".to_string(),
            ));
            return;
        }
    };
    ctx.lock_inflight().insert(request_id.clone());

    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let handle = tokio::task::spawn_blocking(move || {
        let forward = move |payload: serde_json::Value| {
            let _ = progress_tx.send(payload);
        };
        execute(cancel, &forward)
    });

    // The channel closes when the blocking task drops its sender, i.e. when the
    // job returned — so this drains every progress payload before the result.
    let mut announced = false;
    while let Some(payload) = progress_rx.recv().await {
        ctx.send(&ServerFrame::Progress {
            request_id: request_id.clone(),
            payload,
        });
        if !announced {
            // The job is live on the device: announce the busy flip now, so the
            // device list update follows the progress frame that explains it.
            // A job that never reports progress simply skips the intermediate
            // announcement — the release below still reconciles the list.
            announced = true;
            ctx.ports.publish_ownership();
        }
    }

    let outcome = handle.await;

    // Result first, then release, then the device-list push — and all three
    // without an intervening await, so no concurrent publisher can announce
    // `busy=false` before the client has been told why. Releasing earlier would
    // invert that cause-and-effect order.
    let succeeded = matches!(outcome, Ok(Ok(())));
    match outcome {
        Ok(Ok(())) => ctx.send(&ServerFrame::JobResult {
            request_id: request_id.clone(),
            ok: true,
            elapsed_ms: elapsed_ms(started),
            error_code: None,
            message: None,
        }),
        Ok(Err(e)) => ctx.send(&failed(&request_id, started, &e.error_code, e.message)),
        Err(e) => ctx.send(&failed(
            &request_id,
            started,
            "internal",
            format!("job task did not complete: {e}"),
        )),
    }
    // A successful job downgrades its hold to a short handoff reservation
    // instead of releasing outright: "flash, then watch the boot log" is the
    // common path, and the port must still be there when the client asks for
    // it. A failed or cancelled job frees the port immediately — there is
    // nothing to hand over.
    ctx.arbiter.release(ctx.conn_id, &request_id, succeeded);
    ctx.lock_inflight().remove(&request_id);
    ctx.ports.publish_ownership();
}

/// Availability of one port: the bridge's own arbitration table first (only the
/// bridge knows about it), then an OS-level probe through the backend.
async fn check_port_task(ctx: Arc<ConnContext>, port: String) {
    if ctx.arbiter.is_busy(&port) {
        ctx.send(&ServerFrame::CheckPortResult {
            port,
            available: false,
            reason: Some("occupied_by_bridge_job".to_string()),
        });
        return;
    }

    // Off the async worker: a real probe opens the port and can block.
    let backend = Arc::clone(&ctx.backend);
    let probed = port.clone();
    let probe = match tokio::task::spawn_blocking(move || backend.probe_port(&probed)).await {
        Ok(probe) => probe,
        Err(e) => {
            log::warn!("bridge port probe task failed for {port}: {e}");
            PortProbe {
                available: false,
                reason: Some("probe_failed".to_string()),
            }
        }
    };

    ctx.send(&ServerFrame::CheckPortResult {
        port,
        available: probe.available,
        reason: probe.reason,
    });
}

/// Open this connection's serial monitor: validate → claim the port → open the
/// backend session off the async worker (a real open blocks on the OS) →
/// answer.
async fn open_debug_session_task(ctx: Arc<ConnContext>, cfg: WireDebugCfg) {
    let cfg = match cfg.into_core() {
        Ok(cfg) => cfg,
        Err(message) => {
            ctx.send(&open_failed("bad_request", message));
            return;
        }
    };
    let port = cfg.port.clone();

    if !ctx.begin_session(&port) {
        // No test pins this one: the wire allows a single monitor per
        // connection (the tyutool-serve blueprint answers "already open" the
        // same way), so a second open is refused rather than silently
        // replacing the first — replacing would strand the running session's
        // port claim.
        ctx.send(&open_failed(
            "already_open",
            "this connection already has a serial monitor open".to_string(),
        ));
        return;
    }

    if !ctx.arbiter.claim_session(&port, ctx.conn_id) {
        let owed = ctx.abandon_opening();
        ctx.send(&open_failed(
            "port_busy",
            format!("port {port} is already in use"),
        ));
        send_owed_terminal(&ctx, owed);
        return;
    }
    ctx.ports.publish_ownership();

    let pump = ChunkPump::spawn(Arc::downgrade(&ctx));
    let chunk_tx = pump.sender();
    // Weak, not Arc: the backend session owns these callbacks, and the
    // connection owns the session — an Arc here would close that cycle and
    // leak the connection.
    let disconnect_ctx = Arc::downgrade(&ctx);
    // The disconnect callback fires on the backend's reader thread, which is
    // outside the runtime; capture a handle so it can still hand the teardown
    // to the blocking pool.
    let runtime = tokio::runtime::Handle::current();
    let backend = Arc::clone(&ctx.backend);

    let opened = tokio::task::spawn_blocking(move || {
        backend.open_debug_session(
            cfg,
            Box::new(move |chunk| {
                // Blocking send is the backpressure path: the batching thread
                // never blocks on the WS sink (that one is a try_send), so a
                // full queue only ever slows the serial reader down.
                let _ = chunk_tx.send(PumpMessage::Chunk(chunk));
            }),
            Box::new(move |reason| on_session_disconnect(&disconnect_ctx, &runtime, reason)),
        )
    })
    .await;

    let handle = match opened {
        Ok(Ok(handle)) => handle,
        Ok(Err(e)) => {
            let owed = finish_failed_open(&ctx, &port, pump);
            ctx.send(&open_failed(&e.error_code, e.message));
            send_owed_terminal(&ctx, owed);
            return;
        }
        Err(e) => {
            let owed = finish_failed_open(&ctx, &port, pump);
            ctx.send(&open_failed(
                "internal",
                format!("serial monitor open task did not complete: {e}"),
            ));
            send_owed_terminal(&ctx, owed);
            return;
        }
    };

    let session = DebugSession {
        port: port.clone(),
        handle,
        pump,
    };
    match ctx.finish_session(session) {
        FinishedOpen::Published => ctx.send(&ServerFrame::SerialDebugOpened),
        // Closed (or disconnected) while the open was still running: whoever
        // did that stayed silent because only this task can free the port, so
        // tear the session down and answer for them — after the release, never
        // before, since the frame invites an immediate reopen.
        FinishedOpen::Discard { session, owed } => {
            let closing = Arc::clone(&ctx);
            let _ = tokio::task::spawn_blocking(move || {
                session.shutdown();
                closing.arbiter.release_session(&port, closing.conn_id);
                closing.ports.publish_ownership();
                send_owed_terminal(&closing, owed);
            })
            .await;
        }
    }
}

/// Unwind a failed open: no session was created, so only the claim and the
/// (still empty) batching thread need cleaning up. `Some(..)` back means a close
/// or a device disconnect raced this open and is still waiting for its frame.
fn finish_failed_open(
    ctx: &Arc<ConnContext>,
    port: &str,
    pump: ChunkPump,
) -> Option<PendingTerminal> {
    let owed = ctx.abandon_opening();
    ctx.arbiter.release_session(port, ctx.conn_id);
    ctx.ports.publish_ownership();
    // Returns as soon as the thread sees the message; it has nothing buffered.
    pump.shutdown();
    owed
}

/// Answer a close / disconnect that landed on an in-flight open, once that open
/// has been unwound and the port is free again. Skipping it would leave a web
/// client waiting for its `serial_debug_closed` forever.
///
/// Sending only: the one-shot swallow an owed `Disconnected` needs was already
/// left in the slot by the step that took the session over.
fn send_owed_terminal(ctx: &ConnContext, owed: Option<PendingTerminal>) {
    if let Some(terminal) = owed {
        ctx.send(&terminal.into_frame());
    }
}

/// Close this connection's serial monitor. Idempotent: `serial_debug_closed`
/// is answered even with nothing open (the tyutool-serve blueprint does the
/// same, and it keeps the web client's teardown path branch-free).
async fn close_debug_session_task(ctx: Arc<ConnContext>) {
    match ctx.take_session(PendingTerminal::Closed) {
        TakenSession::Open(session) => {
            let closing = Arc::clone(&ctx);
            // Shut down and release *before* answering, so the client can reopen
            // the port the moment it sees `serial_debug_closed` — and so the last
            // buffered bytes are flushed ahead of it.
            let _ = tokio::task::spawn_blocking(move || {
                let port = session.port.clone();
                session.shutdown();
                closing.arbiter.release_session(&port, closing.conn_id);
                closing.ports.publish_ownership();
            })
            .await;
        }
        // The open is still in flight and now carries this close: only its task
        // can release the port, so it answers once it has — answering here would
        // promise a port that is still held.
        TakenSession::Pending => return,
        // A device disconnect took this very session over and owns its terminal
        // frame (already sent, or still on its way out of the teardown);
        // answering here would be the second terminal frame for one session,
        // which the wire contract rules out.
        TakenSession::AlreadyReported => return,
        // Nothing open: still answer, that is the idempotence above.
        TakenSession::None => {}
    }
    ctx.send(&ServerFrame::SerialDebugClosed);
}

/// Device-side end of a session, invoked from the backend's reader thread.
fn on_session_disconnect(
    ctx: &Weak<ConnContext>,
    runtime: &tokio::runtime::Handle,
    reason: String,
) {
    let Some(ctx) = ctx.upgrade() else {
        return;
    };
    // The clone only feeds the `Pending` branch below, where this reason becomes
    // the frame the open task owes; a disconnect is rare enough to not care.
    match ctx.take_session(PendingTerminal::Disconnected {
        reason: reason.clone(),
    }) {
        TakenSession::Open(session) => {
            // Off this thread: `session.shutdown()` joins the very reader
            // thread running this callback, which would self-deadlock. The
            // frame is emitted after the release for the same reason the close
            // path does it — a client reopening on `serial_debug_disconnected`
            // must not race the teardown.
            runtime.spawn_blocking(move || {
                let port = session.port.clone();
                session.shutdown();
                ctx.arbiter.release_session(&port, ctx.conn_id);
                ctx.ports.publish_ownership();
                ctx.send(&ServerFrame::SerialDebugDisconnected { reason });
            });
        }
        // The open is still in flight: its task owns the teardown, and with it
        // the reporting — reporting from here would announce a free port while
        // the session about to materialize still holds it.
        TakenSession::Pending => {}
        // An earlier disconnect already took a session over; nothing left to
        // report here either.
        TakenSession::AlreadyReported => {}
        // The client closed first; that close already answered.
        TakenSession::None => {}
    }
}

fn open_failed(error_code: &str, message: String) -> ServerFrame {
    ServerFrame::SerialDebugOpenFailed {
        error_code: error_code.to_string(),
        message,
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

fn failed(request_id: &str, started: Instant, error_code: &str, message: String) -> ServerFrame {
    ServerFrame::JobResult {
        request_id: request_id.to_string(),
        ok: false,
        elapsed_ms: elapsed_ms(started),
        error_code: Some(error_code.to_string()),
        message: Some(message),
    }
}

/// Handshake gate: the request `Origin` must byte-for-byte match one of
/// [`ORIGIN_ALLOWLIST`].
///
/// A non-allowlisted Origin is refused with 403 and the socket is dropped; a
/// missing Origin is treated as a non-browser caller and refused the same way
/// (非白名单直接断开，缺失 Origin 视为非浏览器来源同样拒绝).
// The Result signature is fixed by tungstenite's handshake callback contract,
// so the large `ErrorResponse` variant cannot be boxed away here.
#[allow(clippy::result_large_err)]
fn check_origin(req: &Request, response: Response) -> Result<Response, ErrorResponse> {
    let origin = req.headers().get("Origin");
    let allowed = origin.is_some_and(|value| {
        ORIGIN_ALLOWLIST
            .iter()
            .any(|allowed| value.as_bytes() == allowed.as_bytes())
    });
    if allowed {
        return Ok(response);
    }

    log::warn!(
        "bridge WS handshake refused, origin not allowlisted: {:?}",
        origin.map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
    );
    let mut refusal = ErrorResponse::new(Some("origin not allowed".to_string()));
    *refusal.status_mut() = StatusCode::FORBIDDEN;
    Err(refusal)
}

// ── OS version probe ─────────────────────────────────────────────────────────

/// Real OS version string for the hello frame, `"unknown"` when undetectable.
///
/// The workspace carries no OS-info dependency, and the hello frame is the only
/// consumer, so shelling out via `std::process::Command` is the lightest option
/// (no new crate, no platform FFI).
fn os_version() -> String {
    detect_os_version().unwrap_or_else(|| "unknown".to_string())
}

#[cfg(target_os = "macos")]
fn detect_os_version() -> Option<String> {
    // e.g. "15.1.1"
    command_first_line("sw_vers", &["-productVersion"])
}

#[cfg(target_os = "linux")]
fn detect_os_version() -> Option<String> {
    // Distro version first (e.g. VERSION_ID="22.04"), kernel release as fallback.
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("VERSION_ID=") {
                let version = rest.trim().trim_matches('"').trim();
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }
    }
    command_first_line("uname", &["-r"])
}

#[cfg(target_os = "windows")]
fn detect_os_version() -> Option<String> {
    // `cmd /c ver` prints e.g. "Microsoft Windows [Version 10.0.22631.4317]".
    let line = command_first_line("cmd", &["/c", "ver"])?;
    line.split(|c: char| c.is_whitespace() || c == '[' || c == ']')
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()) && token.contains('.'))
        .map(|token| token.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn detect_os_version() -> Option<String> {
    None
}

/// Run `program args...` and return its first non-empty stdout line.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn command_first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_stagings_of_identical_firmware_get_separate_files() {
        // Same bytes, same process, back to back: on a coarse system clock both
        // stampings can read the same nanosecond value, and two live jobs must
        // still own distinct files.
        let first = TempFirmware::stage(b"same firmware").expect("stage first");
        let second = TempFirmware::stage(b"same firmware").expect("stage second");

        assert_ne!(
            first.path(),
            second.path(),
            "two stagings must not share a path"
        );
        assert!(first.path().exists(), "{}", first.path().display());
        assert!(second.path().exists(), "{}", second.path().display());
    }

    #[test]
    fn device_count_ignores_ports_without_an_allowlisted_vid() {
        let port = |vid: Option<u16>| EnumeratedPort {
            path: format!("/dev/tty.{vid:?}"),
            vid,
            pid: None,
            vendor: None,
            busy: false,
        };
        // 0x1A86 = WCH CH34x (allowlisted), 0x0403 = FTDI (allowlisted).
        let ports = vec![
            port(Some(0x1A86)),
            port(Some(0x1234)), // unknown vendor
            port(None),         // e.g. a built-in Bluetooth serial port
            port(Some(0x0403)),
        ];

        assert_eq!(allowlisted_device_count(&ports), 2);
    }
}
